import { useEffect, useRef } from "react";
import type { ModelDescriptor } from "../lib/transcription";
interface Props {
  model: ModelDescriptor;
  disabled: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}
/** Traps focus and requires explicit confirmation before deleting local model weights. */
export function ModelDeleteDialog({
  model,
  disabled,
  onConfirm,
  onCancel,
}: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="unsaved-dialog"
      aria-labelledby="delete-model-title"
      onCancel={(event) => {
        event.preventDefault();
        onCancel();
      }}
    >
      <h2 id="delete-model-title">Delete {model.name}?</h2>
      <p>
        The model file and its download cache will be removed from the current
        model folder.
      </p>
      <p>
        {model.purpose === "VAD"
          ? "Silero is required by both transcription models. Download it again before transcribing."
          : "If this model is selected, transcription will require another model selection or a new download."}
      </p>
      <div className="action-row">
        <button className="primary-button" onClick={onCancel} autoFocus>
          Keep model
        </button>
        <button
          className="secondary-button"
          disabled={disabled}
          onClick={onConfirm}
        >
          Delete model
        </button>
      </div>
    </dialog>
  );
}
