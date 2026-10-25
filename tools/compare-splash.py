#!/usr/bin/env python3
"""Compare decoded splash payloads, their spatial error, and review crops.

This tool never re-encodes either input. It requires the same NumPy/Pillow
environment as encode-splash.py; all outputs are disposable review artifacts.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont


SPEC = importlib.util.spec_from_file_location("splash", Path(__file__).with_name("encode-splash.py"))
splash = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(splash)

# Coordinates are logical 160x200 pixels. Enlarged crops use 5x3 physical
# pixels per logical pixel, matching the 640x480 preview's 5:3 pixel aspect.
CROPS = {
    "Title": (20, 0, 140, 55),
    "Ship": (10, 70, 128, 156),
    "Sky": (70, 55, 135, 80),
    "Explosion": (120, 95, 160, 155),
}
FLAT_EDGE_LIMIT = 0.02


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(splash.ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def residual_texture(linear: np.ndarray, indices: np.ndarray) -> dict:
    """Describe residual roughness in source-flat areas; not an acceptance test.

    A useful dither can score worse here while improving spatial color error.
    Only neighbor pairs whose source endpoints are both flat are measured.
    """
    source_lab = splash.linear_to_oklab(linear)
    # edge_weights = 1 + min(3, 12 * max four-neighbor Oklab distance).
    flat = splash.edge_weights(source_lab) <= 1.0 + 12.0 * FLAT_EDGE_LIMIT
    residual = splash.LAB_PALETTE[indices] - source_lab
    squared_sum = 0.0
    count = 0
    for axis in (0, 1):
        if axis == 0:
            mask = flat[:-1] & flat[1:]
        else:
            mask = flat[:, :-1] & flat[:, 1:]
        differences = np.diff(residual, axis=axis)
        squared_sum += float(np.sum(np.sum(differences ** 2 * splash.METRIC, axis=2)[mask]))
        count += int(np.count_nonzero(mask))
    return {
        "source_edge_limit_oklab": FLAT_EDGE_LIMIT,
        "neighbor_pairs": count,
        "mean_squared_residual_difference": squared_sum / count if count else None,
    }


def full_comparison(images: list[Image.Image], labels: list[str]) -> Image.Image:
    margin, heading, width, height = 16, 28, 640, 480
    canvas = Image.new("RGB", (width * 2 + margin * 3, height + heading + margin * 2), "#171b22")
    draw = ImageDraw.Draw(canvas)
    font = ImageFont.load_default(size=17)
    for column, (image, label) in enumerate(zip(images, labels)):
        x = margin + column * (width + margin)
        draw.text((x, margin), label, fill="white", font=font)
        canvas.paste(image.resize((width, height), Image.Resampling.NEAREST), (x, margin + heading))
    return canvas


def crop_comparison(images: list[Image.Image], labels: list[str]) -> Image.Image:
    margin, heading, column_width = 16, 28, 640
    heights = [(box[3] - box[1]) * 3 for box in CROPS.values()]
    canvas = Image.new("RGB", (column_width * 2 + margin * 3,
                                margin + heading + sum(height + heading + margin for height in heights)),
                       "#171b22")
    draw = ImageDraw.Draw(canvas)
    font = ImageFont.load_default(size=17)
    for column, label in enumerate(labels):
        draw.text((margin + column * (column_width + margin), margin), label, fill="white", font=font)
    y = margin + heading
    for (name, box), height in zip(CROPS.items(), heights):
        for column, image in enumerate(images):
            x = margin + column * (column_width + margin)
            draw.text((x, y), name, fill="#c5cedc", font=font)
            crop = image.crop(box).resize(((box[2] - box[0]) * 5, height), Image.Resampling.NEAREST)
            canvas.paste(crop, (x, y + heading))
        y += heading + height + margin
    return canvas


def seconds(value: str) -> float:
    result = float(value)
    if not np.isfinite(result) or result < 0:
        raise argparse.ArgumentTypeError("elapsed seconds must be finite and nonnegative")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True, help="original packed .bin")
    parser.add_argument("--candidate", type=Path, required=True, help="candidate packed .bin")
    parser.add_argument("--source", type=Path, default=splash.SOURCE)
    parser.add_argument("--output", type=Path, default=splash.ROOT / "target/splash-quality/comparison")
    parser.add_argument("--baseline-label", default="Legacy")
    parser.add_argument("--candidate-label", default="Spatial")
    parser.add_argument("--baseline-seconds", type=seconds, help="optional measured conversion time")
    parser.add_argument("--candidate-seconds", type=seconds, help="optional measured conversion time")
    args = parser.parse_args()

    linear = splash.downsample(args.source)
    objective = splash.spatial_objective(linear)
    report = {
        "format": "havoc-splash-comparison-v1",
        "source": {"path": display_path(args.source), "sha256": splash.sha256(args.source.read_bytes()),
                   "reference": "full-composition linear-sRGB Lanczos at 160x200"},
        "scoring": {"converter_sha256": splash.script_sha256(),
                    "spatial_module_sha256": splash.sha256(
                        Path(__file__).with_name("splash_spatial.py").read_bytes().replace(b"\r\n", b"\n")),
                    "texture_note": "Descriptive residual roughness; useful dithering may increase it."},
        "crop_bounds_xyxy": CROPS,
    }
    images = []
    for name, path, elapsed in (("baseline", args.baseline, args.baseline_seconds),
                                ("candidate", args.candidate, args.candidate_seconds)):
        payload = path.read_bytes()
        indices = splash.decode(payload)
        report[name] = {
            "path": display_path(path),
            "sha256": splash.sha256(payload),
            "bytes": len(payload),
            "background_index": payload[-1],
            "spatial_error": {key: float(value) for key, value in objective.score(indices).items()},
            "flat_area_residual_texture": residual_texture(linear, indices),
        }
        if elapsed is not None:
            report[name]["conversion_seconds"] = elapsed
        images.append(Image.fromarray(splash.PALETTE[indices]))
    baseline = report["baseline"]["spatial_error"]["total"]
    candidate = report["candidate"]["spatial_error"]["total"]
    report["spatial_error_reduction_percent"] = 100.0 * (baseline - candidate) / baseline if baseline else None
    args.output.mkdir(parents=True, exist_ok=True)
    labels = [args.baseline_label, args.candidate_label]
    full_comparison(images, labels).save(args.output / "comparison.png")
    crop_comparison(images, labels).save(args.output / "crops.png")
    (args.output / "report.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(f"Spatial error: baseline={baseline:.10f}, candidate={candidate:.10f}")
    print(f"Comparison artifacts: {args.output}")


if __name__ == "__main__":
    main()
