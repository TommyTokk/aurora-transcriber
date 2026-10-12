/** One media element and one transient clip for the entire transcript editor. */
export interface PlaybackSnapshot {
  index: number | null;
  loading: boolean;
  playing: boolean;
  position: number;
  duration: number;
  rate: number;
  error: string | null;
}
export const EMPTY_PLAYBACK: PlaybackSnapshot = {
  index: null,
  loading: false,
  playing: false,
  position: 0,
  duration: 0,
  rate: 1,
  error: null,
};
export const PLAYBACK_RATES = [0.5, 0.75, 1, 1.25, 1.5, 2];
type Media = Pick<
  HTMLAudioElement,
  | "src"
  | "currentTime"
  | "duration"
  | "playbackRate"
  | "play"
  | "pause"
  | "load"
  | "removeAttribute"
  | "ontimeupdate"
  | "onended"
  | "onerror"
  | "onpause"
  | "onloadedmetadata"
>;
interface Dependencies {
  media: Media;
  load: (index: number) => Promise<ArrayBuffer>;
  release: () => Promise<unknown>;
  createUrl: (bytes: ArrayBuffer) => string;
  revokeUrl: (url: string) => void;
  changed: (snapshot: PlaybackSnapshot) => void;
}
export class ReviewPlayback {
  private state = { ...EMPTY_PLAYBACK };
  private revision = 0;
  private url: string | null = null;
  private disposed = false;
  private deps: Dependencies;
  constructor(deps: Dependencies) {
    this.deps = deps;
    const media = deps.media;
    media.ontimeupdate = () => {
      this.update({
        position: Math.min(media.currentTime, this.state.duration),
      });
      if (media.currentTime >= this.state.duration && this.state.duration > 0)
        this.finish();
    };
    media.onended = () => this.finish();
    media.onpause = () => this.update({ playing: false });
    media.onloadedmetadata = () => {
      if (Number.isFinite(media.duration))
        this.update({ duration: media.duration });
    };
    media.onerror = () => {
      if (!this.url) return;
      ++this.revision;
      this.clear();
      this.update({
        loading: false,
        playing: false,
        error: "Unable to play this audio. Try associating the original again.",
      });
    };
  }
  private update(patch: Partial<PlaybackSnapshot>) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.deps.changed(this.state);
  }
  private finish() {
    this.deps.media.pause();
    this.update({ playing: false, position: this.state.duration });
  }
  private clear() {
    const url = this.url;
    this.url = null;
    this.deps.media.pause();
    this.deps.media.removeAttribute("src");
    this.deps.media.load();
    if (url) this.deps.revokeUrl(url);
  }
  async toggle(index: number, duration: number) {
    if (this.disposed) return;
    if (this.state.index === index && this.url) {
      if (this.state.playing) {
        this.deps.media.pause();
        this.update({ playing: false });
        return;
      }
      await this.play();
      return;
    }
    const revision = ++this.revision;
    this.clear();
    this.update({
      index,
      loading: true,
      playing: false,
      position: 0,
      duration,
      error: null,
    });
    try {
      await this.deps.release();
      if (revision !== this.revision || this.disposed) return;
      const bytes = await this.deps.load(index);
      if (revision !== this.revision || this.disposed) return;
      this.url = this.deps.createUrl(bytes);
      this.deps.media.src = this.url;
      this.deps.media.playbackRate = this.state.rate;
      this.deps.media.load();
      this.update({ loading: false });
      await this.play();
    } catch (failure) {
      if (revision !== this.revision || this.disposed) return;
      this.clear();
      this.update({
        loading: false,
        playing: false,
        error:
          typeof failure === "string"
            ? failure
            : "Unable to load this audio block.",
      });
    }
  }
  private async play() {
    const revision = this.revision;
    if (this.state.position >= this.state.duration) {
      this.deps.media.currentTime = 0;
      this.update({ position: 0 });
    }
    try {
      await this.deps.media.play();
      if (revision === this.revision && !this.disposed)
        this.update({ playing: true, error: null });
    } catch {
      if (revision === this.revision && !this.disposed)
        this.update({
          playing: false,
          error: "Playback could not start. Press Play to retry.",
        });
    }
  }
  seek(position: number) {
    if (!this.url || !Number.isFinite(position)) return;
    const bounded = Math.max(0, Math.min(position, this.state.duration));
    this.deps.media.currentTime = bounded;
    this.update({ position: bounded });
    if (bounded >= this.state.duration) this.finish();
  }
  setRate(rate: number) {
    if (!PLAYBACK_RATES.includes(rate)) return;
    this.deps.media.playbackRate = rate;
    this.update({ rate });
  }
  dispose() {
    this.disposed = true;
    ++this.revision;
    this.deps.media.ontimeupdate = null;
    this.deps.media.onended = null;
    this.deps.media.onerror = null;
    this.deps.media.onpause = null;
    this.deps.media.onloadedmetadata = null;
    this.clear();
    void this.deps.release().catch(() => {});
  }
}
