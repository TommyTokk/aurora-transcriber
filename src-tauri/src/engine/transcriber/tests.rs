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

/// Checks native session windows without allocating a model or invoking CPU ASR.
#[test]
fn effective_session_window_is_compatible() -> anyhow::Result<()> {
    for limit in [0, 25_000, 60_000] {
        validate_session_limit(limit)?;
    }
    for limit in [-1, 1, 24_999] {
        assert!(matches!(
            validate_session_limit(limit),
            Err(EngineError::SessionLimits)
        ));
    }
    assert!(matches!(
        map_native_error(transcribe_cpp::Error::Aborted {
            message: "private partial".into(),
            partial: None,
        }),
        EngineError::Cancelled
    ));
    Ok(())
}

/// ABI/version failures remain terminal while native load errors permit recovery.
#[test]
fn native_compatibility_is_not_a_load_failure() {
    for error in [
        transcribe_cpp::Error::VersionMismatch("private".into()),
        transcribe_cpp::Error::BadStructSize("private".into()),
    ] {
        assert!(matches!(
            map_native_error(error),
            EngineError::NativeCompatibility
        ));
    }
    assert!(matches!(
        map_native_error(transcribe_cpp::Error::ModelLoad("private".into())),
        EngineError::ModelLoad
    ));
}

/// Exercises a simulated GPU failure followed by real offline CPU model/session loading.
#[test]
#[ignore = "requires pinned local GGUF via AURORA_ASR_MODEL_DIR; no audio inference"]
fn cpu_recovery_loads_verified_native_sessions() -> anyhow::Result<()> {
    let directory = std::env::var_os("AURORA_ASR_MODEL_DIR")
        .ok_or_else(|| anyhow::anyhow!("Set AURORA_ASR_MODEL_DIR"))?;
    let mut candidates = vec![transcribe_cpp::ModelOptions {
        backend: transcribe_cpp::Backend::Metal,
        device: None,
    }];
    candidates.extend(providers::options(&AsrSelection::Cpu {})?);
    let (mut engine, reason) =
        providers::try_candidates(&candidates, &AsrSelection::Auto {}, |options| {
            if options.backend != transcribe_cpp::Backend::Cpu {
                return Err(EngineError::GpuUnavailable);
            }
            TranscriberEngine::new_with_selection(Path::new(&directory), &AsrSelection::Cpu {})
        })?;
    engine.fallback_reason = reason;
    assert_eq!(engine.provider(), Provider::Cpu);
    assert_eq!(
        engine.fallback_reason(),
        Some(FallbackReason::GpuInitializationFailed)
    );
    let failure = engine.with_file(|file| -> Result<(), EngineError> {
        assert_eq!(file.transcribe_chunk(&[])?, "");
        Err(EngineError::Inference)
    })?;
    assert!(failure.is_err());
    engine.with_file(|file| file.transcribe_chunk(&[]))??;
    assert_eq!(engine.provider(), Provider::Cpu);
    Ok(())
}
