# AURORA

Offline desktop audio transcription application built with Tauri v2, Rust 2021,
React 19, TypeScript, Vite, and Tailwind CSS 4.

## Development

Install Node.js/npm, Rust, and the native prerequisites for Tauri on your OS.
From the repository root:

```sh
npm ci
npm run tauri -- dev
```

The root Cargo manifest is a workspace; the backend lives in `src-tauri` and the
frontend in `src`. The English-language UI uses a light pastel palette and supports native audio
selection or drag-and-drop, local validation, and a file card showing name,
format, size, and decoded duration. Import one WAV, MP3, FLAC, M4A, or OGG file
at a time. You can replace or remove a selection; cancelling the picker keeps
the previous selection. Directories and multiple-file drops are rejected.

Validation runs on a blocking worker and drops decoded samples immediately
after computing duration. IPC receives no filesystem paths from the WebView
and emits no sample buffers or full paths. No audio is uploaded or cached.
Transcription, playback, application model initialization, and export remain future milestones.
The reference icon supplies colors only; the bundled icon is unchanged.

`npm run dev` provides a browser layout preview; native import is available
through `npm run tauri -- dev`.

## Audio API

`aurora_lib::audio::AudioDecoder::decode_to_16khz_mono(&Path)` returns an owned
`Result<Vec<f32>, AudioDecodeError>`. It decodes the first supported audio track
in WAV, MP3, FLAC, M4A, or OGG/Vorbis, resamples each channel to 16 kHz, and then
averages channels into finite mono samples bounded to `[-1.0, 1.0]`.

The output length is the decoded frame count multiplied by the sample-rate ratio,
rounded to the nearest integer. FFT delay and flush padding are excluded. Codec
padding follows the container's gapless metadata; lossy files without usable
padding metadata can retain encoder padding.

Decoding is synchronous: Tauri import commands call it through
`tokio::task::spawn_blocking` after native file selection. Callers own the output
and must drop it after processing. Intermediate audio stays in scoped memory;
decoding neither contacts the network nor writes audio caches or sample logs.

Decoder errors, declared-duration truncation, failed integrity checks, changing
stream parameters, and non-finite samples return errors rather than partial
audio. Integrity checks depend on the codec and container metadata; corruption
that a codec cannot detect cannot be guaranteed to produce an error.

## VAD API

`aurora_lib::vad::SileroVad::new(&Path)` loads a local **Silero v5.1** ONNX
model once with CPU execution and one intra/inter-op thread.
`chunk_audio(&mut self, &[f32])` returns `Result<Vec<AudioChunk<'_>>, VadError>`.
Chunks borrow the original finite, bounded 16 kHz mono samples and contain
`start_sec` and `end_sec` as `f64` timestamps in the original file. No audio
copies are made for chunks. Empty input and silence return no chunks.

Inference follows the [official Silero v5.1 wrapper](https://github.com/snakers4/silero-vad/blob/v5.1/src/silero_vad/utils_vad.py):
512-sample frames, 64 context samples, and recurrent state shaped `[2, 1, 128]`.
The last frame is zero-padded for inference only. Context and recurrent state
are local to each call and dropped on return, including error paths. The
session retains no previous file's audio state. No downloads, audio logs,
temporary audio files, or frontend messages are introduced.

Speech starts at probability >= 0.50 and ends after at least 300 ms below
0.35; probabilities >= 0.50 cancel a pending pause. Events below 1.5 seconds
are discarded before padding. Valid events are grouped across pauses below
1 second toward 15–25 second chunks. The first qualifying pause after 15
seconds closes a chunk; a pause of at least 1 second closes earlier. Continuous
audio is split exactly at 25 seconds, preserving short tails of valid speech
and allowing those tails to group with subsequent speech. Leading/trailing
silence is omitted except for 30 ms margins. Margins are trimmed to the file
and 25 second cap; forced cuts have no margins or overlapping samples.
VAD boundaries follow the 512-sample frame grid; duration comparisons use
integer sample counts. Unconfirmed trailing silence remains part of speech at EOF.

Provide the v5.1 `silero_vad.onnx` model locally and set `ORT_DYLIB_PATH` to an
absolute path to an **ONNX Runtime 1.20.x** library before the first ORT call
(`libonnxruntime.dylib`, `onnxruntime.dll`, or `libonnxruntime.so`). There is
no bundled runtime or automatic resource installation. Missing model files,
incompatible tensor contracts, invalid samples, and invalid outputs return
typed errors. The pinned `ort` rc.9 dependency can panic internally if the
runtime library is absent, unloadable, or incompatible; resource provisioning
is a prerequisite rather than a recoverable runtime guarantee.

The application does not initialize or call VAD yet. Future Tauri integration
must create and retain the session once in application state and run model
loading and chunking on blocking workers. Callers own the decoded audio and
must drop it after processing all borrowed chunks.

## Verification

```sh
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run build
npm run format:check
```

Tests use synthetic audio only. Committed format fixtures allow tests to run
without FFmpeg or network access once dependencies are installed. See
`src-tauri/tests/fixtures/README.md` for regeneration instructions.

Deterministic VAD tests run without ONNX resources. The real-inference test is
explicitly ignored in the ordinary suite and requires local resources:

```sh
export AURORA_SILERO_MODEL=/absolute/path/to/silero_vad.onnx
export ORT_DYLIB_PATH=/absolute/path/to/libonnxruntime.dylib
cargo test --workspace --locked --test silero_vad -- --ignored
```

This test exercises model loading, real probability/state validation, synthetic
silence, partial frames, and independent repeated calls. It fails if explicitly
run without the required resources; it never downloads them. Real inference
has not been verified in this workspace because those resources are absent.

Rust dependencies preserve the versions specified in the project plan. ONNX
`ort` and `ort-sys` are pinned to `2.0.0-rc.9`, with dynamic loading and OS-specific
execution-provider features. The VAD library initializes ONNX only when explicitly
constructed by a backend caller; application startup and native import do not
initialize models. Lockfiles preserve the resolved Rust and frontend dependencies.

All function documentation and code comments are written in English.
