#!/usr/bin/env python3
"""Reject incomplete sustained SFX windows and hidden tail-latency regressions."""
import copy
import runpy
from pathlib import Path
import re
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parent
driver = runpy.run_path(str(TOOLS / "bench-native.py"))
compare = runpy.run_path(str(TOOLS / "bench-sfx.py"))


def account_frames(run):
    cursor = run["start_bus_cycles"]
    for frame in run["frames"]:
        frame["start_bus_cycles"] = cursor
        frame["start_audio_ticks"] = cursor // 20000
        cursor += frame["bus_cycles_delta"]
        frame["bus_cycles"] = cursor
        frame["audio_ticks"] = cursor // 20000
    run["inter_frame_bus_cycles"] = run["boundary_bus_cycles"] - cursor


def report():
    result = dict(sustained=dict(seconds=30, ticks=1500, audio_hz=50, cia_phases=[0, 1, 2]),
                  driver_sha256="driver", smoke_tool_sha256="smoke", bus_hz=1000000,
                  audio_period_cycles=20000, cases=[])
    identity = {field: field for field in compare["IDENTITY"]}
    for name in sorted(compare["SCENARIOS"]):
        schedule = driver["sustained_schedule"](name, 1500)
        injections = [dict(tick=tick, events=events)
                      for tick, events in driver["schedule_batches"](schedule).items()]
        case = dict(fixture=name, schedule=schedule, schedule_sha256=driver["semantic_hash"](schedule),
                    identity=identity, ticks=10, palette="retained", runs=[], statistics={})
        for phase in range(3):
            run = dict(cia_phase=phase, completed_audio_ticks=1500, next_tick=1500,
                injections=copy.deepcopy(injections), requested_events=len(schedule),
                admissions=[dict(tick=tick, groups=[]) for tick in range(1500)], admitted_groups=0,
                scheduled_window_bus_cycles=30000000, boundary_lateness_cycles=0,
                start_bus_cycles=0, first_deadline_bus_cycles=20000, boundary_bus_cycles=30020000,
                actual_window_bus_cycles=30000000,
                tick_entries=[dict(tick=tick, bus_cycles=20000 + tick * 20000, lateness_cycles=0)
                              for tick in range(1501)],
                boundary_frame=dict(partial=True, start_bus_cycles=30020000, bus_cycles_delta=0),
                frames=[dict(identity, simulation_ticks=10, start_audio_ticks=index * 5,
                             audio_ticks=index * 5 + 5, bus_cycles_delta=100000) for index in range(20)])
            account_frames(run)
            case["runs"].append(run)
        result["cases"].append(case)
    return result


class SustainedTests(unittest.TestCase):
    def setUp(self):
        self.reference = report()

    def case(self, name="sfx-dense-combat", value=None):
        return next(case for case in (value or self.reference)["cases"] if case["fixture"] == name)

    def test_every_schedule_has_requests_after_the_old_single_frame_prefix(self):
        for name in compare["SCENARIOS"] - {"sfx-quiet"}:
            schedule = driver["sustained_schedule"](name, 1500)
            self.assertGreater(max(event[0] for event in schedule), 1400)
            self.assertLessEqual(max(map(len, driver["schedule_batches"](schedule).values())), 8)
        self.assertEqual(driver["sustained_schedule"]("sfx-quiet", 1500), [])

    def test_dense_schedule_retriggers_bombs_and_requests_both_explosion_sides(self):
        schedule = driver["sustained_schedule"]("sfx-dense-combat", 1500)
        self.assertIn((15, 6, 0), schedule)
        self.assertIn((35, 6, 0), schedule)
        self.assertIn((15, 7, -100), schedule)
        self.assertIn((16, 7, 100), schedule)

    def test_driver_rejects_a_short_prefix(self):
        case = self.case()
        with self.assertRaisesRegex(RuntimeError, "truncated"):
            driver["validate_sustained_schedule"](case["schedule"], case["runs"][0]["injections"][:5], 1500, 1500)

    def test_driver_rejects_incomplete_clock(self):
        case = self.case()
        with self.assertRaisesRegex(RuntimeError, "clock stopped"):
            driver["validate_sustained_schedule"](case["schedule"], case["runs"][0]["injections"], 1499, 1500)

    def test_complete_matching_reports_pass(self):
        cases = compare["compare_sustained"]("pal", self.reference, copy.deepcopy(self.reference), 1)
        self.assertTrue(all(case["passed"] for case in cases))

    def test_missing_final_request_fails(self):
        self.case()["runs"][0]["injections"].pop()
        with self.assertRaisesRegex(ValueError, "injections"):
            compare["check_sustained"](self.reference, "fixture")

    def test_duplicate_request_fails(self):
        self.case()["runs"][0]["injections"].append(self.case()["runs"][0]["injections"][-1])
        with self.assertRaisesRegex(ValueError, "injections"):
            compare["check_sustained"](self.reference, "fixture")

    def test_missing_admission_tick_fails(self):
        self.case()["runs"][0]["admissions"].pop()
        with self.assertRaisesRegex(ValueError, "admission trace"):
            compare["check_sustained"](self.reference, "fixture")

    def test_changed_admission_fails_even_with_same_requests(self):
        candidate = copy.deepcopy(self.reference)
        run = self.case(value=candidate)["runs"][0]
        run["admissions"][0]["groups"] = [[0, 3]]
        run["admitted_groups"] = 1
        with self.assertRaisesRegex(ValueError, "requested/admitted"):
            compare["compare_sustained"]("pal", self.reference, candidate, 1)

    def test_missing_timer_phase_fails(self):
        self.case()["runs"].pop()
        with self.assertRaisesRegex(ValueError, "CIA phase"):
            compare["check_sustained"](self.reference, "fixture")

    def test_frame_past_the_window_fails(self):
        self.case()["runs"][0]["frames"][-1]["audio_ticks"] = 1501
        with self.assertRaisesRegex(ValueError, "audio counters|scoring window"):
            compare["check_sustained"](self.reference, "fixture")

    def test_truncated_frame_list_fails_even_with_complete_schedule(self):
        for case in self.reference["cases"]:
            for run in case["runs"]:
                run["frames"] = run["frames"][:1]
        with self.assertRaisesRegex(ValueError, "gaps do not reconcile"):
            compare["check_sustained"](self.reference, "fixture")

    def test_reordered_frames_fail(self):
        self.case()["runs"][0]["frames"][1:3] = reversed(self.case()["runs"][0]["frames"][1:3])
        with self.assertRaisesRegex(ValueError, "timeline"):
            compare["check_sustained"](self.reference, "fixture")

    def test_missing_gap_irq_fails(self):
        self.case()["runs"][0]["tick_entries"].pop(500)
        with self.assertRaisesRegex(ValueError, "all-tick IRQ trace"):
            compare["check_sustained"](self.reference, "fixture")

    def test_late_gap_irq_fails(self):
        entry = self.case()["runs"][0]["tick_entries"][500]
        entry["bus_cycles"] += 5000
        entry["lateness_cycles"] += 5000
        with self.assertRaisesRegex(ValueError, "deadline failed inside frame or gap"):
            compare["check_sustained"](self.reference, "fixture")

    def test_false_irq_latency_summary_fails(self):
        self.case()["runs"][0]["tick_entries"][500]["lateness_cycles"] = 1
        with self.assertRaisesRegex(ValueError, "deadline failed"):
            compare["check_sustained"](self.reference, "fixture")

    def test_changed_render_identity_fails(self):
        self.case()["runs"][0]["frames"][-1]["native_sha256"] = "different"
        with self.assertRaisesRegex(ValueError, "render/gameplay"):
            compare["check_sustained"](self.reference, "fixture")

    def test_median_regression_fails(self):
        candidate = copy.deepcopy(self.reference)
        for run in self.case(value=candidate)["runs"]:
            for frame in run["frames"]:
                frame["bus_cycles_delta"] = 102000
            account_frames(run)
        cases = compare["compare_sustained"]("pal", self.reference, candidate, 1)
        self.assertFalse(next(case for case in cases if case["scenario"] == "sfx-dense-combat")["passed"])

    def test_single_phase_p95_regression_fails_with_unchanged_median(self):
        candidate = copy.deepcopy(self.reference)
        for frame in self.case(value=candidate)["runs"][0]["frames"][-2:]:
            frame["bus_cycles_delta"] = 120000
        account_frames(self.case(value=candidate)["runs"][0])
        result = next(case for case in compare["compare_sustained"]("pal", self.reference, candidate, 1)
                      if case["scenario"] == "sfx-dense-combat")
        self.assertEqual(result["additional_frame_time_percent"], 0)
        self.assertFalse(result["passed"])
        self.assertGreater(result["phases"][0]["additional_frame_time_percent"]["p95"], 1)


class BinaryInspectionTests(unittest.TestCase):
    class Monitor:
        def __init__(self, binary, text):
            self.binary, self.text = binary, text
            self.text_reads = 0

        def command(self, command):
            path = re.search(r'bsave "([^"]+)"', command)[1]
            Path(path).write_bytes(self.binary)

        def read(self, _address, _length):
            self.text_reads += 1
            return self.text

    def test_each_range_is_compared_independently_once(self):
        with tempfile.TemporaryDirectory() as directory:
            monitor = self.Monitor(bytes(range(64)), bytes(range(64)))
            reader = driver["BinaryInspection"](monitor, Path(directory))
            self.assertEqual(reader.read(0xa000, 64, "ram"), bytes(range(64)))
            reader.read(0xa000, 64, "ram")
            reader.read(0x8000, 64, "ram")
            self.assertEqual(monitor.text_reads, 2)
            self.assertEqual(len(reader.checks), 2)

    def test_color_comparison_ignores_only_undefined_upper_nibble(self):
        with tempfile.TemporaryDirectory() as directory:
            reader = driver["BinaryInspection"](self.Monitor(b"\xa3", b"\xf3"), Path(directory))
            self.assertEqual(reader.read(0xd800, 1, "io", mask=15), b"\x03")

    def test_different_bytes_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            reader = driver["BinaryInspection"](self.Monitor(b"\x03", b"\x04"), Path(directory))
            with self.assertRaisesRegex(RuntimeError, "Binary/text"):
                reader.read(0xa000, 1, "ram")

    def test_wrong_length_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            reader = driver["BinaryInspection"](self.Monitor(b"\x03", b"\x03"), Path(directory))
            with self.assertRaisesRegex(RuntimeError, "wrong length"):
                reader.read(0xa000, 2, "ram")


if __name__ == "__main__":
    unittest.main()
