#!/usr/bin/env python3
"""Reject incomplete or obsolete player downloads and preserve existing ZIPs on failure."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import zipfile


SPEC = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)

CONFIG = b"""[C64 and Cartridge Settings]
RAM Expansion Unit=Enabled
REU Size=16 MB
Map Ultimate Audio $DF20-DFFF=Enabled
[SID Addressing]
SID Socket 1 Address=Unmapped
SID Socket 2 Address=Unmapped
UltiSID 1 Address=$D400
UltiSID 2 Address=$D420
UltiSID Range Split=1/2 (A6)
Auto Address Mirroring=Disabled
[Audio Mixer]
Vol Socket 1=OFF
Vol Socket 2=OFF
Vol Sampler L=+6 dB
Vol Sampler R=+6 dB
Pan Sampler L=Left 3
Pan Sampler R=Right 3
"""


class ReleasePackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="havoc-release-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.package = self.root / "dist"
        for name in ("dist", "config", "docs", "assets/music"):
            (self.root / name).mkdir(parents=True)
        (self.root / "Cargo.toml").write_text('[package]\nname = "havoc"\nversion = "0.1.0"\n')
        (self.root / "config/havoc.cfg").write_bytes(CONFIG)
        (self.package / "havoc.cfg").write_bytes(CONFIG)
        (self.package / "havoc.prg").write_bytes(b"\x01\x08native fixture")
        reu = bytearray(16 * 1024 * 1024)
        reu[:4] = b"SM64"
        reu[4:8] = (0x534d1100).to_bytes(4, "little")
        reu[17] = 4
        (self.package / "havoc.reu").write_bytes(reu)
        (self.root / "docs/launch.txt").write_bytes(b"Launch instructions\n")
        (self.root / "LICENSE").write_bytes(b"MIT License\n")
        (self.root / "assets/music/LICENSE").write_bytes(
            b"MIT License\n\nCopyright (c) 2026 William Echols\n")

    def build(self):
        return release.package_release(self.package, self.root)

    def test_archive_has_exact_root_entries_and_bytes_without_debug_or_stale_files(self):
        (self.package / "havoc.elf").write_bytes(b"debug symbols")
        (self.package / "old-2sid.zip").write_bytes(b"obsolete")
        output = self.build()
        self.assertEqual(output.name, "havoc-v0.1.0.zip")
        with zipfile.ZipFile(output) as archive:
            self.assertEqual(archive.namelist(), ["havoc.cfg", "havoc.prg", "havoc.reu", "README.txt",
                                                "LICENSE", "MUSIC-LICENSE.txt"])
            for name in ("havoc.cfg", "havoc.prg", "havoc.reu"):
                self.assertEqual(archive.read(name), (self.package / name).read_bytes())
            self.assertEqual(archive.read("README.txt"), (self.root / "docs/launch.txt").read_bytes())
            self.assertEqual(archive.read("LICENSE"), (self.root / "LICENSE").read_bytes())
            self.assertEqual(archive.read("MUSIC-LICENSE.txt"), (self.root / "assets/music/LICENSE").read_bytes())

    def test_missing_or_empty_music_license_does_not_replace_existing_release(self):
        output = self.build()
        original = output.read_bytes()
        path = self.root / "assets/music/LICENSE"
        for missing in (True, False):
            with self.subTest(missing=missing):
                if missing:
                    path.unlink()
                else:
                    path.write_bytes(b"")
                with self.assertRaises(FileNotFoundError if missing else ValueError):
                    self.build()
                self.assertEqual(output.read_bytes(), original)
                self.assertEqual(list(self.package.glob(".*.tmp")), [])

    def test_missing_input_does_not_replace_existing_release(self):
        output = self.build()
        original = output.read_bytes()
        (self.package / "havoc.reu").unlink()
        with self.assertRaises(FileNotFoundError):
            self.build()
        self.assertEqual(output.read_bytes(), original)

    def test_rejects_two_sid_wrong_size_and_wrong_magic_reu(self):
        path = self.package / "havoc.reu"
        original = path.read_bytes()
        obsolete = original[:4] + (0x534d0900).to_bytes(4, "little") + original[8:]
        previous = original[:4] + (0x534d1000).to_bytes(4, "little") + original[8:]
        for data in (original[:17] + b"\x02" + original[18:], original[:-1],
                     original[:8 * 1024 * 1024], b"NOPE" + original[4:], obsolete, previous):
            with self.subTest(length=len(data), header=data[:18]):
                path.write_bytes(data)
                with self.assertRaisesRegex(ValueError, "four-SID REU"):
                    self.build()

    def test_rejects_stale_configuration_and_physical_sid_routing(self):
        path = self.package / "havoc.cfg"
        stale = CONFIG.replace(b"1/2 (A6)", b"Off")
        path.write_bytes(stale)
        with self.assertRaisesRegex(ValueError, "differs from config"):
            self.build()
        (self.root / "config/havoc.cfg").write_bytes(stale)
        with self.assertRaisesRegex(ValueError, "UltiSID Range Split"):
            self.build()
        physical = CONFIG.replace(b"SID Socket 1 Address=Unmapped", b"SID Socket 1 Address=$D440")
        path.write_bytes(physical)
        (self.root / "config/havoc.cfg").write_bytes(physical)
        with self.assertRaisesRegex(ValueError, "SID Socket 1 Address"):
            self.build()

    def test_rejects_invalid_prg_and_empty_launch_text(self):
        (self.package / "havoc.prg").write_bytes(b"\x00\x10invalid")
        with self.assertRaisesRegex(ValueError, "PRG"):
            self.build()
        (self.package / "havoc.prg").write_bytes(b"\x01\x08native fixture")
        (self.root / "docs/launch.txt").write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "empty"):
            self.build()

    def test_rejects_disabled_sampler_and_insufficient_reu_configuration(self):
        for old, new, message in (
                (b"REU Size=16 MB", b"REU Size=8 MB", "REU Size"),
                (b"Map Ultimate Audio $DF20-DFFF=Enabled", b"Map Ultimate Audio $DF20-DFFF=Disabled", "Map Ultimate Audio"),
                (b"Vol Sampler L=+6 dB", b"Vol Sampler L=OFF", "Vol Sampler L"),
                (b"Vol Sampler R=+6 dB", b"Vol Sampler R=OFF", "Vol Sampler R"),
                (b"Vol Sampler L=+6 dB", b"Vol Sampler L= 0 dB", "Vol Sampler L"),
                (b"Vol Sampler R=+6 dB", b"Vol Sampler R= 0 dB", "Vol Sampler R"),
                (b"Pan Sampler L=Left 3", b"Pan Sampler L=Center", "Pan Sampler L"),
                (b"Pan Sampler R=Right 3", b"Pan Sampler R=Left 3", "Pan Sampler R")):
            with self.subTest(setting=message):
                data = CONFIG.replace(old, new)
                (self.package / "havoc.cfg").write_bytes(data)
                (self.root / "config/havoc.cfg").write_bytes(data)
                with self.assertRaisesRegex(ValueError, message):
                    self.build()

    def test_failed_archive_verification_preserves_previous_zip_and_cleans_temporary(self):
        output = self.build()
        original = output.read_bytes()
        with mock.patch.object(release, "verify_archive", side_effect=ValueError("verification failed")):
            with self.assertRaisesRegex(ValueError, "verification failed"):
                self.build()
        self.assertEqual(output.read_bytes(), original)
        self.assertEqual(list(self.package.glob(".*.tmp")), [])

    def test_verifier_rejects_changed_bytes_and_extra_entries(self):
        contents = release.release_contents(self.package, self.root)
        output = self.build()
        with zipfile.ZipFile(output, "w") as archive:
            for name, data in contents.items():
                archive.writestr(name, b"changed" if name == "havoc.prg" else data)
        with self.assertRaisesRegex(ValueError, "differs from validated input"):
            release.verify_archive(output, contents)
        output = self.build()
        with zipfile.ZipFile(output, "a") as archive:
            archive.writestr("havoc.elf", b"unwanted debug file")
        with self.assertRaisesRegex(ValueError, "invalid entries"):
            release.verify_archive(output, contents)

    def test_version_uses_package_table_and_rejects_unsafe_filename(self):
        path = self.root / "Cargo.toml"
        path.write_text('[package]\nversion = "0.2.0-rc.1+test"\n[dependencies.demo]\nversion = "9.9.9"\n')
        self.assertEqual(release.archive_name(self.root), "havoc-v0.2.0-rc.1+test.zip")
        path.write_text('[package]\nversion = "../escape"\n')
        with self.assertRaisesRegex(ValueError, "safe semantic version"):
            release.archive_name(self.root)


if __name__ == "__main__":
    unittest.main()
