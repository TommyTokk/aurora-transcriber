//! Explicit Hugging Face provisioning with verified, atomic local promotion.
use super::{resources, EngineError};
use crate::model_storage::ModelStorage;
use hf_hub::{
    api::{sync::ApiBuilder, Progress},
    Repo, RepoType,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

pub(crate) const SILERO_HASH: &str =
    "2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f";
/// Resolves model files solely inside the managed directory.
pub(crate) fn paths(base: &Path) -> (PathBuf, PathBuf) {
    (base.join("parakeet"), base.join("silero_vad.onnx"))
}
/// Lists absent resource files with safe filenames only; loading also verifies checksums.
pub(crate) fn missing(base: &Path) -> Vec<String> {
    let (asr, vad) = paths(base);
    let mut missing: Vec<_> = resources::FILES
        .iter()
        .filter(|(name, _)| !asr.join(name).is_file())
        .map(|(name, _)| name.to_string())
        .collect();
    if !vad.is_file() {
        missing.push("silero_vad.onnx".into());
    }
    missing
}
/// Verifies local resources without invoking Hugging Face or any network API.
pub(crate) fn validate(base: &Path) -> Result<(), EngineError> {
    let (asr, vad) = paths(base);
    resources::validate(&asr)?;
    resources::verify_file(&vad, SILERO_HASH)
}
/// Copies a verified downloaded file to its final name through an atomic promotion.
fn promote(source: &Path, destination: &Path, hash: &str) -> Result<(), String> {
    resources::verify_file(source, hash).map_err(|_| "Downloaded model failed its checksum.")?;
    let parent = destination.parent().ok_or("Invalid model destination.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Unable to create model directory.")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Unable to store model.")?;
    let mut input = std::fs::File::open(source).map_err(|_| "Unable to read downloaded model.")?;
    std::io::copy(&mut input, &mut temporary)
        .map_err(|_| "Unable to store model. Check available disk space.")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Unable to store model.")?;
    resources::verify_file(temporary.path(), hash)
        .map_err(|_| "Stored model failed its checksum.")?;
    temporary
        .persist(destination)
        .map_err(|_| "Unable to publish model.")?;
    Ok(())
}
/// Reuses verified files offline and invokes a transfer only for absent or corrupt resources.
fn ensure_file(
    destination: &Path,
    hash: &str,
    transfer: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<(), String> {
    if resources::verify_file(destination, hash).is_ok() {
        return Ok(());
    }
    let source = transfer()?;
    promote(&source, destination, hash)
}
/// Throttles path-free UI download updates to avoid overwhelming IPC.
struct DownloadProgress<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    file: String,
    index: usize,
    total: usize,
    received: usize,
    size: usize,
    last: Instant,
}
impl<R: tauri::Runtime> DownloadProgress<R> {
    /// Publishes the current file's byte fraction within the file-count progress range.
    fn publish(&mut self) {
        let fraction = if self.size == 0 {
            0.0
        } else {
            self.received as f64 / self.size as f64
        };
        self.app.state::<super::EngineState>().publish(
            &self.app,
            "downloading",
            None,
            None,
            vec![],
            Some(self.file.clone()),
            Some(((self.index as f64 + fraction.min(1.0)) / self.total as f64 * 100.0) as f32),
        );
        self.last = Instant::now();
    }
}
impl<R: tauri::Runtime> Progress for DownloadProgress<R> {
    /// Initializes one file's safe, byte-based progress.
    fn init(&mut self, size: usize, _filename: &str) {
        self.size = size;
        self.publish();
    }
    /// Accumulates received bytes and emits at most four progress updates per second.
    fn update(&mut self, size: usize) {
        self.received += size;
        if self.last.elapsed() >= Duration::from_millis(250) {
            self.publish();
        }
    }
    /// Marks the network transfer complete; checksum verification still follows.
    fn finish(&mut self) {
        self.received = self.size;
        self.publish();
    }
}
/// Downloads only missing or corrupt files after the explicit Download models action.
pub(crate) fn download<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let base = app.state::<ModelStorage>().prepare()?;
    let (asr, vad) = paths(&base);
    // This builder fixes the endpoint and disables environment token lookup and console output.
    let api = ApiBuilder::new()
        .with_cache_dir(base.join(".download-cache"))
        .with_token(None)
        .with_progress(false)
        .with_retries(2)
        .build()
        .map_err(|_| "Unable to initialize model download.")?;
    let parakeet = api.repo(Repo::with_revision(
        resources::REPOSITORY.into(),
        RepoType::Model,
        resources::REVISION.into(),
    ));
    let silero = api.model("Derur/silero-models".into());
    let total = resources::FILES.len() + 1;
    for (index, (filename, hash)) in resources::FILES
        .iter()
        .copied()
        .chain(std::iter::once(("silero_vad.onnx", SILERO_HASH)))
        .enumerate()
    {
        let target = if filename == "silero_vad.onnx" {
            vad.clone()
        } else {
            asr.join(filename)
        };
        ensure_file(&target, hash, || {
            let progress = DownloadProgress {
                app: app.clone(),
                file: filename.into(),
                index,
                total,
                received: 0,
                size: 0,
                last: Instant::now(),
            };
            let source = if filename == "silero_vad.onnx" {
                silero.download_with_progress("vad/silero_vad.onnx", progress)
            } else {
                parakeet.download_with_progress(filename, progress)
            }
            .map_err(|_| {
                "Model download failed. Check your connection and disk space, then retry."
            })?;
            Ok(source)
        })?;
    }
    validate(&base).map_err(|_| "Model verification failed.")?;
    // Only successful provisioning removes our own redundant transfer cache.
    let _ = std::fs::remove_dir_all(base.join(".download-cache"));
    let _ = app.emit_to("main", "models-downloaded", ());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Requires only GGUF ASR and Silero, preserving but ignoring legacy ASR files.
    #[test]
    fn legacy_asr_files_are_not_required() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let (asr, vad) = paths(directory.path());
        std::fs::create_dir(&asr)?;
        std::fs::write(asr.join("encoder-model.onnx"), b"legacy")?;
        assert_eq!(
            missing(directory.path()),
            vec![resources::MODEL_FILE, "silero_vad.onnx"]
        );
        std::fs::write(asr.join(resources::MODEL_FILE), b"corrupt GGUF")?;
        std::fs::write(&vad, b"corrupt VAD")?;
        assert!(missing(directory.path()).is_empty());
        // Presence makes status cheap; loading still rejects every corrupt resource.
        assert!(validate(directory.path()).is_err());
        assert_eq!(std::fs::read(asr.join("encoder-model.onnx"))?, b"legacy");
        Ok(())
    }
    /// A corrupt transfer cannot replace a previously verified file.
    #[test]
    fn checksum_gates_atomic_promotion() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source");
        let target = dir.path().join("models/weight");
        std::fs::write(&source, b"abc")?;
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        promote(&source, &target, hash).map_err(anyhow::Error::msg)?;
        std::fs::write(&source, b"partial")?;
        assert!(promote(&source, &target, hash).is_err());
        assert_eq!(std::fs::read(&target)?, b"abc");
        assert_eq!(missing(&dir.path().join("models")).len(), 2);
        Ok(())
    }
    /// Simulates failed and successful transfers, ensuring verified files never trigger network work.
    #[test]
    fn simulated_transfers_preserve_valid_resources() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("transfer");
        let target = directory.path().join("weight");
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(ensure_file(&target, hash, || Err("Offline".into())).is_err());
        assert!(!target.exists());
        std::fs::write(&source, b"partial")?;
        assert!(ensure_file(&target, hash, || Ok(source.clone())).is_err());
        assert!(!target.exists());
        std::fs::write(&source, b"abc")?;
        ensure_file(&target, hash, || Ok(source.clone())).map_err(anyhow::Error::msg)?;
        ensure_file(&target, hash, || Err("Must not transfer".into()))
            .map_err(anyhow::Error::msg)?;
        assert_eq!(std::fs::read(target)?, b"abc");
        Ok(())
    }
    /// Downloads only the small pinned-checksum VAD asset; ASR resources are reused locally.
    #[test]
    #[ignore = "explicit network test requiring local GGUF AURORA_ASR_MODEL_DIR"]
    fn explicit_hugging_face_download() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let storage = ModelStorage::new(directory.path());
        let base = storage.prepare().map_err(anyhow::Error::msg)?;
        let source = super::super::runtime::absolute_env("AURORA_ASR_MODEL_DIR")?;
        let (asr, _) = paths(&base);
        std::fs::create_dir_all(&asr)?;
        for (name, _) in resources::FILES {
            std::fs::hard_link(source.join(name), asr.join(name))?;
        }
        let app = tauri::test::mock_app();
        app.manage(storage);
        app.manage(super::super::EngineState::default());
        download(app.handle()).map_err(anyhow::Error::msg)?;
        validate(&base)?;
        assert!(missing(&base).is_empty());
        Ok(())
    }
}
