import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  EMPTY_JOB,
  EMPTY_MODEL,
  isJobActive,
  isNewSnapshot,
  type ModelSnapshot,
  type SubtitleSegment,
  type TranscriptionSnapshot,
  type TranscriptDocument,
  type TranscriptSummary,
} from "../lib/transcription";

/** Coordinates native jobs, edited text, explicit persistence, and archive reopening. */
export function useTranscription() {
  const [model, setModel] = useState(EMPTY_MODEL);
  const [job, setJob] = useState(EMPTY_JOB);
  const latest = useRef(EMPTY_JOB);
  const [segments, setSegments] = useState<SubtitleSegment[]>([]);
  const [document, setDocument] = useState<TranscriptDocument | null>(null);
  const [name, setName] = useState("");
  const [hasResult, setHasResult] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [archive, setArchive] = useState<TranscriptSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [connected, setConnected] = useState(false);
  const [confirmAction, setConfirmAction] = useState<(() => void) | null>(null);
  const native = isTauri();
  const active = isJobActive(job.phase);
  const activeRef = useRef(false);
  activeRef.current = active;

  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let modelEvent = false;
    const listeners: UnlistenFn[] = [];
    /** Releases subscriptions whose async setup completes after unmount. */
    function retain(unlisten: UnlistenFn) {
      if (disposed) unlisten();
      else listeners.push(unlisten);
    }
    /** Applies ordered job snapshots without overwriting a user's later edits. */
    function receive(incoming: TranscriptionSnapshot) {
      if (disposed || !isNewSnapshot(latest.current, incoming)) return;
      latest.current = incoming;
      setJob(incoming);
      setName(incoming.name);
      setDocument(null);
      setDirty(false);
      setNotice(null);
      setSegments(incoming.segments);
      setHasResult(incoming.phase === "completed");
      if (incoming.error) setError(incoming.error);
    }
    /** Subscribes before state reads so fast jobs and model loads are not missed. */
    async function connect() {
      try {
        retain(
          await listen<TranscriptionSnapshot>(
            "transcription-progress",
            (event) => receive(event.payload),
          ),
        );
        retain(
          await listen<ModelSnapshot>("transcriber-state", (event) => {
            if (!disposed) {
              modelEvent = true;
              setModel(event.payload);
            }
          }),
        );
        if (disposed) return;
        const [initialJob, initialModel, entries] = await Promise.all([
          invoke<TranscriptionSnapshot>("transcription_state"),
          invoke<ModelSnapshot>("transcriber_state"),
          invoke<TranscriptSummary[]>("list_transcripts"),
        ]);
        if (disposed) return;
        receive(initialJob);
        if (!modelEvent) setModel(initialModel);
        setArchive(entries);
        setConnected(true);
      } catch {
        if (!disposed)
          setError(
            "Unable to connect to transcription. Please restart the app.",
          );
        listeners.splice(0).forEach((unlisten) => unlisten());
      }
    }
    void connect();
    return () => {
      disposed = true;
      listeners.forEach((unlisten) => unlisten());
    };
  }, [native]);

  /** Runs an exclusive foreground action and displays only native safe failures. */
  async function perform(action: () => Promise<void>) {
    if (pending || !connected) return;
    setPending(true);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (failure) {
      setError(
        typeof failure === "string"
          ? failure
          : "Unable to complete this action. Please try again.",
      );
    } finally {
      setPending(false);
    }
  }
  /** Protects unsaved edits before starting a job or replacing the current result. */
  function guard(action: () => void) {
    if (dirty) setConfirmAction(() => action);
    else action();
  }
  /** Starts the selected native audio job after any unsaved-edit warning. */
  function start(audioId: string) {
    guard(() => {
      void perform(async () => {
        // The promise can complete after a newer event; accept only newer revisions.
        const result = await invoke<TranscriptionSnapshot>(
          "process_audio_file",
          { audioId },
        );
        if (isNewSnapshot(latest.current, result)) {
          latest.current = result;
          setJob(result);
          setSegments(result.segments);
          setName(result.name);
          setDocument(null);
          setDirty(false);
          setHasResult(result.phase === "completed");
        }
      });
    });
  }
  /** Requests cancellation independently of the still-pending start command. */
  async function cancel() {
    try {
      await invoke("cancel_transcription");
      setNotice("Cancellation requested. Waiting for the current operation.");
    } catch {
      setError("Unable to request cancellation.");
    }
  }
  /** Starts model provisioning only after the explicit download button click. */
  function download() {
    void perform(async () => {
      await invoke("download_models");
    });
  }
  /** Edits one segment without modifying original-file timestamps. */
  function edit(index: number, text: string) {
    setSegments((current) =>
      current.map((segment, i) =>
        i === index ? { ...segment, text } : segment,
      ),
    );
    setDirty(true);
    setNotice(null);
  }
  /** Persists current edits, reusing the same archive ID on subsequent saves. */
  function save() {
    void perform(async () => {
      const value = await invoke<TranscriptDocument>("save_transcript", {
        id: document?.id ?? null,
        name,
        segments,
      });
      setDocument(value);
      setDirty(false);
      setNotice("Transcript saved locally.");
      setArchive(await invoke<TranscriptSummary[]>("list_transcripts"));
    });
  }
  /** Reopens a saved document after explicitly handling unsaved current edits. */
  function open(id: string) {
    guard(() => {
      void perform(async () => {
        if (activeRef.current) return;
        const value = await invoke<TranscriptDocument>("open_transcript", {
          id,
        });
        setDocument(value);
        setName(value.name);
        setSegments(value.segments);
        setHasResult(true);
        setDirty(false);
        setError(null);
        setNotice("Saved transcript opened.");
      });
    });
  }
  /** Exports the current edited segments using a backend native save dialog. */
  function exportFile(format: "txt" | "srt" | "vtt") {
    void perform(async () => {
      const saved = await invoke<boolean>("export_subtitles", {
        segments,
        format,
      });
      if (saved) setNotice("Transcript exported.");
    });
  }
  /** Confirms discarding edits and executes the requested replacement action. */
  function discard() {
    const action = confirmAction;
    setConfirmAction(null);
    if (action) action();
  }
  /** Keeps unsaved edits and dismisses the replacement warning. */
  function keepEditing() {
    setConfirmAction(null);
  }

  return {
    model,
    job,
    segments,
    name,
    hasResult,
    dirty,
    archive,
    error,
    notice,
    pending,
    active,
    connected,
    confirmAction: !!confirmAction,
    start,
    cancel,
    download,
    edit,
    save,
    open,
    exportFile,
    discard,
    keepEditing,
  };
}
