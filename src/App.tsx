import { useState } from "react";
import {
  AlertCircle,
  AudioLines,
  BrainCircuit,
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
import { UnsavedDialog } from "./components/UnsavedDialog";

type View = "studio" | "models" | "settings";
const navigation = [
  { id: "studio", label: "Transcription Studio", icon: AudioLines },
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
  models: {
    title: "Your AI Model Library",
    eyebrow: "LOCAL ENGINE & MODELS",
    description:
      "Manage the models that power your private transcription workspace.",
  },
  settings: {
    title: "Application Settings",
    eyebrow: "ENGINE CONFIGURATION",
    description: "Choose how your device powers speech transcription.",
  },
};
export default function App() {
  const [view, setView] = useState<View>("studio");
  const audio = useAudioImport();
  const transcription = useTranscription();
  const folder = useModelStorage();
  const player = useTranscriptPlayback(
    transcription.audioId,
    transcription.audioAvailable,
  );
  const disabled =
    !audio.native ||
    transcription.pending ||
    transcription.associating ||
    !transcription.connected;
  const error =
    audio.connectionError ?? audio.update.error ?? transcription.error;
  const model = transcription.model;
  const asr = model.models.find((item) => item.purpose === "ASR");
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
  function navigate(next: View) {
    if (next !== view) player.pause();
    setView(next);
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
            <div className="studio-grid">
              <aside className="studio-context">
                {audio.update.metadata && (
                  <AudioFileCard
                    file={audio.update.metadata}
                    busy={audio.busy}
                    onReplace={audio.chooseFile}
                    onRemove={audio.removeFile}
                  />
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
                      transcription.start(audio.update.audioId);
                  }}
                  onCancel={() => void transcription.cancel()}
                />
                <TranscriptArchive
                  entries={transcription.archive}
                  disabled={disabled || transcription.active}
                  onOpen={transcription.open}
                />
              </aside>
              <div className="transcript-canvas">
                <ProgressTracker job={transcription.job} />
                {transcription.hasResult ? (
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
                ) : (
                  <section className="panel empty-transcript">
                    <span className="feature-icon">
                      <AudioLines size={32} />
                    </span>
                    <h2>
                      {transcription.active
                        ? "Your words are coming to light"
                        : "A clear space for your words"}
                    </h2>
                    <p className="muted">
                      {transcription.active
                        ? "Processing your audio locally. You can visit Models or Settings while transcription continues."
                        : audio.update.metadata
                          ? "Your audio is selected. Start transcription when the local models are available."
                          : "Choose an audio file or open a saved transcript to begin."}
                    </p>
                    <span className="privacy-badge">
                      <LockKeyhole size={14} />
                      No audio uploads. No tracking.
                    </span>
                  </section>
                )}
              </div>
            </div>
          </div>
          <div hidden={view !== "models"}>
            <Models
              native={audio.native}
              model={model}
              active={transcription.active}
              disabled={disabled}
              onDownload={transcription.download}
              folder={folder}
            />
          </div>
          <div hidden={view !== "settings"} className="settings-content">
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
