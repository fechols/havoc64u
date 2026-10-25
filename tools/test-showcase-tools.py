#!/usr/bin/env python3
"""Check the showcase controller without running or changing the native game."""
import copy
import importlib.util
import math
from pathlib import Path
import tempfile
import unittest
import wave


spec = importlib.util.spec_from_file_location("showcase", Path(__file__).with_name("capture-showcase.py"))
showcase = importlib.util.module_from_spec(spec)
spec.loader.exec_module(showcase)
spec = importlib.util.spec_from_file_location("encode", Path(__file__).with_name("encode-showcase.py"))
encode = importlib.util.module_from_spec(spec)
spec.loader.exec_module(encode)


def state():
    return dict(stage_id=0, ticks=100, mode=1, player=dict(x=0, y=0, z=180),
                flight=dict(frac=[0, 0], vel=[0, 0]), actors=[], shots=[], pickups=[],
                boss_started=0, boss_defeated=0, roll_ticks=0, roll_direction=0)


class ShowcasePilot(unittest.TestCase):
    def setUp(self):
        self.package = bytearray(0xb0800)
        self.package[0xb0000 + 88 + 3] = 20  # Fighter's authored ram radius.

    def test_encoder_reads_legacy_and_hires_capture_widths(self):
        with tempfile.TemporaryDirectory() as directory:
            for width in (160, 320):
                rows = [bytes((x + y) & 255 for x in range(width * 3)) for y in range(200)]
                path = Path(directory) / f"{width}.png"
                showcase.vice.png(path, rows, width, 200)
                self.assertEqual(encode.native_rgb(path), b"".join(rows))

    def test_showcase_preserves_single_hardware_pixel_sprite_detail(self):
        registers = [0] * 47
        registers[0x11], registers[0x16] = 0x3b, 0x18
        registers[0x15], registers[0x27] = 1, 1
        registers[0], registers[1] = 24, 50
        image = bytes([0b01000000]) + bytes(63)
        sprites = dict(format="vic-sprites-v1", origin=[24, 50], registers=registers,
                       pointers=[48] * 8, images=[image.hex()] + [bytes(64).hex()] * 7)
        rows = showcase.pixels(bytes(10000), 0, sprites)
        self.assertEqual(len(rows[0]), 320 * 3)
        self.assertEqual(rows[0][:9], bytes(showcase.vice.PALETTE[0]) +
                         bytes(showcase.vice.PALETTE[1]) + bytes(showcase.vice.PALETTE[0]))

    def test_brakes_for_inertia_and_holds_fire_without_periodic_roll_taps(self):
        game = state()
        self.assertEqual(showcase.showcase_input(game, self.package), 16)
        game["flight"]["vel"] = [768, 0]
        self.assertEqual(showcase.showcase_input(game, self.package), 16 | 4)
        for tick in (101, 102, 112, 200):
            game["ticks"] = tick
            self.assertEqual(showcase.showcase_input(game, self.package), 16 | 4)

    def test_avoids_an_ordinary_fighter_ram_without_modifying_state(self):
        game = state()
        game["actors"] = [dict(kind=1, hp=4, age=100, flags=0, approach_speed=8,
                              origin_x=0, origin_y=0, pos=dict(x=0, y=0, z=500))]
        original = copy.deepcopy(game)
        bits = showcase.showcase_input(game, self.package)
        self.assertEqual(bits & 16, 16)
        self.assertNotEqual(bits & 15, 0, "the aiming target is also a collision threat")
        self.assertEqual(game, original)

    def test_evades_incoming_fire_but_ignores_own_shots_and_retired_actors(self):
        game = state()
        game["shots"] = [dict(kind=2, dx=0, dy=0, pos=dict(x=0, y=0, z=540))]
        self.assertNotEqual(showcase.showcase_input(game, self.package) & 15, 0)
        game["shots"][0]["kind"] = 1
        game["actors"] = [dict(kind=1, hp=0, pos=dict(x=0, y=0, z=300))]
        self.assertEqual(showcase.showcase_input(game, self.package), 16)


class SmoothShowcasePilot(unittest.TestCase):
    def setUp(self):
        self.package = bytearray(0xb0800)
        self.package[0xb0000 + 88 + 3] = 20
        self.controller = showcase.SmoothPilot()

    def input(self, game, target=(0, 0)):
        return showcase.showcase_input(game, self.package, target=target, controller=self.controller)

    def test_scenic_target_does_not_chase_distant_enemies(self):
        game = state()
        for tick, x in ((100, -90), (105, 90), (110, -90)):
            game["ticks"] = tick
            game["actors"] = [dict(kind=1, hp=4, age=tick, flags=0, approach_speed=8,
                                  origin_x=x, origin_y=0, pos=dict(x=x, y=0, z=2000))]
            original = copy.deepcopy(game)
            self.assertEqual(self.input(game), 16)
            self.assertEqual(game, original)

    def test_keeps_safe_destination_then_accepts_a_new_scenic_target(self):
        game = state()
        self.input(game, (-40, 0))
        game["ticks"] = 110
        self.input(game, (40, 0))
        self.assertEqual(self.controller.destination, (-40, 0))
        game["ticks"] = 151
        self.input(game, (40, 0))
        self.assertEqual(self.controller.destination, (40, 0))

    def test_incoming_fire_overrides_destination_hold_immediately(self):
        game = state()
        self.assertEqual(self.input(game), 16)
        game["ticks"] = 105
        game["shots"] = [dict(kind=2, dx=0, dy=0, pos=dict(x=0, y=0, z=540))]
        original = copy.deepcopy(game)
        bits = self.input(game)
        self.assertNotEqual(self.controller.destination, (0, 0))
        self.assertNotEqual(bits & 15, 0)
        self.assertEqual(bits & 16, 16)
        self.assertEqual(game, original)

    def test_anticipates_close_lowland_volley_before_a_bullet_is_visible(self):
        game = state()
        game["stage_info"] = dict(enemy_profile=0)
        game["actors"] = [dict(kind=3, hp=6, age=64, flags=0, approach_speed=8,
                              origin_x=80, origin_y=0, pos=dict(x=80, y=0, z=500))]
        original = copy.deepcopy(game)
        self.assertNotEqual(self.input(game) & 15, 0)
        self.assertEqual(game, original)

    def test_small_stopping_error_does_not_keep_rocking_the_hull(self):
        game = state()
        for tick, x in ((100, -6), (105, 6), (110, -4), (115, 4)):
            game["ticks"], game["player"]["x"] = tick, x
            self.assertEqual(self.input(game), 16)
        game["flight"]["vel"] = [768, 0]
        self.assertEqual(self.input(game), 16 | 4, "substantial momentum must still be braked")

    def test_brakes_for_input_already_queued_during_the_rendered_frame(self):
        game = state()
        game["player"]["x"] = -10
        self.controller.commands = [1, 0]
        previous = copy.deepcopy(game)
        previous["ticks"] -= 7
        bits = showcase.showcase_input(game, self.package, previous=previous, target=(10, 0),
                                       controller=self.controller)
        self.assertEqual(bits & 12, 0, "coast before the pending right input would overshoot")
        self.assertEqual(self.controller.input_delay_ticks, 7)

    def test_horizontal_represses_cannot_become_roll_gestures_across_clock_wrap(self):
        presses, held, edges = {-1: None, 1: None}, 0, {-1: [], 1: []}
        for elapsed in range(100):
            tick = (65520 + elapsed) & 0xffff
            requested = (1, 0, 1, -1, 0, -1)[elapsed % 6]
            command = self.controller.horizontal(requested, held, presses, tick)
            if command and command != held:
                edges[command].append(elapsed)
            held = command
        for direction in (-1, 1):
            self.assertGreaterEqual(len(edges[direction]), 3)
            self.assertTrue(all(b - a >= 27 for a, b in zip(edges[direction], edges[direction][1:])))


class ShowcasePreset(unittest.TestCase):
    def test_readme_teaser_order_duration_and_gallery_override(self):
        scenes = showcase.selected_scenes("readme-teaser")
        self.assertEqual([s["name"] for s in scenes], [
            "title", "lowlands", "crystal-cave", "iron-canyon", "foundry-belt", "storm-front",
            "orbital-ascent", "asteroid-belt", "shattered-moon", "space-carrier", "title-outro"])
        self.assertTrue(math.isclose(sum(s["seconds"] for s in scenes), 91.2))
        self.assertTrue(all(s["smooth_flight"] and not s["gallery"] for s in scenes))
        self.assertEqual(next(s for s in scenes if s["name"] == "orbital-ascent")["seconds"], 10)
        self.assertEqual(next(s for s in scenes if s["name"] == "lowlands")["local"], 0)
        self.assertTrue(next(s for s in showcase.SCENES if s["name"] == "iron-canyon")["gallery"])

    def test_subset_preserves_preset_order_and_rejects_missing_scene(self):
        self.assertEqual([s["name"] for s in showcase.selected_scenes(
            "readme-teaser", ["space-carrier", "iron-canyon"])], ["iron-canyon", "space-carrier"])
        with self.assertRaisesRegex(ValueError, "ship-graveyard"):
            showcase.selected_scenes("readme-teaser", ["ship-graveyard"])
        self.assertEqual(showcase.selected_scenes(), list(showcase.SCENES))


class ShowcaseAudio(unittest.TestCase):
    def test_silent_cinematics_are_allowed_but_lost_combat_effects_are_rejected(self):
        silent = dict(retained_channels=[dict(rms=0), dict(rms=0)])
        panned = dict(retained_channels=[dict(rms=10), dict(rms=0)])
        quiet = dict(left_d400=dict(gate_on=0), right_d420=dict(gate_on=0), source="sid-gates.log")
        firing = dict(left_d400=dict(gate_on=3), right_d420=dict(gate_on=0), source="sid-gates.log")
        showcase.validate_sfx_activity(silent, quiet, expect_effects=False)
        showcase.validate_sfx_activity(panned, firing, expect_effects=True)
        with self.assertRaisesRegex(RuntimeError, "no retained SID effect gates"):
            showcase.validate_sfx_activity(silent, quiet, expect_effects=True)
        for combat in (False, True):
            with self.assertRaisesRegex(RuntimeError, "no audible retained waveform"):
                showcase.validate_sfx_activity(silent, firing, expect_effects=combat)

    def test_silent_effects_are_explicit_and_still_checked_for_format_and_clipping(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "effects.wav"

            def write(samples, channels=2):
                with wave.open(str(path), "wb") as stream:
                    stream.setparams((channels, 2, 44100, 0, "NONE", "not compressed"))
                    stream.writeframes(samples * 4410)

            write(b"\x00\x00\x00\x00")
            with self.assertRaisesRegex(RuntimeError, "silent"):
                showcase.wav_metrics(path, 0, .1)
            result = showcase.wav_metrics(path, 0, .1, allow_silence=True)
            self.assertTrue(result["silence_allowed"])
            self.assertTrue(all(channel["silent"] for channel in result["retained_channels"]))
            with self.assertRaisesRegex(RuntimeError, "Incomplete WAV"):
                showcase.wav_metrics(path, 0, .2, allow_silence=True)
            write(b"\xff\x7f\x00\x00")
            with self.assertRaisesRegex(RuntimeError, "Clipped"):
                showcase.wav_metrics(path, 0, .1, allow_silence=True)
            write(b"\x00\x00", channels=1)
            with self.assertRaisesRegex(RuntimeError, "Unexpected WAV format"):
                showcase.wav_metrics(path, 0, .1, allow_silence=True)


if __name__ == "__main__":
    unittest.main()
