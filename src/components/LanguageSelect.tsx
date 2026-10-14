import type { ChangeEvent } from "react";
import {
  LANGUAGE_OPTIONS,
  isTranscriptionLanguage,
  type TranscriptionLanguage,
} from "../lib/settings";

interface Props {
  id: string;
  label: string;
  value: TranscriptionLanguage;
  disabled?: boolean;
  describedBy: string;
  onSelect: (language: TranscriptionLanguage) => void | Promise<void>;
}

/** Renders the shared, labeled language choices for persistent and per-file controls. */
export function LanguageSelect({
  id,
  label,
  value,
  disabled,
  describedBy,
  onSelect,
}: Props) {
  /** Validates the selected value before passing it to the preference owner. */
  function handleChange(event: ChangeEvent<HTMLSelectElement>) {
    const language = event.target.value;
    if (isTranscriptionLanguage(language)) void onSelect(language);
  }
  return (
    <>
      <label htmlFor={id}>{label}</label>
      <select
        id={id}
        value={value}
        disabled={disabled}
        onChange={handleChange}
        aria-describedby={describedBy}
      >
        {LANGUAGE_OPTIONS.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </>
  );
}
