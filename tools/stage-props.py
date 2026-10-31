#!/usr/bin/env python3
"""Convert reduced Ludo AI prop meshes into model-bank tables.

The props (ROCK and BUILDING obstacles) start as Ludo AI image-to-3D models
of objects from each stage's splash painting (see assets/stage-art/README.md
for the commands). This tool reduces each model to a C64-sized convex hull
and turns it into the game's convention:

  * rotate to face the approaching camera (-Z) and stand on y = 0;
  * keep the progressive convex hull of a few extreme points;
  * fit the footprint and height of the obstacle it replaces, so collision,
    clearance and shadow extents keep their meaning;
  * keep it a closed solid with every face wound outward (the model bank
    always culls back faces);
  * rank faces by the texture they cover, lit from above-left, and give
    them the prop's three art-directed C64 colors.

  python3 tools/stage-props.py                # print a summary and previews
  python3 tools/stage-props.py --write        # rewrite src/host/stage_props.rs (included by src/host.rs)

Requires Pillow and NumPy. Inputs (see assets/stage-art/README.md):
tools/asset-pipeline/ludo-output/models/
<name>_model-1.glb, as saved by `ludo model create`.
"""
from __future__ import annotations

import argparse
import importlib.util
import io
import itertools
import json
from pathlib import Path
import struct
import sys

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
MODELS = ROOT / "tools/asset-pipeline/ludo-output/models"
OUTPUT = ROOT / "src/host/stage_props.rs"

sys.path.insert(0, str(Path(__file__).parent))
_spec = importlib.util.spec_from_file_location("derive_stage_art", Path(__file__).with_name("derive-stage-art.py"))
_art = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_art)

# name: (theme, kind, minimum corner, maximum corner, hull points). Boxes
# match the obstacle meshes they replace, so collision, clearance and shadow
# extents keep their meaning; the cave and ash spires keep the 1.5x height
# (and the cave its 3/4 footprint) those stages already used. A closed hull
# of n points has at most 2n - 4 faces.
ROCK_BOX = ((-32, 0, -30), (35, 42, 30))
BUILDING_BOX = ((-22, 0, -22), (22, 60, 22))
TARGETS = {
    "t00_rock": (0, "ROCK", *ROCK_BOX, 10),
    "t01_rock": (1, "ROCK", *ROCK_BOX, 10),
    "t04_rock": (4, "ROCK", (-24, 0, -22), (26, 63, 22), 10),
    "t05_rock": (5, "ROCK", (-32, 0, -30), (35, 63, 30), 10),
    "t08_rock": (8, "ROCK", (-32, 0, -30), (35, 56, 30), 10),
    "t09_rock": (9, "ROCK", *ROCK_BOX, 10),
    "t02_building": (2, "BUILDING", *BUILDING_BOX, 12),
    "t03_building": (3, "BUILDING", *BUILDING_BOX, 12),
    "t06_building": (6, "BUILDING", *BUILDING_BOX, 12),
}
LIGHT = np.array([-0.45, 0.8, -0.4]) / np.linalg.norm([-0.45, 0.8, -0.4])
# Lit, middle and dark colors per prop, picked from each painting. Three
# colors keep a prop packable into multicolor cells over its ground.
RAMPS = {
    "t00_rock": (8, 2, 9),        # sandstone outcrop
    "t01_rock": (12, 11, 9),      # rust-streaked canyon boulder
    "t04_rock": (3, 14, 4),       # cyan crystal spires on violet rock
    "t05_rock": (12, 11, 0),      # ash-dusted basalt
    "t08_rock": (15, 11, 6),      # slate spire with pale edges
    "t09_rock": (8, 2, 0),        # charred meteorite, glowing cracks
    "t02_building": (15, 12, 6),  # steel tower with blue panels
    "t03_building": (15, 12, 11),  # concrete coastal bunker
    "t06_building": (8, 11, 0),   # furnace glow on dark iron
}


def read_glb(path: Path):
    """Positions, triangles, texture coordinates and base-color image."""
    data = path.read_bytes()
    assert data[:4] == b"glTF", path
    chunks, at = {}, 12
    while at < len(data):
        length, kind = struct.unpack_from("<II", data, at)
        chunks[kind] = data[at + 8:at + 8 + length]
        at += 8 + length
    gltf = json.loads(chunks[0x4E4F534A])
    binary = chunks[0x004E4942]
    sizes = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
    types = {5121: np.uint8, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}

    def accessor(index):
        a = gltf["accessors"][index]
        view = gltf["bufferViews"][a["bufferView"]]
        dtype = np.dtype(types[a["componentType"]])
        width = sizes[a["type"]]
        start = view.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = view.get("byteStride", width * dtype.itemsize)
        rows = [np.frombuffer(binary, dtype, width, start + i * stride) for i in range(a["count"])]
        return np.array(rows, dtype=np.float64)

    primitive = gltf["meshes"][0]["primitives"][0]
    positions = accessor(primitive["attributes"]["POSITION"])
    node = next(n for n in gltf["nodes"] if n.get("mesh") == 0)
    if "matrix" in node:
        matrix = np.array(node["matrix"]).reshape(4, 4).T
        positions = positions @ matrix[:3, :3].T + matrix[:3, 3]
    triangles = accessor(primitive["indices"]).astype(int).reshape(-1, 3)
    uvs = accessor(primitive["attributes"]["TEXCOORD_0"]) if "TEXCOORD_0" in primitive["attributes"] else None
    texture = None
    material = gltf["materials"][primitive.get("material", 0)]
    source = material.get("pbrMetallicRoughness", {}).get("baseColorTexture")
    if source is not None:
        image = gltf["images"][gltf["textures"][source["index"]]["source"]]
        view = gltf["bufferViews"][image["bufferView"]]
        start = view.get("byteOffset", 0)
        texture = Image.open(io.BytesIO(binary[start:start + view["byteLength"]])).convert("RGB")
    return positions, triangles, uvs, texture


def hull(points: np.ndarray, members: list[int]) -> list[list[int]]:
    """Outward triangles of the convex hull of a handful of points.

    Brute force suits the dozen points used here. Coplanar points (box sides)
    are gathered per plane and fanned once around their 2D hull."""
    eps = 1e-6 * np.ptp(points[members], axis=0).max()
    planes = {}
    for i, j, k in itertools.combinations(members, 3):
        n = np.cross(points[j] - points[i], points[k] - points[i])
        length = np.linalg.norm(n)
        if length < eps:
            continue
        n /= length
        side = (points[members] - points[i]) @ n
        if side.max() > eps * 10:
            if side.min() < -eps * 10:
                continue
            n = -n
            side = -side
        key = tuple(np.round(np.append(n, n @ points[i]), 4))
        on = [m for m, s in zip(members, side) if s > -eps * 10]
        planes.setdefault(key, (n, set()))[1].update(on)
    faces = []
    for n, on in planes.values():
        on = list(on)
        # Order the coplanar points around their centre, counter-clockwise
        # about the outward normal, and keep only the 2D hull corners.
        centre = points[on].mean(axis=0)
        u = points[on[0]] - centre
        u /= np.linalg.norm(u)
        v = np.cross(n, u)
        angles = [np.arctan2((points[m] - centre) @ v, (points[m] - centre) @ u) for m in on]
        ring = [m for _, m in sorted(zip(angles, on))]
        corners = []
        for m in ring + ring[:2]:
            while len(corners) >= 2:
                a, b = points[corners[-2]], points[corners[-1]]
                if np.cross(b - a, points[m] - b) @ n > eps:
                    break
                corners.pop()
            corners.append(m)
        corners = list(dict.fromkeys(corners))
        for t in range(1, len(corners) - 1):
            faces.append([corners[0], corners[t], corners[t + 1]])
    return faces


def progressive_hull(points: np.ndarray, count: int) -> tuple[list[int], list[list[int]]]:
    """Grow a hull from the extreme points, always adding the point farthest
    outside the current one, so spires and corners are kept first."""
    first = int(points[:, 1].argmax())
    second = int(np.linalg.norm(points - points[first], axis=1).argmax())
    line = points[second] - points[first]
    off = np.linalg.norm(np.cross(points - points[first], line), axis=1)
    third = int(off.argmax())
    n = np.cross(line, points[third] - points[first])
    fourth = int(np.abs((points - points[first]) @ n).argmax())
    members = [first, second, third, fourth]
    faces = hull(points, members)
    while len(members) < count:
        outside = np.full(len(points), -np.inf)
        for face in faces:
            a, b, c = points[face]
            n = np.cross(b - a, c - a)
            n /= np.linalg.norm(n)
            outside = np.maximum(outside, (points - a) @ n)
        best = int(outside.argmax())
        if outside[best] <= 0:
            break
        members.append(best)
        faces = hull(points, members)
    return members, faces


def normal(points: np.ndarray, face) -> np.ndarray:
    a, b, c = points[list(face)]
    n = np.cross(b - a, c - a)
    length = np.linalg.norm(n)
    return n / length if length else n


def convert(name: str) -> tuple[list[list[int]], list[list[int]], dict]:
    theme, kind, box_min, box_max, count = TARGETS[name]
    box_min, box_max = np.array(box_min, dtype=float), np.array(box_max, dtype=float)
    positions, triangles, uvs, texture = read_glb(MODELS / f"{name}_model-1.glb")
    # glTF fronts face +Z; obstacles face the camera, which looks along +Z.
    positions = positions * [-1, 1, -1]
    # Fit the replaced obstacle's box exactly; the base rests on y = 0.
    low, high = positions.min(axis=0), positions.max(axis=0)
    positions = box_min + (positions - low) * (box_max - box_min) / (high - low)

    # The model bank requires closed solids, so the underside stays. Refit
    # the box to the hull's own corners; per-axis scaling keeps it convex.
    members, faces = progressive_hull(positions, count)
    low, high = positions[members].min(axis=0), positions[members].max(axis=0)
    positions = box_min + (positions - low) * (box_max - box_min) / (high - low)

    # Average the texture over the original triangles nearest each face.
    centroids = np.array([positions[f].mean(axis=0) for f in faces])
    normals = np.array([normal(positions, f) for f in faces])
    sums = np.zeros((len(faces), 3))
    weights = np.zeros(len(faces))
    if texture is not None and uvs is not None:
        tex = np.asarray(texture, dtype=np.float64) / 255.0
        th, tw = tex.shape[:2]
        for tri in triangles:
            corners = positions[tri]
            n = np.cross(corners[1] - corners[0], corners[2] - corners[0])
            area = np.linalg.norm(n) / 2
            if area == 0:
                continue
            n /= 2 * area
            # Hull faces point outward; match each surface patch to the face
            # it sits under, preferring faces that look the same way.
            distance = np.abs(np.einsum("ij,ij->i", corners.mean(axis=0) - centroids, normals))
            fit = distance + np.linalg.norm(centroids - corners.mean(axis=0), axis=1) * 0.5 + (1 - normals @ n) * 20
            best = int(fit.argmin())
            for w in ((1 / 3, 1 / 3, 1 / 3), (0.6, 0.2, 0.2), (0.2, 0.6, 0.2), (0.2, 0.2, 0.6)):
                uv = np.array(w) @ uvs[tri]
                px = int(np.clip(uv[0] * tw, 0, tw - 1))
                py = int(np.clip(uv[1] * th, 0, th - 1))
                sums[best] += _art._splash.srgb_to_linear(tex[py, px]) * area
                weights[best] += area
    base = np.where(weights[:, None] > 0, sums / np.maximum(weights, 1e-9)[:, None], 0.2)
    light = 0.35 + 0.95 * np.clip(normals @ LIGHT, 0, 1)
    # Texture brightness under the light ranks the faces; thirds of that
    # ranking take the prop's lit, middle and dark colors.
    value = (base @ [0.2126, 0.7152, 0.0722]) * light
    rank = np.argsort(np.argsort(-value, kind="stable"), kind="stable")
    ramp = RAMPS[name]
    colors = [ramp[min(int(r * 3 // len(faces)), 2)] for r in rank]
    palette = ramp

    used = sorted({i for f in faces for i in f})
    lookup = {old: new for new, old in enumerate(used)}
    vertices = [[int(round(v)) for v in positions[i]] for i in used]
    table = [[lookup[f[0]], lookup[f[1]], lookup[f[2]], color] for f, color in zip(faces, colors)]
    return vertices, table, {"theme": theme, "kind": kind, "palette": palette}


def render(vertices, faces, size=200) -> Image.Image:
    """Flat-shaded preview from behind and above, as the player sees props."""
    image = Image.new("RGB", (size, size), (40, 40, 48))
    draw = ImageDraw.Draw(image)
    points = np.array(vertices, dtype=float)
    camera = np.array([0.0, 60.0, -150.0])
    forward = np.array([0.0, -0.3, 1.0]) / np.linalg.norm([0.0, -0.3, 1.0])
    right = np.cross([0.0, 1.0, 0.0], forward)
    right /= np.linalg.norm(right)
    up = np.cross(forward, right)
    relative = points - camera
    depth = relative @ forward
    screen = np.stack([size / 2 + (relative @ right) * 300 / depth, size / 2 - (relative @ up) * 300 / depth], axis=1)
    palette = [tuple(int(h[i:i + 2], 16) for i in (0, 2, 4)) for h in _art._splash.PALETTE_HEX]
    for a, b, c, color in sorted(faces, key=lambda f: -depth[list(f[:3])].mean()):
        n = normal(points, (a, b, c))
        if n @ (points[[a, b, c]].mean(axis=0) - camera) >= 0:
            continue  # Back face: the model bank culls it too.
        draw.polygon([tuple(q) for q in screen[[a, b, c]]], fill=palette[color])
    return image


def rust(results: dict) -> str:
    lines = [
        "// Stage obstacle meshes generated by tools/stage-props.py from Ludo AI",
        "// models of the stage splash paintings. Rerun the tool instead of editing.",
        "// Included by src/host.rs.",
        "",
        "/// The generated mesh replacing `kind` for `theme`, if there is one.",
        "fn stage_prop(theme: u8, kind: u8) -> Option<Model> {",
        "    let (vertices, faces): (&[[i16; 3]], &[[u8; 4]]) = match (theme, kind) {",
    ]
    for name, (vertices, faces, meta) in results.items():
        upper = name.upper()
        lines.append(f"        ({meta['theme']}, {meta['kind']}) => (&{upper}_VERTICES, &{upper}_FACES),")
    lines += ["        _ => return None,", "    };", "    Some((vertices.to_vec(), faces.to_vec()))", "}", ""]
    for name, (vertices, faces, meta) in results.items():
        upper = name.upper()
        lines.append("#[rustfmt::skip]")
        lines.append(f"static {upper}_VERTICES: [[i16; 3]; {len(vertices)}] = [")
        lines += ["    " + " ".join(f"[{x}, {y}, {z}]," for x, y, z in vertices[i:i + 6]) for i in range(0, len(vertices), 6)]
        lines.append("];")
        lines.append("#[rustfmt::skip]")
        lines.append(f"static {upper}_FACES: [[u8; 4]; {len(faces)}] = [")
        lines += ["    " + " ".join(f"[{a}, {b}, {c}, {d}]," for a, b, c, d in faces[i:i + 6]) for i in range(0, len(faces), 6)]
        lines.append("];")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="rewrite src/host/stage_props.rs")
    parser.add_argument("--preview", type=Path, help="write a preview sheet of every prop")
    args = parser.parse_args()
    results = {name: convert(name) for name in TARGETS}
    for name, (vertices, faces, meta) in results.items():
        print(f"{name}: {len(vertices)} vertices, {len(faces)} faces, colors {meta['palette']}")
    if args.preview:
        tiles = [render(v, f) for v, f, _ in results.values()]
        sheet = Image.new("RGB", (160 * len(tiles), 160))
        for i, tile in enumerate(tiles):
            sheet.paste(tile, (160 * i, 0))
        sheet.save(args.preview)
    if args.write:
        OUTPUT.parent.mkdir(exist_ok=True)
        OUTPUT.write_text(rust(results), encoding="utf-8", newline="\n")
        print(f"wrote {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
