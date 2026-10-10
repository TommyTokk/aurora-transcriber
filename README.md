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
The ASR engine initializes once on a blocking startup worker when local resources are configured. Transcription controls, playback, VAD orchestration, and export remain future milestones.
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
absolute path to an **ONNX Runtime 1.20.1** library before the first ORT call
(`libonnxruntime.dylib`, `onnxruntime.dll`, or `libonnxruntime.so`). There is
no bundled runtime or automatic resource installation. Missing model files,
incompatible tensor contracts, invalid samples, and invalid outputs return
typed errors. A shared fallible preflight checks the trusted native library, version, and C API
before entering the pinned `ort` rc.9 loader. Missing, unloadable, and incompatible
runtimes return errors. The library is retained for the process lifetime and
conflicting runtime paths are rejected. Only trusted local native libraries are
supported: preflight cannot sandbox executable code in an arbitrary library.

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
run without the required resources; it never downloads them. Real inference was verified locally with the official Silero v5.1 checksum
`2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f`
and ONNX Runtime 1.20.1 on macOS arm64. The byte-identical Hugging Face mirror is
[Derur/silero-models](https://huggingface.co/Derur/silero-models/blob/main/vad/silero_vad.onnx).

Rust dependencies preserve the versions specified in the project plan. ONNX
`ort` and `ort-sys` are pinned to `2.0.0-rc.9`, with dynamic loading and OS-specific
execution-provider features. The VAD library initializes its session only when explicitly
constructed by a backend caller. ASR startup shares the same guarded runtime
environment; native import does not initialize or use inference sessions. Lockfiles preserve the resolved Rust and frontend dependencies.

All function documentation and code comments are written in English.

## Parakeet ASR API and configuration

`TranscriberEngine::new(&Path)` loads the local FP32 export of
[NVIDIA Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3),
converted by [istupakov](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx).
The supported revision is **8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce**.
SHA-256 hashes for all six resources are enforced in
`src-tauri/src/engine/resources.rs` with a bounded streaming buffer:

- `encoder-model.onnx` and `encoder-model.onnx.data`
- `decoder_joint-model.onnx`
- `nemo128.onnx`
- `config.json` and `vocab.txt`

This export uses an indexed SentencePiece vocabulary, not `tokenizer.json`.
The model is CC-BY-4.0; preserve attribution when distributing its weights.
Models and runtime are provisioned explicitly outside tracked files. The app
never downloads them. Before launching AURORA, set absolute backend paths:

```sh
export AURORA_ASR_MODEL_DIR=/absolute/path/to/parakeet-fp32
export ORT_DYLIB_PATH=/absolute/path/to/libonnxruntime.dylib
npm run tauri -- dev
```

Use a trusted official ONNX Runtime **1.20.1** package for the host OS and
architecture, with any required provider dependencies. Cargo provider features
do not guarantee that the native runtime package includes that provider.

Tauri owns one `EngineState`, initialized in setup with one `spawn_blocking`
worker. The argument-free `transcriber_state()` command returns
`{ phase, provider, error }`; phases are `loading`, `ready`, and `unavailable`.
Providers serialize as `Cpu`, `CoreMl`, `DirectMl`, or `Cuda`, where compiled.
Errors are safe English messages; no paths, tensors, or underlying ORT errors
are serialized. Failure leaves the native importer usable and is terminal until
restart. No retry, resource picker, or status UI is added in this milestone.

`transcribe_chunk(&mut self, &[f32]) -> Result<String, EngineError>` accepts
finite mono 16 kHz samples in `[-1, 1]`, at most 25 seconds. Empty input returns
empty text. Nonempty inference errors propagate instead of returning partial
text. The engine performs ONNX NeMo preprocessing, encoding, and bounded greedy
TDT decoding, including blank-state rejection, duration prediction, and a
10-token limit for zero-duration emissions per encoder step. Predictor state
resets for every chunk. Vocabulary spacing follows the upstream reference.

All platforms process chunks serially with batch 1. Preprocessing runs on CPU.
ASR session creation attempts CoreML → CPU on macOS, DirectML → CUDA → CPU on
Windows, and CUDA → CPU on Linux. The Windows sequence is the explicitly approved
exception to AGENTS.md's immediate CPU fallback rule. Partial session pairs are
dropped before fallback. DirectML uses sequential execution and disabled memory
patterns. A configured provider can delegate unsupported operators to CPU.

The caller must use a blocking worker for construction and inference. Future
Tauri commands can call `EngineState::with_engine` from their worker to borrow
the retained engine exclusively. Status has an independent mutex and remains
readable during inference. `new_cpu` provides an explicit CPU constructor for
reproducible tests. Transient inputs, features, predictor states, and token IDs
are scoped to one call and released on success or error; returned text belongs
to the caller. The safe rc.9 constructor copies borrowed input slices into a
bounded chunk tensor. No audio cache, transcript log, or processing network
request is introduced. Allocator arenas and memory-pattern reuse are disabled
for CPU transient buffers; this is scoped release, not guaranteed memory zeroing.

## ASR integration verification

Ordinary tests and doc tests require no model downloads. Explicit integration
runs require local resources. To generate independent expectations, install
`onnx-asr==0.12.0`, `onnxruntime==1.20.1`, and `soundfile` in a development-only
Python environment. Prepare explicitly authorized **mono 16 kHz WAV** samples
up to 25 seconds so the reference and Rust engine receive identical waveforms.
A local cases JSON initially contains objects such as:

```json
[
  { "audio_path": "/absolute/it.wav", "language": "it" },
  { "audio_path": "/absolute/en.wav", "language": "en" }
]
```

```sh
python scripts/asr_reference.py --model-dir "$AURORA_ASR_MODEL_DIR" \
  --cases /absolute/cases-input.json --output /absolute/cases-reference.json
export AURORA_ASR_TEST_CASES=/absolute/cases-reference.json
cargo test --workspace --locked --test parakeet -- --ignored --test-threads=1
cargo test --workspace --locked ready_engine_is_reused -- --ignored
```

The reference helper reads local files only and uses the selected export's exact
`nemo128.onnx` graph. Its output contains reference text and is an explicitly
requested test artifact. Do not use private recordings without authorization.
The tests check IT/EN text parity, reversed repeated calls on one engine, input
errors, platform-provider inference, isolated invalid-runtime processes, and
ready-state reuse. The isolated subprocesses prevent environment/global ORT
state from contaminating other tests.

Local verification used two CC-BY-4.0 public examples:
[MINDS-14 Italian](https://huggingface.co/datasets/PolyAI/minds14),
`it-IT~CASH_DEPOSIT/CAe448511ca16ebe60e67e3b2cd2d159f2_1.wav` (9.52 seconds),
and [FLEURS English](https://huggingface.co/datasets/google/fleurs),
`en_us/validation`, row 0, ID 1548 (6.54 seconds). The Italian source was decoded
and resampled offline with FFmpeg to mono Float32 16 kHz for identical-input
comparison. Reference generation, CPU parity, repeated calls, real Silero, and
platform fallback passed on macOS arm64. CoreML session initialization failed
and CPU fallback succeeded; actual CoreML acceleration is not validated.
Windows/DirectML and Linux/CUDA execution remain unverified on real hardware.

A headless probe can validate the release `panic = "abort"` configuration:

```sh
cargo run --release --locked --example engine_probe
```

It prints only readiness or safe failure diagnostics. Missing configuration,
a missing runtime, a non-library runtime file, and an incompatible-version test
library were checked in release and
returned `unavailable` without terminating the process. The macOS release bundle
also opened without resource configuration and successfully imported a WAV through
the native picker.

Native dialog selection, cancellation, replacement, and removal were verified
on macOS. Cross-application invalid drag-and-drop gestures have not been verified
through UI automation in this session.
