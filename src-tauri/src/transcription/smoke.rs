//! Authorized short Metal verification using isolated local models and temporary outputs.
use super::*;
use crate::engine::{AsrSelection, Provider};

/// Verifies explicit Metal placement, a ten-second clip, teardown and offline reload.
#[test]
#[ignore = "requires local models, Metal and authorized first ten seconds of audio_samples/audio.m4a"]
fn milestone8_metal_pipeline() -> anyhow::Result<()> {
    run_metal_pipeline("AURORA_M8_MODEL_BASE", false)
}

/// Verifies a ten-second Metal clip, native cancellation, edited save/reopen and all exports.
#[test]
#[ignore = "requires local models, Metal and authorized first ten seconds of audio_samples/audio.m4a"]
fn milestone11_metal_pipeline() -> anyhow::Result<()> {
    run_metal_pipeline("AURORA_M11_MODEL_BASE", true)
}

/// Hard-links verified local models into an isolated profile and exercises their shared lifecycle.
///
/// Only the first ten seconds reach VAD/ASR. Existing model files remain untouched,
/// and no download or CPU ASR is performed. Optional archive checks write only in this profile.
fn run_metal_pipeline(source_env: &str, verify_archive_and_cancel: bool) -> anyhow::Result<()> {
    let source = crate::resources::absolute_env(source_env)?;
    let profile = tempfile::tempdir()?;
    let base = profile.path().join("models");
    let (asr, vad) = crate::engine::fetcher::paths(&base);
    let (source_asr, source_vad) = crate::engine::fetcher::paths(&source);
    std::fs::create_dir_all(&asr)?;
    std::fs::hard_link(
        source_asr.join(crate::engine::resources::MODEL_FILE),
        asr.join(crate::engine::resources::MODEL_FILE),
    )?;
    std::fs::hard_link(source_vad, vad)?;
    let app = tauri::test::mock_app();
    app.manage(crate::model_storage::ModelStorage::new(profile.path()));
    app.manage(EngineState::default());
    app.manage(TranscriptionState::default());
    app.manage(crate::storage::Storage::new(profile.path())?);
    let state = app.state::<EngineState>();
    let metal = crate::engine::discover_devices()?
        .into_iter()
        .find(|device| device.backend == Provider::Metal)
        .ok_or_else(|| anyhow::anyhow!("A real Metal GPU is required"))?;
    let selection = AsrSelection::Device {
        id: metal.id.clone(),
    };
    state.reserve().map_err(anyhow::Error::msg)?;
    let result = (|| -> anyhow::Result<()> {
        state
            .select(app.handle(), selection.clone())
            .map_err(anyhow::Error::msg)?;
        let ready = state.snapshot().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            ready.provider == Some(Provider::Metal) && ready.device.as_ref() == Some(&metal),
            "Resolved Metal device differs from the explicit request"
        );
        let audio =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio_samples/audio.m4a");
        let mut samples = crate::audio::AudioDecoder::decode_to_16khz_mono(&audio)?;
        samples.truncate(10 * 16_000);
        // Return the full-file allocation before inference; no extracted clip is written.
        samples.shrink_to_fit();
        let job = app.state::<TranscriptionState>();
        job.begin("authorized clip".into(), TranscriptionLanguage::Auto)
            .map_err(anyhow::Error::msg)?;
        let result = state.with_sessions(app.handle(), |sessions| {
            process_samples(app.handle(), sessions, &samples)
        });
        job.finish(app.handle(), result);
        let snapshot = job.snapshot().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            snapshot.phase == TranscriptionPhase::Completed,
            "Short Metal pipeline failed"
        );
        anyhow::ensure!(
            snapshot
                .segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty()),
            "No usable speech output"
        );
        assert_eq!(snapshot.current_chunk, snapshot.total_chunks);
        crate::export::validate_segments(&snapshot.segments).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            snapshot
                .segments
                .iter()
                .all(|segment| segment.end_sec <= 10.0),
            "Segment exceeds the authorized clip"
        );
        if verify_archive_and_cancel {
            verify_archive(&app, profile.path(), snapshot.segments)?;
            verify_cancellation(&app, &samples)?;
        } else {
            drop(snapshot);
        }
        drop(samples);
        // Erase the mock job's owned transcript before checking resident model lifecycle.
        *job.0
            .lock()
            .map_err(|_| anyhow::anyhow!("Job lock failed"))? = Job::default();
        Ok(())
    })();
    state.release();
    if let Err(error) = result {
        state.shutdown();
        return Err(error);
    }
    state.unload_idle_at(
        app.handle(),
        std::time::Instant::now() + std::time::Duration::from_secs(301),
    );
    let unloaded = state.snapshot().map_err(anyhow::Error::msg)?;
    assert_eq!(unloaded.phase, "unloaded");
    assert!(unloaded.device.is_none() && unloaded.provider.is_none());
    assert_eq!(unloaded.selection, selection);
    state.reserve().map_err(anyhow::Error::msg)?;
    let reload = state.with_sessions(app.handle(), |sessions| {
        assert_eq!(sessions.asr.device(), &metal);
        // Readiness and a new empty file session exercise teardown without another ASR run.
        sessions
            .asr
            .with_file(|_| ())
            .map_err(|error| error.to_string())?;
        Ok(())
    });
    state.release();
    state.shutdown();
    reload.map_err(anyhow::Error::msg)?;
    assert_eq!(
        state.snapshot().map_err(anyhow::Error::msg)?.selection,
        selection
    );
    println!("Short Metal pipeline and offline reload passed; archive/cancellation checks: {verify_archive_and_cancel}.");
    Ok(())
}

/// Saves edited text and verifies archive reopening and export bytes in the temporary profile.
fn verify_archive(
    app: &tauri::App<tauri::test::MockRuntime>,
    destination_dir: &std::path::Path,
    segments: Vec<SubtitleSegment>,
) -> anyhow::Result<()> {
    let mut edited = segments;
    let first = edited
        .first_mut()
        .ok_or_else(|| anyhow::anyhow!("Missing speech segment"))?;
    first.text = "Edited verification text.".into();
    let storage = app.state::<crate::storage::Storage>();
    let saved = storage
        .save(None, "authorized clip".into(), edited)
        .map_err(anyhow::Error::msg)?;
    let reopened = storage.read(&saved.id).map_err(anyhow::Error::msg)?;
    assert_eq!(reopened.segments[0].text, "Edited verification text.");
    for format in [
        crate::export::ExportFormat::Txt,
        crate::export::ExportFormat::Srt,
        crate::export::ExportFormat::Vtt,
    ] {
        let content = crate::export::format_segments(&reopened.segments, format)
            .map_err(anyhow::Error::msg)?;
        let destination = destination_dir.join(format!("verified.{}", format.extension()));
        std::fs::write(&destination, &content)?;
        assert_eq!(std::fs::read_to_string(destination)?, content);
    }
    drop(saved);
    drop(reopened);
    Ok(())
}

/// Tests the real abort callback and cancels a second pipeline job at the ASR boundary.
fn verify_cancellation(
    app: &tauri::App<tauri::test::MockRuntime>,
    samples: &[f32],
) -> anyhow::Result<()> {
    // Exercise the real native abort callback without an extra decoding workload.
    let state = app.state::<EngineState>();
    let job = app.state::<TranscriptionState>();
    let cancelled = CancelToken::new();
    cancelled.cancel();
    state
        .with_sessions(app.handle(), |sessions| {
            sessions
                .asr
                .with_file_cancellable(&cancelled, |file| {
                    assert!(matches!(
                        file.transcribe_chunk(samples),
                        Err(crate::engine::EngineError::Cancelled)
                    ));
                })
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .map_err(anyhow::Error::msg)?;
    job.begin("cancelled clip".into(), TranscriptionLanguage::Auto)
        .map_err(anyhow::Error::msg)?;
    let second = std::thread::scope(|scope| {
        let cancellation_app = app.handle().clone();
        let watcher = scope.spawn(move || -> Result<(), String> {
            let job = cancellation_app.state::<TranscriptionState>();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                let phase = job.0.lock().map_err(|_| "Job lock failed")?.snapshot.phase;
                if phase == TranscriptionPhase::Transcribing {
                    cancel_transcription(cancellation_app.state::<TranscriptionState>())?;
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    return Err("Cancellation boundary was not reached.".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        let result = state.with_sessions(app.handle(), |sessions| {
            process_samples(app.handle(), sessions, samples)
        });
        let requested = watcher
            .join()
            .map_err(|_| "Cancellation watcher failed".to_string())?;
        requested?;
        Ok::<_, String>(result)
    })
    .map_err(anyhow::Error::msg)?;
    job.finish(app.handle(), second);
    let cancelled = job.snapshot().map_err(anyhow::Error::msg)?;
    assert_eq!(cancelled.phase, TranscriptionPhase::Cancelled);
    assert!(cancelled.segments.is_empty() && cancelled.error.is_none());
    drop(cancelled);
    Ok(())
}
