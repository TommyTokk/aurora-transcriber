# Audio fixtures

These files contain a synthetic 440 Hz stereo tone lasting 250 ms at 48 kHz.
They contain no speech or personal data. WAV uses signed 16-bit PCM; the other
files use MP3, FLAC, AAC in M4A, and Vorbis in OGG respectively.

Regenerate from the repository root with `python3 scripts/generate_audio_fixtures.py`.
FFmpeg is needed only for regeneration; normal Rust tests run entirely offline.
Lossy encoders may introduce padding that is handled according to container metadata.

`changing-rate.ogg` chains the 48 kHz tone with a 44.1 kHz stream to exercise
rejection of changing stream parameters.
