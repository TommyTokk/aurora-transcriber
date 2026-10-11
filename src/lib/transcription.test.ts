import assert from "node:assert/strict";
import test from "node:test";
import {
  EMPTY_JOB,
  EMPTY_MODEL,
  isJobActive,
  isNewSnapshot,
  isNewModelSnapshot,
  selectionFromValue,
  selectionValue,
} from "./transcription.ts";

/** Ensures late events from an old job cannot replace a newer job or edited terminal result. */
test("snapshots reject duplicates and stale job events", () => {
  const current = { ...EMPTY_JOB, jobId: "new", revision: 12 };
  assert.equal(
    isNewSnapshot(current, { ...EMPTY_JOB, jobId: "old", revision: 11 }),
    false,
  );
  assert.equal(isNewSnapshot(current, current), false);
  assert.equal(isNewSnapshot(current, { ...current, revision: 13 }), true);
});
/** Verifies only processing phases keep controls locked. */
test("only active phases retain the background job", () => {
  for (const phase of ["loading", "decoding", "vad", "transcribing"])
    assert.equal(isJobActive(phase), true);
  for (const phase of ["idle", "completed", "cancelled", "error"])
    assert.equal(isJobActive(phase), false);
});

/** Checks ordered device snapshots so late reads cannot reinstate old GPU diagnostics. */
test("model snapshots reject stale device reads and duplicate events", () => {
  const current = {
    ...EMPTY_MODEL,
    revision: 5,
    selection: { mode: "cpu" as const },
  };
  assert.equal(
    isNewModelSnapshot(current, { ...EMPTY_MODEL, revision: 4 }),
    false,
  );
  assert.equal(isNewModelSnapshot(current, current), false);
  assert.equal(
    isNewModelSnapshot(current, {
      ...current,
      revision: 6,
      phase: "unloaded",
      device: null,
    }),
    true,
  );
  assert.equal(
    isNewModelSnapshot(EMPTY_MODEL, { ...EMPTY_MODEL, revision: 0 }),
    true,
  );
});

/** Ensures Auto, CPU and opaque GPU selections round-trip without choosing CPU for unknown IDs. */
test("ASR select values preserve explicit intent", () => {
  for (const selection of [
    { mode: "auto" as const },
    { mode: "cpu" as const },
    { mode: "device" as const, id: "opaque-gpu-id" },
  ]) {
    assert.deepEqual(selectionFromValue(selectionValue(selection)), selection);
  }
  assert.deepEqual(selectionFromValue("unknown"), {
    mode: "device",
    id: "unknown",
  });
});
