import { LanguageSelect } from "./LanguageSelect";
import type { TranscriptionLanguage } from "../lib/settings";

interface Props {
  language: TranscriptionLanguage;
  usesDefault: boolean;
  disabled: boolean;
  onSelect: (language: TranscriptionLanguage) => void;
  onUseDefault: () => void;
}

/** Displays a transient language choice for the current file and an explicit return to the saved default. */
export function FileTranscriptionLanguageControl({
  language,
  usesDefault,
  disabled,
  onSelect,
  onUseDefault,
}: Props) {
  return (
    <fieldset className="file-language-control" disabled={disabled}>
      <div className="file-language-choice">
        <LanguageSelect
          id="file-transcription-language"
          label="Transcription language"
          value={language}
          describedBy="file-language-policy"
          onSelect={onSelect}
        />
        <button
          type="button"
          className="secondary-button"
          onClick={onUseDefault}
          disabled={usesDefault}
        >
          Use default
        </button>
      </div>
      <p id="file-language-policy" className="muted" role="status">
        {usesDefault
          ? "Using your saved language preference."
          : "Applies only to this file. Your saved preference is unchanged."}
      </p>
    </fieldset>
  );
}
