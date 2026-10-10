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
and emits no sample buffers or audio paths. Model settings expose the selected
folder solely for display. No audio is uploaded or cached.
The workspace includes transcription controls, real progress, a segment editor,
TXT/SRT/VTT export, and an explicitly saved local transcript archive. Use **Download
models** to provision the supported ASR and VAD files. All transcription runs offline.
Both sessions are reused while resident and released after more than five idle minutes.
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

Standalone tools and low-level tests provide the v5.1 model and an absolute
`ORT_DYLIB_PATH` to trusted ONNX Runtime 1.20.1 before the first ORT call.
The macOS desktop app includes the official arm64 runtime and ignores this variable. The desktop app provisions model resources only through an
explicit Download models action. Missing model files,
incompatible tensor contracts, invalid samples, and invalid outputs return
typed errors. A shared fallible preflight checks the trusted native library, version, and C API
before entering the pinned `ort` rc.9 loader. Missing, unloadable, and incompatible
runtimes return errors. The library is retained for the process lifetime and
conflicting runtime paths are rejected. Only trusted local native libraries are
supported: preflight cannot sandbox executable code in an arbitrary library.

The application retains VAD alongside ASR in Tauri state, loading and running both
on blocking workers. Their sessions are released together after inactivity and
reloaded from disk when the next transcription starts. Decoded audio is scoped
to a single job and dropped after its borrowed chunks are processed.

## Verification

```sh
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm test
npm run build
npm run format:check
```

Ordinary tests use synthetic audio and fixture documents only. Explicit integration
tests may use user-authorized local audio; they never print transcript contents. Committed format fixtures allow tests to run
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
The desktop app downloads model files explicitly from Hugging Face into its
per-user managed directory. The macOS arm64 desktop includes its runtime;
`npm run tauri -- dev` and Finder launch need no runtime environment variables.

`AURORA_ASR_MODEL_DIR` and `AURORA_SILERO_MODEL` are still used by explicitly
run low-level tests and probes, but the desktop app resolves models itself.

Standalone tools on other platforms use a trusted official ONNX Runtime
**1.20.1** package for their host OS and architecture, with required provider dependencies. Cargo provider features
do not guarantee that the native runtime package includes that provider.

Tauri owns one `EngineState` for ASR and Silero VAD. The argument-free
`transcriber_state()` command and `transcriber-state` event expose path-free
`{ phase, provider, error, missing, downloadFile, percentage }` snapshots. Phases
are `loading`, `missing`, `ready`, `downloading`, `unloaded`, and `unavailable`.
Providers serialize as `Cpu`, `CoreMl`, `DirectMl`, or `Cuda`, where compiled.
Errors are safe English messages. Missing models leave native import available;
the explicit download action repairs absent or corrupt resources and initializes
sessions without restarting. Runtime setup failures require fixing the trusted
runtime configuration and restarting the app.

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

The caller must use a blocking worker for construction and inference. Tauri orchestration pins both sessions throughout one job. Existing blocking
callers can borrow the resident ASR using `EngineState::with_engine`. Status has an independent mutex and remains
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
# Desktop pipeline integration is documented below.
```

The reference helper reads local files only and uses the selected export's exact
`nemo128.onnx` graph. Its output contains reference text and is an explicitly
requested test artifact. Do not use private recordings without authorization.
The tests check IT/EN text parity, reversed repeated calls on one engine, input
errors, platform-provider inference, isolated invalid-runtime processes, and
ready-session reuse. The isolated subprocesses prevent environment/global ORT
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

## Desktop storage and workflow

Tauri resolves the current user's application data directory. On macOS it is
`~/Library/Application Support/com.aurora.desktop/`:

```text
com.aurora.desktop/
├── model-storage.json   # Version 1; null directory selects default
├── models/
│   ├── parakeet/         # Six verified FP32 model resources
│   └── silero_vad.onnx   # Verified Silero v5.1 model
└── transcripts/
    └── <uuid>.json       # Explicitly saved, versioned documents
```

The **Models** section displays the current destination and resource availability.
**Choose folder** opens a backend native directory picker and uses that exact folder;
`parakeet/`, `silero_vad.onnx`, and temporary `.download-cache/` stay inside it.
**Use default** selects `models/` in the application data directory. **Open folder**
uses the backend opener plugin; the WebView has no permission to open arbitrary paths.
**Download models** is the explicit network action.

The first launch creates the default automatically on a worker. Configuration is
versioned JSON published through a synced same-directory temporary file and atomic
rename. The chosen directory must exist, be readable, and accept a temporary write
probe before activation. Cancelling the picker leaves the choice and sessions intact.
Changing folders never transfers or deletes previous files. Loading, downloading,
transcription, and the picker share one native reservation. After the choice commits,
the worker drops the old sessions and verifies/loads available models offline. Missing
resources require an explicit download. An unavailable custom destination stays selected
and reports an error with **Choose folder** and **Use default** available for recovery.
Settings recheck accessibility on window focus and every 15 seconds while open.

`model_storage_state()`, `choose_model_directory()`, `reset_model_directory()`, and
`open_model_directory()` accept no path arguments. `model-storage-state` reports
`path`, `isDefault`, `accessible`, safe missing filenames, and `error`;
`model-storage-error` reports a failed asynchronous picker activation. Paths are
display-only and never sent back to Rust as filesystem authority.

Downloads use `hf-hub` against Hugging Face only, preserving the pinned Parakeet
revision and Silero v5.1 SHA-256. Files are verified and atomically promoted;
incomplete or corrupt transfers cannot become usable models. Valid local files
are reused. An internal `.download-cache/` supports retries and is removed after
successful provisioning. Allow temporary disk space for downloaded and staged
copies of large model files. Progress counts completed files plus the current
file's byte fraction; it is not an estimate of remaining time.

After native import, **Transcribe audio** decodes, detects speech, and runs serial
ASR on blocking workers. Progress identifies loading, decoding, VAD, and completed
ASR chunks. **Cancel transcription** takes effect between phases/chunks; it waits
for an already-running decoder, VAD pass, or ONNX call and discards partial results.
Import, replacement, and removal are locked until the worker finishes. Files with
no detected speech complete with an empty result.

Each editor segment uses the original VAD chunk's immutable timestamps, up to
25 seconds. These are chunk boundaries, not word-level subtitle alignment.
**Save transcript** explicitly persists current text, timestamps, display name,
UUID, schema version, and UTC Unix dates. Subsequent saves update the same document
atomically. No source audio, source paths, or automatic saves are included. The
archive reopens valid documents and skips corrupt files. Unsaved edits are guarded
before replacing the result. Saved documents and deliberate exports contain
unencrypted text in the user's local directory; no private audio is cached.

**Export TXT/SRT/VTT** opens a native save dialog initially in `transcripts/`.
Exports reflect current edits, omit empty segments, and number nonempty cues
consecutively. Cancelling the dialog writes nothing. Subtitle text is escaped as
plain text, and finite, nonoverlapping timestamps are validated before export.

The idle timer uses a monotonic clock from the end of model loading or the last
job, including failed/cancelled jobs. A one-second monitor releases both sessions
strictly after 300 seconds with no reserved job. Their model files remain on disk.
The next job reloads them offline. Releasing sessions does not guarantee an
immediate equivalent drop in process RSS: allocator/native runtime bookkeeping
can remain resident. ONNX Runtime itself stays loaded for the process lifetime.

## Milestone 6 verification and macOS packaging

Frontend event-order tests use Node.js 22.6+ with type stripping (`npm test`).
Rust tests cover atomic model promotion with simulated transfers, exclusive
operations, controlled idle deadlines, loader reuse/reload, native selection IDs,
cancellation cleanup, archive corruption, and subtitle formatting.

Run the authorized sample integration explicitly with existing local resources:

```sh
export AURORA_ASR_MODEL_DIR=/absolute/path/to/parakeet-fp32
export AURORA_SILERO_MODEL=/absolute/path/to/silero-v5.1.onnx
export ORT_DYLIB_PATH=/absolute/path/to/libonnxruntime.dylib
cargo test --workspace --locked local_audio_sample_pipeline_and_archive -- --ignored --test-threads=1
```

It uses `audio_samples/audio.m4a`, provisions isolated temporary model directories
with local hard links, runs real inference, edits/saves/reopens a document, checks
all export formats, and forces an idle deadline to verify real release and offline
reload. It neither downloads models nor logs the sample's text. The private
`audio_samples/` folder is ignored by Git. A separate explicitly invoked network
test checks the small Hugging Face VAD download while reusing local ASR files:

```sh
cargo test --workspace --locked explicit_hugging_face_download -- --ignored --test-threads=1
```

Build unsigned local macOS artifacts with:

```sh
npm run tauri -- build --bundles app,dmg
```

The outputs are `target/release/bundle/macos/AURORA.app` and
`target/release/bundle/dmg/AURORA_0.1.0_aarch64.dmg` on this Apple Silicon host.
Open `AURORA.app` from Finder. No environment variables, model paths, or symlinks
are required. The macOS arm64 bundle includes official ONNX Runtime 1.20.1, its
MIT license, and `ThirdPartyNotices.txt`. `resources/onnxruntime/manifest.json`
records the official HTTPS source and SHA-256 hashes of the archive and extracted
resources. Microsoft did not publish a digest for this release asset; the archive
hash records the verified HTTPS download, rather than an independently signed checksum.
The build checks every resource against the manifest, and the worker verifies the
library hash and ABI before initializing ORT. Its dependencies are system libraries
and Apple frameworks only; it has no dependency on `/private/tmp`.

Signing, notarization, and Windows/Linux packaging remain outside this intervention.

Verified on this macOS arm64 host: the ordinary Rust suite (52 tests plus one
doc test), two frontend event-order tests, Clippy, formatting, frontend production
build, explicit Hugging Face Silero provisioning, and the real authorized sample
pipeline with forced model release/reload. The installed app also completed native
sample import/transcription, three-segment editing, the unsaved-change warning,
explicit save and archive reopening, SRT export, and cancelled-export handling.
Native export checks used replacement test text instead of persisting the sample's
original transcription. The live five-minute monitor released sessions, a subsequent
job reloaded them offline, and native cancellation discarded its partial result. The runtime-enabled installed app reported `CoreMl`;
this does not establish how much inference was delegated to the Apple Neural Engine.
Unsigned `.app` and `.dmg` packaging succeeded with system packaging permissions.

## Model storage intervention verification (2026-10-10)

Verified on the available Apple Silicon Mac:

- 56 ordinary Rust tests and one doc test; two frontend tests; Clippy with warnings denied; Rust/Prettier formatting; production frontend build.
- Native picker cancellation, exact empty-folder selection, choice persistence, unavailable custom-folder recovery through Use default, and backend Open folder. Unit tests also cover nonwritable candidates, atomic configuration, unsupported versions, active-operation rejection, and unchanged previous-folder files.
- A complete explicitly authorized Hugging Face download into an empty isolated temporary profile, followed by real `audio_samples/audio.m4a` transcription, forced strictly-over-300-second session release, and offline reload. The temporary profile was automatically removed. No transcript contents were logged.
- A separate offline real-model run recognized valid models in a second destination without downloads, reset to default, verified both destinations remained intact, and exercised edited save/reopen and TXT/SRT/VTT formatting.
- Final `.app` and `.dmg` generation, matching installed/build executable hashes, all bundled runtime/license/notice hashes, and `hdiutil verify` for the final DMG. Finder launch recognized existing verified models before cleanup. After cleanup, the installed app opened with default storage, all seven resources missing, enabled folder/download actions, and the existing transcript archive preserved.

The one-time cleanup ran only after package and inference verification, with AURORA closed and the default explicitly selected. It removed this exact list (absent entries were skipped):

```text
~/Library/Application Support/com.aurora.desktop/models/parakeet/
~/Library/Application Support/com.aurora.desktop/models/silero_vad.onnx
~/Library/Application Support/com.aurora.desktop/models/.download-cache/
/private/tmp/aurora-m5/
/private/tmp/aurora-m6-native.srt
/private/tmp/aurora-m6-provision.py
/private/tmp/aurora-parakeet-metadata.json
/private/tmp/aurora-preprocessor.py
/private/tmp/aurora-preprocessors.py
/private/tmp/aurora-runtime-release.json
/private/tmp/aurora-silero-metadata.json
/private/tmp/aurora-runtime-1.20.1-release.json
/private/tmp/aurora-runtime-1.20.1.tgz
/private/tmp/aurora-model-storage-ui/
```

Preservation hashes for the 12 inventoried files across the existing transcript archive, `audio_samples`, and Handy application data matched before and after cleanup. No shared Hugging Face cache was deleted. The installed configuration is version 1 with `directory: null`; the default models folder is empty. The cleanup was not added to startup, application commands, or the installer.
