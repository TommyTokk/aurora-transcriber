use super::*;

/// Missing model selection persists without network access and publishes only its own requirements.
#[test]
fn selection_survives_missing_weights_and_restart() -> anyhow::Result<()> {
    let profile = tempfile::tempdir()?;
    let app = tauri::test::mock_app();
    app.manage(ModelStorage::new(profile.path()));
    app.manage(EngineState::default());
    app.manage(TranscriptionSettings::new(profile.path()));
    let result = tauri::async_runtime::block_on(perform(
        app.handle().clone(),
        ModelAction::Select(AsrModelId::WhisperTurbo),
    ))
    .map_err(anyhow::Error::msg)?;
    assert_eq!(result.selected_model, AsrModelId::WhisperTurbo);
    assert_eq!(result.phase, "missing");
    assert_eq!(
        result.missing,
        vec![manifest::WHISPER_TURBO.filename, manifest::SILERO.filename]
    );
    assert!(!result
        .missing
        .contains(&manifest::PARAKEET.filename.to_string()));
    assert_eq!(
        TranscriptionSettings::new(profile.path())
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .selected_model,
        AsrModelId::WhisperTurbo
    );
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
