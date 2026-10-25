#!/usr/bin/env python3
"""Host-side invariants for the offline C64 splash converter."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import numpy as np
from PIL import Image

SPEC = importlib.util.spec_from_file_location("splash", Path(__file__).with_name("encode-splash.py"))
splash = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(splash)


class SplashConverterTests(unittest.TestCase):
    def test_known_bitmap_layout_and_independent_decode(self):
        codes = np.tile(np.array([0, 1, 2, 3], dtype=np.uint8), (200, 40))
        palettes = np.tile(np.array([6, 1, 2, 3], dtype=np.uint8), (25, 40, 1))
        payload = splash.pack(codes, palettes)
        self.assertEqual(payload, b"\x1b" * 8000 + b"\x12" * 1000 + b"\x03" * 1000 + b"\x06")
        self.assertTrue(np.array_equal(splash.decode(payload), np.tile([6, 1, 2, 3], (200, 40))))

    def test_cell_major_addressing(self):
        codes = np.zeros((200, 160), dtype=np.uint8)
        palettes = np.tile(np.array([6, 1, 2, 3], dtype=np.uint8), (25, 40, 1))
        codes[9, 6] = 3
        payload = splash.pack(codes, palettes)
        expected = bytearray(8000)
        expected[(1 * 40 + 1) * 8 + 1] = 0x0c
        self.assertEqual(payload[:8000], expected)

    def test_every_exact_palette_color_stays_solid(self):
        for color in range(16):
            linear = np.tile(splash.LINEAR_PALETTE[color], (8, 4, 1))
            palettes = splash.choose_palettes(linear)
            codes = splash.dither(linear, palettes)
            self.assertTrue(np.all(palettes[0, 0, codes] == color), color)

    def test_preserves_four_legal_colors_across_sharp_edges(self):
        colors = np.tile([6, 0, 1, 14], (8, 1))
        linear = splash.LINEAR_PALETTE[colors]
        palettes = splash.choose_palettes(linear)
        actual = palettes[0, 0, splash.dither(linear, palettes)]
        self.assertTrue(np.array_equal(actual, colors))
        self.assertEqual(set(palettes[0, 0]), {6, 0, 1, 14})

    def test_gradient_uses_dither_to_preserve_mean_light(self):
        # Fixed black/white palette isolates diffusion from local palette choice.
        ramp = np.linspace(0.1, 0.85, 64)
        linear = np.repeat(np.tile(ramp, (64, 1))[:, :, None], 3, axis=2)
        palettes = np.tile(np.array([0, 1, 0, 1], dtype=np.uint8), (8, 16, 1))
        codes = splash.dither(linear, palettes)
        actual = splash.LINEAR_PALETTE[palettes[np.arange(64)[:, None] // 8,
                                               np.arange(64)[None, :] // 4, codes]]
        self.assertLess(abs(float(np.mean(actual)) - float(np.mean(linear))), 0.025)
        self.assertGreater(len(np.unique(codes)), 1)

    def test_conversion_is_deterministic_and_palette_legal(self):
        rng = np.random.default_rng(0xC64)
        linear = rng.random((16, 12, 3))
        palettes = splash.choose_palettes(linear)
        second_palettes = splash.choose_palettes(linear)
        self.assertTrue(np.array_equal(palettes, second_palettes))
        self.assertTrue(np.all(palettes[:, :, 0] == 6))
        self.assertTrue(np.all(palettes < 16))
        self.assertTrue(np.array_equal(splash.dither(linear, palettes), splash.dither(linear, second_palettes)))
        for row in palettes:
            for palette in row:
                self.assertEqual(len(set(palette)), 4)

    def test_committed_payload_decodes_and_has_blue_background(self):
        payload = (splash.OUTPUT / "havoc_splash.bin").read_bytes()
        self.assertEqual(len(payload), 10001)
        self.assertEqual(payload[-1], 6)
        pixels = splash.decode(payload)
        self.assertEqual(pixels.shape, (200, 160))
        self.assertTrue(np.all(pixels < 16))

    def test_previews_and_provenance_describe_committed_bytes(self):
        payload = (splash.OUTPUT / "havoc_splash.bin").read_bytes()
        settings = json.loads((splash.OUTPUT / "havoc_splash.json").read_text(encoding="utf-8"))
        self.assertEqual(settings["payload_sha256"], hashlib.sha256(payload).hexdigest())
        self.assertEqual(settings["source_sha256"], hashlib.sha256(splash.SOURCE.read_bytes()).hexdigest())
        rgb = splash.PALETTE[splash.decode(payload)]
        with Image.open(splash.OUTPUT / "havoc_splash.native.png") as preview:
            self.assertTrue(np.array_equal(np.asarray(preview), rgb))
        with Image.open(splash.OUTPUT / "havoc_splash.preview.png") as preview:
            self.assertEqual(preview.size, (640, 480))
            self.assertEqual(preview.tobytes(), Image.fromarray(rgb).resize((640, 480), Image.Resampling.NEAREST).tobytes())

    def test_rejects_invalid_hardware_payload(self):
        for payload in (bytes(10000), bytes(10000) + b"\x10",
                        bytes(9000) + b"\x10" + bytes(1000)):
            with self.assertRaises(ValueError):
                splash.decode(payload)

    def test_fast_manifest_check_rejects_stale_asset_and_converter(self):
        payload = (splash.OUTPUT / "havoc_splash.bin").read_bytes()
        settings = json.loads((splash.OUTPUT / "havoc_splash.json").read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            binary = out / "havoc_splash.bin"
            manifest = out / "havoc_splash.json"
            binary.write_bytes(bytes([payload[0] ^ 1]) + payload[1:])
            manifest.write_text(json.dumps(settings), encoding="utf-8")
            with self.assertRaisesRegex(SystemExit, "generated bitmap"):
                splash.verify_assets(splash.SOURCE, out)
            binary.write_bytes(payload)
            settings["generator"]["script_sha256"] = "0" * 64
            manifest.write_text(json.dumps(settings), encoding="utf-8")
            with self.assertRaisesRegex(SystemExit, "converter"):
                splash.verify_assets(splash.SOURCE, out)


if __name__ == "__main__":
    unittest.main()
