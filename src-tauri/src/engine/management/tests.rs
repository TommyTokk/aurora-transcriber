use super::*;

/// Missing ASR weights leave the saved choice and engine unchanged and release the reservation.
#[test]
fn rejects_missing_weights_without_changing_selection() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let app = tauri::test::mock_app();
    app.manage(ModelStorage::new(profile.path()));
    app.manage(EngineState::default());
    app.manage(TranscriptionSettings::new(profile.path()));
    app.state::<TranscriptionSettings>()
        .save_model(AsrModelId::Parakeet)
        .map_err(anyhow::Error::msg)?;
    // Finish startup discovery before asserting that a rejected user action changes nothing.
    app.state::<EngineState>()
        .reload_destination(app.handle())
        .map_err(anyhow::Error::msg)?;
    let before = app
        .state::<EngineState>()
        .snapshot()
        .map_err(anyhow::Error::msg)?;
    let result = tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Select(AsrModelId::WhisperTurbo),
    ));
    assert!(result.is_err_and(|message| message.contains("Download")));
    let state = app.state::<EngineState>();
    let after = state.snapshot().map_err(anyhow::Error::msg)?;
    assert_eq!(after.selected_model, before.selected_model);
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.revision, before.revision);
    state.reserve().map_err(anyhow::Error::msg)?;
    state.release();
    assert_eq!(
        TranscriptionSettings::new(profile.path())
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .selected_model,
        AsrModelId::Parakeet
    );
    assert!(!profile.path().join("models/.download-cache").exists());
    Ok(())
}

/// Deleting an inactive ASR blocks selection until its weights are restored, without requiring VAD.
#[test]
fn restored_weights_allow_selection_after_deletion() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let storage = ModelStorage::new(profile.path());
    let base = storage.prepare().map_err(anyhow::Error::msg)?;
    let target = manifest::WHISPER_TURBO.destination(&base);
    std::fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
    )?;
    std::fs::write(&target, b"dummy weights")?;
    let app = tauri::test::mock_app();
    app.manage(storage);
    app.manage(EngineState::default());
    app.manage(TranscriptionSettings::new(profile.path()));
    tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Delete(manifest::WHISPER_TURBO.descriptor.id.into()),
    ))
    .map_err(anyhow::Error::msg)?;
    assert!(tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Select(AsrModelId::WhisperTurbo),
    ))
    .is_err());

    // A directory at the expected filename must never count as installed weights.
    std::fs::create_dir(&target)?;
    assert!(tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Select(AsrModelId::WhisperTurbo),
    ))
    .is_err());
    std::fs::remove_dir(&target)?;
    std::fs::write(&target, b"dummy weights")?;
    let result = tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Select(AsrModelId::WhisperTurbo),
    ))
    .map_err(anyhow::Error::msg)?;
    assert_eq!(result.selected_model, AsrModelId::WhisperTurbo);
    // Missing VAD prevents inference; ASR selection itself remains valid and offline.
    assert_eq!(result.phase, "missing");
    assert!(result
        .installed
        .contains(&manifest::WHISPER_TURBO.descriptor.id.to_string()));
    assert!(!profile.path().join("models/.download-cache").exists());
    Ok(())
}

/// Catalog actions share the job reservation; forged identities never touch files or preferences.
#[test]
fn rejects_busy_and_unknown_actions_without_side_effects() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let app = tauri::test::mock_app();
    app.manage(ModelStorage::new(profile.path()));
    app.manage(EngineState::default());
    app.manage(TranscriptionSettings::new(profile.path()));
    let state = app.state::<EngineState>();
    state.reserve().map_err(anyhow::Error::msg)?;
    for action in [
        ModelAction::Select(AsrModelId::WhisperTurbo),
        ModelAction::Unload(AsrModelId::Parakeet),
        ModelAction::Download(manifest::SILERO.descriptor.id.into()),
        ModelAction::Delete(manifest::SILERO.descriptor.id.into()),
    ] {
        assert!(tauri::async_runtime::block_on(perform(app.handle().clone(), action)).is_err());
    }
    state.release();
    for action in [
        ModelAction::Delete("../outside".into()),
        ModelAction::Download("unknown".into()),
    ] {
        assert!(tauri::async_runtime::block_on(perform(app.handle().clone(), action)).is_err());
        state.reserve().map_err(anyhow::Error::msg)?;
        state.release();
    }
    assert!(!profile.path().join("settings.json").exists());
    assert!(!profile.path().join("models").exists());
    assert!(serde_json::from_str::<AsrModelId>("\"unknown\"").is_err());
    Ok(())
}

/// Manual unload preserves disk artifacts and settings, rejects stale IDs and is safe to repeat.
#[test]
fn manual_unload_preserves_catalog_and_rejects_stale_requests() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let storage = ModelStorage::new(profile.path());
    let base = storage.prepare().map_err(anyhow::Error::msg)?;
    let target = manifest::PARAKEET.destination(&base);
    std::fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
    )?;
    std::fs::write(&target, b"dummy weights")?;
    let settings = TranscriptionSettings::new(profile.path());
    settings
        .save_model(AsrModelId::Parakeet)
        .map_err(anyhow::Error::msg)?;
    let app = tauri::test::mock_app();
    app.manage(storage);
    app.manage(settings);
    app.manage(EngineState::default());
    let state = app.state::<EngineState>();
    state
        .refresh_catalog(app.handle())
        .map_err(anyhow::Error::msg)?;
    // Model inference is unnecessary here; the residency test separately proves immediate drops.
    state.publish(app.handle(), "ready", None, None, vec![], None, None);
    let before = state.snapshot().map_err(anyhow::Error::msg)?;
    assert!(tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Unload(AsrModelId::WhisperTurbo),
    ))
    .is_err());
    assert_eq!(
        state.snapshot().map_err(anyhow::Error::msg)?.revision,
        before.revision
    );
    let after = tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Unload(AsrModelId::Parakeet),
    ))
    .map_err(anyhow::Error::msg)?;
    assert_eq!(after.phase, "unloaded");
    assert!(after.revision > before.revision);
    assert_eq!(after.installed, before.installed);
    assert_eq!(after.selected_model, before.selected_model);
    assert!(after.device.is_none() && after.provider.is_none() && after.fallback_reason.is_none());
    assert_eq!(std::fs::read(&target)?, b"dummy weights");
    assert_eq!(
        TranscriptionSettings::new(profile.path())
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .selected_model,
        before.selected_model
    );
    let repeated = tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Unload(AsrModelId::Parakeet),
    ))
    .map_err(anyhow::Error::msg)?;
    assert_eq!(repeated.revision, after.revision);
    for phase in ["missing", "unavailable"] {
        state.publish(app.handle(), phase, None, None, vec![], None, None);
        assert!(tauri::async_runtime::block_on(perform(
            app.handle().clone(),
            ModelAction::Unload(AsrModelId::Parakeet),
        ))
        .is_err());
        assert_eq!(state.snapshot().map_err(anyhow::Error::msg)?.phase, phase);
        state.reserve().map_err(anyhow::Error::msg)?;
        state.release();
    }
    Ok(())
}

/// Removing active ASR or shared VAD preserves the selection and publishes fresh missing state.
#[test]
fn deletion_tracks_presence_without_switching_selection() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let storage = ModelStorage::new(profile.path());
    let base = storage.prepare().map_err(anyhow::Error::msg)?;
    for model in manifest::MODELS {
        let target = model.destination(&base);
        std::fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
        )?;
        std::fs::write(target, b"dummy weights")?;
    }
    let app = tauri::test::mock_app();
    app.manage(storage);
    app.manage(EngineState::default());
    app.manage(TranscriptionSettings::new(profile.path()));
    app.state::<EngineState>()
        .refresh_catalog(app.handle())
        .map_err(anyhow::Error::msg)?;
    for (id, missing) in [
        (manifest::WHISPER_TURBO.descriptor.id, vec![]),
        (
            manifest::PARAKEET.descriptor.id,
            vec![manifest::PARAKEET.filename],
        ),
        (
            manifest::SILERO.descriptor.id,
            vec![manifest::PARAKEET.filename, manifest::SILERO.filename],
        ),
    ] {
        let before = app
            .state::<EngineState>()
            .snapshot()
            .map_err(anyhow::Error::msg)?;
        let after = tauri::async_runtime::block_on(perform(
            app.handle().clone(),
            ModelAction::Delete(id.into()),
        ))
        .map_err(anyhow::Error::msg)?;
        assert!(after.revision > before.revision);
        assert_eq!(after.selected_model, AsrModelId::Parakeet);
        assert!(!after.installed.contains(&id.to_string()));
        assert_eq!(after.missing, missing);
        if !after.missing.is_empty() {
            assert_eq!(after.phase, "missing");
        }
    }
    Ok(())
}
