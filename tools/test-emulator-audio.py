#!/usr/bin/env python3
"""Check explicit emulator audio selection without modifying package files."""
import argparse
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


spec = importlib.util.spec_from_file_location("vice_smoke", Path(__file__).with_name("vice-smoke.py"))
vice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vice)
spec = importlib.util.spec_from_file_location("bench_native", Path(__file__).with_name("bench-native.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)
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
            image[4:8] = (0x534d0b00).to_bytes(4, "little")
            image[16:18] = bytes((18, 4))
            (package / "havoc.reu").write_bytes(image)
            result = subprocess.run(
                [sys.executable, str(Path(__file__).with_name("bench-native.py")),
                 "--package-dir", str(package), "--out-dir", str(folder / "output"),
                 "--audio-backend", "native"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("SID audio snapshots cannot restore DAC state", result.stderr)


class Memory:
    def __init__(self):
        self.data = bytearray([0xa5]) * 65536
        self.reads = []
        self.writes = []

    def read(self, address, size):
        self.reads.append((address, size))
        return self.data[address:address + size]

    def write(self, address, data):
        self.writes.append((address, len(data)))
        self.data[address:address + len(data)] = data


def audio_layouts(split):
    """Synthetic DWARF deliberately separates owners and includes untouched bytes."""
    layouts = bench.Layouts(None, None, None)
    music = {
        "underruns": {"offset": 2, "type": "u16"},
        "row": {"offset": 5, "type": "u8"},
        "row_ticks": {"offset": 6, "type": "u8"},
        "loaded": {"offset": 8, "type": "bool"},
        "ready": {"offset": 9, "type": "bool"},
        "waiting": {"offset": 10, "type": "bool"},
        "kernel": {"offset": 12, "type": "u8"},
        "enabled": {"offset": 14, "type": "bool"},
        "initialized": {"offset": 15, "type": "bool"},
    }
    if split:
        layouts.structs = {
            "MusicEngine": {"size": 18, "fields": music},
            "EffectsEngine": {"size": 12, "fields": {
                "event_count": {"offset": 1, "type": "u8"},
                "enabled": {"offset": 4, "type": "bool"},
                "initialized": {"offset": 6, "type": "bool"},
                "remaining": {"offset": 9, "type": "u8"},
            }},
        }
    else:
        layouts.structs = {"AudioEngine": {"size": 30, "fields": dict(
            music, event_count={"offset": 22, "type": "u8"},
            remaining={"offset": 27, "type": "u8"})}}
    return layouts


class BenchmarkAudioTests(unittest.TestCase):
    def initialized(self, split=True):
        layouts = audio_layouts(split)
        # Actual local Rust statics have mangled symbols, unlike exported hooks.
        symbols = {"_ZN9havoc_c645AUDIO17habc123E": 0x2200}
        if split:
            symbols["_ZN9havoc_c647EFFECTS17hdef456E.1"] = 0x2600
        audio = bench.AudioState(layouts, symbols)
        mon = Memory()
        for kind, address in audio.regions.items():
            size = layouts.size(kind)
            mon.data[address:address + size] = bytes(range(11, 11 + size))
            for field in ("initialized", "enabled"):
                offset, _ = layouts.member(kind, field)
                mon.data[address + offset] = 1
        return audio, mon

    def test_split_snapshot_restores_both_owners_without_touching_surrounding_memory(self):
        audio, mon = self.initialized()
        before = bytes(mon.data)
        snapshot = audio.capture(mon)
        self.assertEqual(mon.reads, [(0x2200, 18), (0x2600, 12)])
        for address, size in mon.reads:
            mon.data[address:address + size] = bytes(size)
        audio.restore(mon.write, snapshot)
        self.assertEqual(mon.writes, [(0x2200, 18), (0x2600, 12)])
        self.assertEqual(bytes(mon.data), before)

    def test_split_restart_seed_resets_transport_and_queue_in_their_actual_owners(self):
        audio, mon = self.initialized()
        snapshot = audio.capture(mon)
        before = dict(snapshot)
        seed = audio.restart_seed(snapshot)
        expected_music = bytearray(snapshot["MusicEngine"])
        for offset in (2, 3, 5, 6, 8, 9, 10):
            expected_music[offset] = 0
        expected_music[12] = 255
        expected_effects = bytearray(snapshot["EffectsEngine"])
        expected_effects[1] = 0
        self.assertEqual(seed, {"MusicEngine": bytes(expected_music),
                                "EffectsEngine": bytes(expected_effects)})
        self.assertEqual(snapshot, before)

    def test_legacy_snapshot_and_restart_keep_the_single_owner_layout(self):
        audio, mon = self.initialized(split=False)
        snapshot = audio.capture(mon)
        self.assertEqual(mon.reads, [(0x2200, 30)])
        seed = audio.restart_seed(snapshot)
        expected = bytearray(snapshot["AudioEngine"])
        for offset in (2, 3, 5, 6, 8, 9, 10, 22):
            expected[offset] = 0
        expected[12] = 255
        self.assertEqual(seed, {"AudioEngine": bytes(expected)})
        audio.restore(mon.write, seed)
        self.assertEqual(mon.writes, [(0x2200, 30)])
        self.assertEqual(mon.data[0x2200:0x221e], expected)

    def test_uninitialized_or_disabled_split_owners_reject_the_warmup(self):
        audio, mon = self.initialized()
        for kind in ("MusicEngine", "EffectsEngine"):
            for field in ("initialized", "enabled"):
                with self.subTest(kind=kind, field=field):
                    snapshot = audio.capture(mon)
                    raw = bytearray(snapshot[kind])
                    audio.layouts.patch(raw, kind, field, 0)
                    snapshot[kind] = bytes(raw)
                    with self.assertRaisesRegex(RuntimeError, f"did not initialize SID {kind}"):
                        audio.restart_seed(snapshot)

    def test_ambiguous_symbols_and_missing_split_layout_do_not_use_legacy_state(self):
        layouts = audio_layouts(split=True)
        with self.assertRaisesRegex(RuntimeError, "Expected one resident EFFECTS ELF symbol"):
            bench.AudioState(layouts, {"AUDIO": 0x2200, "EFFECTS": 0x2600,
                                      "_ZN9havoc_c647EFFECTS17habc123E": 0x2800})
        with self.assertRaisesRegex(RuntimeError, "Expected one resident AUDIO ELF symbol"):
            bench.AudioState(layouts, {"EFFECTS": 0x2600})
        original_load = layouts.load

        def load(kind):
            if kind == "EffectsEngine":
                raise RuntimeError("Missing DWARF type EffectsEngine")
            return original_load(kind)

        with mock.patch.object(layouts, "load", side_effect=load):
            with self.assertRaisesRegex(RuntimeError, "Missing DWARF type EffectsEngine"):
                bench.AudioState(layouts, {"AUDIO": 0x2200, "EFFECTS": 0x2600})


if __name__ == "__main__":
    unittest.main()
