#!/usr/bin/env python3
"""Check capture composition on VIC hardware pixels, without starting VICE."""
import copy
import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location("vice_smoke", Path(__file__).with_name("vice-smoke.py"))
vice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vice)
spec = importlib.util.spec_from_file_location("bench_native", Path(__file__).with_name("bench-native.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


def state():
    registers = [0] * 47
    registers[0x11] = 0x3b
    registers[0x16] = 0x18
    registers[0x15] = 255
    registers[0x1c] = 255
    registers[0x26] = 1
    for sprite in range(8):
        x, y = ((3, 12), (133, 12), (3, 175), (133, 175))[sprite // 2]
        x = 24 + x * 2 + sprite % 2 * 24
        registers[sprite * 2] = x & 255
        registers[sprite * 2 + 1] = y + 50
        registers[0x10] |= (x >> 8) << sprite
        registers[0x27 + sprite] = 3
    return dict(format="vic-sprites-v1", origin=[24, 50], registers=registers,
                pointers=list(range(48, 56)), images=[bytes(64).hex()] * 8)


def multiplex_state():
    sprites = state()
    sprites["registers"][0x25] = 12
    instances = vice.sprite_instances(sprites)
    image = bytearray(64)
    image[0] = 0b10100000
    image[63] = 255  # Padding is never displayed, including in hires.
    instances.append(dict(sprite=0, enabled=True, x=172, y=140, pointer=255,
                          image=image.hex(), color=1, multicolor=False,
                          x_expand=False, y_expand=False, priority=False))
    sprites.update(format="vic-sprites-v2", instances=instances, phase=0, reticle_misses=0)
    return sprites


class SpriteCaptureTests(unittest.TestCase):
    def test_native_capture_uses_latched_state_without_advancing_emulation(self):
        syms = {name: 0x200 + index * 2 for index, name in enumerate((
            "HUD_SCROLL", "HUD_ENABLE_ACTIVE", "HUD_SPRITE0_COLOR", "RETICLE_ACTIVE_X",
            "RETICLE_ACTIVE_Y", "RETICLE_ACTIVE_VISIBLE", "SPRITE_PHASE", "RETICLE_MISSES"))}

        class Monitor:
            memory = bytearray(65536)

            def command(self, command):
                self.assert_bank(command)

            @staticmethod
            def assert_bank(command):
                if command not in ("bank io", "bank ram", "bank cpu"):
                    raise AssertionError(f"Capture advanced or changed emulation: {command}")

            def read(self, address, length):
                return bytes(self.memory[address:address + length])

            def integer(self, address, length=2):
                return int.from_bytes(self.read(address, length), "little")

        mon = Monitor()
        sprites = state()
        sprites["registers"][0x18] = 8
        mon.memory[0xd000:0xd02f] = bytes(sprites["registers"])
        mon.memory[0xdd00] = 1
        mon.memory[0x83f8:0x8400] = bytes([255, *range(49, 56)])
        mon.memory[0xd000:0xd002] = bytes([172, 140])
        mon.memory[0x8c00] = 0xc0
        mon.memory[0xbfc0] = 0xa0
        for name, value in (("HUD_SCROLL", 24), ("HUD_ENABLE_ACTIVE", 255), ("HUD_SPRITE0_COLOR", 3),
                            ("RETICLE_ACTIVE_X", 172), ("RETICLE_ACTIVE_Y", 140),
                            ("RETICLE_ACTIVE_VISIBLE", 1), ("SPRITE_PHASE", 1)):
            mon.memory[syms[name]] = value
        captured = vice.sprite_state(mon, syms)
        self.assertEqual(captured["format"], "vic-sprites-v2")
        self.assertEqual(captured["pointers"][0], 255)
        self.assertEqual(captured["instances"][0]["x"], 30)
        self.assertEqual(captured["instances"][0]["y"], 62)
        self.assertEqual(captured["instances"][0]["pointer"], 48)
        self.assertEqual(bytes.fromhex(captured["instances"][0]["image"])[0], 0xc0)
        self.assertEqual(captured["instances"][8]["x"], 172)
        self.assertEqual(bytes.fromhex(captured["instances"][8]["image"])[0], 0xa0)

    def test_multiplexed_slot_renders_both_hud_and_hires_reticle(self):
        sprites = multiplex_state()
        hud = bytearray(64)
        hud[0] = 0b11000000
        sprites["instances"][0]["image"] = hud.hex()
        pixels = vice.composite_pixels(bytes([5]) * 32000, sprites)
        self.assertEqual(pixels[12 * 320 + 6:12 * 320 + 8], bytes([1, 1]))
        self.assertEqual(pixels[90 * 320 + 148:90 * 320 + 152], bytes([1, 5, 1, 5]))
        self.assertEqual(pixels[111 * 320 + 148], 5)
        self.assertEqual(len(vice.sprite_images(sprites)), 9 * 64)

    def test_multiplex_visual_comparison_ignores_instantaneous_phase(self):
        original = multiplex_state()
        armed = copy.deepcopy(original)
        armed["phase"] = 1
        armed["registers"][0:2] = [172, 140]
        armed["registers"][0x1c] &= 254
        armed["registers"][0x27] = 1
        armed["pointers"][0] = 255
        armed["images"][0] = armed["instances"][8]["image"]
        self.assertEqual(vice.sprite_visual_state(original), vice.sprite_visual_state(armed))
        self.assertEqual(vice.sprite_images(original), vice.sprite_images(armed))
        self.assertEqual(vice.composite_pixels(bytes(32000), original),
                         vice.composite_pixels(bytes(32000), armed))
        vice.sprite_phase_check(original)
        vice.sprite_phase_check(armed)
        armed["instances"][8]["x"] += 1
        self.assertNotEqual(vice.sprite_visual_state(original), vice.sprite_visual_state(armed))
        with self.assertRaisesRegex(RuntimeError, "phase state"):
            vice.sprite_phase_check(armed)

    def test_hidden_reticle_preserves_bitmap_and_ignores_stale_art(self):
        sprites = multiplex_state()
        sprites["instances"][8]["enabled"] = False
        changed = copy.deepcopy(sprites)
        changed["instances"][8]["image"] = bytes([255] * 64).hex()
        self.assertEqual(vice.sprite_visual_state(sprites), vice.sprite_visual_state(changed))
        self.assertEqual(vice.composite_pixels(bytes([5]) * 32000, changed), bytes([5]) * 64000)

    def test_multiplex_shake_checks_canonical_hud_in_reticle_phase(self):
        sprites = multiplex_state()
        sprites["registers"][0x16] |= 1
        sprites["registers"][0x11] += 1
        sprites["registers"][0:2] = [173, 141]  # Slot zero is currently the reticle.
        for index, instance in enumerate(sprites["instances"]):
            instance["x"] += 1 if index == 8 else -1
            instance["y"] += 1 if index == 8 else -1
        self.assertEqual(vice.sprite_shake_alignment(sprites)["offset"], [1, 1])
        pixels = vice.composite_pixels(bytes([5]) * 32000, sprites)
        self.assertEqual(pixels[91 * 320 + 149:91 * 320 + 152], bytes([1, 5, 1]))

    def test_multicolor_transparency_shared_and_individual_colors(self):
        sprites = state()
        image = bytearray(64)
        image[0] = 0b00011011  # Transparent, black, cyan, white.
        image[63] = 255  # Padding byte must never become a visible row.
        sprites["images"][0] = image.hex()
        pixels = vice.composite_pixels(bytes([5]) * 32000, sprites)
        at = 12 * 320 + 6
        self.assertEqual(pixels[at:at + 8], bytes([5, 5, 0, 0, 3, 3, 1, 1]))
        self.assertEqual(pixels[33 * 320 + 6], 5)

    def test_odd_horizontal_shake_keeps_opposite_hud_motion_on_hardware_pixels(self):
        sprites = state()
        image = bytearray(64)
        image[0] = 0b11000000
        sprites["images"][0] = image.hex()
        sprites["registers"][0x16] |= 1
        sprites["registers"][0x11] = 0x3c
        for sprite in range(8):
            sprites["registers"][sprite * 2] -= 1
            sprites["registers"][sprite * 2 + 1] -= 1
        self.assertEqual(vice.sprite_shake_alignment(sprites)["offset"], [1, 1])
        source = bytes(x % 16 for y in range(200) for x in range(160))
        pixels = vice.composite_pixels(source, sprites)
        self.assertEqual(pixels[320:326], bytes([0, 0, 0, 1, 1, 2]))
        self.assertEqual(pixels[11 * 320 + 5:11 * 320 + 7], bytes([1, 1]))
        self.assertEqual(pixels[11 * 320 + 7], 3)
        self.assertEqual(pixels[13 * 320 + 7:13 * 320 + 9], bytes([3, 3]))

    def test_high_x_bit_and_sprite_priority(self):
        sprites = state()
        image = bytearray(64)
        image[0] = 0b10000000
        sprites["images"][2] = image.hex()
        pixels = vice.composite_pixels(bytes([5]) * 32000, sprites)
        self.assertEqual(pixels[12 * 320 + 266:12 * 320 + 268], bytes([3, 3]))
        sprites["images"][3] = image.hex()
        sprites["registers"][6:8] = sprites["registers"][4:6]
        sprites["registers"][0x2a] = 7
        self.assertEqual(vice.composite_pixels(bytes([5]) * 32000, sprites)[12 * 320 + 266], 3)

    def test_comparison_ignores_raster_and_hidden_image_bytes(self):
        original = state()
        original["registers"][0x15] = 3
        changed = copy.deepcopy(original)
        changed["registers"][0x12] = 100
        changed["registers"][0x29] = 7
        changed["images"][2] = bytes([255]) * 64
        self.assertEqual(vice.sprite_visual_state(original), vice.sprite_visual_state(changed))
        changed["images"][0] = bytes([255]) * 64
        self.assertNotEqual(vice.sprite_visual_state(original), vice.sprite_visual_state(changed))

    def test_alignment_checks_all_corners_and_reset(self):
        sprites = state()
        self.assertEqual(vice.sprite_shake_alignment(sprites)["offset"], [0, 0])
        for index in range(16):
            changed = copy.deepcopy(sprites)
            changed["registers"][index] += 1
            with self.subTest(register=index), self.assertRaisesRegex(RuntimeError, "shake"):
                vice.sprite_shake_alignment(changed)

    def test_alignment_accepts_opposite_scroll_and_rejects_same_direction(self):
        for dx in range(8):
            for y_scroll in range(8):
                dy = y_scroll - 3
                sprites = state()
                registers = sprites["registers"]
                registers[0x16] = 0x18 | dx
                registers[0x11] = 0x38 | y_scroll
                for sprite in range(8):
                    registers[sprite * 2] -= dx
                    registers[sprite * 2 + 1] -= dy
                with self.subTest(offset=(dx, dy)):
                    self.assertEqual(vice.sprite_shake_alignment(sprites)["offset"], [dx, dy])
                    if dx or dy:
                        for sprite in range(8):
                            registers[sprite * 2] += 2 * dx
                            registers[sprite * 2 + 1] += 2 * dy
                        with self.assertRaisesRegex(RuntimeError, "shake"):
                            vice.sprite_shake_alignment(sprites)

    def test_commit_timing_accepts_no_dirty_corners_and_guard_retry(self):
        symbols = dict(hud_publish_guard=0x4000, hud_publish_commit=0x4010, hud_publish_done=0x4050)
        self.assertEqual(bench.hud_commit_profile("", symbols, 63)["copies"], 0)
        trace = "".join(f"Trace exec ${address:04x}\n.C:{address:04x} NOP {cycle}\n"
                        for address, cycle in ((0x4000, 10), (0x4000, 30), (0x4010, 40), (0x4050, 120)))
        result = bench.hud_commit_profile(trace, symbols, 63)
        self.assertEqual((result["copies"], result["guard_retries"], result["max_protected_bus_cycles"]), (1, 1, 90))

    def test_commit_timing_rejects_slow_or_unguarded_copies(self):
        symbols = dict(hud_publish_guard=0x4000, hud_publish_commit=0x4010, hud_publish_done=0x4050)
        for events in (((0x4010, 40), (0x4050, 120)),
                       ((0x4000, 10), (0x4010, 40), (0x4050, 136))):
            trace = "".join(f"Trace exec ${address:04x}\n.C:{address:04x} NOP {cycle}\n" for address, cycle in events)
            with self.assertRaises(RuntimeError):
                bench.hud_commit_profile(trace, symbols, 63)


if __name__ == "__main__":
    unittest.main()
