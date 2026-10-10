//! Named, typed tensor contracts for the fixed Parakeet FP32 export.
use super::EngineError;
use ort::{session::Session, tensor::TensorElementType as Ty, value::ValueType};

type TensorSpec<'a> = (&'a str, Ty, &'a [i64]);

/// Accepts dynamic axes but requires fixed dimensions to match the expected graph.
fn matches(value: &ValueType, ty: Ty, expected: &[i64]) -> bool {
    matches!(value, ValueType::Tensor { ty: actual, dimensions, .. }
        if *actual == ty && dimensions.len() == expected.len()
        && dimensions.iter().zip(expected).all(|(actual, expected)| *actual == -1 || (*expected == -1 && *actual > 0) || actual == expected))
}

/// Checks every named input and output; additional unexpected tensors are rejected.
fn check(
    session: &Session,
    inputs: &[TensorSpec<'_>],
    outputs: &[TensorSpec<'_>],
) -> Result<(), EngineError> {
    if session.inputs.len() != inputs.len()
        || session.outputs.len() != outputs.len()
        || inputs.iter().any(|(name, ty, shape)| {
            !session
                .inputs
                .iter()
                .any(|input| input.name == *name && matches(&input.input_type, *ty, shape))
        })
        || outputs.iter().any(|(name, ty, shape)| {
            !session
                .outputs
                .iter()
                .any(|output| output.name == *name && matches(&output.output_type, *ty, shape))
        })
    {
        return Err(EngineError::ModelContract);
    }
    Ok(())
}

/// Validates all graph metadata before accepting a loaded engine.
pub(super) fn validate(
    preprocessor: &Session,
    encoder: &Session,
    decoder: &Session,
    vocabulary: usize,
) -> Result<(), EngineError> {
    check(
        preprocessor,
        &[
            ("waveforms", Ty::Float32, &[1, -1]),
            ("waveforms_lens", Ty::Int64, &[1]),
        ],
        &[
            ("features", Ty::Float32, &[1, 128, -1]),
            ("features_lens", Ty::Int64, &[1]),
        ],
    )?;
    check(
        encoder,
        &[
            ("audio_signal", Ty::Float32, &[1, 128, -1]),
            ("length", Ty::Int64, &[1]),
        ],
        &[
            ("outputs", Ty::Float32, &[1, 1024, -1]),
            ("encoded_lengths", Ty::Int64, &[1]),
        ],
    )?;
    check(
        decoder,
        &[
            ("encoder_outputs", Ty::Float32, &[1, 1024, 1]),
            ("targets", Ty::Int32, &[1, 1]),
            ("target_length", Ty::Int32, &[1]),
            ("input_states_1", Ty::Float32, &[2, 1, 640]),
            ("input_states_2", Ty::Float32, &[2, 1, 640]),
        ],
        &[
            ("outputs", Ty::Float32, &[1, 1, 1, (vocabulary + 5) as i64]),
            ("prednet_lengths", Ty::Int32, &[1]),
            ("output_states_1", Ty::Float32, &[2, 1, 640]),
            ("output_states_2", Ty::Float32, &[2, 1, 640]),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Rejects wrong rank, dtype, and fixed dimensions without allocating ORT tensors.
    #[test]
    fn metadata_contract() {
        let value = ValueType::Tensor {
            ty: Ty::Float32,
            dimensions: vec![-1, 128, -1],
            dimension_symbols: vec![None; 3],
        };
        assert!(matches(&value, Ty::Float32, &[1, 128, -1]));
        assert!(!matches(&value, Ty::Int64, &[1, 128, -1]));
        assert!(!matches(&value, Ty::Float32, &[1, 80, -1]));
        assert!(!matches(&value, Ty::Float32, &[1, 128]));
    }
}
