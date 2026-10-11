//! Offline Parakeet GGUF inference with explicit devices and per-file sessions.

#[cfg(all(
    any(feature = "asr-cuda", feature = "asr-vulkan"),
    not(any(target_os = "windows", target_os = "linux"))
))]
compile_error!("ASR CUDA/Vulkan features are supported only on Windows/Linux.");

pub(crate) mod fetcher;
pub(crate) mod manifest;
mod providers;
pub(crate) mod resources;
pub(crate) mod runtime;
pub(crate) mod state;
mod transcriber;

pub use manifest::{ModelDescriptor, ModelFormat, ModelPurpose};
pub(crate) use providers::initialize as initialize_native;
pub use providers::{discover_devices, AsrSelection, DeviceSnapshot, Provider};
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
        "No compatible ASR GPU could be initialized. Check GPU drivers and build backends, select another GPU, or explicitly choose CPU."
    )]
    GpuUnavailable,
    #[error("The selected ASR device is unavailable. Select an available device or Auto.")]
    SelectionUnavailable,
    #[error(
        "The ASR model could not be initialized. Check available memory and repair the models."
    )]
    ModelLoad,
    #[error("Local transcription failed. Check the selected device or retry.")]
    Inference,
    #[error("There is insufficient memory to run the local transcription model.")]
    OutOfMemory,
    #[error("The transcription engine is unavailable.")]
    Unavailable,
}
