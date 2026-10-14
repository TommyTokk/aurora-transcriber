import { useState } from "react";
import {
  effectiveFileLanguage,
  synchronizeFileLanguage,
  type FileLanguageSelection,
} from "../lib/fileLanguage";
import type { TranscriptionLanguage } from "../lib/settings";

/** Keeps a per-import override in memory while untouched files follow the saved language. */
export function useFileTranscriptionLanguage(
  audioId: string | null,
  defaultLanguage: TranscriptionLanguage,
) {
  const [selection, setSelection] = useState<FileLanguageSelection>({
    audioId,
    language: null,
  });
  const current = synchronizeFileLanguage(selection, audioId);
  // Reset during render so a newly imported file can never start with the previous file's override.
  if (current !== selection) setSelection(current);

  /** Sets a local override without persisting or changing the global preference. */
  function selectLanguage(language: TranscriptionLanguage) {
    if (audioId !== null) setSelection({ audioId, language });
  }

  /** Removes the local override and resumes following subsequent global preference changes. */
  function useDefault() {
    setSelection({ audioId, language: null });
  }

  return {
    language: effectiveFileLanguage(current, defaultLanguage),
    override: current.language ?? undefined,
    usesDefault: current.language === null,
    selectLanguage,
    useDefault,
  };
}
