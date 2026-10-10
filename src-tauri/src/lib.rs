pub mod audio;
mod import;

/// Starts Tauri with native file plugins, returning any application startup error.
///
/// Audio import uses native selections and a blocking worker for decoding.
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .manage(import::ImportState::default())
        .invoke_handler(tauri::generate_handler![
            import::choose_audio_file,
            import::audio_import_state,
            import::clear_audio_file
        ])
        .on_webview_event(import::handle_drag_drop)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .run(tauri::generate_context!())?;
    Ok(())
}
