import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  DRAG_EVENT,
  EMPTY_IMPORT,
  IMPORT_EVENT,
  type ImportUpdate,
} from "../lib/audioImport";
/** Synchronizes native imports and releases listeners even after late setup. */
export function useAudioImport() {
  const [update, setUpdate] = useState<ImportUpdate>(EMPTY_IMPORT);
  const [dragging, setDragging] = useState(false);
  const [pending, setPending] = useState(false);
  const [connected, setConnected] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const native = isTauri();
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let receivedEvent = false;
    const listeners: UnlistenFn[] = [];
    /** Releases a listener immediately if setup finishes after unmount. */
    function retain(unlisten: UnlistenFn) {
      if (disposed) unlisten();
      else listeners.push(unlisten);
    }
    /** Subscribes before reading state so completed imports cannot be missed. */
    async function connect() {
      try {
        retain(
          await listen<ImportUpdate>(IMPORT_EVENT, (event) => {
            if (disposed) return;
            receivedEvent = true;
            setUpdate(event.payload);
          }),
        );
        if (disposed) return;
        retain(
          await listen<boolean>(DRAG_EVENT, (event) => {
            if (!disposed) setDragging(event.payload);
          }),
        );
        if (disposed) return;
        const snapshot = await invoke<ImportUpdate>("audio_import_state");
        if (!disposed) {
          // Events received during setup take precedence over the initial snapshot.
          if (!receivedEvent) setUpdate(snapshot);
          setConnected(true);
        }
      } catch {
        if (!disposed)
          setConnectionError(
            "Unable to connect to audio import. Please restart the app.",
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
  const busy =
    pending ||
    update.processing ||
    update.phase === "picking" ||
    update.phase === "validating";
  /** Invokes a path-free native action and displays safe command failures. */
  async function perform(command: "choose_audio_file" | "clear_audio_file") {
    if (!connected || busy) return;
    setPending(true);
    try {
      await invoke(command);
    } catch {
      setUpdate({
        ...EMPTY_IMPORT,
        phase: "error",
        error: "Unable to complete this action. Please try again.",
      });
    } finally {
      setPending(false);
    }
  }
  /** Opens the OS picker without passing paths from JavaScript. */
  async function chooseFile() {
    await perform("choose_audio_file");
  }
  /** Clears the current selection in both native and frontend state. */
  async function removeFile() {
    await perform("clear_audio_file");
  }
  return {
    update,
    dragging: dragging && !busy,
    busy,
    available: connected,
    native,
    connectionError,
    chooseFile,
    removeFile,
  };
}
