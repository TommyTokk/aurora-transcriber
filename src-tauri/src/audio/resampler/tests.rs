use super::*;

/// Builds interleaved sine samples with independent gains for every channel.
fn sine(sample_rate: usize, frames: usize, gains: &[f32]) -> Vec<f32> {
    (0..frames)
        .flat_map(|index| {
            let value = (std::f32::consts::TAU * 440.0 * index as f32 / sample_rate as f32).sin();
            gains.iter().map(move |gain| value * gain)
        })
        .collect()
}

/// Converts samples using deliberately irregular packet boundaries.
fn convert(
    sample_rate: usize,
    channels: usize,
    input: &[f32],
) -> Result<Vec<f32>, AudioDecodeError> {
    let mut pipeline = MonoPipeline::new(sample_rate, channels)?;
    for packet in input.chunks(137 * channels) {
        pipeline.push_interleaved(packet)?;
    }
    pipeline.finish()
}

/// Checks duration, channel averaging, filter alignment, and amplitude at three rates.
#[test]
fn rates_and_channel_counts_preserve_signal() -> Result<(), AudioDecodeError> {
    for rate in [16_000, 44_100, 48_000] {
        for gains in [&[0.6][..], &[0.6, 0.2][..], &[0.6, 0.2, -0.1, 0.1][..]] {
            let frames = rate / 3 + 17;
            let input = sine(rate, frames, gains);
            let result = convert(rate, gains.len(), &input)?;
            assert_eq!(
                result.len(),
                (frames * TARGET_SAMPLE_RATE + rate / 2) / rate
            );
            let gain = gains.iter().sum::<f32>() / gains.len() as f32;
            let mse = result
                .iter()
                .enumerate()
                .skip(128)
                .take(result.len() - 256)
                .map(|(index, sample)| {
                    let expected = gain
                        * (std::f32::consts::TAU * 440.0 * index as f32
                            / TARGET_SAMPLE_RATE as f32)
                            .sin();
                    (sample - expected).powi(2)
                })
                .sum::<f32>()
                / (result.len() - 256) as f32;
            assert!(
                mse.sqrt() < 0.003,
                "rate={rate}, channels={}, rms={}",
                gains.len(),
                mse.sqrt()
            );
            assert!(result
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 1.0));
        }
    }
    Ok(())
}

/// Checks rounded length for tiny clips and incomplete or exactly full FFT blocks.
#[test]
fn short_and_partial_blocks_have_exact_duration() -> Result<(), AudioDecodeError> {
    for rate in [16_000, 44_100, 48_000] {
        for frames in [1, 3, 19, 1023, 1024, 1025, 1026, 1763, 1764, 1765, 2048] {
            let result = convert(rate, 1, &vec![0.25; frames])?;
            assert_eq!(
                result.len(),
                (frames * TARGET_SAMPLE_RATE + rate / 2) / rate
            );
        }
    }
    Ok(())
}

/// Ensures flushing retains a signal present only in the final portion of a clip.
#[test]
fn delayed_tail_is_not_lost() -> Result<(), AudioDecodeError> {
    for rate in [44_100, 48_000] {
        let mut input = vec![0.0; 3001];
        input[2700..].fill(0.5);
        let result = convert(rate, 1, &input)?;
        assert!(result[result.len() - 40..]
            .iter()
            .all(|sample| *sample > 0.35));
        assert!(result[..100].iter().all(|sample| sample.abs() < 0.001));
    }
    Ok(())
}

/// Confirms unity clamping without normalizing quiet samples or changing silence.
#[test]
fn compatible_audio_is_averaged_and_clamped() -> Result<(), AudioDecodeError> {
    let result = convert(16_000, 2, &[2.0, 2.0, -2.0, -2.0, 0.25, 0.75, 0.0, 0.0])?;
    assert_eq!(result, [1.0, -1.0, 0.5, 0.0]);
    Ok(())
}

/// Rejects empty audio, invalid geometry, incomplete frames, and non-finite samples.
#[test]
fn invalid_input_is_rejected() -> Result<(), AudioDecodeError> {
    assert!(matches!(
        MonoPipeline::new(0, 1),
        Err(AudioDecodeError::InvalidParameters(_))
    ));
    assert!(matches!(
        MonoPipeline::new(16_000, 0),
        Err(AudioDecodeError::InvalidParameters(_))
    ));
    assert!(matches!(
        convert(16_000, 1, &[]),
        Err(AudioDecodeError::EmptyAudio)
    ));
    let mut pipeline = MonoPipeline::new(48_000, 2)?;
    assert!(matches!(
        pipeline.push_interleaved(&[0.0]),
        Err(AudioDecodeError::InvalidParameters(_))
    ));
    for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(matches!(
            pipeline.push_interleaved(&[sample, 0.0]),
            Err(AudioDecodeError::NonFiniteSample)
        ));
    }
    Ok(())
}

/// Verifies that downsampling removes content above the target Nyquist frequency.
#[test]
fn downsampling_filters_high_frequencies() -> Result<(), AudioDecodeError> {
    let input: Vec<_> = (0..48_000)
        .map(|index| 0.5 * (std::f32::consts::TAU * 12_000.0 * index as f32 / 48_000.0).sin())
        .collect();
    let result = convert(48_000, 1, &input)?;
    let rms = (result[256..result.len() - 256]
        .iter()
        .map(|sample| sample * sample)
        .sum::<f32>()
        / (result.len() - 512) as f32)
        .sqrt();
    assert!(rms < 0.01, "high-frequency RMS={rms}");
    Ok(())
}
