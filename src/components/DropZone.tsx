import { ArrowUpRight, FolderOpen, LoaderCircle, Upload } from "lucide-react";
import { SUPPORTED_FORMATS } from "../lib/audioImport";
interface DropZoneProps {
  dragging: boolean;
  busy: boolean;
  available: boolean;
  validating: boolean;
  onChoose: () => Promise<void>;
}
/** Presents file selection and feedback from native drag-and-drop events. */
export function DropZone({
  dragging,
  busy,
  available,
  validating,
  onChoose,
}: DropZoneProps) {
  return (
    <section
      className={`drop-zone ${dragging ? "is-dragging" : ""}`}
      aria-label="Audio file import"
      aria-busy={busy}
    >
      <div className="upload-icon" aria-hidden="true">
        {busy ? (
          <LoaderCircle className="spinner" size={28} />
        ) : (
          <Upload size={28} strokeWidth={1.6} />
        )}
      </div>
      <h2>
        {validating
          ? "Checking your audio…"
          : busy
            ? "Choose your audio"
            : dragging
              ? "Drop your audio here"
              : "A little audio. A lot of possibility."}
      </h2>
      <p>
        {validating
          ? "Verifying the file locally. This may take a moment."
          : "Drag an audio file into this window, or choose one below."}
      </p>
      <button
        className="button button-primary"
        disabled={busy || !available}
        onClick={onChoose}
      >
        <FolderOpen size={17} aria-hidden="true" />
        Choose audio file
        <ArrowUpRight size={16} aria-hidden="true" />
      </button>
      <span className="format-hint">
        {SUPPORTED_FORMATS} <span aria-hidden="true">·</span> One file at a time
      </span>
    </section>
  );
}
