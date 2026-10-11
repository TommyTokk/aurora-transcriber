//! Strict Metal policy; ASR must never fall back to CPU.
use super::EngineError;
use serde::Serialize;
use std::sync::Once;
use transcribe_cpp::{Backend, DeviceType, Model, ModelOptions};

static NATIVE_STARTUP: Once = Once::new();

/// Actual initialized ASR backend, serialized through the existing provider field.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Provider {
    Metal,
}

/// Silences native diagnostics once before registry access or model loading.
pub(crate) fn initialize() {
    NATIVE_STARTUP.call_once(transcribe_cpp::disable_logging);
}

/// Requires Metal on Apple Silicon before loading any weights.
pub(super) fn options() -> Result<ModelOptions, EngineError> {
    initialize();
    // Upstream diagnostic dumps bypass its logging callback and write sensitive tensors.
    if std::env::var_os("TRANSCRIBE_DUMP_DIR").is_some_and(|value| !value.is_empty()) {
        return Err(EngineError::Configuration);
    }
    if !cfg!(all(target_os = "macos", target_arch = "aarch64"))
        || !transcribe_cpp::backend_available(Backend::Metal)
    {
        return Err(EngineError::GpuUnavailable);
    }
    Ok(ModelOptions {
        backend: Backend::Metal,
        device: None,
    })
}

/// Verifies the resolved device instead of inferring GPU use from Cargo features.
pub(super) fn verify(model: &Model) -> Result<Provider, EngineError> {
    let device = model.device().map_err(|_| EngineError::GpuUnavailable)?;
    verify_backend(
        &model.backend(),
        &device.name,
        &device.kind,
        device.device_type,
    )
}

/// Rejects CPU fallback, unresolved devices, and other accelerators.
fn verify_backend(
    backend: &str,
    device_name: &str,
    kind: &str,
    class: DeviceType,
) -> Result<Provider, EngineError> {
    // Parakeet 0.2.4 reports the ggml name (MTL0), rather than the documented kind.
    // Match the resolved device identity as well as its Metal kind and GPU class.
    if (backend == "metal" || (!device_name.is_empty() && backend == device_name))
        && kind == "metal"
        && matches!(class, DeviceType::Gpu | DeviceType::Igpu)
    {
        Ok(Provider::Metal)
    } else {
        Err(EngineError::GpuUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Accepts Metal GPUs and rejects hidden CPU or unresolved devices.
    #[test]
    fn actual_backend_must_be_metal() -> anyhow::Result<()> {
        assert_eq!(
            verify_backend("metal", "MTL0", "metal", DeviceType::Gpu)?,
            Provider::Metal
        );
        assert_eq!(
            verify_backend("metal", "MTL0", "metal", DeviceType::Igpu)?,
            Provider::Metal
        );
        assert_eq!(
            verify_backend("MTL0", "MTL0", "metal", DeviceType::Gpu)?,
            Provider::Metal
        );
        for (backend, kind, class) in [
            ("cpu", "cpu", DeviceType::Cpu),
            ("", "metal", DeviceType::Gpu),
            ("MTL1", "metal", DeviceType::Gpu),
            ("metal", "cpu", DeviceType::Cpu),
            ("vulkan", "vulkan", DeviceType::Gpu),
            ("metal", "metal", DeviceType::Unknown),
        ] {
            assert!(matches!(
                verify_backend(backend, "MTL0", kind, class),
                Err(EngineError::GpuUnavailable)
            ));
        }
        Ok(())
    }
}
