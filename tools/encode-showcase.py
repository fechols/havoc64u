#!/usr/bin/env python3
"""Encode native capture manifests without speeding up the game or replacing audio.

Requires FFmpeg and FFprobe. Under WSL, Windows .exe paths are supported too.
Intermediate files and the verification report stay beside the capture manifest.
"""
import argparse
import bisect
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import zlib


def external_path(path, executable):
    path = Path(path).resolve()
    if os.name != "nt" and str(executable).lower().endswith(".exe"):
        return subprocess.check_output(["wslpath", "-w", str(path)], text=True).strip().replace("\\", "/")
    return str(path)


def executable(value, name):
    found = value or shutil.which(name) or shutil.which(name + ".exe")
    if not found:
        candidate = Path("/mnt/c/ffmpeg-8.1/bin") / (name + ".exe")
        if candidate.is_file():
            found = str(candidate)
    if not found:
        raise RuntimeError(f"Cannot find {name}; pass --{name}")
    return found


def native_rgb(path):
    """Read the unfiltered RGB PNGs emitted by vice-smoke's native decoder."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"Not a PNG: {path}")
    offset, compressed, dimensions = 8, bytearray(), None
    while offset < len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        kind, payload = data[offset + 4:offset + 8], data[offset + 8:offset + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
            if (width, height, depth, color, compression, filtering, interlace) != (160, 200, 8, 2, 0, 0, 0):
                raise ValueError(f"Unexpected native PNG format: {path}")
            dimensions = (width, height)
        elif kind == b"IDAT":
            compressed.extend(payload)
        offset += length + 12
    if dimensions is None:
        raise ValueError(f"Missing PNG dimensions: {path}")
    rows = zlib.decompress(compressed)
    stride = 160 * 3 + 1
    if len(rows) != stride * 200 or any(rows[y * stride] for y in range(200)):
        raise ValueError(f"Expected unfiltered native RGB rows: {path}")
    return b"".join(rows[y * stride + 1:(y + 1) * stride] for y in range(200))


def probe(path, ffprobe):
    return json.loads(subprocess.check_output([
        ffprobe, "-v", "error", "-show_streams", "-show_format", "-of", "json",
        external_path(path, ffprobe),
    ], text=True))


def run(command, log_path):
    with log_path.open("w") as log:
        subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--ffmpeg")
    parser.add_argument("--ffprobe")
    args = parser.parse_args()
    ffmpeg = executable(args.ffmpeg, "ffmpeg")
    ffprobe = executable(args.ffprobe, "ffprobe")
    manifest_path = args.manifest.resolve()
    manifest = json.loads(manifest_path.read_text())
    if manifest["schema"] != 1 or manifest["sid_count"] != 4:
        raise ValueError("Expected a four-SID capture manifest, schema 1")
    if not manifest.get("passed") or not manifest.get("package_files_unchanged"):
        raise ValueError("Capture is incomplete or its package-integrity check failed")
    output = (args.output or manifest_path.parent / "havoc-showcase.mp4").resolve()
    intermediate = manifest_path.parent / "encoded"
    intermediate.mkdir(exist_ok=True)
    fps, duration, encoded, source_peak = 50, 0.0, [], 0
    report = {"passed": False, "source": str(manifest_path), "scenes": [], "fps": fps}
    for entry in manifest["scenes"]:
        if not entry.get("included_in_video", True):
            continue
        scene_path = manifest_path.parent / entry["manifest"]
        scene = json.loads(scene_path.read_text())
        if not scene["passed"]:
            raise ValueError(f"Unverified scene: {scene_path}")
        frames = scene["frames"]
        times = [frame["seconds"] for frame in frames]
        start, length = scene["trim_start_seconds"], scene["duration_seconds"]
        if not times or times != sorted(times) or times[0] > start or times[-1] < start + length:
            raise ValueError(f"Frames do not cover the retained interval: {scene_path}")
        count = round(length * fps)
        if not math.isclose(count / fps, length, abs_tol=1e-7):
            raise ValueError("Scene length must be a multiple of 20 ms")
        audio_path = scene_path.parent / scene["audio"]["path"]
        source_peak = max(source_peak, *(channel["peak"] for channel in scene["audio"]["retained_channels"]))
        audio_probe = probe(audio_path, ffprobe)
        audio_stream = next(s for s in audio_probe["streams"] if s["codec_type"] == "audio")
        if audio_stream["channels"] != 2 or float(audio_probe["format"]["duration"]) < start + length:
            raise ValueError(f"Incomplete stereo audio: {audio_path}")
        clip = intermediate / (entry["name"] + ".mkv")
        # Very short fades prevent clicks where independent native takes meet;
        # they do not move, stretch, or replace any scene's soundtrack.
        fade_out = .25 if entry["name"] == "title-outro" else .025
        command = [ffmpeg, "-hide_banner", "-loglevel", "warning", "-y",
                   "-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", "160x200",
                   "-framerate", str(fps), "-i", "pipe:0", "-ss", str(start),
                   "-i", external_path(audio_path, ffmpeg), "-map", "0:v:0", "-map", "1:a:0",
                   "-vf", "scale=640:480:flags=neighbor,setsar=1",
                   "-af", f"afade=t=in:d=0.025,afade=t=out:st={length-fade_out}:d={fade_out}", "-t", str(length),
                   "-c:v", "libx264", "-preset", "slow", "-crf", "16", "-pix_fmt", "yuv420p",
                   "-c:a", "pcm_s16le", "-ar", "44100", "-ac", "2", external_path(clip, ffmpeg)]
        with (intermediate / (entry["name"] + ".log")).open("w") as log:
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
            previous, pixels = -1, None
            try:
                for number in range(count):
                    index = bisect.bisect_right(times, start + number / fps) - 1
                    if index != previous:
                        pixels = native_rgb(scene_path.parent / frames[index]["path"])
                        previous = index
                    process.stdin.write(pixels)
                process.stdin.close()
                if process.wait() != 0:
                    raise RuntimeError(f"FFmpeg failed for {entry['name']}; see encoded log")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
        encoded.append(clip)
        duration += length
        report["scenes"].append({"name": entry["name"], "duration": length, "output_frames": count})
        print(f"ENCODED {entry['name']}: {length:.2f}s", flush=True)
    if not encoded:
        raise ValueError("No video scenes in capture manifest")
    listing = intermediate / "clips.ffconcat"
    listing.write_text("ffconcat version 1.0\n" + "".join(
        "file '" + external_path(path, ffmpeg).replace("'", "'\\''") + "'\n" for path in encoded))
    base = [ffmpeg, "-hide_banner", "-loglevel", "warning", "-y", "-f", "concat", "-safe", "0",
            "-i", external_path(listing, ffmpeg)]
    # One gain for the complete reel keeps the original balance and dynamics,
    # raises quiet emulator output modestly, and leaves encoding headroom.
    gain_db = min(6.0, 20 * math.log10(.70 / (source_peak / 32768)))
    audio_options = ["-af", f"volume={gain_db:.6f}dB", "-c:a", "aac", "-b:a", "192k", "-ar", "44100", "-ac", "2", "-t", str(duration),
                     "-movflags", "+faststart", "-metadata", "title=H.A.V.O.C. — four-SID gameplay showcase"]
    run(base + ["-c:v", "copy"] + audio_options + [external_path(output, ffmpeg)], intermediate / "mux.log")
    if output.stat().st_size >= 9_800_000:
        bitrate = int(9_400_000 * 8 / duration - 192_000 - 30_000)
        run(base + ["-c:v", "libx264", "-preset", "slow", "-b:v", str(bitrate), "-maxrate", str(bitrate),
                    "-bufsize", str(bitrate * 2)] + audio_options + [external_path(output, ffmpeg)], intermediate / "bounded-encode.log")
    details = probe(output, ffprobe)
    video = next(s for s in details["streams"] if s["codec_type"] == "video")
    audio = next(s for s in details["streams"] if s["codec_type"] == "audio")
    if (video["codec_name"], video["width"], video["height"], audio["codec_name"], audio["channels"]) != ("h264", 640, 480, "aac", 2):
        raise RuntimeError("Unexpected output streams")
    if abs(float(video["duration"]) - duration) > .0201 or abs(float(audio["duration"]) - duration) > .0201:
        raise RuntimeError("Output duration drift exceeds one 20 ms frame")
    if output.stat().st_size >= 9_800_000:
        raise RuntimeError("Video exceeds GitHub attachment budget")
    report.update(passed=True, duration_seconds=duration, bytes=output.stat().st_size, audio_gain_db=gain_db,
                  sha256=hashlib.sha256(output.read_bytes()).hexdigest(), probe=details)
    (intermediate / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"VERIFIED {output}: {duration:.2f}s, {output.stat().st_size:,} bytes", flush=True)


if __name__ == "__main__":
    main()
