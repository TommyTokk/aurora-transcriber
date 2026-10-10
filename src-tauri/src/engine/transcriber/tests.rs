use super::*;
/// Rejects invalid samples and oversized chunks before calling ONNX Runtime.
#[test]
fn input_bounds() -> anyhow::Result<()> {
    validate_samples(&[])?;
    validate_samples(&[-1.0, 0.0, 1.0])?;
    validate_samples(&vec![0.0; MAX_SAMPLES])?;
    for samples in [&[f32::NAN][..], &[f32::INFINITY][..], &[1.01][..]] {
        assert!(validate_samples(samples).is_err());
    }
    assert!(validate_samples(&vec![0.0; MAX_SAMPLES + 1]).is_err());
    Ok(())
}

/// Rejects malformed feature matrices and batch lengths before indexing.
#[test]
fn invalid_model_outputs() -> anyhow::Result<()> {
    validate_matrix(&[1, 2, 2], &[0.0; 4], 2)?;
    for (shape, data) in [
        (&[1, 2][..], &[0.0; 4][..]),
        (&[2, 2, 1][..], &[0.0; 4][..]),
        (&[1, 2, 0][..], &[][..]),
        (&[1, 2, 2][..], &[0.0; 3][..]),
        (&[1, 2, 1][..], &[0.0, f32::NAN][..]),
    ] {
        assert!(validate_matrix(shape, data, 2).is_err());
    }
    assert_eq!(validate_length_data(&[1], &[2], 2)?, 2);
    for lengths in [&[][..], &[0][..], &[-1][..], &[3][..], &[1, 1][..]] {
        assert!(validate_length_data(&[1], lengths, 2).is_err());
    }
    assert!(validate_length_data(&[1, 1], &[1], 2).is_err());
    Ok(())
}
