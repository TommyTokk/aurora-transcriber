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
    let first = state.begin("voice.wav".into(), TranscriptionLanguage::Auto)?;
    assert!(state
        .begin("other.wav".into(), TranscriptionLanguage::Auto)
        .is_err());
    state.update(app.handle(), |s| {
        s.segments.push(SubtitleSegment {
            start_sec: 0.0,
            end_sec: 2.0,
            text: "transient".into(),
        })
    });
    {
        let job = state.0.lock().map_err(|_| "lock")?;
        job.cancel.cancel();
    }
    assert!(state.check().is_err());
    state.finish(app.handle(), Ok(()));
    let cancelled = state.snapshot()?;
    assert_eq!(cancelled.phase, TranscriptionPhase::Cancelled);
    assert!(cancelled.segments.is_empty());
    let second = state.begin("other.wav".into(), TranscriptionLanguage::Auto)?;
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
    state.begin("silence.wav".into(), TranscriptionLanguage::Auto)?;
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
    let profile = crate::resources::absolute_env("AURORA_M7_PROFILE")?;
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
        job.begin("audio.m4a".into(), TranscriptionLanguage::Auto)
            .map_err(anyhow::Error::msg)?;
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

/// Runs a long synthetic timeline through production orchestration without native inference.
#[test]
fn many_chunks_preserve_source_time_and_order() -> Result<(), String> {
    let app = app();
    let state = TranscriptionState::default();
    state.begin("long.wav".into(), TranscriptionLanguage::Auto)?;
    let samples = [0.0; 16];
    let chunks: Vec<_> = (0..200)
        .map(|i| crate::vad::AudioChunk {
            samples: &samples,
            start_sec: i as f64 * 26.0,
            end_sec: i as f64 * 26.0 + 25.0,
        })
        .collect();
    let mut revisions = Vec::new();
    let result = chunks::transcribe(
        &chunks,
        &state.cancel_token()?,
        |_| Ok("simulated".into()),
        |index, segment| {
            state.update(app.handle(), |s| {
                s.segments.push(segment);
                s.current_chunk = index;
                s.total_chunks = chunks.len();
            });
            revisions.push(state.snapshot().map(|s| s.revision));
        },
    );
    state.finish(app.handle(), result);
    let snapshot = state.snapshot()?;
    assert_eq!(snapshot.phase, TranscriptionPhase::Completed);
    assert_eq!(snapshot.current_chunk, 200);
    assert!(snapshot.segments[199].start_sec > 3600.0);
    crate::export::validate_segments(&snapshot.segments)?;
    let revisions: Vec<_> = revisions.into_iter().collect::<Result<_, _>>()?;
    assert!(revisions.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}

/// Covers cancellation at every publication boundary and failures after partial progress.
#[test]
fn chunk_cancellation_discards_results_and_allows_next_job() -> Result<(), String> {
    let app = app();
    let state = TranscriptionState::default();
    let samples = [0.0];
    let chunks: Vec<_> = (0..3)
        .map(|i| crate::vad::AudioChunk {
            samples: &samples,
            start_sec: i as f64 * 2.0,
            end_sec: i as f64 * 2.0 + 1.0,
        })
        .collect();
    for scenario in 0..5 {
        state.begin("sample.wav".into(), TranscriptionLanguage::Auto)?;
        let cancel = state.cancel_token()?;
        let mut calls = 0;
        if scenario == 0 {
            cancel.cancel();
        }
        let result = chunks::transcribe(
            &chunks,
            &cancel,
            |_| {
                calls += 1;
                if scenario == 1 {
                    cancel.cancel();
                }
                if scenario == 4 && calls == 2 {
                    return Err(crate::engine::EngineError::Inference);
                }
                Ok("transient".into())
            },
            |index, segment| {
                state.update(app.handle(), |s| s.segments.push(segment));
                if (scenario == 2 && index == 1) || (scenario == 3 && index == 3) {
                    cancel.cancel();
                }
            },
        );
        assert!(result.is_err());
        state.finish(app.handle(), result);
        let snapshot = state.snapshot()?;
        assert!(snapshot.segments.is_empty());
        assert_eq!(
            snapshot.phase,
            if scenario == 4 {
                TranscriptionPhase::Error
            } else {
                TranscriptionPhase::Cancelled
            }
        );
        assert_eq!(calls, [0, 1, 1, 3, 2][scenario]);
    }
    state.begin("silence.wav".into(), TranscriptionLanguage::Auto)?;
    assert!(!state.cancel_token()?.is_cancelled());
    let result = chunks::transcribe(
        &[],
        &state.cancel_token()?,
        |_| Err(crate::engine::EngineError::Inference),
        |_, _| {},
    );
    state.finish(app.handle(), result);
    assert_eq!(state.snapshot()?.phase, TranscriptionPhase::Completed);
    Ok(())
}

/// Classifies typed native aborts as cancelled without requiring an explicit cancel command.
#[test]
fn native_abort_is_terminal_cancellation() -> Result<(), String> {
    let app = app();
    let state = TranscriptionState::default();
    state.begin("clip.wav".into(), TranscriptionLanguage::Auto)?;
    let samples = [0.0];
    let chunk = crate::vad::AudioChunk {
        samples: &samples,
        start_sec: 0.0,
        end_sec: 1.0,
    };
    let result = chunks::transcribe(
        &[chunk],
        &state.cancel_token()?,
        |_| Err(crate::engine::EngineError::Cancelled),
        |_, _| {},
    );
    state.finish(app.handle(), result);
    let result = state.snapshot()?;
    assert_eq!(result.phase, TranscriptionPhase::Cancelled);
    assert!(result.error.is_none() && result.segments.is_empty());
    Ok(())
}

/// Cancels from another thread while simulated inference owns its scoped resources.
#[test]
fn cancellation_reaches_inflight_worker_and_releases_scope() -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    /// Counts teardown of the fake native session on the error path.
    struct SessionGuard(Arc<AtomicUsize>);
    impl Drop for SessionGuard {
        /// Records that the worker's session was freed before the test completes.
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let cancel = CancelToken::new();
    let barrier = Arc::new(Barrier::new(2));
    let released = Arc::new(AtomicUsize::new(0));
    let worker_cancel = cancel.clone();
    let worker_barrier = barrier.clone();
    let worker_released = released.clone();
    let worker = std::thread::spawn(move || {
        let _session = SessionGuard(worker_released);
        let samples = [0.0];
        let chunk = crate::vad::AudioChunk {
            samples: &samples,
            start_sec: 0.0,
            end_sec: 1.0,
        };
        chunks::transcribe(
            &[chunk],
            &worker_cancel,
            |_| {
                worker_barrier.wait();
                worker_barrier.wait();
                Ok("discarded".into())
            },
            |_, _| panic!("Cancelled inference must not publish text"),
        )
    });
    barrier.wait();
    cancel.cancel();
    barrier.wait();
    let result = worker
        .join()
        .map_err(|_| anyhow::anyhow!("Simulated worker failed"))?;
    assert!(result.is_err());
    assert_eq!(released.load(Ordering::SeqCst), 1);
    Ok(())
}

/// Progress, cancellation, and rejected duplicate jobs cannot change the captured language.
#[test]
fn job_language_is_fixed_until_the_next_job() -> Result<(), String> {
    let app = app();
    let state = TranscriptionState::default();
    for language in [
        TranscriptionLanguage::Italian,
        TranscriptionLanguage::English,
        TranscriptionLanguage::Auto,
    ] {
        state.begin("clip.wav".into(), language)?;
        assert!(state
            .begin("other.wav".into(), TranscriptionLanguage::Auto)
            .is_err());
        let samples = [0.0; 16];
        let chunks: Vec<_> = (0..3)
            .map(|index| crate::vad::AudioChunk {
                samples: &samples,
                start_sec: index as f64,
                end_sec: index as f64 + 1.0,
            })
            .collect();
        let mut hints = Vec::new();
        chunks::transcribe(
            &chunks,
            &state.cancel_token()?,
            |_| {
                let captured = state
                    .language()
                    .map_err(|_| crate::engine::EngineError::Unavailable)?;
                hints.push(captured.hint());
                Ok("test".into())
            },
            |index, segment| {
                state.update(app.handle(), |snapshot| {
                    snapshot.current_chunk = index;
                    snapshot.segments.push(segment);
                })
            },
        )?;
        assert_eq!(hints, vec![language.hint(); 3]);
        state.cancel_token()?.cancel();
        state.finish(app.handle(), Ok(()));
        assert_eq!(state.snapshot()?.phase, TranscriptionPhase::Cancelled);
        assert_eq!(state.language()?, language);
    }
    Ok(())
}
