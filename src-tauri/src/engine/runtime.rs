//! Fallible preflight for the pinned dynamic ONNX Runtime C interface.

use super::EngineError;
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

/// Reads an absolute, backend-owned resource path without exposing it to IPC.
pub(crate) fn absolute_env(name: &str) -> Result<PathBuf, EngineError> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or(EngineError::Configuration)?;
    if !path.is_absolute() {
        return Err(EngineError::Configuration);
    }
    Ok(path)
}

/// Validates the runtime before entering rc.9's internally panicking loader.
///
/// Only trusted native libraries are supported. Loading arbitrary executable code
/// cannot be sandboxed by this preflight. The handle lives for the process lifetime.
pub(crate) fn initialize() -> Result<(), EngineError> {
    if RUNTIME
        .lock()
        .map_err(|_| EngineError::Unavailable)?
        .is_some()
    {
        return Ok(());
    }
    let path = absolute_env("ORT_DYLIB_PATH")?;
    initialize_path(&path)
}

/// Resolves the bundled macOS runtime before any model constructor can consult development settings.
pub(crate) fn initialize_desktop<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), EngineError> {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let path = app
            .path()
            .resolve(
                "onnxruntime/libonnxruntime.1.20.1.dylib",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|_| EngineError::RuntimeLibrary)?;
        // The unsigned local bundle must still contain the exact official library.
        super::resources::verify_file(
            &path,
            "530cdb5a0de774677d369a83ec8912b0242e9769acd12d34b61412ee6ae368ae",
        )?;
        initialize_path(&path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        initialize()
    }
}

/// Initializes the global environment once, rejecting a different runtime path.
fn initialize_path(path: &Path) -> Result<(), EngineError> {
    let path = path
        .canonicalize()
        .map_err(|_| EngineError::RuntimeLibrary)?;
    let mut current = RUNTIME.lock().map_err(|_| EngineError::Unavailable)?;
    if let Some(guard) = current.as_ref() {
        return if guard.path == path {
            Ok(())
        } else {
            Err(EngineError::RuntimeLibrary)
        };
    }
    let library = preflight(&path)?;
    let path_string = path.to_str().ok_or(EngineError::RuntimeLibrary)?;
    ort::init_from(path_string).with_name("aurora").commit()?;
    *current = Some(RuntimeGuard {
        path,
        _library: library,
    });
    Ok(())
}

/// Checks the official 1.20.1 ABI, null pointers, and required entry points.
fn preflight(path: &Path) -> Result<Library, EngineError> {
    if !path.is_absolute() || !path.is_file() {
        return Err(EngineError::RuntimeLibrary);
    }
    // SAFETY: This explicitly configured local library is trusted executable code;
    // its handle is retained while symbols and the returned C interface are used.
    let library = unsafe { Library::new(path) }.map_err(|_| EngineError::RuntimeLibrary)?;
    // SAFETY: OrtGetApiBase's signature is fixed by the official ONNX Runtime ABI.
    let getter: Symbol<'_, unsafe extern "system" fn() -> *const ort_sys::OrtApiBase> =
        unsafe { library.get(b"OrtGetApiBase\0") }.map_err(|_| EngineError::RuntimeLibrary)?;
    // SAFETY: The validated symbol takes no arguments and returns a static API base.
    let base = unsafe { getter() };
    // SAFETY: A non-null pointer from the trusted runtime refers to OrtApiBase and
    // remains valid for the lifetime of the retained library handle.
    let base = unsafe { base.as_ref() }.ok_or(EngineError::RuntimeLibrary)?;
    let version_fn = base.GetVersionString.ok_or(EngineError::RuntimeLibrary)?;
    let api_fn = base.GetApi.ok_or(EngineError::RuntimeLibrary)?;
    // SAFETY: This official callback takes no arguments and returns a static string.
    let version = unsafe { version_fn() };
    if version.is_null() {
        return Err(EngineError::RuntimeLibrary);
    }
    // SAFETY: ONNX Runtime guarantees a NUL-terminated static version string.
    let version = unsafe { CStr::from_ptr(version) };
    if version.to_bytes() != b"1.20.1" {
        return Err(EngineError::RuntimeLibrary);
    }
    // SAFETY: The API version is provided by the pinned ort-sys C bindings.
    let api = unsafe { api_fn(ort_sys::ORT_API_VERSION) };
    if api.is_null() {
        return Err(EngineError::RuntimeLibrary);
    }
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
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
