import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  modelLabel,
  type AsrSelection,
  type ModelSnapshot,
} from "../lib/transcription";
import { AsrDeviceControl } from "./AsrDeviceControl";

interface StorageSnapshot {
  path: string;
  isDefault: boolean;
  accessible: boolean;
  missing: string[];
  error: string | null;
}
interface Props {
  model: ModelSnapshot;
  active: boolean;
  disabled: boolean;
  onDownload: () => void;
  onSelectDevice: (selection: AsrSelection) => void;
}
/** Displays native model storage and invokes only commands without path arguments. */
export function Models({
  model,
  active,
  disabled,
  onDownload,
  onSelectDevice,
}: Props) {
  const [storage, setStorage] = useState<StorageSnapshot | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busy =
    disabled ||
    active ||
    pending ||
    ["loading", "downloading"].includes(model.phase);
  useEffect(() => {
    let disposed = false;
    let revision = 0;
    let refreshing = false;
    const listeners: UnlistenFn[] = [];
    /** Retains subscriptions safely when setup completes after component teardown. */
    function retain(listener: UnlistenFn) {
      if (disposed) listener();
      else listeners.push(listener);
    }
    /** Refreshes removable-folder availability without overwriting newer selection events.
     * Polling never completes or releases a pending native picker.
     */
    async function refresh() {
      if (disposed || refreshing) return;
      refreshing = true;
      const ticket = ++revision;
      try {
        const value = await invoke<StorageSnapshot>("model_storage_state");
        if (!disposed && ticket === revision) setStorage(value);
      } catch {
        if (!disposed && ticket === revision)
          setError("Unable to read model folder settings.");
      } finally {
        refreshing = false;
      }
    }
    /** Subscribes before reading state so native picker completion cannot be missed. */
    async function connect() {
      try {
        retain(
          await listen<StorageSnapshot>(
            "model-storage-state",
            ({ payload }) => {
              if (!disposed) {
                revision += 1;
                setStorage(payload);
                setPending(false);
              }
            },
          ),
        );
        retain(
          await listen<string>("model-storage-error", ({ payload }) => {
            if (!disposed) {
              setError(payload);
              setPending(false);
            }
          }),
        );
        await refresh();
      } catch {
        if (!disposed) setError("Unable to read model folder settings.");
      }
    }
    void connect();
    const interval = window.setInterval(() => void refresh(), 15_000);
    window.addEventListener("focus", refresh);
    return () => {
      disposed = true;
      window.clearInterval(interval);
      window.removeEventListener("focus", refresh);
      listeners.forEach((listener) => listener());
    };
  }, []);
  /** Shows safe failures and keeps the picker reserved until the backend completion event. */
  async function perform(
    command:
      | "choose_model_directory"
      | "reset_model_directory"
      | "open_model_directory",
  ) {
    if (pending) return;
    setPending(true);
    setError(null);
    try {
      await invoke(command);
    } catch (failure) {
      setError(
        typeof failure === "string"
          ? failure
          : "Unable to update model folder.",
      );
      setPending(false);
      return;
    }
    if (command !== "choose_model_directory") setPending(false);
  }
  return (
    <section className="transcription-panel models-panel" aria-label="Models">
      <h2>Models</h2>
      <ul aria-label="Required models" className="muted">
        {model.models.map((item) => (
          <li key={item.id}>{modelLabel(item)}</li>
        ))}
      </ul>
      <AsrDeviceControl
        model={model}
        disabled={busy}
        onSelect={onSelectDevice}
      />
      <p className="model-path">{storage?.path ?? "Resolving model folder…"}</p>
      <p className="muted">
        {storage?.isDefault
          ? "Default application folder"
          : "Custom model folder"}
      </p>
      {(error || storage?.error) && (
        <p className="error-message" role="alert">
          {error ?? storage?.error}
        </p>
      )}
      <p role="status">
        {storage?.error
          ? "Model folder unavailable"
          : model.phase === "ready"
            ? "Verified models ready"
            : model.phase === "unloaded"
              ? "Models on disk; sessions will reload offline."
              : model.phase === "downloading"
                ? `Downloading ${model.downloadFile ?? "models"}…`
                : model.phase === "loading"
                  ? "Verifying and loading local models…"
                  : "Models unavailable"}
      </p>
      {model.error && model.error !== storage?.error && (
        <p className="error-message">{model.error}</p>
      )}
      {!!storage?.missing.length && (
        <p className="muted">Missing: {storage.missing.join(", ")}</p>
      )}
      {model.phase === "downloading" && (
        <progress
          max={100}
          value={model.percentage ?? 0}
          aria-label="Model download progress"
        />
      )}
      <div className="action-row">
        <button
          className="secondary-button"
          disabled={busy}
          onClick={() => void perform("choose_model_directory")}
        >
          Choose folder
        </button>
        <button
          className="secondary-button"
          disabled={busy}
          onClick={() => void perform("reset_model_directory")}
        >
          Use default
        </button>
        <button
          className="secondary-button"
          disabled={busy || !storage?.accessible}
          onClick={() => void perform("open_model_directory")}
        >
          Open folder
        </button>
        <button
          className="secondary-button"
          disabled={busy || !storage?.accessible}
          onClick={onDownload}
        >
          Download models
        </button>
      </div>
      <p className="muted">
        Changing folders keeps existing files in place. Models leave memory
        after 5 minutes without use.
      </p>
    </section>
  );
}
