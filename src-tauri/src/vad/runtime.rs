//! Fallible preflight for the pinned dynamic ONNX Runtime C interface.

use super::VadError;
use libloading::{Library, Symbol};
use std::{
    ffi::CStr,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Keeps the validated library loaded and prevents conflicting global ORT paths.
struct RuntimeGuard {
    path: PathBuf,
    _library: Library,
}

static RUNTIME: Mutex<Option<RuntimeGuard>> = Mutex::new(None);

/// Validates the runtime before entering rc.9's internally panicking loader.
///
/// Only trusted native libraries are supported. Loading arbitrary executable code
/// cannot be sandboxed by this preflight. The handle lives for the process lifetime.
pub(crate) fn initialize() -> Result<(), VadError> {
    if RUNTIME
        .lock()
        .map_err(|_| VadError::RuntimeLibrary)?
        .is_some()
    {
        return Ok(());
    }
    let path =
        crate::resources::absolute_env("ORT_DYLIB_PATH").map_err(|_| VadError::RuntimeLibrary)?;
    initialize_path(&path)
}

/// Resolves the bundled macOS runtime before any model constructor can consult development settings.
pub(crate) fn initialize_desktop<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), VadError> {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let path = app
            .path()
            .resolve(
                "onnxruntime/libonnxruntime.1.20.1.dylib",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|_| VadError::RuntimeLibrary)?;
        // The unsigned local bundle must still contain the exact official library.
        crate::resources::verify_file(
            &path,
            "530cdb5a0de774677d369a83ec8912b0242e9769acd12d34b61412ee6ae368ae",
        )
        .map_err(|_| VadError::RuntimeLibrary)?;
        initialize_path(&path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        initialize()
    }
}

/// Initializes the global environment once, rejecting a different runtime path.
fn initialize_path(path: &Path) -> Result<(), VadError> {
    if !path.is_absolute() {
        return Err(VadError::RuntimeLibrary);
    }
    let path = path.canonicalize().map_err(|_| VadError::RuntimeLibrary)?;
    let mut current = RUNTIME.lock().map_err(|_| VadError::RuntimeLibrary)?;
    if let Some(guard) = current.as_ref() {
        return if guard.path == path {
            Ok(())
        } else {
            Err(VadError::RuntimeLibrary)
        };
    }
    let library = preflight(&path)?;
    let path_string = path.to_str().ok_or(VadError::RuntimeLibrary)?;
    ort::init_from(path_string)
        .with_name("aurora-silero-vad")
        .commit()?;
    *current = Some(RuntimeGuard {
        path,
        _library: library,
    });
    Ok(())
}

/// Checks the official 1.20.1 ABI, null pointers, and required entry points.
fn preflight(path: &Path) -> Result<Library, VadError> {
    if !path.is_absolute() || !path.is_file() {
        return Err(VadError::RuntimeLibrary);
    }
    // SAFETY: This explicitly configured local library is trusted executable code;
    // its handle is retained while symbols and the returned C interface are used.
    let library = unsafe { Library::new(path) }.map_err(|_| VadError::RuntimeLibrary)?;
    // SAFETY: OrtGetApiBase's signature is fixed by the official ONNX Runtime ABI.
    let getter: Symbol<'_, unsafe extern "system" fn() -> *const ort_sys::OrtApiBase> =
        unsafe { library.get(b"OrtGetApiBase\0") }.map_err(|_| VadError::RuntimeLibrary)?;
    // SAFETY: The validated symbol takes no arguments and returns a static API base.
    let base = unsafe { getter() };
    // SAFETY: A non-null pointer from the trusted runtime refers to OrtApiBase and
    // remains valid for the lifetime of the retained library handle.
    let base = unsafe { base.as_ref() }.ok_or(VadError::RuntimeLibrary)?;
    let version_fn = base.GetVersionString.ok_or(VadError::RuntimeLibrary)?;
    let api_fn = base.GetApi.ok_or(VadError::RuntimeLibrary)?;
    // SAFETY: This official callback takes no arguments and returns a static string.
    let version = unsafe { version_fn() };
    if version.is_null() {
        return Err(VadError::RuntimeLibrary);
    }
    // SAFETY: ONNX Runtime guarantees a NUL-terminated static version string.
    let version = unsafe { CStr::from_ptr(version) };
    validate_version(version.to_bytes())?;
    // SAFETY: The API version is provided by the pinned ort-sys C bindings.
    let api = unsafe { api_fn(ort_sys::ORT_API_VERSION) };
    if api.is_null() {
        return Err(VadError::RuntimeLibrary);
    }
    Ok(library)
}

/// Rejects native runtime versions outside the pinned wrapper's trusted ABI.
fn validate_version(version: &[u8]) -> Result<(), VadError> {
    if version != b"1.20.1" {
        return Err(VadError::RuntimeLibrary);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rejects incompatible native versions before requesting the C API.
    #[test]
    fn incompatible_versions_are_rejected() -> anyhow::Result<()> {
        validate_version(b"1.20.1")?;
        for version in [b"1.20.0".as_slice(), b"1.21.0", b""] {
            assert!(validate_version(version).is_err());
        }
        Ok(())
    }

    /// Reuses the real local environment and rejects a conflicting path before loading it.
    #[test]
    #[ignore = "requires explicitly configured trusted ONNX Runtime 1.20.1; no ASR"]
    fn local_runtime_reuse_and_conflicting_path() -> anyhow::Result<()> {
        let path = crate::resources::absolute_env("ORT_DYLIB_PATH")?;
        initialize_path(&path)?;
        initialize_path(&path)?;
        initialize()?;
        let conflicting = tempfile::NamedTempFile::new()?;
        assert!(matches!(
            initialize_path(conflicting.path()),
            Err(VadError::RuntimeLibrary)
        ));
        assert!(initialize_path(Path::new("relative.dylib")).is_err());
        initialize_path(&path)?;
        Ok(())
    }

    /// Rejects absent and non-library files without entering ORT's panic paths.
    #[test]
    fn invalid_libraries_are_fallible() -> anyhow::Result<()> {
        let file = tempfile::NamedTempFile::new()?;
        assert!(preflight(file.path()).is_err());
        assert!(preflight(Path::new("/missing/onnxruntime.dylib")).is_err());
        assert!(preflight(Path::new("relative.dylib")).is_err());
        Ok(())
    }
}
