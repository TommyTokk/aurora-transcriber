/** Display metadata without filesystem paths or decoded samples. */
export interface AudioMetadata {
  name: string;
  format: string;
  sizeBytes: number;
  durationSeconds: number;
}
/** Path-free snapshot shared by native IPC commands and events. */
export interface ImportUpdate {
  phase: "idle" | "picking" | "validating" | "ready" | "error";
  metadata: AudioMetadata | null;
  error: string | null;
}
export const IMPORT_EVENT = "audio-import";
export const DRAG_EVENT = "audio-drag";
export const SUPPORTED_FORMATS = "WAV, MP3, FLAC, M4A, OGG";
export const EMPTY_IMPORT: ImportUpdate = {
  phase: "idle",
  metadata: null,
  error: null,
};
/** Formats byte counts using compact binary units. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
/** Formats decoded duration with hours only when needed. */
export function formatDuration(seconds: number): string {
  if (seconds < 1) return `${seconds.toFixed(2)} s`;
  const total = Math.round(seconds);
  const minutes = Math.floor(total / 60);
  const remainder = String(total % 60).padStart(2, "0");
  if (minutes < 60) return `${minutes}:${remainder}`;
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}:${remainder}`;
}
