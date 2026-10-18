# Architecture and data contracts

AURORA uses Tauri v2, Rust 2021, React 19, TypeScript, Vite and Tailwind CSS 4.
Native operations own filesystem access and run heavy audio/model work on
blocking workers. Shared frontend IPC types live in `src/lib/`; Rust modules
separate import, audio, VAD, ASR, playback, persistence and export responsibilities.

## Audio and voice activity detection

`AudioDecoder::decode_to_16khz_mono(&Path)` returns an owned
`Result<Vec<f32>, AudioDecodeError>`. It decodes the first supported audio track
in WAV, MP3, FLAC, M4A or OGG/Vorbis. Audio channels are converted to planar
buffers before resampling, then averaged into finite 16 kHz mono samples bounded
to `[-1, 1]`. FFT delay and flush padding are excluded. Codec padding depends on
container metadata. Invalid samples, changing stream parameters and detectable
corruption produce errors; detection depends on the codec and available metadata.
Callers own and drop decoded buffers after processing.

`SileroVad::new(&Path)` loads Silero v5.1 using CPU execution with one intra/inter-op
thread. `chunk_audio(&mut self, &[f32])` returns borrowed `AudioChunk` slices with
original-file timestamps. Empty input and silence yield no chunks.

Inference uses 512-sample frames, 64 context samples and recurrent state shaped
`[2, 1, 128]`. The final frame is padded only for inference. Context and recurrent
state are scoped to each call. Speech starts at probability >= 0.50 and ends
after at least 300 ms below 0.35. Events shorter than 1.5 seconds are discarded
before padding. Short pauses group events toward 15–25 second ASR chunks;
continuous speech splits at 25 seconds. Margins are at most 30 ms, bounded by the
file and chunk limits. These inference chunks differ from editor review blocks.

## Models and runtime lifecycle

Available artifacts are declared centrally in the backend model manifest. Only the selected ASR and shared VAD are required:

| Role | Artifact                         | Source                                                       |
| ---- | -------------------------------- | ------------------------------------------------------------ |
| ASR  | Parakeet TDT 0.6B v3 Q8_0 GGUF   | `handy-computer/parakeet-tdt-0.6b-v3-gguf` on Hugging Face   |
| ASR  | Whisper large-v3-turbo Q8_0 GGUF | `handy-computer/whisper-large-v3-turbo-gguf` on Hugging Face |
| VAD  | Silero v5.1 ONNX                 | `Derur/silero-models` on Hugging Face                        |

Parakeet uses revision `90f082450fcbacdb54e5900c44ef697c9ea59622` and the
739,508,576-byte `parakeet-tdt-0.6b-v3-Q8_0.gguf` artifact. SHA-256 digests in the
manifest pin every artifact. Whisper Turbo uses revision
`18ada752005f5fddef499256f9a458d0b37a203a`, size 886,381,728 bytes and SHA-256
`257aa7ec9121c22c167d5e2c49e3e76b4ba0e6e27ebc13074eba0a80f8190220`. Downloading is explicit, disables Hugging Face
authentication tokens, verifies files and atomically promotes staged copies.
Valid local files are reused. A `.download-cache/` inside the selected model
folder supports transfers; only requested repository caches are removed after
successful provisioning. A model-specific download repairs that ASR and shared
Silero; downloading Silero alone never downloads an ASR.

`select_asr_model(modelId)`, `download_model(modelId)` and `delete_model(modelId)`
accept catalog identities only. Selection persists before an offline reload and may
refer to an absent model. `EngineSnapshot.selectedModel` identifies the selected
ASR, `installed` lists present artifact IDs, and descriptors include `sizeBytes`.
Presence does not imply integrity: readiness requires checksum and native contract
validation. Catalog status and device diagnostics use the same monotonic revision.

Deletion is confirmed in a focus-trapping dialog, then uses the native reservation
shared by downloads, folder changes and transcription. Required sessions are
released on a worker before removing the exact artifact and its repository cache.
The selected ASR remains unchanged when deleted. Removing Silero blocks both ASRs.
Deleting an inactive ASR leaves resident sessions intact. Descendant symlinks are
rejected; unrelated files, other ASRs and other folders remain untouched. Filesystem
errors are recoverable and partial cache cleanup failures are reported.

Resident ASR weights and the VAD session are managed through Tauri state.
Each file gets a scoped native ASR session, released on completion, failure or
cancellation. Serial inference consumes borrowed VAD chunks; decoded audio and
partial text stay scoped to the job. A one-second monitor releases resident
weights and VAD strictly after 300 idle seconds with no reserved operation.
Subsequent jobs reload offline. Runtime/allocator bookkeeping may remain resident.

Auto tries supported registered GPU devices, then CPU. Apple Silicon uses Metal;
Windows/Linux optional builds prioritize CUDA then Vulkan. An explicit GPU choice
tries that device and then CPU; explicit CPU skips GPUs. Initialization failures
eligible for CPU recovery expose a safe fallback category. Corrupt resources,
invalid configuration and cancellation stop initialization. Inference failures
do not automatically repeat transcription. Requested device preferences last
only for the current process; device IDs are opaque and process-local.

`TranscriberEngine::new(&Path)` uses Auto; `new_with_selection` accepts `AsrSelection`; both retain Parakeet compatibility.
`new_with_model` accepts `AsrModelId` and `AsrSelection` for catalog-specific loading.
`transcribe_chunk` accepts finite mono 16 kHz samples, at most 25 seconds, and
returns empty text for empty input. `with_file` provides scoped session reuse;
language-aware APIs accept `TranscriptionLanguage` (`auto`, `it`, `en`).

Silero's ONNX Runtime is independent of ASR acceleration. A fallible preflight
checks the trusted local library, version and C API before entering the pinned
ORT loader. The library remains loaded for the process lifetime and conflicting
paths are rejected. The macOS arm64 desktop uses bundled resources and ignores
`ORT_DYLIB_PATH`; standalone tests and Windows/Linux use explicitly configured
trusted libraries. Loading a native library executes code and is not sandboxed
by preflight. Bundled digests and third-party notices accompany the runtime.

## Import, transcription and playback

Native dialogs and native drag-and-drop authorize imports. WebView commands do
not supply arbitrary filesystem paths to backend file operations. Imported audio
is validated on a blocking worker; validation samples are dropped after duration
calculation. Playback uses bounded in-memory audio for an authorized source.

Jobs decode, detect speech, transcribe serial chunks and group the completed
result into review blocks. Files up to 30 minutes target 10-minute blocks;
longer files target 20-minute blocks, choosing nearby chunk boundaries. Blocks
have immutable original-file timing and editable text. Subtitle exports use
these block boundaries rather than word-level alignment.

Cancellation uses a job-scoped token checked between phases/chunks and at
supported native decode boundaries. Operations may finish their current work;
GPU work is not forcibly terminated. Cancellation discards partial results and
wins until terminal completion. Import/replacement/removal remain locked while
the worker finishes. Navigation preserves edits and jobs while pausing playback.

## Persistence and privacy

Tauri resolves the per-user application data directory. It contains:

```text
com.aurora.desktop/
├── settings.json          # Versioned language and selected ASR preferences
├── model-storage.json     # Versioned selected model directory
├── models/                # Default model destination
│   ├── parakeet/
│   ├── whisper/
│   └── silero_vad.onnx
└── transcripts/
    └── <uuid>.json         # Explicitly saved transcript documents
```

A custom model directory replaces only the model destination. Changing folders
validates access but does not move or delete files. Unavailable custom folders
remain selected and report a recoverable error. Model loading, downloading,
transcription and folder changes share native operation reservations.

Settings saves are atomic and independent of model storage. Settings v2 stores
`selectedModel` alongside `language`; v1 restores Parakeet while preserving the
saved language, and migrates on the next explicit save. Changing either preference
preserves the other. Invalid settings activate Auto-detect and Parakeet with a warning. Per-file overrides remain in memory;
untouched imports follow preference updates. Job start captures the effective
language for all chunks. A source-language hint is not a translation request.

Transcript saves are explicit and atomic. Documents include UUID, name, schema
version, timestamps, edited text and block timing; v2 source metadata includes
audio duration and SHA-256 fingerprint. They do not embed audio or source paths.
Legacy documents remain readable; playback can require native reassociation
with matching source audio. Corrupt archive documents are skipped.

TXT/SRT/VTT export uses native save dialogs, reflects current edits, omits empty
blocks and validates timing. Cancelling the dialog writes nothing. Saved
transcripts and deliberate exports contain unencrypted text. Audio is neither
uploaded nor written to processing caches. Diagnostics use safe categories and
exclude audio, transcript contents and sensitive native paths. Model downloads
are the explicit network operation; fonts are bundled locally.

Third-party notices are stored with runtime resources and fonts. Parakeet weights
are distributed separately under CC-BY-4.0; attribute NVIDIA's original model and
Handy's conversion when redistributing them. Whisper Turbo weights are MIT licensed,
with OpenAI attribution and Handy conversion provenance in the bundled notices.
No project-level license is declared.
