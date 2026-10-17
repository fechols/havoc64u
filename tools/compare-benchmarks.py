#!/usr/bin/env python3
"""Diagnose same-fixture native differences; this does not waive quality gates."""
import argparse
import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("vice_smoke", ROOT / "tools/vice-smoke.py")
vice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vice)


def visible(sample):
    capture = sample["capture"]
    raw = Path(capture["native"]).read_bytes()
    pixels = []
    for y in range(200):
        for x in range(160):
            cell = y // 8 * 40 + x // 4
            slot = raw[cell * 8 + y % 8] >> (6 - 2 * (x % 4)) & 3
            pixels.append((capture["background"], raw[8000+cell] >> 4,
                           raw[8000+cell] & 15, raw[9000+cell])[slot])
    return raw, pixels


def geometry(sample):
    path = Path(sample["capture"]["native"]).with_name("geometry.json")
    return [dict(triangle, reserved=triangle["reserved"] & ~6) for triangle in json.loads(path.read_text())]


def neighborhood(pixels, x, y):
    return {pixels[row*160+column]
            for row in range(max(y-1, 0), min(y+2, 200))
            for column in range(max(x-1, 0), min(x+2, 160))}


def row_rounding(triangles):
    """Independent closed-form old Q8/new Q16 endpoints for every visible row."""
    allowed, errors, maximum = set(), [], 0

    def trunc_div(value, divisor):
        return (abs(value) // divisor) * (-1 if value < 0 else 1)

    for index, triangle in enumerate(triangles):
        for row in range(200):
            old, new = [], []
            for edge in range(3):
                other = (edge + 1) % 3
                ax, ay = triangle["x"][edge], triangle["y"][edge]
                bx, by = triangle["x"][other], triangle["y"][other]
                if ay > by:
                    ax, ay, bx, by = bx, by, ax, ay
                if not ay <= row < by:
                    continue
                dx, dy = bx - ax, by - ay
                start = max(ay, row // 8 * 8)
                fraction = ((2 * (start - ay) + 1) << 14) // dy
                q8 = (ax << 8) + ((dx * fraction) >> 7) + (row - start) * trunc_div(dx << 8, dy)
                start = max(ay, 0)
                fraction = ((2 * (start - ay) + 1) << 14) // dy
                q16 = (ax << 16) + 2 * dx * fraction + (row - start) * trunc_div(dx << 16, dy)
                old.append(max(0, min(160, (q8 + 127) >> 8)))
                new.append(max(0, min(160, (q16 + 32767) >> 16)))
            if len(old) not in (0, 2) or len(new) != len(old):
                errors.append({"triangle": index, "row": row, "old": old, "new": new})
                continue
            if not old:
                continue
            old.sort()
            new.sort()
            delta = max(abs(a-b) for a, b in zip(old, new))
            maximum = max(maximum, delta)
            if delta > 1:
                errors.append({"triangle": index, "row": row, "old": old, "new": new})
            allowed.update((x, row) for x in set(range(*old)) ^ set(range(*new)))
    return allowed, {"maximum_endpoint_delta": maximum, "invalid_rows": errors,
                     "edge_change_pixels": len(allowed)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    baseline, candidate = [json.loads(path.read_text()) for path in (args.baseline, args.candidate)]
    if baseline["video"] != candidate["video"]:
        parser.error("video standards differ")
    index = {(case["fixture"], case["ticks"], case["palette"]): case for case in baseline["cases"]}
    report = {"baseline": str(args.baseline), "candidate": str(args.candidate),
              "scope": "Difference diagnostics; one-pixel neighborhood is evidence, not automatic approval", "cases": []}
    for case in candidate["cases"]:
        key = case["fixture"], case["ticks"], case["palette"]
        if not case["samples"] or key not in index:
            continue
        a, b = index[key]["samples"][0], case["samples"][0]
        raw_a, pixels_a = visible(a)
        raw_b, pixels_b = visible(b)
        geom_a, geom_b = geometry(a), geometry(b)
        changed = [(offset % 160, offset // 160) for offset, pair in enumerate(zip(pixels_a, pixels_b)) if pair[0] != pair[1]]
        nonlocal_changes = [(x, y) for x, y in changed
                            if pixels_b[y*160+x] not in neighborhood(pixels_a, x, y)
                            or pixels_a[y*160+x] not in neighborhood(pixels_b, x, y)]
        differences = [{"index": n, "baseline": old, "candidate": new}
                       for n, (old, new) in enumerate(zip(geom_a, geom_b)) if old != new]
        allowed, rounding = row_rounding(geom_a)
        outside = [(x, y) for x, y in changed if (x, y) not in allowed]
        rounding.update(visible_changes_outside_edge_mask=len(outside), first_outside_pixels=outside[:32])
        result = {"fixture": key[0], "ticks": key[1], "palette": key[2],
                  "game_exact": a["game_sha256"] == b["game_sha256"],
                  "geometry_exact": geom_a == geom_b, "triangle_counts": [len(geom_a), len(geom_b)],
                  "geometry_difference_count": len(differences), "geometry_differences": differences[:32],
                  "native_exact": raw_a == raw_b, "visible_exact": pixels_a == pixels_b,
                  "visible_changed_pixels": len(changed), "radius_one_boundary_changes": len(changed)-len(nonlocal_changes),
                  "nonlocal_color_changes": len(nonlocal_changes), "first_nonlocal_pixels": nonlocal_changes[:32],
                  "first_changed_pixels": [{"x": x, "y": y, "baseline": pixels_a[y*160+x], "candidate": pixels_b[y*160+x]} for x, y in changed[:32]],
                  "row_rounding": rounding,
                  "changed_bounds": [min(x for x, y in changed), min(y for x, y in changed),
                                     max(x for x, y in changed), max(y for x, y in changed)] if changed else None,
                  "changed_bytes": {name: sum(old != new for old, new in zip(raw_a[start:end], raw_b[start:end]))
                                    for name, start, end in (("bitmap", 0, 8000), ("screen", 8000, 9000), ("color", 9000, 10000))}}
        if "median_cycles" in case:
            result["speedup"] = index[key]["median_cycles"]/case["median_cycles"]
        result["rounding_qualified"] = result["game_exact"] and result["geometry_exact"] and not rounding["invalid_rows"] and not outside
        report["cases"].append(result)
        changed_set, nonlocal_set = set(changed), set(nonlocal_changes)
        rows = []
        for y in range(200):
            row = bytearray()
            for x in range(160):
                row.extend((255, 0, 255) if (x, y) in nonlocal_set else (255, 0, 0) if (x, y) in changed_set else (24, 24, 24))
            rows.append(bytes(row))
        vice.png(args.out_dir / f"{key[0]}-{key[1]}-{key[2]}-difference.png", rows, 160, 200)
        print(f"{key[0]}/{key[1]}/{key[2]}: geometry={result['geometry_exact']} game={result['game_exact']} "
              f"pixels={len(changed)} nonlocal={len(nonlocal_changes)} speedup={result.get('speedup', 0):.3f}x")
    report["exact"] = bool(report["cases"]) and all(case["game_exact"] and case["geometry_exact"] and case["native_exact"] for case in report["cases"])
    (args.out_dir / "report.json").write_text(json.dumps(report, indent=2)+"\n")
    return 0 if report["exact"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
