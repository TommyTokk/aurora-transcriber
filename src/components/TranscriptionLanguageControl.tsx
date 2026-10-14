import { LanguageSelect } from "./LanguageSelect";
import type {
  TranscriptionLanguage,
  TranscriptionSettings,
} from "../lib/settings";

interface Props {
  settings: TranscriptionSettings | null;
  disabled: boolean;
  pending: boolean;
  error: string | null;
  onSelect: (language: TranscriptionLanguage) => Promise<void>;
}
/** Displays the persistent source-language preference and safe read or save failures. */
export function TranscriptionLanguageControl({
  settings,
  disabled,
  pending,
  error,
  onSelect,
}: Props) {
  return (
    <section className="panel" aria-labelledby="language-heading">
      <div className="section-heading">
        <h2 id="language-heading">Transcription Language</h2>
        <span className="badge">Saved preference</span>
      </div>
      <fieldset
        className="asr-device-control"
        disabled={disabled || pending || (!settings && !error)}
      >
        <legend>Source language</legend>
        <LanguageSelect
          id="transcription-language"
          label="Audio language"
          value={settings?.language ?? "auto"}
          describedBy="language-policy"
          onSelect={onSelect}
        />
        <p id="language-policy" className="muted">
          Your choice is saved automatically and is the default for loaded
          files. You can choose a different language for an individual file.
          Auto-detect lets the model handle the language. A selected language is
          a source-language hint, not a translation request.
        </p>
      </fieldset>
      {pending && (
        <p className="muted" role="status">
          Saving language preference…
        </p>
      )}
      {settings?.warning && (
        <p className="error-message" role="status">
          {settings.warning}
        </p>
      )}
      {error && (
        <p className="error-message" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
