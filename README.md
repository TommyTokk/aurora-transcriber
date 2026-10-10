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
Transcription, playback, model loading, and export remain future milestones.
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

Rust dependencies preserve the versions specified in the project plan. ONNX
`ort` and `ort-sys` are pinned to `2.0.0-rc.9`, with dynamic loading and OS-specific
execution-provider features. No ONNX runtime or models are initialized in this
milestone. Lockfiles preserve the resolved Rust and frontend dependencies.

All function documentation and code comments are written in English.
