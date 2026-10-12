# AURORA

Offline desktop audio transcription application built with Tauri v2, Rust 2021,
React 19, TypeScript, Vite, and Tailwind CSS 4.

## Development

Install Node.js/npm, Rust, CMake, a C++ toolchain, and the native prerequisites for Tauri.
The macOS package minimum is **11.0**; actual execution is verified only on the current Mac.
On macOS, install Apple Command Line Tools and run `brew install cmake`.
The static Metal build embeds its shader sources and compiles them through Metal at runtime;
it does not require the separate `xcrun metal` compiler or model downloads during builds.
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
ASR weights and the CPU VAD session remain resident until more than five idle minutes.
Each file gets a scoped ASR session on the selected device, released on completion, cancellation, or failure.
The bundled desktop icon uses the supplied artwork, preserved in
`src-tauri/icons/source.svg`. Regenerate the icons with Tauri's `icon` command
using `--fit cover` to preserve proportions while cropping to a square.

The Luminous workspace opens at 1280 × 800 with three views: **Transcription
Studio**, **AI Models**, and **Settings**. Studio groups audio import, job controls
and the archive alongside the transcript editor. AI Models displays the native
ASR/VAD descriptors and model-folder actions; Settings owns the session-only
ASR device preference. Navigation preserves unsaved edits and running jobs,
while pausing audio without losing the current block, position or speed.

The sidebar collapses below 1024 px and becomes top navigation below 768 px.
Inter and Plus Jakarta Sans are bundled locally as WOFF2 fonts; their OFL
licenses ship in `public/licenses`. No fonts are fetched during app use.

`npm run dev` provides all three views as a browser layout preview with native
actions disabled and no simulated models or transcripts. Native import is
available through `npm run tauri -- dev`.

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
on blocking workers. ASR weights and the VAD session are released together after inactivity and
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

Rust dependencies pin `ort`/`ort-sys` to `2.0.0-rc.9` for Silero VAD and
`transcribe-cpp`/`transcribe-cpp-sys` to **0.2.4** for ASR. Lockfiles preserve
resolved dependencies. The native ASR library is statically linked; macOS builds
enable Metal with embedded shaders. ONNX Runtime remains bundled solely for VAD.
All function documentation and important code comments are written in English.

## Parakeet GGUF ASR API and configuration

`TranscriberEngine::new(&Path)` loads the verified local **Parakeet TDT 0.6B v3 Q8_0**
from [Handy's official GGUF repository](https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf).
The pinned revision is `90f082450fcbacdb54e5900c44ef697c9ea59622` and the sole ASR file is
`parakeet-tdt-0.6b-v3-Q8_0.gguf` (739,508,576 bytes), with SHA-256
`5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7`.
Streaming checksum verification precedes native parsing. Metadata must identify
`parakeet`, `tdt-0.6b-v3`, and a 16 kHz sample rate. Weights are CC-BY-4.0;
attribute NVIDIA's original model and Handy's conversion when redistributing them.

ASR defaults to **Auto, GPU only**: Metal on macOS Apple Silicon, CUDA then Vulkan
on Windows/Linux. Auto tries registered GPUs in backend priority and registry order,
releasing each failed attempt before the next. It never uses CPU ASR implicitly.
An explicit GPU selection must initialize that exact native device or fail.
**CPU · explicit opt-in** enables strict CPU ASR only when selected by the user;
CPU is never a fallback for Auto or GPU selections. CPU decoding/resampling,
Silero VAD and auxiliary native scheduler operations remain expected.

The Models panel distinguishes the requested selection from the actual initialized
backend/device. Choices apply immediately while model operations are idle, freeing
old weights before reload; failures retain the requested choice and expose an
actionable safe error. **Reload selection** retries the same choice. Preference is
session-only, survives model-folder changes and inactivity reload, and resets to
Auto on every app restart. Device IDs are opaque and valid only in the current
process. Discovery reports the static native registry, not Cargo features; driver
or device changes may require restarting the application.

`TranscriberEngine::new(&Path)` retains GPU-only Auto behavior.
`new_with_selection(&Path, &AsrSelection)` accepts `Auto {}`, `Cpu {}` or
`Device { id }`. `discover_devices()` reports supported registered devices without
loading weights. `device()` returns the verified hardware description and backend;
`provider()` remains available. Verification checks the exact native handle,
backend and GPU/CPU class after model loading and before each file session.
Import and the archive remain usable when ASR is unavailable.

### Native build backends

macOS enables static Metal by default; GPU ASR is supported on Apple Silicon.
Intel macOS GPU ASR and other operating systems are unsupported by the application
policy; CPU remains an explicit option wherever the native model can initialize.
Windows/Linux default builds have no GPU backend enabled. Enable the optional
features when building on those systems:

```sh
cargo build --release --locked --features asr-cuda
cargo build --release --locked --features asr-vulkan
cargo build --release --locked --features asr-cuda,asr-vulkan
# With Tauri packaging on Windows/Linux:
npm run tauri -- build --features asr-cuda,asr-vulkan
```

CUDA builds require NVIDIA's CUDA toolkit (including nvcc), a compatible native
C++ toolchain and runtime NVIDIA drivers. Vulkan builds require Vulkan headers,
libraries and the `glslc` shader compiler (Vulkan SDK), plus a compatible runtime
GPU driver. Both require CMake and the normal Tauri platform prerequisites.
CUDA/Vulkan features are rejected outside Windows/Linux. Backends remain statically
linked; there is no native provider-directory picker. Windows/Linux still require
the existing trusted ONNX Runtime 1.20.1 setup for CPU Silero VAD. CUDA/Vulkan builds,
hardware execution and installers are **unverified** until exercised on actual targets.

`transcribe_chunk(&mut self, &[f32]) -> Result<String, EngineError>` preserves the
finite mono 16 kHz contract in `[-1, 1]`, at most 25 seconds, with empty input
returning empty text. Standalone calls own a short-lived native session.
`with_file` provides one scoped `FileTranscriber` session for a file's serial chunks,
without reloading the resident weights. Every run uses offline transcription,
no native timestamps, no language hint, and no cross-chunk prompt. Only text is
returned; source-relative subtitle boundaries continue to come from AURORA's VAD.

Tauri owns the model and CPU VAD in `EngineState`. Construction, inference, idle
unloading, destination changes and exit cleanup use blocking workers. Native RAII
session ownership releases results and compute buffers at file completion, error,
cancellation or unwinding. Rust token/result copies drop after extracting text.
Returned text belongs to the caller; release does not promise allocator zeroing.
Native logging is disabled before loading, and `TRANSCRIBE_DUMP_DIR` is rejected
when nonempty because upstream tensor dumps would bypass logging controls.

The `transcriber-state` event and `transcriber_state()` snapshot retain phase,
provider, error, missing, downloadFile and percentage, and add revision, selection,
devices and device. Provider is `Metal`, `CUDA`, `Vulkan` or `CPU` only for verified
ready models; device is null during loading, missing/unavailable status and inactivity
unload. Selection is `{ mode: "auto" }`, `{ mode: "cpu" }` or
`{ mode: "device", id }`. Catalog entries are `{ id, backend, name, description }`.
`transcriber_devices()` discovers hardware on a blocking worker and returns the same
snapshot. `select_asr_device({ selection })` reserves model operations and reloads
offline. Unknown IDs, CPU disguised as a GPU ID and unexpected payload fields are
rejected. Monotonic revisions prevent stale reads/events from restoring old device
status. Hardware diagnostics contain no native handles, audio paths or transcript
contents; errors omit native error strings and model paths.

Model folder configuration, explicit downloads, retries and atomic verified promotion
remain. The required installation is GGUF ASR plus Silero v5.1 ONNX. Legacy ONNX ASR
files are preserved on disk but ignored. No automatic model download or audio cache is added.

The desktop resolves models through native storage. Standalone GGUF probes use
`AURORA_ASR_MODEL_DIR` without ORT. Tests that also use Silero require trusted
`ORT_DYLIB_PATH` and the local VAD model. ONNX runtime preflight remains for VAD.

## ASR integration verification

Ordinary Rust/frontend tests do not download models or execute CPU ASR. The ignored
Metal tests are explicit, local functional checks, with no speed benchmarks:

```sh
export AURORA_ASR_MODEL_DIR=/absolute/model/directory/parakeet
cargo test --workspace --locked --test parakeet metal_load_and_file_sessions -- --ignored --exact
cargo run --release --locked --example engine_probe
```

The first check verifies loading and session recovery with empty inputs, including
an error inside a file operation. The probe prints only readiness or safe errors.
The application pipeline check downloads only missing/corrupt models into the
explicitly supplied native profile and uses the first **at most ten seconds** of
`audio_samples/audio.m4a`. It keeps audio and output in memory, saves no transcript,
prints no transcript contents, and verifies Metal, VAD boundaries, progress,
per-file teardown, inactivity unload and offline reload:

```sh
export AURORA_M7_PROFILE="/absolute/application/data/profile"
export ORT_DYLIB_PATH="/absolute/trusted/libonnxruntime.1.20.1.dylib"
cargo test --workspace --locked --lib transcription::tests::milestone7_metal_pipeline -- --ignored --exact --nocapture --test-threads=1
```

Milestone 8 adds a separate **offline** check using already verified local models.
It creates an isolated temporary profile with hard links, leaves user configuration
and models untouched, and checks exact explicit Metal placement, the same authorized
<=10-second pipeline, resource release, and selection preservation across offline reload:

```sh
export AURORA_M8_MODEL_BASE="/absolute/existing/model/folder"
export ORT_DYLIB_PATH="/absolute/trusted/libonnxruntime.1.20.1.dylib"
cargo test --workspace --locked --lib transcription::smoke::milestone8_metal_pipeline -- --ignored --exact --nocapture --test-threads=1
```

Run this ignored test only with authorization for the first ten seconds of
`audio_samples/audio.m4a`. It prints only a safe pass/fail summary. Ordinary tests
use simulated GPU loaders and verify CPU opt-in policy without running CPU ASR.
The Metal pipeline and exact placement were verified on the available Apple Silicon
Mac on 2026-10-11; no CPU ASR benchmarks or additional long audio runs were performed.

Historical milestone 5 used the istupakov FP32 ONNX export and verified CPU parity
against onnx-asr 0.12.0 with short public Italian and English samples. CoreML
initialization failed and CPU fallback worked. Those results do not describe the
current GGUF backend. Milestone 12 removed the historical ONNX ASR reference
script and retired graph, TDT and vocabulary modules; these historical results
are retained only as documentation.

## Desktop storage and workflow

Tauri resolves the current user's application data directory. On macOS it is
`~/Library/Application Support/com.aurora.desktop/`:

```text
com.aurora.desktop/
├── model-storage.json   # Version 1; null directory selects default
├── models/
│   ├── parakeet/         # Verified Parakeet v3 Q8_0 GGUF; legacy files may remain
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
ASR chunks. **Cancel transcription** signals a fresh job-scoped token, checked
between phases/chunks and by Parakeet's native abort callback at supported decode
boundaries. Loading, decoding and the CPU VAD pass finish their current operation;
native GPU work is not forcibly terminated. Cancellation discards all partial text,
including native abort results, and wins until terminal completion is published.
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
job, including failed/cancelled jobs. A one-second monitor releases ASR weights and the VAD session
strictly after 300 seconds with no reserved job. Their model files remain on disk.
The next job reloads them offline. Releasing sessions does not guarantee an
immediate equivalent drop in process RSS: allocator/native runtime bookkeeping
can remain resident. ONNX Runtime itself stays loaded for the process lifetime.

## Milestone 11 pipeline and cancellation

The serial chunk orchestrator is shared by production and deterministic tests.
It preserves borrowed audio and original-file `f64` timestamps, with one scoped
ASR session per file. The effective native session window must accept the existing
25-second VAD chunks; incompatible configurations fail with a safe error.
Events and command responses use one frontend application path. Terminal jobs
reject further same-job snapshots so late responses cannot replace edited text.
IPC payloads and the version-1 archive format remain unchanged.

Decoding still reads the complete file into RAM before VAD. Long-file memory use
therefore grows with decoded duration (mono 16 kHz `f32` alone uses about 230 MB
per hour, with additional decoder/resampler/native overhead). This milestone
adds neither streaming decode nor a promise of bounded memory for arbitrary files.
Long timelines and many chunks are tested deterministically, without long ASR runs.

The ignored functional test requires explicit authorization to use at most the
first ten seconds of `audio_samples/audio.m4a` and existing verified local models:

```sh
export AURORA_M11_MODEL_BASE="/absolute/existing/model/folder"
export ORT_DYLIB_PATH="/absolute/path/to/libonnxruntime.1.20.1.dylib"
cargo test --workspace --locked --lib transcription::smoke::milestone11_metal_pipeline -- --ignored --exact --nocapture --test-threads=1
```

It hard-links models into a temporary profile, verifies the exact Metal device,
transcribes the short clip with real CPU Silero VAD, exercises native abort and
cancellation of a second short pipeline operation, edits/saves/reopens a transcript,
and verifies TXT/SRT/VTT files inside that profile. It also checks joint idle
unload and offline reload. No model download, CPU ASR, audio cache, transcript
logging, or write to the personal archive occurs. The temporary profile is removed.

Verification on macOS Apple Silicon (2026-10-11): 72 ordinary Rust tests and
6 frontend tests passed, plus the explicit short Metal test and real Silero
silence/repeated-call test. Frontend production build, Rust/Prettier formatting,
and Clippy with warnings denied passed. Deterministic tests cover cancellation
before processing, during simulated inference from another thread, between chunks,
after the final chunk, typed native abort, failure cleanup, next-job isolation,
long timestamps and ordered revisions. Existing tests cover the strict idle
threshold, busy protection, native imports, archive validation and subtitle formatting.
The functional checks use the production backend with a mock Tauri runtime;
native picker/save-dialog interaction, packaging, Windows/Linux runtime execution
and sustained long-file inference were not performed for this milestone.

## Historical milestone 6 verification and macOS packaging

Frontend event-order tests use Node.js 22.6+ with type stripping (`npm test`).
Rust tests cover atomic model promotion with simulated transfers, exclusive
operations, controlled idle deadlines, loader reuse/reload, native selection IDs,
cancellation cleanup, archive corruption, and subtitle formatting.

The following commands and full-file workflow document milestone 6 history.
They are superseded by the bounded milestone 7 checks above and are not current test commands:

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

## Milestone 10 model management

The typed installation manifest distinguishes **ASR / GGUF** (Parakeet TDT
0.6B v3, Q8_0) from **VAD / ONNX** (Silero v5.1, CPU-only). Download,
missing-file reporting and local integrity verification use the same artifact
metadata. ASR uses the existing immutable Handy revision and SHA-256; Silero
retains its existing `Derur/silero-models` source on `main`, with its trusted
SHA-256 pinning the accepted content. Model folders keep the existing layout:
`parakeet/parakeet-tdt-0.6b-v3-Q8_0.gguf` and `silero_vad.onnx`.

`transcriber_state` and `transcriber-state` include a `models` array with
`id`, `name`, `version`, `purpose` (`ASR` or `VAD`), `format` (`GGUF` or `ONNX`),
and nullable `quantization`. These descriptors remain available during loading,
downloads and idle release; they reveal no local paths, download sources or
checksums. The Models panel displays both identities alongside existing controls.
Presence checks are cheap and do not claim integrity; `ready` requires verified
files and successful initialization of both runtimes.

Download models remains explicit. Verified files are reused, failed or corrupt
transfers can be retried, and only verified staged files are atomically promoted.
Legacy ASR ONNX files and all other existing user files remain untouched. There
is no configuration migration or VAD algorithm change; milestone 9 stays skipped.

The opt-in test downloads both complete artifacts into an empty temporary
profile, loads Metal ASR and CPU VAD **without audio inference**, advances the
idle deadline, reloads locally, switches to a second temporary folder with
verified hard-linked artifacts, and reconstructs application state to check the
persisted custom choice. It requires network access and a working Metal GPU:

```sh
ORT_DYLIB_PATH="$PWD/src-tauri/resources/onnxruntime/libonnxruntime.1.20.1.dylib" \
  cargo test --lib model_storage::tests::milestone10_clean_install_and_custom_reload \
  -- --ignored --exact --nocapture
```

## Milestone 10 verification (2026-10-11)

On the available Apple Silicon Mac, 67 ordinary Rust tests and 5 frontend tests
passed, along with Clippy (`--workspace --all-targets -- -D warnings`), Rust and
Prettier formatting checks, the production frontend build, and server rendering
of both model identities and the existing Models controls. Tests cover mixed
resource presence, checksum failures, atomic promotion, retry, preservation of
legacy/unrelated files, snapshot serialization and metadata retention across
ordered updates.

The opt-in clean-install test passed with a complete Hugging Face download into
an empty temporary profile, verified artifact hashes, actual Metal model loading,
CPU Silero initialization, idle release and offline reload. It also loaded both
models from a second custom directory and recreated native storage/engine state
to verify persisted selection and offline initialization. Temporary profiles and
models were released automatically. No audio was decoded or transcribed.

Hugging Face metadata at the pinned Handy revision confirmed the configured
GGUF SHA-256 and exact size (739,508,576 bytes). Native-window interaction and
new package generation were not performed for this milestone; the backend test
uses the same provisioning code as the unchanged UI action. Windows/Linux native
execution remains unverified. These milestone 10 results did not validate packaging;
ONNX ASR cleanup was subsequently completed in milestone 12.

## Milestone 7 verification (2026-10-11)

Verified on the available Apple Silicon Mac running macOS 26.6.2: 56 ordinary
Rust tests, two frontend tests, Rust/Prettier formatting, Clippy with warnings
denied, and the production frontend build. The authorized pipeline test downloaded
and verified the pinned GGUF through model provisioning, used real Metal ASR and
Silero CPU VAD on at most the first 10 seconds of `audio_samples/audio.m4a`, and
checked usable output, timestamps, idle unload and offline reload. The extract stayed
in memory; no transcript was printed or saved. No CPU ASR or benchmark was run.

Unsigned arm64 `.app` and `.dmg` builds succeeded. The app declares macOS 11.0,
contains the transcribe.cpp/GGML license notices and the retained Silero ONNX Runtime
resources, and links ASR statically with system frameworks. Older macOS versions,
Intel Macs and Windows/Linux execution remain unverified. The current Homebrew Rust
toolchain emits a deployment-version warning for its prebuilt compiler builtins;
declaring 11.0 does not establish execution compatibility on an older Mac.

## Model storage intervention verification (historical, 2026-10-10)

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

## VAD-only ONNX Runtime isolation (milestone 12)

Parakeet ASR uses GGUF through transcribe.cpp. Only Silero v5.1 VAD uses
ONNX Runtime: its private `vad::runtime` module owns the pinned rc.9 loader,
fallible 1.20.1 C preflight and one process-lifetime retained library handle.
Repeated model loads reuse the initialized environment; attempts to initialize
another canonical runtime path are rejected. Silero owns one CPU-only session
with one intra-op and one inter-op thread, with no ONNX GPU providers enabled.
The ASR error type and inference modules contain no ORT types or initialization.
Checksum and absolute environment-path utilities are runtime-independent.

The macOS arm64 desktop keeps its checksum-validated official ONNX Runtime
1.20.1 dylib, MIT license, third-party notices and checksum manifest. Desktop
loading uses Tauri resources; `ORT_DYLIB_PATH` is used by standalone VAD tools
and explicit local tests on macOS. Windows/Linux desktop loading retains the
existing requirement for an absolute `ORT_DYLIB_PATH` naming a trusted 1.20.1
library. Their native runtime packaging and execution remain unverified.
GPU CUDA/Vulkan features refer exclusively to transcribe.cpp ASR.

The Models panel reports the actual ASR backend with transcribe.cpp and the
independent `VAD: CPU (ONNX)` role. Explicit CPU ASR remains available, while
Auto and explicit GPU selection keep their existing GPU-only policy. IPC shapes,
model checksums, downloads, cancellation and joint 300-second idle unloading
are unchanged. Legacy user-owned ONNX models remain on disk and are ignored;
no automatic cleanup or downloads were introduced.

A lightweight local runtime test checks repeated initialization and rejects a
conflicting library path without running any ASR:

```sh
ORT_DYLIB_PATH="$PWD/src-tauri/resources/onnxruntime/libonnxruntime.1.20.1.dylib" \
cargo test --workspace --locked --lib vad::runtime::tests::local_runtime_reuse_and_conflicting_path -- --ignored --exact
```

Verified on macOS Apple Silicon on 2026-10-11: 73 ordinary Rust tests,
six frontend tests, the explicit real-runtime reuse/conflicting-path test and
the real Silero silence/padding/repeated-call test passed. The local Silero file
matched its pinned digest. Clippy with warnings denied (all local targets),
Rust/Prettier formatting, native `cargo build --workspace --locked`, frontend
production build and server rendering of
unloaded/Metal/explicit CPU runtime labels passed. Cargo feature inspection on
macOS shows only dynamic-loading ORT features, with no ONNX GPU providers;
manifest inspection confirms no target-specific ORT GPU features remain.
The lockfile removes 36 unused packages without introducing new dependency
versions. All four bundled ORT resource digests passed, and bundle mappings
retain ORT and transcribe.cpp license resources without ASR ONNX graphs.
Cross-target Cargo tree inspection could not finish offline because some
non-macOS dependencies are not cached; no downloads were performed. Native
Windows/Linux execution, new app/DMG packaging and native-window interaction
were not tested here. No ASR inference, benchmark or user-owned file changes
were performed.

## Milestone 13: functional validation and macOS packaging

Build the local unsigned Apple Silicon packages and verify their composition:

```sh
npm run tauri -- build --bundles app,dmg -- --locked --offline
python3 scripts/verify_macos_bundle.py
python3 -m unittest discover -s scripts -p 'test_verify_macos_bundle.py'
```

The verifier uses only Python's standard library and macOS `lipo`, `otool` and
`hdiutil`. It checks the bundled executable against `target/release/aurora`,
arm64 architecture, system-only native dependencies, absence of development
rpaths, Metal linkage and embedded Parakeet/GGML diagnostics. Those static checks
corroborate backend inclusion; the short real Metal test verifies execution.
ORT's install name is not mistaken for a loaded dependency. Every pinned ORT
resource and the bundled manifest must match; transcribe.cpp/GGML licenses and
notice must match their source copies. Unexpected separate native libraries and
bundled model weights are rejected. Finally, `hdiutil verify` checks DMG integrity.
Failures return a nonzero exit status; paths can be overridden with `--app`,
`--dmg` and `--build-binary` for another build directory.

The outputs are `target/release/bundle/macos/AURORA.app` and
`target/release/bundle/dmg/AURORA_0.1.0_aarch64.dmg`. transcribe.cpp/GGML and Metal
shader sources remain statically embedded. CPU ONNX Runtime 1.20.1 and native
license notices are packaged as resources. **Both Parakeet GGUF and Silero v5.1
ONNX weights remain separately provisioned** in the selected model folder through
the existing explicit download action; they are not included in the installer.

Repeat ordinary checks without model downloads or real ASR:

```sh
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
npm test
npm run format:check
npm run build
```

The separately authorized functional checks use existing verified local models:

```sh
export AURORA_M11_MODEL_BASE="/absolute/existing/model/folder"
export AURORA_SILERO_MODEL="$AURORA_M11_MODEL_BASE/silero_vad.onnx"
export ORT_DYLIB_PATH="$PWD/src-tauri/resources/onnxruntime/libonnxruntime.1.20.1.dylib"
cargo test --workspace --locked --offline --lib transcription::smoke::milestone11_metal_pipeline -- --ignored --exact --nocapture --test-threads=1
cargo test --workspace --locked --offline --test silero_vad -- --ignored --exact local_silero_silence_and_repeated_calls --test-threads=1
cargo test --workspace --locked --offline --lib vad::runtime::tests::local_runtime_reuse_and_conflicting_path -- --ignored --exact --test-threads=1
```

The Metal check uses at most the first ten seconds of `audio_samples/audio.m4a`
in memory and an isolated temporary profile for cancellation, archive, exports
and idle release/offline reload. It never prints the personal transcript or
creates an audio extract. Hard-link isolation can require permission outside a
filesystem sandbox. DMG creation also requires access to macOS packaging tools.

For native-window validation, generate a non-personal fixture using an already
installed local voice, then import the WAV from the new `.app` using its dialog:

```sh
AURORA_M13_TMP=$(mktemp -d /private/tmp/aurora-m13.XXXXXX)
say -v Samantha -r 160 -o "$AURORA_M13_TMP/synthetic.aiff" \
  'Aurora checks local speech. This is a short synthetic test.'
ffmpeg -nostdin -hide_banner -loglevel error \
  -i "$AURORA_M13_TMP/synthetic.aiff" -t 10 -ar 16000 -ac 1 \
  -map_metadata -1 "$AURORA_M13_TMP/synthetic.wav"
ffprobe -v error -show_entries format=duration \
  -of default=noprint_wrappers=1 "$AURORA_M13_TMP/synthetic.wav"
```

Check that the fixture is nonempty and no longer than ten seconds; sandboxed
speech-service access can otherwise produce an empty file. Confirm Metal ASR and
CPU ONNX VAD labels, start transcription, edit the text and export TXT/SRT/VTT to
this temporary directory. Do not press Save: archive save/reopen is exercised in
the isolated backend test, leaving the personal archive unchanged.

Verified on macOS Apple Silicon on 2026-10-11: 73 ordinary Rust tests, six
frontend tests and five portable verifier tests passed, together with formatting,
Clippy with warnings denied, the production frontend build and unsigned `.app` /
`.dmg` packaging. The explicit ten-second Metal pipeline, real Silero fixture and
runtime reuse/conflicting-path checks passed using local models without downloads.
The generated app passed executable/resource identity and native linkage checks;
the DMG passed `hdiutil verify`. The verifier tests reject external dependencies
(including weak/reexport/upward loads), development rpaths, incorrect architectures,
resource changes and symlinks while accepting ORT's dylib identity and local rpath.

The generated bundle was launched directly and its process path confirmed.
Native import and save dialogs, real Metal on Apple M4 Pro (MTL0), CPU ONNX VAD,
completion at 100%, editing and TXT/SRT/VTT export passed with a 3.732-second local
synthetic fixture. Export bytes contained the edited text and correct source
timestamps. The personal archive was unchanged; the app was closed without saving
an archive entry. No CPU ASR benchmark, long ASR inference, personal audio extract,
model download or legacy model cleanup was performed.

Signing/notarization, Intel and older macOS execution, Windows/Linux and
CUDA/Vulkan packaging remain unverified. A declared macOS 11.0 deployment target
does not establish compatibility with older hardware or systems.
