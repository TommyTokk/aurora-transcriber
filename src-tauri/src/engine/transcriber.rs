//! Reusable ONNX graphs with per-chunk predictor state and transient tensors.
use super::{
    contract, providers, resources, runtime,
    tdt::{self, Step},
    vocabulary::Vocabulary,
    EngineError, Provider,
};
use ort::{
    session::Session,
    value::{DynValue, Tensor},
};
use std::path::Path;

const STATE_SIZE: usize = 2 * 640;
const ENCODER_WIDTH: usize = 1024;
const MAX_SAMPLES: usize = 25 * 16_000;

/// Loads each graph once; no file audio, recurrent state, or emitted tokens persist.
pub struct TranscriberEngine {
    preprocessor: Session,
    encoder: Session,
    decoder: Session,
    vocabulary: Vocabulary,
    provider: Provider,
}

impl TranscriberEngine {
    /// Loads the pinned local export using platform acceleration with CPU fallback.
    ///
    /// Requires absolute `ORT_DYLIB_PATH` pointing to trusted ONNX Runtime 1.20.1.
    /// This synchronous operation must run on a blocking worker in Tauri.
    ///
    /// ```no_run
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// use aurora_lib::{audio::AudioDecoder, engine::TranscriberEngine};
    /// use std::path::Path;
    /// let mut engine = TranscriberEngine::new(Path::new("/absolute/model/directory"))?;
    /// let samples = AudioDecoder::decode_to_16khz_mono(Path::new("/native/selected/chunk.wav"))?;
    /// let text = engine.transcribe_chunk(&samples)?;
    /// drop(samples);
    /// // The caller decides how long to retain the resulting transcript.
    /// drop(text);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(model_dir: &Path) -> Result<Self, EngineError> {
        Self::load(model_dir, false)
    }

    /// Loads the same export exclusively on CPU for reproducible integration tests.
    pub fn new_cpu(model_dir: &Path) -> Result<Self, EngineError> {
        Self::load(model_dir, true)
    }

    /// Validates resources before initializing any heavy model session.
    fn load(model_dir: &Path, cpu_only: bool) -> Result<Self, EngineError> {
        resources::validate(model_dir)?;
        let vocabulary = Vocabulary::parse(
            &std::fs::read_to_string(model_dir.join("vocab.txt"))
                .map_err(|_| EngineError::Resources)?,
        )?;
        if vocabulary.len() != 8193 || vocabulary.blank != 8192 {
            return Err(EngineError::Vocabulary);
        }
        runtime::initialize()?;
        let preprocessor = providers::session(&model_dir.join("nemo128.onnx"), Provider::Cpu)?;
        let (encoder, decoder, provider) = providers::load_pair(model_dir, cpu_only)?;
        contract::validate(&preprocessor, &encoder, &decoder, vocabulary.len())?;
        Ok(Self {
            preprocessor,
            encoder,
            decoder,
            vocabulary,
            provider,
        })
    }

    /// Reports the configured ASR provider without claiming all operators use it.
    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// Transcribes finite mono 16 kHz audio, at most 25 seconds, on a blocking worker.
    ///
    /// Every call resets predictor state. Empty audio returns empty text without
    /// inference. Callers retain ownership of their audio and must drop it afterward.
    pub fn transcribe_chunk(&mut self, samples: &[f32]) -> Result<String, EngineError> {
        validate_samples(samples)?;
        if samples.is_empty() {
            return Ok(String::new());
        }
        // rc.9's safe tensor constructor copies borrowed slices. Keep this bounded
        // to one chunk, and never retain input tensors in application state.
        let features = self.preprocessor.run(ort::inputs![
            "waveforms" => Tensor::from_array(([1, samples.len()], samples))?,
            "waveforms_lens" => Tensor::from_array(([1], vec![samples.len() as i64]))?
        ]?)?;
        let feature_tensor = features.get("features").ok_or(EngineError::InvalidOutput)?;
        let (shape, values) = feature_tensor.try_extract_raw_tensor::<f32>()?;
        validate_matrix(shape, values, 128)?;
        let feature_len = features
            .get("features_lens")
            .ok_or(EngineError::InvalidOutput)?;
        validate_length(feature_len, shape[2] as usize)?;
        let encoded = self.encoder.run(vec![
            (
                "audio_signal",
                ort::session::SessionInputValue::from(feature_tensor.view()),
            ),
            (
                "length",
                ort::session::SessionInputValue::from(feature_len.view()),
            ),
        ])?;
        drop(features);
        let encoded_tensor = encoded.get("outputs").ok_or(EngineError::InvalidOutput)?;
        let (shape, values) = encoded_tensor.try_extract_raw_tensor::<f32>()?;
        validate_matrix(shape, values, ENCODER_WIDTH)?;
        let width = shape[2] as usize;
        let frames = validate_length(
            encoded
                .get("encoded_lengths")
                .ok_or(EngineError::InvalidOutput)?,
            width,
        )?;
        let decoder = &mut self.decoder;
        let vocab_size = self.vocabulary.len();
        let tokens = tdt::decode(
            frames,
            self.vocabulary.blank,
            (vec![0.0; STATE_SIZE], vec![0.0; STATE_SIZE]),
            |time, previous, state| {
                // The encoder returns [1, channels, time]; gather only the current
                // frame instead of copying or transposing the full encoded chunk.
                let frame: Vec<f32> = (0..ENCODER_WIDTH)
                    .map(|channel| values[channel * width + time])
                    .collect();
                let outputs = decoder.run(ort::inputs![
                    "encoder_outputs" => Tensor::from_array(([1, ENCODER_WIDTH, 1], frame))?,
                    "targets" => Tensor::from_array(([1, 1], vec![previous as i32]))?,
                    "target_length" => Tensor::from_array(([1], vec![1_i32]))?,
                    "input_states_1" => Tensor::from_array(([2, 1, 640], state.0.as_slice()))?,
                    "input_states_2" => Tensor::from_array(([2, 1, 640], state.1.as_slice()))?
                ]?)?;
                let (shape, logits) = outputs
                    .get("outputs")
                    .ok_or(EngineError::InvalidOutput)?
                    .try_extract_raw_tensor::<f32>()?;
                if shape != [1, 1, 1, (vocab_size + 5) as i64] || logits.len() != vocab_size + 5 {
                    return Err(EngineError::InvalidOutput);
                }
                let token = tdt::argmax(&logits[..vocab_size])?;
                let duration = tdt::argmax(&logits[vocab_size..])?;
                let (shape, lengths) = outputs
                    .get("prednet_lengths")
                    .ok_or(EngineError::InvalidOutput)?
                    .try_extract_raw_tensor::<i32>()?;
                if shape != [1] || lengths != [1] {
                    return Err(EngineError::InvalidOutput);
                }
                let next_state = (
                    extract_state(
                        outputs
                            .get("output_states_1")
                            .ok_or(EngineError::InvalidOutput)?,
                    )?,
                    extract_state(
                        outputs
                            .get("output_states_2")
                            .ok_or(EngineError::InvalidOutput)?,
                    )?,
                );
                Ok(Step {
                    token,
                    duration,
                    state: next_state,
                })
            },
        )?;
        let text = self.vocabulary.decode(&tokens)?;
        drop(tokens);
        drop(encoded);
        Ok(text)
    }
}

/// Validates the public audio invariant before allocating tensors.
fn validate_samples(samples: &[f32]) -> Result<(), EngineError> {
    if samples.len() > MAX_SAMPLES
        || samples
            .iter()
            .any(|value| !value.is_finite() || !(-1.0..=1.0).contains(value))
    {
        return Err(EngineError::InvalidSamples);
    }
    Ok(())
}

/// Checks matrix rank, finite values, and storage size before indexing features.
fn validate_matrix(shape: &[i64], values: &[f32], channels: usize) -> Result<(), EngineError> {
    if shape.len() != 3
        || shape[0] != 1
        || shape[1] != channels as i64
        || shape[2] <= 0
        || channels.checked_mul(shape[2] as usize) != Some(values.len())
        || values.iter().any(|value| !value.is_finite())
    {
        return Err(EngineError::InvalidOutput);
    }
    Ok(())
}

/// Checks a scalar batch length before indexing or advancing through encoded data.
fn validate_length(tensor: &DynValue, maximum: usize) -> Result<usize, EngineError> {
    let (shape, lengths) = tensor.try_extract_raw_tensor::<i64>()?;
    validate_length_data(shape, lengths, maximum)
}

/// Validates batch lengths without requiring model resources in deterministic tests.
fn validate_length_data(
    shape: &[i64],
    lengths: &[i64],
    maximum: usize,
) -> Result<usize, EngineError> {
    if shape != [1] || lengths.len() != 1 || lengths[0] <= 0 || lengths[0] as u64 > maximum as u64 {
        return Err(EngineError::InvalidOutput);
    }
    Ok(lengths[0] as usize)
}

/// Copies only the small next predictor state after shape and finiteness checks.
fn extract_state(tensor: &DynValue) -> Result<Vec<f32>, EngineError> {
    let (shape, values) = tensor.try_extract_raw_tensor::<f32>()?;
    if shape != [2, 1, 640]
        || values.len() != STATE_SIZE
        || values.iter().any(|value| !value.is_finite())
    {
        return Err(EngineError::InvalidOutput);
    }
    Ok(values.to_vec())
}

#[cfg(test)]
mod tests;
