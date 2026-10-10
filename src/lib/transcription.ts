/** Editable chunk text with original-file timestamp boundaries. */
export interface SubtitleSegment {
  startSec: number;
  endSec: number;
  text: string;
}
/** Path-free model residency and explicit download status. */
export interface ModelSnapshot {
  phase: string;
  provider: string | null;
  error: string | null;
  missing: string[];
  downloadFile: string | null;
  percentage: number | null;
}
/** Ordered snapshot for one background transcription job. */
export interface TranscriptionSnapshot {
  jobId: string | null;
  revision: number;
  phase:
    | "idle"
    | "loading"
    | "decoding"
    | "vad"
    | "transcribing"
    | "completed"
    | "cancelled"
    | "error";
  name: string;
  currentChunk: number;
  totalChunks: number;
  percentage: number | null;
  segments: SubtitleSegment[];
  error: string | null;
}
/** Versioned document persisted only through the Save action. */
export interface TranscriptDocument {
  version: number;
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  segments: SubtitleSegment[];
}
/** Lightweight entry in the user-owned local archive. */
export interface TranscriptSummary {
  id: string;
  name: string;
  updatedAt: number;
}
export const EMPTY_JOB: TranscriptionSnapshot = {
  jobId: null,
  revision: 0,
  phase: "idle",
  name: "",
  currentChunk: 0,
  totalChunks: 0,
  percentage: null,
  segments: [],
  error: null,
};
export const EMPTY_MODEL: ModelSnapshot = {
  phase: "loading",
  provider: null,
  error: null,
  missing: [],
  downloadFile: null,
  percentage: null,
};
/** Rejects duplicate or out-of-order events even when they belong to a previous job. */
export function isNewSnapshot(
  current: TranscriptionSnapshot,
  incoming: TranscriptionSnapshot,
): boolean {
  return incoming.revision > current.revision;
}
/** Reports whether a snapshot represents a background job that still owns resources. */
export function isJobActive(phase: string): boolean {
  return ["loading", "decoding", "vad", "transcribing"].includes(phase);
}
