//! Immutable export identity and streaming resource integrity checks.
use super::EngineError;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

/// Official Handy conversion revision, confirmed through Hugging Face model metadata.
pub const REVISION: &str = super::manifest::PARAKEET.revision;
/// The single supported ASR filename, shared with the native loader and local tools.
pub(crate) const MODEL_FILE: &str = super::manifest::PARAKEET.filename;

/// Verifies the fixed local GGUF before native parsing or weight allocation.
pub(crate) fn validate(dir: &Path) -> Result<(), EngineError> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(EngineError::Configuration);
    }
    verify_file(&dir.join(MODEL_FILE), super::manifest::PARAKEET.sha256)
}

/// Rejects missing, non-file, truncated, or modified resources without logging paths.
pub(crate) fn verify_file(path: &Path, expected: &str) -> Result<(), EngineError> {
    let mut file = File::open(path).map_err(|_| EngineError::Resources)?;
    if !file
        .metadata()
        .map_err(|_| EngineError::Resources)?
        .is_file()
    {
        return Err(EngineError::Resources);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = file.read(&mut buffer).map_err(|_| EngineError::Resources)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err(EngineError::Resources);
    }
    Ok(())
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
