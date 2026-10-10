import { Check, FileAudio, RefreshCw, X } from "lucide-react";
import {
  formatDuration,
  formatSize,
  type AudioMetadata,
} from "../lib/audioImport";
interface AudioFileCardProps {
  file: AudioMetadata;
  busy: boolean;
  onReplace: () => Promise<void>;
  onRemove: () => Promise<void>;
}
/** Displays validated metadata and accessible replace and remove controls. */
export function AudioFileCard({
  file,
  busy,
  onReplace,
  onRemove,
}: AudioFileCardProps) {
  return (
    <section className="file-card" aria-label="Selected audio file">
      <div className="file-icon">
        <FileAudio size={25} aria-hidden="true" />
      </div>
      <div className="file-details">
        <h3 title={file.name}>{file.name}</h3>
        <p>
          {file.format} <span aria-hidden="true">·</span>{" "}
          {formatSize(file.sizeBytes)} <span aria-hidden="true">·</span>{" "}
          <span aria-label="Duration">
            {formatDuration(file.durationSeconds)}
          </span>
        </p>
        <span className="ready-label">
          <Check size={13} aria-hidden="true" /> Audio verified
        </span>
      </div>
      <div className="file-actions">
        <button
          className="icon-button"
          aria-label="Replace audio file"
          title="Replace audio file"
          disabled={busy}
          onClick={onReplace}
        >
          <RefreshCw size={17} />
        </button>
        <button
          className="icon-button"
          aria-label="Remove audio file"
          title="Remove audio file"
          disabled={busy}
          onClick={onRemove}
        >
          <X size={19} />
        </button>
      </div>
    </section>
  );
}
