//! Process-local device discovery and strict, explicit ASR placement.
use super::EngineError;
use serde::{Deserialize, Serialize};
use std::sync::{Once, OnceLock};
use transcribe_cpp::{Backend, Device, DeviceType, Model, ModelOptions};
use uuid::Uuid;

static NATIVE_STARTUP: Once = Once::new();
static DEVICES: OnceLock<Vec<RegisteredDevice>> = OnceLock::new();

/// Actual initialized ASR backend, including an explicitly attempted CPU fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Provider {
    Metal,
    #[serde(rename = "CUDA")]
    Cuda,
    Vulkan,
    #[serde(rename = "CPU")]
    Cpu,
}
impl Provider {
    /// Maps verified providers to explicit native requests, never native Auto.
    fn backend(self) -> Backend {
        match self {
            Self::Metal => Backend::Metal,
            Self::Cuda => Backend::Cuda,
            Self::Vulkan => Backend::Vulkan,
            Self::Cpu => Backend::Cpu,
        }
    }
    /// Returns the native registry's normalized vendor kind.
    fn kind(self) -> &'static str {
        match self {
            Self::Metal => "metal",
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
            Self::Cpu => "cpu",
        }
    }
}

/// Session-only preference; opaque device identifiers never contain native handles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
pub enum AsrSelection {
    Auto {},
    Cpu {},
    Device { id: String },
}

impl Default for AsrSelection {
    /// Starts every process with Auto; preferences are not persisted.
    fn default() -> Self {
        Self::Auto {}
    }
}

/// Safe initialization diagnostics retained with resident weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    NoGpu,
    GpuInitializationFailed,
    GpuOutOfMemory,
}

/// Hardware-only diagnostics, with an identifier valid for this process alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    pub id: String,
    pub backend: Provider,
    pub name: String,
    pub description: String,
}
/// Keeps native registry handles entirely behind the Rust boundary.
struct RegisteredDevice {
    native: Device,
    snapshot: DeviceSnapshot,
}

/// Silences native diagnostics once before registry access or model loading.
pub(crate) fn initialize() {
    NATIVE_STARTUP.call_once(transcribe_cpp::disable_logging);
}

/// Rejects native tensor dumps, which bypass the disabled logging callback.
fn check_configuration() -> Result<(), EngineError> {
    initialize();
    if std::env::var_os("TRANSCRIBE_DUMP_DIR").is_some_and(|value| !value.is_empty()) {
        return Err(EngineError::Configuration);
    }
    Ok(())
}

/// Lists the permitted GPU backend order for an operating system and architecture.
fn gpu_order(os: &str, arch: &str) -> &'static [Provider] {
    match (os, arch) {
        ("macos", "aarch64") => &[Provider::Metal],
        ("windows" | "linux", _) => &[Provider::Cuda, Provider::Vulkan],
        _ => &[],
    }
}

/// Classifies only known GPU vendors and strict CPU devices; accelerators are excluded.
fn classify(kind: &str, class: DeviceType) -> Option<Provider> {
    match (kind, class) {
        ("metal", DeviceType::Gpu | DeviceType::Igpu) => Some(Provider::Metal),
        ("cuda", DeviceType::Gpu | DeviceType::Igpu) => Some(Provider::Cuda),
        ("vulkan", DeviceType::Gpu | DeviceType::Igpu) => Some(Provider::Vulkan),
        ("cpu", DeviceType::Cpu) => Some(Provider::Cpu),
        _ => None,
    }
}

/// Enumerates static compiled-in devices once, on the caller's blocking worker.
fn registry() -> Result<&'static [RegisteredDevice], EngineError> {
    check_configuration()?;
    // This application uses static backends only. No module-registration calls or
    // user-controlled native-library search paths can race this immutable catalog.
    Ok(DEVICES.get_or_init(|| {
        let order = gpu_order(std::env::consts::OS, std::env::consts::ARCH);
        transcribe_cpp::devices()
            .into_iter()
            .filter_map(|native| {
                let backend = classify(&native.kind, native.device_type)?;
                if backend != Provider::Cpu && !order.contains(&backend) {
                    return None;
                }
                let snapshot = DeviceSnapshot {
                    id: Uuid::new_v4().to_string(),
                    backend,
                    name: native.name.clone(),
                    description: native.description.clone(),
                };
                Some(RegisteredDevice { native, snapshot })
            })
            .collect()
    }))
}

/// Returns registered, supported hardware without loading models or exposing handles.
pub fn discover_devices() -> Result<Vec<DeviceSnapshot>, EngineError> {
    Ok(registry()?.iter().map(|d| d.snapshot.clone()).collect())
}

/// Resolves exact GPU requests followed by one registered CPU candidate.
fn candidate_indices(
    devices: &[DeviceSnapshot],
    selection: &AsrSelection,
    order: &[Provider],
) -> Result<Vec<usize>, EngineError> {
    let mut indices: Vec<_> = match selection {
        AsrSelection::Auto {} => order
            .iter()
            .flat_map(|provider| {
                devices
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, d)| (d.backend == *provider).then_some(index))
            })
            .collect(),
        AsrSelection::Cpu {} => devices
            .iter()
            .position(|d| d.backend == Provider::Cpu)
            .into_iter()
            .collect(),
        AsrSelection::Device { id } => devices
            .iter()
            .position(|d| d.id == *id && d.backend != Provider::Cpu)
            .into_iter()
            .collect(),
    };
    if matches!(selection, AsrSelection::Device { .. }) && indices.is_empty() {
        return Err(EngineError::SelectionUnavailable);
    }
    if !matches!(selection, AsrSelection::Cpu {}) {
        indices.extend(devices.iter().position(|d| d.backend == Provider::Cpu));
    }
    if indices.is_empty() {
        return Err(match selection {
            AsrSelection::Cpu {} => EngineError::SelectionUnavailable,
            _ => EngineError::CpuFallbackUnavailable,
        });
    }
    Ok(indices)
}

/// Builds exact native requests, including CPU recovery for GPU preferences.
pub(super) fn options(selection: &AsrSelection) -> Result<Vec<ModelOptions>, EngineError> {
    let devices = registry()?;
    let snapshots: Vec<_> = devices.iter().map(|d| d.snapshot.clone()).collect();
    let indices = candidate_indices(
        &snapshots,
        selection,
        gpu_order(std::env::consts::OS, std::env::consts::ARCH),
    )?;
    Ok(indices
        .into_iter()
        .map(|index| ModelOptions {
            backend: devices[index].snapshot.backend.backend(),
            device: Some(devices[index].native.clone()),
        })
        .collect())
}

/// Validates backend reporting, vendor and device class, including ggml device-name reporting.
fn verify_backend(
    backend: &str,
    name: &str,
    kind: &str,
    class: DeviceType,
) -> Result<Provider, EngineError> {
    let provider = classify(kind, class).ok_or(EngineError::GpuUnavailable)?;
    // Parakeet 0.2.4 can report MTL0/CUDA0 instead of the normalized vendor name.
    if backend != provider.kind() && (name.is_empty() || backend != name) {
        return Err(EngineError::GpuUnavailable);
    }
    Ok(provider)
}

/// Requires the initialized model to match the exact requested handle and backend.
pub(super) fn verify(model: &Model, options: &ModelOptions) -> Result<DeviceSnapshot, EngineError> {
    let device = model.device().map_err(|_| EngineError::GpuUnavailable)?;
    let provider = verify_backend(
        &model.backend(),
        &device.name,
        &device.kind,
        device.device_type,
    )?;
    let expected = options
        .device
        .as_ref()
        .ok_or(EngineError::SelectionUnavailable)?;
    verify_placement(provider, options.backend, device == *expected)?;
    registry()?
        .iter()
        .find(|d| d.native == device)
        .map(|d| d.snapshot.clone())
        .ok_or(EngineError::SelectionUnavailable)
}

/// Rejects silent native substitution, including CPU returned for a GPU request.
fn verify_placement(
    provider: Provider,
    requested: Backend,
    exact_device: bool,
) -> Result<(), EngineError> {
    if !exact_device || provider.backend() != requested {
        return Err(EngineError::GpuUnavailable);
    }
    Ok(())
}

/// Tries exact requests; the loader must release failed assets before returning.
pub(super) fn try_candidates<T>(
    candidates: &[ModelOptions],
    selection: &AsrSelection,
    mut load: impl FnMut(&ModelOptions) -> Result<T, EngineError>,
) -> Result<(T, Option<FallbackReason>), EngineError> {
    let mut reason = None;
    for options in candidates {
        let cpu = options.backend == Backend::Cpu;
        let fallback = cpu && !matches!(selection, AsrSelection::Cpu {});
        if fallback {
            let recovery = *reason.get_or_insert(FallbackReason::NoGpu);
            eprintln!("ASR recovery: backend=CPU reason={recovery:?}");
        }
        match load(options) {
            Ok(value) => return Ok((value, if fallback { reason } else { None })),
            Err(error) if fallback => {
                return Err(EngineError::CpuFallbackFailed(Box::new(error)));
            }
            Err(error) if !cpu => {
                let category = match error {
                    EngineError::GpuUnavailable => "gpu_unavailable",
                    EngineError::ModelLoad => "model_load",
                    EngineError::OutOfMemory => "gpu_out_of_memory",
                    _ => return Err(error),
                };
                eprintln!(
                    "ASR initialization failed: backend={:?} category={category}",
                    options.backend
                );
                // Preserve memory pressure even if a later GPU fails differently.
                if matches!(error, EngineError::OutOfMemory) {
                    reason = Some(FallbackReason::GpuOutOfMemory);
                } else if reason.is_none() {
                    reason = Some(FallbackReason::GpuInitializationFailed);
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(EngineError::CpuFallbackUnavailable)
}

#[cfg(test)]
mod tests;
