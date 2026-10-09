use std::{
    fs,
    path::{Path, PathBuf},
};

use aurora_lib::audio::{AudioDecodeError, AudioDecoder};
use hound::{SampleFormat, WavSpec, WavWriter};
use tempfile::TempDir;

/// Returns the repository path of a committed synthetic audio fixture.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Writes a synthetic floating-point WAV to `path` with the requested rate and gains.
/// Propagates WAV creation, sample writing, and finalization errors.
fn write_wave(path: &Path, rate: u32, frames: usize, gains: &[f32]) -> anyhow::Result<()> {
    let spec = WavSpec {
        channels: gains.len() as u16,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for index in 0..frames {
        let sample = (std::f32::consts::TAU * 440.0 * index as f32 / rate as f32).sin();
        for gain in gains {
            writer.write_sample(sample * gain)?;
        }
    }
    writer.finalize()?;
    Ok(())
}

/// Verifies decoding and resampling through the public API across rates and layouts.
#[test]
fn wav_rates_and_channels() -> anyhow::Result<()> {
    let temporary = TempDir::new()?;
    let path = temporary.path().join("synthetic.wav");
    for rate in [16_000, 44_100, 48_000] {
        for gains in [&[0.6][..], &[0.6, 0.2][..], &[0.6, 0.2, -0.1, 0.1][..]] {
            let frames = rate as usize / 2 + 17;
            write_wave(&path, rate, frames, gains)?;
            let samples = AudioDecoder::decode_to_16khz_mono(&path)?;
            assert_eq!(
                samples.len(),
                (frames * 16_000 + rate as usize / 2) / rate as usize
            );
            let gain = gains.iter().sum::<f32>() / gains.len() as f32;
            let rms = (samples[128..samples.len() - 128]
                .iter()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / (samples.len() - 256) as f32)
                .sqrt();
            assert!((rms - gain.abs() / 2.0_f32.sqrt()).abs() < 0.005);
        }
    }
    Ok(())
}

/// Reads all required containers using local, non-sensitive committed fixtures.
#[test]
fn supported_formats_decode_offline() -> anyhow::Result<()> {
    for name in ["tone.wav", "tone.mp3", "tone.flac", "tone.m4a", "tone.ogg"] {
        let samples = AudioDecoder::decode_to_16khz_mono(&fixture(name))?;
        // Lossy codecs may retain some container padding, but the short tone must survive.
        assert!(
            (3900..=4600).contains(&samples.len()),
            "{name}: {} samples",
            samples.len()
        );
        assert!(samples
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 1.0));
        let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>()
            / samples.len() as f32)
            .sqrt();
        assert!(rms > 0.02 && rms < 0.15, "{name}: rms={rms}");
    }
    Ok(())
}

/// Confirms content probing works even when an audio file has no extension.
#[test]
fn probes_content_without_extension() -> anyhow::Result<()> {
    let temporary = TempDir::new()?;
    let path = temporary.path().join("audio");
    fs::copy(fixture("tone.flac"), &path)?;
    assert_eq!(AudioDecoder::decode_to_16khz_mono(&path)?.len(), 4000);
    Ok(())
}

/// Checks missing files, empty files, header-only WAVs, and non-audio input.
#[test]
fn missing_empty_and_unsupported_files_fail() -> anyhow::Result<()> {
    let temporary = TempDir::new()?;
    let path = temporary.path().join("input.wav");
    assert!(matches!(
        AudioDecoder::decode_to_16khz_mono(&path),
        Err(AudioDecodeError::Io(_))
    ));
    fs::write(&path, [])?;
    assert!(matches!(
        AudioDecoder::decode_to_16khz_mono(&path),
        Err(AudioDecodeError::EmptyAudio)
    ));
    write_wave(&path, 16_000, 0, &[1.0])?;
    assert!(matches!(
        AudioDecoder::decode_to_16khz_mono(&path),
        Err(AudioDecodeError::EmptyAudio)
    ));
    fs::write(&path, b"This is not an audio container.")?;
    assert!(AudioDecoder::decode_to_16khz_mono(&path).is_err());
    Ok(())
}

/// Ensures truncation cannot silently return a partial WAV transcription input.
#[test]
fn truncated_wave_fails() -> anyhow::Result<()> {
    let temporary = TempDir::new()?;
    let path = temporary.path().join("truncated.wav");
    write_wave(&path, 48_000, 24_000, &[0.5])?;
    let file = fs::OpenOptions::new().write(true).open(&path)?;
    file.set_len(file.metadata()?.len() / 2)?;
    assert!(AudioDecoder::decode_to_16khz_mono(&path).is_err());
    Ok(())
}

/// Ensures decoder integrity verification rejects a damaged FLAC frame.
#[test]
fn corrupt_flac_fails() -> anyhow::Result<()> {
    let temporary = TempDir::new()?;
    let path = temporary.path().join("corrupt.flac");
    let mut bytes = fs::read(fixture("tone.flac"))?;
    let index = bytes.len() - 20;
    bytes[index] ^= 0xff;
    fs::write(&path, bytes)?;
    assert!(AudioDecoder::decode_to_16khz_mono(&path).is_err());
    Ok(())
}

/// Rejects a chained OGG file that switches sample rate during decoding.
#[test]
fn changing_sample_rate_fails() {
    assert!(matches!(
        AudioDecoder::decode_to_16khz_mono(&fixture("changing-rate.ogg")),
        Err(AudioDecodeError::ChangedParameters)
    ));
}

/// Rejects non-finite floating-point samples decoded from a valid WAV container.
#[test]
fn non_finite_wave_fails() -> anyhow::Result<()> {
    let temporary = TempDir::new()?;
    let path = temporary.path().join("non-finite.wav");
    let mut writer = WavWriter::create(
        &path,
        WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    )?;
    writer.write_sample(f32::NAN)?;
    writer.finalize()?;
    assert!(matches!(
        AudioDecoder::decode_to_16khz_mono(&path),
        Err(AudioDecodeError::NonFiniteSample)
    ));
    Ok(())
}
