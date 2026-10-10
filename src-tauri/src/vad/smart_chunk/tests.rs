use super::*;

/// Feeds deterministic probability spans without requiring an ONNX runtime.
fn detect<'a>(spans: &[(usize, f32)], samples: &'a [f32]) -> Result<Vec<AudioChunk<'a>>, VadError> {
    let mut segmenter = Segmenter::default();
    let mut start = 0;
    for &(length, probability) in spans {
        for offset in (0..length).step_by(160) {
            let end = (offset + 160).min(length);
            segmenter.push(probability, start + offset..start + end)?;
        }
        start += length;
    }
    assert_eq!(start, samples.len());
    Ok(segmenter.finish(samples))
}

/// Confirms original-file timestamps, borrowed slices, ordering, and duration caps.
fn check_chunks(chunks: &[AudioChunk<'_>], samples: &[f32]) {
    let mut previous_end = 0;
    for chunk in chunks {
        let start = (chunk.start_sec * TARGET_SAMPLE_RATE as f64).round() as usize;
        let end = (chunk.end_sec * TARGET_SAMPLE_RATE as f64).round() as usize;
        assert!(start >= previous_end && end > start && end <= samples.len());
        assert_eq!(end - start, chunk.samples.len());
        assert!(chunk.samples.len() <= MAX_CHUNK);
        assert_eq!(chunk.samples.as_ptr(), samples[start..end].as_ptr());
        previous_end = end;
    }
}

/// Verifies empty audio, silence, and events below the exact minimum are omitted.
#[test]
fn empty_silence_and_short_events() -> Result<(), VadError> {
    assert!(detect(&[], &[])?.is_empty());
    for spans in [
        vec![(MIN_SPEECH, 0.0)],
        vec![(MIN_SPEECH - 1, 0.9), (MIN_SILENCE, 0.0)],
    ] {
        let samples = vec![0.0; spans.iter().map(|span| span.0).sum()];
        assert!(detect(&spans, &samples)?.is_empty());
    }
    let samples = vec![0.0; MIN_SPEECH];
    assert_eq!(detect(&[(MIN_SPEECH, 0.5)], &samples)?.len(), 1);
    Ok(())
}

/// Checks enter/exit threshold equality, uncertain speech, and pause cancellation.
#[test]
fn hysteresis_and_unconfirmed_pause() -> Result<(), VadError> {
    let spans = [
        (1600, 0.49),
        (MIN_SPEECH, 0.5),
        (1600, 0.35),
        (MIN_SILENCE - 1, 0.34),
        (1600, 0.5),
    ];
    let samples = vec![0.0; spans.iter().map(|span| span.0).sum()];
    let chunks = detect(&spans, &samples)?;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].samples.len(), samples.len() - 1600 + MARGIN);
    check_chunks(&chunks, &samples);
    Ok(())
}

/// Confirms a 300 ms pause ends an event while a shorter pause stays inside it.
#[test]
fn exact_pause_boundary() -> Result<(), VadError> {
    for silence in [MIN_SILENCE - 1, MIN_SILENCE] {
        let samples = vec![0.0; MIN_SPEECH + silence];
        let chunks = detect(&[(MIN_SPEECH, 0.9), (silence, 0.0)], &samples)?;
        let expected = if silence < MIN_SILENCE {
            samples.len()
        } else {
            MIN_SPEECH + MARGIN
        };
        assert_eq!(chunks[0].samples.len(), expected);
        check_chunks(&chunks, &samples);
    }
    Ok(())
}

/// Groups short pauses but closes at one second even below the target duration.
#[test]
fn long_pause_boundary() -> Result<(), VadError> {
    for gap in [MIN_SILENCE, LONG_SILENCE - 1, LONG_SILENCE] {
        let spans = [(MIN_SPEECH, 0.9), (gap, 0.0), (MIN_SPEECH, 0.9)];
        let samples = vec![0.0; MIN_SPEECH * 2 + gap];
        let chunks = detect(&spans, &samples)?;
        assert_eq!(chunks.len(), if gap < LONG_SILENCE { 1 } else { 2 });
        check_chunks(&chunks, &samples);
    }
    Ok(())
}

/// Closes at the first qualifying pause once the 15 second target is reached.
#[test]
fn target_pause_boundary() -> Result<(), VadError> {
    for speech in [TARGET_CHUNK - 1, TARGET_CHUNK] {
        let spans = [(speech, 0.9), (MIN_SILENCE, 0.0), (MIN_SPEECH, 0.9)];
        let samples = vec![0.0; speech + MIN_SILENCE + MIN_SPEECH];
        let chunks = detect(&spans, &samples)?;
        assert_eq!(chunks.len(), if speech < TARGET_CHUNK { 1 } else { 2 });
        check_chunks(&chunks, &samples);
    }
    Ok(())
}

/// Cuts continuous speech exactly at 25 seconds and preserves short valid tails.
#[test]
fn forced_cuts_preserve_audio_and_tail() -> Result<(), VadError> {
    for length in [MAX_CHUNK, MAX_CHUNK + 1, MAX_CHUNK * 2 + 513] {
        let samples = vec![0.0; length];
        let chunks = detect(&[(length, 0.9)], &samples)?;
        assert_eq!(chunks.len(), length.div_ceil(MAX_CHUNK));
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.samples.len())
                .sum::<usize>(),
            length
        );
        assert_eq!(chunks[0].samples.len(), MAX_CHUNK);
        check_chunks(&chunks, &samples);
    }
    Ok(())
}

/// Pads natural boundaries while keeping forced cuts adjacent and bounded.
#[test]
fn padding_and_forced_cut_boundaries() -> Result<(), VadError> {
    let length = MAX_CHUNK + 1000;
    let spans = [(1600, 0.0), (length, 0.9), (LONG_SILENCE, 0.0)];
    let samples = vec![0.0; 1600 + length + LONG_SILENCE];
    let chunks = detect(&spans, &samples)?;
    assert_eq!(chunks.len(), 2);
    // A full-length chunk has no spare capacity for an outer margin.
    assert_eq!(chunks[0].start_sec, 0.1);
    assert_eq!(chunks[0].end_sec, chunks[1].start_sec);
    assert_eq!(chunks[1].samples.len(), 1000 + MARGIN);
    check_chunks(&chunks, &samples);
    let spans = [(1600, 0.0), (MIN_SPEECH, 0.9), (LONG_SILENCE, 0.0)];
    let samples = vec![0.0; 1600 + MIN_SPEECH + LONG_SILENCE];
    let chunks = detect(&spans, &samples)?;
    assert_eq!(chunks[0].samples.len(), MIN_SPEECH + MARGIN * 2);
    check_chunks(&chunks, &samples);
    Ok(())
}

/// Groups new speech with the short tail remaining after a forced 25 second cut.
#[test]
fn groups_tail_after_forced_cut() -> Result<(), VadError> {
    let spans = [
        (TARGET_SAMPLE_RATE * 30, 0.9),
        (MIN_SILENCE, 0.0),
        (TARGET_SAMPLE_RATE * 2, 0.9),
    ];
    let samples = vec![0.0; spans.iter().map(|span| span.0).sum()];
    let chunks = detect(&spans, &samples)?;
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].samples.len(), MAX_CHUNK);
    assert_eq!(chunks[1].samples.len(), samples.len() - MAX_CHUNK);
    check_chunks(&chunks, &samples);
    Ok(())
}

/// Rejects malformed model probabilities rather than producing partial chunks.
#[test]
fn invalid_probabilities() {
    for probability in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        assert!(matches!(
            Segmenter::default().push(probability, 0..512),
            Err(VadError::InvalidOutput)
        ));
    }
}

/// Clamps final-frame timestamps to real samples with native 512-sample frames.
#[test]
fn partial_final_frame() -> Result<(), VadError> {
    let samples = vec![0.0; MIN_SPEECH + 17];
    let mut segmenter = Segmenter::default();
    for start in (0..samples.len()).step_by(512) {
        segmenter.push(0.9, start..(start + 512).min(samples.len()))?;
    }
    let chunks = segmenter.finish(&samples);
    assert_eq!(chunks[0].samples.len(), samples.len());
    check_chunks(&chunks, &samples);
    Ok(())
}
