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


def package(space, version=None):
    count = 18 if space else 12
    data = bytearray(0x2000 + count * 2048)
    data[:4] = b"SM64"
    data[4:8] = (version if version is not None else
                 0x534d0800 if space else 0x534d0500).to_bytes(4, "little")
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
    def test_encounter_cache_reads_follow_linked_addresses(self):
        for first in (0x0600, 0x2e00):
            symbols = dict(__segment0_start=first, __segment0_end=first + 512,
                           __segment1_start=0x9700, __segment1_end=0x9900)
            self.assertEqual(vice.segment_cache_addresses(symbols), (first, 0x9700))
            for field in symbols:
                missing = dict(symbols)
                del missing[field]
                with self.assertRaisesRegex(RuntimeError, "encounter cache"):
                    vice.segment_cache_addresses(missing)
            symbols["__segment0_end"] -= 1
            with self.assertRaisesRegex(RuntimeError, "encounter cache 0"):
                vice.segment_cache_addresses(symbols)

    def test_hit_feedback_flashes_gray_hull_facets_but_preserves_gray_shadows(self):
        def triangle(depth, color, flags=0):
            return dict(x=[10, 20, 15], y=[20, 20, 10], depth=depth,
                        color=color, reserved=flags)

        mesh, shadow = triangle(900, 11), triangle(910, 11, 0x40)
        key = lambda command: tuple(command["x"] + command["y"] + [command["depth"]])
        reference = {key(command): (command["color"], command["reserved"])
                     for command in (mesh, shadow)}
        checked = vice.check_feedback_geometry(reference, [triangle(900, 1), shadow], True)
        self.assertEqual(checked, dict(white=True, triangles=1, shadow_triangles=1))
        vice.check_feedback_geometry(reference, [mesh, shadow], False)
        vice.check_feedback_geometry(reference, [triangle(900, 2), shadow], False,
                                     pulse={key(mesh)}, core_color=2)
        for commands, message in (([mesh, shadow], "target mesh"),
                                  ([triangle(900, 1), triangle(910, 1, 0x40)], "ground shadow"),
                                  ([triangle(900, 1)], "ground shadow")):
            with self.assertRaisesRegex(RuntimeError, message):
                vice.check_feedback_geometry(reference, commands, True)

    def test_pixel_profile_accounts_for_wait_separately_from_pack_and_copy(self):
        events = [{"address": 0x200, "value": marker, "bus_cycles": cycle}
                  for marker, cycle in ((3, 10), (9, 30), (4, 80), (1, 110), (0, 140))]
        frame = {"bus_cycles": 150, "phase_bus_cycles": {"pixels": 140}}
        vice.pixel_profile(events, 0x200, {"bus_cycles": 0}, [frame])
        self.assertEqual(frame["pixel_bus_cycles_detail"],
                         {"raster": 30, "hud": 0, "pack": 20, "copy": 30, "wait": 50})
        self.assertEqual(frame["pixel_unprofiled_bus_cycles"], 10)

    def test_scanout_commit_trace_pairs_all_bands_and_enforces_bus_time_bound(self):
        symbols = {"publish_guard": 0x3000, "publish_commit": 0x3100, "publish_done": 0x3200}

        def trace(events):
            return "".join(f"#3 (Trace on exec ${address:04x})\n"
                           f".C:{address:04x} EA NOP - A:00 X:00 Y:00 {cycle}\n"
                           for address, cycle in events)

        events = [(address, 1000 + band * 1000 + delta)
                  for band in range(25) for address, delta in ((0x3000, 0), (0x3100, 4), (0x3200, 440))]
        profile = bench.scanout_commit_profile(trace(events), symbols, 63)
        self.assertEqual(profile["copies"], 25)
        self.assertEqual(profile["max_bus_cycles"], 436)
        self.assertEqual(profile["max_protected_bus_cycles"], 440)
        self.assertEqual(profile["limit_bus_cycles_exclusive"], 441)
        self.assertEqual(profile["duration_bus_cycles"], [436] * 25)
        self.assertEqual(profile["guard_retries"], 0)
        retried = [(0x3000, 900), *events]
        self.assertEqual(bench.scanout_commit_profile(trace(retried), symbols, 63)["guard_retries"], 1)
        self.assertIsNone(bench.scanout_commit_profile("", {}, 63))
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            bench.scanout_commit_profile("", {"publish_commit": 0x3100}, 63)
        for bad in (events[:-1], events[1:], events[:2] + events, events[:3] + events[2:],
                    events[:-3], events + [(0x3000, 27000), (0x3100, 27004), (0x3200, 27440)]):
            with self.assertRaisesRegex(RuntimeError, "Scan-out commit"):
                bench.scanout_commit_profile(trace(bad), symbols, 63)
        for duration in (4, 441, 455):
            bad = list(events)
            bad[2] = (0x3200, 1000 + duration)
            with self.assertRaisesRegex(RuntimeError, "seven-scanline bound"):
                bench.scanout_commit_profile(trace(bad), symbols, 63)
        ntsc = [(address, cycle + (14 if address == 0x3200 else 0)) for address, cycle in events]
        self.assertEqual(bench.scanout_commit_profile(trace(ntsc), symbols, 65)["max_protected_bus_cycles"], 454)
        ntsc[2] = (0x3200, 1455)
        with self.assertRaisesRegex(RuntimeError, "seven-scanline bound"):
            bench.scanout_commit_profile(trace(ntsc), symbols, 65)

    def test_full_boss_seeds_follow_initialized_old_and_balanced_targets(self):
        for health in ((20, 20, 60), (40, 40, 120)):
            state = dict(zip(("boss_left", "boss_right", "boss_core"), health))
            selected = vice.full_boss_health(dict(state, mode=0))
            self.assertEqual(selected, state)
            for fixture in (bench.fixtures(True, True, True, selected)):
                for name, value in state.items():
                    if name in fixture["patch"]:
                        self.assertEqual(fixture["patch"][name], value, fixture["name"])
        for health in ((0, 40, 120), (40, 40, 256), (100, 100, 100)):
            with self.assertRaisesRegex(RuntimeError, "initialized boss health"):
                vice.full_boss_health(dict(zip(("boss_left", "boss_right", "boss_core"), health)))

    def test_array_boss_reports_and_legacy_fixture_paths_follow_dwarf_offsets(self):
        fields = {"bosses": {"offset": 50, "type": "havoc::game::BossInstance[4]"}}
        boss_fields = {"pos": {"offset": 0, "type": "havoc::Vec3"},
                       **{name: {"offset": 6 + index, "type": "u8"}
                          for index, name in enumerate(("left", "right", "core", "flash"))}}
        vec_fields = {name: {"offset": 2 * index, "type": "i16"}
                      for index, name in enumerate(("x", "y", "z"))}
        vice.add_boss_layout_fields(fields, boss_fields, vec_fields, 10)
        self.assertEqual(fields["boss_z"], fields["bosses[0].pos.z"])
        self.assertEqual(fields["boss_hit_flash_ticks"], fields["bosses[0].flash"])
        self.assertEqual(fields["bosses[3].core"]["offset"], 88)
        layouts = bench.Layouts(None, None, None)
        layouts.structs = {"Game": {"size": 90, "fields": {"bosses": fields["bosses"]}},
                           "BossInstance": {"size": 10, "fields": boss_fields},
                           "Vec3": {"size": 6, "fields": vec_fields}}
        raw = bytearray(90)
        layouts.patch(raw, "Game", "boss_left", 40)
        layouts.patch(raw, "Game", "bosses[3].core", 30)
        self.assertEqual((raw[56], raw[88]), (40, 30))
        state = dict(bosses=[dict(pos=dict(x=0, y=0, z=680), left=40, right=40, core=120, flash=0)])
        self.assertEqual(vice.full_boss_health(state), dict(boss_left=40, boss_right=40, boss_core=120))
        self.assertEqual(vice.logical_game_state(state)["boss_z"], 680)

    def test_split_boss_fixtures_seed_full_waves_and_saturated_shots(self):
        split = [fixture for fixture in bench.fixtures(True, True, True, split_boss=True)
                 if "boss_phase" in fixture["patch"]]
        self.assertEqual(len(split), 4)
        for fixture in split:
            patch = fixture["patch"]
            phase = patch["boss_phase"]
            count = 1 << phase
            self.assertEqual(patch["boss_split_ticks"], 0)
            self.assertEqual(sum(patch[f"bosses[{index}].core"] for index in range(count)), 120)
            self.assertTrue(all(patch[f"shots[{index}].kind"] in (1, 2) for index in range(24)))
            self.assertEqual(patch[f"bosses[{count - 1}].pos.z"], 500)

    def test_original_boss_fixture_clears_previous_split_state(self):
        fields = {"boss_phase": {}, "boss_split_ticks": {}}
        self.assertEqual(vice.clock_fixture(dict(boss_started=1, boss_z=680), fields, 18),
                         dict(boss_started=1, boss_z=680, boss_phase=0, boss_split_ticks=0,
                              **{"bosses[0].pos.x": 0, "bosses[0].pos.y": 0}))
        split = dict(boss_started=1, boss_phase=2, boss_split_ticks=17)
        self.assertEqual(vice.clock_fixture(split, fields, 18), split)

    def test_music_selection_defaults_to_complete_package_and_rejects_unknown_cues(self):
        self.assertEqual(vice.music_cue_selection(13, None), list(range(13)))
        self.assertEqual(vice.music_cue_selection(21, None), list(range(21)))
        self.assertEqual(vice.music_cue_selection(21, [20, 19, 13, 20]), [13, 19, 20])
        for count, requested in ((13, [13]), (21, [-1]), (21, [21]), (21, [])):
            with self.assertRaisesRegex(RuntimeError, "this .*cue package"):
                vice.music_cue_selection(count, requested)

    def test_victory_fade_and_restart_flow_follow_package_generation(self):
        digital = vice.package_model_layout(package(True, 0x534d0a00))
        legacy = vice.package_model_layout(package(True, 0x534d0800))
        self.assertEqual([vice.victory_music_cue(digital, tick) for tick in (0, 40, 99, 100, 400)],
                         [20, 20, 20, 5, 5])
        self.assertEqual([vice.victory_music_cue(legacy, tick) for tick in (0, 99, 100, 400)],
                         [5, 5, 5, 5])
        self.assertEqual(vice.results_restart_modes(digital, "Victory"), ("Title", "Playing"))
        self.assertEqual(vice.results_restart_modes(legacy, "Victory"), ("Playing",))
        for version in (legacy, digital):
            self.assertEqual(vice.results_restart_modes(version, "GameOver"), ("Playing",))

    def test_menu_fixtures_select_both_sticky_tunes_without_changing_legacy_fields(self):
        digital = vice.package_model_layout(package(True, 0x534d0a00))
        legacy = vice.package_model_layout(package(True, 0x534d0800))
        fields = {"menu_music_galactic": {"type": "bool", "offset": 10}}
        self.assertEqual(vice.menu_music_fixture(digital, fields, 0), {"menu_music_galactic": 0})
        self.assertEqual(vice.menu_music_fixture(digital, fields, 5), {"menu_music_galactic": 1})
        self.assertEqual(fields["menu_music_galactic"]["size"], 1)
        self.assertEqual(vice.menu_music_fixture(legacy, {}, 0), {})
        with self.assertRaisesRegex(RuntimeError, "menu_music_galactic"):
            vice.menu_music_fixture(digital, {}, 0)
        with self.assertRaisesRegex(RuntimeError, "title or victory"):
            vice.menu_music_fixture(digital, fields, 20)

    def test_catalog_menu_fixtures_keep_track_identity_separate_from_cues(self):
        digital = vice.package_model_layout(package(True, 0x534d0a00))
        fields = {"menu_music_track": {"type": "u8", "offset": 10}}
        self.assertEqual(vice.menu_music_fixture(digital, fields, 0), {"menu_music_track": 1})
        self.assertEqual(vice.menu_music_fixture(digital, fields, 5), {"menu_music_track": 0})
        self.assertEqual(fields["menu_music_track"]["size"], 1)
        for track in range(21):
            game = {"menu_music_track": track}
            cue = 5 if track == 0 else 0
            self.assertEqual(vice.menu_music_track(game), track)
            self.assertEqual(vice.menu_music_cue(game), cue)
            frame = {"music_cue": cue, "requested_menu_track": track}
            self.assertEqual(vice.check_menu_music(frame, cue, track=track), (cue, 0))
            with self.assertRaisesRegex(RuntimeError, "wrong catalog track"):
                vice.check_menu_music(frame, cue, track=(track + 1) % 21)
        for old, track in ((False, 1), (True, 0)):
            self.assertEqual(vice.menu_music_track({"menu_music_galactic": old}), track)
        with self.assertRaisesRegex(RuntimeError, "menu_music_track u8"):
            vice.menu_music_fixture(digital, {"menu_music_track": {"type": "bool"}}, 0)

    def test_menu_page_checks_allow_cue_handoff_and_loop_but_reject_restarts(self):
        def frame(cue, page, active=None):
            return {"music_cue": cue, "music_active_cue": cue if active is None else active,
                    "music_page": page}

        previous = vice.check_menu_music(frame(20, 2), 20)
        # The tick-100 reward handoff intentionally starts a new cue at page 0.
        previous = vice.check_menu_music(frame(5, 0), 5, previous)
        self.assertEqual(previous, (5, 0))
        for cue in (0, 5):
            previous = None
            for page in (0, 0, 1, 2, 3, 0, 1):
                previous = vice.check_menu_music(frame(cue, page), cue, previous, 4)
            for invalid in (frame(cue, 0), frame(cue, 3), frame(cue, 4)):
                with self.assertRaisesRegex(RuntimeError, "restarted|skipped|out-of-range"):
                    vice.check_menu_music(invalid, cue, previous, 4)
            for invalid in (frame(20, 1), frame(cue, 1, 20)):
                with self.assertRaisesRegex(RuntimeError, "wrong cue"):
                    vice.check_menu_music(invalid, cue, previous, 4)
        with self.assertRaisesRegex(RuntimeError, "restarted"):
            vice.check_menu_music(frame(5, 0), 5, (5, 1))

    def test_versions_and_boss_overlay_aliases(self):
        self.assertEqual(vice.package_model_layout(package(False))["count"], 12)
        self.assertEqual(vice.package_model_layout(package(True))["count"], 18)
        self.assertEqual([vice.boss_sim_profile(2, kind) for kind in range(6)],
                         [2, 4, 5, 6, 6, 5])

    def test_compact_package_relocations_fit_one_mib_and_leave_pixels_bank_exclusive(self):
        compact = vice.package_model_layout(package(True))
        self.assertEqual((compact["reu_size"], compact["music_offset"]), (0x100000, 0xc0000))
        self.assertEqual((compact["magic"], compact["descriptors"]), (b"SMA4", 88))
        offsets = compact["overlay_offsets"]
        self.assertEqual((offsets[12], offsets[13], offsets[23], offsets[30]),
                         (0x15000, 0x1a000, 0x5a000, 0x8a000))
        self.assertEqual(len(offsets), 24)
        self.assertEqual([page for page, offset in offsets.items() if offset >> 16 == 4], [4])
        self.assertEqual(offsets[4], 0x40000)
        reservations = [(0, 0x180), (0x1000, 0x1c00), (0x2000, 0xb000),
                        (0xb0000, 0xb9000), (compact["music_offset"], compact["music_offset"] + 0x40000)]
        reservations.extend((offset, offset + 0x4200) for offset in offsets.values())
        reservations.sort()
        for (start, end), (next_start, _) in zip(reservations, reservations[1:]):
            self.assertLess(start, end)
            self.assertLessEqual(end, next_start)
        self.assertEqual(reservations[-1][1], compact["reu_size"])

    def test_historical_package_layouts_preserve_music_and_overlay_locations(self):
        for version in range(3, 8):
            data = package(version >= 6, 0x534d0000 + (version << 8))
            if version < 5:
                data[16] = 4
            legacy = vice.package_model_layout(data)
            self.assertEqual(legacy["reu_size"], 16 * 1024 * 1024)
            self.assertEqual(legacy["music_offset"], 0x200000 if version >= 6 else 0x100000)
            self.assertTrue(all(offset == page * 0x10000
                                for page, offset in legacy["overlay_offsets"].items()))

    def test_package_size_validation_rejects_truncated_extended_and_wrong_capacity_images(self):
        for version, expected_size, wrong_size in ((0x534d0c00, 0x1000000, 0x800000),
                                                   (0x534d0b00, 0x1000000, 0x800000),
                                                   (0x534d0a00, 0x1000000, 0x800000),
                                                   (0x534d0900, 0x800000, 0x100000),
                                                   (0x534d0800, 0x100000, 0x1000000),
                                                   (0x534d0700, 0x1000000, 0x100000)):
            data = package(True, version)
            data.extend(bytes(expected_size - len(data)))
            self.assertEqual(vice.package_model_layout(data, validate_size=True)["reu_size"], expected_size)
            for invalid in (data[:-1], data + b"\0", data[:18] + bytes(wrong_size - 18)):
                with self.assertRaisesRegex(RuntimeError, "must be exactly"):
                    vice.package_model_layout(invalid, validate_size=True)
        for invalid in (b"", b"SM64", package(True)[:17]):
            with self.assertRaisesRegex(RuntimeError, "Expected supported package"):
                vice.package_model_layout(invalid, validate_size=True)

    def test_digital_package_keeps_compact_game_offsets_and_expands_reu(self):
        current = vice.package_model_layout(package(True, 0x534d0a00))
        mono = vice.package_model_layout(package(True, 0x534d0900))
        previous = vice.package_model_layout(package(True, 0x534d0800))
        self.assertTrue(current["digital"])
        self.assertFalse(previous["digital"])
        self.assertTrue(mono["digital"])
        self.assertEqual(mono["reu_size"], 0x800000)
        self.assertEqual(current["reu_size"], 0x1000000)
        self.assertEqual(current["music_offset"], 0xc0000)
        self.assertEqual(current["overlay_offsets"], previous["overlay_offsets"])
        self.assertEqual(current["overlay_offsets"], mono["overlay_offsets"])

    def test_outward_mesh_package_keeps_stereo_capacity_and_export_layout(self):
        current = vice.package_model_layout(package(True, 0x534d0b00))
        previous = vice.package_model_layout(package(True, 0x534d0a00))
        self.assertEqual(current["magic"], b"SMA5")
        self.assertEqual(previous["magic"], b"SMA4")
        self.assertEqual(current["version"], 0x534d0b00)
        for field in previous.keys() - {"version", "magic"}:
            self.assertEqual(current[field], previous[field], field)
        self.assertEqual(current["descriptors"], 88)
        self.assertEqual(current["reu_size"], 0x1000000)
        self.assertEqual(vice.results_restart_modes(current, "Victory"), ("Title", "Playing"))

    def test_shadow_package_adds_two_bounded_overlays_and_silhouette_metadata(self):
        current = vice.package_model_layout(package(True, 0x534d0c00))
        previous = vice.package_model_layout(package(True, 0x534d0b00))
        self.assertEqual(current["magic"], b"SMA6")
        self.assertEqual(current["descriptors"], 88)
        self.assertEqual(current["reu_size"], 0x1000000)
        self.assertEqual(current["overlay_offsets"], previous["overlay_offsets"] | {31: 0x95000, 32: 0xa5000, 33: 0xaa000})

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

    def test_encounter_fixtures_disable_or_rewind_both_spawn_streams(self):
        old_fields = layout(True)["fields"]
        fields = dict(old_fields, repeat_spawn_cursor={"offset": 3, "size": 1, "type": "u8"})
        for cursor in (0, 255):
            original = {"spawn_cursor": cursor}
            self.assertEqual(vice.clock_fixture(original, fields, 18),
                             dict(original, repeat_spawn_cursor=cursor))
            self.assertEqual(vice.clock_fixture(original, old_fields, 18), original)
            self.assertEqual(original, {"spawn_cursor": cursor})
            explicit = dict(original, repeat_spawn_cursor=17)
            self.assertEqual(vice.clock_fixture(explicit, fields, 18), explicit)
        for fixture in bench.fixtures(True, True, True):
            seeded = vice.clock_fixture(fixture["patch"], fields, 18)
            if "spawn_cursor" in seeded:
                self.assertEqual(seeded["repeat_spawn_cursor"], seeded["spawn_cursor"], fixture["name"])

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
        for fixture in expanded:
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
        self.assertEqual(checked[0]["frequency_writes"], 3)
        self.assertEqual(checked[0]["modulated_frequency_writes"], 0)
        self.assertEqual(checked[0]["verified_gate_attacks"], 3)
        # A detuned pitch under a gate attack is rejected...
        events[0]["value"] ^= 16
        with self.assertRaisesRegex(RuntimeError, "detuned"):
            vice.music_pitch_audit(events, registers, [(0, 0)], [set(notes)], 985248)
        # ...while a kernel's gate-less modulation write is merely counted.
        events[0]["value"] ^= 16
        swing = frequency(69) + 40
        events[3:3] = [{"address": 0xd400, "value": swing & 255},
                       {"address": 0xd401, "value": swing >> 8}]
        checked = vice.music_pitch_audit(events, registers, [(0, 0)], [set(notes)], 985248)
        self.assertEqual(checked[0]["frequency_writes"], 4)
        self.assertEqual(checked[0]["modulated_frequency_writes"], 1)
        self.assertEqual(checked[0]["verified_gate_attacks"], 3)
        constant = frequency(82)
        registers[0][0:2] = constant.to_bytes(2, "little")
        checked = vice.music_pitch_audit([{"address": 0xd404, "value": 0x81}],
                                        registers, [(0, 0)], [{82}], 985248)
        self.assertEqual(checked[0]["observed_notes"], [82])
        self.assertEqual(checked[0]["verified_gate_attacks"], 1)


if __name__ == "__main__":
    unittest.main()
