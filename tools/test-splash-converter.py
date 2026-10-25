#!/usr/bin/env python3
"""Host-side invariants for the offline C64 splash converter."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import numpy as np
from PIL import Image

SPEC = importlib.util.spec_from_file_location("splash", Path(__file__).with_name("encode-splash.py"))
splash = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(splash)


def reference_filter(rgb, sigma, radius):
    """Direct 2-D convolution, independent of the optimizer's separable cache."""
    height, width = rgb.shape[:2]
    result = np.empty_like(rgb, dtype=np.float64)
    for y in range(height):
        for x in range(width):
            total = np.zeros(3, dtype=np.float64)
            weight_sum = 0.0
            for dy in range(-radius[1], radius[1] + 1):
                for dx in range(-radius[0], radius[0] + 1):
                    if 0 <= y + dy < height and 0 <= x + dx < width:
                        weight = np.exp(-0.5 * ((dx / sigma[0]) ** 2 + (dy / sigma[1]) ** 2))
                        total += weight * rgb[y + dy, x + dx]
                        weight_sum += weight
            result[y, x] = total / weight_sum
    return result


def reference_score(linear, indices):
    """Recompute all three errors from scratch, with no optimizer helpers."""
    output = splash.LINEAR_PALETTE[indices]
    lab = splash.linear_to_oklab(linear)
    weights = splash.edge_weights(lab)
    raw = float(np.sum(np.sum((lab - splash.LAB_PALETTE[indices]) ** 2 * splash.METRIC,
                              axis=2) * weights) / np.sum(weights))
    terms = []
    for sigma, radius in (((0.6, 1.0), (1, 2)), ((1.2, 2.0), (2, 4))):
        source_lab = splash.linear_to_oklab(reference_filter(linear, sigma, radius))
        output_lab = splash.linear_to_oklab(reference_filter(output, sigma, radius))
        terms.append(float(np.mean(np.sum((source_lab - output_lab) ** 2 * splash.METRIC, axis=2))))
    return {"raw": raw, "fine": terms[0], "coarse": terms[1],
            "total": 0.25 * raw + 0.5 * terms[0] + 0.25 * terms[1]}


def spatial_objective(linear):
    return splash.spatial.SpatialObjective(linear, splash.LINEAR_PALETTE, splash.LAB_PALETTE,
                                          splash.METRIC, splash.edge_weights, splash.linear_to_oklab)


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

    def test_fast_manifest_check_respects_explicit_method(self):
        settings = json.loads((splash.OUTPUT / "havoc_splash.json").read_text(encoding="utf-8"))
        other = "legacy" if settings["method"] == "spatial" else "spatial"
        with self.assertRaisesRegex(SystemExit, "conversion method"):
            splash.verify_assets(splash.SOURCE, splash.OUTPUT, other)

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
            settings["generator"]["script_sha256"] = splash.script_sha256()
            settings["generator"]["support_files"] = {"tools/splash_spatial.py": "0" * 64}
            manifest.write_text(json.dumps(settings), encoding="utf-8")
            with self.assertRaisesRegex(SystemExit, "spatial converter"):
                splash.verify_assets(splash.SOURCE, out)


class SpatialSplashTests(unittest.TestCase):
    def assert_legal(self, indices, background=6):
        self.assertTrue(np.all((indices >= 0) & (indices < 16)))
        for y in range(0, indices.shape[0], 8):
            for x in range(0, indices.shape[1], 4):
                colors = set(int(c) for c in indices[y:y + 8, x:x + 4].flat)
                self.assertLessEqual(len(colors - {background}), 3, (y, x, colors))

    def test_filters_match_direct_convolution_at_boundaries(self):
        rng = np.random.default_rng(64)
        # Smaller than the coarse kernel; every sample exercises truncation.
        rgb = rng.random((3, 4, 3))
        for sigma, radius in (((0.6, 1.0), (1, 2)), ((1.2, 2.0), (2, 4))):
            actual = splash.spatial.filter_linear(rgb, sigma, radius)
            np.testing.assert_allclose(actual, reference_filter(rgb, sigma, radius), atol=2e-15)
            constant = np.tile([0.13, 0.57, 0.91], (3, 4, 1))
            np.testing.assert_allclose(splash.spatial.filter_linear(constant, sigma, radius),
                                       constant, atol=2e-15)

    def test_cached_scores_match_full_recomputation_for_recolors_swaps_and_groups(self):
        rng = np.random.default_rng(0xC64)
        linear = rng.random((8, 8, 3))
        indices = rng.choice(np.array([0, 1, 6, 14], dtype=np.uint8), (8, 8))
        objective = spatial_objective(linear)
        state = splash.spatial.SpatialState(objective, indices)
        groups = [np.array([[0, 0]]), np.array([[7, 7]]),
                  np.array([[3, 3], [3, 4]]), np.array([[0, 7], [1, 7]]),
                  np.argwhere(indices[:, :4] == 14)]
        accepted = 0
        for positions in groups:
            if not len(positions):
                continue
            old = state.indices.copy()
            before = reference_score(linear, old)
            candidates = np.repeat(np.array([0, 1, 6, 14], dtype=np.uint8)[:, None],
                                   len(positions), axis=1)
            if len(positions) == 2:
                swapped = old[positions[::-1, 0], positions[::-1, 1]]
                candidates = np.vstack((candidates, swapped))
            expected = []
            for candidate in candidates:
                trial = old.copy()
                trial[positions[:, 0], positions[:, 1]] = candidate
                expected.append(reference_score(linear, trial)["total"] - before["total"])
            np.testing.assert_allclose(state.delta_scores(positions, candidates), expected,
                                       rtol=1e-11, atol=2e-15)
            best = int(np.argmin(expected))
            changed = state.try_changes(positions, candidates)
            self.assertEqual(changed, expected[best] < -1e-12)
            wanted = old.copy()
            if changed:
                accepted += 1
                wanted[positions[:, 0], positions[:, 1]] = candidates[best]
            np.testing.assert_array_equal(state.indices, wanted)
            full = reference_score(linear, state.indices)
            for name in full:
                self.assertAlmostEqual(state.score[name], full[name], places=13)
                self.assertAlmostEqual(objective.score(state.indices)[name], full[name], places=13)
            self.assertLessEqual(full["total"], before["total"] + 2e-15)
        self.assertGreater(accepted, 0)

    def test_optimizer_never_proposes_illegal_cross_cell_moves(self):
        # Both sides use all three slots, so naively swapping the boundary
        # pixels would introduce a fourth non-background color in each cell.
        indices = np.tile(np.array([0, 1, 2, 0, 3, 4, 5, 3], dtype=np.uint8), (8, 1))
        wanted = indices.copy()
        wanted[3, 3], wanted[3, 4] = wanted[3, 4], wanted[3, 3]
        objective = spatial_objective(splash.LINEAR_PALETTE[wanted])
        # Audit the hot-loop entry as well as the final output: an illegal
        # intermediate palette might otherwise be hidden by a later move.
        original_try = splash.spatial.SpatialState._try
        proposals = []

        def audit(state, positions, candidates):
            positions = np.asarray(positions)
            for candidate in candidates:
                trial = state.indices.copy()
                trial[positions[:, 0], positions[:, 1]] = candidate
                self.assert_legal(trial)
            proposals.append(len(candidates))
            return original_try(state, positions, candidates)

        with patch.object(splash.spatial.SpatialState, "_try", audit):
            result = splash.spatial.refine(objective, indices)
        self.assertTrue(proposals)
        self.assert_legal(result)
        self.assertLessEqual(objective.score(result)["total"], objective.score(indices)["total"])

    def test_palette_replacement_to_black_does_not_accumulate_cancellation_error(self):
        linear = np.zeros((8, 4, 3), dtype=np.float64)
        objective = spatial_objective(linear)
        state = splash.spatial.SpatialState(objective, np.ones((8, 4), dtype=np.uint8))
        positions = np.argwhere(np.ones((8, 4), dtype=bool))
        black = np.zeros((1, len(positions)), dtype=np.uint8)
        self.assertTrue(state.try_changes(positions, black))
        np.testing.assert_array_equal(state.indices, np.zeros((8, 4), dtype=np.uint8))
        # Cube roots near zero must not amplify filtered-RGB cancellation into
        # a meaningful cached loss for an exactly black reconstructed image.
        self.assertLess(state.score["total"], 1e-14)
        self.assertEqual(objective.score(state.indices)["total"], 0.0)
        np.testing.assert_allclose(state.delta_scores(positions, black), [0.0], atol=1e-14)
        self.assertFalse(state.try_changes(positions, black))

    def test_unused_palette_capacity_can_introduce_a_new_color(self):
        indices = np.zeros((8, 4), dtype=np.uint8)
        target = indices.copy()
        target[3, 2] = 1
        result = splash.spatial.refine(spatial_objective(splash.LINEAR_PALETTE[target]), indices)
        np.testing.assert_array_equal(result, target)

    def test_exact_colors_edges_and_thin_details_survive_spatial_conversion(self):
        for color in range(16):
            indices = np.full((8, 4), color, dtype=np.uint8)
            result = splash.spatial_indices(splash.LINEAR_PALETTE[indices])
            np.testing.assert_array_equal(result, indices)
        indices = np.tile(np.array([6, 0, 1, 14], dtype=np.uint8), (8, 1))
        np.testing.assert_array_equal(splash.spatial_indices(splash.LINEAR_PALETTE[indices]), indices)
        thin = np.zeros((16, 12), dtype=np.uint8)
        thin[:, 3] = 1
        thin[7, :] = 14
        thin[0, 0] = 6
        np.testing.assert_array_equal(splash.spatial_indices(splash.LINEAR_PALETTE[thin]), thin)

    def test_spatial_conversion_is_deterministic_legal_and_improves_seed(self):
        rng = np.random.default_rng(7)
        linear = rng.random((8, 8, 3))
        palettes = splash.choose_palettes(linear)
        codes = splash.dither(linear, palettes)
        legacy = palettes[np.arange(8)[:, None] // 8, np.arange(8)[None, :] // 4, codes]
        first = splash.spatial_indices(linear, palettes)
        second = splash.spatial_indices(linear, palettes)
        np.testing.assert_array_equal(first, second)
        self.assert_legal(first)
        objective = spatial_objective(linear)
        self.assertLess(objective.score(first)["total"], objective.score(legacy)["total"])

    def test_spatial_gradient_preserves_mean_light(self):
        ramp = np.linspace(0.05, 0.85, 16)
        linear = np.repeat(np.tile(ramp, (16, 1))[:, :, None], 3, axis=2)
        indices = splash.spatial_indices(linear)
        self.assert_legal(indices)
        self.assertLess(abs(float(np.mean(splash.LINEAR_PALETTE[indices])) - float(np.mean(linear))), 0.025)

    def test_canonical_palette_encoding_reuses_unused_slots(self):
        indices = np.full((16, 8), 6, dtype=np.uint8)
        indices[:8, :4] = 14
        indices[8:, 4:] = np.tile([6, 1, 2, 3], (8, 1))
        codes, palettes = splash.indices_to_codes(indices)
        reconstructed = palettes[np.arange(16)[:, None] // 8, np.arange(8)[None, :] // 4, codes]
        np.testing.assert_array_equal(reconstructed, indices)
        np.testing.assert_array_equal(palettes[0, 1], [6, 0, 1, 2])
        for row in palettes:
            for palette in row:
                self.assertEqual(palette[0], 6)
                self.assertEqual(len(set(palette)), 4)
                self.assertEqual(list(palette[1:]), sorted(palette[1:]))


if __name__ == "__main__":
    unittest.main()
