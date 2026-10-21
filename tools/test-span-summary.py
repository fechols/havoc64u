#!/usr/bin/env python3
"""Small, deterministic regression tests for the benchmark evidence gates."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


TOOL = Path(__file__).with_name("summarize-span-benchmarks.py")
SPEC = importlib.util.spec_from_file_location("span_summary", TOOL)
summary = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(summary)


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


class SummaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="havoc-span-summary-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        costs = {"baseline": (1000, 2000), "candidate": (900, 1800),
                 "original": (1000, 2000), "corrected": (1010, 2020),
                 "unoptimized": (1050, 2100), "unhoisted": (920, 1840),
                 "spans": (900, 1800), "front": (800, 1600)}
        for name, cycles in costs.items():
            for profile in summary.HISTORICAL_PROFILES:
                package = self.root / f"{name}-{profile}"
                package.mkdir()
                hashes = {}
                for suffix in ("prg", "reu", "elf"):
                    data = f"synthetic {name} {profile} {suffix}".encode()
                    (package / f"havoc.{suffix}").write_bytes(data)
                    hashes[suffix] = hashlib.sha256(data).hexdigest()
                for video in summary.VIDEOS:
                    cases = []
                    for fixture, ticks, palette, cost in (("title", 0, "cold", cycles[0]),
                                                          ("play", 10, "retained", cycles[1])):
                        samples = []
                        for phase in range(3):
                            samples.append({
                                "cia_phase": phase,
                                "initial": {"game_sha256": digest(f"{fixture} initial"),
                                            "first_audio_period_cycles": (20000, 13333, 6666)[phase]},
                                "raw_geometry_sha256": digest(f"{fixture} geometry"),
                                "game_sha256": digest(f"{fixture} game"),
                                "native_pixels_sha256": digest(f"{fixture} pixels"),
                                # Palette slot ordering may differ without changing decoded pixels.
                                "native_sha256": digest(f"{fixture} {name} bytes"),
                                "bus_cycles_delta": cost + phase - 1,
                                "phase_work_bus_cycles": {"simulation": 50, "geometry": 100,
                                                          "pixels": cost // 2, "audio": 20, "scenery": 30},
                                "pixel_bus_cycles_detail": {"raster": cost // 4, "hud": 50,
                                                            "pack": 20, "copy": 10, "wait": 5},
                                "max_audio_lateness_ms": 1.0,
                            })
                        cases.append({"fixture": fixture, "ticks": ticks, "palette": palette, "samples": samples})
                    report = {"passed": True, "video": video, "sid_count": int(profile[0]),
                              "hashes": hashes, "protocol": "havoc-fixed-frame-v1", "samples": 3,
                              "bus_hz": 1_000_000, "real_time": {"fps": 12.0}, "cases": cases}
                    for field in ("fixture_sha256", "vice_sha256", "driver_sha256", "smoke_tool_sha256"):
                        report[field] = digest(field)
                    self.save(report, name, profile, video)

    def path(self, name="candidate", profile="4sid", video="pal"):
        return self.root / f"{name}-{profile}-{video}" / "report.json"

    def load(self, name="candidate", profile="4sid", video="pal"):
        return json.loads(self.path(name, profile, video).read_text())

    def save(self, report, name="candidate", profile="4sid", video="pal"):
        path = self.path(name, profile, video)
        path.parent.mkdir(exist_ok=True)
        path.write_text(json.dumps(report))

    def compare(self):
        return summary.compare(self.root, "4sid", "pal")

    def matrix(self, profiles=summary.PROFILES):
        return [summary.compare(self.root, profile, video, baseline, candidate)
                for baseline, candidate in summary.MATRIX_PAIRS
                for profile in profiles for video in summary.VIDEOS]

    def run_cli(self, *arguments):
        return subprocess.run([sys.executable, str(TOOL), str(self.root), *arguments],
                              capture_output=True, text=True)

    def test_matching_evidence_and_decoded_pixel_equality_win(self):
        result = self.compare()
        self.assertTrue(result["exact_geometry_state_and_pixels"])
        self.assertTrue(result["performance_win"])
        self.assertEqual((result["case_count"], result["sample_pairs"]), (2, 6))
        self.assertTrue(all(not case["native_bytes_equal"] for case in result["cases"]))

    def test_package_hashes_are_complete_valid_and_match_actual_files(self):
        good = self.load()
        for hashes in ({}, {"prg": good["hashes"]["prg"]}, {**good["hashes"], "elf": ""},
                       {**good["hashes"], "reu": digest("wrong")}):
            with self.subTest(hashes=hashes):
                report = copy.deepcopy(good)
                report["hashes"] = hashes
                self.save(report)
                with self.assertRaises(AssertionError):
                    self.compare()
        self.save(good)
        (self.root / "candidate-4sid/havoc.prg").write_bytes(b"changed after recording")
        with self.assertRaisesRegex(AssertionError, "Changed package"):
            self.compare()

    def test_protocol_tool_fixture_clock_and_sample_metadata_must_match(self):
        good = self.load()
        for field in ("protocol", "fixture_sha256", "vice_sha256", "driver_sha256", "smoke_tool_sha256",
                      "samples", "bus_hz", "video", "sid_count"):
            with self.subTest(field=field):
                report = copy.deepcopy(good)
                report[field] = (digest("different") if field.endswith("sha256") else
                                 report[field] + 1 if isinstance(report[field], int) else "different")
                self.save(report)
                with self.assertRaises(AssertionError):
                    self.compare()

    def test_cross_sid_reports_cannot_substitute_for_selected_profile(self):
        for profile, other in (("4sid", "2sid"), ("2sid", "4sid")):
            with self.subTest(profile=profile):
                original = self.load("candidate", profile)
                self.save(self.load("candidate", other), "candidate", profile)
                with self.assertRaises(AssertionError):
                    summary.compare(self.root, profile, "pal")
                self.save(original, "candidate", profile)

    def test_failed_truthy_or_incomplete_reports_are_rejected(self):
        good = self.load()
        for passed in (False, "false", 1, None):
            with self.subTest(passed=passed):
                report = copy.deepcopy(good)
                report["passed"] = passed
                self.save(report)
                with self.assertRaisesRegex(AssertionError, "Incomplete or failed"):
                    self.compare()

    def test_cia_phase_pairing_and_sequence_are_required(self):
        report = self.load()
        report["cases"][0]["samples"][1]["cia_phase"] = 0
        self.save(report)
        with self.assertRaisesRegex(AssertionError, "CIA phase"):
            self.compare()
        baseline = self.load("baseline")
        baseline["cases"][0]["samples"][1]["cia_phase"] = 0
        self.save(baseline, "baseline")
        with self.assertRaisesRegex(AssertionError, "CIA phase"):
            self.compare()  # identical bad schedules are not valid evidence

    def test_initial_game_and_timer_state_must_match(self):
        good = self.load()
        for field, value in (("game_sha256", digest("other initial game")), ("first_audio_period_cycles", 123)):
            with self.subTest(field=field):
                report = copy.deepcopy(good)
                report["cases"][0]["samples"][0]["initial"][field] = value
                self.save(report)
                with self.assertRaisesRegex(AssertionError, "initial state"):
                    self.compare()

    def test_profiled_phase_sets_must_match(self):
        good = self.load()
        for group, field in (("phase_work_bus_cycles", "audio"), ("pixel_bus_cycles_detail", "hud")):
            with self.subTest(group=group):
                report = copy.deepcopy(good)
                del report["cases"][0]["samples"][0][group][field]
                self.save(report)
                with self.assertRaisesRegex(AssertionError, group):
                    self.compare()

    def test_each_geometry_game_and_decoded_pixel_difference_fails_correctness(self):
        good = self.load()
        for field in ("raw_geometry_sha256", "game_sha256", "native_pixels_sha256"):
            with self.subTest(field=field):
                report = copy.deepcopy(good)
                report["cases"][1]["samples"][2][field] = digest("mismatch")
                self.save(report)
                result = self.compare()
                self.assertFalse(result["exact_geometry_state_and_pixels"])
                self.assertEqual(result["cases"][1]["differences"], [{"sample": 2, "field": field}])

    def test_missing_or_empty_semantic_hashes_are_not_evidence(self):
        good = self.load()
        for field in ("raw_geometry_sha256", "game_sha256", "native_pixels_sha256", "native_sha256"):
            with self.subTest(field=field):
                report = copy.deepcopy(good)
                report["cases"][0]["samples"][0][field] = ""
                self.save(report)
                with self.assertRaisesRegex(AssertionError, "Missing or invalid"):
                    self.compare()

    def test_empty_duplicate_missing_cases_and_sample_count_are_rejected(self):
        good = self.load()
        for change in (lambda r: r.update(cases=[]),
                       lambda r: r["cases"].append(copy.deepcopy(r["cases"][0])),
                       lambda r: r["cases"].pop(),
                       lambda r: r["cases"][0]["samples"].pop(),
                       lambda r: r.update(samples=0)):
            report = copy.deepcopy(good)
            change(report)
            self.save(report)
            with self.assertRaises(AssertionError):
                self.compare()

    def test_invalid_numeric_measurements_are_rejected(self):
        good = self.load()
        for change in (lambda r: r.update(bus_hz=0),
                       lambda r: r["real_time"].update(fps=float("nan")),
                       lambda r: r["cases"][0]["samples"][0].update(bus_cycles_delta=0),
                       lambda r: r["cases"][0]["samples"][0].update(bus_cycles_delta=float("inf")),
                       lambda r: r["cases"][0]["samples"][0]["phase_work_bus_cycles"].update(pixels=-1)):
            report = copy.deepcopy(good)
            change(report)
            self.save(report)
            with self.assertRaisesRegex(AssertionError, "Invalid"):
                self.compare()

    def test_five_percent_case_gate_cannot_be_hidden_by_better_average(self):
        report = self.load()
        for case, cycles in zip(report["cases"], (1060, 1000)):
            for sample in case["samples"]:
                sample["bus_cycles_delta"] = cycles
        self.save(report)
        result = self.compare()
        self.assertLess(result["mean_frame_change_percent"], 0)
        self.assertEqual(result["over_five_percent_regressions"], 1)
        self.assertFalse(result["performance_win"])

    def test_exact_five_percent_boundary_is_allowed_if_mean_is_faster(self):
        baseline, report = self.load("baseline"), self.load()
        # Dividing two rounded millisecond values at this clock produces
        # 0.9499999999999998; the exact cycle ratio remains 950/1000.
        baseline["bus_hz"] = report["bus_hz"] = 985009
        for sample in baseline["cases"][0]["samples"]:
            sample["bus_cycles_delta"] = 950
        for case in report["cases"]:
            for sample in case["samples"]:
                sample["bus_cycles_delta"] = 1000
        self.save(baseline, "baseline")
        self.save(report)
        result = self.compare()
        self.assertAlmostEqual(result["minimum_fps_ratio"], .95)
        self.assertTrue(result["performance_win"])

    def test_equal_or_slower_mean_is_not_a_performance_win(self):
        good = self.load()
        for multiplier in (1, 1.01):
            report = copy.deepcopy(good)
            for case, cycles in zip(report["cases"], (1000, 2000)):
                for sample in case["samples"]:
                    sample["bus_cycles_delta"] = cycles * multiplier
            self.save(report)
            self.assertFalse(self.compare()["performance_win"])

    def test_held_fire_is_reported_separately_from_the_approved_gate(self):
        report = self.load()
        report["real_time"]["fps"] = 6
        self.save(report)
        result = self.compare()
        self.assertEqual(result["real_time_fps_change_percent"], -50)
        self.assertTrue(result["performance_win"])

    def test_complete_matrix_requires_both_baselines_and_all_selected_configurations(self):
        for profiles in (summary.PROFILES, summary.HISTORICAL_PROFILES):
            combinations = self.matrix(profiles)
            result = summary.summarize(combinations, profiles, True)
            self.assertEqual(result["renderer_winners"], {"spans": True, "front": True})
            self.assertTrue(result["pointer_hoist_retention_gate_passed"])
            self.assertEqual(result["expected_profiles"], list(profiles))
            for index, item in enumerate(combinations):
                if item["candidate"] not in ("spans", "front") or item["baseline"] not in ("original", "corrected"):
                    continue
                with self.subTest(profiles=profiles, missing=(item["baseline"], item["candidate"], item["profile"], item["video"])):
                    incomplete = combinations[:index] + combinations[index + 1:]
                    self.assertFalse(summary.summarize(incomplete, profiles, True)["renderer_winners"][item["candidate"]])
                for gate in ("performance_win", "exact_geometry_state_and_pixels"):
                    with self.subTest(profiles=profiles, gate=gate, configuration=(item["profile"], item["video"])):
                        damaged = copy.deepcopy(combinations)
                        damaged[index][gate] = False
                        self.assertFalse(summary.summarize(damaged, profiles, True)["renderer_winners"][item["candidate"]])

    def test_profile_selection_is_explicit_and_never_inferred_from_evidence(self):
        current, historical = self.matrix(), self.matrix(summary.HISTORICAL_PROFILES)
        for combinations, expected in ((current, summary.HISTORICAL_PROFILES),
                                       (historical, summary.PROFILES)):
            result = summary.summarize(combinations, expected, True)
            self.assertEqual(result["renderer_winners"], {"spans": False, "front": False})
            self.assertFalse(result["pointer_hoist_retention_gate_passed"])
        for profiles in ((), ("4sid", "4sid"), ("8sid",)):
            with self.assertRaisesRegex(AssertionError, "Invalid expected profiles"):
                summary.summarize(current, profiles, True)

    def test_simple_summary_requires_all_selected_configurations_without_duplicates(self):
        for profiles in (summary.PROFILES, summary.HISTORICAL_PROFILES):
            combinations = [summary.compare(self.root, profile, video)
                            for profile in profiles for video in summary.VIDEOS]
            self.assertTrue(summary.summarize(combinations, profiles)["performance_gate_passed"])
            for index in range(len(combinations)):
                incomplete = combinations[:index] + combinations[index + 1:]
                self.assertFalse(summary.summarize(incomplete, profiles)["performance_gate_passed"])
                duplicate = combinations + [combinations[index]]
                self.assertFalse(summary.summarize(duplicate, profiles)["performance_gate_passed"])
            mixed_pairs = copy.deepcopy(combinations)
            mixed_pairs[0]["baseline"] = "original"
            self.assertFalse(summary.summarize(mixed_pairs, profiles)["performance_gate_passed"])
        for profiles, expected in ((summary.PROFILES, summary.HISTORICAL_PROFILES),
                                   (summary.HISTORICAL_PROFILES, summary.PROFILES)):
            combinations = [summary.compare(self.root, profile, video)
                            for profile in profiles for video in summary.VIDEOS]
            self.assertFalse(summary.summarize(combinations, expected)["performance_gate_passed"])

    def test_empty_or_duplicate_matrix_evidence_cannot_win(self):
        for matrix_mode in (False, True):
            empty = summary.summarize([], summary.PROFILES, matrix_mode)
            self.assertFalse(empty["correctness_passed"])
            self.assertFalse(empty["performance_gate_passed"])
        self.assertEqual(summary.summarize([], summary.PROFILES, True)["renderer_winners"], {"spans": False, "front": False})
        combinations = self.matrix()
        duplicate = next(item for item in combinations if (item["baseline"], item["candidate"]) == ("original", "front"))
        self.assertFalse(summary.summarize(combinations + [duplicate], summary.PROFILES, True)["renderer_winners"]["front"])

    def test_pointer_hoist_retention_requires_its_own_complete_nonregressing_pairs(self):
        for profiles in (summary.PROFILES, summary.HISTORICAL_PROFILES):
            combinations = self.matrix(profiles)
            indices = [index for index, item in enumerate(combinations)
                       if (item["baseline"], item["candidate"]) == ("unhoisted", "spans")]
            self.assertEqual(len(indices), len(profiles) * 2)
            for index in indices:
                damaged = copy.deepcopy(combinations)
                frame = damaged[index]["mean_milliseconds"]["frame"]
                frame[1] = frame[0]
                self.assertTrue(summary.summarize(damaged, profiles, True)["pointer_hoist_retention_gate_passed"])
                frame[1] += .001
                result = summary.summarize(damaged, profiles, True)
                self.assertFalse(result["pointer_hoist_retention_gate_passed"])
                self.assertTrue(result["renderer_winners"]["spans"])
                del damaged[index]
                self.assertFalse(summary.summarize(damaged, profiles, True)["pointer_hoist_retention_gate_passed"])
            self.assertFalse(summary.summarize(combinations + [combinations[indices[0]]], profiles, True)["pointer_hoist_retention_gate_passed"])

    def test_cli_preserves_default_and_custom_prefixes_and_output(self):
        for arguments, output in (((), "summary.json"),
                                  (("--baseline", "corrected", "--candidate", "front", "--output", "custom.json"), "custom.json")):
            result = self.run_cli(*arguments)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            report = json.loads((self.root / output).read_text())
            self.assertEqual(len(report["comparisons"]), 2)
            self.assertEqual(report["expected_profiles"], ["4sid"])
            self.assertTrue(report["correctness_passed"])

    def test_historical_cli_requires_full_two_and_four_sid_matrix(self):
        for arguments, count in (((), 4), (("--optimization-matrix",), 32)):
            result = self.run_cli("--historical-dual-sid", *arguments)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            report = json.loads((self.root / "summary.json").read_text())
            self.assertEqual(len(report["comparisons"]), count)
            self.assertEqual(report["expected_profiles"], ["2sid", "4sid"])
            self.assertTrue(report["correctness_passed"])
            self.assertTrue(report["performance_gate_passed"])

    def test_default_cli_does_not_require_historical_evidence_but_historical_mode_does(self):
        self.path("candidate", "2sid", "ntsc").unlink()
        self.path("front", "2sid", "ntsc").unlink()
        for arguments in ((), ("--optimization-matrix",)):
            result = self.run_cli(*arguments)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            historical = self.run_cli("--historical-dual-sid", *arguments)
            self.assertNotEqual(historical.returncode, 0)
            self.assertIn("FileNotFoundError", historical.stderr)

    def test_cli_never_infers_a_smaller_matrix_when_current_evidence_is_missing(self):
        self.path("candidate", "4sid", "ntsc").unlink()
        self.path("front", "4sid", "ntsc").unlink()
        for selection in ((), ("--historical-dual-sid",)):
            for mode in ((), ("--optimization-matrix",)):
                result = self.run_cli(*selection, *mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("FileNotFoundError", result.stderr)

    def test_matrix_cli_and_pixel_failure_exit_status(self):
        result = self.run_cli("--optimization-matrix")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        report = json.loads((self.root / "summary.json").read_text())
        self.assertEqual(len(report["comparisons"]), 16)
        self.assertEqual(report["renderer_winners"], {"spans": True, "front": True})
        front = self.load("front", "4sid", "ntsc")
        front["cases"][0]["samples"][2]["native_pixels_sha256"] = digest("wrong pixels")
        self.save(front, "front", "4sid", "ntsc")
        result = self.run_cli("--optimization-matrix")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        report = json.loads((self.root / "summary.json").read_text())
        self.assertFalse(report["correctness_passed"])
        self.assertEqual(report["renderer_winners"], {"spans": True, "front": False})


if __name__ == "__main__":
    unittest.main()
