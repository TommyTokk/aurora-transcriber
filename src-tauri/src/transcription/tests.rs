use super::*;
use crate::{
    export::{format_segments, ExportFormat},
    storage::Storage,
};
use std::path::Path;

/// Creates an isolated mock runtime without opening windows or contacting services.
fn app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_app()
}
/// Checks exclusive reservation, terminal cancellation cleanup, and independent next jobs.
#[test]
fn jobs_are_exclusive_and_cancellation_is_scoped() -> Result<(), String> {
    let app = app();
    let state = TranscriptionState::default();
    let first = state.begin("voice.wav".into())?;
    assert!(state.begin("other.wav".into()).is_err());
    state.update(app.handle(), |s| {
        s.segments.push(SubtitleSegment {
            start_sec: 0.0,
            end_sec: 2.0,
            text: "transient".into(),
        })
    });
    {
        let mut job = state.0.lock().map_err(|_| "lock")?;
        job.cancel = true;
    }
    assert!(state.check().is_err());
    state.finish(app.handle(), Ok(()));
    let cancelled = state.snapshot()?;
    assert_eq!(cancelled.phase, TranscriptionPhase::Cancelled);
    assert!(cancelled.segments.is_empty());
    let second = state.begin("other.wav".into())?;
    assert_ne!(first, second);
    state.check()?;
    state.finish(app.handle(), Err("Safe failure.".into()));
    assert!(state.snapshot()?.segments.is_empty());
    assert_eq!(state.snapshot()?.phase, TranscriptionPhase::Error);
    assert!(state.snapshot()?.revision > cancelled.revision);
    Ok(())
}
/// Verifies silence completion and safe failure snapshots without an ASR installation.
#[test]
fn empty_completion_has_no_partial_results() -> Result<(), String> {
    let app = app();
    let state = TranscriptionState::default();
    assert!(state.snapshot()?.phase == TranscriptionPhase::Idle);
    state.begin("silence.wav".into())?;
    state.finish(app.handle(), Ok(()));
    assert_eq!(state.snapshot()?.phase, TranscriptionPhase::Completed);
    assert_eq!(state.snapshot()?.percentage, Some(100.0));
    assert!(state.snapshot()?.segments.is_empty());
    Ok(())
}
/// Links local test resources into a temporary managed directory without downloading.
fn provision(models: &crate::model_storage::ModelStorage) -> anyhow::Result<()> {
    let source = crate::engine::runtime::absolute_env("AURORA_ASR_MODEL_DIR")?;
    let vad = crate::engine::runtime::absolute_env("AURORA_SILERO_MODEL")?;
    let base = models.prepare().map_err(anyhow::Error::msg)?;
    let target = base.join("parakeet");
    std::fs::create_dir_all(&target)?;
    for (name, _) in crate::engine::resources::FILES {
        std::fs::hard_link(source.join(name), target.join(name))?;
    }
    std::fs::hard_link(vad, base.join("silero_vad.onnx"))?;
    Ok(())
}
/// Exercises user-authorized audio, real VAD/ASR, explicit edited save and all exports offline.
#[test]
#[ignore = "requires local Parakeet, Silero, ORT 1.20.1 and authorized audio_samples/audio.m4a"]
fn local_audio_sample_pipeline_and_archive() -> anyhow::Result<()> {
    sample_pipeline(false)
}
/// Provisions an empty isolated profile explicitly, then verifies real audio and offline reload.
#[test]
#[ignore = "explicit multi-GB Hugging Face download and authorized audio_samples/audio.m4a"]
fn download_sample_and_offline_reload() -> anyhow::Result<()> {
    sample_pipeline(true)
}
/// Shares the real sample checks between offline resources and explicit network provisioning.
fn sample_pipeline(download: bool) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let storage = Storage::new(directory.path())?;
    let models = crate::model_storage::ModelStorage::new(directory.path());
    if !download {
        provision(&models)?;
    }
    let app = app();
    app.manage(storage);
    app.manage(models);
    app.manage(EngineState::default());
    app.manage(TranscriptionState::default());
    if download {
        crate::engine::fetcher::download(app.handle()).map_err(anyhow::Error::msg)?;
    }
    let audio = Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio_samples/audio.m4a");
    let job = app.state::<TranscriptionState>();
    let engine = app.state::<EngineState>();
    engine.reserve().map_err(anyhow::Error::msg)?;
    job.begin("audio.m4a".into()).map_err(anyhow::Error::msg)?;
    let result = process(app.handle(), &audio);
    job.finish(app.handle(), result);
    engine.release();
    let snapshot = job.snapshot().map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        snapshot.phase == TranscriptionPhase::Completed,
        "Local pipeline did not complete: {}",
        snapshot.error.as_deref().unwrap_or("unknown failure")
    );
    anyhow::ensure!(
        snapshot.segments.iter().any(|s| !s.text.trim().is_empty()),
        "No speech was transcribed"
    );
    assert_eq!(snapshot.current_chunk, snapshot.total_chunks);
    crate::export::validate_segments(&snapshot.segments).map_err(anyhow::Error::msg)?;
    let mut edited = snapshot.segments;
    edited[0].text = "Edited local verification text.".into();
    let storage = app.state::<Storage>();
    let saved = storage
        .save(None, "audio.m4a".into(), edited.clone())
        .map_err(anyhow::Error::msg)?;
    let reopened = storage.read(&saved.id).map_err(anyhow::Error::msg)?;
    assert_eq!(reopened.segments[0].text, edited[0].text);
    for format in [ExportFormat::Txt, ExportFormat::Srt, ExportFormat::Vtt] {
        let output = format_segments(&reopened.segments, format).map_err(anyhow::Error::msg)?;
        assert!(output.contains("Edited local verification text."));
    }
    // Selecting valid resources recognizes them offline and never migrates either folder.
    let alternate = tempfile::tempdir()?;
    let original = app
        .state::<crate::model_storage::ModelStorage>()
        .path()
        .map_err(anyhow::Error::msg)?;
    let alternate_asr = alternate.path().join("parakeet");
    std::fs::create_dir(&alternate_asr)?;
    for (name, _) in crate::engine::resources::FILES {
        std::fs::hard_link(
            original.join("parakeet").join(name),
            alternate_asr.join(name),
        )?;
    }
    std::fs::hard_link(
        original.join("silero_vad.onnx"),
        alternate.path().join("silero_vad.onnx"),
    )?;
    engine.reserve().map_err(anyhow::Error::msg)?;
    crate::model_storage::switch(app.handle(), Some(alternate.path().into()))
        .map_err(anyhow::Error::msg)?;
    engine.release();
    assert_eq!(
        engine.snapshot().map_err(anyhow::Error::msg)?.phase,
        "ready"
    );
    engine.reserve().map_err(anyhow::Error::msg)?;
    crate::model_storage::switch(app.handle(), None).map_err(anyhow::Error::msg)?;
    engine.release();
    assert_eq!(
        engine.snapshot().map_err(anyhow::Error::msg)?.phase,
        "ready"
    );
    crate::engine::fetcher::validate(&original)?;
    crate::engine::fetcher::validate(alternate.path())?;
    // A forced monotonic deadline verifies real session drop and subsequent offline reload.
    engine.unload_idle_at(
        app.handle(),
        std::time::Instant::now() + std::time::Duration::from_secs(301),
    );
    assert_eq!(
        engine.snapshot().map_err(anyhow::Error::msg)?.phase,
        "unloaded"
    );
    // A second call on silence reloads sessions and drops previous per-file VAD state.
    engine
        .with_sessions(app.handle(), |sessions| {
            assert!(sessions
                .vad
                .chunk_audio(&vec![0.0; 32_000])
                .map_err(|_| "VAD failed".to_string())?
                .is_empty());
            Ok(())
        })
        .map_err(anyhow::Error::msg)?;
    Ok(())
}
