import { AlertCircle, AudioLines, LockKeyhole, Sparkles } from "lucide-react";
import { AudioFileCard } from "./components/AudioFileCard";
import { DropZone } from "./components/DropZone";
import { useAudioImport } from "./hooks/useAudioImport";
/** Renders the local audio workspace with native import and validation states. */
export default function App() {
  const {
    update,
    dragging,
    busy,
    available,
    native,
    connectionError,
    chooseFile,
    removeFile,
  } = useAudioImport();
  const error = connectionError ?? update.error;
  const status =
    update.phase === "validating"
      ? "Validating audio locally."
      : update.phase === "picking"
        ? "File picker open."
        : update.phase === "ready"
          ? "Audio verified. Your file is ready."
          : "Choose an audio file to get started.";
  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="brand">
          <span className="brand-mark">
            <AudioLines size={24} aria-hidden="true" />
          </span>
          <span>AURORA</span>
        </div>
        <span className="privacy-badge">
          <LockKeyhole size={13} aria-hidden="true" /> Local by nature
        </span>
      </header>
      <div className="workspace">
        <div className="intro">
          <span className="eyebrow">
            <Sparkles size={14} aria-hidden="true" /> YOUR AUDIO, YOUR SPACE
          </span>
          <h1>
            Let your words <span>come to light.</span>
          </h1>
          <p>A quiet space for your audio. Private, local, and always yours.</p>
        </div>
        <DropZone
          dragging={dragging}
          busy={busy}
          available={available}
          validating={update.phase === "validating"}
          onChoose={chooseFile}
        />
        <div className="selection-area">
          {error && (
            <p className="error-message" role="alert">
              <AlertCircle size={18} aria-hidden="true" />
              {error}
            </p>
          )}
          {update.metadata && (
            <AudioFileCard
              file={update.metadata}
              busy={busy}
              onReplace={chooseFile}
              onRemove={removeFile}
            />
          )}
          {!native && (
            <p className="preview-note">
              Open the AURORA desktop app to choose and validate audio files.
            </p>
          )}
          {update.phase === "ready" && (
            <p className="next-step">
              Your audio is verified. Transcription will arrive in a future
              update.
            </p>
          )}
        </div>
        <p className="sr-only" role="status" aria-live="polite">
          {status}
        </p>
      </div>
      <footer className="app-footer">
        <LockKeyhole size={13} aria-hidden="true" />
        <span>Your audio stays on this device. No uploads. No tracking.</span>
      </footer>
    </main>
  );
}
