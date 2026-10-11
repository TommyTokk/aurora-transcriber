//! Single-job offline transcription with cooperative cancellation and recoverable snapshots.
use crate::{audio::AudioDecoder, engine::EngineState, import::ImportState};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

/// One editable text chunk with immutable original-file boundaries in seconds.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtitleSegment {
    pub start_sec: f64,
    pub end_sec: f64,
    pub text: String,
}
/// Exhaustive processing phases shared by snapshots and progress events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionPhase {
    #[default]
    Idle,
    Loading,
    Decoding,
    Vad,
    Transcribing,
    Completed,
    Cancelled,
    Error,
}
/// Recoverable job snapshot; only completed jobs may be exported or saved.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSnapshot {
    pub job_id: Option<String>,
    pub revision: u64,
    pub phase: TranscriptionPhase,
    pub name: String,
    pub current_chunk: usize,
    pub total_chunks: usize,
    pub percentage: Option<f32>,
    pub segments: Vec<SubtitleSegment>,
    pub error: Option<String>,
}
/// Serializes job creation, cancellation, and progress publication.
#[derive(Default)]
pub struct TranscriptionState(Mutex<Job>);
/// Cancellation belongs to exactly one job and is reset on reservation.
#[derive(Default)]
struct Job {
    snapshot: TranscriptionSnapshot,
    active: bool,
    cancel: bool,
}
impl TranscriptionState {
    /// Starts one job while rejecting duplicate commands before any audio work.
    fn begin(&self, name: String) -> Result<String, String> {
        let mut job = self.0.lock().map_err(|_| "Transcription is unavailable.")?;
        if job.active {
            return Err("A transcription is already in progress.".into());
        }
        let id = Uuid::new_v4().to_string();
        let revision = job.snapshot.revision + 1;
        *job = Job {
            active: true,
            cancel: false,
            snapshot: TranscriptionSnapshot {
                job_id: Some(id.clone()),
                revision,
                phase: TranscriptionPhase::Loading,
                name,
                ..Default::default()
            },
        };
        Ok(id)
    }
    /// Checks cancellation at a safe boundary, without interrupting native inference calls.
    fn check(&self) -> Result<(), String> {
        let job = self.0.lock().map_err(|_| "Transcription is unavailable.")?;
        if job.cancel {
            Err("Transcription cancelled.".into())
        } else {
            Ok(())
        }
    }
    /// Mutates and emits snapshots in revision order under the same short lock.
    fn update<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        operation: impl FnOnce(&mut TranscriptionSnapshot),
    ) {
        if let Ok(mut job) = self.0.lock() {
            operation(&mut job.snapshot);
            job.snapshot.revision += 1;
            let _ = app.emit_to("main", "transcription-progress", &job.snapshot);
        }
    }
    /// Publishes terminal status, discarding partial text on every failure or cancellation.
    fn finish<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, result: Result<(), String>) {
        if let Ok(mut job) = self.0.lock() {
            job.active = false;
            // A cancellation arriving after the last chunk still wins before publication.
            if job.cancel {
                job.snapshot.phase = TranscriptionPhase::Cancelled;
                job.snapshot.segments.clear();
                job.snapshot.error = None;
            } else if let Err(error) = result {
                job.snapshot.phase = TranscriptionPhase::Error;
                job.snapshot.error = Some(error);
                job.snapshot.segments.clear();
            } else {
                job.snapshot.phase = TranscriptionPhase::Completed;
                job.snapshot.percentage = Some(100.0);
            }
            job.snapshot.revision += 1;
            let _ = app.emit_to("main", "transcription-progress", &job.snapshot);
        }
    }
    /// Clones a path-free snapshot independently of the heavy inference lock.
    fn snapshot(&self) -> Result<TranscriptionSnapshot, String> {
        self.0
            .lock()
            .map(|j| j.snapshot.clone())
            .map_err(|_| "Transcription is unavailable.".into())
    }
}
/// Runs decoding and the offline pipeline while pinning resident ASR weights and VAD.
fn process<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &std::path::Path,
) -> Result<(), String> {
    let job = app.state::<TranscriptionState>();
    job.check()?;
    app.state::<EngineState>().with_sessions(app, |sessions| {
        job.check()?;
        job.update(app, |s| s.phase = TranscriptionPhase::Decoding);
        let samples = AudioDecoder::decode_to_16khz_mono(path).map_err(|_| {
            "Unable to decode this audio file. It may have changed since import.".to_string()
        })?;
        let result = process_samples(app, sessions, &samples);
        drop(samples);
        result
    })
}
/// Processes borrowed mono samples through the same VAD, progress, and scoped ASR path.
fn process_samples<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    sessions: &mut crate::engine::state::Sessions,
    samples: &[f32],
) -> Result<(), String> {
    let job = app.state::<TranscriptionState>();
    job.check()?;
    job.update(app, |s| s.phase = TranscriptionPhase::Vad);
    let chunks = sessions
        .vad
        .chunk_audio(samples)
        .map_err(|_| "Voice detection failed.".to_string())?;
    job.check()?;
    job.update(app, |s| {
        s.phase = TranscriptionPhase::Transcribing;
        s.total_chunks = chunks.len();
        s.percentage = Some(0.0);
    });
    // One RAII session owns native results for this file and drops on every exit.
    sessions
        .asr
        .with_file(|file| -> Result<(), String> {
            for (index, chunk) in chunks.iter().enumerate() {
                job.check()?;
                let text = file
                    .transcribe_chunk(chunk.samples)
                    .map_err(|error| error.to_string())?;
                job.check()?;
                job.update(app, |s| {
                    s.segments.push(SubtitleSegment {
                        start_sec: chunk.start_sec,
                        end_sec: chunk.end_sec,
                        text,
                    });
                    s.current_chunk = index + 1;
                    s.percentage = Some((index + 1) as f32 / chunks.len() as f32 * 100.0);
                });
            }
            Ok(())
        })
        .map_err(|error| error.to_string())??;
    // Chunks borrow caller audio; session teardown has already freed native results.
    drop(chunks);
    Ok(())
}
/// Reserves an opaque native selection and starts one cancellable offline job.
#[tauri::command]
pub async fn process_audio_file(
    app: tauri::AppHandle,
    audio_id: String,
) -> Result<TranscriptionSnapshot, String> {
    app.state::<EngineState>().reserve()?;
    let (path, name) = match app.state::<ImportState>().acquire(&app, &audio_id) {
        Ok(value) => value,
        Err(error) => {
            app.state::<EngineState>().release();
            return Err(error);
        }
    };
    if let Err(error) = app.state::<TranscriptionState>().begin(name) {
        app.state::<ImportState>().release(&app);
        app.state::<EngineState>().release();
        return Err(error);
    }
    app.state::<TranscriptionState>().update(&app, |_| {});
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || process(&worker, &path))
        .await
        .map_err(|_| "Transcription worker failed.".to_string())
        .and_then(|r| r);
    app.state::<TranscriptionState>().finish(&app, result);
    app.state::<ImportState>().release(&app);
    app.state::<EngineState>().release();
    app.state::<TranscriptionState>().snapshot()
}
/// Requests cancellation of the current job without terminating active inference.
#[tauri::command]
pub fn cancel_transcription(state: State<'_, TranscriptionState>) -> Result<(), String> {
    let mut job = state
        .0
        .lock()
        .map_err(|_| "Transcription is unavailable.")?;
    if job.active {
        job.cancel = true;
    }
    Ok(())
}
/// Recovers current progress or the last terminal result after frontend subscription.
#[tauri::command]
pub fn transcription_state(
    state: State<'_, TranscriptionState>,
) -> Result<TranscriptionSnapshot, String> {
    state.snapshot()
}
#[cfg(test)]
mod smoke;
#[cfg(test)]
mod tests;
