//! Local audio decoding and sample-rate conversion for the transcription pipeline.

mod decoder;
mod resampler;

pub use decoder::{AudioDecodeError, AudioDecoder};

/// The sample rate expected by downstream speech processing.
pub const TARGET_SAMPLE_RATE: usize = 16_000;
