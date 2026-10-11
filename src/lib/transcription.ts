/** Editable chunk text with original-file timestamp boundaries. */
export interface SubtitleSegment {
  startSec: number;
  endSec: number;
  text: string;
}
/** Native ASR backend names after runtime classification. */
export type AsrBackend = "Metal" | "CUDA" | "Vulkan" | "CPU";
/** Session-only user preference; device IDs belong to the current native process. */
export type AsrSelection =
  { mode: "auto" } | { mode: "cpu" } | { mode: "device"; id: string };
/** Hardware-only diagnostics without native handles or file paths. */
export interface AsrDevice {
  id: string;
  backend: AsrBackend;
  name: string;
  description: string;
}
/** Public model identity; provisioning details and native paths remain in Rust. */
export interface ModelDescriptor {
  id: string;
  name: string;
  version: string;
  purpose: "ASR" | "VAD";
  format: "GGUF" | "ONNX";
  quantization: string | null;
}
/** Formats model identity and the CPU-only VAD role for the Models panel. */
export function modelLabel(model: ModelDescriptor): string {
  const details = [model.purpose, model.format];
  return `${model.name} ${model.version} — ${details.join(" · ")}${model.quantization ? ` · ${model.quantization}` : ""}${model.purpose === "VAD" ? " · CPU" : ""}`;
}
/** Ordered, path-free model residency, device selection and download status. */
export interface ModelSnapshot {
  models: ModelDescriptor[];
  revision: number;
  selection: AsrSelection;
  devices: AsrDevice[];
  device: AsrDevice | null;
  phase: string;
  provider: AsrBackend | null;
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
  models: [],
  revision: -1,
  selection: { mode: "auto" },
  devices: [],
  device: null,
  phase: "loading",
  provider: null,
  error: null,
  missing: [],
  downloadFile: null,
  percentage: null,
};
/** Rejects stale command reads and duplicate model events using the same native revision. */
export function isNewModelSnapshot(
  current: ModelSnapshot,
  incoming: ModelSnapshot,
): boolean {
  return incoming.revision > current.revision;
}
/** Encodes the preference for a native HTML select without using hardware names as IDs. */
export function selectionValue(selection: AsrSelection): string {
  return selection.mode === "device" ? selection.id : selection.mode;
}
/** Decodes a UI selection; the backend remains responsible for validating the device ID. */
export function selectionFromValue(value: string): AsrSelection {
  if (value === "auto" || value === "cpu") return { mode: value };
  return { mode: "device", id: value };
}
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
