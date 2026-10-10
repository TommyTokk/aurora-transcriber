//! Explicit offline ASR tests requiring pinned local resources and licensed audio.
use aurora_lib::{
    audio::AudioDecoder,
    engine::{EngineError, Provider, TranscriberEngine},
};
use serde::Deserialize;
use std::path::PathBuf;

/// A local licensed sample and the CPU output of the pinned Python reference.
#[derive(Deserialize)]
struct Case {
    audio_path: PathBuf,
    reference_text: String,
    language: String,
}

/// Reads backend test configuration without downloading models or audio.
fn model_dir() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("AURORA_ASR_MODEL_DIR")
            .ok_or_else(|| anyhow::anyhow!("Set AURORA_ASR_MODEL_DIR"))?,
    ))
}

/// Loads caller-provided cases whose expected text was generated independently.
fn cases() -> anyhow::Result<Vec<Case>> {
    let file = std::env::var_os("AURORA_ASR_TEST_CASES")
        .ok_or_else(|| anyhow::anyhow!("Set AURORA_ASR_TEST_CASES"))?;
    Ok(serde_json::from_slice(&std::fs::read(file)?)?)
}

/// Compares IT/EN chunks with the same export on CPU and checks per-call reset.
#[test]
#[ignore = "requires local Parakeet FP32, ORT 1.20.1, and reference IT/EN cases"]
fn cpu_reference_and_repeated_calls() -> anyhow::Result<()> {
    let mut engine = TranscriberEngine::new_cpu(&model_dir()?)?;
    assert_eq!(engine.provider(), Provider::Cpu);
    assert_eq!(engine.transcribe_chunk(&[])?, "");
    assert!(matches!(
        engine.transcribe_chunk(&[f32::NAN]),
        Err(EngineError::InvalidSamples)
    ));
    let cases = cases()?;
    anyhow::ensure!(
        cases.iter().any(|case| case.language == "it")
            && cases.iter().any(|case| case.language == "en"),
        "Provide IT and EN reference cases"
    );
    // Reverse the second pass so each sample follows a different predecessor.
    // Both passes use this same engine; no model construction occurs in the loop.
    for case in cases.iter().chain(cases.iter().rev()) {
        let samples = AudioDecoder::decode_to_16khz_mono(&case.audio_path)?;
        let text = engine.transcribe_chunk(&samples)?;
        anyhow::ensure!(
            text == case.reference_text,
            "ASR differs from the pinned reference for {} ({} versus {} characters)",
            case.language,
            text.chars().count(),
            case.reference_text.chars().count()
        );
        drop(text);
        drop(samples);
    }
    Ok(())
}

/// Exercises the platform provider chain with real sessions and finite input.
#[test]
#[ignore = "requires local Parakeet FP32 and ORT 1.20.1 with optional GPU providers"]
fn platform_provider_smoke() -> anyhow::Result<()> {
    let mut engine = TranscriberEngine::new(&model_dir()?)?;
    eprintln!("Configured ASR provider: {:?}", engine.provider());
    for case in cases()? {
        let samples = AudioDecoder::decode_to_16khz_mono(&case.audio_path)?;
        let text = engine.transcribe_chunk(&samples)?;
        anyhow::ensure!(!text.trim().is_empty(), "Speech sample returned empty text");
        drop(text);
        drop(samples);
    }
    Ok(())
}

/// Verifies startup-facing errors stay fallible with a missing or invalid runtime.
///
/// Child processes isolate ORT's global state and environment from other tests.
#[test]
#[ignore = "requires a valid local model directory; uses isolated invalid-runtime children"]
fn invalid_runtime_is_recoverable() -> anyhow::Result<()> {
    if std::env::var_os("AURORA_INVALID_RUNTIME_CHILD").is_some() {
        assert!(matches!(
            TranscriberEngine::new(&model_dir()?),
            Err(EngineError::Configuration | EngineError::RuntimeLibrary)
        ));
        return Ok(());
    }
    let invalid = tempfile::NamedTempFile::new()?;
    let missing = invalid.path().with_extension("missing-library");
    for path in [
        None,
        Some(invalid.path()),
        Some(missing.as_path()),
        Some(std::path::Path::new("relative-library")),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .args(["--ignored", "--exact", "invalid_runtime_is_recoverable"])
            .env("AURORA_INVALID_RUNTIME_CHILD", "1");
        match path {
            Some(path) => {
                command.env("ORT_DYLIB_PATH", path);
            }
            None => {
                command.env_remove("ORT_DYLIB_PATH");
            }
        }
        anyhow::ensure!(
            command.output()?.status.success(),
            "Invalid runtime terminated the isolated engine process"
        );
    }
    Ok(())
}
