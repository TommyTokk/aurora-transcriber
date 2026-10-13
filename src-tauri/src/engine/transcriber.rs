//! Resident GGUF weights with scoped, serial per-file native inference.
use super::{
    providers, resources, AsrSelection, DeviceSnapshot, EngineError, FallbackReason, Provider,
};
use std::path::Path;
use transcribe_cpp::{CancelToken, Feature, Model, RunOptions, Session, TimestampKind};

const MAX_SAMPLES: usize = 25 * 16_000;

/// Owns verified Parakeet weights; no session or transcript survives a file operation.
pub struct TranscriberEngine {
    model: Model,
    device: DeviceSnapshot,
    options: transcribe_cpp::ModelOptions,
    fallback_reason: Option<FallbackReason>,
}

/// Owns one file's native results and context; dropping it releases session resources.
pub struct FileTranscriber {
    session: Session,
}

impl TranscriberEngine {
    /// Loads the pinned local Q8_0 model on an automatic GPU with CPU recovery.
    ///
    /// Directories must be absolute and contain the verified GGUF filename.
    /// Tauri constructs this on a blocking worker and retains it in application
    /// state until inactivity, a folder change, or shutdown.
    pub fn new(model_dir: &Path) -> Result<Self, EngineError> {
        Self::new_with_selection(model_dir, &AsrSelection::Auto {})
    }

    /// Loads on ordered GPU candidates with CPU recovery, entirely offline.
    ///
    /// Failed attempts release model and session allocations before the next GPU
    /// is tried. Explicit GPU choices try only that GPU before CPU recovery.
    pub fn new_with_selection(
        model_dir: &Path,
        selection: &AsrSelection,
    ) -> Result<Self, EngineError> {
        let candidates = providers::options(selection)?;
        resources::validate(model_dir)?;
        let (mut engine, fallback_reason) =
            providers::try_candidates(&candidates, selection, |options| {
                let model = Model::load_with(model_dir.join(resources::MODEL_FILE), options)
                    .map_err(map_native_error)?;
                validate_contract(
                    &model.arch(),
                    &model.variant(),
                    model.capabilities().native_sample_rate,
                )?;
                let device = providers::verify(&model, options)?;
                // Check session allocation before readiness, without retaining native results.
                let session = model.session().map_err(map_native_error)?;
                validate_session_limit(
                    session
                        .limits()
                        .map_err(map_native_error)?
                        .effective_max_audio_ms,
                )?;
                drop(session);
                Ok(Self {
                    model,
                    device,
                    options: options.clone(),
                    fallback_reason: None,
                })
            })?;
        engine.fallback_reason = fallback_reason;
        Ok(engine)
    }

    /// Reports safe CPU recovery diagnostics for these resident weights.
    pub fn fallback_reason(&self) -> Option<FallbackReason> {
        self.fallback_reason
    }

    /// Reports only the backend verified during model initialization.
    pub fn provider(&self) -> Provider {
        self.device.backend
    }

    /// Reports the verified process-local device without exposing model or audio paths.
    pub fn device(&self) -> &DeviceSnapshot {
        &self.device
    }

    /// Opens a scoped session reused by every chunk of one file.
    ///
    /// Ownership frees the session on completion, errors, or unwinding while
    /// keeping resident weights alive. A nested result preserves caller errors.
    pub fn with_file<T>(
        &mut self,
        operation: impl FnOnce(&mut FileTranscriber) -> T,
    ) -> Result<T, EngineError> {
        self.with_file_cancellable(&CancelToken::new(), operation)
    }

    /// Opens a file session with job-scoped native cancellation when supported by the model.
    pub(crate) fn with_file_cancellable<T>(
        &mut self,
        cancel: &CancelToken,
        operation: impl FnOnce(&mut FileTranscriber) -> T,
    ) -> Result<T, EngineError> {
        providers::verify(&self.model, &self.options)?;
        let mut file = FileTranscriber {
            session: self.model.session().map_err(map_native_error)?,
        };
        validate_session_limit(
            file.session
                .limits()
                .map_err(map_native_error)?
                .effective_max_audio_ms,
        )?;
        if self.model.supports(Feature::Cancellation) {
            file.session.set_cancel_token(cancel);
        }
        let result = operation(&mut file);
        drop(file);
        Ok(result)
    }

    /// Transcribes one standalone chunk and frees its native session before returning.
    ///
    /// Multi-chunk consumers use with_file to reuse a session. Empty or malformed
    /// audio is handled before allocating a session; weights are never reloaded here.
    pub fn transcribe_chunk(&mut self, samples: &[f32]) -> Result<String, EngineError> {
        validate_samples(samples)?;
        if samples.is_empty() {
            return Ok(String::new());
        }
        self.with_file(|file| file.transcribe_chunk(samples))?
    }
}

impl FileTranscriber {
    /// Runs an independent offline chunk without prompting or native timestamps.
    ///
    /// The synchronous call borrows audio. Only owned text moves to the caller;
    /// unused token/result copies drop here. Native results are replaced on the
    /// next run and released when the file session ends.
    pub fn transcribe_chunk(&mut self, samples: &[f32]) -> Result<String, EngineError> {
        validate_samples(samples)?;
        if samples.is_empty() {
            return Ok(String::new());
        }
        validate_session_limit(
            self.session
                .limits()
                .map_err(map_native_error)?
                .effective_max_audio_ms,
        )?;
        let options = RunOptions {
            timestamps: TimestampKind::None,
            ..RunOptions::default()
        };
        let result = self
            .session
            .run(samples, &options)
            .map_err(map_native_error)?;
        Ok(result.text)
    }
}

/// Rejects sessions too small for unchanged VAD windows; zero denotes no practical limit.
fn validate_session_limit(max_audio_ms: i64) -> Result<(), EngineError> {
    if max_audio_ms < 0 || (max_audio_ms > 0 && max_audio_ms < 25_000) {
        return Err(EngineError::SessionLimits);
    }
    Ok(())
}

/// Rejects incompatible model metadata without inventing timing or language information.
fn validate_contract(arch: &str, variant: &str, sample_rate: i32) -> Result<(), EngineError> {
    if arch != "parakeet" || variant != "tdt-0.6b-v3" || sample_rate != 16_000 {
        return Err(EngineError::ModelContract);
    }
    Ok(())
}

/// Validates bounded finite mono samples before crossing the native boundary.
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

/// Drops native error detail and partial transcripts instead of exposing them to IPC.
fn map_native_error(error: transcribe_cpp::Error) -> EngineError {
    match error {
        transcribe_cpp::Error::Aborted { .. } => EngineError::Cancelled,
        transcribe_cpp::Error::Backend(_) => EngineError::GpuUnavailable,
        transcribe_cpp::Error::OutOfMemory(_) => EngineError::OutOfMemory,
        transcribe_cpp::Error::ModelFileNotFound(_) => EngineError::Resources,
        transcribe_cpp::Error::ModelLoad(_) => EngineError::ModelLoad,
        transcribe_cpp::Error::VersionMismatch(_) | transcribe_cpp::Error::BadStructSize(_) => {
            EngineError::NativeCompatibility
        }
        transcribe_cpp::Error::InputTooLong(_) => EngineError::InvalidSamples,
        _ => EngineError::Inference,
    }
}

#[cfg(test)]
mod tests;
