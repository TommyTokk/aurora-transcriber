use std::path::Path;

use super::{inspect_audio, ImportPhase, ImportUpdate};

/// Rejects overlapping requests and preserves metadata on picker cancellation.
#[test]
fn busy_imports_are_rejected_and_cancellation_preserves_selection() -> Result<(), String> {
    let mut update = ImportUpdate::default();
    update.begin(ImportPhase::Picking)?;
    assert!(update.begin(ImportPhase::Validating).is_err());
    update.cancel_selection();
    assert!(update.phase == ImportPhase::Idle);
    update.metadata = Some(super::AudioMetadata {
        name: "tone.wav".into(),
        ..Default::default()
    });
    update.phase = ImportPhase::Ready;
    update.begin(ImportPhase::Picking)?;
    update.cancel_selection();
    assert!(update.phase == ImportPhase::Ready);
    assert_eq!(
        update.metadata.as_ref().map(|file| file.name.as_str()),
        Some("tone.wav")
    );
    update.begin(ImportPhase::Validating)?;
    assert!(update.begin(ImportPhase::Picking).is_err());
    Ok(())
}

/// Checks metadata and decoded duration for every supported fixture format.
#[test]
fn supported_formats_return_display_metadata() -> Result<(), Box<dyn std::error::Error>> {
    for extension in ["wav", "mp3", "flac", "m4a", "ogg"] {
        let name = format!("tone.{extension}");
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(&name);
        let metadata = inspect_audio(&path)?;
        assert_eq!(metadata.name, name);
        assert_eq!(metadata.format, extension.to_ascii_uppercase());
        assert_eq!(metadata.size_bytes, path.metadata()?.len());
        assert!((metadata.duration_seconds - 0.25).abs() < 0.035);
        let payload = serde_json::to_string(&metadata)?;
        assert!(!payload.contains("fixtures"));
        assert!(!payload.contains("samples"));
        assert!(payload.contains("durationSeconds"));
    }
    Ok(())
}

/// Rejects directories, missing paths, unsupported extensions and invalid audio.
#[test]
fn invalid_files_return_safe_errors() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    assert_eq!(
        inspect_audio(directory.path()).err().as_deref(),
        Some("Choose an audio file, not a folder.")
    );
    let missing = directory.path().join("private-recording.wav");
    let message = inspect_audio(&missing)
        .err()
        .ok_or("Expected missing-file error")?;
    assert!(!message.contains("private-recording"));
    assert!(!message.contains(&directory.path().to_string_lossy().to_string()));
    for (name, content) in [
        ("empty.wav", &b""[..]),
        ("damaged.mp3", &b"invalid audio"[..]),
        ("notes.txt", &b"notes"[..]),
    ] {
        let path = directory.path().join(name);
        std::fs::write(&path, content)?;
        assert!(inspect_audio(&path).is_err());
    }
    Ok(())
}

/// Accepts uppercase extensions while preserving the displayed file name.
#[test]
fn uppercase_extensions_are_supported() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("VOICE.WAV");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.wav"),
        &path,
    )?;
    let metadata = inspect_audio(&path)?;
    assert_eq!(metadata.name, "VOICE.WAV");
    assert_eq!(metadata.format, "WAV");
    Ok(())
}

/// Keeps the native event contract aligned with frontend phase names.
#[test]
fn import_snapshots_serialize_without_paths() -> Result<(), serde_json::Error> {
    for (phase, name) in [
        (ImportPhase::Idle, "idle"),
        (ImportPhase::Picking, "picking"),
        (ImportPhase::Validating, "validating"),
        (ImportPhase::Ready, "ready"),
        (ImportPhase::Error, "error"),
    ] {
        let snapshot = ImportUpdate {
            phase,
            ..Default::default()
        };
        let value = serde_json::to_value(snapshot)?;
        assert_eq!(value["phase"], name);
        assert!(value["metadata"].is_null());
        assert!(value.get("path").is_none());
    }
    Ok(())
}

/// Verifies opaque selection binding, busy native import rejection, and path-free snapshots.
#[test]
fn selected_paths_stay_private_and_ids_are_bound() -> Result<(), String> {
    let app = tauri::test::mock_app();
    let id = uuid::Uuid::new_v4().to_string();
    let state = super::ImportState(std::sync::Mutex::new(ImportUpdate {
        phase: ImportPhase::Ready,
        audio_id: Some(id.clone()),
        path: Some(std::path::PathBuf::from("/private/native/voice.wav")),
        metadata: Some(super::AudioMetadata {
            name: "voice.wav".into(),
            ..Default::default()
        }),
        ..Default::default()
    }));
    assert!(state.acquire(app.handle(), "stale").is_err());
    let (path, name) = state.acquire(app.handle(), &id)?;
    assert_eq!(name, "voice.wav");
    assert_eq!(path, std::path::PathBuf::from("/private/native/voice.wav"));
    let mut current = state.0.lock().map_err(|_| "lock")?;
    assert!(current.begin(ImportPhase::Picking).is_err());
    let json = serde_json::to_string(&*current).map_err(|_| "serialization")?;
    assert!(!json.contains("/private/native"));
    drop(current);
    state.release(app.handle());
    state.acquire(app.handle(), &id)?;
    Ok(())
}
