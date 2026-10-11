//! Shared serial chunk orchestration, independent of native model ownership.
use super::SubtitleSegment;
use crate::{engine::EngineError, vad::AudioChunk};
use transcribe_cpp::CancelToken;

/// Processes borrowed chunks in order, publishing only results completed before cancellation.
///
/// Native and simulated inference use the same boundaries. The caller owns session teardown
/// and discards all previously published segments if this operation fails or is cancelled.
pub(super) fn transcribe(
    chunks: &[AudioChunk<'_>],
    cancel: &CancelToken,
    mut infer: impl FnMut(&[f32]) -> Result<String, EngineError>,
    mut publish: impl FnMut(usize, SubtitleSegment),
) -> Result<(), String> {
    check(cancel)?;
    for (index, chunk) in chunks.iter().enumerate() {
        check(cancel)?;
        let text = infer(chunk.samples).map_err(|error| {
            // Preserve the typed native abort as cancellation before the IPC-safe conversion.
            if matches!(error, EngineError::Cancelled) {
                cancel.cancel();
            }
            error.to_string()
        })?;
        // Cancellation during inference wins before partial text can reach the UI.
        check(cancel)?;
        publish(
            index + 1,
            SubtitleSegment {
                start_sec: chunk.start_sec,
                end_sec: chunk.end_sec,
                text,
            },
        );
    }
    check(cancel)
}

/// Checks a job-scoped atomic token without locking inference or progress state.
fn check(cancel: &CancelToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err(EngineError::Cancelled.to_string())
    } else {
        Ok(())
    }
}
