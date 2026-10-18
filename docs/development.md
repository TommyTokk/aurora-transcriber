# Development

See the [README](../README.md) for prerequisites, desktop startup and user workflows.
The repository is a Cargo workspace with the backend in `src-tauri/` and the
React/TypeScript frontend in `src/`. Run commands from the repository root.

## Checks

```sh
cargo test --workspace --locked --offline
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
npm test
npm run format:check
npm run build
python3 -m unittest discover -s scripts -p 'test_verify_macos_bundle.py'
```

Cargo's `--offline` flag requires cached dependencies. Install dependencies
beforehand when needed; `npm ci` uses the committed npm lockfile. Ordinary tests
use synthetic audio and fixture documents, without private recordings, model
downloads or real model inference. FFmpeg is only needed to regenerate the
[audio fixtures](../src-tauri/tests/fixtures/README.md).

Format changes with `cargo fmt --all` and `npm run format`.
`npm run dev` is a browser preview with native actions disabled;
`npm run tauri -- dev` runs the desktop integration.

## Optional local model tests

These tests are ignored in the ordinary suite. Supply existing verified models
and a trusted native runtime; the tests never download them. Paths below are
placeholders for resources on your own computer.

```sh
export AURORA_SILERO_MODEL=/absolute/path/to/silero_vad.onnx
export ORT_DYLIB_PATH=/absolute/path/to/libonnxruntime.dylib
cargo test --workspace --locked --offline --test silero_vad -- --ignored
```

Silero requires ONNX Runtime **1.20.1**. On macOS Apple Silicon, the repository's
bundled runtime can be used for standalone tests:

```sh
export ORT_DYLIB_PATH="$PWD/src-tauri/resources/onnxruntime/libonnxruntime.1.20.1.dylib"
cargo test --workspace --locked --offline --lib vad::runtime::tests::local_runtime_reuse_and_conflicting_path -- --ignored --exact
```

The optional Metal test loads resident ASR weights and checks scoped file
sessions without transcribing audio:

```sh
export AURORA_ASR_MODEL_DIR=/absolute/path/to/models/parakeet
cargo test --workspace --locked --offline --test parakeet -- --ignored
```

It requires an Apple Silicon Metal device and the pinned Parakeet GGUF.
The catalog lifecycle smoke test requires a model folder containing both ASRs
(`parakeet/` and `whisper/`) and `silero_vad.onnx`:

```sh
export AURORA_MODEL_SMOKE_BASE=/absolute/path/to/models
cargo test --workspace --locked --offline --lib catalog_models_pipeline_and_lifecycle -- --ignored --nocapture
```

It requires real Metal execution and transcribes only the first ten seconds of
`audio_samples/audio.m4a`. It uses an isolated temporary profile and hard-linked
weights to verify switching, persisted selection, offline repair/reload and deletion.
It never prints transcript content or deletes source model files. The recording
must be authorized for local testing. Other ignored pipeline smoke tests are
environment-specific and are not part of the ordinary verification commands.

## Native backends

The native ASR dependencies `transcribe-cpp` and `transcribe-cpp-sys` are pinned
to 0.2.4 and statically linked. macOS enables Metal with embedded shader sources.
The `ort` and `ort-sys` dependencies are pinned to 2.0.0-rc.9 and used only for
CPU Silero VAD through dynamic loading of ONNX Runtime 1.20.1.

Windows/Linux builds have optional ASR features:

```sh
cargo build --release --locked --features asr-cuda
cargo build --release --locked --features asr-vulkan
npm run tauri -- build --features asr-cuda,asr-vulkan
```

CUDA requires the CUDA toolkit including `nvcc`, a compatible C++ toolchain and
NVIDIA drivers. Vulkan requires Vulkan headers/libraries, the `glslc` shader
compiler and compatible drivers. Both also require CMake and Tauri's native
platform prerequisites. These features are rejected outside Windows/Linux.
Their hardware execution and packaging are unverified.

Windows/Linux desktop VAD requires an absolute `ORT_DYLIB_PATH` pointing to a
trusted ONNX Runtime 1.20.1 library. The bundled runtime is macOS arm64 only;
Intel macOS and other platform runtime packaging are unverified.

## macOS packaging

On an appropriately configured Apple Silicon Mac:

```sh
npm run tauri -- build --bundles app,dmg -- --locked --offline
python3 scripts/verify_macos_bundle.py
```

For version 0.1.0, outputs are:

- `target/release/bundle/macos/AURORA.app`
- `target/release/bundle/dmg/AURORA_0.1.0_aarch64.dmg`

The verifier uses Python's standard library and macOS `lipo`, `otool` and
`hdiutil`. It checks executable identity, architecture, native dependencies,
resource digests, license resources and DMG integrity, and rejects bundled model
weights. Use `--app`, `--dmg` and `--build-binary` to override locations.
Static Metal checks establish backend inclusion; hardware execution requires
separate functional testing. Packages are unsigned; signing, notarization and
older macOS compatibility remain unverified.

Model weights are provisioned separately through the model card **Download** actions or **Download selected + VAD**.
Packaging includes the CPU ONNX Runtime and transcribe.cpp/GGML license notices.

## App icon

The source artwork is `src-tauri/icons/source.svg`. Regenerate native icon
formats with:

```sh
npm run tauri -- icon src-tauri/icons/source.svg --output src-tauri/icons
```

The README reuses the existing PNG. Keep the original proportions and transparent
corners when changing the artwork.

## Repository hygiene

Keep local credentials, recordings, model weights and saved transcripts out of
Git. Ignore rules cover common credential files, `.env` files and macOS metadata;
sanitized `.env.example` files may be shared after review. Ignore rules do not
remove files already committed or erase history. Review staged files and scan
all reachable history before publishing. If a credential was committed, revoke
it and agree on history cleanup before publishing; never include its value in
an issue, log or report.
