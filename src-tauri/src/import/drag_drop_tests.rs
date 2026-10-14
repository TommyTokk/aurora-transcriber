//! Regression coverage for native window events and frontend-safe import updates.

use std::{path::PathBuf, sync::mpsc, time::Duration};

use tauri::{test::MockRuntime, DragDropEvent, Listener, Manager, Window, WindowEvent};

use super::{handle_drag_drop, ImportPhase, ImportState, ImportUpdate, IMPORT_EVENT};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Creates the main window with the same window-event callback used in production.
fn import_app() -> Result<(tauri::App<MockRuntime>, Window<MockRuntime>), tauri::Error> {
    let app = tauri::test::mock_builder()
        .manage(ImportState::default())
        .on_window_event(handle_drag_drop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build()?;
    let window = webview.as_ref().window();
    Ok((app, window))
}

/// Copies the current import state without holding its lock across worker completion.
fn snapshot(app: &tauri::App<MockRuntime>) -> Result<ImportUpdate, String> {
    app.state::<ImportState>()
        .0
        .lock()
        .map(|current| current.clone())
        .map_err(|_| "Audio import is unavailable.".into())
}

/// Builds a drop event using paths supplied by the native event, never by IPC.
fn drop_event(paths: Vec<PathBuf>) -> WindowEvent {
    WindowEvent::DragDrop(DragDropEvent::Drop {
        paths,
        position: tauri::PhysicalPosition::new(20.0, 30.0),
    })
}

/// Waits for a terminal import event with a deadline instead of relying on sleeps.
fn wait_for_completion(
    events: &mpsc::Receiver<String>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let payload =
            events.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))?;
        let update: serde_json::Value = serde_json::from_str(&payload)?;
        if update["phase"] == "ready" || update["phase"] == "error" {
            return Ok(update);
        }
    }
}

/// Verifies drag feedback and rejects unrelated events and secondary-window imports.
#[test]
fn window_events_emit_drag_feedback_only_for_main() -> TestResult {
    let (app, window) = import_app()?;
    let (sender, events) = mpsc::channel();
    app.listen_any("audio-drag", move |event| {
        let _ = sender.send(event.payload().to_owned());
    });
    let enter = WindowEvent::DragDrop(DragDropEvent::Enter {
        paths: vec![PathBuf::from("voice.wav")],
        position: tauri::PhysicalPosition::new(20.0, 30.0),
    });
    handle_drag_drop(&window, &enter);
    assert_eq!(events.recv_timeout(Duration::from_secs(1))?, "true");
    handle_drag_drop(&window, &WindowEvent::DragDrop(DragDropEvent::Leave));
    assert_eq!(events.recv_timeout(Duration::from_secs(1))?, "false");

    let secondary =
        tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build()?;
    let secondary_window = secondary.as_ref().window();
    handle_drag_drop(&secondary_window, &enter);
    handle_drag_drop(
        &secondary_window,
        &drop_event(vec![PathBuf::from("voice.wav")]),
    );
    handle_drag_drop(&window, &WindowEvent::Focused(true));
    handle_drag_drop(
        &window,
        &WindowEvent::DragDrop(DragDropEvent::Over {
            position: tauri::PhysicalPosition::new(40.0, 50.0),
        }),
    );
    assert!(events.try_recv().is_err());
    assert!(snapshot(&app)?.phase == ImportPhase::Idle);
    Ok(())
}

/// Imports each supported format through the window callback and binds the transcription ID.
#[test]
fn native_drops_publish_ready_metadata_and_transcription_selection() -> TestResult {
    let (app, window) = import_app()?;
    let (sender, events) = mpsc::channel();
    app.listen_any(IMPORT_EVENT, move |event| {
        let _ = sender.send(event.payload().to_owned());
    });
    for extension in ["wav", "mp3", "flac", "m4a", "ogg"] {
        let name = format!("tone.{extension}");
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(&name);
        handle_drag_drop(&window, &drop_event(vec![path.clone()]));
        let payload = wait_for_completion(&events)?;
        assert_eq!(payload["phase"], "ready");
        assert_eq!(payload["metadata"]["name"], name);
        assert_eq!(
            payload["metadata"]["format"],
            extension.to_ascii_uppercase()
        );
        assert_eq!(payload["metadata"]["sizeBytes"], path.metadata()?.len());
        assert!(!payload["processing"]
            .as_bool()
            .ok_or("Missing processing flag")?);
        assert!(payload.get("path").is_none());
        let duration = payload["metadata"]["durationSeconds"]
            .as_f64()
            .ok_or("Missing duration")?;
        assert!((duration - 0.25).abs() < 0.035);
        let id = payload["audioId"].as_str().ok_or("Missing audio ID")?;
        uuid::Uuid::parse_str(id)?;
        assert!(snapshot(&app)?.phase == ImportPhase::Ready);
        // Exercise the same reservation used when the frontend starts transcription.
        let state = app.state::<ImportState>();
        assert_eq!(state.acquire(app.handle(), id)?, (path, name));
        state.release(app.handle());
        // Discard the release snapshot before checking the next import's completion.
        while events.try_recv().is_ok() {}
    }
    Ok(())
}

/// Rejects empty and multiple-file drops while clearing the drag highlight.
#[test]
fn invalid_drop_counts_publish_safe_errors() -> TestResult {
    let (app, window) = import_app()?;
    let (sender, events) = mpsc::channel();
    app.listen_any("audio-drag", move |event| {
        let _ = sender.send(event.payload().to_owned());
    });
    for paths in [vec![], vec![PathBuf::from("a.wav"), PathBuf::from("b.wav")]] {
        handle_drag_drop(&window, &drop_event(paths));
        assert_eq!(events.recv_timeout(Duration::from_secs(1))?, "false");
        let update = snapshot(&app)?;
        assert!(update.phase == ImportPhase::Error);
        assert_eq!(
            update.error.as_deref(),
            Some("Drop one audio file at a time.")
        );
        assert!(update.audio_id.is_none());
        assert!(update.metadata.is_none());
    }
    Ok(())
}

/// Preserves the reserved selection when a drop occurs during picking, validation, or transcription.
#[test]
fn busy_window_drops_preserve_the_current_selection() -> TestResult {
    let (app, window) = import_app()?;
    for (phase, processing) in [
        (ImportPhase::Picking, false),
        (ImportPhase::Validating, false),
        (ImportPhase::Ready, true),
    ] {
        let reserved = ImportUpdate {
            phase,
            processing,
            audio_id: Some("reserved".into()),
            path: Some(PathBuf::from("selected.wav")),
            metadata: Some(super::AudioMetadata {
                name: "selected.wav".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        app.state::<ImportState>()
            .publish(app.handle(), reserved.clone());
        // An empty drop would publish an error if the synchronous reservation were bypassed.
        handle_drag_drop(&window, &drop_event(vec![]));
        let current = snapshot(&app)?;
        assert_eq!(
            serde_json::to_value(&current)?,
            serde_json::to_value(&reserved)?
        );
        assert_eq!(current.path, reserved.path);
    }
    Ok(())
}

/// Ensures dropped folders and malformed audio finish validation with safe UI errors.
#[test]
fn invalid_native_drops_finish_without_a_transcription_id() -> TestResult {
    let (app, window) = import_app()?;
    let (sender, events) = mpsc::channel();
    app.listen_any(IMPORT_EVENT, move |event| {
        let _ = sender.send(event.payload().to_owned());
    });
    let directory = tempfile::tempdir()?;
    let damaged = directory.path().join("private-recording.wav");
    std::fs::write(&damaged, b"invalid audio")?;
    for path in [directory.path().to_path_buf(), damaged] {
        handle_drag_drop(&window, &drop_event(vec![path]));
        let payload = wait_for_completion(&events)?;
        assert_eq!(payload["phase"], "error");
        assert!(payload["audioId"].is_null());
        assert!(payload["metadata"].is_null());
        assert!(!payload.to_string().contains("private-recording"));
        assert!(!payload
            .to_string()
            .contains(&directory.path().to_string_lossy().to_string()));
    }
    Ok(())
}
