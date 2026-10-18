import { useState } from "react";
import {
  AlertCircle,
  AudioLines,
  BrainCircuit,
  FileText,
  LockKeyhole,
  Settings,
  SlidersHorizontal,
} from "lucide-react";
import { AudioFileCard } from "./components/AudioFileCard";
import { DropZone } from "./components/DropZone";
import { useAudioImport } from "./hooks/useAudioImport";
import { useTranscription } from "./hooks/useTranscription";
import { useModelStorage } from "./hooks/useModelStorage";
import { useTranscriptPlayback } from "./hooks/useTranscriptPlayback";
import { TranscriptionControls } from "./components/TranscriptionControls";
import { ProgressTracker } from "./components/ProgressTracker";
import { TranscriptEditor } from "./components/TranscriptEditor";
import { TranscriptActions } from "./components/TranscriptActions";
import { TranscriptArchive } from "./components/TranscriptArchive";
import { Models } from "./components/Models";
import { AsrDeviceControl } from "./components/AsrDeviceControl";
import { useTranscriptionSettings } from "./hooks/useTranscriptionSettings";
import { TranscriptionLanguageControl } from "./components/TranscriptionLanguageControl";
import { useFileTranscriptionLanguage } from "./hooks/useFileTranscriptionLanguage";
import { FileTranscriptionLanguageControl } from "./components/FileTranscriptionLanguageControl";
import { UnsavedDialog } from "./components/UnsavedDialog";

type View = "studio" | "archive" | "models" | "settings";
const navigation = [
  { id: "studio", label: "Transcription Studio", icon: AudioLines },
  { id: "archive", label: "Saved transcripts", icon: FileText },
  { id: "models", label: "AI Models", icon: BrainCircuit },
  { id: "settings", label: "Settings", icon: Settings },
] as const;
const headings: Record<
  View,
  { title: string; eyebrow: string; description: string }
> = {
  studio: {
    title: "Transcription & Audio Studio",
    eyebrow: "YOUR LOCAL WORKSPACE",
    description:
      "Bring your audio into focus. Transcribe, listen and refine, all on your device.",
  },
  archive: {
    title: "Saved transcripts",
    eyebrow: "YOUR LOCAL ARCHIVE",
    description:
      "Open a saved transcript to review and refine it in your studio.",
  },
  models: {
    title: "Your AI Model Library",
    eyebrow: "LOCAL ENGINE & MODELS",
    description:
      "Manage the models that power your private transcription workspace.",
  },
  settings: {
    title: "Application Settings",
    eyebrow: "ENGINE CONFIGURATION",
    description:
      "Choose the audio language and how your device powers speech transcription.",
  },
};
/** Keeps the local workspace state alive while switching between application pages. */
export default function App() {
  const [view, setView] = useState<View>("studio");
  const audio = useAudioImport();
  const transcription = useTranscription();
  const folder = useModelStorage();
  const language = useTranscriptionSettings();
  const fileLanguage = useFileTranscriptionLanguage(
    audio.update.audioId,
    language.settings?.language ?? "auto",
  );
  const player = useTranscriptPlayback(
    transcription.audioId,
    transcription.audioAvailable,
  );
  const disabled =
    !audio.native ||
    transcription.pending ||
    language.pending ||
    transcription.associating ||
    !transcription.connected;
  const error =
    audio.connectionError ?? audio.update.error ?? transcription.error;
  const model = transcription.model;
  const asr = model.models.find((item) => item.id === model.selectedModel);
  const status = !audio.native
    ? "Desktop preview"
    : transcription.active
      ? "Processing"
      : !transcription.connected
        ? "Connecting"
        : model.phase === "ready"
          ? "Ready"
          : model.phase === "unloaded"
            ? "On disk"
            : model.phase === "downloading"
              ? "Downloading"
              : model.phase === "loading"
                ? "Loading"
                : "Models unavailable";
  /** Changes the visible page and pauses playback without clearing workspace state. */
  function navigate(next: View) {
    if (next !== view) player.pause();
    setView(next);
  }
  /** Returns to the studio only after the guarded archive opening succeeds. */
  function openTranscript(id: string) {
    // Keep the archive visible if opening fails or the user keeps unsaved edits.
    transcription.open(id, () => navigate("studio"));
  }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <AudioLines size={24} />
          </span>
          <span>AURORA</span>
        </div>
        <div className="sidebar-status">
          <span className="status-dot" />
          <span>{status}</span>
        </div>
        <span className="nav-caption">WORKSPACE</span>
        <nav aria-label="Workspace">
          {navigation.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              className={`nav-item ${view === id ? "is-active" : ""}`}
              aria-current={view === id ? "page" : undefined}
              title={label}
              onClick={() => navigate(id)}
            >
              <Icon size={19} />
              <span>{label}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <LockKeyhole size={18} />
          <div>
            <strong>Private by nature</strong>
            <p>Audio stays on your device</p>
          </div>
        </div>
      </aside>
      <div className="app-body">
        <header className="app-header">
          <span className="engine-chip">
            <AudioLines size={17} />
            {asr ? `${asr.name} ${asr.version}` : "Local transcription engine"}
          </span>
          <div className="header-actions">
            <span className="status-badge">
              <span className="status-dot" />
              {status}
            </span>
            <button
              className="icon-button"
              aria-label="Open settings"
              onClick={() => navigate("settings")}
            >
              <SlidersHorizontal size={20} />
            </button>
          </div>
        </header>
        <main className="workspace">
          <div className="page-heading">
            <div>
              <span className="eyebrow">{headings[view].eyebrow}</span>
              <h1>{headings[view].title}</h1>
              <p className="muted">{headings[view].description}</p>
            </div>
            <span className="privacy-badge">
              <LockKeyhole size={15} />
              Local & private
            </span>
          </div>
          {!audio.native && (
            <p className="preview-note">
              Desktop preview. Open the AURORA desktop app to import audio,
              manage models and transcribe.
            </p>
          )}
          {error && (
            <p className="error-message" role="alert">
              <AlertCircle size={18} />
              {error}
            </p>
          )}
          {transcription.notice && (
            <p className="notice" role="status">
              {transcription.notice}
            </p>
          )}
          <div hidden={view !== "studio"}>
            <div className="studio-content">
              {audio.update.metadata && (
                <AudioFileCard
                  file={audio.update.metadata}
                  busy={audio.busy}
                  onReplace={audio.chooseFile}
                  onRemove={audio.removeFile}
                >
                  <FileTranscriptionLanguageControl
                    language={fileLanguage.language}
                    usesDefault={fileLanguage.usesDefault}
                    disabled={
                      disabled ||
                      audio.busy ||
                      transcription.active ||
                      !language.settings
                    }
                    onSelect={fileLanguage.selectLanguage}
                    onUseDefault={fileLanguage.useDefault}
                  />
                </AudioFileCard>
              )}
              <DropZone
                dragging={audio.dragging}
                busy={audio.busy}
                available={audio.available}
                validating={audio.update.phase === "validating"}
                onChoose={audio.chooseFile}
              />
              <TranscriptionControls
                model={model}
                canStart={
                  !!audio.update.audioId &&
                  !audio.busy &&
                  transcription.connected
                }
                active={transcription.active}
                disabled={disabled}
                onStart={() => {
                  if (audio.update.audioId)
                    transcription.start(
                      audio.update.audioId,
                      fileLanguage.override,
                    );
                }}
                onCancel={() => void transcription.cancel()}
              />

              <ProgressTracker job={transcription.job} />
              {transcription.hasResult && (
                <>
                  <div className="transcript-toolbar">
                    <TranscriptActions
                      dirty={transcription.dirty}
                      disabled={disabled || transcription.active}
                      onSave={transcription.save}
                      onExport={transcription.exportFile}
                    />
                  </div>
                  <TranscriptEditor
                    player={player}
                    name={transcription.name}
                    segments={transcription.segments}
                    disabled={disabled || transcription.active}
                    onEdit={transcription.edit}
                    audioId={transcription.audioId}
                    audioAvailable={transcription.audioAvailable}
                    legacy={transcription.legacy}
                    associating={transcription.associating}
                    onAssociate={() => void transcription.associateAudio()}
                  />
                </>
              )}
            </div>
          </div>
          <div hidden={view !== "archive"}>
            <TranscriptArchive
              entries={transcription.archive}
              disabled={disabled || transcription.active}
              onOpen={openTranscript}
            />
          </div>
          <div hidden={view !== "models"}>
            <Models
              native={audio.native}
              model={model}
              active={transcription.active}
              disabled={disabled}
              onDownload={transcription.download}
              onSelect={transcription.selectModel}
              onDownloadModel={transcription.downloadModel}
              onDelete={transcription.deleteModel}
              folder={folder}
            />
          </div>
          <div hidden={view !== "settings"} className="settings-content">
            <TranscriptionLanguageControl
              settings={language.settings}
              pending={language.pending}
              error={language.error}
              disabled={
                disabled ||
                transcription.active ||
                folder.pending ||
                ["loading", "downloading"].includes(model.phase)
              }
              onSelect={language.selectLanguage}
            />
            <section className="panel">
              <div className="section-heading">
                <Settings size={21} />
                <h2>Hardware & Acceleration</h2>
                <span className="badge">Session preference</span>
              </div>
              <p className="muted">
                Device changes apply immediately. Transcription must finish
                before switching devices.
              </p>
              <AsrDeviceControl
                model={model}
                disabled={
                  disabled ||
                  transcription.active ||
                  folder.pending ||
                  ["loading", "downloading"].includes(model.phase)
                }
                onSelect={transcription.selectDevice}
              />
            </section>
            <section className="panel privacy-panel">
              <div className="section-heading">
                <LockKeyhole size={21} />
                <h2>Local & Private</h2>
              </div>
              <p>
                Audio processing and transcription run on your device. Model
                downloads are explicitly initiated from AI Models.
              </p>
              <p className="muted">
                Transcripts are saved only when you choose Save. Your audio is
                never stored in the transcript archive.
              </p>
            </section>
          </div>
        </main>
      </div>
      {transcription.confirmAction && (
        <UnsavedDialog
          onDiscard={transcription.discard}
          onKeep={transcription.keepEditing}
        />
      )}
    </div>
  );
}
