//! Explicit integration tests requiring a local Silero v5.1 model and ORT 1.20.1.

use std::path::PathBuf;

use aurora_lib::vad::{SileroVad, VadError};

/// Loads caller-provided resources; an explicit test run fails if they are absent.
fn local_vad() -> anyhow::Result<SileroVad> {
    let model = PathBuf::from(std::env::var_os("AURORA_SILERO_MODEL").ok_or_else(|| {
        anyhow::anyhow!("Set AURORA_SILERO_MODEL to the local Silero v5.1 ONNX file")
    })?);
    let runtime = PathBuf::from(std::env::var_os("ORT_DYLIB_PATH").ok_or_else(|| {
        anyhow::anyhow!("Set ORT_DYLIB_PATH to the local ONNX Runtime 1.20.1 library")
    })?);
    anyhow::ensure!(
        runtime.is_file(),
        "ORT_DYLIB_PATH must name a local library"
    );
    Ok(SileroVad::new(&model)?)
}

/// Exercises real inference, zero-length input, final padding, and repeated calls.
#[test]
#[ignore = "requires local Silero v5.1 and ONNX Runtime 1.20.1"]
fn local_silero_silence_and_repeated_calls() -> anyhow::Result<()> {
    let mut vad = local_vad()?;
    assert!(vad.chunk_audio(&[])?.is_empty());
    for length in [1, 511, 512, 513, 32_017] {
        let silence = vec![0.0; length];
        assert!(vad.chunk_audio(&silence)?.is_empty());
        drop(silence);
    }
    // A different signal primes inference; the next file must start from zeros.
    let signal: Vec<f32> = (0..32_017)
        .map(|index| (index as f32 * 0.17).sin() * 0.5)
        .collect();
    let first: Vec<(f64, f64)> = vad
        .chunk_audio(&signal)?
        .iter()
        .map(|chunk| (chunk.start_sec, chunk.end_sec))
        .collect();
    assert!(vad.chunk_audio(&vec![0.0; 32_017])?.is_empty());
    let second: Vec<(f64, f64)> = vad
        .chunk_audio(&signal)?
        .iter()
        .map(|chunk| (chunk.start_sec, chunk.end_sec))
        .collect();
    assert_eq!(first, second);
    drop(signal);
    assert!(matches!(
        vad.chunk_audio(&[f32::NAN]),
        Err(VadError::InvalidSamples)
    ));
    Ok(())
}
