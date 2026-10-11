"""Portable rejection tests for the macOS package verifier's trust boundaries."""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import verify_macos_bundle as verifier


# Match otool's load-command shape, including a dylib identity that is not loaded.
SYSTEM_LOADS = """library:
Load command 0
          cmd LC_ID_DYLIB
      cmdsize 64
         name @rpath/libonnxruntime.1.20.1.dylib (offset 24)
Load command 1
          cmd LC_LOAD_DYLIB
      cmdsize 56
         name /usr/lib/libSystem.B.dylib (offset 24)
"""


class NativeInspectionTests(unittest.TestCase):
    """Exercise package portability checks without executing macOS tools or inference."""

    def inspect(self, loads: str, architecture: str = "arm64") -> list[str]:
        """Supply native tool output so dependency validation can run on any platform."""
        with patch.object(verifier, "command", side_effect=[architecture, loads]):
            return verifier.native_library(Path("fixture.dylib"))

    def test_dylib_identity_is_not_a_dependency(self) -> None:
        """Accept ORT's install name while validating its actual system dependencies."""
        self.assertEqual(self.inspect(SYSTEM_LOADS), ["/usr/lib/libSystem.B.dylib"])

    def test_development_and_dynamic_asr_dependencies_fail(self) -> None:
        """Reject absolute build paths and external ASR libraries, including weak loads."""
        for name in ["/private/tmp/build/libggml.dylib", "/opt/homebrew/lib/libc++.dylib",
                     "@rpath/libtranscribe.dylib"]:
            for kind in ["LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB",
                         "LC_LOAD_UPWARD_DYLIB"]:
                loads = SYSTEM_LOADS.replace("/usr/lib/libSystem.B.dylib", name)
                loads = loads.replace("cmd LC_LOAD_DYLIB", f"cmd {kind}")
                with self.subTest(name=name, kind=kind):
                    with self.assertRaisesRegex(ValueError, "Non-system native dependency"):
                        self.inspect(loads)

    def test_development_rpath_fails(self) -> None:
        """Reject a hidden build search path even when all loaded libraries are system ones."""
        loads = SYSTEM_LOADS + """Load command 2
          cmd LC_RPATH
      cmdsize 48
         path /Users/developer/build (offset 12)
"""
        with self.assertRaisesRegex(ValueError, "Development rpath"):
            self.inspect(loads)
        for name in ["@executable_path/../Frameworks", "@loader_path"]:
            with self.subTest(name=name):
                self.assertEqual(self.inspect(loads.replace("/Users/developer/build", name)),
                                 ["/usr/lib/libSystem.B.dylib"])

    def test_wrong_architecture_fails(self) -> None:
        """Do not label Intel or universal packages as the verified arm64-only build."""
        for architecture in ["x86_64", "x86_64 arm64"]:
            with self.subTest(architecture=architecture):
                with self.assertRaisesRegex(ValueError, "Expected arm64 only"):
                    self.inspect(SYSTEM_LOADS, architecture)

    def test_resource_mutation_and_symlink(self) -> None:
        """Detect changed bytes and reject links that could escape packaged resources."""
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "notice.txt"
            file.write_bytes(b"abc")
            self.assertEqual(verifier.digest(file),
                             "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            original = verifier.digest(file)
            file.write_bytes(b"ab")
            self.assertNotEqual(verifier.digest(file), original)
            link = Path(directory) / "linked.txt"
            link.symlink_to(file)
            with self.assertRaisesRegex(ValueError, "Missing regular resource"):
                verifier.digest(link)


if __name__ == "__main__":
    unittest.main()
