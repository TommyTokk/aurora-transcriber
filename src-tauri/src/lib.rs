pub mod audio;
pub mod engine;
pub mod export;
mod import;
mod model_storage;
mod playback;
mod resources;
pub mod settings;
mod storage;
pub mod transcription;
pub mod vad;

/// Starts Tauri with native file plugins, returning any application startup error.
///
/// Audio import uses native selections and a blocking worker for decoding.
/// ASR and VAD load on workers, release when idle, and reload offline on demand.
pub fn run() -> anyhow::Result<()> {
    // Native libraries must be silenced before the startup worker loads any models.
    engine::initialize_native();
    tauri::Builder::default()
        .manage(import::ImportState::default())
        .manage(playback::PlaybackState::default())
        .manage(engine::EngineState::desktop())
        .manage(transcription::TranscriptionState::default())
        .setup(|app| {
            use tauri::Manager;
            let storage = storage::Storage::new(&app.path().app_data_dir()?)?;
            app.manage(storage);
            app.manage(model_storage::ModelStorage::new(
                &app.path().app_data_dir()?,
            ));
            app.manage(settings::TranscriptionSettings::new(
                &app.path().app_data_dir()?,
            ));
            engine::state_start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import::choose_audio_file,
            import::audio_import_state,
            import::clear_audio_file,
            engine::state::transcriber_state,
            engine::state::transcriber_devices,
            engine::state::select_asr_device,
            engine::state::download_models,
            engine::management::select_asr_model,
            engine::management::download_model,
            engine::management::delete_model,
            model_storage::model_storage_state,
            model_storage::choose_model_directory,
            model_storage::reset_model_directory,
            model_storage::open_model_directory,
            settings::transcription_settings,
            settings::set_transcription_language,
            transcription::process_audio_file,
            transcription::cancel_transcription,
            transcription::transcription_state,
            playback::associate_transcript_audio,
            playback::load_transcript_block,
            playback::release_transcript_audio,
            storage::save_transcript,
            storage::list_transcripts,
            storage::open_transcript,
            export::export_subtitles
        ])
        // Tauri routes drops on the main window's content through window events.
        .on_window_event(import::handle_drag_drop)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                use tauri::Manager;
                app.state::<engine::EngineState>().shutdown();
            }
        });
    Ok(())
}
