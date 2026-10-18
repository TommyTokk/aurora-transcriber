//! Offline selectable GGUF inference with explicit devices and per-file sessions.

#[cfg(all(
    any(feature = "asr-cuda", feature = "asr-vulkan"),
    not(any(target_os = "windows", target_os = "linux"))
))]
compile_error!("ASR CUDA/Vulkan features are supported only on Windows/Linux.");

pub(crate) mod fetcher;
pub(crate) mod management;
pub(crate) mod manifest;
mod model_files;
mod providers;
pub(crate) mod resources;
pub(crate) mod state;
mod transcriber;

pub use manifest::{AsrModelId, ModelDescriptor, ModelFormat, ModelPurpose};
pub(crate) use providers::initialize as initialize_native;
pub use providers::{discover_devices, AsrSelection, DeviceSnapshot, FallbackReason, Provider};
pub use resources::REVISION as MODEL_REVISION;
pub(crate) use state::start as state_start;
pub use state::{transcriber_state, EngineSnapshot, EngineState};
use thiserror::Error;
pub use transcriber::{FileTranscriber, TranscriberEngine};

/// Safe domain messages; underlying paths and runtime diagnostics stay out of IPC.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Transcription cancelled.")]
    Cancelled,
    #[error("The ASR session cannot process the required 25-second audio chunks.")]
    SessionLimits,
    #[error("Configure an absolute local model directory containing the supported models.")]
    Configuration,
    #[error("Local model verification failed. Download models to repair the installation.")]
    Resources,
    #[error("The GGUF model does not match the selected ASR contract.")]
    ModelContract,
    #[error("Audio must be finite mono 16 kHz samples within [-1, 1], at most 25 seconds.")]
    InvalidSamples,
    #[error("The transcription model returned invalid output.")]
    InvalidOutput,
    #[error(
        "The requested ASR GPU could not be initialized. Check GPU drivers and build backends."
    )]
    GpuUnavailable,
    #[error("The selected ASR device is unavailable. Select an available device or Auto.")]
    SelectionUnavailable,
    #[error(
        "The ASR model could not be initialized. Check available memory and repair the models."
    )]
    ModelLoad,
    #[error(
        "The native ASR library is incompatible with this application. Repair the installation."
    )]
    NativeCompatibility,
    #[error("No compatible ASR GPU could be initialized and no CPU device is registered.")]
    CpuFallbackUnavailable,
    #[error("Automatic CPU recovery failed: {0}")]
    CpuFallbackFailed(#[source] Box<EngineError>),
    #[error("Local transcription failed. Check the selected device or retry.")]
    Inference,
    #[error("There is insufficient memory to run the local transcription model.")]
    OutOfMemory,
    #[error("The transcription engine is unavailable.")]
    Unavailable,
}
