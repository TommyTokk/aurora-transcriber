import { BrainCircuit, FolderOpen, Download, AudioLines } from "lucide-react";
import type { ModelSnapshot } from "../lib/transcription";
import type { useModelStorage } from "../hooks/useModelStorage";
interface Props {
  native: boolean;
  model: ModelSnapshot;
  active: boolean;
  disabled: boolean;
  onDownload: () => void;
  folder: ReturnType<typeof useModelStorage>;
}
export function Models({
  native,
  model,
  active,
  disabled,
  onDownload,
  folder,
}: Props) {
  const { storage, pending, error, perform } = folder;
  const busy =
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
            {model.models.find((item) => item.purpose === "ASR")?.name ??
              "No model information available"}
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
            <p className="muted">{item.version}</p>
            <div className="tag-row">
              <span className="badge">{item.format}</span>
              {item.quantization && (
                <span className="badge">{item.quantization}</span>
              )}
              {item.purpose === "VAD" && (
                <span className="badge mint">CPU</span>
              )}
            </div>
          </section>
        ))}
      </div>
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
            <Download size={16} /> Download models
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
