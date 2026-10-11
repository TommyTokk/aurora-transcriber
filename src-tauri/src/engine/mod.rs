//! Offline Parakeet GGUF inference with reusable Metal models and per-file sessions.

pub(crate) mod fetcher;
mod providers;
pub(crate) mod resources;
pub(crate) mod runtime;
pub(crate) mod state;
mod transcriber;

pub(crate) use providers::initialize as initialize_native;
pub use providers::Provider;
pub use resources::REVISION as MODEL_REVISION;
pub(crate) use state::start as state_start;
pub use state::{transcriber_state, EngineSnapshot, EngineState};
use thiserror::Error;
pub use transcriber::{FileTranscriber, TranscriberEngine};

/// Safe domain messages; underlying paths and runtime diagnostics stay out of IPC.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Configure an absolute local model directory containing the supported models.")]
    Configuration,
    #[error("Local model verification failed. Download models to repair the installation.")]
    Resources,
    #[error("The GGUF model does not match the supported Parakeet TDT v3 contract.")]
    ModelContract,
    #[error("ONNX Runtime 1.20.1 could not be loaded or its API is incompatible.")]
    RuntimeLibrary,
    #[error("Audio must be finite mono 16 kHz samples within [-1, 1], at most 25 seconds.")]
    InvalidSamples,
    #[error("The transcription model returned invalid output.")]
    InvalidOutput,
    #[error("The Silero ONNX runtime could not be initialized.")]
    OnnxRuntime(#[from] ort::Error),
    #[error(
        "Metal ASR is unavailable. A compatible Apple Silicon Mac is required in this release."
    )]
    GpuUnavailable,
    #[error(
        "The ASR model could not be initialized. Check available memory and repair the models."
    )]
    ModelLoad,
    #[error("Local GPU transcription failed.")]
    Inference,
    #[error("There is insufficient memory to run the local transcription model.")]
    OutOfMemory,
    #[error("The transcription engine is unavailable.")]
    Unavailable,
}
