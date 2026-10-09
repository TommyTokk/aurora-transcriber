"""Generate non-sensitive audio fixtures locally; requires FFmpeg on PATH."""

from pathlib import Path
import subprocess


def main():
    """Encode a 250 ms synthetic stereo tone in every supported test container."""
    output = Path(__file__).resolve().parents[1] / "src-tauri/tests/fixtures"
    output.mkdir(parents=True, exist_ok=True)
    source = output / "tone.wav"
    # Generate synthetic audio only; no microphones, recordings, or network inputs.
    subprocess.run([
        "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y",
        "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=0.25",
        "-ac", "2", "-c:a", "pcm_s16le", str(source),
    ], check=True)
    for extension, codec in [("mp3", "libmp3lame"), ("flac", "flac"),
                             ("m4a", "aac"), ("ogg", "vorbis")]:
        subprocess.run([
            "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y",
            "-i", str(source), "-map_metadata", "-1", "-c:a", codec, "-strict", "-2",
            str(output / f"tone.{extension}"),
        ], check=True)
    # Chained OGG streams exercise rejection of mid-file sample-rate changes.
    changed = subprocess.run([
        "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error",
        "-i", str(source), "-ar", "44100", "-map_metadata", "-1",
        "-c:a", "vorbis", "-strict", "-2", "-f", "ogg", "pipe:1",
    ], check=True, capture_output=True)
    (output / "changing-rate.ogg").write_bytes(
        (output / "tone.ogg").read_bytes() + changed.stdout
    )


if __name__ == "__main__":
    main()
