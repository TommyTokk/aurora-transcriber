//! Immutable export identity and streaming resource integrity checks.
use super::EngineError;
use std::path::Path;

/// Official Handy conversion revision, confirmed through Hugging Face model metadata.
pub const REVISION: &str = super::manifest::PARAKEET.revision;
/// Legacy Parakeet filename retained by compatibility and local resource tests.
#[cfg(test)]
pub(crate) const MODEL_FILE: &str = super::manifest::PARAKEET.filename;

/// Verifies the fixed local GGUF before native parsing or weight allocation.
#[cfg(test)]
pub(crate) fn validate(dir: &Path) -> Result<(), EngineError> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(EngineError::Configuration);
    }
    verify_file(&dir.join(MODEL_FILE), super::manifest::PARAKEET.sha256)
}

/// Rejects missing, non-file, truncated, or modified resources without logging paths.
pub(crate) fn verify_file(path: &Path, expected: &str) -> Result<(), EngineError> {
    crate::resources::verify_file(path, expected).map_err(|_| EngineError::Resources)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Detects resource modification before native model loading.
    #[test]
    fn integrity_and_missing_resources() -> anyhow::Result<()> {
        let file = tempfile::NamedTempFile::new()?;
        std::fs::write(file.path(), b"abc")?;
        verify_file(
            file.path(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )?;
        std::fs::write(file.path(), b"ab")?;
        assert!(verify_file(
            file.path(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        )
        .is_err());
        assert!(validate(tempfile::tempdir()?.path()).is_err());
        assert_eq!(REVISION.len(), 40);
        Ok(())
    }
}
