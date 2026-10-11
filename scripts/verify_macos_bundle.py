"""Verify local arm64 AURORA packaging without loading models or processing audio."""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def require(condition: bool, message: str) -> None:
    """Stop verification with a readable diagnostic when an invariant fails."""
    if not condition:
        raise ValueError(message)


def command(*arguments: str) -> str:
    """Run a native inspection command and preserve failures without shell evaluation."""
    return subprocess.run(
        arguments, check=True, capture_output=True, text=True
    ).stdout


def digest(path: Path) -> str:
    """Hash a regular resource with bounded memory instead of reading model-sized files."""
    require(path.is_file() and not path.is_symlink(), f"Missing regular resource: {path.name}")
    sha = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(65536), b""):
            sha.update(block)
    return sha.hexdigest()


def native_library(path: Path) -> list[str]:
    """Require arm64 Mach-O code with system-only dependencies and no development rpaths."""
    require(command("/usr/bin/lipo", "-archs", str(path)).strip() == "arm64",
            f"Expected arm64 only: {path.name}")
    dependencies = []
    # LC_ID_DYLIB is an install name, not a dependency; inspect load commands
    # rather than accidentally rejecting ORT's legitimate @rpath identity.
    blocks = command("/usr/bin/otool", "-l", str(path)).split("Load command ")[1:]
    for block in blocks:
        lines = [line.strip() for line in block.splitlines()]
        if any(line in {"cmd LC_LOAD_DYLIB", "cmd LC_LOAD_WEAK_DYLIB",
                        "cmd LC_REEXPORT_DYLIB", "cmd LC_LOAD_UPWARD_DYLIB"}
               for line in lines):
            name = next(line[5:].rsplit(" (offset ", 1)[0]
                        for line in lines if line.startswith("name "))
            require(name.startswith(("/usr/lib/", "/System/Library/")),
                    f"Non-system native dependency in {path.name}: {name}")
            dependencies.append(name)
        if "cmd LC_RPATH" in lines:
            name = next(line[5:].rsplit(" (offset ", 1)[0]
                        for line in lines if line.startswith("path "))
            require(name in {"@executable_path", "@loader_path"}
                    or name.startswith(("@executable_path/", "@loader_path/",
                                        "/usr/lib/", "/System/Library/")),
                    f"Development rpath in {path.name}: {name}")
    return dependencies


def verify_app(app: Path, build_binary: Path) -> None:
    """Check executable identity, native linkage, pinned resources and separate model provisioning."""
    contents = app / "Contents"
    with (contents / "Info.plist").open("rb") as source:
        info = plistlib.load(source)
    require(info["CFBundleIdentifier"] == "com.aurora.desktop", "Unexpected bundle identifier")
    require(info.get("LSMinimumSystemVersion") == "11.0", "Unexpected deployment target")
    executable = contents / "MacOS" / info["CFBundleExecutable"]
    require(digest(executable) == digest(build_binary), "Bundle/build executable mismatch")
    dependencies = native_library(executable)
    require(any("/Metal.framework/" in name for name in dependencies), "Metal linkage missing")
    # Native diagnostic strings survive release stripping. Together with system-only
    # linkage and the real GPU smoke test they corroborate the embedded ASR backend.
    binary = executable.read_bytes()
    require(b"parakeet run:" in binary and b"ggml_metal_library_init" in binary,
            "Embedded Parakeet/GGML Metal markers missing")
    del binary
    resources = contents / "Resources"
    source_ort = ROOT / "src-tauri/resources/onnxruntime"
    manifest = json.loads((source_ort / "manifest.json").read_text())
    require(manifest["version"] == "1.20.1" and manifest["architecture"] == "arm64",
            "Unexpected ORT manifest")
    require(digest(resources / "onnxruntime/manifest.json") == digest(source_ort / "manifest.json"),
            "Bundled ORT manifest mismatch")
    for name, expected in manifest["files"].items():
        require(Path(name).name == name, "Unsafe manifest resource name")
        require(digest(resources / "onnxruntime" / name) == expected,
                f"Bundled ORT checksum mismatch: {name}")
    native_library(resources / "onnxruntime/libonnxruntime.1.20.1.dylib")
    for name in ["LICENSE.txt", "GGML-LICENSE.txt", "NOTICE.txt"]:
        require(digest(resources / "transcribe" / name)
                == digest(ROOT / "src-tauri/resources/transcribe" / name),
                f"Bundled transcribe.cpp/GGML notice mismatch: {name}")
    require(not any(path.suffix.lower() in {".onnx", ".gguf"}
                    for path in resources.rglob("*")),
            "Model weights must remain in separately provisioned storage")
    native_files = {path.relative_to(contents).as_posix() for path in contents.rglob("*")
                    if path.is_file() and path.suffix.lower() in {".dylib", ".so", ".a"}}
    require(native_files == {"Resources/onnxruntime/libonnxruntime.1.20.1.dylib"},
            "Unexpected additional native libraries")
    print("APP passed: arm64, build identity, embedded Metal ASR, pinned CPU VAD runtime and notices.")


def main() -> int:
    """Verify explicit or default unsigned build artifacts and return a failing exit status on error."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path,
                        default=ROOT / "target/release/bundle/macos/AURORA.app")
    parser.add_argument("--dmg", type=Path,
                        default=ROOT / "target/release/bundle/dmg/AURORA_0.1.0_aarch64.dmg")
    parser.add_argument("--build-binary", type=Path, default=ROOT / "target/release/aurora")
    args = parser.parse_args()
    try:
        require(sys.platform == "darwin", "Verification requires macOS native tools")
        verify_app(args.app, args.build_binary)
        require(args.dmg.is_file(), "Missing DMG")
        command("/usr/bin/hdiutil", "verify", str(args.dmg))
        print("DMG passed: hdiutil integrity verification.")
        return 0
    except (OSError, ValueError, KeyError, StopIteration, subprocess.CalledProcessError) as error:
        print(f"Bundle verification failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
