//! Local, atomic transcription preferences independent of model residency.
use crate::engine::{AsrModelId, EngineState};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};

const READ_WARNING: &str = "Transcription settings could not be read. Auto-detect and Parakeet are active. Select a language to save your preference again.";
const SAVE_ERROR: &str =
    "Unable to save transcription settings. Your previous preference is still active.";

/// Supported source-language hints; Auto leaves language handling to the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptionLanguage {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "en")]
    English,
}
impl TranscriptionLanguage {
    /// Converts the saved preference into the native source-language hint.
    pub(crate) fn hint(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Italian => Some("it"),
            Self::English => Some("en"),
        }
    }
}

/// Versioned preference file owned exclusively by the native application.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    version: u32,
    language: TranscriptionLanguage,
    #[serde(default)]
    selected_model: Option<AsrModelId>,
}

/// Effective preference and a recoverable configuration warning for the UI.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub language: TranscriptionLanguage,
    pub selected_model: AsrModelId,
    pub warning: Option<String>,
}

/// Serializes preference reads and commits without retaining audio or model data.
pub struct TranscriptionSettings {
    configuration: PathBuf,
    value: Mutex<SettingsSnapshot>,
}
impl TranscriptionSettings {
    /// Restores a valid preference or reports a read failure with Auto-detect fallback.
    pub fn new(base: &Path) -> Self {
        let configuration = base.join("settings.json");
        let result = (|| -> anyhow::Result<(TranscriptionLanguage, AsrModelId)> {
            let file = match std::fs::File::open(&configuration) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok((TranscriptionLanguage::Auto, AsrModelId::default()))
                }
                Err(error) => return Err(error.into()),
            };
            let value: Configuration = serde_json::from_reader(file)?;
            anyhow::ensure!(
                value.version == 1 || value.version == 2,
                "Unsupported settings version"
            );
            anyhow::ensure!(
                value.version == 1 || value.selected_model.is_some(),
                "Missing model preference"
            );
            Ok((value.language, value.selected_model.unwrap_or_default()))
        })();
        let value = match result {
            Ok((language, selected_model)) => SettingsSnapshot {
                language,
                selected_model,
                warning: None,
            },
            Err(_) => SettingsSnapshot {
                language: TranscriptionLanguage::Auto,
                selected_model: AsrModelId::default(),
                warning: Some(READ_WARNING.into()),
            },
        };
        Self {
            configuration,
            value: Mutex::new(value),
        }
    }

    /// Returns the effective preference without exposing its native storage path.
    pub(crate) fn snapshot(&self) -> Result<SettingsSnapshot, String> {
        self.value
            .lock()
            .map(|value| value.clone())
            .map_err(|_| "Language settings are unavailable.".into())
    }

    /// Resolves an optional file override without reading or modifying the saved default when supplied.
    pub(crate) fn language_for(
        &self,
        override_language: Option<TranscriptionLanguage>,
    ) -> Result<TranscriptionLanguage, String> {
        match override_language {
            Some(language) => Ok(language),
            None => self.snapshot().map(|settings| settings.language),
        }
    }

    /// Commits an atomic replacement before updating the effective in-memory preference.
    fn save(&self, language: TranscriptionLanguage) -> Result<SettingsSnapshot, String> {
        self.save_preferences(Some(language), None)
    }

    /// Persists the chosen ASR while preserving the current source-language preference.
    pub(crate) fn save_model(&self, model: AsrModelId) -> Result<SettingsSnapshot, String> {
        self.save_preferences(None, Some(model))
    }

    /// Commits both preferences under one lock; failed writes preserve disk and live state.
    fn save_preferences(
        &self,
        language: Option<TranscriptionLanguage>,
        model: Option<AsrModelId>,
    ) -> Result<SettingsSnapshot, String> {
        let mut value = self.value.lock().map_err(|_| SAVE_ERROR)?;
        let language = language.unwrap_or(value.language);
        let selected_model = model.unwrap_or(value.selected_model);
        let parent = self.configuration.parent().ok_or(SAVE_ERROR)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| SAVE_ERROR)?;
        serde_json::to_writer(
            &mut temporary,
            &Configuration {
                version: 2,
                language,
                selected_model: Some(selected_model),
            },
        )
        .map_err(|_| SAVE_ERROR)?;
        temporary.flush().map_err(|_| SAVE_ERROR)?;
        temporary.as_file().sync_all().map_err(|_| SAVE_ERROR)?;
        // Keep the old file and live preference intact if replacement fails.
        temporary
            .persist(&self.configuration)
            .map_err(|_| SAVE_ERROR)?;
        *value = SettingsSnapshot {
            language,
            selected_model,
            warning: None,
        };
        Ok(value.clone())
    }
}

/// Reads the backend-owned preference independently of navigation and model loading.
#[tauri::command]
pub fn transcription_settings(
    state: State<'_, TranscriptionSettings>,
) -> Result<SettingsSnapshot, String> {
    state.snapshot()
}

/// Saves a validated language while excluding concurrent transcription and model operations.
#[tauri::command]
pub async fn set_transcription_language(
    app: tauri::AppHandle,
    language: TranscriptionLanguage,
) -> Result<SettingsSnapshot, String> {
    save_language(app, language).await
}

/// Holds the shared reservation until the blocking atomic save finishes, including failures.
async fn save_language<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    language: TranscriptionLanguage,
) -> Result<SettingsSnapshot, String> {
    app.state::<EngineState>().reserve()?;
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        worker.state::<TranscriptionSettings>().save(language)
    })
    .await
    .map_err(|_| SAVE_ERROR.to_string())
    .and_then(|result| result);
    app.state::<EngineState>().release();
    result
}

#[cfg(test)]
mod tests;
