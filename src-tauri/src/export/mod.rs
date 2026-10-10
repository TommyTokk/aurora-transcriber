//! Explicit native export of edited text and chunk-level subtitle timestamps.
mod subtitle;
use crate::{storage::Storage, transcription::SubtitleSegment};
pub use subtitle::{format_segments, validate_segments, ExportFormat};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// Opens a native save dialog and writes only the explicitly exported current text.
#[tauri::command]
pub async fn export_subtitles(
    app: tauri::AppHandle,
    segments: Vec<SubtitleSegment>,
    format: ExportFormat,
) -> Result<bool, String> {
    let content = format_segments(&segments, format)?;
    let extension = format.extension();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export transcript")
        .set_directory(&app.state::<Storage>().transcripts)
        .set_file_name(format!("transcript.{extension}"))
        .add_filter(extension.to_ascii_uppercase(), &[extension])
        .save_file(move |file| {
            let _ = sender.send(file);
        });
    let Some(file) = receiver.await.map_err(|_| "Save dialog is unavailable.")? else {
        return Ok(false);
    };
    // The destination originates from the native dialog, never from WebView input.
    let path = file
        .into_path()
        .map_err(|_| "Unable to access export destination.")?;
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(path, content).map_err(|_| "Unable to export transcript.".to_string())
    })
    .await
    .map_err(|_| "Unable to export transcript.".to_string())??;
    Ok(true)
}
