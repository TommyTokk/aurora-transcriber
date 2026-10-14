use super::*;

/// Restores every supported choice after reopening without persisting an implicit default.
#[test]
fn preferences_survive_reopening() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let settings = TranscriptionSettings::new(directory.path());
    assert_eq!(
        settings.snapshot().map_err(anyhow::Error::msg)?.language,
        TranscriptionLanguage::Auto
    );
    assert!(!directory.path().join("settings.json").exists());
    for language in [
        TranscriptionLanguage::Italian,
        TranscriptionLanguage::English,
        TranscriptionLanguage::Auto,
    ] {
        settings.save(language).map_err(anyhow::Error::msg)?;
        let restored = TranscriptionSettings::new(directory.path())
            .snapshot()
            .map_err(anyhow::Error::msg)?;
        assert_eq!(restored.language, language);
        assert!(restored.warning.is_none());
    }
    Ok(())
}

/// Invalid files fall back visibly and remain untouched until an explicit successful save.
#[test]
fn invalid_configuration_is_recoverable() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("settings.json");
    for contents in [
        "{",
        r#"{"version":1,"language":"fr"}"#,
        r#"{"version":2,"language":"it"}"#,
        r#"{"version":1}"#,
    ] {
        std::fs::write(&path, contents)?;
        let settings = TranscriptionSettings::new(directory.path());
        let snapshot = settings.snapshot().map_err(anyhow::Error::msg)?;
        assert_eq!(snapshot.language, TranscriptionLanguage::Auto);
        assert!(snapshot.warning.is_some());
        assert_eq!(std::fs::read_to_string(&path)?, contents);
        let saved = settings
            .save(TranscriptionLanguage::Italian)
            .map_err(anyhow::Error::msg)?;
        assert!(saved.warning.is_none());
        assert_eq!(
            TranscriptionSettings::new(directory.path())
                .snapshot()
                .map_err(anyhow::Error::msg)?
                .language,
            TranscriptionLanguage::Italian
        );
    }
    assert!(serde_json::from_str::<TranscriptionLanguage>("\"fr\"").is_err());
    Ok(())
}

/// Failed atomic replacements preserve the live preference, previous disk file, and temporary cleanup.
#[test]
fn failed_save_keeps_previous_preference() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut settings = TranscriptionSettings::new(directory.path());
    settings
        .save(TranscriptionLanguage::English)
        .map_err(anyhow::Error::msg)?;
    let original = std::fs::read(directory.path().join("settings.json"))?;
    let blocked = directory.path().join("blocked");
    std::fs::create_dir(&blocked)?;
    settings.configuration = blocked;
    assert!(settings.save(TranscriptionLanguage::Italian).is_err());
    assert_eq!(
        settings.snapshot().map_err(anyhow::Error::msg)?.language,
        TranscriptionLanguage::English
    );
    assert_eq!(
        std::fs::read(directory.path().join("settings.json"))?,
        original
    );
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 2);
    settings.configuration = directory.path().join("missing/settings.json");
    assert!(settings.save(TranscriptionLanguage::Auto).is_err());
    assert_eq!(
        settings.snapshot().map_err(anyhow::Error::msg)?.language,
        TranscriptionLanguage::English
    );
    Ok(())
}

/// Unreadable settings keep their warning when recovery cannot be saved.
#[test]
fn unreadable_configuration_preserves_warning_on_failed_save() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::create_dir(directory.path().join("settings.json"))?;
    let settings = TranscriptionSettings::new(directory.path());
    assert!(settings
        .snapshot()
        .map_err(anyhow::Error::msg)?
        .warning
        .is_some());
    assert!(settings.save(TranscriptionLanguage::English).is_err());
    assert!(settings
        .snapshot()
        .map_err(anyhow::Error::msg)?
        .warning
        .is_some());
    Ok(())
}

/// Shared reservation rejects language writes during jobs and is released after failed saves.
#[test]
fn language_updates_respect_job_reservation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let app = tauri::test::mock_app();
    app.manage(EngineState::default());
    app.manage(TranscriptionSettings::new(directory.path()));
    let engine = app.state::<EngineState>();
    engine.reserve().map_err(anyhow::Error::msg)?;
    let result = tauri::async_runtime::block_on(save_language(
        app.handle().clone(),
        TranscriptionLanguage::Italian,
    ));
    assert!(result.is_err());
    assert!(!directory.path().join("settings.json").exists());
    engine.release();
    let result = tauri::async_runtime::block_on(save_language(
        app.handle().clone(),
        TranscriptionLanguage::English,
    ))
    .map_err(anyhow::Error::msg)?;
    assert_eq!(result.language, TranscriptionLanguage::English);
    std::fs::remove_file(directory.path().join("settings.json"))?;
    std::fs::create_dir(directory.path().join("settings.json"))?;
    assert!(tauri::async_runtime::block_on(save_language(
        app.handle().clone(),
        TranscriptionLanguage::Italian
    ))
    .is_err());
    engine.reserve().map_err(anyhow::Error::msg)?;
    engine.release();
    assert_eq!(
        app.state::<TranscriptionSettings>()
            .snapshot()
            .map_err(anyhow::Error::msg)?
            .language,
        TranscriptionLanguage::English
    );
    Ok(())
}

/// File-only overrides, including Auto, take precedence without changing the persistent preference.
#[test]
fn file_language_overrides_do_not_modify_settings() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let settings = TranscriptionSettings::new(directory.path());
    settings
        .save(TranscriptionLanguage::Italian)
        .map_err(anyhow::Error::msg)?;
    let original = std::fs::read(directory.path().join("settings.json"))?;
    assert_eq!(
        settings.language_for(None).map_err(anyhow::Error::msg)?,
        TranscriptionLanguage::Italian
    );
    for language in [
        TranscriptionLanguage::Auto,
        TranscriptionLanguage::Italian,
        TranscriptionLanguage::English,
    ] {
        assert_eq!(
            settings
                .language_for(Some(language))
                .map_err(anyhow::Error::msg)?,
            language
        );
        assert_eq!(
            settings.snapshot().map_err(anyhow::Error::msg)?.language,
            TranscriptionLanguage::Italian
        );
        assert_eq!(
            std::fs::read(directory.path().join("settings.json"))?,
            original
        );
    }
    settings
        .save(TranscriptionLanguage::English)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        settings.language_for(None).map_err(anyhow::Error::msg)?,
        TranscriptionLanguage::English
    );
    assert!(serde_json::from_str::<Option<TranscriptionLanguage>>("\"fr\"").is_err());
    assert_eq!(
        serde_json::from_str::<Option<TranscriptionLanguage>>("null")?,
        None
    );
    Ok(())
}
