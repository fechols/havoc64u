#!/usr/bin/env python3
"""Check explicit emulator audio selection without modifying package files."""
import argparse
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("vice_smoke", Path(__file__).with_name("vice-smoke.py"))
vice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vice)
SYMBOLS = {"main": 0x0900, "EMULATOR_SID_ONLY": 0x0300}


class Monitor:
    def __init__(self, retain_write=True):
        self.calls = []
        self.value = 255  # CRT has not initialized this byte yet.
        self.bank = "cpu"
        self.at_main = False
        self.retain_write = retain_write

    def run_until(self, address, failures):
        self.calls.append(("run_until", address, failures))
        assert address == SYMBOLS["main"]
        self.at_main = True
        self.value = 0

    def command(self, command):
        self.calls.append(command)
        if command.startswith("bank "):
            self.bank = command.split()[1]
        else:
            assert self.at_main and self.bank == "ram"
            assert command == "> 0300 01"
            if self.retain_write:
                self.value = 1

    def integer(self, address, width):
        assert self.at_main and self.bank == "ram"
        assert (address, width) == (SYMBOLS["EMULATOR_SID_ONLY"], 1)
        return self.value


class EmulatorAudioTests(unittest.TestCase):
    def test_sid_override_waits_for_crt_and_records_live_ram_change(self):
        mon = Monitor()
        failures = {0x1000: "fail"}
        report = vice.configure_emulator_audio(mon, SYMBOLS, failures, {"digital": True})
        self.assertEqual(mon.calls, [("run_until", 0x0900, failures),
                                     "bank ram", "> 0300 01", "bank cpu"])
        self.assertEqual(mon.value, 1)
        self.assertTrue(report["sid_substitution"])
        self.assertEqual(report["monitor_patch"],
                         {"symbol": "EMULATOR_SID_ONLY", "address": 0x0300,
                          "before": 0, "value": 1})

    def test_native_mode_never_overrides_production_audio_even_without_hook(self):
        mon = Monitor()
        report = vice.configure_emulator_audio(mon, {}, {}, {"digital": True}, "native")
        self.assertEqual(mon.calls, [])
        self.assertEqual(report["backend"], "native")
        self.assertFalse(report["sid_substitution"])

    def test_legacy_sid_packages_need_no_hook_or_monitor_patch(self):
        for backend in ("sid", "native"):
            mon = Monitor()
            report = vice.configure_emulator_audio(mon, {}, {}, {"digital": False}, backend)
            self.assertEqual(mon.calls, [])
            self.assertEqual(report["backend"], "sid")
            self.assertFalse(report["sid_substitution"])

    def test_old_digital_package_requires_rebuild_before_any_execution(self):
        mon = Monitor()
        with self.assertRaisesRegex(RuntimeError, "rebuild with scripts/build.sh"):
            vice.configure_emulator_audio(mon, {"main": 0x0900}, {}, {"digital": True})
        self.assertEqual(mon.calls, [])

    def test_missing_main_is_rejected_before_any_monitor_write(self):
        mon = Monitor()
        with self.assertRaisesRegex(RuntimeError, "Missing native main"):
            vice.configure_emulator_audio(mon, {"EMULATOR_SID_ONLY": 0x0300}, {}, {"digital": True})
        self.assertEqual(mon.calls, [])

    def test_failed_write_restores_cpu_bank_and_rejects_run(self):
        mon = Monitor(retain_write=False)
        with self.assertRaisesRegex(RuntimeError, "did not retain"):
            vice.configure_emulator_audio(mon, SYMBOLS, {}, {"digital": True})
        self.assertEqual(mon.bank, "cpu")

    def test_cli_defaults_to_sid_and_offers_native_opt_in(self):
        parser = argparse.ArgumentParser()
        vice.add_emulator_audio_argument(parser)
        self.assertEqual(parser.parse_args([]).audio_backend, "sid")
        self.assertEqual(parser.parse_args(["--audio-backend", "native"]).audio_backend, "native")
        with self.assertRaisesRegex(ValueError, "Unknown emulator audio backend"):
            vice.configure_emulator_audio(Monitor(), SYMBOLS, {}, {"digital": True}, "automatic")

    def test_digital_benchmark_rejects_dac_mode_before_loading_an_engine_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            package = folder / "package"
            package.mkdir()
            image = bytearray(16 * 1024 * 1024)
            image[:4] = b"SM64"
            image[4:8] = (0x534d0a00).to_bytes(4, "little")
            image[16:18] = bytes((18, 4))
            (package / "havoc.reu").write_bytes(image)
            result = subprocess.run(
                [sys.executable, str(Path(__file__).with_name("bench-native.py")),
                 "--package-dir", str(package), "--out-dir", str(folder / "output"),
                 "--audio-backend", "native"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("AudioEngine snapshots cannot restore DAC state", result.stderr)


if __name__ == "__main__":
    unittest.main()
