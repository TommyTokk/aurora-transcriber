//! Application-owned paths and explicit, atomic transcript persistence.
use crate::transcription::SubtitleSegment;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
use uuid::Uuid;

/// Backend-resolved directories; never accepted from WebView arguments.
pub struct Storage {
    pub transcripts: PathBuf,
}
impl Storage {
    /// Creates writable per-user model and transcript directories.
    pub fn new(base: &Path) -> std::io::Result<Self> {
        let value = Self {
            transcripts: base.join("transcripts"),
        };
        std::fs::create_dir_all(&value.transcripts)?;
        Ok(value)
    }
    /// Resolves a strictly validated identifier within the transcript directory.
    fn transcript_path(&self, id: &str) -> Result<PathBuf, String> {
        let id = Uuid::parse_str(id).map_err(|_| "Invalid transcript identifier.")?;
        Ok(self.transcripts.join(format!("{id}.json")))
    }
    /// Loads and validates one explicit user-saved document.
    pub(crate) fn read(&self, id: &str) -> Result<TranscriptDocument, String> {
        let file = std::fs::File::open(self.transcript_path(id)?)
            .map_err(|_| "Unable to open transcript.")?;
        let value: TranscriptDocument =
            serde_json::from_reader(file).map_err(|_| "Invalid transcript document.")?;
        value.validate()?;
        if value.id != id {
            return Err("Transcript identifier mismatch.".into());
        }
        Ok(value)
    }
    /// Saves a new document or atomically replaces the same existing document.
    pub(crate) fn save(
        &self,
        id: Option<String>,
        name: String,
        segments: Vec<SubtitleSegment>,
    ) -> Result<TranscriptDocument, String> {
        let now = unix_seconds()?;
        let (id, created_at) = match id {
            Some(id) => {
                let old = self.read(&id)?;
                (id, old.created_at)
            }
            None => (Uuid::new_v4().to_string(), now),
        };
        let value = TranscriptDocument {
            version: 1,
            id,
            name,
            created_at,
            updated_at: now,
            segments,
        };
        value.validate()?;
        // A private same-directory temporary file is published only after a complete write.
        let mut temporary = tempfile::NamedTempFile::new_in(&self.transcripts)
            .map_err(|_| "Unable to save transcript.")?;
        serde_json::to_writer(&mut temporary, &value).map_err(|_| "Unable to save transcript.")?;
        temporary
            .flush()
            .map_err(|_| "Unable to save transcript.")?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| "Unable to save transcript.")?;
        temporary
            .persist(self.transcript_path(&value.id)?)
            .map_err(|_| "Unable to save transcript.")?;
        Ok(value)
    }
    /// Lists valid documents, skipping corrupt entries without exposing their contents.
    fn list(&self) -> Result<Vec<TranscriptSummary>, String> {
        let entries =
            std::fs::read_dir(&self.transcripts).map_err(|_| "Unable to list transcripts.")?;
        let mut result = Vec::new();
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Some(id) = entry.path().file_stem().and_then(|s| s.to_str()) {
                if let Ok(document) = self.read(id) {
                    result.push(TranscriptSummary {
                        id: document.id,
                        name: document.name,
                        updated_at: document.updated_at,
                    });
                }
            }
        }
        result.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok(result)
    }
}
/// Versioned archive format containing text and timestamps, never source paths or audio.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptDocument {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub segments: Vec<SubtitleSegment>,
}
impl TranscriptDocument {
    /// Rejects unsupported versions, invalid identifiers, names, and segment timelines.
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || Uuid::parse_str(&self.id).is_err()
            || self.name.trim().is_empty()
            || self.name.len() > 255
            || self.name.contains(['/', '\\'])
            || self.updated_at < self.created_at
        {
            return Err("Invalid transcript document.".into());
        }
        crate::export::validate_segments(&self.segments)
    }
}
/// Lightweight local archive entry for the frontend list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSummary {
    pub id: String,
    pub name: String,
    pub updated_at: u64,
}
/// Returns UTC Unix seconds without relying on the local timezone.
fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| "System clock is unavailable.".into())
}
/// Writes a document only in response to the explicit Save action.
#[tauri::command]
pub async fn save_transcript(
    app: tauri::AppHandle,
    id: Option<String>,
    name: String,
    segments: Vec<SubtitleSegment>,
) -> Result<TranscriptDocument, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().save(id, name, segments))
        .await
        .map_err(|_| "Unable to save transcript.".to_string())?
}
/// Returns the local archive without accepting filesystem paths.
#[tauri::command]
pub async fn list_transcripts(app: tauri::AppHandle) -> Result<Vec<TranscriptSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().list())
        .await
        .map_err(|_| "Unable to list transcripts.".to_string())?
}
/// Reopens one explicitly selected local document on a blocking worker.
#[tauri::command]
pub async fn open_transcript(
    app: tauri::AppHandle,
    id: String,
) -> Result<TranscriptDocument, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().read(&id))
        .await
        .map_err(|_| "Unable to open transcript.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Checks stable IDs, replacement, corrupt entries, and path traversal rejection.
    #[test]
    fn archive_is_atomic_and_path_bound() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let storage = Storage::new(dir.path())?;
        let doc = storage
            .save(None, "voice.wav".into(), vec![])
            .map_err(anyhow::Error::msg)?;
        let updated = storage
            .save(Some(doc.id.clone()), "edited".into(), vec![])
            .map_err(anyhow::Error::msg)?;
        assert_eq!(doc.id, updated.id);
        assert_eq!(doc.created_at, updated.created_at);
        std::fs::write(storage.transcripts.join("broken.json"), b"invalid")?;
        assert_eq!(storage.list().map_err(anyhow::Error::msg)?.len(), 1);
        assert!(storage.read("../../secret").is_err());
        assert!(storage.save(None, "../secret".into(), vec![]).is_err());
        assert_eq!(
            storage.read(&doc.id).map_err(anyhow::Error::msg)?.name,
            "edited"
        );
        Ok(())
    }
}
