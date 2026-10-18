<p align="center">
  <img src="src-tauri/icons/icon.png" alt="AURORA app icon" width="128" height="128" />
</p>

# AURORA

A desktop app for local audio transcription, built with Tauri, Rust and React.
Import an audio file, transcribe it offline, review the result alongside the
original audio, and save or export your edited text.

## Features

- Import one WAV, MP3, FLAC, M4A or OGG/Vorbis file using a native picker or drag and drop.
- Transcribe locally with Parakeet TDT 0.6B v3 Q8_0 and Silero v5.1 voice activity detection.
- Choose Auto-detect, Italiano or English as a source-language hint.
- Edit transcript blocks with playback, seeking and speed controls.
- Keep an explicitly saved local transcript archive and export TXT, SRT or VTT.
- Manage model storage and select automatic, GPU or CPU ASR execution.

## Platform support

The desktop workflow and unsigned app/DMG packaging have been verified on
**macOS with Apple Silicon**. Metal accelerates ASR; voice activity detection
runs on CPU. The declared macOS deployment target is 11.0, but compatibility
with older macOS versions and Intel Macs has not been verified.

Windows/Linux execution and installers are unverified. Their default builds
have no GPU backend enabled and require a trusted local ONNX Runtime 1.20.1
library for VAD. Optional CUDA/Vulkan build instructions are in the
[development guide](docs/development.md). Signing and notarization are unverified.

## Run from source

### Requirements

- Node.js and npm. The frontend test command requires a Node.js version supporting
  `--experimental-strip-types`; the current local toolchain uses Node.js 25.
- Rust with Cargo and a native C/C++ toolchain.
- CMake and the native prerequisites for Tauri on your platform.

On macOS, install Apple Command Line Tools (`xcode-select --install`) and CMake
(`brew install cmake` if you use Homebrew). ASR embeds its Metal shaders;
a separate `xcrun metal` compiler is not required.

### Start the desktop app

From the repository root:

```sh
npm ci
npm run tauri -- dev
```

Installing dependencies requires network access unless they are already cached.
Builds do not download transcription models. Provision models explicitly in the app
as described below.

`npm run dev` starts a browser layout preview. Native file access, model
management and transcription require the desktop app.

## First launch: set up models

1. Open **AI Models**.
2. Use the default model directory or select **Choose folder** to use an existing,
   writable directory. **Open folder** shows the current destination;
   **Use default** restores the default location.
3. Select **Download models**. This explicitly connects to Hugging Face to fetch
   and verify the required Parakeet and Silero files. No API key is required.
4. Wait for **Verified models ready**, then open **Transcription Studio**.

The Parakeet file alone is approximately 740 MB. Leave additional free space
for download and staging copies. Changing the model directory leaves existing
files in place; it does not move or delete them.

Once models are installed, transcription and model reloads work offline.
Models leave memory after more than five idle minutes and reload from disk
when needed.

## Transcribe and review audio

1. In **Transcription Studio**, choose or drop one supported audio file.
   Directories and multiple-file drops are rejected.
2. Check the file details and **Transcription language**. New imports use the
   preference saved in **Settings**. A file-specific choice lasts for that import;
   **Use default** restores the saved preference.
3. Select **Transcribe audio** and follow the progress indicator. Use
   **Cancel transcription** to stop; cancellation discards partial text and may
   wait for the current processing operation to finish.
4. Edit the completed transcript. Use **Play**, **Pause**, the position slider and
   **Speed** to review each block against the original audio.
5. Select **Save transcript** to retain the result in the local archive, or
   **Export TXT**, **Export SRT** or **Export VTT** to choose an output file.

The language choice is a model hint, not a translation request or a guarantee
of the output language. Audio with no detected speech produces an empty result.

Review blocks target about 10 minutes for files up to 30 minutes, or 20 minutes
for longer files. Exported subtitle cues use those block boundaries; they are
not word-aligned captions. Exports include current edits and omit empty blocks.

Saved transcripts can be reopened from the archive. Audio is not embedded in
saved documents; playback after reopening may require selecting the matching
original file again. Navigation preserves unsaved edits and pauses playback.

## Privacy and local storage

Audio decoding and transcription run on your computer. The app does not upload
audio or transcripts, and it includes no telemetry. Its model download action
uses Hugging Face; installed models are reused offline. Fonts are bundled locally.

Audio and playback buffers are processed in memory. Saving a transcript is
explicit: saved documents and exports contain **unencrypted text** on your disk.
Saved documents include transcript text, timing and source metadata such as
an audio fingerprint and duration, but do not embed source audio or its path.
Protect saved files as you would other personal documents.

Tauri resolves the per-user application data directory. On macOS this is
`~/Library/Application Support/com.aurora.desktop/`. It contains saved transcripts,
language settings and model-folder configuration, plus models when using the
default destination. Language defaults and the selected model folder persist;
the requested ASR device resets to Auto on restart.

## Troubleshooting

| Problem                            | Action                                                                                                                   |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Models unavailable                 | Check the selected model folder and use **Download models** for missing or corrupt files.                                |
| Custom model folder inaccessible   | Reconnect the drive, choose a writable folder, or select **Use default**.                                                |
| GPU initialization fails           | Auto and explicit GPU selections attempt CPU recovery. Check the active device label; use **Reload selection** to retry. |
| Import fails                       | Check that the file is a supported, readable audio file. Corrupt streams or changing audio parameters can be rejected.   |
| Archive playback needs audio       | Select the matching original audio file through the native picker.                                                       |
| Native actions disabled in browser | Start the desktop app with `npm run tauri -- dev`.                                                                       |

## Developer documentation

- [Development, checks and packaging](docs/development.md)
- [Architecture, model contracts and storage](docs/architecture.md)
- [Synthetic audio fixtures](src-tauri/tests/fixtures/README.md)

Third-party runtime and font notices are included in
`src-tauri/resources/` and `public/licenses/`. Model weights are downloaded
separately; consult their licenses before redistribution. This repository does
not currently declare a project-level license.
