import { formatDuration } from "../lib/audioImport";
import type { useTranscriptPlayback } from "../hooks/useTranscriptPlayback";
import { TranscriptBlockPlayer } from "./TranscriptBlockPlayer";
import type { SubtitleSegment } from "../lib/transcription";
interface Props {
  player: ReturnType<typeof useTranscriptPlayback>;
  name: string;
  segments: SubtitleSegment[];
  disabled: boolean;
  onEdit: (index: number, text: string) => void;
  audioId: string | null;
  audioAvailable: boolean;
  legacy: boolean;
  associating: boolean;
  onAssociate: () => void;
}
/** Edits review text while preserving the original immutable audio timestamps. */
export function TranscriptEditor({
  player,
  name,
  segments,
  disabled,
  onEdit,
  audioId,
  audioAvailable,
  legacy,
  associating,
  onAssociate,
}: Props) {
  return (
    <section className="transcription-panel" aria-label="Transcript editor">
      <h2>{name || "Transcript"}</h2>
      <p className="muted">
        {legacy
          ? "Original speech segments. Subtitle timing follows chunk boundaries."
          : "Review blocks of about 10 or 20 minutes. Subtitle timing follows block boundaries."}
      </p>
      {!!segments.length && audioId && !audioAvailable && (
        <div>
          <button
            type="button"
            onClick={onAssociate}
            disabled={disabled || associating}
          >
            {associating ? "Associating…" : "Associate original audio"}
          </button>
          {legacy && (
            <p className="muted">
              This older transcript has no audio fingerprint. Only timestamp
              coverage can be verified.
            </p>
          )}
        </div>
      )}
      {segments.length === 0 && <p>No speech detected.</p>}
      {segments.map((segment, index) => (
        <div className="segment-editor" key={`${index}-${segment.startSec}`}>
          <label htmlFor={`segment-${index}`}>
            Segment {index + 1} · {formatDuration(segment.startSec)} –{" "}
            {formatDuration(segment.endSec)}
          </label>
          <TranscriptBlockPlayer
            index={index}
            duration={segment.endSec - segment.startSec}
            snapshot={player.snapshot}
            disabled={disabled || !audioAvailable || associating}
            onToggle={() =>
              player.toggle(index, segment.endSec - segment.startSec)
            }
            onSeek={player.seek}
            onRate={player.setRate}
          />
          <textarea
            id={`segment-${index}`}
            value={segment.text}
            disabled={disabled}
            onChange={(event) => onEdit(index, event.target.value)}
            rows={legacy ? 3 : 12}
          />
        </div>
      ))}
    </section>
  );
}
