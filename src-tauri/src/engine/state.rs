//! Serialized model residency with cheap status snapshots and offline reload.
use super::{
    fetcher, manifest, model_files, AsrModelId, AsrSelection, DeviceSnapshot, EngineError,
    FallbackReason, Provider, TranscriberEngine,
};
use crate::{model_storage::ModelStorage, vad::SileroVad};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};

const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Public model status; resource errors expose safe filenames only.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub revision: u64,
    pub models: Vec<super::ModelDescriptor>,
    pub selected_model: AsrModelId,
    pub installed: Vec<String>,
    pub selection: AsrSelection,
    pub devices: Vec<DeviceSnapshot>,
    pub device: Option<DeviceSnapshot>,
    pub phase: String,
    pub provider: Option<Provider>,
    pub fallback_reason: Option<FallbackReason>,
    pub error: Option<String>,
    pub missing: Vec<String>,
    pub download_file: Option<String>,
    pub percentage: Option<f32>,
}
/// Resident ASR weights and CPU VAD session, dropped together when idle.
pub(crate) struct Sessions {
    pub asr: TranscriberEngine,
    pub vad: SileroVad,
}
/// Monotonic idle bookkeeping independent of inference implementations.
struct Residency<T> {
    value: Option<T>,
    last_used: Instant,
}
impl<T> Residency<T> {
    /// Drops the previous assets before loading a replacement, retaining failures as unloaded.
    fn replace(&mut self, loader: impl FnOnce() -> Result<T, String>) -> Result<(), String> {
        self.value = None;
        let result = loader();
        self.last_used = Instant::now();
        self.value = Some(result?);
        Ok(())
    }
    /// Reuses a resident value or invokes the offline loader exactly once while locked.
    fn get_or_load(
        &mut self,
        loader: impl FnOnce() -> Result<T, String>,
    ) -> Result<&mut T, String> {
        if self.value.is_none() {
            self.value = Some(loader()?);
        }
        self.value
            .as_mut()
            .ok_or_else(|| "Models are unavailable.".into())
    }
    /// Releases a resident value only strictly after the idle threshold.
    fn unload(&mut self, now: Instant, busy: bool) -> bool {
        if !busy
            && self.value.is_some()
            && now.saturating_duration_since(self.last_used) > IDLE_TIMEOUT
        {
            self.value = None;
            return true;
        }
        false
    }
}
/// Owns sessions and a shared reservation for startup, download, and transcription.
pub struct EngineState {
    resident: Mutex<Residency<Sessions>>,
    snapshot: Mutex<EngineSnapshot>,
    busy: AtomicBool,
    desktop: bool,
}
impl Default for EngineState {
    /// Begins with no sessions and an unreserved model manager.
    fn default() -> Self {
        Self {
            resident: Mutex::new(Residency {
                value: None,
                last_used: Instant::now(),
            }),
            snapshot: Mutex::new(EngineSnapshot {
                revision: 0,
                models: super::manifest::descriptors(),
                selected_model: AsrModelId::default(),
                installed: vec![],
                selection: AsrSelection::Auto {},
                devices: vec![],
                device: None,
                phase: "loading".into(),
                provider: None,
                fallback_reason: None,
                error: None,
                missing: vec![],
                download_file: None,
                percentage: None,
            }),
            busy: AtomicBool::new(false),
            desktop: false,
        }
    }
}
impl EngineState {
    /// Requires the bundled runtime for every desktop load, including retries after failure.
    pub(crate) fn desktop() -> Self {
        Self {
            desktop: true,
            ..Self::default()
        }
    }

    /// Atomically rejects overlapping model operations before scheduling a worker.
    pub(crate) fn reserve(&self) -> Result<(), String> {
        let _resident = self
            .resident
            .try_lock()
            .map_err(|_| "Models are busy. Please wait.".to_string())?;
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "Models are busy. Please wait.".into())
    }
    /// Releases the reservation after the worker has actually completed.
    pub(crate) fn release(&self) {
        self.busy.store(false, Ordering::Release);
    }
    /// Releases resident native assets on a worker before the application exits.
    pub(crate) fn shutdown(&self) {
        let sessions = match self.resident.lock() {
            Ok(mut resident) => resident.value.take(),
            // Even a failed worker must not leave native assets to the UI's final state drop.
            Err(poisoned) => poisoned.into_inner().value.take(),
        };
        if let Some(sessions) = sessions {
            let worker = tauri::async_runtime::spawn_blocking(move || drop(sessions));
            let _ = tauri::async_runtime::block_on(worker);
        }
    }
    /// Updates and emits one ordered snapshot under a short lock, independent of inference.
    fn update<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        operation: impl FnOnce(&mut EngineSnapshot),
    ) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            operation(&mut snapshot);
            snapshot.revision += 1;
            let _ = app.emit_to("main", "transcriber-state", &*snapshot);
        }
    }

    /// Updates model residency while preserving the session preference and hardware catalog.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        phase: &str,
        device: Option<DeviceSnapshot>,
        error: Option<String>,
        missing: Vec<String>,
        download_file: Option<String>,
        percentage: Option<f32>,
    ) {
        self.update(app, |snapshot| {
            snapshot.phase = phase.into();
            snapshot.provider = device.as_ref().map(|d| d.backend);
            snapshot.device = device;
            snapshot.fallback_reason = None;
            snapshot.error = error;
            snapshot.missing = missing;
            snapshot.download_file = download_file;
            snapshot.percentage = percentage;
        });
    }

    /// Publishes verified readiness and recovery diagnostics in one ordered event.
    fn publish_ready<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        device: DeviceSnapshot,
        fallback_reason: Option<FallbackReason>,
    ) {
        self.update(app, |snapshot| {
            snapshot.phase = "ready".into();
            snapshot.provider = Some(device.backend);
            snapshot.device = Some(device);
            snapshot.fallback_reason = fallback_reason;
            snapshot.error = None;
            snapshot.missing.clear();
            snapshot.download_file = None;
            snapshot.percentage = None;
        });
    }

    /// Discovers hardware on a worker even when local model files are missing.
    fn discover<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<(), String> {
        let devices = super::discover_devices().map_err(|error| error.to_string())?;
        self.update(app, |snapshot| snapshot.devices = devices);
        Ok(())
    }

    /// Applies a validated preference while reserved, freeing old assets before immediate reload.
    pub(crate) fn select<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        selection: AsrSelection,
    ) -> Result<(), String> {
        // Reject forged, expired and CPU-as-device IDs before touching the current model.
        if matches!(selection, AsrSelection::Device { .. }) {
            super::providers::options(&selection).map_err(|error| error.to_string())?;
        }
        let mut resident = self
            .resident
            .lock()
            .map_err(|_| "Models are unavailable.")?;
        resident.replace(|| {
            self.update(app, |snapshot| {
                snapshot.selection = selection;
                snapshot.phase = "loading".into();
                snapshot.provider = None;
                snapshot.device = None;
                snapshot.fallback_reason = None;
                snapshot.error = None;
                snapshot.percentage = None;
                snapshot.download_file = None;
            });
            self.load(app)
        })
    }
    /// Constructs verified local ASR weights and VAD on a worker without network access.
    fn load<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<Sessions, String> {
        self.refresh_catalog(app)?;
        if let Err(error) = self.discover(app) {
            self.publish(
                app,
                "unavailable",
                None,
                Some(error.clone()),
                vec![],
                None,
                None,
            );
            return Err(error);
        }
        let base = app.state::<ModelStorage>().prepare().inspect_err(|error| {
            self.publish(
                app,
                "unavailable",
                None,
                Some(error.clone()),
                vec![],
                None,
                None,
            );
        })?;
        if self.desktop {
            crate::vad::initialize_desktop(app).map_err(|error| {
                self.publish(
                    app,
                    "unavailable",
                    None,
                    Some(error.to_string()),
                    vec![],
                    None,
                    None,
                );
                error.to_string()
            })?;
        }
        let selected = self.snapshot()?.selected_model;
        let missing = fetcher::missing_for(&base, selected);
        if !missing.is_empty() {
            self.publish(
                app,
                "missing",
                None,
                Some("Download the missing models to transcribe.".into()),
                missing,
                None,
                None,
            );
            return Err("Download the missing models to transcribe.".into());
        }
        self.publish(app, "loading", None, None, vec![], None, None);
        let result: Result<Sessions, String> = (|| {
            let resource = selected.resource();
            let asr_file = model_files::artifact(&base, resource)?;
            let asr = asr_file.parent().ok_or("Invalid ASR destination.")?;
            let vad = model_files::artifact(&base, &manifest::SILERO)?;
            // ASR verifies its own files during construction; avoid hashing its GB weights twice.
            crate::resources::verify_file(&vad, fetcher::SILERO_HASH).map_err(|_| {
                "Local Silero model failed verification. Download models to repair it.".to_string()
            })?;
            let selection = self.snapshot()?.selection;
            let asr = TranscriberEngine::new_with_model(asr, selected, &selection)
                .map_err(|e| e.to_string())?;
            let vad = SileroVad::new(&vad)
                .map_err(|_| "Unable to initialize the local Silero VAD model.".to_string())?;
            Ok(Sessions { asr, vad })
        })();
        match &result {
            Ok(sessions) => self.publish_ready(
                app,
                sessions.asr.device().clone(),
                sessions.asr.fallback_reason(),
            ),
            Err(error) => self.publish(
                app,
                "unavailable",
                None,
                Some(error.clone()),
                vec![],
                None,
                None,
            ),
        }
        result
    }
    /// Refreshes cheap artifact presence and the saved ASR independently of inference readiness.
    pub(crate) fn refresh_catalog<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<(), String> {
        let selected = match app.try_state::<crate::settings::TranscriptionSettings>() {
            Some(settings) => settings.snapshot()?.selected_model,
            None => self.snapshot()?.selected_model,
        };
        let installed = app
            .state::<ModelStorage>()
            .path()
            .map(|base| {
                manifest::MODELS
                    .iter()
                    .filter(|model| {
                        model_files::artifact(&base, model).is_ok_and(|path| path.is_file())
                    })
                    .map(|model| model.descriptor.id.to_string())
                    .collect()
            })
            .unwrap_or_default();
        self.update(app, |snapshot| {
            snapshot.selected_model = selected;
            snapshot.installed = installed;
        });
        Ok(())
    }

    /// Releases all native sessions on the reserved blocking worker before removing required files.
    pub(crate) fn clear_sessions(&self) -> Result<(), String> {
        self.resident
            .lock()
            .map_err(|_| "Models are unavailable.")?
            .value = None;
        Ok(())
    }

    /// Releases the old session pair on a worker and verifies the newly selected destination offline.
    pub(crate) fn reload_destination<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<(), String> {
        let mut resident = self
            .resident
            .lock()
            .map_err(|_| "Models are unavailable.")?;
        // Missing/corrupt resources are reported by load; the folder choice itself remains successful.
        let _ = resident.replace(|| {
            self.publish(app, "loading", None, None, vec![], None, None);
            self.load(app)
        });
        Ok(())
    }
    /// Pins both sessions throughout one reserved job and updates idle time on every exit.
    pub(crate) fn with_sessions<T, R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        operation: impl FnOnce(&mut Sessions) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut resident = self
            .resident
            .lock()
            .map_err(|_| "Models are unavailable.".to_string())?;
        let sessions = resident.get_or_load(|| self.load(app))?;
        // A successful download can reuse already resident weights. Restore ready
        // diagnostics rather than leaving the UI stuck in the downloading phase.
        if self.snapshot()?.phase != "ready" {
            self.publish_ready(
                app,
                sessions.asr.device().clone(),
                sessions.asr.fallback_reason(),
            );
        }
        let result = operation(sessions);
        resident.last_used = Instant::now();
        result
    }
    /// Borrows the ASR engine for existing blocking callers without triggering a reload.
    pub fn with_engine<T>(
        &self,
        operation: impl FnOnce(&mut TranscriberEngine) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let mut resident = self.resident.lock().map_err(|_| EngineError::Unavailable)?;
        let result = operation(&mut resident.value.as_mut().ok_or(EngineError::Unavailable)?.asr);
        resident.last_used = Instant::now();
        result
    }
    /// Returns the current status independently of active inference.
    pub(crate) fn snapshot(&self) -> Result<EngineSnapshot, String> {
        self.snapshot
            .lock()
            .map(|s| s.clone())
            .map_err(|_| "Models are unavailable.".into())
    }
    /// Drops idle sessions on a worker, skipping every reserved or locked operation.
    pub(crate) fn unload_idle_at<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        now: Instant,
    ) {
        if let Ok(mut resident) = self.resident.try_lock() {
            if resident.unload(now, self.busy.load(Ordering::Acquire)) {
                self.publish(app, "unloaded", None, None, vec![], None, None);
            }
        }
    }
}
/// Loads available local models at startup and schedules a one-second idle monitor.
pub(crate) fn start(app: tauri::AppHandle) {
    if app.state::<EngineState>().reserve().is_err() {
        return;
    }
    let startup = app.clone();
    tauri::async_runtime::spawn(async move {
        let worker = startup.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let state = worker.state::<EngineState>();
            let result = state.with_sessions(&worker, |_| Ok(()));
            crate::model_storage::publish(&worker);
            result
        })
        .await;
        if result.is_err() {
            startup.state::<EngineState>().publish(
                &startup,
                "unavailable",
                None,
                Some("Model initialization worker failed.".into()),
                vec![],
                None,
                None,
            );
        }
        startup.state::<EngineState>().release();
    });
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let worker = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                worker
                    .state::<EngineState>()
                    .unload_idle_at(&worker, Instant::now())
            })
            .await;
        }
    });
}
/// Returns model status without accepting model paths from the WebView.
#[tauri::command]
pub fn transcriber_state(state: State<'_, EngineState>) -> Result<EngineSnapshot, String> {
    state.snapshot()
}
/// Discovers supported devices on a worker and returns the ordered combined status.
#[tauri::command]
pub async fn transcriber_devices(app: tauri::AppHandle) -> Result<EngineSnapshot, String> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<EngineState>();
        state.discover(&worker)?;
        state.snapshot()
    })
    .await
    .map_err(|_| "Device discovery worker failed.".to_string())?
}

/// Applies a session-only preference, excluding all overlapping model and transcription work.
#[tauri::command]
pub async fn select_asr_device(
    app: tauri::AppHandle,
    selection: AsrSelection,
) -> Result<EngineSnapshot, String> {
    app.state::<EngineState>().reserve()?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        worker.state::<EngineState>().select(&worker, selection)
    })
    .await;
    if result.is_err() {
        app.state::<EngineState>().publish(
            &app,
            "unavailable",
            None,
            Some("Device loading worker failed. Retry your selection.".into()),
            vec![],
            None,
            None,
        );
    }
    app.state::<EngineState>().release();
    result.map_err(|_| "Device loading worker failed.".to_string())??;
    app.state::<EngineState>().snapshot()
}
/// Performs an explicitly requested download and initializes the verified resources.
#[tauri::command]
pub async fn download_models(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<EngineState>().reserve()?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<EngineState>();
        state.publish(&worker, "downloading", None, None, vec![], None, Some(0.0));
        let download = fetcher::download(&worker);
        state.refresh_catalog(&worker)?;
        download?;
        state.with_sessions(&worker, |_| Ok(()))
    })
    .await
    .map_err(|_| "Model download worker failed.".to_string())
    .and_then(|r| r);
    if let Err(error) = &result {
        app.state::<EngineState>().publish(
            &app,
            "unavailable",
            None,
            Some(error.clone()),
            vec![],
            None,
            None,
        );
    }
    app.state::<EngineState>().release();
    crate::model_storage::publish(&app);
    result
}
#[cfg(test)]
mod tests;
