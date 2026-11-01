#!/usr/bin/env python3
"""Fit the orbital stages' scenery and hazard geometry to Ludo AI models.

The six orbital stages (themes 12..17) draw their scenery from small vertex
tables. Each table keeps its vertex count, face list and budget; only the
coordinates come from Ludo AI image-to-3D models of objects in that stage's
splash painting (see assets/stage-art/README.md):

  * tNN_asteroid: the stage's tumbling ROCK hazard (src/host/space_art.rs).
    Its axis extremes stay exactly where the pinned collision, fragment and
    shadow bounds put them; the other coordinates of those six points and
    the relief of the six raised facets follow the model.
  * t12_asteroid: the shape of the scenery rocks drifting past the asteroid
    belt and the moon (ROCK_SHAPE in src/scenery/space.rs).
  * t14_wreck / t16_hull: the graveyard wrecks and the parked carrier hulls
    (GRAVEYARD_HULL_POINTS / HULL_POINTS), inside the original hull's box.

Every point moves to the model's extreme point in the direction of the
original vertex (a support-function fit), then the whole shape is blended
back toward the original just far enough that every facet still faces
outward, so the shells stay closed and cull correctly.

  python3 tools/derive-space-art.py           # print the fitted tables
  python3 tools/derive-space-art.py --write   # rewrite them in the sources

Requires NumPy and Pillow (through tools/stage-props.py's GLB reader).
"""
from __future__ import annotations

import argparse
import importlib.util
from pathlib import Path
import re
import sys

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
MODELS = ROOT / "tools/asset-pipeline/ludo-output/models"
SPACE = ROOT / "src/scenery/space.rs"
SPACE_ART = ROOT / "src/host/space_art.rs"

sys.path.insert(0, str(Path(__file__).parent))
_spec = importlib.util.spec_from_file_location("stage_props", Path(__file__).with_name("stage-props.py"))
_props = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_props)

# The original tumbling hazards (themes 12..17). Their axis extremes are the
# pinned bounds in tests/assets.rs; vertex order is -z, -x, +x, +y, -y, +z.
ROCKS = [
    [[0, 0, -35], [-38, -5, 0], [31, 5, 0], [0, 37, 0], [0, -37, 0], [0, 0, 35]],
    [[0, 0, -27], [-34, 0, 0], [34, 0, 0], [0, 46, 4], [0, -46, -4], [0, 0, 27]],
    [[-8, 0, -43], [-45, 0, -12], [45, 0, 12], [0, 19, 5], [0, -19, -5], [8, 0, 43]],
    [[0, 0, -38], [-28, 0, 0], [28, 0, 0], [0, 29, 0], [0, -29, 0], [0, 0, 38]],
    [[0, 0, -45], [-35, 0, -10], [35, 0, 10], [0, 22, 0], [0, -22, 0], [0, 0, 45]],
    [[0, 0, -32], [-40, 0, 0], [40, 0, 0], [0, 40, 0], [0, -40, 0], [0, 0, 32]],
]
AXES = [(2, -1), (0, -1), (0, 1), (1, 1), (1, -1), (2, 1)]
# flight_mesh's eight coarse faces; the first six get raised centers.
COARSE = [[0, 1, 3], [0, 3, 2], [0, 4, 1], [0, 2, 4], [1, 5, 3], [3, 5, 2], [1, 4, 5], [2, 5, 4]]
RELIEF_ONE = 64  # relief units per 1.0x of the weighted face centroid

# Scenery rock template (units of the radius / 128) and its shell faces.
ROCK_SHAPE = [[-32, 128, 0], [32, -128, 0], [-128, 16, 0], [-43, -32, 128],
              [96, 0, 64], [128, -16, 0], [43, 32, -128], [-96, 0, -64]]
RING = [2, 3, 4, 5, 6, 7]
ROCK_FACES = [f for i, a in enumerate(RING) for b in [RING[(i + 1) % 6]] for f in ([0, a, b], [1, b, a])]
FAR_RING = [2, 3, 5, 6]
ROCK_FACES += [f for i, a in enumerate(FAR_RING) for b in [FAR_RING[(i + 1) % 4]] for f in ([0, a, b], [1, b, a])]

HULL = [[-100, 0, 0], [100, 0, 0], [65, 90, 620], [-65, 90, 620],
        [65, -70, 620], [-65, -70, 620], [0, -120, -100], [210, 140, 770]]
HULL_FACES = [[0, 3, 2], [0, 2, 1], [0, 6, 5], [0, 5, 3], [1, 2, 4], [1, 4, 6],
              [0, 1, 6], [6, 4, 5], [3, 5, 4], [3, 4, 2]]  # the fin (7) is double-sided


def load(name: str) -> np.ndarray:
    positions, triangles, _, _ = _props.read_glb(MODELS / f"{name}_model-1.glb")
    used = np.unique(triangles)
    # glTF fronts face +Z; scenery faces the camera, which looks along +Z.
    return positions[used] * [-1, 1, -1]


def to_box(points: np.ndarray, low, high) -> np.ndarray:
    lo, hi = points.min(axis=0), points.max(axis=0)
    return np.asarray(low, float) + (points - lo) * (np.asarray(high, float) - low) / (hi - lo)


def support(points: np.ndarray, center, direction) -> np.ndarray:
    d = np.asarray(direction, float)
    d = d / (np.linalg.norm(d) or 1)
    return points[int(np.argmax((points - center) @ d))]


def outward(vertices, faces, center) -> bool:
    v = np.asarray(vertices, float) - center
    for a, b, c in faces:
        n = np.cross(v[b] - v[a], v[c] - v[a])
        if n @ (v[a] + v[b] + v[c]) <= 0:
            return False
    return True


def blend(original, fitted, faces, center, integral=True):
    """The most faithful mix of fitted and original that keeps every face outward."""
    original, fitted = np.asarray(original, float), np.asarray(fitted, float)
    for alpha in np.linspace(1, 0, 21):
        mix = original + alpha * (fitted - original)
        if integral:
            mix = np.round(mix)
        if outward(mix, faces, center):
            return mix.astype(int), float(alpha)
    return np.asarray(original, int), 0.0


def tumbling_rock(level: int, model: np.ndarray):
    rock = np.array(ROCKS[level], float)
    low = np.array([rock[1][0], rock[4][1], rock[0][2]])
    high = np.array([rock[2][0], rock[3][1], rock[5][2]])
    shape = to_box(model, low, high)
    center = (low + high) / 2
    fitted = rock.copy()
    for k, (axis, sign) in enumerate(AXES):
        direction = np.zeros(3)
        direction[axis] = sign
        q = support(shape, center, direction)
        for other in range(3):
            if other == axis:
                continue
            # Keep the extremes exact: off-axis offsets stay within 45% of the
            # half extent so no other vertex can become a new extreme.
            half = (high[other] - low[other]) / 2 * 0.45
            fitted[k][other] = np.clip(q[other] - center[other], -half, half)
    fitted[3][1], fitted[4][1] = high[1], low[1]
    vertices, alpha = blend(rock, fitted, COARSE, np.zeros(3))
    raw, limits = [], []
    for index, (a, b, c) in enumerate(COARSE[:6]):
        variation = (level + index) % 3
        weights = np.array([3 + variation, 4, 5 - variation], float)
        corners = vertices[[a, b, c]].astype(float)
        centroid = weights @ corners / 12
        q = support(shape, center, centroid)
        raw.append(float(np.dot(q - center, centroid) / np.dot(centroid, centroid)))
        # Never past the pinned bounds.
        limits.append(min((high[axis] - 1) / centroid[axis] if centroid[axis] > 0
                          else (low[axis] + 1) / centroid[axis]
                          for axis in range(3) if abs(centroid[axis]) > 1e-6))
    # The model's relative bulges, spread over 1.15x..1.5x of each centroid:
    # always raised beyond the face plane, never puffier than the originals.
    raw = np.array(raw)
    span = raw.max() - raw.min()
    scaled = 1.15 + 0.35 * ((raw - raw.min()) / span if span > 1e-6 else np.full(6, 0.5))
    relief = [int(np.floor(min(f, limit) * RELIEF_ONE)) for f, limit in zip(scaled, limits)]
    return vertices.tolist(), relief, alpha


def scenery_rock(model: np.ndarray):
    shape = to_box(model, [-128] * 3, [128] * 3)
    original = np.array(ROCK_SHAPE, float)
    fitted = np.array([support(shape, np.zeros(3), p) for p in original])
    # Keep the template's unit span so the three radii keep their meaning.
    fitted *= 128 / np.abs(fitted).max()
    return blend(original, fitted, ROCK_FACES, np.zeros(3))


def hull(model: np.ndarray):
    # Principal axes: the longest becomes z, the one nearest the model's up
    # axis becomes y. The nose (the narrower end) goes toward low z.
    centered = model - model.mean(axis=0)
    _, _, axes = np.linalg.svd(centered, full_matrices=False)
    long_axis = axes[0]
    rest = axes[1:]
    up = rest[int(np.argmax(np.abs(rest[:, 1])))]
    up = up * np.sign(up[1] or 1)
    side = np.cross(up, long_axis)
    m = centered @ np.array([side, up, long_axis]).T
    lo, hi = m[:, 2].min(), m[:, 2].max()
    ends = [m[m[:, 2] < lo + (hi - lo) * 0.15], m[m[:, 2] > hi - (hi - lo) * 0.15]]
    area = [np.ptp(e[:, 0]) * np.ptp(e[:, 1]) for e in ends]
    if area[1] < area[0]:
        m[:, 2] = -m[:, 2]
    original = np.array(HULL, float)
    # Fit the body (the fin tip is a separate marker) and search in box-unit
    # space: on a long hull a plain direction would always find the tail tip.
    low, high = original[:7].min(axis=0), original[:7].max(axis=0)
    shape = to_box(m, low, high)
    center = (low + high) / 2
    half = (high - low) / 2
    unit = (shape - center) / half
    fitted = np.array([center + half * unit[int(np.argmax(unit @ ((p - center) / half)))]
                       for p in original])
    fitted[7] = original[7]  # the fin tip keeps its silhouette marker
    # Keep the port and starboard points mirrored about the keel so the
    # silhouette stays a ship: proportions and taper come from the model.
    for a, b in ((0, 1), (2, 3), (4, 5)):
        width = (abs(fitted[a][0] - center[0]) + abs(fitted[b][0] - center[0])) / 2
        y, z = (fitted[a][1] + fitted[b][1]) / 2, (fitted[a][2] + fitted[b][2]) / 2
        for k in (a, b):
            fitted[k] = [center[0] + np.sign(original[k][0] - center[0]) * width, y, z]
    fitted[6][0] = center[0]
    # The keel point stays the prow: frontmost, ahead of the bow corners.
    fitted[6][2] = min(fitted[6][2], min(fitted[0][2], fitted[1][2]) - 20)
    # The fin keeps its original offset from the rear deck corners it joins.
    fitted[7] = original[7] + (fitted[2] + fitted[3]) / 2 - (original[2] + original[3]) / 2
    return blend(original, fitted, HULL_FACES, original[:7].mean(axis=0))


def rust_points(rows, width=3) -> str:
    return ", ".join("[" + ", ".join(str(int(v)) for v in row[:width]) + "]" for row in rows)


def write_space_art(rocks) -> None:
    lines = [
        "// Orbital hazard shapes generated by tools/derive-space-art.py from Ludo AI",
        "// models of the stage splash paintings. Rerun the tool instead of editing.",
        "// Included by src/host.rs.",
        "",
        "/// The six tumbling hazards (themes 12..17): octahedron points (-z, -x,",
        "/// +x, +y, -y, +z) and the raised relief of the first six coarse faces,",
        "/// in 1/64ths of each face's weighted centroid.",
        "#[rustfmt::skip]",
        "const SPACE_ROCKS: [([[i16; 3]; 6], [u16; 6]); 6] = [",
    ]
    for vertices, relief, _ in rocks:
        lines.append(f"    ([{rust_points(vertices)}], {relief}),")
    lines.append("];")
    SPACE_ART.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def replace_static(text: str, name: str, body: str) -> str:
    pattern = re.compile(rf"((?:static|const) {name}: [^=]+= )\[.*?\n\];", re.S)
    assert pattern.search(text), name
    return pattern.sub(lambda m: m.group(1) + body, text, count=1)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="rewrite the generated tables")
    args = parser.parse_args()
    rocks = [tumbling_rock(level, load(f"t{12 + level}_asteroid")) for level in range(6)]
    for level, (vertices, relief, alpha) in enumerate(rocks):
        print(f"t{12 + level} hazard (fit {alpha:.2f}): {vertices} relief {relief}")
    shape, rock_alpha = scenery_rock(load("t12_asteroid"))
    print(f"scenery rock (fit {rock_alpha:.2f}): {shape.tolist()}")
    wreck, wreck_alpha = hull(load("t14_wreck"))
    carrier, carrier_alpha = hull(load("t16_hull"))
    print(f"graveyard wreck (fit {wreck_alpha:.2f}): {wreck.tolist()}")
    print(f"carrier hull (fit {carrier_alpha:.2f}): {carrier.tolist()}")
    if args.write:
        write_space_art(rocks)
        text = SPACE.read_text(encoding="utf-8").replace("\r\n", "\n")
        rows = lambda points: "[\n" + "".join(f"    [{', '.join(str(v) for v in p)}],\n" for p in points) + "];"
        text = replace_static(text, "ROCK_SHAPE", rows(shape.tolist()))
        text = replace_static(text, "GRAVEYARD_HULL_POINTS", rows(wreck.tolist()))
        text = replace_static(text, "HULL_POINTS", rows(carrier.tolist()))
        SPACE.write_text(text, encoding="utf-8", newline="\n")
        print(f"updated {SPACE_ART.relative_to(ROOT)} and {SPACE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
