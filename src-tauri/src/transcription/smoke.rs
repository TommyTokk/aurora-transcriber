//! Authorized short Metal verification; no downloads, transcript files or CPU ASR.
use super::*;
use crate::engine::{AsrSelection, Provider};

/// Verifies explicit Metal placement, short app-pipeline inference, teardown and offline reload.
///
/// AURORA_M8_MODEL_BASE names an existing model folder containing parakeet/ and
/// silero_vad.onnx. Hard links populate an isolated temporary profile, preserving
/// user-owned models and configuration. Only the first ten seconds reach ASR.
#[test]
#[ignore = "requires local verified models, real Metal and authorized first ten seconds of audio_samples/audio.m4a"]
fn milestone8_metal_pipeline() -> anyhow::Result<()> {
    let source = crate::engine::runtime::absolute_env("AURORA_M8_MODEL_BASE")?;
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
        job.begin("authorized clip".into())
            .map_err(anyhow::Error::msg)?;
        let result = state.with_sessions(app.handle(), |sessions| {
            process_samples(app.handle(), sessions, &samples)
        });
        drop(samples);
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
        drop(snapshot);
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
    println!("Milestone 8: exact Metal device, <=10 s pipeline, session cleanup and offline selection reload passed.");
    Ok(())
}
