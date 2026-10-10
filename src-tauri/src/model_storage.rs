//! Native model destination selection, atomic configuration, and recovery without migration.
use crate::engine::{fetcher, state::EngineState};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

const DIRECTORY_ERROR: &str =
    "Model folder is unavailable or not writable. Choose a folder or use default.";
/// Versioned native configuration. An absent selection resolves to the application default.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    version: u32,
    directory: Option<PathBuf>,
}
/// Keeps configuration errors recoverable without silently activating another destination.
struct Selection {
    directory: Option<PathBuf>,
    error: Option<String>,
}
/// Owns backend paths; only display snapshots expose a path to the WebView.
pub struct ModelStorage {
    default: PathBuf,
    configuration: PathBuf,
    selection: Mutex<Selection>,
}
/// Display-only directory state, separate from inference residency and progress.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStorageSnapshot {
    path: String,
    is_default: bool,
    accessible: bool,
    missing: Vec<String>,
    error: Option<String>,
}
impl ModelStorage {
    /// Reads the existing choice without creating or repairing an unavailable custom directory.
    pub fn new(base: &Path) -> Self {
        let configuration = base.join("model-storage.json");
        let result = (|| -> anyhow::Result<Option<PathBuf>> {
            if !configuration.exists() {
                return Ok(None);
            }
            let config: Configuration =
                serde_json::from_reader(std::fs::File::open(&configuration)?)?;
            anyhow::ensure!(config.version == 1, "Unsupported model storage version");
            anyhow::ensure!(
                config.directory.as_ref().is_none_or(|p| p.is_absolute()),
                "Invalid model folder"
            );
            Ok(config.directory)
        })();
        let selection = match result {
            Ok(directory) => Selection {
                directory,
                error: None,
            },
            Err(_) => Selection {
                directory: None,
                error: Some(
                    "Model folder configuration could not be read. Choose a folder or use default."
                        .into(),
                ),
            },
        };
        Self {
            default: base.join("models"),
            configuration,
            selection: Mutex::new(selection),
        }
    }
    /// Resolves the selected native path, preserving errors until explicit recovery.
    pub(crate) fn path(&self) -> Result<PathBuf, String> {
        let selection = self.selection.lock().map_err(|_| DIRECTORY_ERROR)?;
        if let Some(error) = &selection.error {
            return Err(error.clone());
        }
        Ok(selection
            .directory
            .as_ref()
            .unwrap_or(&self.default)
            .clone())
    }
    /// Creates only the default folder; a missing custom destination remains an error.
    pub(crate) fn prepare(&self) -> Result<PathBuf, String> {
        let path = self.path()?;
        let selection = self.selection.lock().map_err(|_| DIRECTORY_ERROR)?;
        if selection.directory.is_none() {
            std::fs::create_dir_all(&path).map_err(|_| DIRECTORY_ERROR)?;
        }
        check_writable(&path)?;
        Ok(path)
    }
    /// Checks a candidate before atomically publishing its configuration; never moves model files.
    fn activate(&self, directory: Option<PathBuf>) -> Result<(), String> {
        let path = directory.as_ref().unwrap_or(&self.default);
        if !path.is_absolute() {
            return Err(DIRECTORY_ERROR.into());
        }
        if directory.is_none() {
            std::fs::create_dir_all(path).map_err(|_| DIRECTORY_ERROR)?;
        }
        check_writable(path)?;
        let mut selection = self.selection.lock().map_err(|_| DIRECTORY_ERROR)?;
        let parent = self.configuration.parent().ok_or(DIRECTORY_ERROR)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|_| "Unable to save model folder configuration.")?;
        serde_json::to_writer(
            &mut temporary,
            &Configuration {
                version: 1,
                directory: directory.clone(),
            },
        )
        .map_err(|_| "Unable to save model folder configuration.")?;
        temporary
            .flush()
            .map_err(|_| "Unable to save model folder configuration.")?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| "Unable to save model folder configuration.")?;
        temporary
            .persist(&self.configuration)
            .map_err(|_| "Unable to save model folder configuration.")?;
        *selection = Selection {
            directory,
            error: None,
        };
        Ok(())
    }
    /// Reports accessibility and file presence without hashing GB weights on status reads.
    fn snapshot(&self) -> Result<ModelStorageSnapshot, String> {
        let selection = self.selection.lock().map_err(|_| DIRECTORY_ERROR)?;
        let path = selection.directory.as_ref().unwrap_or(&self.default);
        let error = selection
            .error
            .clone()
            .or_else(|| check_writable(path).err());
        Ok(ModelStorageSnapshot {
            path: path.display().to_string(),
            is_default: selection.directory.is_none() && selection.error.is_none(),
            accessible: error.is_none(),
            missing: fetcher::missing(path),
            error,
        })
    }
}
/// Probes real directory access using a private temporary file that is immediately removed.
fn check_writable(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(DIRECTORY_ERROR.into());
    }
    let probe = tempfile::NamedTempFile::new_in(path).map_err(|_| DIRECTORY_ERROR)?;
    probe.as_file().sync_all().map_err(|_| DIRECTORY_ERROR)?;
    probe.close().map_err(|_| DIRECTORY_ERROR)?;
    std::fs::read_dir(path).map_err(|_| DIRECTORY_ERROR)?;
    Ok(())
}
/// Emits fresh directory status after selection, cancellation, or provisioning.
pub(crate) fn publish<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Ok(snapshot) = app.state::<ModelStorage>().snapshot() {
        let _ = app.emit_to("main", "model-storage-state", snapshot);
    }
}
/// Executes a reserved destination switch and drops old sessions only after configuration commits.
pub(crate) fn switch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    directory: Option<PathBuf>,
) -> Result<(), String> {
    app.state::<ModelStorage>().activate(directory)?;
    app.state::<EngineState>().reload_destination(app)?;
    Ok(())
}
/// Finishes a native folder choice on a worker; cancellation preserves configuration and sessions.
async fn finish_choice<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    selection: Option<Result<PathBuf, String>>,
) {
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        if let Some(path) = selection {
            switch(&worker, Some(path?))?;
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "Model folder worker failed.".to_string())
    .and_then(|r| r);
    app.state::<EngineState>().release();
    publish(&app);
    if let Err(error) = result {
        let _ = app.emit_to("main", "model-storage-error", error);
    }
}
/// Returns display state without accepting a path argument.
#[tauri::command]
pub async fn model_storage_state(app: tauri::AppHandle) -> Result<ModelStorageSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<ModelStorage>().snapshot())
        .await
        .map_err(|_| DIRECTORY_ERROR.to_string())?
}
/// Reserves model operations throughout the native picker and destination activation.
#[tauri::command]
pub fn choose_model_directory(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<EngineState>().reserve()?;
    app.dialog()
        .file()
        .set_title("Choose model folder")
        .pick_folder(move |selected| {
            let selection =
                selected.map(|file| file.into_path().map_err(|_| DIRECTORY_ERROR.to_string()));
            tauri::async_runtime::spawn(finish_choice(app, selection));
        });
    Ok(())
}
/// Atomically resets the selection to the application default without touching previous models.
#[tauri::command]
pub async fn reset_model_directory(app: tauri::AppHandle) -> Result<(), String> {
    reset(app).await
}
/// Shares native reservation and reset behavior with isolated runtime tests.
async fn reset<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    app.state::<EngineState>().reserve()?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || switch(&worker, None))
        .await
        .map_err(|_| "Model folder worker failed.".to_string())
        .and_then(|r| r);
    app.state::<EngineState>().release();
    publish(&app);
    result
}
/// Opens only the backend-owned current destination through the native opener plugin.
#[tauri::command]
pub async fn open_model_directory(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = app.state::<ModelStorage>().prepare()?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|_| "Unable to open model folder.".into())
    })
    .await
    .map_err(|_| "Model folder worker failed.".to_string())?
}

#[cfg(test)]
mod tests;
