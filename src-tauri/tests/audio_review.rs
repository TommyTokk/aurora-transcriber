//! Review extraction must match the original transcription clock and filter delay.
use aurora_lib::audio::{AudioDecodeError, AudioDecoder};
use std::path::Path;

#[test]
fn ranges_match_full_decode_in_every_supported_format() -> anyhow::Result<()> {
    for extension in ["wav", "mp3", "flac", "m4a", "ogg"] {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/tone.{extension}"));
        let full = AudioDecoder::decode_to_16khz_mono(&path)?;
        for range in [0..100, 777..3000, full.len() - 137..full.len()] {
            let clip = AudioDecoder::decode_range(&path, range.clone(), || false)?;
            assert_eq!(clip, full[range], "format {extension}");
        }
        assert!(matches!(
            AudioDecoder::decode_range(&path, 0..100, || true),
            Err(AudioDecodeError::Cancelled)
        ));
    }
    Ok(())
}

#[test]
fn ranges_preserve_resampler_delay_and_partial_tail() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("synthetic.wav");
    for rate in [16000, 44100, 48000] {
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for i in 0..rate + 17 {
            let sample = (i as f32 * 0.017).sin() * 0.5;
            writer.write_sample(sample)?;
            writer.write_sample(sample * 0.25)?;
        }
        writer.finalize()?;
        let full = AudioDecoder::decode_to_16khz_mono(&path)?;
        for range in [0..1, 23..2345, full.len() - 100..full.len()] {
            assert_eq!(
                AudioDecoder::decode_range(&path, range.clone(), || false)?,
                full[range],
                "rate {rate}"
            );
        }
        assert!(
            AudioDecoder::decode_range(&path, full.len()..full.len() + 10, || false)?.is_empty()
        );
    }
    Ok(())
}

#[test]
fn synthetic_fixtures_have_matching_duration_and_review_audio() -> anyhow::Result<()> {
    for name in ["tone.wav", "tone.m4a"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let full = AudioDecoder::decode_to_16khz_mono(&path)?;
        assert!(full.len() >= 3000, "fixture must cover the review ranges");
        let duration = AudioDecoder::duration_seconds(&path)?;
        assert_eq!(duration, full.len() as f64 / 16000.0);
        for range in [0..1000, 1500..3000, full.len() - 100..full.len()] {
            assert_eq!(
                AudioDecoder::decode_range(&path, range.clone(), || false)?,
                full[range]
            );
        }
    }
    Ok(())
}
