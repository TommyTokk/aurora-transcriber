//! Offline GGUF construction checks; no CPU ASR or performance runs.
use aurora_lib::engine::{EngineError, Provider, TranscriberEngine};
use std::path::PathBuf;

/// Resolves the explicitly supplied local GGUF directory without downloading anything.
fn model_dir() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("AURORA_ASR_MODEL_DIR")
            .ok_or_else(|| anyhow::anyhow!("Set AURORA_ASR_MODEL_DIR"))?,
    ))
}

/// Rejects missing local resources without returning unsafe paths or aborting.
#[test]
fn absent_model_is_recoverable() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let error = match TranscriberEngine::new(directory.path()) {
        Ok(_) => anyhow::bail!("Missing GGUF unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        EngineError::Resources | EngineError::GpuUnavailable
    ));
    assert!(!error
        .to_string()
        .contains(&directory.path().display().to_string()));
    Ok(())
}

/// Loads Metal GGUF independently of ORT and checks safe session reuse without audio inference.
#[test]
#[ignore = "requires pinned local GGUF and an Apple Silicon Metal device; no CPU ASR"]
fn metal_load_and_file_sessions() -> anyhow::Result<()> {
    let mut engine = TranscriberEngine::new(&model_dir()?)?;
    assert_eq!(engine.provider(), Provider::Metal);
    assert_eq!(engine.transcribe_chunk(&[])?, "");
    assert!(matches!(
        engine.transcribe_chunk(&[f32::NAN]),
        Err(EngineError::InvalidSamples)
    ));
    let failure = engine.with_file(|file| -> Result<(), EngineError> {
        assert_eq!(file.transcribe_chunk(&[])?, "");
        Err(EngineError::Inference)
    })?;
    assert!(failure.is_err());
    // A failed file releases its session while resident weights remain usable.
    engine.with_file(|file| -> Result<(), EngineError> {
        assert_eq!(file.transcribe_chunk(&[])?, "");
        Ok(())
    })??;
    Ok(())
}
