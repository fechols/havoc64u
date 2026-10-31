#!/usr/bin/env python3
"""Derive the terrestrial stage backdrops and palettes from the stage art.

Each theme's flat reference (assets/stage-art/tNN_flat.png, a Ludo AI flat
rendition of that stage's splash painting) is matched to the C64 Ultimate
palette and traced into the existing SCENERY tables: sky, horizon and cloud
colors, the five-triangle horizon silhouette, three cloud centers with a
shared outline, and the terrain facet/ground colors. Table sizes and the
backdrop face count never change, so only data moves in the C64 image.

  python3 tools/derive-stage-art.py            # print the derived tables
  python3 tools/derive-stage-art.py --apply    # rewrite them in src/scenery.rs
  python3 tools/derive-stage-art.py --orbital  # rank the orbital references' colors

Requires Pillow and NumPy.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import re
import sys

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ART = ROOT / "assets/stage-art"
SCENERY = ROOT / "src/scenery.rs"

sys.path.insert(0, str(Path(__file__).parent))
_spec = importlib.util.spec_from_file_location("encode_splash", Path(__file__).with_name("encode-splash.py"))
_splash = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_splash)
PALETTE = np.array([[int(h[i:i + 2], 16) for i in (0, 2, 4)] for h in _splash.PALETTE_HEX]) / 255.0
PALETTE_LAB = _splash.linear_to_oklab(_splash.srgb_to_linear(PALETTE))

# Theme order matches scenery.rs: lowland, canyon, fortress, coast, crystal,
# ash, foundry, citadel, storm, meteor. Cities use block silhouettes.
THEMES = ("lowland", "canyon", "fortress", "coast", "crystal", "ash", "foundry", "citadel", "storm", "meteor")
SKYLINE = {2: "blocks", 6: "blocks", 7: "blocks"}
CAVE = 4
# Final color choices, picked from each painting with the derived rankings as
# candidates. Straight nearest-color matching turns muted paint into C64 grey
# and cannot tell a sea from a sky, so these are art-directed per theme:
# (sky, horizon [lit, shade], clouds [lit, shade], ground, ground shade,
#  terrain side facets [first, second]).
ART_DIRECTION = {
    0: (14, [15, 6], [1, 15], 5, 13, [5, 13]),    # blue sky, snowy ranges, green hills
    1: (8, [12, 11], [7, 10], 9, 11, [12, 8]),    # orange sky, sunlit grey cliffs
    2: (11, [15, 12], [15, 12], 12, 11, [12, 6]),  # storm grey, steel and blue towers
    3: (6, [12, 11], [10, 4], 14, 6, [11, 12]),   # sunset clouds over a blue sea
    4: (0, [6, 11], [3, 14], 6, 14, [11, 6]),     # indigo rock, cyan crystals, water
    5: (12, [11, 9], [11, 0], 11, 12, [15, 11]),  # smoke-grey sky and ash dunes
    6: (8, [11, 9], [7, 10], 11, 12, [8, 9]),     # furnace sky, dark mills, molten
    7: (11, [10, 2], [15, 12], 12, 15, [12, 2]),  # red-lit grey citadel
    8: (6, [11, 0], [1, 15], 14, 15, [11, 14]),   # dark peaks above a cloud sea
    9: (6, [10, 4], [15, 4], 12, 9, [9, 11]),     # violet dusk, glowing horizon
}
WIDTH, HEIGHT = 176, 132  # Analysis grid: one column per horizon table x unit.
CLOUD_DEPTHS = (96, 112, 127)
# Horizon tables use x*32 and y*16 at depth 2350: ~1.09 and ~0.91 pixels per unit.
X_PIXELS, Y_PIXELS = 80 * 32 / 2350, 133 * 16 / 2350
BASE = -34
PEAK_LIMIT = 36


def load(theme: int) -> np.ndarray:
    image = Image.open(ART / f"t{theme:02d}_flat.png").convert("RGB")
    # 160 screen pixels show table x -73..73; widen to the full -88..88 table.
    return np.asarray(image.resize((WIDTH, HEIGHT), Image.Resampling.BOX), dtype=np.float64) / 255.0


def indices(rgb: np.ndarray, saturation: float = 1.6) -> np.ndarray:
    """Nearest Ultimate palette entries; chroma is boosted so muted painted
    hues keep their color instead of collapsing onto the C64 greys."""
    lab = oklab(rgb).reshape(-1, 3)
    lab[:, 1:] *= saturation
    distance = ((lab[:, None, :] - PALETTE_LAB[None, :, :]) ** 2).sum(axis=2)
    return distance.argmin(axis=1).reshape(rgb.shape[:-1])


def ranked(colors: np.ndarray, exclude=()) -> list[int]:
    counts = np.bincount(colors.ravel(), minlength=16)
    return [int(c) for c in np.argsort(-counts, kind="stable") if counts[c] and c not in exclude]


def luma(color: int) -> float:
    return float(PALETTE_LAB[color][0])


def oklab(rgb: np.ndarray) -> np.ndarray:
    return _splash.linear_to_oklab(_splash.srgb_to_linear(rgb.reshape(-1, 3))).reshape(rgb.shape)


def open_sky(rgb: np.ndarray) -> np.ndarray:
    """Pixels matching the painted sky measured across the top rows."""
    lab = oklab(rgb)
    sky = np.median(lab[:6].reshape(-1, 3), axis=0)
    return np.sqrt(((lab - sky) ** 2).sum(axis=2)) < 0.08


def skyline(sky: np.ndarray) -> tuple[np.ndarray, int]:
    """Per-column top of the land silhouette and the image row of its foot.

    Land is whatever rises without a gap from the first fully solid row, so
    clouds floating in the sky never join the silhouette."""
    coverage = sky.mean(axis=1)
    solid = np.flatnonzero(coverage < 0.04)
    foot = int(solid[0]) if len(solid) else HEIGHT - 1
    top = np.full(WIDTH, foot, dtype=float)
    for x in range(WIDTH):
        y = foot
        while y > 0 and not sky[y - 1:y + 1, x].all():
            y -= 1
        top[x] = y
    top = np.convolve(np.pad(top, 2, mode="edge"), np.ones(5) / 5, mode="valid")
    return top, foot


def heights(top: np.ndarray, foot: int) -> np.ndarray:
    """Silhouette height in table units, rescaled into the backdrop's range."""
    height = (foot - top) * (200 / HEIGHT) / Y_PIXELS
    peak = height.max()
    if peak > PEAK_LIMIT:
        height *= PEAK_LIMIT / peak
    elif 0 < peak < 14:
        height *= 14 / peak
    return height


def peaks(height: np.ndarray) -> list[list[list[int]]]:
    xs = np.arange(WIDTH) - WIDTH // 2
    chosen: list[int] = []
    for x in np.argsort(-height, kind="stable"):
        if len(chosen) == 5:
            break
        if all(abs(int(x) - c) >= 26 for c in chosen):
            chosen.append(int(x))
    for x in np.linspace(10, WIDTH - 10, 5).astype(int):
        if len(chosen) < 5 and all(abs(int(x) - c) >= 12 for c in chosen):
            chosen.append(int(x))
    triangles = []
    for x in sorted(chosen):
        h = max(float(height[x]), 3.0)

        def reach(step: int) -> int:
            i = x
            while 0 < i < WIDTH - 1 and height[i] > h / 2:
                i += step
            return max(abs(i - x), 4)
        # Extend each half-height reach down the same slope to the base,
        # which sits below the horizon where the terrain hides it.
        rise = (h - BASE) / (h / 2)
        left = int(xs[x] - min(reach(-1) * rise, 60))
        right = int(xs[x] + min(reach(1) * rise, 60))
        triangles.append([[left, BASE], [int(xs[x]), int(round(h))], [right, BASE]])
    # The silhouette's foot must span the whole horizon with no sky gap:
    # widen each base until it meets the next and reaches both edges.
    triangles[0][0][0] = -88
    triangles[-1][2][0] = 88
    for before, after in zip(triangles, triangles[1:]):
        if after[0][0] > before[2][0]:
            middle = (before[2][0] + after[0][0]) // 2
            before[2][0], after[0][0] = middle + 2, middle - 2
    return clamp(triangles)


def blocks(height: np.ndarray) -> list[list[list[int]]]:
    xs = np.arange(WIDTH) - WIDTH // 2
    middle = WIDTH // 2
    spike = int(np.argmax(height[middle - 40:middle + 40])) + middle - 40
    gap = 10
    left_h = int(round(np.percentile(height[:spike - gap], 55)))
    right_h = int(round(np.percentile(height[spike + gap:], 55)))
    spike_h = int(round(max(height[spike], left_h + 6, right_h + 6)))
    lx, rx = int(xs[spike - gap]), int(xs[spike + gap])
    s0, s1, sx = int(xs[max(spike - 16, 0)]), int(xs[min(spike + 16, WIDTH - 1)]), int(xs[spike])
    return clamp([
        [[-88, BASE], [lx, left_h], [lx, BASE]],
        [[-88, BASE], [-88, left_h], [lx, left_h]],
        [[rx, BASE], [88, right_h], [88, BASE]],
        [[rx, BASE], [rx, right_h], [88, right_h]],
        [[s0, BASE], [sx, spike_h], [s1, BASE]],
    ])


def clamp(triangles):
    return [[[max(-88, min(88, x)), max(BASE, min(PEAK_LIMIT, y))] for x, y in tri] for tri in triangles]


def components(mask: np.ndarray) -> list[np.ndarray]:
    labels = np.zeros(mask.shape, dtype=int)
    blobs = []
    for y, x in zip(*np.nonzero(mask)):
        if labels[y, x]:
            continue
        label = len(blobs) + 1
        labels[y, x] = label
        stack, cells = [(y, x)], []
        while stack:
            cy, cx = stack.pop()
            cells.append((cy, cx))
            for ny, nx in ((cy + 1, cx), (cy - 1, cx), (cy, cx + 1), (cy, cx - 1)):
                if 0 <= ny < mask.shape[0] and 0 <= nx < mask.shape[1] and mask[ny, nx] and not labels[ny, nx]:
                    labels[ny, nx] = label
                    stack.append((ny, nx))
        blobs.append(np.array(cells))
    return blobs


def two(ranks: list[int], other: int) -> list[int]:
    return ranks[:2] if len(ranks) >= 2 else [ranks[0], other]


def derive(theme: int) -> dict:
    rgb = load(theme)
    colors = indices(rgb)
    sky = open_sky(rgb)
    sky_color = 0 if theme == CAVE else ranked(colors[:6])[0]
    top, foot = skyline(sky)
    rows = np.arange(HEIGHT)[:, None]
    clouds = (rows < top[None, :] - 1) & ~sky
    land = (rows >= top[None, :]) & (rows <= foot)
    ground = two(ranked(colors[min(foot + 4, HEIGHT - 1):], exclude=(sky_color,)) or [5], 11)
    horizon = two(ranked(colors[land], exclude=(sky_color,)) or [ground[0]], 11)
    cloud_rank = ranked(colors[clouds], exclude=(sky_color,)) or [1]
    cloud_rank = two(cloud_rank, 15 if cloud_rank[0] != 15 else 12)

    result = {
        "theme": THEMES[theme],
        "sky": sky_color,
        # Face parity: even faces take the lit color, odd faces the shade.
        "horizon_colors": sorted(horizon, key=luma, reverse=True),
        "cloud_colors": sorted(cloud_rank, key=luma, reverse=True),
        "terrain": ground,
        "ground": ground[0],
        "ground_shade": ground[1],
    }
    result["derived"] = {key: result[key] for key in ("sky", "horizon_colors", "cloud_colors", "terrain")}
    sky_color, horizon, clouds_pick, ground_color, shade, terrain = ART_DIRECTION[theme]
    result.update(sky=sky_color, horizon_colors=horizon, cloud_colors=clouds_pick,
                  ground=ground_color, ground_shade=shade, terrain=terrain)
    if theme == CAVE:
        # The cave mouth geometry is engine-specific; only its colors change.
        return result
    height = heights(top, foot)
    triangles = blocks(height) if SKYLINE.get(theme) == "blocks" else peaks(height)
    result["horizon"] = [vertex for tri in triangles for vertex in tri]

    blobs = sorted((b for b in components(clouds) if len(b) >= 8), key=len, reverse=True)[:3]
    blobs.sort(key=lambda b: b[:, 1].mean())
    widths = [np.ptp(b[:, 1]) + 1 for b in blobs] or [20]
    tall = [np.ptp(b[:, 0]) + 1 for b in blobs] or [5]
    half = float(np.clip(np.mean(widths) / 2 * X_PIXELS * 112 / 80, 18, 46))
    rise = float(np.clip(np.mean(tall) * (200 / HEIGHT) * 112 * 2 / 133, 6, 16))
    r = lambda v: int(round(v))
    outline = [[-r(half), 0], [-r(half * 0.6), r(rise * 0.7)], [r(half * 0.2), r(rise)], [r(half), 1], [r(half * 0.75), -r(rise * 0.5)]]
    result["cloud_outline"] = outline
    xs = [x for x, _ in outline]
    ys = [y for _, y in outline]
    defaults = ([-50, 0.5], [8, 0.8], [60, 0.6])
    centers = []
    for index, depth in enumerate(CLOUD_DEPTHS):
        # Clouds use x*32 and y*16 at depth*32 without the camera lift:
        # screen x = 80 + 80*x/depth, screen y = 100 - 66.5*y/depth. Every
        # corner stays on screen, below the attract labels (row 24) and
        # above the horizon (row 90).
        low_y = 11 * depth / 66.5 - min(ys) + 1
        high_y = min(75 * depth / 66.5 - max(ys) - 1, 127)
        reach_x = 74 * depth / 80
        if index < len(blobs):
            b = blobs[index]
            x = (b[:, 1].mean() - WIDTH // 2) * X_PIXELS * depth / 80
            y = ((foot - b[:, 0].mean()) * (200 / HEIGHT) + 2) * depth / 66.5
        else:
            x = defaults[index][0] * depth / 80
            y = low_y + (high_y - low_y) * defaults[index][1]
        centers.append([int(np.clip(round(x), -reach_x - min(xs), reach_x - max(xs))),
                        int(np.clip(round(y), np.ceil(low_y), np.floor(high_y))), depth])
    result["cloud_centers"] = centers
    return result


def rust_rows(rows, fmt) -> list[str]:
    return [fmt(row) for row in rows]


def pairs(points) -> str:
    return ", ".join(f"[{a}, {b}]" for a, b in points)


def replace_table(text: str, name: str, rows: list[str | None], one_line: bool = False) -> str:
    pattern = re.compile(rf"(static {name}: [^=]+= )(\[.*?\n?\];)", re.S)
    match = pattern.search(text)
    assert match, name
    body = match.group(2)
    if one_line:
        replacement = "[\n    " + ", ".join(rows) + ",\n];"
    else:
        lines = body.split("\n")
        out, row = [], 0
        for line in lines:
            stripped = line.strip()
            if stripped.startswith("[") and stripped.endswith("],") and line.startswith("    "):
                if rows[row] is not None:
                    line = "    " + rows[row] + ","
                row += 1
            out.append(line)
        assert row == len(rows), (name, row)
        replacement = "\n".join(out)
    return text[: match.start(2)] + replacement + text[match.end(2):]


def apply(results: list[dict]) -> None:
    text = SCENERY.read_text(encoding="utf-8").replace("\r\n", "\n")
    text = replace_table(text, "SKY_COLORS", [str(r["sky"]) for r in results], one_line=True)
    text = re.sub(r"static SKY_COLORS: \[u8; 10\] = \[\n    (.*?),\n\];", r"static SKY_COLORS: [u8; 10] = [\1];", text)
    text = replace_table(text, "TERRAIN_PALETTES", [f"0x{r['terrain'][1]:02x}{r['terrain'][0]:02x}" for r in results], one_line=True)
    text = replace_table(text, "GROUND_COLORS", [str(r["ground"]) for r in results], one_line=True)
    text = re.sub(r"static GROUND_COLORS: \[u8; 10\] = \[\n    (.*?),\n\];", r"static GROUND_COLORS: [u8; 10] = [\1];", text)
    text = replace_table(text, "GROUND_SHADES", [str(r["ground_shade"]) for r in results], one_line=True)
    text = re.sub(r"static GROUND_SHADES: \[u8; 10\] = \[\n    (.*?),\n\];", r"static GROUND_SHADES: [u8; 10] = [\1];", text)
    text = replace_table(text, "HORIZON_COLORS", [f"[{a}, {b}]" for a, b in (r["horizon_colors"] for r in results)])
    text = replace_table(text, "CLOUD_COLORS", [f"[{a}, {b}]" for a, b in (r["cloud_colors"] for r in results)])
    text = replace_table(text, "HORIZONS", [f"[{pairs(r['horizon'])}]" if "horizon" in r else None for r in results])
    text = replace_table(text, "CLOUD_CENTERS", [
        "[" + ", ".join(f"[{x}, {y}, {d}]" for x, y, d in r["cloud_centers"]) + "]" if "cloud_centers" in r else None
        for r in results
    ])
    text = replace_table(text, "CLOUD_OUTLINES", [f"[{pairs(r['cloud_outline'])}]" if "cloud_outline" in r else None for r in results])
    SCENERY.write_text(text, encoding="utf-8", newline="\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--apply", action="store_true", help="rewrite the tables in src/scenery.rs")
    parser.add_argument("--json", type=Path, help="also write the derived values as JSON")
    parser.add_argument("--orbital", action="store_true", help="rank the orbital references' colors instead")
    args = parser.parse_args()
    if args.orbital:
        # Orbital scenery is authored in src/scenery/space.rs; its colors are
        # picked by hand from these candidates (black is the fixed sky).
        for theme in range(len(THEMES), len(THEMES) + 6):
            counts = np.bincount(indices(load(theme)).ravel(), minlength=16) / (WIDTH * HEIGHT)
            ranked = [f"{c}:{counts[c]:.0%}" for c in np.argsort(-counts, kind="stable") if counts[c] >= 0.01]
            print(f"t{theme}: {' '.join(ranked)}")
        return
    results = [derive(theme) for theme in range(len(THEMES))]
    for r in results:
        print(json.dumps(r))
    if args.json:
        args.json.write_text(json.dumps(results, indent=1) + "\n", encoding="utf-8")
    if args.apply:
        apply(results)
        print(f"updated {SCENERY.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
