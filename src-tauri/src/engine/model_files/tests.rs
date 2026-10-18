use super::*;
use crate::engine::manifest;

/// Removing weights is idempotent and preserves adjacent user files and other model caches.
#[test]
fn deletes_only_the_catalog_artifact_and_its_cache() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let model = &manifest::WHISPER_TURBO;
    let target = artifact(dir.path(), model).map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
    )?;
    std::fs::write(&target, b"weights")?;
    let unrelated = target.with_file_name("user-file");
    std::fs::write(&unrelated, b"keep")?;
    let cache = cache(dir.path(), model).map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(&cache)?;
    std::fs::write(cache.join("partial"), b"transfer")?;
    let other_cache = super::cache(dir.path(), &manifest::PARAKEET).map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(&other_cache)?;
    std::fs::write(other_cache.join("keep"), b"keep")?;
    delete(dir.path(), model).map_err(anyhow::Error::msg)?;
    delete(dir.path(), model).map_err(anyhow::Error::msg)?;
    assert!(!target.exists() && !cache.exists());
    assert_eq!(std::fs::read(unrelated)?, b"keep");
    assert_eq!(std::fs::read(other_cache.join("keep"))?, b"keep");
    Ok(())
}

/// Invalid target types fail without recursively deleting user directories.
#[test]
fn directory_in_place_of_weights_is_preserved() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let target = artifact(dir.path(), &manifest::SILERO).map_err(anyhow::Error::msg)?;
    std::fs::create_dir(&target)?;
    std::fs::write(target.join("keep"), b"keep")?;
    assert!(delete(dir.path(), &manifest::SILERO).is_err());
    assert_eq!(std::fs::read(target.join("keep"))?, b"keep");
    assert!(checked_child(dir.path(), Path::new("../outside")).is_err());
    Ok(())
}

/// Symlinked weights, ancestors and cache repositories cannot target external data.
#[cfg(unix)]
#[test]
fn rejects_symlinks_before_removing_anything() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("keep"), b"keep")?;
    symlink(outside.path(), dir.path().join("whisper"))?;
    assert!(artifact(dir.path(), &manifest::WHISPER_TURBO).is_err());
    let silero = manifest::SILERO.destination(dir.path());
    symlink(outside.path().join("keep"), &silero)?;
    assert!(delete(dir.path(), &manifest::SILERO).is_err());
    std::fs::remove_file(&silero)?;
    std::fs::write(&silero, b"weights")?;
    let cache = cache(dir.path(), &manifest::SILERO).map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(
        cache
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
    )?;
    symlink(outside.path(), &cache)?;
    assert!(delete(dir.path(), &manifest::SILERO).is_err());
    assert_eq!(std::fs::read(silero)?, b"weights");
    assert_eq!(std::fs::read(outside.path().join("keep"))?, b"keep");
    Ok(())
}
