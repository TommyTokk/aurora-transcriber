import { BrainCircuit, FolderOpen, Download, AudioLines } from "lucide-react";
import { useState } from "react";
import { ModelDeleteDialog } from "./ModelDeleteDialog";
import type {
  AsrModelId,
  ModelDescriptor,
  ModelSnapshot,
} from "../lib/transcription";
import type { useModelStorage } from "../hooks/useModelStorage";
interface Props {
  native: boolean;
  model: ModelSnapshot;
  active: boolean;
  disabled: boolean;
  onDownload: () => void;
  onSelect: (id: AsrModelId) => void;
  onUnload: (id: AsrModelId) => void;
  onDownloadModel: (id: string) => void;
  onDelete: (id: string) => void;
  folder: ReturnType<typeof useModelStorage>;
}
/** Offers Select for installed ASR weights and Unload for the currently resident model. */
export function Models({
  native,
  model,
  active,
  disabled,
  onDownload,
  onSelect,
  onUnload,
  onDownloadModel,
  onDelete,
  folder,
}: Props) {
  const [deleting, setDeleting] = useState<ModelDescriptor | null>(null);
  const { storage, pending, error, perform } = folder;
  const busy =
    !native ||
    disabled ||
    active ||
    pending ||
    ["loading", "downloading"].includes(model.phase);
  return (
    <div className="models-content">
      <section className="panel model-overview">
        <span className="feature-icon">
          <BrainCircuit size={28} />
        </span>
        <div>
          <span className="eyebrow">LOCAL MODEL ENGINE</span>
          <h2>
            {model.models.find((item) => item.id === model.selectedModel)
              ?.name ?? "No model information available"}
          </h2>
          <p role="status">
            {!native
              ? "Model status is available in the desktop app."
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
        </div>
        <span className="privacy-badge">Local processing</span>
      </section>
      <div className="model-grid">
        {model.models.map((item) => (
          <section className="panel model-card" key={item.id}>
            <span
              className={`feature-icon ${item.purpose === "VAD" ? "mint" : ""}`}
            >
              <AudioLines size={22} />
            </span>
            <span className="badge">
              {item.purpose === "ASR"
                ? "Speech transcription"
                : "Voice activity detection"}
            </span>
            <h2>{item.name}</h2>
            <p className="muted">
              {item.version} ·{" "}
              {(item.sizeBytes / 1_000_000).toFixed(
                item.sizeBytes < 10_000_000 ? 1 : 0,
              )}{" "}
              MB
            </p>
            <p role="status">
              {model.installed.includes(item.id) ? "On disk" : "Not downloaded"}
              {item.id === model.selectedModel ? " · Selected ASR" : ""}
              {item.id === model.selectedModel && model.phase === "ready"
                ? " · Verified and ready"
                : ""}
            </p>
            <div className="tag-row">
              <span className="badge">{item.format}</span>
              {item.quantization && (
                <span className="badge">{item.quantization}</span>
              )}
              {item.purpose === "VAD" && (
                <span className="badge mint">CPU</span>
              )}
            </div>
            <div className="action-row">
              {item.purpose === "ASR" && (
                <button
                  className="secondary-button"
                  disabled={busy || !model.installed.includes(item.id)}
                  onClick={() => {
                    // Readiness belongs only to the selected ASR; other installed models can be selected.
                    if (
                      item.id === model.selectedModel &&
                      model.phase === "ready"
                    )
                      onUnload(item.id as AsrModelId);
                    else onSelect(item.id as AsrModelId);
                  }}
                >
                  {item.id === model.selectedModel && model.phase === "ready"
                    ? "Unload"
                    : "Select"}
                </button>
              )}
              <button
                className="primary-button"
                disabled={busy || !storage?.accessible}
                onClick={() => onDownloadModel(item.id)}
              >
                <Download size={16} />{" "}
                {model.installed.includes(item.id)
                  ? "Verify / repair"
                  : "Download"}
              </button>
              <button
                className="secondary-button"
                disabled={
                  busy ||
                  !storage?.accessible ||
                  !model.installed.includes(item.id)
                }
                onClick={() => setDeleting(item)}
              >
                Delete
              </button>
            </div>
          </section>
        ))}
      </div>
      {deleting && (
        <ModelDeleteDialog
          model={deleting}
          disabled={busy}
          onCancel={() => setDeleting(null)}
          onConfirm={() => {
            onDelete(deleting.id);
            setDeleting(null);
          }}
        />
      )}
      <section className="panel storage-panel">
        <div className="section-heading">
          <FolderOpen size={21} />
          <h2>Model storage</h2>
        </div>
        <p className="model-path">
          {storage?.path ?? "Model folder available in the desktop app"}
        </p>
        <p className="muted">
          {storage
            ? storage.isDefault
              ? "Default application folder"
              : "Custom model folder"
            : "Open AURORA desktop to manage local models."}
        </p>
        {(error || storage?.error) && (
          <p className="error-message" role="alert">
            {error ?? storage?.error}
          </p>
        )}
        {model.error && model.error !== storage?.error && (
          <p className="error-message" role="alert">
            {model.error}
          </p>
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
            className="primary-button"
            disabled={busy || !storage?.accessible}
            onClick={onDownload}
          >
            <Download size={16} /> Download selected + VAD
          </button>
        </div>
        <p className="muted">
          Changing folders keeps existing files in place. Models leave memory
          after 5 minutes without use.
        </p>
      </section>
    </div>
  );
}
