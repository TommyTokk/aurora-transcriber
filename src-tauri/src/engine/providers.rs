//! Platform-specific session construction with explicit, fallible fallback.

use super::EngineError;
use ort::{
    execution_providers::{CPUExecutionProvider, ExecutionProviderDispatch},
    session::Session,
};
use serde::Serialize;
use std::path::Path;

/// Requested ASR provider; individual unsupported operators can still run on CPU.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Provider {
    Cpu,
    #[cfg(target_os = "macos")]
    CoreMl,
    #[cfg(target_os = "windows")]
    DirectMl,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    Cuda,
}

impl Provider {
    /// Creates a provider dispatch that reports registration failures to callers.
    fn dispatch(self) -> ExecutionProviderDispatch {
        match self {
            Self::Cpu => CPUExecutionProvider::default().build(),
            #[cfg(target_os = "macos")]
            Self::CoreMl => ort::execution_providers::CoreMLExecutionProvider::default().build(),
            #[cfg(target_os = "windows")]
            Self::DirectMl => {
                ort::execution_providers::DirectMLExecutionProvider::default().build()
            }
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            Self::Cuda => ort::execution_providers::CUDAExecutionProvider::default().build(),
        }
        .error_on_failure()
    }
}

/// Lists compiled candidates in the user-approved platform priority order.
fn candidates() -> &'static [Provider] {
    &[
        #[cfg(target_os = "macos")]
        Provider::CoreMl,
        #[cfg(target_os = "windows")]
        Provider::DirectMl,
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        Provider::Cuda,
        Provider::Cpu,
    ]
}

/// Builds a session without reusable transient CPU tensor arenas or patterns.
pub(super) fn session(path: &Path, provider: Provider) -> Result<Session, EngineError> {
    let session = Session::builder()?
        .with_execution_providers([provider.dispatch(), CPUExecutionProvider::default().build()])?
        .with_parallel_execution(false)?
        .with_memory_pattern(false)?
        .with_intra_threads(std::thread::available_parallelism().map_or(1, usize::from))?
        .with_inter_threads(1)?
        .commit_from_file(path)?;
    Ok(session)
}

/// Attempts complete encoder/decoder pairs, dropping partial pairs before retry.
pub(super) fn load_pair(
    dir: &Path,
    cpu_only: bool,
) -> Result<(Session, Session, Provider), EngineError> {
    let candidates = if cpu_only {
        &[Provider::Cpu][..]
    } else {
        candidates()
    };
    try_candidates(candidates, |provider| {
        let encoder = session(&dir.join("encoder-model.onnx"), provider)?;
        let decoder = session(&dir.join("decoder_joint-model.onnx"), provider)?;
        Ok((encoder, decoder, provider))
    })
}

/// Tries providers sequentially without printing runtime errors or resource paths.
fn try_candidates<T>(
    candidates: &[Provider],
    mut attempt: impl FnMut(Provider) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    for &provider in candidates {
        match attempt(provider) {
            Ok(value) => return Ok(value),
            Err(error) if provider == Provider::Cpu => return Err(error),
            Err(_) => eprintln!(
                "AURORA: {provider:?} session initialization failed; trying the next provider."
            ),
        }
    }
    Err(EngineError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Ensures every platform ends with CPU and retries failures in order.
    #[test]
    fn priority_and_fallback() -> anyhow::Result<()> {
        assert_eq!(candidates().last(), Some(&Provider::Cpu));
        let mut attempts = Vec::new();
        let result = try_candidates(candidates(), |provider| {
            attempts.push(provider);
            if provider == Provider::Cpu {
                Ok(provider)
            } else {
                Err(EngineError::Unavailable)
            }
        })?;
        assert_eq!(result, Provider::Cpu);
        assert_eq!(attempts, candidates());
        assert!(try_candidates::<()>(candidates(), |_| Err(EngineError::Unavailable)).is_err());
        Ok(())
    }
}
