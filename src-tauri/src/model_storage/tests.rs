use super::*;

/// Switching to a missing installation publishes GGUF requirements without deleting old files.
#[test]
fn destination_switch_reports_new_requirements() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let custom = tempfile::tempdir()?;
    std::fs::write(custom.path().join("legacy.onnx"), b"keep")?;
    let app = tauri::test::mock_app();
    app.manage(ModelStorage::new(profile.path()));
    app.manage(EngineState::default());
    app.state::<EngineState>()
        .reserve()
        .map_err(anyhow::Error::msg)?;
    switch(app.handle(), Some(custom.path().into())).map_err(anyhow::Error::msg)?;
    app.state::<EngineState>().release();
    let snapshot = app
        .state::<EngineState>()
        .snapshot()
        .map_err(anyhow::Error::msg)?;
    assert_eq!(snapshot.phase, "missing");
    assert_eq!(
        snapshot.missing,
        vec![crate::engine::resources::MODEL_FILE, "silero_vad.onnx"]
    );
    assert_eq!(std::fs::read(custom.path().join("legacy.onnx"))?, b"keep");
    assert_eq!(
        app.state::<ModelStorage>()
            .path()
            .map_err(anyhow::Error::msg)?,
        custom.path()
    );
    app.state::<EngineState>().shutdown();
    Ok(())
}

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

/// Explicitly provisions a clean temporary profile and loads both runtimes without audio inference.
///
/// Requires network access, a real Metal device and the trusted ORT_DYLIB_PATH.
/// All initialization, native teardown and model hashing run on a blocking worker.
#[test]
#[ignore = "explicit full GGUF/Silero download, Metal loading and offline custom-folder reload"]
fn milestone10_clean_install_and_custom_reload() -> anyhow::Result<()> {
    tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking(
        || -> anyhow::Result<()> {
            let profile = tempfile::tempdir()?;
            let custom = tempfile::tempdir()?;
            let app = tauri::test::mock_app();
            app.manage(ModelStorage::new(profile.path()));
            app.manage(EngineState::default());
            let state = app.state::<EngineState>();
            state.reserve().map_err(anyhow::Error::msg)?;
            let result = (|| -> anyhow::Result<()> {
                let base = app
                    .state::<ModelStorage>()
                    .prepare()
                    .map_err(anyhow::Error::msg)?;
                assert_eq!(fetcher::missing(&base).len(), 2);
                fetcher::download(app.handle()).map_err(anyhow::Error::msg)?;
                fetcher::validate(&base)?;
                assert!(fetcher::missing(&base).is_empty());
                state
                    .with_sessions(app.handle(), |sessions| {
                        assert_eq!(sessions.asr.provider(), crate::engine::Provider::Metal);
                        Ok(())
                    })
                    .map_err(anyhow::Error::msg)?;
                assert_eq!(state.snapshot().map_err(anyhow::Error::msg)?.phase, "ready");
                state.release();
                // Advance the monotonic deadline without waiting five minutes or running ASR.
                state.unload_idle_at(
                    app.handle(),
                    std::time::Instant::now() + std::time::Duration::from_secs(301),
                );
                assert_eq!(
                    state.snapshot().map_err(anyhow::Error::msg)?.phase,
                    "unloaded"
                );
                state.reserve().map_err(anyhow::Error::msg)?;
                state
                    .with_sessions(app.handle(), |_| Ok(()))
                    .map_err(anyhow::Error::msg)?;
                assert_eq!(state.snapshot().map_err(anyhow::Error::msg)?.phase, "ready");
                // Hard links preserve verified bytes while exercising a distinct native directory.
                for model in crate::engine::manifest::MODELS {
                    let target = model.destination(custom.path());
                    std::fs::create_dir_all(
                        target
                            .parent()
                            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
                    )?;
                    std::fs::hard_link(model.destination(&base), target)?;
                }
                let legacy = custom.path().join("parakeet/encoder-model.onnx");
                std::fs::write(&legacy, b"preserved legacy model")?;
                app.state::<ModelStorage>()
                    .activate(Some(custom.path().into()))
                    .map_err(anyhow::Error::msg)?;
                state
                    .reload_destination(app.handle())
                    .map_err(anyhow::Error::msg)?;
                assert_eq!(state.snapshot().map_err(anyhow::Error::msg)?.phase, "ready");
                assert_eq!(std::fs::read(&legacy)?, b"preserved legacy model");
                let restarted = tauri::test::mock_app();
                restarted.manage(ModelStorage::new(profile.path()));
                restarted.manage(EngineState::default());
                assert_eq!(
                    restarted
                        .state::<ModelStorage>()
                        .path()
                        .map_err(anyhow::Error::msg)?,
                    custom.path()
                );
                state.shutdown();
                let next = restarted.state::<EngineState>();
                next.reserve().map_err(anyhow::Error::msg)?;
                let reload = next.with_sessions(restarted.handle(), |sessions| {
                    assert_eq!(sessions.asr.provider(), crate::engine::Provider::Metal);
                    Ok(())
                });
                next.release();
                next.shutdown();
                reload.map_err(anyhow::Error::msg)?;
                assert_eq!(next.snapshot().map_err(anyhow::Error::msg)?.phase, "ready");
                fetcher::validate(&base)?;
                fetcher::validate(custom.path())?;
                Ok(())
            })();
            // Mock applications do not emit Exit; always release native assets before temp folders.
            state.release();
            state.shutdown();
            result?;
            println!("Milestone 10: full clean download, Metal ASR/CPU VAD loading, idle reload and persisted custom-folder reload passed; no audio inference.");
            Ok(())
        },
    ))??;
    Ok(())
}
