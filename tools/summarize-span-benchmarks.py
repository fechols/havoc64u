#!/usr/bin/env python3
"""Summarize matched span-buffer experiments without waiving pixel differences.

By default, reads baseline/candidate-4sid-{pal,ntsc}/report.json beneath ROOT;
--historical-dual-sid additionally requires the archived 2sid configurations.
--baseline, --candidate and --output select other prefixes and output.
--optimization-matrix reads original, corrected, unoptimized, unhoisted, spans
and front reports. A renderer wins only with exact geometry/state/pixels and a
faster mean full frame against BOTH painter baselines in all selected SID/video
configurations, with no per-case FPS ratio below 0.95. Pointer-hoist retention
separately requires complete unhoisted-to-spans comparisons with no mean
frame regression. Held-FIRE FPS changes are reported without changing either
gate. Packages, tools, workload, initial state and CIA phases must match;
palette-history differences never waive decoded-pixel mismatches.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import statistics


PROFILES = ("4sid",)
HISTORICAL_PROFILES = ("2sid", "4sid")
VIDEOS = ("pal", "ntsc")
MATRIX_PAIRS = (("original", "corrected"), ("corrected", "unoptimized"),
                ("unoptimized", "spans"), ("unhoisted", "spans"),
                ("corrected", "spans"), ("original", "spans"),
                ("corrected", "front"), ("original", "front"))


def require_digest(value, label):
    assert isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value), f"Missing or invalid {label}"


def require_number(value, label, positive=False):
    assert (isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and (value > 0 if positive else value >= 0)), f"Invalid {label}"


def case_key(case):
    return case["fixture"], case["ticks"], case["palette"]


def median_ms(case, hz, getter):
    return statistics.median(getter(sample) for sample in case["samples"]) * 1000 / hz


def compare(root, profile, video, baseline_name="baseline", candidate_name="candidate"):
    paths = [root / f"{kind}-{profile}-{video}" / "report.json" for kind in (baseline_name, candidate_name)]
    before, after = [json.loads(path.read_text()) for path in paths]
    for report in (before, after):
        assert report["passed"] is True, f"Incomplete or failed benchmark: {report.get('error')}"
        assert report["video"] == video and report["sid_count"] == int(profile[0])
        assert set(report["hashes"]) == {"prg", "reu", "elf"}, "Missing package hashes"
        assert isinstance(report["samples"], int) and not isinstance(report["samples"], bool) and report["samples"] > 0, "Invalid sample count"
        require_number(report["bus_hz"], "bus_hz", positive=True)
        require_number(report["real_time"]["fps"], "real-time FPS", positive=True)
        for field in ("fixture_sha256", "vice_sha256", "driver_sha256", "smoke_tool_sha256"):
            require_digest(report[field], field)
        for suffix, expected in report["hashes"].items():
            require_digest(expected, f"{suffix} package hash")
            package = root / f"{baseline_name if report is before else candidate_name}-{profile}" / f"havoc.{suffix}"
            assert hashlib.sha256(package.read_bytes()).hexdigest() == expected, f"Changed package: {package}"
    for field in ("protocol", "fixture_sha256", "samples", "bus_hz", "vice_sha256", "driver_sha256", "smoke_tool_sha256"):
        assert before[field] == after[field], f"Mismatched {field}"
    old = {case_key(case): case for case in before["cases"]}
    new = {case_key(case): case for case in after["cases"]}
    assert len(old) == len(before["cases"]) and len(new) == len(after["cases"]), "Duplicate cases"
    assert old.keys() == new.keys() and old, "Missing comparison cases"
    cases = []
    checks = ("raw_geometry_sha256", "game_sha256", "native_pixels_sha256")
    for key, candidate in new.items():
        baseline = old[key]
        a, b = baseline["samples"], candidate["samples"]
        assert len(a) == len(b) == after["samples"]
        differences = []
        for index, (left, right) in enumerate(zip(a, b)):
            assert left["cia_phase"] == right["cia_phase"] == index % 3, f"Mismatched CIA phase: {key}, sample {index}"
            assert left["initial"] == right["initial"], f"Mismatched initial state: {key}, sample {index}"
            for group in ("phase_work_bus_cycles", "pixel_bus_cycles_detail"):
                assert left[group].keys() == right[group].keys(), f"Mismatched {group}: {key}, sample {index}"
            for sample in (left, right):
                require_digest(sample["initial"]["game_sha256"], "initial game hash")
                require_number(sample["initial"]["first_audio_period_cycles"], "initial CIA period", positive=True)
                for field in (*checks, "native_sha256"):
                    require_digest(sample[field], field)
                require_number(sample["bus_cycles_delta"], "frame cycles", positive=True)
                require_number(sample["max_audio_lateness_ms"], "audio lateness")
                for group in ("phase_work_bus_cycles", "pixel_bus_cycles_detail"):
                    for phase, cycles in sample[group].items():
                        require_number(cycles, f"{group}.{phase}")
            differences += [{"sample": index, "field": field} for field in checks if left[field] != right[field]]
        timing = {}
        metrics = {
            "frame": lambda sample: sample["bus_cycles_delta"],
            "phase_work": lambda sample: sum(sample["phase_work_bus_cycles"].values()),
            "pixels": lambda sample: sample["phase_work_bus_cycles"]["pixels"],
            "raster": lambda sample: sample["pixel_bus_cycles_detail"]["raster"],
            "pack": lambda sample: sample["pixel_bus_cycles_detail"]["pack"],
        }
        for metric, getter in metrics.items():
            timing[metric] = [median_ms(case, after["bus_hz"], getter) for case in (baseline, candidate)]
        # The common clock cancels. Compare cycles directly so milliseconds
        # conversion cannot round an exact 0.95 boundary below the gate.
        speedup = statistics.median(sample["bus_cycles_delta"] for sample in a) / statistics.median(sample["bus_cycles_delta"] for sample in b)
        cases.append({"fixture": key[0], "ticks": key[1], "palette": key[2],
                      "differences": differences, "milliseconds": timing, "fps_ratio": speedup,
                      "native_bytes_equal": all(left["native_sha256"] == right["native_sha256"] for left, right in zip(a, b))})
    means = {metric: [statistics.mean(case["milliseconds"][metric][i] for case in cases) for i in (0, 1)]
             for metric in cases[0]["milliseconds"]}
    exact = not any(case["differences"] for case in cases)
    regressions = [case for case in cases if case["fps_ratio"] < .95]
    return {"profile": profile, "video": video, "baseline": baseline_name, "candidate": candidate_name,
            "reports": [str(path) for path in paths],
            "package_hashes": {"baseline": before["hashes"], "candidate": after["hashes"]},
            "case_count": len(cases), "sample_pairs": len(cases) * after["samples"],
            "exact_geometry_state_and_pixels": exact, "mean_milliseconds": means,
            "mean_frame_change_percent": (means["frame"][1] / means["frame"][0] - 1) * 100,
            "real_time_fps": [before["real_time"]["fps"], after["real_time"]["fps"]],
            "real_time_fps_change_percent": (after["real_time"]["fps"] / before["real_time"]["fps"] - 1) * 100,
            "minimum_fps_ratio": min(case["fps_ratio"] for case in cases),
            "over_five_percent_regressions": len(regressions),
            "maximum_candidate_audio_lateness_ms": max(sample["max_audio_lateness_ms"] for case in after["cases"] for sample in case["samples"]),
            "performance_win": means["frame"][1] < means["frame"][0] and not regressions,
            "cases": cases}


def summarize(combinations, profiles, optimization_matrix=False):
    assert profiles and len(set(profiles)) == len(profiles) and set(profiles) <= set(HISTORICAL_PROFILES), "Invalid expected profiles"
    configurations = {(profile, video) for profile in profiles for video in VIDEOS}
    expected = {(baseline, profile, video) for baseline in ("original", "corrected")
                for profile in profiles for video in VIDEOS}
    winners = {}
    pointer_hoist_retained = None
    if optimization_matrix:
        for candidate in ("spans", "front"):
            evidence = [item for item in combinations if item["candidate"] == candidate
                        and item["baseline"] in ("original", "corrected")]
            actual = {(item["baseline"], item["profile"], item["video"]) for item in evidence}
            winners[candidate] = (len(evidence) == len(expected) and actual == expected
                                  and all(item["performance_win"] and item["exact_geometry_state_and_pixels"]
                                          for item in evidence))
        evidence = [item for item in combinations if (item["baseline"], item["candidate"]) == ("unhoisted", "spans")]
        actual = {(item["profile"], item["video"]) for item in evidence}
        pointer_hoist_retained = (len(evidence) == len(configurations)
                                 and actual == configurations
                                 and all(item["exact_geometry_state_and_pixels"]
                                         and item["mean_milliseconds"]["frame"][1] <= item["mean_milliseconds"]["frame"][0]
                                         for item in evidence))
    else:
        actual = {(item["profile"], item["video"]) for item in combinations}
        pairs = {(item["baseline"], item["candidate"]) for item in combinations}
        complete = len(combinations) == len(configurations) and actual == configurations and len(pairs) == 1
    return {"scope": "Calibrated VICE, matched packages; no physical-hardware timing claim",
            "expected_profiles": list(profiles),
            "correctness_passed": bool(combinations) and all(item["exact_geometry_state_and_pixels"] for item in combinations),
            "performance_gate_passed": any(winners.values()) if optimization_matrix else
                                       complete and all(item["performance_win"] for item in combinations),
            "renderer_winners": winners,
            "pointer_hoist_retention_gate_passed": pointer_hoist_retained,
            "sample_pairs": sum(item["sample_pairs"] for item in combinations), "comparisons": combinations}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--baseline", default="baseline", help="baseline package/report name prefix")
    parser.add_argument("--candidate", default="candidate", help="candidate package/report name prefix")
    parser.add_argument("--output", default="summary.json")
    parser.add_argument("--optimization-matrix", action="store_true",
                        help="compare shared HUD fix, deferred optimization and front traversal against both painter baselines")
    parser.add_argument("--historical-dual-sid", action="store_true",
                        help="require both archived 2sid and current 4sid PAL/NTSC evidence")
    args = parser.parse_args()
    profiles = HISTORICAL_PROFILES if args.historical_dual_sid else PROFILES
    pairs = MATRIX_PAIRS if args.optimization_matrix else [(args.baseline, args.candidate)]
    combinations = [compare(args.root, profile, video, baseline, candidate)
                    for baseline, candidate in pairs for profile in profiles for video in VIDEOS]
    summary = summarize(combinations, profiles, args.optimization_matrix)
    output = args.root / args.output
    output.write_text(json.dumps(summary, indent=2) + "\n")
    for item in combinations:
        print(f"{item['baseline']} -> {item['candidate']} {item['profile']} {item['video']}: {item['case_count']} cases, exact={item['exact_geometry_state_and_pixels']}, "
              f"frame {item['mean_frame_change_percent']:+.2f}%, worst FPS {item['minimum_fps_ratio']:.4f}x, "
              f"regressions={item['over_five_percent_regressions']}")
    print(output)
    return 0 if summary["correctness_passed"] and summary["performance_gate_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
