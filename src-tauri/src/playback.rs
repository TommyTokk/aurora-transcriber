//! Session-scoped source authorization and bounded, in-memory review audio.
use crate::{
    audio::{AudioDecoder, TARGET_SAMPLE_RATE},
    transcription::SubtitleSegment,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceMetadata {
    pub duration_seconds: f64,
    pub sha256: String,
}

#[derive(Clone)]
struct Source {
    id: String,
    path: Option<PathBuf>,
    metadata: Option<SourceMetadata>,
    ranges: Vec<(f64, f64)>,
    completed_job: bool,
}

#[derive(Default)]
struct Associations {
    sources: HashMap<String, Source>,
    loading: Option<String>,
}

#[derive(Default)]
pub struct PlaybackState {
    sources: Mutex<Associations>,
    generation: AtomicU64,
    // Serial workers keep rapid block changes from decoding several clips concurrently.
    worker: Mutex<()>,
}

pub(crate) fn fingerprint(path: &Path, cancelled: impl Fn() -> bool) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "Unable to read the original audio.")?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        if cancelled() {
            return Err("Audio loading cancelled.".into());
        }
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Unable to read the original audio.")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl PlaybackState {
    pub(crate) fn register(
        &self,
        path: Option<PathBuf>,
        metadata: Option<SourceMetadata>,
        segments: &[SubtitleSegment],
    ) -> Result<String, String> {
        let mut associations = self
            .sources
            .lock()
            .map_err(|_| "Audio review is unavailable.")?;
        self.generation.fetch_add(1, Ordering::SeqCst);
        associations.loading = None;
        let completed_job = path.is_some();
        // Keep only the last completed job and one archive document. This lets a
        // recovering frontend use the last job snapshot after opening the archive.
        associations
            .sources
            .retain(|_, source| source.completed_job && !completed_job);
        let id = uuid::Uuid::new_v4().to_string();
        associations.sources.insert(
            id.clone(),
            Source {
                id: id.clone(),
                path,
                metadata,
                completed_job,
                ranges: segments.iter().map(|s| (s.start_sec, s.end_sec)).collect(),
            },
        );
        Ok(id)
    }
    fn get(&self, id: &str) -> Result<Source, String> {
        self.sources
            .lock()
            .map_err(|_| "Audio review is unavailable.")?
            .sources
            .get(id)
            .cloned()
            .ok_or_else(|| "This audio association is no longer available.".into())
    }
    fn reserve_load(&self, id: &str) -> Result<(Source, u64), String> {
        let mut associations = self
            .sources
            .lock()
            .map_err(|_| "Audio review is unavailable.")?;
        let source = associations
            .sources
            .get(id)
            .cloned()
            .ok_or("This audio association is no longer available.")?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        associations.loading = Some(id.to_owned());
        Ok((source, generation))
    }

    fn release(&self, id: &str) -> Result<(), String> {
        let mut associations = self
            .sources
            .lock()
            .map_err(|_| "Audio review is unavailable.")?;
        if associations.loading.as_deref() == Some(id) {
            self.generation.fetch_add(1, Ordering::SeqCst);
            associations.loading = None;
        }
        Ok(())
    }

    pub(crate) fn metadata(&self, id: &str) -> Result<Option<SourceMetadata>, String> {
        Ok(self.get(id)?.metadata)
    }
}

/// Re-associates through a native dialog; selection cancellation preserves the current source.
#[tauri::command]
pub async fn associate_transcript_audio(
    app: tauri::AppHandle,
    audio_id: String,
) -> Result<bool, String> {
    let source = app.state::<PlaybackState>().get(&audio_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<PlaybackState>();
        let selection = app
            .dialog()
            .file()
            .set_title("Associate original audio")
            .add_filter("Audio", &["wav", "mp3", "flac", "m4a", "ogg"])
            .blocking_pick_file();
        let Some(selection) = selection else {
            return Ok(false);
        };
        let path = selection
            .into_path()
            .map_err(|_| "Unable to access this audio.")?;
        if let Some(metadata) = &source.metadata {
            if fingerprint(&path, || false)? != metadata.sha256 {
                return Err("This file does not match the original audio.".into());
            }
        } else {
            let duration = AudioDecoder::duration_seconds(&path)
                .map_err(|_| "Unable to decode this audio.")?;
            if source
                .ranges
                .last()
                .is_some_and(|(_, end)| *end > duration + 0.0001)
            {
                return Err("This audio does not cover all transcript timestamps.".into());
            }
        }
        let mut associations = state
            .sources
            .lock()
            .map_err(|_| "Audio review is unavailable.")?;
        let current = associations
            .sources
            .get_mut(&source.id)
            .ok_or("This transcript is no longer open.")?;
        current.path = Some(path);
        if associations.loading.as_deref() == Some(&source.id) {
            state.generation.fetch_add(1, Ordering::SeqCst);
            associations.loading = None;
        }
        Ok(true)
    })
    .await
    .map_err(|_| "Audio association could not finish.")?
}

/// Resolves immutable source boundaries in Rust; no WebView paths or times are trusted.
#[tauri::command]
pub async fn load_transcript_block(
    app: tauri::AppHandle,
    audio_id: String,
    index: usize,
) -> Result<tauri::ipc::Response, String> {
    let state = app.state::<PlaybackState>();
    let (source, generation) = state.reserve_load(&audio_id)?;
    let (start, end) = *source.ranges.get(index).ok_or("Invalid audio block.")?;
    let path = source
        .path
        .ok_or("Associate the original audio before playback.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<PlaybackState>();
        let _worker = state
            .worker
            .lock()
            .map_err(|_| "Audio review is unavailable.")?;
        let cancelled = || state.generation.load(Ordering::SeqCst) != generation;
        if cancelled() {
            return Err("Audio loading cancelled.".into());
        }
        if let Some(metadata) = source.metadata {
            if fingerprint(&path, cancelled)? != metadata.sha256 {
                return Err("The original audio has changed. Associate it again.".into());
            }
        }
        let range = (start * TARGET_SAMPLE_RATE as f64).round() as usize
            ..(end * TARGET_SAMPLE_RATE as f64).round() as usize;
        let expected = range.len();
        let samples = AudioDecoder::decode_range(&path, range, cancelled)
            .map_err(|_| "Unable to load this audio block. The file may be missing or changed.")?;
        if cancelled() {
            return Err("Audio loading cancelled.".into());
        }
        if samples.len() != expected {
            return Err("The audio no longer covers this block.".into());
        }
        let mut output = Cursor::new(Vec::new());
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: TARGET_SAMPLE_RATE as u32,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::new(&mut output, spec)
                .map_err(|_| "Unable to prepare audio playback.")?;
            for packet in samples.chunks(4096) {
                if cancelled() {
                    return Err("Audio loading cancelled.".into());
                }
                for sample in packet {
                    writer
                        .write_sample((sample * i16::MAX as f32).round() as i16)
                        .map_err(|_| "Unable to prepare audio playback.")?;
                }
            }
            writer
                .finalize()
                .map_err(|_| "Unable to prepare audio playback.")?;
        }
        drop(samples);
        if cancelled() {
            return Err("Audio loading cancelled.".into());
        }
        Ok(tauri::ipc::Response::new(output.into_inner()))
    })
    .await
    .map_err(|_| "Audio loading could not finish.")?
}

/// Invalidates in-flight work without retaining decoded samples between requests.
#[tauri::command]
pub fn release_transcript_audio(
    state: State<'_, PlaybackState>,
    audio_id: String,
) -> Result<(), String> {
    state.release(&audio_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn associations_are_opaque_and_replaced() -> Result<(), String> {
        let state = PlaybackState::default();
        let first = state.register(None, None, &[])?;
        assert!(state.get("../../secret").is_err());
        assert!(state.get(&first)?.path.is_none());
        let second = state.register(None, None, &[])?;
        assert!(state.get(&first).is_err());
        assert!(state.get(&second).is_ok());
        Ok(())
    }
    #[test]
    fn fingerprints_detect_changed_files() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("source");
        std::fs::write(&path, b"original")?;
        let first = fingerprint(&path, || false).map_err(anyhow::Error::msg)?;
        std::fs::write(&path, b"modified")?;
        assert_ne!(
            first,
            fingerprint(&path, || false).map_err(anyhow::Error::msg)?
        );
        assert!(fingerprint(&path, || true).is_err());
        Ok(())
    }
    #[test]
    fn completed_job_survives_archive_open_and_stale_release() -> Result<(), String> {
        let state = PlaybackState::default();
        let job = state.register(Some(PathBuf::from("native-selection")), None, &[])?;
        let archive = state.register(None, None, &[])?;
        assert!(state.get(&job)?.path.is_some());
        let (_, generation) = state.reserve_load(&archive)?;
        state.release(&job)?;
        assert_eq!(state.generation.load(Ordering::SeqCst), generation);
        state.release(&archive)?;
        assert!(state.generation.load(Ordering::SeqCst) > generation);
        let next_archive = state.register(None, None, &[])?;
        assert!(state.get(&archive).is_err());
        assert!(state.get(&job).is_ok());
        state.register(Some(PathBuf::from("new-native-selection")), None, &[])?;
        assert!(state.get(&job).is_err());
        assert!(state.get(&next_archive).is_err());
        Ok(())
    }
}
