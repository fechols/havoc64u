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


class SpriteCaptureTests(unittest.TestCase):
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
