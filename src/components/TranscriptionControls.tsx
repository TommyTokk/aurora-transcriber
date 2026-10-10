import { Play, Square } from "lucide-react";
import type { ModelSnapshot } from "../lib/transcription";
interface Props {
  model: ModelSnapshot;
  canStart: boolean;
  active: boolean;
  disabled: boolean;
  onStart: () => void;
  onCancel: () => void;
}
/** Displays model availability and explicit download, start, and cancel controls. */
export function TranscriptionControls({
  model,
  canStart,
  active,
  disabled,
  onStart,
  onCancel,
}: Props) {
  const modelBusy = ["loading", "downloading"].includes(model.phase);
  return (
    <section
      className="transcription-panel"
      aria-label="Transcription controls"
    >
      <div className="action-row">
        {active ? (
          <button className="secondary-button" onClick={onCancel}>
            <Square size={16} /> Cancel transcription
          </button>
        ) : (
          <button
            className="primary-button"
            onClick={onStart}
            disabled={
              !canStart ||
              disabled ||
              modelBusy ||
              !["ready", "unloaded"].includes(model.phase)
            }
          >
            <Play size={16} /> Transcribe audio
          </button>
        )}
      </div>
      <p className="muted">
        Models stay on disk and leave memory after 5 minutes without use.
      </p>
    </section>
  );
}
