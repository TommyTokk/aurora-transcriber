import assert from "node:assert/strict";
import test from "node:test";
import { ReviewPlayback, type PlaybackSnapshot } from "./playback.ts";

function harness(
  load: (index: number) => Promise<ArrayBuffer> = async () =>
    new ArrayBuffer(4),
) {
  let snapshot: PlaybackSnapshot | undefined;
  let loads = 0;
  let releases = 0;
  const revoked: string[] = [];
  const media = {
    src: "",
    currentTime: 0,
    duration: 10,
    playbackRate: 1,
    play: async () => {},
    pause: () => {},
    load: () => {},
    removeAttribute: (_name: string) => {
      media.src = "";
    },
    ontimeupdate: null as (() => void) | null,
    onended: null as (() => void) | null,
    onerror: null as (() => void) | null,
    onpause: null as (() => void) | null,
    onloadedmetadata: null as (() => void) | null,
  };
  const player = new ReviewPlayback({
    media,
    load: async (index) => {
      loads++;
      return load(index);
    },
    release: async () => {
      releases++;
    },
    createUrl: () => `blob:clip-${loads}`,
    revokeUrl: (url) => revoked.push(url),
    changed: (value) => {
      snapshot = value;
    },
  });
  return {
    player,
    media,
    revoked,
    state: () => snapshot!,
    loads: () => loads,
    releases: () => releases,
  };
}

test("pause, seek, rate and replay reuse the loaded clip", async () => {
  const h = harness();
  await h.player.toggle(0, 10);
  assert.equal(h.state().playing, true);
  h.player.seek(4);
  h.player.setRate(1.5);
  assert.equal(h.media.currentTime, 4);
  assert.equal(h.media.playbackRate, 1.5);
  await h.player.toggle(0, 10);
  assert.equal(h.state().playing, false);
  await h.player.toggle(0, 10);
  h.player.seek(999);
  assert.equal(h.state().position, 10);
  assert.equal(h.state().playing, false);
  await h.player.toggle(0, 10);
  assert.equal(h.media.currentTime, 0);
  assert.equal(h.loads(), 1);
  h.media.onended?.();
  assert.equal(h.state().playing, false);
  h.player.dispose();
  assert.deepEqual(h.revoked, ["blob:clip-1"]);
});

test("rapid changes ignore stale clip responses and release only the current clip", async () => {
  let finishFirst: ((bytes: ArrayBuffer) => void) | undefined;
  const h = harness((index) =>
    index === 0
      ? new Promise((resolve) => {
          finishFirst = resolve;
        })
      : Promise.resolve(new ArrayBuffer(8)),
  );
  const first = h.player.toggle(0, 10);
  await Promise.resolve();
  await h.player.toggle(1, 20);
  finishFirst?.(new ArrayBuffer(4));
  await first;
  assert.equal(h.state().index, 1);
  assert.equal(h.state().playing, true);
  assert.equal(h.media.src, "blob:clip-2");
  await h.player.toggle(2, 30);
  assert.deepEqual(h.revoked, ["blob:clip-2"]);
  h.player.dispose();
  assert.equal(h.releases(), 4);
});

test("disposal cancels pending results and failures allow retry", async () => {
  let finish: ((bytes: ArrayBuffer) => void) | undefined;
  const h = harness(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const loading = h.player.toggle(0, 10);
  await Promise.resolve();
  h.player.dispose();
  finish?.(new ArrayBuffer(4));
  await loading;
  assert.equal(h.media.src, "");
  assert.deepEqual(h.revoked, []);
  let fails = true;
  const retry = harness(async () => {
    if (fails) throw "Original missing.";
    return new ArrayBuffer(4);
  });
  await retry.player.toggle(0, 10);
  assert.equal(retry.state().error, "Original missing.");
  fails = false;
  await retry.player.toggle(0, 10);
  assert.equal(retry.state().playing, true);
  retry.player.dispose();
});

test("empty-source events while loading cannot cancel a valid clip", async () => {
  let complete: ((bytes: ArrayBuffer) => void) | undefined;
  const h = harness(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  );
  const loading = h.player.toggle(0, 10);
  await Promise.resolve();
  h.media.onerror?.();
  assert.equal(h.state().loading, true);
  complete?.(new ArrayBuffer(4));
  await loading;
  assert.equal(h.state().playing, true);
  h.player.dispose();
});

test("navigation pause preserves the loaded block, position and speed", async () => {
  const h = harness();
  await h.player.toggle(2, 20);
  h.player.seek(7);
  h.player.setRate(1.5);
  h.player.pause();
  assert.equal(h.state().playing, false);
  assert.equal(h.state().index, 2);
  assert.equal(h.state().position, 7);
  assert.equal(h.state().rate, 1.5);
  await h.player.toggle(2, 20);
  assert.equal(h.state().playing, true);
  assert.equal(h.state().position, 7);
  assert.equal(h.loads(), 1);
  h.player.dispose();
});

test("navigation during a clip load suppresses late autoplay but allows manual resume", async () => {
  let finish: ((bytes: ArrayBuffer) => void) | undefined;
  const h = harness(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let plays = 0;
  h.media.play = async () => {
    plays++;
  };
  const loading = h.player.toggle(0, 10);
  await Promise.resolve();
  h.player.pause();
  finish?.(new ArrayBuffer(4));
  await loading;
  assert.equal(plays, 0);
  assert.equal(h.state().loading, false);
  assert.equal(h.state().playing, false);
  await h.player.toggle(0, 10);
  assert.equal(plays, 1);
  assert.equal(h.loads(), 1);
  h.player.dispose();
});

test("navigation ignores a play promise that resolves after pause", async () => {
  const h = harness();
  let complete: (() => void) | undefined;
  h.media.play = () =>
    new Promise((resolve) => {
      complete = resolve;
    });
  const playing = h.player.toggle(0, 10);
  await new Promise((resolve) => setTimeout(resolve, 0));
  h.player.pause();
  complete?.();
  await playing;
  assert.equal(h.state().playing, false);
  h.player.dispose();
});
