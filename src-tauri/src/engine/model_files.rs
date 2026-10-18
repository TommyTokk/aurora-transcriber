//! Catalog-scoped filesystem access; WebView identities never become arbitrary paths.
use super::manifest::ModelResource;
use std::path::{Path, PathBuf};

/// Resolves a managed descendant, rejecting symlinks and non-directory ancestors.
pub(crate) fn checked_child(base: &Path, relative: &Path) -> Result<PathBuf, String> {
    let root = base
        .canonicalize()
        .map_err(|_| "Model folder is unavailable.")?;
    let mut current = root;
    for part in relative.components() {
        let std::path::Component::Normal(part) = part else {
            return Err("Invalid model destination.".into());
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Symbolic links are not allowed in managed model destinations.".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Unable to inspect model destination.".into()),
        }
    }
    Ok(current)
}

/// Resolves exactly one catalog artifact inside the native selected directory.
pub(crate) fn artifact(base: &Path, model: &ModelResource) -> Result<PathBuf, String> {
    checked_child(base, &model.destination(Path::new("")))
}

/// Names only this artifact's repository cache, including transfers interrupted by older versions.
pub(crate) fn cache(base: &Path, model: &ModelResource) -> Result<PathBuf, String> {
    checked_child(
        base,
        &Path::new(".download-cache")
            .join(format!("models--{}", model.repository.replace('/', "--"))),
    )
}

/// Deletes only catalog-owned weights and their redundant repository cache, never the model folder.
pub(crate) fn delete(base: &Path, model: &ModelResource) -> Result<(), String> {
    // Validate both targets before deleting either, so an unsafe cache cannot cause a partial delete.
    let target = artifact(base, model)?;
    let cache = cache(base, model)?;
    match std::fs::remove_file(&target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Unable to delete model. Check folder permissions.".into()),
    }
    match std::fs::remove_dir_all(cache) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Model removed, but its download cache could not be deleted.".into()),
    }
}

#[cfg(test)]
mod tests;
