interface Props {
  dirty: boolean;
  disabled: boolean;
  onSave: () => void;
  onExport: (format: "txt" | "srt" | "vtt") => void;
}
/** Offers explicit archive persistence and export of the current edited text. */
export function TranscriptActions({
  dirty,
  disabled,
  onSave,
  onExport,
}: Props) {
  return (
    <div className="transcript-actions">
      <button className="primary-button" disabled={disabled} onClick={onSave}>
        Save transcript{dirty ? " *" : ""}
      </button>
      {(["txt", "srt", "vtt"] as const).map((format) => (
        <button
          className="secondary-button"
          key={format}
          disabled={disabled}
          onClick={() => onExport(format)}
        >
          Export {format.toUpperCase()}
        </button>
      ))}
      <p className="muted">
        {dirty
          ? "You have unsaved changes."
          : "Saving is explicit. Your audio is never stored."}
      </p>
    </div>
  );
}
