import type { TranscriptionLanguage } from "./settings";

/** A transient override belongs to one native import identity; null means use the saved default. */
export interface FileLanguageSelection {
  audioId: string | null;
  language: TranscriptionLanguage | null;
}

/** Preserves an override for the same import, but discards it after removal or replacement. */
export function synchronizeFileLanguage(
  selection: FileLanguageSelection,
  audioId: string | null,
): FileLanguageSelection {
  return selection.audioId === audioId
    ? selection
    : { audioId, language: null };
}

/** Resolves the displayed language without treating an explicit Auto-detect override as inheritance. */
export function effectiveFileLanguage(
  selection: FileLanguageSelection,
  defaultLanguage: TranscriptionLanguage,
): TranscriptionLanguage {
  return selection.language ?? defaultLanguage;
}
