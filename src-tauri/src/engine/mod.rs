//! Offline Parakeet TDT v3 inference and one-time native application state.

mod contract;
pub(crate) mod fetcher;
mod providers;
pub(crate) mod resources;
pub(crate) mod runtime;
pub(crate) mod state;
mod tdt;
mod transcriber;
mod vocabulary;

pub use providers::Provider;
pub use resources::REVISION as MODEL_REVISION;
pub(crate) use state::start as state_start;
pub use state::{transcriber_state, EngineSnapshot, EngineState};
use thiserror::Error;
pub use transcriber::TranscriberEngine;

/// Safe domain messages; underlying paths and runtime diagnostics stay out of IPC.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Configure a trusted absolute ORT_DYLIB_PATH and download the supported models.")]
    Configuration,
    #[error("The local model files do not match the supported Parakeet FP32 export.")]
    Resources,
    #[error("The model does not match the Parakeet TDT v3 tensor contract.")]
    ModelContract,
    #[error("The model vocabulary is invalid.")]
    Vocabulary,
    #[error("ONNX Runtime 1.20.1 could not be loaded or its API is incompatible.")]
    RuntimeLibrary,
    #[error("Audio must be finite mono 16 kHz samples within [-1, 1], at most 25 seconds.")]
    InvalidSamples,
    #[error("The transcription model returned invalid output.")]
    InvalidOutput,
    #[error("Local model initialization or inference failed.")]
    Inference(#[from] ort::Error),
    #[error("The transcription engine is unavailable.")]
    Unavailable,
}
