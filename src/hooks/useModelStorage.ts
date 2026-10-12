import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface StorageSnapshot {
  path: string;
  isDefault: boolean;
  accessible: boolean;
  missing: string[];
  error: string | null;
}
/** Owns model-folder events independently of navigation and native picker lifetime. */
export function useModelStorage() {
  const native = isTauri();
  const [storage, setStorage] = useState<StorageSnapshot | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let revision = 0;
    let refreshing = false;
    const listeners: UnlistenFn[] = [];
    function retain(listener: UnlistenFn) {
      if (disposed) listener();
      else listeners.push(listener);
    }
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
  }, [native]);
  async function perform(
    command:
      | "choose_model_directory"
      | "reset_model_directory"
      | "open_model_directory",
  ) {
    if (!native || pending) return;
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
  return { storage, pending, error, perform };
}
