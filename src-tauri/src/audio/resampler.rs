use rubato::{FftFixedIn, Resampler};

use super::{AudioDecodeError, TARGET_SAMPLE_RATE};

const INPUT_BLOCK_FRAMES: usize = 1024;

/// Bounded planar staging buffers and the owned mono result for one audio stream.
pub(super) struct MonoPipeline {
    sample_rate: usize,
    channels: usize,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    resampler: Option<FftFixedIn<f32>>,
    filled: usize,
    input_frames: usize,
    delay_remaining: usize,
    mono: Vec<f32>,
    range: std::ops::Range<usize>,
    emitted: usize,
}

impl MonoPipeline {
    /// Creates reusable channel buffers for `sample_rate` and `channels`.
    /// Returns an error for invalid parameters or FFT initialization failure.
    pub(super) fn new(sample_rate: usize, channels: usize) -> Result<Self, AudioDecodeError> {
        Self::with_range(sample_rate, channels, 0..usize::MAX)
    }

    pub(super) fn with_range(
        sample_rate: usize,
        channels: usize,
        range: std::ops::Range<usize>,
    ) -> Result<Self, AudioDecodeError> {
        if sample_rate == 0 || channels == 0 {
            return Err(AudioDecodeError::InvalidParameters(
                "zero sample rate or channel count",
            ));
        }
        // Even FFT input/output sizes make the filter delay an integer sample count.
        // Otherwise output_delay() rounds a half-sample delay down at some rates.
        let minimum_input = sample_rate / num_integer::gcd(sample_rate, TARGET_SAMPLE_RATE);
        let block_frames = INPUT_BLOCK_FRAMES.div_ceil(2 * minimum_input) * 2 * minimum_input;
        // Already compatible audio avoids both FFT work and artificial filter delay.
        let resampler = if sample_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            Some(FftFixedIn::<f32>::new(
                sample_rate,
                TARGET_SAMPLE_RATE,
                block_frames,
                1,
                channels,
            )?)
        };
        let output = resampler
            .as_ref()
            .map_or_else(Vec::new, |value| value.output_buffer_allocate(true));
        let delay_remaining = resampler.as_ref().map_or(0, |value| value.output_delay());
        Ok(Self {
            sample_rate,
            channels,
            input: vec![vec![0.0; block_frames]; channels],
            output,
            resampler,
            filled: 0,
            input_frames: 0,
            delay_remaining,
            mono: Vec::new(),
            range,
            emitted: 0,
        })
    }

    /// Validates interleaved `samples`, separates channels, and processes full blocks.
    /// Returns an error for incomplete frames, non-finite samples, or resampling failure.
    pub(super) fn push_interleaved(&mut self, samples: &[f32]) -> Result<(), AudioDecodeError> {
        if !samples.len().is_multiple_of(self.channels) {
            return Err(AudioDecodeError::InvalidParameters(
                "incomplete interleaved frame",
            ));
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(AudioDecodeError::NonFiniteSample);
        }
        self.input_frames = self
            .input_frames
            .checked_add(samples.len() / self.channels)
            .ok_or(AudioDecodeError::InvalidParameters(
                "audio duration overflow",
            ))?;
        for frame in samples.chunks_exact(self.channels) {
            // Preserve each channel separately until resampling has completed.
            for (channel, sample) in self.input.iter_mut().zip(frame) {
                channel[self.filled] = *sample;
            }
            self.filled += 1;
            if self.filled == self.input[0].len() {
                self.process_block()?;
            }
        }
        Ok(())
    }

    /// Processes the current planar block, discards filter delay, and appends mono.
    /// Resampler errors and non-finite output are propagated to the caller.
    fn process_block(&mut self) -> Result<(), AudioDecodeError> {
        if let Some(resampler) = self.resampler.as_mut() {
            let (_, written) =
                resampler.process_into_buffer(&self.input, &mut self.output, None)?;
            // Remove startup delay incrementally without copying the entire result.
            let skip = self.delay_remaining.min(written);
            self.delay_remaining -= skip;
            append_mono(
                &self.output,
                skip,
                written,
                &mut self.mono,
                &self.range,
                &mut self.emitted,
            )?;
        } else {
            append_mono(
                &self.input,
                0,
                self.filled,
                &mut self.mono,
                &self.range,
                &mut self.emitted,
            )?;
        }
        self.filled = 0;
        Ok(())
    }

    pub(super) fn duration_seconds(&self) -> f64 {
        let frames = (self.input_frames as u128 * TARGET_SAMPLE_RATE as u128
            + self.sample_rate as u128 / 2)
            / self.sample_rate as u128;
        frames as f64 / TARGET_SAMPLE_RATE as f64
    }

    pub(super) fn range_complete(&self) -> bool {
        self.emitted >= self.range.end
    }

    /// Flushes the final block and delayed tail, returning exactly the rounded duration.
    /// Consumes the pipeline so all intermediate audio buffers are dropped afterwards.
    pub(super) fn finish(mut self) -> Result<Vec<f32>, AudioDecodeError> {
        if self.input_frames == 0 {
            return Err(AudioDecodeError::EmptyAudio);
        }
        let target_frames = usize::try_from(
            (self.input_frames as u128 * TARGET_SAMPLE_RATE as u128 + self.sample_rate as u128 / 2)
                / self.sample_rate as u128,
        )
        .map_err(|_| AudioDecodeError::InvalidParameters("output duration overflow"))?;
        if self.filled > 0 {
            // Zero padding completes the final FFT block without extending duration.
            for channel in &mut self.input {
                channel[self.filled..].fill(0.0);
            }
            self.process_block()?;
        }
        // Additional silent blocks release samples still held in the FFT overlap.
        if self.resampler.is_some() {
            for channel in &mut self.input {
                channel.fill(0.0);
            }
            while self.emitted < target_frames {
                self.process_block()?;
            }
        }
        self.mono.truncate(
            target_frames
                .min(self.range.end)
                .saturating_sub(self.range.start),
        );
        let mono = std::mem::take(&mut self.mono);
        drop(self);
        Ok(mono)
    }
}

/// Averages planar `channels[start..end]` into `mono`, rejecting non-finite output.
/// Uses a wider accumulator to avoid overflow, then bounds each sample to unity.
fn append_mono(
    channels: &[Vec<f32>],
    start: usize,
    end: usize,
    mono: &mut Vec<f32>,
    range: &std::ops::Range<usize>,
    emitted: &mut usize,
) -> Result<(), AudioDecodeError> {
    for index in start..end {
        let mut sum = 0.0_f64;
        for channel in channels {
            let sample = channel[index];
            if !sample.is_finite() {
                return Err(AudioDecodeError::NonFiniteSample);
            }
            sum += f64::from(sample);
        }
        if range.contains(emitted) {
            mono.push((sum / channels.len() as f64).clamp(-1.0, 1.0) as f32);
        }
        *emitted += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
