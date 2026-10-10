import { formatDuration } from "../lib/audioImport";
import type { SubtitleSegment } from "../lib/transcription";
interface Props {
  name: string;
  segments: SubtitleSegment[];
  disabled: boolean;
  onEdit: (index: number, text: string) => void;
}
/** Edits chunk text while preserving the original immutable audio timestamps. */
export function TranscriptEditor({ name, segments, disabled, onEdit }: Props) {
  return (
    <section className="transcription-panel" aria-label="Transcript editor">
      <h2>{name || "Transcript"}</h2>
      <p className="muted">
        One segment per detected speech chunk. Subtitle timing follows chunk
        boundaries.
      </p>
      {segments.length === 0 && <p>No speech detected.</p>}
      {segments.map((segment, index) => (
        <div className="segment-editor" key={`${index}-${segment.startSec}`}>
          <label htmlFor={`segment-${index}`}>
            Segment {index + 1} · {formatDuration(segment.startSec)} –{" "}
            {formatDuration(segment.endSec)}
          </label>
          <textarea
            id={`segment-${index}`}
            value={segment.text}
            disabled={disabled}
            onChange={(event) => onEdit(index, event.target.value)}
            rows={3}
          />
        </div>
      ))}
    </section>
  );
}
