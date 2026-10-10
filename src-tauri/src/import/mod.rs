//! Native-only audio import with transient decoding and path-free UI messages.

use std::{path::Path, sync::Mutex};

use serde::Serialize;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, Webview, WebviewEvent};
use tauri_plugin_dialog::DialogExt;

use crate::audio::{AudioDecoder, TARGET_SAMPLE_RATE};

const IMPORT_EVENT: &str = "audio-import";
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "m4a", "ogg"];

/// Metadata safe to display without exposing paths or decoded audio.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMetadata {
    name: String,
    format: String,
    size_bytes: u64,
    duration_seconds: f64,
}

/// Import phases shared by command responses and native drop events.
#[derive(Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ImportPhase {
    #[default]
    Idle,
    Picking,
    Validating,
    Ready,
    Error,
}

/// Snapshot used to recover state when the frontend subscribes or remounts.
#[derive(Clone, Default, Serialize)]
pub struct ImportUpdate {
    #[serde(rename = "audioId")]
    audio_id: Option<String>,
    #[serde(skip)]
    path: Option<std::path::PathBuf>,
    processing: bool,
    phase: ImportPhase,
    metadata: Option<AudioMetadata>,
    error: Option<String>,
}

impl ImportUpdate {
    /// Rejects overlapping imports while preserving metadata during selection.
    fn begin(&mut self, phase: ImportPhase) -> Result<(), String> {
        if self.processing || matches!(self.phase, ImportPhase::Picking | ImportPhase::Validating) {
            return Err("An audio import is already in progress.".into());
        }
        self.phase = phase;
        self.error = None;
        Ok(())
    }

    /// Restores the selected file or the empty workspace after picker cancellation.
    fn cancel_selection(&mut self) {
        self.phase = if self.metadata.is_some() {
            ImportPhase::Ready
        } else {
            ImportPhase::Idle
        };
    }
}

/// Serializes imports and retains the native selection privately for the current session.
#[derive(Default)]
pub struct ImportState(Mutex<ImportUpdate>);

impl ImportState {
    /// Reserves the exact native selection and rejects stale IDs or concurrent imports.
    pub(crate) fn acquire<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
    ) -> Result<(std::path::PathBuf, String), String> {
        let mut current = self.0.lock().map_err(|_| "Audio import is unavailable.")?;
        if current.processing
            || current.phase != ImportPhase::Ready
            || current.audio_id.as_deref() != Some(id)
        {
            return Err("This audio selection is unavailable or busy.".into());
        }
        let path = current
            .path
            .clone()
            .ok_or("Audio selection is unavailable.")?;
        let name = current
            .metadata
            .as_ref()
            .ok_or("Audio selection is unavailable.")?
            .name
            .clone();
        current.processing = true;
        let _ = app.emit_to("main", IMPORT_EVENT, &*current);
        Ok((path, name))
    }
    /// Unlocks native import only after the transcription worker has finished.
    pub(crate) fn release<R: tauri::Runtime>(&self, app: &AppHandle<R>) {
        if let Ok(mut current) = self.0.lock() {
            current.processing = false;
            let _ = app.emit_to("main", IMPORT_EVENT, &*current);
        }
    }

    /// Reserves the importer so dialogs and native drops cannot overlap.
    fn begin(&self, app: &AppHandle, phase: ImportPhase) -> Result<(), String> {
        let mut current = self.0.lock().map_err(|_| "Audio import is unavailable.")?;
        current.begin(phase)?;
        let _ = app.emit_to("main", IMPORT_EVENT, &*current);
        Ok(())
    }

    /// Stores and emits an ordered snapshot without logging sensitive data.
    fn publish(&self, app: &AppHandle, update: ImportUpdate) {
        if let Ok(mut current) = self.0.lock() {
            *current = update;
            let _ = app.emit_to("main", IMPORT_EVENT, &*current);
        }
    }
}

/// Validates file contents locally and drops decoded samples before returning.
fn inspect_audio(path: &Path) -> Result<AudioMetadata, String> {
    let properties = path
        .metadata()
        .map_err(|_| "Unable to read this audio file.")?;
    if !properties.is_file() {
        return Err("Choose an audio file, not a folder.".into());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err("Supported formats are WAV, MP3, FLAC, M4A, and OGG.".into());
    }
    // Decoder errors may contain OS paths; expose only a safe user-facing message.
    let samples = AudioDecoder::decode_to_16khz_mono(path).map_err(|_| {
        "This audio file is empty, damaged, or contains an unsupported audio stream."
    })?;
    let duration_seconds = samples.len() as f64 / TARGET_SAMPLE_RATE as f64;
    drop(samples);
    Ok(AudioMetadata {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        format: extension.to_ascii_uppercase(),
        size_bytes: properties.len(),
        duration_seconds,
    })
}

/// Runs synchronous decoding away from the UI thread and reports its outcome.
async fn validate(app: AppHandle, path: std::path::PathBuf) {
    app.state::<ImportState>().publish(
        &app,
        ImportUpdate {
            phase: ImportPhase::Validating,
            ..Default::default()
        },
    );
    let result = tauri::async_runtime::spawn_blocking(move || {
        inspect_audio(&path).map(|metadata| (metadata, path))
    })
    .await;
    let update = match result {
        Ok(Ok((metadata, path))) => ImportUpdate {
            audio_id: Some(uuid::Uuid::new_v4().to_string()),
            path: Some(path),
            processing: false,
            phase: ImportPhase::Ready,
            metadata: Some(metadata),
            error: None,
        },
        Ok(Err(error)) => ImportUpdate {
            phase: ImportPhase::Error,
            metadata: None,
            error: Some(error),
            ..Default::default()
        },
        Err(_) => ImportUpdate {
            phase: ImportPhase::Error,
            metadata: None,
            error: Some("Audio validation could not finish. Please try again.".into()),
            ..Default::default()
        },
    };
    app.state::<ImportState>().publish(&app, update);
}

/// Opens the native picker; no filesystem path is accepted from the WebView.
#[tauri::command]
pub fn choose_audio_file(app: AppHandle, state: State<'_, ImportState>) -> Result<(), String> {
    state.begin(&app, ImportPhase::Picking)?;
    app.dialog()
        .file()
        .set_title("Choose audio file")
        .add_filter("Audio files", AUDIO_EXTENSIONS)
        .pick_file(move |selection| {
            match selection {
                Some(file) => match file.into_path() {
                    Ok(path) => {
                        tauri::async_runtime::spawn(validate(app, path));
                    }
                    Err(_) => app.state::<ImportState>().publish(
                        &app,
                        ImportUpdate {
                            phase: ImportPhase::Error,
                            metadata: None,
                            error: Some("Unable to access this audio file.".into()),
                            ..Default::default()
                        },
                    ),
                },
                None => {
                    // Cancelling preserves the previously selected file.
                    if let Ok(mut current) = app.state::<ImportState>().0.lock() {
                        current.cancel_selection();
                        let _ = app.emit_to("main", IMPORT_EVENT, &*current);
                    }
                }
            }
        });
    Ok(())
}

/// Returns the current path-free import state for frontend synchronization.
#[tauri::command]
pub fn audio_import_state(state: State<'_, ImportState>) -> Result<ImportUpdate, String> {
    state
        .0
        .lock()
        .map(|current| current.clone())
        .map_err(|_| "Audio import is unavailable.".into())
}

/// Removes display metadata when no dialog or validation is in progress.
#[tauri::command]
pub fn clear_audio_file(app: AppHandle, state: State<'_, ImportState>) -> Result<(), String> {
    let mut current = state.0.lock().map_err(|_| "Audio import is unavailable.")?;
    if current.processing
        || matches!(
            current.phase,
            ImportPhase::Picking | ImportPhase::Validating
        )
    {
        return Err("An audio import is already in progress.".into());
    }
    // Clear under the same lock to prevent a concurrent native drop being lost.
    *current = ImportUpdate::default();
    let _ = app.emit_to("main", IMPORT_EVENT, &*current);
    Ok(())
}

/// Imports only OS-provided drop paths and reports drag feedback separately.
pub fn handle_drag_drop(webview: &Webview, event: &WebviewEvent) {
    if webview.label() != "main" {
        return;
    }
    let WebviewEvent::DragDrop(event) = event else {
        return;
    };
    let app = webview.app_handle();
    match event {
        DragDropEvent::Enter { .. } => {
            let _ = app.emit_to("main", "audio-drag", true);
        }
        DragDropEvent::Leave => {
            let _ = app.emit_to("main", "audio-drag", false);
        }
        DragDropEvent::Drop { paths, .. } => {
            let _ = app.emit_to("main", "audio-drag", false);
            let state = app.state::<ImportState>();
            if state.begin(app, ImportPhase::Validating).is_err() {
                return;
            }
            if paths.len() != 1 {
                state.publish(
                    app,
                    ImportUpdate {
                        phase: ImportPhase::Error,
                        metadata: None,
                        error: Some("Drop one audio file at a time.".into()),
                        ..Default::default()
                    },
                );
            } else if let Some(path) = paths.first() {
                tauri::async_runtime::spawn(validate(app.clone(), path.clone()));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
