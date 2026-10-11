use super::*;
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
/// Downloads explicitly into the chosen profile, then tests only a ten-second Metal sample.
///
/// AURORA_M7_PROFILE selects an existing/native model configuration. This ignored
/// test never saves transcripts or audio and never performs CPU ASR.
#[test]
#[ignore = "explicit GGUF download and authorized first ten seconds of audio_samples/audio.m4a"]
fn milestone7_metal_pipeline() -> anyhow::Result<()> {
    let profile = crate::engine::runtime::absolute_env("AURORA_M7_PROFILE")?;
    let models = crate::model_storage::ModelStorage::new(&profile);
    let app = app();
    app.manage(models);
    app.manage(EngineState::default());
    app.manage(TranscriptionState::default());
    let engine = app.state::<EngineState>();
    engine.reserve().map_err(anyhow::Error::msg)?;
    let result = (|| -> anyhow::Result<()> {
        crate::engine::fetcher::download(app.handle()).map_err(anyhow::Error::msg)?;
        let audio = Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio_samples/audio.m4a");
        let mut samples = crate::audio::AudioDecoder::decode_to_16khz_mono(&audio)?;
        samples.truncate(10 * 16_000);
        // Reallocation releases the original full-file buffer before ASR processing.
        samples.shrink_to_fit();
        let job = app.state::<TranscriptionState>();
        job.begin("audio.m4a".into()).map_err(anyhow::Error::msg)?;
        let result = engine.with_sessions(app.handle(), |sessions| {
            process_samples(app.handle(), sessions, &samples)
        });
        drop(samples);
        job.finish(app.handle(), result);
        let snapshot = job.snapshot().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            snapshot.phase == TranscriptionPhase::Completed,
            "Short Metal pipeline failed: {}",
            snapshot.error.as_deref().unwrap_or("unknown safe error")
        );
        anyhow::ensure!(
            snapshot
                .segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty()),
            "No speech output"
        );
        assert_eq!(snapshot.current_chunk, snapshot.total_chunks);
        crate::export::validate_segments(&snapshot.segments).map_err(anyhow::Error::msg)?;
        assert_eq!(
            engine.snapshot().map_err(anyhow::Error::msg)?.provider,
            Some(crate::engine::Provider::Metal)
        );
        drop(snapshot);
        // Discard the in-memory output before checking resource lifecycle.
        *job.0
            .lock()
            .map_err(|_| anyhow::anyhow!("Job lock failed"))? = Job::default();
        let base = app
            .state::<crate::model_storage::ModelStorage>()
            .prepare()
            .map_err(anyhow::Error::msg)?;
        crate::engine::fetcher::validate(&base)?;
        Ok(())
    })();
    engine.release();
    result?;
    engine.unload_idle_at(
        app.handle(),
        std::time::Instant::now() + std::time::Duration::from_secs(301),
    );
    assert_eq!(
        engine.snapshot().map_err(anyhow::Error::msg)?.phase,
        "unloaded"
    );
    // Reload does not contact Hugging Face and session errors do not unload the weights.
    engine
        .with_sessions(app.handle(), |sessions| {
            sessions
                .asr
                .with_file(|file| file.transcribe_chunk(&[]))
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        engine.snapshot().map_err(anyhow::Error::msg)?.phase,
        "ready"
    );
    // Mock apps do not run Tauri's Exit event; explicitly exercise the same worker teardown.
    engine.shutdown();
    println!("Milestone 7: verified GGUF, Metal device, <=10 s pipeline, session cleanup and offline reload passed.");
    Ok(())
}
