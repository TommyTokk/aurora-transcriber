//! Serialized model residency with cheap status snapshots and offline reload.
use super::{fetcher, EngineError, Provider, TranscriberEngine};
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
    pub phase: String,
    pub provider: Option<Provider>,
    pub error: Option<String>,
    pub missing: Vec<String>,
    pub download_file: Option<String>,
    pub percentage: Option<f32>,
}
/// Both reusable inference sessions, dropped together when idle.
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
                phase: "loading".into(),
                provider: None,
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
    /// Updates cheap public status without requiring the session mutex.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        phase: &str,
        provider: Option<Provider>,
        error: Option<String>,
        missing: Vec<String>,
        download_file: Option<String>,
        percentage: Option<f32>,
    ) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            *snapshot = EngineSnapshot {
                phase: phase.into(),
                provider,
                error,
                missing,
                download_file,
                percentage,
            };
            let _ = app.emit_to("main", "transcriber-state", &*snapshot);
        }
    }
    /// Constructs verified local sessions on a blocking worker, with no network access.
    fn load<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<Sessions, String> {
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
            super::runtime::initialize_desktop(app).map_err(|error| {
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
        let missing = fetcher::missing(&base);
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
            let (asr, vad) = fetcher::paths(&base);
            // ASR verifies its own files during construction; avoid hashing its GB weights twice.
            super::resources::verify_file(&vad, fetcher::SILERO_HASH).map_err(|_| {
                "Local Silero model failed verification. Download models to repair it.".to_string()
            })?;
            let asr = TranscriberEngine::new(&asr).map_err(|e| e.to_string())?;
            let vad = SileroVad::new(&vad)
                .map_err(|_| "Unable to initialize the local Silero VAD model.".to_string())?;
            Ok(Sessions { asr, vad })
        })();
        match &result {
            Ok(sessions) => self.publish(
                app,
                "ready",
                Some(sessions.asr.provider()),
                None,
                vec![],
                None,
                None,
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
    /// Releases the old session pair on a worker and verifies the newly selected destination offline.
    pub(crate) fn reload_destination<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<(), String> {
        let mut resident = self
            .resident
            .lock()
            .map_err(|_| "Models are unavailable.")?;
        resident.value = None;
        // Missing/corrupt resources are reported by load; the folder choice itself remains successful.
        if let Ok(sessions) = self.load(app) {
            resident.value = Some(sessions);
        }
        resident.last_used = Instant::now();
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
/// Performs an explicitly requested download and initializes the verified resources.
#[tauri::command]
pub async fn download_models(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<EngineState>().reserve()?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<EngineState>();
        state.publish(&worker, "downloading", None, None, vec![], None, Some(0.0));
        fetcher::download(&worker)?;
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
mod tests {
    use super::*;
    /// Uses a controlled monotonic clock to verify strict timeout and busy protection.
    #[test]
    fn idle_deadline_and_reservation() -> Result<(), String> {
        let now = Instant::now();
        let mut resident = Residency {
            value: Some(42),
            last_used: now,
        };
        assert!(!resident.unload(now + IDLE_TIMEOUT, false));
        assert!(!resident.unload(now + IDLE_TIMEOUT + Duration::from_secs(1), true));
        assert!(resident.unload(now + IDLE_TIMEOUT + Duration::from_secs(1), false));
        assert!(resident.value.is_none());
        let state = EngineState::default();
        state.reserve()?;
        assert!(state.reserve().is_err());
        state.release();
        state.reserve()?;
        state.release();
        Ok(())
    }
    /// A fake loader proves reuse, failure recovery, and exactly one reload after expiry.
    #[test]
    fn offline_loader_reuses_and_reloads_once() -> Result<(), String> {
        let now = Instant::now();
        let mut resident = Residency {
            value: None,
            last_used: now,
        };
        let mut loads = 0;
        assert!(resident
            .get_or_load(|| Err("Missing model".into()))
            .is_err());
        assert!(resident.value.is_none());
        for _ in 0..3 {
            let value = resident.get_or_load(|| {
                loads += 1;
                Ok(42)
            })?;
            assert_eq!(*value, 42);
        }
        assert_eq!(loads, 1);
        assert!(resident.unload(now + Duration::from_secs(301), false));
        resident.get_or_load(|| {
            loads += 1;
            Ok(43)
        })?;
        assert_eq!(loads, 2);
        Ok(())
    }
    /// Simultaneous requests admit one owner for download or transcription.
    #[test]
    fn competing_model_operations_have_one_owner() -> anyhow::Result<()> {
        let state = std::sync::Arc::new(EngineState::default());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let state = state.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    state.reserve().is_ok()
                })
            })
            .collect();
        let mut admitted = 0;
        for worker in workers {
            if worker
                .join()
                .map_err(|_| anyhow::anyhow!("Worker failed"))?
            {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 1);
        state.release();
        Ok(())
    }
}
