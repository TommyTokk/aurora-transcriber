use super::*;

/// Verifies context alignment and zero padding independently of ONNX inference.
#[test]
fn prepares_context_and_partial_frames() {
    let mut context = [0.0; CONTEXT];
    let mut frame = [0.25; FRAME];
    frame[FRAME - CONTEXT..].fill(0.75);
    let input = prepare_frame(&frame, &mut context);
    assert_eq!(&input[..CONTEXT], &[0.0; CONTEXT]);
    assert_eq!(&input[CONTEXT..], &frame);
    assert_eq!(context, [0.75; CONTEXT]);
    let input = prepare_frame(&[0.5; 17], &mut context);
    assert_eq!(&input[..CONTEXT], &[0.75; CONTEXT]);
    assert_eq!(&input[CONTEXT..CONTEXT + 17], &[0.5; 17]);
    assert!(input[CONTEXT + 17..].iter().all(|value| *value == 0.0));
    assert_eq!(context, [0.0; CONTEXT]);
}

/// Checks input validation without initializing ONNX or reading sensitive audio.
#[test]
fn validates_audio_samples() {
    assert!(validate_samples(&[]).is_ok());
    assert!(validate_samples(&[-1.0, 0.0, 1.0]).is_ok());
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.01, 1.01] {
        assert!(matches!(
            validate_samples(&[value]),
            Err(VadError::InvalidSamples)
        ));
    }
}

/// Verifies missing models and directories fail before loading the ONNX runtime.
#[test]
fn rejects_missing_model_and_directory() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    assert!(matches!(
        SileroVad::new(directory.path()),
        Err(VadError::ModelNotFile)
    ));
    assert!(matches!(
        SileroVad::new(&directory.path().join("missing.onnx")),
        Err(VadError::ModelIo(_))
    ));
    Ok(())
}

/// Exercises contract dimension and element-type checks without a runtime.
#[test]
fn validates_tensor_contract() {
    let value = ValueType::Tensor {
        ty: TensorElementType::Float32,
        dimensions: vec![2, -1, 128],
        dimension_symbols: vec![None; 3],
    };
    assert!(matches_tensor(
        &value,
        TensorElementType::Float32,
        &[2, 1, 128]
    ));
    assert!(!matches_tensor(
        &value,
        TensorElementType::Int64,
        &[2, 1, 128]
    ));
    assert!(!matches_tensor(
        &value,
        TensorElementType::Float32,
        &[2, 1, 64]
    ));
    assert!(!matches_tensor(
        &value,
        TensorElementType::Float32,
        &[2, 128]
    ));
}
