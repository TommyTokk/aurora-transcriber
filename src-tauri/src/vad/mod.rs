//! Offline voice detection and borrowed, silence-aware transcription chunks.

mod silero;
mod smart_chunk;

pub use silero::SileroVad;
use thiserror::Error;

/// A contiguous view of the caller's audio with original-file timestamps.
///
/// This type deliberately omits `Debug` and serialization to avoid exposing audio.
pub struct AudioChunk<'a> {
    /// Borrows the contiguous original-file samples included in this chunk.
    pub samples: &'a [f32],
    /// Marks the inclusive start boundary in original-file seconds.
    pub start_sec: f64,
    /// Marks the exclusive end boundary in original-file seconds.
    pub end_sec: f64,
}

/// Failures in local model loading, input validation, or VAD inference.
#[derive(Debug, Error)]
pub enum VadError {
    #[error("Unable to access the local VAD model")]
    ModelIo(#[from] std::io::Error),
    #[error("The VAD model path must refer to a file")]
    ModelNotFile,
    #[error("The model does not match the Silero v5.1 ONNX contract")]
    ModelContract,
    #[error("Audio samples must be finite and within [-1, 1]")]
    InvalidSamples,
    #[error("The VAD model returned invalid probabilities or recurrent state")]
    InvalidOutput,
    #[error("VAD initialization or inference failed")]
    Runtime(#[from] ort::Error),
}
