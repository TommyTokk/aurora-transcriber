//! One startup attempt and independent path-free engine status snapshots.
use super::{runtime, EngineError, Provider, TranscriberEngine};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Manager, State};

/// Startup phases; a failed initialization is terminal until the next app launch.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Loading,
    Ready,
    Unavailable,
}

/// Public status without model paths, audio, tokens, or underlying error details.
#[derive(Clone, Serialize)]
pub struct EngineSnapshot {
    phase: Phase,
    provider: Option<Provider>,
    error: Option<String>,
}

/// Owns heavy sessions once; status reads do not wait for an active transcription.
pub struct EngineState {
    engine: Mutex<Option<TranscriberEngine>>,
    snapshot: Mutex<EngineSnapshot>,
}

impl Default for EngineState {
    /// Starts in loading state before the one application startup worker is spawned.
    fn default() -> Self {
        Self {
            engine: Mutex::new(None),
            snapshot: Mutex::new(EngineSnapshot {
                phase: Phase::Loading,
                provider: None,
                error: None,
            }),
        }
    }
}

impl EngineState {
    /// Publishes the startup result after storing any successfully loaded engine.
    fn finish(&self, result: Result<TranscriberEngine, EngineError>) -> Result<(), EngineError> {
        let mut engine = self.engine.lock().map_err(|_| EngineError::Unavailable)?;
        let mut snapshot = self.snapshot.lock().map_err(|_| EngineError::Unavailable)?;
        if snapshot.phase != Phase::Loading {
            return Err(EngineError::Unavailable);
        }
        match result {
            Ok(value) => {
                snapshot.provider = Some(value.provider());
                *engine = Some(value);
                snapshot.phase = Phase::Ready;
            }
            Err(error) => {
                snapshot.phase = Phase::Unavailable;
                snapshot.error = Some(error.to_string());
            }
        }
        Ok(())
    }

    /// Borrows the retained engine exclusively; callers must use a blocking worker.
    pub fn with_engine<T>(
        &self,
        operation: impl FnOnce(&mut TranscriberEngine) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let mut engine = self.engine.lock().map_err(|_| EngineError::Unavailable)?;
        operation(engine.as_mut().ok_or(EngineError::Unavailable)?)
    }

    /// Returns a cheap snapshot without acquiring the heavy-engine mutex.
    fn snapshot(&self) -> Result<EngineSnapshot, EngineError> {
        Ok(self
            .snapshot
            .lock()
            .map_err(|_| EngineError::Unavailable)?
            .clone())
    }
}

/// Starts exactly one model-loading worker from Tauri setup without blocking UI.
pub(crate) fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(|| {
            let dir = runtime::absolute_env("AURORA_ASR_MODEL_DIR")?;
            TranscriberEngine::new(&dir)
        })
        .await
        .unwrap_or(Err(EngineError::Unavailable));
        if app.state::<EngineState>().finish(result).is_err() {
            eprintln!("AURORA: unable to publish transcription engine status.");
        }
    });
}

/// Returns the startup snapshot without accepting resource paths from the WebView.
#[tauri::command]
pub fn transcriber_state(state: State<'_, EngineState>) -> Result<EngineSnapshot, String> {
    state.snapshot().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Checks terminal failure, safe serialization, and rejection of repeated setup.
    #[test]
    fn unavailable_state_is_terminal_and_path_free() -> anyhow::Result<()> {
        let state = EngineState::default();
        assert_eq!(state.snapshot()?.phase, Phase::Loading);
        state.finish(Err(EngineError::RuntimeLibrary))?;
        let snapshot = state.snapshot()?;
        assert_eq!(snapshot.phase, Phase::Unavailable);
        assert!(snapshot.provider.is_none());
        assert!(state.with_engine(|_| Ok(())).is_err());
        assert!(state.finish(Err(EngineError::Configuration)).is_err());
        let json = serde_json::to_value(snapshot)?;
        assert_eq!(json["phase"], "unavailable");
        assert!(json.get("path").is_none());
        Ok(())
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Publishes a real loaded engine once and checks reuse and independent snapshots.
    #[test]
    #[ignore = "requires pinned local ASR resources and ONNX Runtime 1.20.1"]
    fn ready_engine_is_reused() -> anyhow::Result<()> {
        let dir = runtime::absolute_env("AURORA_ASR_MODEL_DIR")?;
        let state = EngineState::default();
        state.finish(TranscriberEngine::new_cpu(&dir))?;
        assert_eq!(state.snapshot()?.phase, Phase::Ready);
        let first = state.with_engine(|engine| {
            assert_eq!(engine.transcribe_chunk(&[])?, "");
            Ok(engine as *const TranscriberEngine as usize)
        })?;
        let second = state.with_engine(|engine| Ok(engine as *const TranscriberEngine as usize))?;
        assert_eq!(first, second);
        let _busy_engine = state.engine.lock().map_err(|_| EngineError::Unavailable)?;
        assert_eq!(state.snapshot()?.phase, Phase::Ready);
        Ok(())
    }
}
