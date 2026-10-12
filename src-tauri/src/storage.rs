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
        self.save_with_source(id, name, segments, None)
    }
    fn save_with_source(
        &self,
        id: Option<String>,
        name: String,
        segments: Vec<SubtitleSegment>,
        source: Option<crate::playback::SourceMetadata>,
    ) -> Result<TranscriptDocument, String> {
        let now = unix_seconds()?;
        let (id, created_at, version, source) = match id {
            Some(id) => {
                let old = self.read(&id)?;
                (id, old.created_at, old.version, old.source)
            }
            None => (
                Uuid::new_v4().to_string(),
                now,
                if source.is_some() { 2 } else { 1 },
                source,
            ),
        };
        let value = TranscriptDocument {
            version,
            source,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::playback::SourceMetadata>,
}
impl TranscriptDocument {
    /// Rejects unsupported versions, invalid identifiers, names, and segment timelines.
    fn validate(&self) -> Result<(), String> {
        if !matches!(self.version, 1 | 2)
            || Uuid::parse_str(&self.id).is_err()
            || self.name.trim().is_empty()
            || self.name.len() > 255
            || self.name.contains(['/', '\\'])
            || self.updated_at < self.created_at
        {
            return Err("Invalid transcript document.".into());
        }
        match (self.version, &self.source) {
            (1, None) => {}
            (2, Some(source))
                if source.duration_seconds.is_finite()
                    && source.duration_seconds > 0.0
                    && source.sha256.len() == 64
                    && source.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && self
                        .segments
                        .last()
                        .is_none_or(|s| s.end_sec <= source.duration_seconds) => {}
            _ => return Err("Invalid transcript source metadata.".into()),
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
    audio_id: Option<String>,
) -> Result<TranscriptDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = audio_id
            .as_deref()
            .map(|id| app.state::<crate::playback::PlaybackState>().metadata(id))
            .transpose()?
            .flatten();
        if source.is_some() {
            app.state::<Storage>()
                .save_with_source(id, name, segments, source)
        } else {
            app.state::<Storage>().save(id, name, segments)
        }
    })
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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTranscript {
    document: TranscriptDocument,
    audio_id: String,
}

/// Reopens one explicitly selected local document on a blocking worker.
#[tauri::command]
pub async fn open_transcript(app: tauri::AppHandle, id: String) -> Result<OpenTranscript, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let document = app.state::<Storage>().read(&id)?;
        let audio_id = app.state::<crate::playback::PlaybackState>().register(
            None,
            document.source.clone(),
            &document.segments,
        )?;
        Ok(OpenTranscript { document, audio_id })
    })
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
    #[test]
    fn new_documents_preserve_blocks_and_source_while_legacy_stays_v1() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let storage = Storage::new(dir.path())?;
        let blocks = || {
            vec![
                SubtitleSegment {
                    start_sec: 0.0,
                    end_sec: 600.0,
                    text: "corrected words".into(),
                },
                SubtitleSegment {
                    start_sec: 600.0,
                    end_sec: 900.0,
                    text: "last block".into(),
                },
            ]
        };
        let source = crate::playback::SourceMetadata {
            duration_seconds: 900.0,
            sha256: "a".repeat(64),
        };
        let document = storage
            .save_with_source(None, "voice.wav".into(), blocks(), Some(source.clone()))
            .map_err(anyhow::Error::msg)?;
        assert_eq!(document.version, 2);
        let reopened = storage.read(&document.id).map_err(anyhow::Error::msg)?;
        assert_eq!(reopened.segments.len(), 2);
        assert_eq!(reopened.segments[1].start_sec, 600.0);
        assert_eq!(
            reopened.source.as_ref().map(|s| s.sha256.as_str()),
            Some(source.sha256.as_str())
        );
        let updated = storage
            .save(Some(document.id), "voice.wav".into(), blocks())
            .map_err(anyhow::Error::msg)?;
        assert_eq!(updated.version, 2);
        assert!(updated.source.is_some());
        let raw = serde_json::to_string(&updated)?;
        assert!(!raw.contains("audioId") && !raw.contains("path"));
        let legacy = storage
            .save(None, "old.wav".into(), blocks())
            .map_err(anyhow::Error::msg)?;
        let legacy = storage
            .save_with_source(Some(legacy.id), "old.wav".into(), blocks(), Some(source))
            .map_err(anyhow::Error::msg)?;
        assert_eq!(legacy.version, 1);
        assert!(legacy.source.is_none());
        let txt =
            crate::export::format_segments(&updated.segments, crate::export::ExportFormat::Txt)
                .map_err(anyhow::Error::msg)?;
        assert!(txt.contains("corrected words"));
        let srt =
            crate::export::format_segments(&updated.segments, crate::export::ExportFormat::Srt)
                .map_err(anyhow::Error::msg)?;
        assert!(srt.contains("00:00:00,000 --> 00:10:00,000"));
        assert!(srt.contains("00:10:00,000 --> 00:15:00,000"));
        Ok(())
    }
    #[test]
    fn invalid_source_metadata_is_rejected() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let storage = Storage::new(dir.path())?;
        for (duration, sha256) in [
            (f64::NAN, "a".repeat(64)),
            (0.0, "a".repeat(64)),
            (100.0, "invalid".into()),
        ] {
            assert!(storage
                .save_with_source(
                    None,
                    "voice.wav".into(),
                    vec![],
                    Some(crate::playback::SourceMetadata {
                        duration_seconds: duration,
                        sha256
                    })
                )
                .is_err());
        }
        Ok(())
    }
}
