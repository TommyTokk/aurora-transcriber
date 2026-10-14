import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  TranscriptionLanguage,
  TranscriptionSettings,
} from "../lib/settings";

/** Restores native language preferences and commits changes before updating the visible selection. */
export function useTranscriptionSettings() {
  const native = isTauri();
  const [settings, setSettings] = useState<TranscriptionSettings | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    /** Loads the backend preference while preventing updates after unmount. */
    async function load() {
      try {
        const value = await invoke<TranscriptionSettings>(
          "transcription_settings",
        );
        if (!disposed) setSettings(value);
      } catch {
        if (!disposed)
          setError(
            "Unable to read transcription language. Select a language to retry saving.",
          );
      }
    }
    void load();
    return () => {
      disposed = true;
    };
  }, [native]);
  /** Saves one preference, preserving the previous choice and warning when persistence fails. */
  async function selectLanguage(language: TranscriptionLanguage) {
    if (!native || saving.current) return;
    saving.current = true;
    setPending(true);
    setError(null);
    try {
      const value = await invoke<TranscriptionSettings>(
        "set_transcription_language",
        { language },
      );
      setSettings(value);
    } catch (failure) {
      setError(
        typeof failure === "string"
          ? failure
          : "Unable to save transcription language. Your previous preference is still active.",
      );
    } finally {
      saving.current = false;
      setPending(false);
    }
  }
  return { settings, pending, error, selectLanguage };
}
