use std::path::Path;

use ort::{
    execution_providers::CPUExecutionProvider,
    session::Session,
    tensor::TensorElementType,
    value::{Tensor, ValueType},
};

use super::{smart_chunk::Segmenter, AudioChunk, VadError};

const FRAME: usize = 512;
const CONTEXT: usize = 64;
const STATE: usize = 256;

/// Reusable CPU-only Silero v5.1 session, containing no per-file audio state.
pub struct SileroVad {
    session: Session,
}

impl SileroVad {
    /// Loads a local Silero v5.1 model once and validates its tensor contract.
    ///
    /// Set `ORT_DYLIB_PATH` to ONNX Runtime 1.20.x before the first ORT call.
    /// The pinned ORT dependency can panic internally for a missing or incompatible
    /// runtime. No model or runtime is downloaded. Future Tauri callers must retain
    /// this session in application state and initialize it on a blocking worker.
    pub fn new(model_path: &Path) -> Result<Self, VadError> {
        if !model_path.metadata()?.is_file() {
            return Err(VadError::ModelNotFile);
        }
        let session = Session::builder()?
            // The default CPU provider disables its arena allocator.
            .with_execution_providers([CPUExecutionProvider::default().build()])?
            .with_intra_threads(1)?
            .with_inter_threads(1)?
            // Also disable memory-pattern reuse for transient tensor allocations.
            .with_memory_pattern(false)?
            .commit_from_file(model_path)?;
        validate_contract(&session)?;
        Ok(Self { session })
    }

    /// Segments borrowed, finite 16 kHz mono audio without copying the full file.
    ///
    /// The caller owns and drops the audio after processing. This synchronous
    /// method must run through `spawn_blocking` when used by Tauri commands.
    /// Context, recurrent state, and tensors are scoped to this call, including
    /// error paths, so repeated calls cannot share a previous file's speech state.
    pub fn chunk_audio<'a>(&mut self, samples: &'a [f32]) -> Result<Vec<AudioChunk<'a>>, VadError> {
        validate_samples(samples)?;
        let mut segmenter = Segmenter::default();
        let mut context = [0.0_f32; CONTEXT];
        let mut state = [0.0_f32; STATE];
        for (index, frame) in samples.chunks(FRAME).enumerate() {
            // Only the final inference frame is padded; timestamps use real data.
            let input = prepare_frame(frame, &mut context);
            let outputs = self.session.run(ort::inputs![
                "input" => Tensor::from_array(([1, CONTEXT + FRAME], input))?,
                "state" => Tensor::from_array(([2, 1, 128], state.to_vec()))?,
                "sr" => Tensor::from_array((Vec::<usize>::new(), vec![16_000_i64]))?
            ]?)?;
            let (shape, probability) = outputs
                .get("output")
                .ok_or(VadError::InvalidOutput)?
                .try_extract_raw_tensor::<f32>()
                .map_err(|_| VadError::InvalidOutput)?;
            if shape != [1, 1] || probability.len() != 1 {
                return Err(VadError::InvalidOutput);
            }
            let (shape, next_state) = outputs
                .get("stateN")
                .ok_or(VadError::InvalidOutput)?
                .try_extract_raw_tensor::<f32>()
                .map_err(|_| VadError::InvalidOutput)?;
            if shape != [2, 1, 128]
                || next_state.len() != STATE
                || next_state.iter().any(|value| !value.is_finite())
            {
                return Err(VadError::InvalidOutput);
            }
            state.copy_from_slice(next_state);
            let start = index * FRAME;
            segmenter.push(probability[0], start..start + frame.len())?;
        }
        Ok(segmenter.finish(samples))
    }
}

/// Prepends retained context, pads a frame, and retains its last 64 input samples.
fn prepare_frame(frame: &[f32], context: &mut [f32; CONTEXT]) -> Vec<f32> {
    let mut input = vec![0.0_f32; CONTEXT + FRAME];
    input[..CONTEXT].copy_from_slice(context);
    input[CONTEXT..CONTEXT + frame.len()].copy_from_slice(frame);
    context.copy_from_slice(&input[FRAME..]);
    input
}

/// Rejects invalid samples before any tensor allocation or inference.
fn validate_samples(samples: &[f32]) -> Result<(), VadError> {
    if samples
        .iter()
        .any(|value| !value.is_finite() || !(-1.0..=1.0).contains(value))
    {
        return Err(VadError::InvalidSamples);
    }
    Ok(())
}

/// Matches fixed dimensions and permits ONNX's explicitly dynamic dimensions.
fn matches_tensor(value: &ValueType, ty: TensorElementType, expected: &[i64]) -> bool {
    matches!(value, ValueType::Tensor { ty: actual, dimensions, .. }
        if *actual == ty && dimensions.len() == expected.len()
            && dimensions.iter().zip(expected).all(|(actual, expected)|
                *actual == -1 || actual == expected))
}

/// Validates named model inputs and outputs before any audio is processed.
fn validate_contract(session: &Session) -> Result<(), VadError> {
    let inputs = [
        ("input", TensorElementType::Float32, &[1, 576][..]),
        ("state", TensorElementType::Float32, &[2, 1, 128][..]),
        ("sr", TensorElementType::Int64, &[][..]),
    ];
    let outputs = [
        ("output", TensorElementType::Float32, &[1, 1][..]),
        ("stateN", TensorElementType::Float32, &[2, 1, 128][..]),
    ];
    if session.inputs.len() != inputs.len()
        || session.outputs.len() != outputs.len()
        || inputs.iter().any(|(name, ty, shape)| {
            !session
                .inputs
                .iter()
                .any(|input| input.name == *name && matches_tensor(&input.input_type, *ty, shape))
        })
        || outputs.iter().any(|(name, ty, shape)| {
            !session.outputs.iter().any(|output| {
                output.name == *name && matches_tensor(&output.output_type, *ty, shape)
            })
        })
    {
        return Err(VadError::ModelContract);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
