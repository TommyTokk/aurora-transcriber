/** Supported source-language preferences shared by settings UI and native IPC. */
export type TranscriptionLanguage = "auto" | "it" | "en";
/** Effective native preference and recoverable configuration warning. */
export interface TranscriptionSettings {
  language: TranscriptionLanguage;
  warning: string | null;
}
export const LANGUAGE_OPTIONS = [
  { value: "auto", label: "Auto-detect" },
  { value: "it", label: "Italiano" },
  { value: "en", label: "English" },
] as const;
/** Rejects unknown select values instead of silently changing the user's preference. */
export function isTranscriptionLanguage(
  value: string,
): value is TranscriptionLanguage {
  return LANGUAGE_OPTIONS.some((option) => option.value === value);
}
