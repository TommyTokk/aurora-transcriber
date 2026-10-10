use std::ops::Range;

use crate::audio::TARGET_SAMPLE_RATE;

use super::{AudioChunk, VadError};

const MIN_SPEECH: usize = TARGET_SAMPLE_RATE * 3 / 2;
const MIN_SILENCE: usize = TARGET_SAMPLE_RATE * 3 / 10;
const LONG_SILENCE: usize = TARGET_SAMPLE_RATE;
const TARGET_CHUNK: usize = TARGET_SAMPLE_RATE * 15;
const MAX_CHUNK: usize = TARGET_SAMPLE_RATE * 25;
const MARGIN: usize = TARGET_SAMPLE_RATE * 3 / 100;

/// Per-call hysteresis state and accepted speech ranges, never sample copies.
#[derive(Default)]
pub(super) struct Segmenter {
    start: Option<usize>,
    silence: Option<usize>,
    speech: Vec<Range<usize>>,
}

impl Segmenter {
    /// Consumes one frame's probability and its real, unpadded sample boundaries.
    pub(super) fn push(&mut self, probability: f32, frame: Range<usize>) -> Result<(), VadError> {
        if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
            return Err(VadError::InvalidOutput);
        }
        // Separate enter/exit thresholds prevent uncertain frames from flickering.
        if probability >= 0.50 {
            self.start.get_or_insert(frame.start);
            self.silence = None;
        } else if probability < 0.35 && self.start.is_some() {
            let silence = *self.silence.get_or_insert(frame.start);
            if frame.end - silence >= MIN_SILENCE {
                self.close(silence);
            }
        }
        Ok(())
    }

    /// Accepts an event only if its unpadded duration reaches the speech minimum.
    fn close(&mut self, end: usize) {
        if let Some(start) = self.start.take() {
            if end - start >= MIN_SPEECH {
                self.speech.push(start..end);
            }
        }
        self.silence = None;
    }

    /// Closes trailing speech, groups events, and borrows bounded audio slices.
    pub(super) fn finish(mut self, samples: &[f32]) -> Vec<AudioChunk<'_>> {
        // An unconfirmed trailing pause belongs to the event until EOF.
        self.close(samples.len());
        let mut groups: Vec<Range<usize>> = Vec::new();
        for event in self.speech {
            if let Some(group) = groups.last_mut() {
                let gap = event.start - group.end;
                // After forced cuts, the remaining tail becomes the next target window.
                let tail_length = (group.end - group.start - 1) % MAX_CHUNK + 1;
                if gap < LONG_SILENCE && tail_length < TARGET_CHUNK {
                    group.end = event.end;
                    continue;
                }
            }
            groups.push(event);
        }

        let mut chunks = Vec::new();
        for group in groups {
            let mut start = group.start;
            while start < group.end {
                // Exact sample cuts preserve even a short tail of a valid event.
                let end = start.saturating_add(MAX_CHUNK).min(group.end);
                let spare = MAX_CHUNK - (end - start);
                let left = if start == group.start {
                    MARGIN.min(start).min(spare)
                } else {
                    0
                };
                let right = if end == group.end {
                    MARGIN.min(samples.len() - end).min(spare - left)
                } else {
                    0
                };
                let padded = start - left..end + right;
                chunks.push(AudioChunk {
                    samples: &samples[padded.clone()],
                    start_sec: padded.start as f64 / TARGET_SAMPLE_RATE as f64,
                    end_sec: padded.end as f64 / TARGET_SAMPLE_RATE as f64,
                });
                start = end;
            }
        }
        chunks
    }
}

#[cfg(test)]
mod tests;
