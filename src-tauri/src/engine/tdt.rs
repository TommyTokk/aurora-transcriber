//! Bounded greedy TDT decoding, independent of ONNX for deterministic tests.
use super::EngineError;

/// Per-call decoder output; recurrent state is accepted only for nonblank tokens.
pub(super) struct Step<S> {
    pub token: usize,
    pub duration: usize,
    pub state: S,
}

/// Applies the reference TDT blank/state/zero-duration rules with bounded work.
pub(super) fn decode<S>(
    frames: usize,
    blank: usize,
    initial: S,
    mut step: impl FnMut(usize, usize, &S) -> Result<Step<S>, EngineError>,
) -> Result<Vec<usize>, EngineError> {
    let mut state = initial;
    let mut tokens = Vec::new();
    let mut time = 0;
    let mut emitted = 0;
    let mut previous = blank;
    while time < frames {
        let next = step(time, previous, &state)?;
        if next.token != blank {
            state = next.state;
            previous = next.token;
            tokens.push(next.token);
            emitted += 1;
        }
        // TDT predicts frame advances separately from vocabulary classes. Blank
        // never commits predictor state; zero-duration emissions are capped at 10.
        if next.duration > 0 {
            time = time.saturating_add(next.duration);
            emitted = 0;
        } else if next.token == blank || emitted == 10 {
            time += 1;
            emitted = 0;
        }
    }
    Ok(tokens)
}

/// Returns the first maximum index and rejects empty or non-finite model logits.
pub(super) fn argmax(values: &[f32]) -> Result<usize, EngineError> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err(EngineError::InvalidOutput);
    }
    let mut best = 0;
    for index in 1..values.len() {
        if values[index] > values[best] {
            best = index;
        }
    }
    Ok(best)
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Confirms blank state rejection, duration skipping, and predictor retention.
    #[test]
    fn blank_and_duration_state_rules() -> anyhow::Result<()> {
        let mut calls = Vec::new();
        let tokens = decode(4, 2, 0, |time, token, state| {
            calls.push((time, token, *state));
            Ok(match calls.len() {
                1 => Step {
                    token: 0,
                    duration: 0,
                    state: 7,
                },
                2 => Step {
                    token: 2,
                    duration: 2,
                    state: 99,
                },
                _ => Step {
                    token: 1,
                    duration: 2,
                    state: 8,
                },
            })
        })?;
        assert_eq!(tokens, [0, 1]);
        assert_eq!(calls, [(0, 2, 0), (0, 0, 7), (2, 0, 7)]);
        Ok(())
    }
    /// Bounds zero-duration outputs and initializes state independently per call.
    #[test]
    fn zero_duration_is_bounded_and_state_resets() -> anyhow::Result<()> {
        for _ in 0..2 {
            let mut calls = 0;
            let tokens = decode(2, 1, 0, |_, _, state| {
                assert_eq!(*state, calls);
                calls += 1;
                Ok(Step {
                    token: 0,
                    duration: 0,
                    state: calls,
                })
            })?;
            assert_eq!(tokens.len(), 20);
        }
        assert!(decode(1, 1, (), |_, _, _| Err(EngineError::InvalidOutput)).is_err());
        assert_eq!(argmax(&[2.0, 2.0, 1.0])?, 0);
        for values in [&[][..], &[f32::NAN][..], &[f32::INFINITY][..]] {
            assert!(argmax(values).is_err());
        }
        Ok(())
    }
}
