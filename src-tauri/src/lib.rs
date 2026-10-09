pub mod audio;

/// Starts Tauri with native file plugins, returning any application startup error.
///
/// Future IPC callers must run synchronous decoding on a blocking worker and
/// obtain file access through native file selection.
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .run(tauri::generate_context!())?;
    Ok(())
}
