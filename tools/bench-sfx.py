#!/usr/bin/env python3
"""Validate matched PAL/NTSC SFX frame benchmarks without rerunning the emulator.

Inputs are directories containing bench-pal/report.json and bench-ntsc/report.json
from bench-native.py --sfx-workloads --samples 9 --ticks 10 --palettes retained.
The nine samples cover three CIA phases three times each. This comparison
includes native IRQ and SID work, excludes DAC service, and is not hardware FPS.
"""
import argparse
import bisect
import hashlib
import json
from pathlib import Path
import statistics


SCENARIOS = {"sfx-quiet", "sfx-firing", "sfx-dense-combat", "sfx-boss", "sfx-scenic"}
IDENTITY = ("game_sha256", "geometry_sha256", "native_sha256", "sprite_state_sha256")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def summary(values):
    values = sorted(values)
    require(bool(values), "Empty sustained frame sample")
    return dict(median=statistics.median(values), p95=values[(len(values) * 95 + 99) // 100 - 1],
                max=values[-1], count=len(values))


def check_sustained_timeline(report, run, ticks, label):
    entries = run["tick_entries"]
    require([entry["tick"] for entry in entries] == list(range(ticks + 1)),
            f"Incomplete all-tick IRQ trace: {label}")
    cycles = [entry["bus_cycles"] for entry in entries]
    require(all(a < b for a, b in zip(cycles, cycles[1:])), f"IRQ timestamps run backward: {label}")
    first = run["first_deadline_bus_cycles"]
    for entry in entries:
        lateness = entry["bus_cycles"] - (first + entry["tick"] * report["audio_period_cycles"])
        require(lateness == entry["lateness_cycles"] and -2 <= lateness <= report["bus_hz"] * 0.002,
                f"IRQ deadline failed inside frame or gap: {label}/{entry['tick']}")
    require(cycles[-1] == run["boundary_bus_cycles"]
            and run["actual_window_bus_cycles"] == cycles[-1] - first
            and run["boundary_lateness_cycles"] == entries[-1]["lateness_cycles"],
            f"Terminal IRQ does not match the window boundary: {label}")
    boundary = run["boundary_frame"]
    require(boundary.get("partial") is True and boundary["bus_cycles_delta"] >= 0,
            f"Missing partial boundary accounting: {label}")
    cursor = run["start_bus_cycles"]
    gaps = 0
    for index, frame in enumerate(run["frames"]):
        start, end = frame["start_bus_cycles"], frame["bus_cycles"]
        require(start >= cursor and end == start + frame["bus_cycles_delta"],
                f"Frame timeline is truncated, overlapping or reordered: {label}/{index}")
        if index == 0:
            require(start == cursor, f"Initial frame is missing: {label}")
        gaps += start - cursor
        # VICE reports integral bus cycles while the CPU runs at64x; an IRQ
        # and checkpoint can share a timestamp on opposite sides of the count.
        require(bisect.bisect_left(cycles[:-1], start) <= frame["start_audio_ticks"] <= bisect.bisect_right(cycles[:-1], start)
                and bisect.bisect_left(cycles[:-1], end) <= frame["audio_ticks"] <= bisect.bisect_right(cycles[:-1], end),
                f"Frame audio counters do not match its IRQ timestamps: {label}/{index}")
        cursor = end
    require(boundary["start_bus_cycles"] >= cursor
            and boundary["start_bus_cycles"] + boundary["bus_cycles_delta"] == run["boundary_bus_cycles"],
            f"Partial frame does not end at the exact window boundary: {label}")
    gaps += boundary["start_bus_cycles"] - cursor
    require(gaps == run["inter_frame_bus_cycles"], f"Inter-frame gaps do not reconcile: {label}")
    accounted = (sum(frame["bus_cycles_delta"] for frame in run["frames"])
                 + gaps + boundary["bus_cycles_delta"])
    require(accounted == run["boundary_bus_cycles"] - run["start_bus_cycles"],
            f"Frame list does not cover the complete sustained window: {label}")


def check_sustained(report, name):
    settings = report.get("sustained", {})
    ticks = settings.get("ticks", 0)
    require(ticks >= 1500 and ticks == settings.get("seconds", 0) * 50
            and settings.get("audio_hz") == 50 and settings.get("cia_phases") == [0, 1, 2],
            f"Incomplete sustained window: {name}")
    cases = {case["fixture"]: case for case in report["cases"]}
    require(set(cases) == SCENARIOS and len(report["cases"]) == 5, f"Incomplete sustained scenarios: {name}")
    for scenario, case in cases.items():
        schedule = case["schedule"]
        encoded = json.dumps(schedule, sort_keys=True, separators=(",", ":")).encode()
        require(hashlib.sha256(encoded).hexdigest() == case["schedule_sha256"], f"Schedule digest differs: {name}/{scenario}")
        require(all(len(entry) == 3 and 0 <= entry[0] < ticks for entry in schedule)
                and schedule == sorted(schedule, key=lambda entry: entry[0]), f"Invalid schedule: {name}/{scenario}")
        expected = []
        for tick, effect, pan in schedule:
            if not expected or expected[-1]["tick"] != tick:
                expected.append(dict(tick=tick, events=[]))
            expected[-1]["events"].append([effect, pan])
        require(all(len(entry["events"]) <= 8 for entry in expected), f"Schedule queue overflow: {name}/{scenario}")
        require([run["cia_phase"] for run in case["runs"]] == [0, 1, 2], f"Missing CIA phase: {name}/{scenario}")
        for run in case["runs"]:
            label = f"{name}/{scenario}/{run['cia_phase']}"
            require(run["completed_audio_ticks"] == ticks and run["next_tick"] == ticks,
                    f"Truncated sustained clock: {label}")
            require(run["injections"] == expected and run["requested_events"] == len(schedule),
                    f"Missing, reordered, duplicated or truncated injections: {label}")
            require([entry["tick"] for entry in run["admissions"]] == list(range(ticks)),
                    f"Incomplete admission trace: {label}")
            require(run["admitted_groups"] == sum(len(entry["groups"]) for entry in run["admissions"]),
                    f"Admission count differs: {label}")
            require(run["scheduled_window_bus_cycles"] == ticks * report["audio_period_cycles"],
                    f"Unequal sustained duration: {label}")
            require(abs(run["boundary_lateness_cycles"]) <= report["bus_hz"] * 0.002,
                    f"Sustained terminal IRQ missed deadline: {label}")
            require(bool(run["frames"]), f"No complete frames: {label}")
            check_sustained_timeline(report, run, ticks, label)
            for frame in run["frames"]:
                require(all(frame[field] == case["identity"][field] for field in IDENTITY),
                        f"Repeated render/gameplay work differs: {label}")
                require(frame["simulation_ticks"] == run["frames"][0]["simulation_ticks"]
                        and 0 <= frame["start_audio_ticks"] <= frame["audio_ticks"] <= ticks
                        and frame["bus_cycles_delta"] > 0,
                        f"Frame crosses sustained scoring window: {label}")
    return cases


def compare_sustained(standard, baseline, candidate, limit):
    require(baseline["sustained"] == candidate["sustained"], f"Different sustained protocol: {standard}")
    for field in ("driver_sha256", "smoke_tool_sha256", "bus_hz", "audio_period_cycles"):
        require(baseline.get(field) == candidate.get(field), f"Different {field}: {standard}")
    old_cases, new_cases = (check_sustained(report, standard) for report in (baseline, candidate))
    results = []
    for name in sorted(SCENARIOS):
        old, new = old_cases[name], new_cases[name]
        require(old["ticks"] == new["ticks"] == 10 and old["palette"] == new["palette"] == "retained",
                f"Different fixed render work: {standard}/{name}")
        require(old["identity"] == new["identity"] and old["schedule"] == new["schedule"],
                f"Different identity or request stream: {standard}/{name}")
        phases = []
        for previous, current in zip(old["runs"], new["runs"]):
            require(previous["injections"] == current["injections"]
                    and previous["admissions"] == current["admissions"],
                    f"Different requested/admitted SFX stream: {standard}/{name}/{previous['cia_phase']}")
            before = summary(frame["bus_cycles_delta"] for frame in previous["frames"])
            after = summary(frame["bus_cycles_delta"] for frame in current["frames"])
            changes = {key: (after[key] / before[key] - 1) * 100 for key in ("median", "p95", "max")}
            phases.append(dict(cia_phase=previous["cia_phase"], baseline=before, candidate=after,
                               additional_frame_time_percent=changes,
                               passed=changes["median"] < limit and changes["p95"] < limit))
        before = summary(frame["bus_cycles_delta"] for run in old["runs"] for frame in run["frames"])
        after = summary(frame["bus_cycles_delta"] for run in new["runs"] for frame in run["frames"])
        changes = {key: (after[key] / before[key] - 1) * 100 for key in ("median", "p95", "max")}
        results.append(dict(scenario=name, phases=phases,
            baseline_median_ms=before["median"] * 1000 / baseline["bus_hz"],
            candidate_median_ms=after["median"] * 1000 / candidate["bus_hz"],
            baseline_p95_ms=before["p95"] * 1000 / baseline["bus_hz"],
            candidate_p95_ms=after["p95"] * 1000 / candidate["bus_hz"],
            baseline_max_ms=before["max"] * 1000 / baseline["bus_hz"],
            candidate_max_ms=after["max"] * 1000 / candidate["bus_hz"],
            baseline_statistics=old["statistics"], candidate_statistics=new["statistics"],
            additional_frame_time_percent=changes["median"], additional_p95_frame_time_percent=changes["p95"],
            passed=all(phase["passed"] for phase in phases)
                and changes["median"] < limit and changes["p95"] < limit))
    return results


def compare(baseline_dir, candidate_dir, limit):
    result = dict(passed=False, protocol="havoc-sfx-frame-comparison-v1",
        scope="Matched nine-sample native VICE frames including IRQ and SID work; excludes DAC service and physical hardware timing",
        maximum_additional_frame_time_percent=limit,
        gameplay_identity="All gameplay fields except audio, audio_head, audio_len; raw state hashes retained in source reports",
        standards={})
    for standard in ("pal", "ntsc"):
        reports = []
        sources = []
        for directory in (baseline_dir, candidate_dir):
            path = directory / f"bench-{standard}" / "report.json"
            raw = path.read_bytes()
            report = json.loads(raw)
            require(report.get("passed"), f"Failed benchmark report: {path}")
            require(report.get("video") == standard and report.get("sfx_workloads") is True,
                    f"Wrong benchmark mode: {path}")
            require(report.get("samples") == (3 if report.get("sustained") else 9), f"Wrong sample/phase count: {path}")
            require(report.get("benchmark_helpers", {}).get("verified_before_first_frame") is True,
                    f"Unverified benchmark helper load: {path}")
            sources.append(dict(path=str(path.resolve()), sha256=hashlib.sha256(raw).hexdigest(),
                                package_hashes=report["hashes"]))
            reports.append(report)
        baseline, candidate = reports
        for field in ("protocol", "fixture_sha256", "vice_sha256", "sid_count", "stage_count"):
            require(baseline.get(field) == candidate.get(field), f"Different {field}: {standard}")
        if baseline.get("protocol") == "havoc-sustained-sfx-v1":
            result["protocol"] = "havoc-sfx-sustained-comparison-v1"
            result["scope"] = "Three continuous30-second-or-longer controlled50Hz SFX runs per workload; identical repeated render/gameplay work, full request/admission streams; excludes DAC and physical hardware"
            result["standards"][standard] = dict(sources=sources,
                cases=compare_sustained(standard, baseline, candidate, limit))
            continue
        maps = [{case["fixture"]: case for case in report["cases"]} for report in reports]
        require(all(set(cases) == SCENARIOS and len(report["cases"]) == 5
                    for cases, report in zip(maps, reports)), f"Incomplete scenario set: {standard}")
        cases = []
        for name in sorted(SCENARIOS):
            old, new = (mapping[name] for mapping in maps)
            require(all(case["ticks"] == 10 and case["palette"] == "retained"
                        and len(case["samples"]) == 9 for case in (old, new)),
                    f"Different fixed work or sample count: {standard}/{name}")
            for index, (previous, current) in enumerate(zip(old["samples"], new["samples"])):
                for field in IDENTITY:
                    require(previous[field] == current[field], f"Different {field}: {standard}/{name}/{index}")
                require(previous.get("sfx_injections") == current.get("sfx_injections")
                        and "sfx_injections" in previous,
                        f"Different/missing actual SFX injection trace: {standard}/{name}/{index}")
            old_cycles = statistics.median(sample["bus_cycles_delta"] for sample in old["samples"])
            new_cycles = statistics.median(sample["bus_cycles_delta"] for sample in new["samples"])
            change = (new_cycles / old_cycles - 1) * 100
            cases.append(dict(scenario=name, samples=9,
                              baseline_median_ms=old_cycles * 1000 / baseline["bus_hz"],
                              candidate_median_ms=new_cycles * 1000 / candidate["bus_hz"],
                              additional_frame_time_percent=change, passed=change < limit))
        result["standards"][standard] = dict(sources=sources, cases=cases)
    result["passed"] = all(case["passed"] for standard in result["standards"].values() for case in standard["cases"])
    result["worst_additional_frame_time_percent"] = max(
        case["additional_frame_time_percent"] for standard in result["standards"].values() for case in standard["cases"])
    if result["protocol"] == "havoc-sfx-sustained-comparison-v1":
        result["worst_additional_p95_frame_time_percent"] = max(
            case["additional_p95_frame_time_percent"] for standard in result["standards"].values() for case in standard["cases"])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=Path("target/synth-performance.json"))
    parser.add_argument("--limit", type=float, default=1.0)
    args = parser.parse_args()
    require(0 < args.limit <= 1, "SFX acceptance limit must be positive and no greater than 1 percent")
    result = compare(args.baseline, args.candidate, args.limit)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    for name, standard in result["standards"].items():
        for case in standard["cases"]:
            suffix = (f", p95{case['additional_p95_frame_time_percent']:+.4f}%"
                      if "additional_p95_frame_time_percent" in case else "")
            print(f"{name} {case['scenario']}: {case['additional_frame_time_percent']:+.4f}% median frame time{suffix}")
    print("PASS" if result["passed"] else "FAIL", args.out)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
