"""Keep archived parent-shadow and current suffix-shadow evidence comparable."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location(
    'comparison', Path(__file__).with_name('compare-shadow-benchmarks.py'))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)

spec = importlib.util.spec_from_file_location(
    'shadow_driver', Path(__file__).with_name('bench-shadows.py'))
driver = importlib.util.module_from_spec(spec)
# Load the driver's pure checks, stopping before its command-line/emulator setup.
with mock.patch('argparse.ArgumentParser.parse_args', side_effect=SystemExit):
    try:
        spec.loader.exec_module(driver)
    except SystemExit:
        pass


class ShadowEvidence(unittest.TestCase):
    def test_native_shadow_ink_matches_the_level_and_rejects_wrong_commands(self):
        for stage in range(18):
            color = 0 if stage == 16 else 11
            with self.subTest(stage=stage):
                self.assertEqual(driver.validate_shadow_ink(
                    stage, [{'reserved': 0x40, 'color': color}]), color)
                with self.assertRaises(AssertionError):
                    driver.validate_shadow_ink(
                        stage, [{'reserved': 0x40, 'color': 11 if color == 0 else 0}])
                with self.assertRaises(AssertionError):
                    driver.validate_shadow_ink(
                        stage, [{'reserved': 0, 'color': color}])

    def test_black_ordinary_geometry_is_not_mistaken_for_a_shadow(self):
        base = [{'reserved': 0x40, 'color': 0}, {'reserved': 0, 'color': 5}]
        old = [base[0], {'reserved': 0xc0, 'color': 11}, base[1]]
        new = base + [{'reserved': 0x40, 'color': 0}]
        self.assertEqual(comparison.ordinary(old, {}), base)
        self.assertEqual(comparison.ordinary(new, {
            'shadow_counts': {'base_count': 2, 'command_count': 1}}), base)

    def test_corrupt_suffix_count_is_rejected(self):
        with self.assertRaises(AssertionError):
            comparison.ordinary([{'reserved': 0}], {
                'shadow_counts': {'base_count': 1, 'command_count': 1}})

    def test_comparison_checks_exact_ordinary_order_in_both_formats(self):
        with tempfile.TemporaryDirectory() as folder:
            before, after = [Path(folder) / name for name in ('before', 'after')]
            base = [{'reserved': 0, 'color': 5}, {'reserved': 0x40, 'color': 0}]
            for directory, commands, counts, cycles in (
                (before, [base[0], {'reserved': 0x80, 'color': 11}, base[1]], {}, 200),
                (after, base + [{'reserved': 0x40, 'color': 0}],
                 {'base_count': 2, 'command_count': 1}, 120),
            ):
                capture = directory / 'scene' / 'retained' / '0'
                capture.mkdir(parents=True)
                (capture / 'geometry.json').write_text(json.dumps(commands))
                sample = {'cia_phase': 0, 'shadow_counts': counts,
                          'counters': {'frame_dropped': 0},
                          'base_triangle_count': 2, 'shadow_triangle_count': 1}
                (directory / 'report.json').write_text(json.dumps({
                    'passed': True, 'video': 'pal', 'hud_checks': {'cases': [{
                        'fixture': 'scene', 'palette': 'retained',
                        'median_cycles': cycles, 'samples': [sample]}]}}))
            report = comparison.compare(before, after)
            self.assertTrue(report['ordinary_geometry_identical'])
            self.assertAlmostEqual(report['summed_change_percent'], -40)
            (after / 'scene/retained/0/geometry.json').write_text(json.dumps(
                list(reversed(base)) + [{'reserved': 0x40, 'color': 0}]))
            with self.assertRaisesRegex(AssertionError, 'ordinary geometry changed'):
                comparison.compare(before, after)

    def test_color_change_cannot_hide_different_shadow_geometry(self):
        with tempfile.TemporaryDirectory() as folder:
            before, after = [Path(folder) / name for name in ('before', 'after')]
            for directory, color in ((before, 0), (after, 5)):
                capture = directory / 'scene/retained/0'
                capture.mkdir(parents=True)
                (capture / 'geometry.json').write_text(json.dumps([
                    {'reserved': 0, 'color': 5, 'x': [0, 2, 4]},
                    {'reserved': 0x40, 'color': color, 'x': [1, 3, 5]}]))
                sample = {'cia_phase': 0, 'shadow_counts': {'base_count': 1, 'command_count': 1},
                          'counters': {'frame_dropped': 0},
                          'base_triangle_count': 1, 'shadow_triangle_count': 1}
                (directory / 'report.json').write_text(json.dumps({
                    'passed': True, 'video': 'pal', 'hud_checks': {'cases': [{
                        'fixture': 'scene', 'palette': 'retained',
                        'median_cycles': 100, 'samples': [sample]}]}}))
            self.assertTrue(comparison.compare(before, after, True)['shadow_geometry_identical'])
            path = after / 'scene/retained/0/geometry.json'
            commands = json.loads(path.read_text())
            commands[1]['x'][0] += 1
            path.write_text(json.dumps(commands))
            with self.assertRaisesRegex(AssertionError, 'shadow geometry changed'):
                comparison.compare(before, after, True)


if __name__ == '__main__':
    unittest.main()
