use super::*;

/// Rejects invalid and oversized audio before calling native inference.
#[test]
fn input_bounds() -> anyhow::Result<()> {
    validate_samples(&[])?;
    validate_samples(&[-1.0, 0.0, 1.0])?;
    validate_samples(&vec![0.0; MAX_SAMPLES])?;
    for samples in [
        &[f32::NAN][..],
        &[f32::INFINITY][..],
        &[1.01][..],
        &[-1.01][..],
    ] {
        assert!(matches!(
            validate_samples(samples),
            Err(EngineError::InvalidSamples)
        ));
    }
    assert!(validate_samples(&vec![0.0; MAX_SAMPLES + 1]).is_err());
    Ok(())
}

/// Rejects other GGUF variants and incompatible sample rates.
#[test]
fn model_identity_is_strict() -> anyhow::Result<()> {
    validate_contract("parakeet", "tdt-0.6b-v3", 16_000)?;
    for (arch, variant, rate) in [
        ("whisper", "tdt-0.6b-v3", 16_000),
        ("parakeet", "tdt-0.6b-v2", 16_000),
        ("parakeet", "", 16_000),
        ("parakeet", "tdt-0.6b-v3", 8_000),
    ] {
        assert!(matches!(
            validate_contract(arch, variant, rate),
            Err(EngineError::ModelContract)
        ));
    }
    Ok(())
}

/// Ensures native details and partial transcripts cannot leak through error messages.
#[test]
fn native_errors_are_safe_and_typed() {
    for error in [
        transcribe_cpp::Error::ModelLoad("/private/model".into()),
        transcribe_cpp::Error::Aborted {
            message: "secret".into(),
            partial: None,
        },
        transcribe_cpp::Error::OutputTruncated {
            message: "secret".into(),
            partial: None,
        },
        transcribe_cpp::Error::Backend("/private/device".into()),
        transcribe_cpp::Error::OutOfMemory("secret".into()),
    ] {
        let safe = map_native_error(error).to_string();
        assert!(!safe.contains("secret"));
        assert!(!safe.contains("/private"));
    }
    assert!(matches!(
        map_native_error(transcribe_cpp::Error::Backend(String::new())),
        EngineError::GpuUnavailable
    ));
    assert!(matches!(
        map_native_error(transcribe_cpp::Error::OutOfMemory(String::new())),
        EngineError::OutOfMemory
    ));
}
