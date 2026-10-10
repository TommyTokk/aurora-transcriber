//! Immutable export identity and streaming resource integrity checks.
use super::EngineError;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

/// Hugging Face revision of istupakov/parakeet-tdt-0.6b-v3-onnx.
pub const REVISION: &str = "8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce";
/// Required FP32 files and their SHA-256 hashes at the pinned revision.
const FILES: &[(&str, &str)] = &[
    (
        "encoder-model.onnx",
        "98a74b21b4cc0017c1e7030319a4a96f4a9506e50f0708f3a516d02a77c96bb1",
    ),
    (
        "encoder-model.onnx.data",
        "9a22d372c51455c34f13405da2520baefb7125bd16981397561423ed32d24f36",
    ),
    (
        "decoder_joint-model.onnx",
        "e978ddf6688527182c10fde2eb4b83068421648985ef23f7a86be732be8706c1",
    ),
    (
        "nemo128.onnx",
        "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
    ),
    (
        "config.json",
        "666903c76b9798caf2c210afd4f6cd60b08a8dbf9800ec8d7a3bc0d2148ac466",
    ),
    (
        "vocab.txt",
        "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    ),
];

/// Export metadata needed to reject incompatible preprocessing and decoding.
#[derive(Deserialize)]
struct Config {
    model_type: String,
    features_size: usize,
    subsampling_factor: usize,
}

/// Streams weights through a bounded buffer and validates pinned model metadata.
pub(super) fn validate(dir: &Path) -> Result<(), EngineError> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(EngineError::Configuration);
    }
    for &(name, expected) in FILES {
        verify_file(&dir.join(name), expected)?;
    }
    let config: Config = serde_json::from_slice(
        &std::fs::read(dir.join("config.json")).map_err(|_| EngineError::Resources)?,
    )
    .map_err(|_| EngineError::Resources)?;
    if config.model_type != "nemo-conformer-tdt"
        || config.features_size != 128
        || config.subsampling_factor != 8
    {
        return Err(EngineError::ModelContract);
    }
    Ok(())
}

/// Rejects missing, non-file, truncated, or modified resources without logging paths.
fn verify_file(path: &Path, expected: &str) -> Result<(), EngineError> {
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
    /// Detects resource modification independently of ONNX model loading.
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
