//! Immutable export identity and streaming resource integrity checks.
use super::EngineError;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

/// Official Handy conversion revision, confirmed through Hugging Face model metadata.
pub const REVISION: &str = "90f082450fcbacdb54e5900c44ef697c9ea59622";
/// Repository containing transcribe.cpp-compatible multilingual Parakeet weights.
pub(crate) const REPOSITORY: &str = "handy-computer/parakeet-tdt-0.6b-v3-gguf";
/// The sole ASR resource required by this milestone.
pub(crate) const MODEL_FILE: &str = "parakeet-tdt-0.6b-v3-Q8_0.gguf";
/// SHA-256 of the complete GGUF, distinct from its Xet storage hash.
pub(crate) const MODEL_HASH: &str =
    "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7";
/// Immutable ASR download manifest; legacy ONNX files are neither required nor deleted.
pub(crate) const FILES: &[(&str, &str)] = &[(MODEL_FILE, MODEL_HASH)];

/// Verifies the fixed local GGUF before native parsing or weight allocation.
pub(crate) fn validate(dir: &Path) -> Result<(), EngineError> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(EngineError::Configuration);
    }
    verify_file(&dir.join(MODEL_FILE), MODEL_HASH)
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
