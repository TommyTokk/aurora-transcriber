use std::{fs::File, io::ErrorKind, path::Path};

use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};
use thiserror::Error;

use super::resampler::MonoPipeline;

/// Failures encountered while reading, decoding, or converting local audio.
#[derive(Debug, Error)]
pub enum AudioDecodeError {
    #[error("Audio loading cancelled")]
    Cancelled,
    #[error("Unable to read the audio file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Audio decoding failed: {0}")]
    Decode(#[from] SymphoniaError),
    #[error("No supported audio track was found")]
    UnsupportedTrack,
    #[error("The audio stream contains no samples")]
    EmptyAudio,
    #[error("Invalid audio parameters: {0}")]
    InvalidParameters(&'static str),
    #[error("The audio sample rate or channel layout changed during decoding")]
    ChangedParameters,
    #[error("The audio contains non-finite samples")]
    NonFiniteSample,
    #[error("The audio stream is truncated or failed integrity verification")]
    CorruptAudio,
    #[error("Unable to initialize the resampler: {0}")]
    ResamplerConstruction(#[from] rubato::ResamplerConstructionError),
    #[error("Audio resampling failed: {0}")]
    Resample(#[from] rubato::ResampleError),
}

/// Stateless decoder for local files; no models, network access, or global buffers.
pub struct AudioDecoder;

impl AudioDecoder {
    /// Decodes the first supported audio track at `path` into 16 kHz mono samples.
    ///
    /// Channels are separated before FFT resampling and averaged afterwards.
    /// The output is finite and bounded to `[-1.0, 1.0]`, with filter delay removed.
    /// Errors are returned for unreadable, unsupported, empty, corrupt, or changing
    /// streams. Intermediate buffers are dropped on both success and failure.
    ///
    /// This synchronous operation must run on a blocking worker when called by UI
    /// code. The caller owns the returned samples and must drop them after use.
    pub fn decode_to_16khz_mono(path: &Path) -> Result<Vec<f32>, AudioDecodeError> {
        Self::decode_range(path, 0..usize::MAX, || false)
    }

    /// Decodes with bounded output storage and cooperative cancellation at packet boundaries.
    pub fn decode_range(
        path: &Path,
        range: std::ops::Range<usize>,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<f32>, AudioDecodeError> {
        Self::decode_with_duration(path, range, cancelled).map(|(samples, _)| samples)
    }

    /// Validates the full stream without retaining decoded audio samples.
    pub fn duration_seconds(path: &Path) -> Result<f64, AudioDecodeError> {
        Self::decode_with_duration(path, usize::MAX..usize::MAX, || false)
            .map(|(_, duration)| duration)
    }

    fn decode_with_duration(
        path: &Path,
        range: std::ops::Range<usize>,
        cancelled: impl Fn() -> bool,
    ) -> Result<(Vec<f32>, f64), AudioDecodeError> {
        if range.start > range.end {
            return Err(AudioDecodeError::InvalidParameters("reversed audio range"));
        }
        let file = File::open(path)?;
        if file.metadata()?.len() == 0 {
            return Err(AudioDecodeError::EmptyAudio);
        }
        let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
            hint.with_extension(extension);
        }

        // Probe actual file contents and request codec-aware removal of padding.
        let mut format = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
                &MetadataOptions::default(),
            )?
            .format;
        let codecs = symphonia::default::get_codecs();
        let options = DecoderOptions { verify: true };
        let selected = format.tracks().iter().find_map(|track| {
            codecs
                .make(&track.codec_params, &options)
                .ok()
                .map(|decoder| (track.id, track.codec_params.clone(), decoder))
        });
        let (track_id, parameters, mut decoder) =
            selected.ok_or(AudioDecodeError::UnsupportedTrack)?;
        let mut pipeline = None;
        let mut initial_spec = None;
        let mut sample_buffer = None;
        let mut packet_end = parameters.start_ts;
        let mut range_complete = false;

        loop {
            if cancelled() {
                return Err(AudioDecodeError::Cancelled);
            }
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                // EOF is normal only at a packet boundary. Decoder EOF is an error.
                Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                    break
                }
                Err(SymphoniaError::ResetRequired) => {
                    return Err(AudioDecodeError::ChangedParameters)
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id() != track_id {
                continue;
            }
            packet_end = packet_end.max(packet.ts().saturating_add(packet.dur()));
            let decoded = match decoder.decode(&packet) {
                Err(SymphoniaError::ResetRequired) => {
                    return Err(AudioDecodeError::ChangedParameters)
                }
                result => result?,
            };
            let spec = *decoded.spec();
            if spec.rate == 0 || spec.channels.count() == 0 {
                return Err(AudioDecodeError::InvalidParameters(
                    "zero sample rate or channel count",
                ));
            }
            if initial_spec.is_some_and(|initial| initial != spec) {
                return Err(AudioDecodeError::ChangedParameters);
            }
            if initial_spec.is_none() {
                initial_spec = Some(spec);
                pipeline = Some(if range == (0..usize::MAX) {
                    MonoPipeline::new(spec.rate as usize, spec.channels.count())?
                } else {
                    MonoPipeline::with_range(
                        spec.rate as usize,
                        spec.channels.count(),
                        range.clone(),
                    )?
                });
            }
            if decoded.frames() == 0 {
                continue;
            }

            // Reuse the conversion buffer unless a larger decoded packet arrives.
            let required = decoded.capacity() * spec.channels.count();
            if sample_buffer
                .as_ref()
                .is_none_or(|buffer: &SampleBuffer<f32>| buffer.capacity() < required)
            {
                sample_buffer = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            }
            let buffer = sample_buffer.as_mut().ok_or(AudioDecodeError::EmptyAudio)?;
            buffer.copy_interleaved_ref(decoded);
            pipeline
                .as_mut()
                .ok_or(AudioDecodeError::EmptyAudio)?
                .push_interleaved(buffer.samples())?;
            if pipeline.as_ref().is_some_and(MonoPipeline::range_complete) {
                range_complete = true;
                break;
            }
        }

        // Declared packet duration catches premature EOF in containers that expose it.
        if !range_complete
            && (parameters
                .n_frames
                .is_some_and(|frames| packet_end < parameters.start_ts.saturating_add(frames))
                || decoder.finalize().verify_ok == Some(false))
        {
            return Err(AudioDecodeError::CorruptAudio);
        }
        // Conversion and decoder buffers never escape this call or reach a cache.
        drop(sample_buffer);
        drop(decoder);
        drop(format);
        let pipeline = pipeline.ok_or(AudioDecodeError::EmptyAudio)?;
        let duration = pipeline.duration_seconds();
        Ok((pipeline.finish()?, duration))
    }
}
