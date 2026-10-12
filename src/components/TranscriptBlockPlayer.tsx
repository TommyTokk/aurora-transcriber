import { formatDuration } from "../lib/audioImport";
import { PLAYBACK_RATES, type PlaybackSnapshot } from "../lib/playback";
interface Props {
  index: number;
  duration: number;
  snapshot: PlaybackSnapshot;
  disabled: boolean;
  onToggle: () => void;
  onSeek: (position: number) => void;
  onRate: (rate: number) => void;
}
/** Accessible review controls for one immutable audio interval. */
export function TranscriptBlockPlayer({
  index,
  duration,
  snapshot,
  disabled,
  onToggle,
  onSeek,
  onRate,
}: Props) {
  const selected = snapshot.index === index;
  const loaded = selected && !snapshot.loading && !snapshot.error;
  const total = selected ? snapshot.duration : duration;
  const position = selected ? snapshot.position : 0;
  return (
    <div className="block-player" aria-label={`Audio segment ${index + 1}`}>
      <button
        type="button"
        disabled={disabled}
        onClick={onToggle}
        aria-label={`${selected && snapshot.playing ? "Pause" : "Play"} segment ${index + 1}`}
      >
        {selected && snapshot.loading
          ? "Loading…"
          : selected && snapshot.playing
            ? "Pause"
            : "Play"}
      </button>
      <input
        type="range"
        min={0}
        max={total}
        step={0.1}
        value={position}
        disabled={disabled || !loaded}
        onChange={(e) => onSeek(Number(e.target.value))}
        aria-label={`Position in segment ${index + 1}`}
      />
      <span>
        {formatDuration(position)} / {formatDuration(total)}
      </span>
      <label>
        Speed{" "}
        <select
          value={snapshot.rate}
          disabled={disabled}
          onChange={(e) => onRate(Number(e.target.value))}
        >
          {PLAYBACK_RATES.map((rate) => (
            <option key={rate} value={rate}>
              {rate}×
            </option>
          ))}
        </select>
      </label>
      {selected && snapshot.loading && (
        <p role="status">
          Preparing this block locally. Long audio may take a moment.
        </p>
      )}
      {selected && snapshot.error && (
        <p role="alert" className="error-message">
          {snapshot.error}
        </p>
      )}
    </div>
  );
}
