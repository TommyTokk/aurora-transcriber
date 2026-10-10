use super::*;

/// Verifies first launch, restart persistence, cancellation, reset, and file preservation.
#[test]
fn destination_lifecycle_preserves_files() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let custom = tempfile::tempdir()?;
    let storage = ModelStorage::new(profile.path());
    let default = storage.prepare().map_err(anyhow::Error::msg)?;
    assert_eq!(default, profile.path().join("models"));
    assert!(storage.snapshot().map_err(anyhow::Error::msg)?.is_default);
    std::fs::write(default.join("keep"), b"old models")?;
    std::fs::write(custom.path().join("keep"), b"new models")?;
    storage
        .activate(Some(custom.path().into()))
        .map_err(anyhow::Error::msg)?;
    let restarted = ModelStorage::new(profile.path());
    assert_eq!(
        restarted.prepare().map_err(anyhow::Error::msg)?,
        custom.path()
    );
    let before = std::fs::read(&storage.configuration)?;
    // The same asynchronous picker completion path is exercised with cancellation.
    let app = tauri::test::mock_app();
    app.manage(restarted);
    app.manage(EngineState::default());
    app.state::<EngineState>()
        .reserve()
        .map_err(anyhow::Error::msg)?;
    tauri::async_runtime::block_on(finish_choice(app.handle().clone(), None));
    assert_eq!(std::fs::read(&storage.configuration)?, before);
    app.state::<EngineState>()
        .reserve()
        .map_err(anyhow::Error::msg)?;
    app.state::<EngineState>().release();
    storage.activate(None).map_err(anyhow::Error::msg)?;
    assert!(
        ModelStorage::new(profile.path())
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .is_default
    );
    assert_eq!(std::fs::read(default.join("keep"))?, b"old models");
    assert_eq!(std::fs::read(custom.path().join("keep"))?, b"new models");
    Ok(())
}
/// Invalid candidates cannot overwrite a working choice; missing custom paths never fall back.
#[test]
fn unavailable_and_invalid_destinations_are_recoverable() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let custom = tempfile::tempdir()?;
    let storage = ModelStorage::new(profile.path());
    storage
        .activate(Some(custom.path().into()))
        .map_err(anyhow::Error::msg)?;
    let before = std::fs::read(&storage.configuration)?;
    let unavailable = profile.path().join("missing");
    assert!(storage.activate(Some(unavailable)).is_err());
    assert!(storage.activate(Some(PathBuf::from("relative"))).is_err());
    assert_eq!(std::fs::read(&storage.configuration)?, before);
    let selected = custom.path().to_owned();
    custom.close()?;
    let restarted = ModelStorage::new(profile.path());
    assert!(restarted.prepare().is_err());
    assert_eq!(restarted.path().map_err(anyhow::Error::msg)?, selected);
    assert!(!selected.exists());
    assert!(!restarted.snapshot().map_err(anyhow::Error::msg)?.accessible);
    restarted.activate(None).map_err(anyhow::Error::msg)?;
    assert!(restarted.prepare().is_ok());
    std::fs::write(
        &storage.configuration,
        b"{\"version\":999,\"directory\":null}",
    )?;
    let invalid = ModelStorage::new(profile.path());
    assert!(invalid.prepare().is_err());
    invalid.activate(None).map_err(anyhow::Error::msg)?;
    Ok(())
}
/// An actual filesystem permission failure prevents configuration replacement.
#[cfg(unix)]
#[test]
fn unwritable_directory_is_rejected() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let profile = tempfile::tempdir()?;
    let candidate = tempfile::tempdir()?;
    let storage = ModelStorage::new(profile.path());
    storage.prepare().map_err(anyhow::Error::msg)?;
    std::fs::set_permissions(candidate.path(), std::fs::Permissions::from_mode(0o500))?;
    let result = storage.activate(Some(candidate.path().into()));
    std::fs::set_permissions(candidate.path(), std::fs::Permissions::from_mode(0o700))?;
    assert!(result.is_err());
    assert!(!storage.configuration.exists());
    Ok(())
}
/// Native switch commands use the same reservation as startup, download, and transcription.
#[test]
fn active_operations_reject_reset() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let app = tauri::test::mock_app();
    // Reservation itself is runtime-independent; the real command checks it before all mutations.
    let state = EngineState::default();
    state.reserve().map_err(anyhow::Error::msg)?;
    assert!(state.reserve().is_err());
    state.release();
    state.reserve().map_err(anyhow::Error::msg)?;
    let storage = ModelStorage::new(profile.path());
    storage.prepare().map_err(anyhow::Error::msg)?;
    app.manage(storage);
    app.manage(state);
    assert!(tauri::async_runtime::block_on(reset(app.handle().clone())).is_err());
    assert!(!profile.path().join("model-storage.json").exists());
    Ok(())
}
