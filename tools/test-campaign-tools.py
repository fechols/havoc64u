#!/usr/bin/env python3
"""Check native fixture clock conversion and expanded campaign coverage."""
import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


vice = load("vice-smoke")
bench = load("bench-native")


def package(space):
    count = 18 if space else 12
    data = bytearray(0x2000 + count * 2048)
    data[:4] = b"SM64"
    data[4:8] = (0x534d0600 if space else 0x534d0500).to_bytes(4, "little")
    data[12:16] = (0x2000).to_bytes(4, "little")
    data[16:18] = bytes((count, 2))
    for stage in range(count):
        start = 0x2000 + stage * 2048
        data[start:start + 4] = b"SMS2"
        data[start + 8:start + 16] = bytes((stage % 4, 13 + stage - 12 if stage >= 12 else 1,
                                          4 if stage >= 12 else 0,
                                          5 if stage == 17 else 4 if stage == 16 else 0,
                                          min(stage, 15), min(stage + 1, 15), max(stage - 1, 0), 0))
    return data


def layout(space):
    return {"fields": {"stage_id": {"offset": 0, "size": 1, "type": "u8"},
                       ("stage_ticks" if space else "rail_ticks"):
                           {"offset": 1, "size": 2, "type": "u16"}},
            "modes": {"Playing": 1, "Transition": 6}, "address": 0}


class CampaignTools(unittest.TestCase):
    def test_full_boss_seeds_follow_initialized_old_and_balanced_targets(self):
        for health in ((20, 20, 60), (40, 40, 120)):
            state = dict(zip(("boss_left", "boss_right", "boss_core"), health))
            selected = vice.full_boss_health(dict(state, mode=0))
            self.assertEqual(selected, state)
            for fixture in (bench.fixtures(True, True, True, selected)
                            + bench.cache_sequence(True, True, True, selected)):
                for name, value in state.items():
                    if name in fixture["patch"]:
                        self.assertEqual(fixture["patch"][name], value, fixture["name"])
        for health in ((0, 40, 120), (40, 40, 256), (100, 100, 100)):
            with self.assertRaisesRegex(RuntimeError, "initialized boss health"):
                vice.full_boss_health(dict(zip(("boss_left", "boss_right", "boss_core"), health)))

    def test_music_selection_defaults_to_complete_package_and_rejects_unknown_cues(self):
        self.assertEqual(vice.music_cue_selection(13, None), list(range(13)))
        self.assertEqual(vice.music_cue_selection(21, None), list(range(21)))
        self.assertEqual(vice.music_cue_selection(21, [20, 19, 13, 20]), [13, 19, 20])
        for count, requested in ((13, [13]), (21, [-1]), (21, [21]), (21, [])):
            with self.assertRaisesRegex(RuntimeError, "this .*cue package"):
                vice.music_cue_selection(count, requested)

    def test_versions_and_boss_overlay_aliases(self):
        self.assertEqual(vice.package_model_layout(package(False))["count"], 12)
        self.assertEqual(vice.package_model_layout(package(True))["count"], 18)
        self.assertEqual([vice.boss_sim_profile(2, kind) for kind in range(6)],
                         [2, 4, 5, 6, 6, 5])

    def test_last_stage_clock_exceeds_old_absolute_limit_without_wrapping(self):
        values = vice.stage_fixture({"stage_id": 17, "rail_ticks": 17 * 3750 + 2600},
                                    layout(True), package(True))
        self.assertNotIn("rail_ticks", values)
        self.assertEqual((values["stage_id"], values["stage_ticks"]), (17, 2600))
        self.assertEqual(values["stage_info.boss_kind"], 5)
        self.assertEqual(vice.logical_game_state(values)["rail_ticks"], 66350)

    def test_crossing_belongs_to_outgoing_stage_and_victory_to_final_stage(self):
        crossing = vice.stage_fixture({"mode": 6, "rail_ticks": 12 * 3750},
                                      layout(True), package(True))
        self.assertEqual((crossing["stage_id"], crossing["stage_ticks"]), (11, 3750))
        victory = vice.stage_fixture({"mode": 3, "rail_ticks": 18 * 3750},
                                     layout(True), package(True))
        self.assertEqual((victory["stage_id"], victory["stage_ticks"]), (17, 3750))

    def test_late_boss_clock_uses_explicit_identity(self):
        late = vice.clock_fixture({"stage_id": 16, "rail_ticks": 16 * 3750 + 8000},
                                  layout(True)["fields"], 18)
        self.assertEqual((late["stage_id"], late["stage_ticks"]), (16, 8000))

    def test_baseline_clock_conversion_is_explicit_and_checks_conflicts(self):
        local = {"stage_id": 11, "stage_ticks": 1200}
        old = vice.clock_fixture(local, layout(False)["fields"], 12)
        self.assertEqual(old, {"stage_id": 11, "rail_ticks": 42450})
        self.assertEqual(vice.clock_fixture(old, layout(True)["fields"], 18), local)
        with self.assertRaisesRegex(RuntimeError, "Conflicting"):
            vice.clock_fixture(dict(local, rail_ticks=42451), layout(True)["fields"], 18)
        with self.assertRaisesRegex(RuntimeError, "u16"):
            vice.clock_fixture({"rail_ticks": 67500}, layout(False)["fields"], 18)
        with self.assertRaisesRegex(RuntimeError, "u16"):
            vice.clock_fixture({"stage_id": 14, "rail_ticks": 100}, layout(True)["fields"], 18)

    def test_reads_expose_actual_and_derived_clocks_without_fake_dwarf_fields(self):
        fields = layout(True)
        state = vice.read_game_state(None, fields, ["rail_ticks"], bytes((17, 0x28, 0x0a)))
        self.assertEqual(state, {"stage_id": 17, "stage_ticks": 2600, "rail_ticks": 66350})
        self.assertNotIn("rail_ticks", fields["fields"])

    def test_all_benchmark_fixtures_convert_and_preserve_original_terrestrial_workloads(self):
        baseline = bench.fixtures(True, True)
        expanded = bench.fixtures(True, True, True)
        before = {item["name"]: item for item in baseline}
        after = {item["name"]: item for item in expanded}
        for name in ("title", "lowlands", "canyon", "base", "carrier", "missiles",
                     "full-pools", "section-boundary"):
            self.assertEqual(before[name], after[name])
        self.assertEqual({item["section"] for item in expanded
                          if item["name"].startswith("bridge-")}, set(range(17)))
        for name in ("boss-dreadnought", "boss-gate-guardian", "hazards-asteroid-belt",
                     "enemies-jump-gate-full", "bridge-11-200", "bridge-16-399"):
            self.assertIn(name, after)
        for fixture in expanded + bench.cache_sequence(True, True, True):
            values = vice.clock_fixture(fixture["patch"], layout(True)["fields"], 18)
            self.assertNotIn("rail_ticks", values)
            self.assertTrue(0 <= values["stage_ticks"] <= 65535, fixture["name"])
            self.assertEqual(values["stage_id"], fixture["section"], fixture["name"])

    def test_pixels_dma_rejects_gap_writes_wrong_addresses_and_missing_reload(self):
        symbols = {"__phase_pixels_prefix_size": 12180, "__phase_pixels_aux_size": 643,
                   "__phase_pixels_aux_load": 0x43f20, "__phase_pixels_aux_start": 0x6f20}
        transfers = [{"reu": 0x40000 + offset, "cpu": 0x3000 + offset,
                      "bytes": min(1024, 12180 - offset), "phase_marker": 3}
                     for offset in range(0, 12180, 1024)]
        transfers.append({"reu": 0x43f20, "cpu": 0x6f20, "bytes": 643, "phase_marker": 7})
        self.assertTrue(bench.validate_pixels_transfers(transfers, symbols)["gap_skipped"])
        self.assertIsNone(bench.validate_pixels_transfers([], {}))
        for field, value in (("reu", 0x43f21), ("cpu", 0x6f1f), ("bytes", 644), ("phase_marker", 6)):
            bad = [dict(item) for item in transfers]
            bad[-1][field] = value
            with self.assertRaisesRegex(RuntimeError, "PIXELS split DMA"):
                bench.validate_pixels_transfers(bad, symbols)
        for bad in (transfers[:-1],
                    transfers + [{"reu": 0x43000, "cpu": 0x6000, "bytes": 1024, "phase_marker": 7}]):
            with self.assertRaisesRegex(RuntimeError, "PIXELS split DMA"):
                bench.validate_pixels_transfers(bad, symbols)

    def test_native_pitch_audit_combines_bytes_and_ignores_effect_lanes(self):
        def frequency(note):
            return round(440 * 2 ** ((note - 69) / 12) * (1 << 24) / 985248)
        registers = [bytearray(25), bytearray(25)]
        notes = [69, 72, 76]
        events = []
        for note in notes:
            value = frequency(note)
            events += [{"address": 0xd400, "value": value & 255},
                       {"address": 0xd401, "value": value >> 8},
                       {"address": 0xd404, "value": 0x41}]
        events += [{"address": 0xd40e, "value": 255},
                   {"address": 0xd40f, "value": 255}]
        checked = vice.music_pitch_audit(events, registers, [(0, 0)], [set(notes)], 985248)
        self.assertEqual(checked[0]["observed_notes"], notes)
        self.assertEqual(checked[0]["verified_frequency_writes"], 3)
        events[0]["value"] ^= 16
        with self.assertRaisesRegex(RuntimeError, "detuned"):
            vice.music_pitch_audit(events, registers, [(0, 0)], [set(notes)], 985248)
        constant = frequency(82)
        registers[0][0:2] = constant.to_bytes(2, "little")
        checked = vice.music_pitch_audit([{"address": 0xd404, "value": 0x81}],
                                        registers, [(0, 0)], [{82}], 985248)
        self.assertEqual(checked[0]["observed_notes"], [82])
        self.assertEqual(checked[0]["verified_gate_attacks"], 1)


if __name__ == "__main__":
    unittest.main()
