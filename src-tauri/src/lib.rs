pub mod audio;
pub mod engine;
mod import;
pub mod vad;

/// Starts Tauri with native file plugins, returning any application startup error.
///
/// Audio import uses native selections and a blocking worker for decoding.
/// ASR initializes once on its own worker; failure leaves native import available.
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .manage(import::ImportState::default())
        .manage(engine::EngineState::default())
        .setup(|app| {
            engine::state_start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import::choose_audio_file,
            import::audio_import_state,
            import::clear_audio_file,
            engine::state::transcriber_state
        ])
        .on_webview_event(import::handle_drag_drop)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .run(tauri::generate_context!())?;
    Ok(())
}
