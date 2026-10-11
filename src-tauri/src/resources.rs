//! Runtime-independent local resource checks without logging private paths.
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// Reads a backend-owned absolute environment path without exposing it to IPC.
pub(crate) fn absolute_env(name: &str) -> std::io::Result<PathBuf> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or(std::io::ErrorKind::NotFound)?;
    if !path.is_absolute() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    Ok(path)
}

/// Streams a file's SHA-256 using bounded memory and rejects changed or absent resources.
pub(crate) fn verify_file(path: &Path, expected: &str) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
    }
    Ok(())
}
