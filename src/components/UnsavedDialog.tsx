import { useEffect, useRef } from "react";
interface Props {
  onDiscard: () => void;
  onKeep: () => void;
}
/** Uses a modal native HTML dialog to protect unsaved text and trap keyboard focus. */
export function UnsavedDialog({ onDiscard, onKeep }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => {
      element?.close();
    };
  }, []);
  return (
    <dialog
      ref={dialog}
      className="unsaved-dialog"
      aria-labelledby="unsaved-title"
      onCancel={(event) => {
        event.preventDefault();
        onKeep();
      }}
    >
      <h2 id="unsaved-title">Unsaved changes</h2>
      <p>
        Save your transcript before leaving if you want to keep your changes.
      </p>
      <div className="action-row">
        <button className="primary-button" onClick={onKeep} autoFocus>
          Keep editing
        </button>
        <button className="secondary-button" onClick={onDiscard}>
          Discard changes
        </button>
      </div>
    </dialog>
  );
}
