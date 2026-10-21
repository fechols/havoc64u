#!/usr/bin/env python3
"""Summarize matched span-buffer experiments without waiving pixel differences.

Expects baseline/candidate-{2sid,4sid}-{pal,ntsc}/report.json beneath ROOT.
Every case and CIA phase must match. Palette-history differences need separate
oracle evidence; this summary deliberately does not approve mismatching pixels.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics


def case_key(case):
    return case["fixture"], case["ticks"], case["palette"]


def median_ms(case, hz, getter):
    return statistics.median(getter(sample) for sample in case["samples"]) * 1000 / hz


def compare(root, profile, video):
    paths = [root / f"{kind}-{profile}-{video}" / "report.json" for kind in ("baseline", "candidate")]
    before, after = [json.loads(path.read_text()) for path in paths]
    for report in (before, after):
        assert report["passed"], f"Incomplete or failed benchmark: {report.get('error')}"
        assert report["video"] == video and report["sid_count"] == int(profile[0])
        for suffix, expected in report["hashes"].items():
            package = root / f"{'baseline' if report is before else 'candidate'}-{profile}" / f"havoc.{suffix}"
            assert hashlib.sha256(package.read_bytes()).hexdigest() == expected, f"Changed package: {package}"
    for field in ("protocol", "fixture_sha256", "samples", "bus_hz", "vice_sha256"):
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
            assert left["cia_phase"] == right["cia_phase"]
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
        speedup = timing["frame"][0] / timing["frame"][1]
        cases.append({"fixture": key[0], "ticks": key[1], "palette": key[2],
                      "differences": differences, "milliseconds": timing, "fps_ratio": speedup,
                      "native_bytes_equal": all(left["native_sha256"] == right["native_sha256"] for left, right in zip(a, b))})
    means = {metric: [statistics.mean(case["milliseconds"][metric][i] for case in cases) for i in (0, 1)]
             for metric in cases[0]["milliseconds"]}
    exact = not any(case["differences"] for case in cases)
    regressions = [case for case in cases if case["fps_ratio"] < .95]
    return {"profile": profile, "video": video, "reports": [str(path) for path in paths],
            "package_hashes": {"baseline": before["hashes"], "candidate": after["hashes"]},
            "case_count": len(cases), "sample_pairs": len(cases) * after["samples"],
            "exact_geometry_state_and_pixels": exact, "mean_milliseconds": means,
            "mean_frame_change_percent": (means["frame"][1] / means["frame"][0] - 1) * 100,
            "real_time_fps": [before["real_time"]["fps"], after["real_time"]["fps"]],
            "minimum_fps_ratio": min(case["fps_ratio"] for case in cases),
            "over_five_percent_regressions": len(regressions),
            "maximum_candidate_audio_lateness_ms": max(sample["max_audio_lateness_ms"] for case in after["cases"] for sample in case["samples"]),
            "performance_win": means["frame"][1] < means["frame"][0] and not regressions,
            "cases": cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    combinations = [compare(args.root, profile, video) for profile in ("2sid", "4sid") for video in ("pal", "ntsc")]
    summary = {"scope": "Calibrated VICE, matched post-refactor packages; no physical-hardware timing claim",
               "correctness_passed": all(item["exact_geometry_state_and_pixels"] for item in combinations),
               "performance_gate_passed": all(item["performance_win"] for item in combinations),
               "sample_pairs": sum(item["sample_pairs"] for item in combinations), "comparisons": combinations}
    output = args.root / "summary.json"
    output.write_text(json.dumps(summary, indent=2) + "\n")
    for item in combinations:
        print(f"{item['profile']} {item['video']}: {item['case_count']} cases, exact={item['exact_geometry_state_and_pixels']}, "
              f"frame {item['mean_frame_change_percent']:+.2f}%, worst FPS {item['minimum_fps_ratio']:.4f}x, "
              f"regressions={item['over_five_percent_regressions']}")
    print(output)
    return 0 if summary["correctness_passed"] and summary["performance_gate_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
