#!/usr/bin/env python3
"""Encode native capture manifests without speeding up the game.

Requires FFmpeg and FFprobe. Under WSL, Windows .exe paths are supported too.
Intermediate files and the verification report stay beside the capture manifest.
An optional continuous source soundtrack mixes with quieter captured effects;
the source, balance, headroom adjustment and final true peak are recorded.
"""
import argparse
import bisect
import hashlib
import json
import math
import os
from pathlib import Path
import re
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
            if width not in (160, 320) or (height, depth, color, compression, filtering, interlace) != (200, 8, 2, 0, 0, 0):
                raise ValueError(f"Unexpected native PNG format: {path}")
            dimensions = (width, height)
        elif kind == b"IDAT":
            compressed.extend(payload)
        offset += length + 12
    if dimensions is None:
        raise ValueError(f"Missing PNG dimensions: {path}")
    rows = zlib.decompress(compressed)
    stride = dimensions[0] * 3 + 1
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


def effects_normalization_db(source_peak):
    """Keep the legacy bounded normalization, including legitimately silent takes."""
    return min(6.0, 20 * math.log10(.70 / (source_peak / 32768))) if source_peak else 0.0


def true_peak(path, ffmpeg, log_path):
    """Measure decoded true peak, allowing for EBU R128's 0.1 dB rounding."""
    run([ffmpeg, "-hide_banner", "-nostats", "-i", external_path(path, ffmpeg),
         "-map", "0:a:0", "-af", "ebur128=peak=true", "-f", "null", "-"], log_path)
    values = re.findall(r"Peak:\s+(-?(?:\d+(?:\.\d+)?|inf))\s+dBFS", log_path.read_text())
    if not values:
        raise RuntimeError(f"No true-peak result; see {log_path}")
    peak = float(values[-1])
    # JSON has no infinity. Null is the measured digital-silence case.
    return {"dbtp": peak if math.isfinite(peak) else None,
            "upper_bound_dbtp": peak + .05 if math.isfinite(peak) else None,
            "meter": "FFmpeg EBU R128 true peak; +0.05 dB rounding bound"}


def mix_audio(listing, music, duration, source_peak, sfx_gain_db, ffmpeg, ffprobe, intermediate):
    normalization = effects_normalization_db(source_peak)
    mix_path = intermediate / "audio-mix.wav"
    report = {"mode": "source soundtrack plus native effects" if music else "native captured audio",
              "native_normalization_db": normalization,
              "effects_gain_db": sfx_gain_db if music else 0.0,
              "music_gain_db": 0.0 if music else None,
              "time_stretch": False, "duration_seconds": duration}
    command = [ffmpeg, "-hide_banner", "-loglevel", "warning", "-y",
               "-f", "concat", "-safe", "0", "-i", external_path(listing, ffmpeg)]
    if music:
        music = music.resolve()
        details = probe(music, ffprobe)
        streams = [s for s in details["streams"] if s["codec_type"] == "audio"]
        if not streams:
            raise ValueError("Soundtrack has no audio stream")
        music_duration = float(streams[0].get("duration", details["format"]["duration"]))
        if music_duration + .0201 < duration:
            raise ValueError(f"Soundtrack is too short: {music_duration:.3f}s for a {duration:.3f}s reel")
        report["music"] = {"path": str(music), "sha256": hashlib.sha256(music.read_bytes()).hexdigest(),
                           "source_duration_seconds": music_duration, "start_seconds": 0.0,
                           "retained_duration_seconds": min(duration, music_duration),
                           "full_source": abs(music_duration - duration) <= .0201,
                           "looped": False, "probe": details}
        command += ["-i", external_path(music, ffmpeg), "-filter_complex",
                    f"[0:a:0]volume={normalization+sfx_gain_db:.6f}dB[effects];"
                    "[1:a:0]aresample=44100,aformat=channel_layouts=stereo,apad[music];"
                    "[effects][music]amix=inputs=2:duration=first:normalize=0[mix]", "-map", "[mix]"]
    else:
        command += ["-map", "0:a:0", "-af", f"volume={normalization:.6f}dB"]
    # Float PCM preserves any overs until one global headroom correction is
    # chosen. No limiter, compression, ducking or per-scene music restarts.
    command += ["-vn", "-t", str(duration), "-c:a", "pcm_f32le", "-ar", "44100", "-ac", "2",
                external_path(mix_path, ffmpeg)]
    run(command, intermediate / "audio-mix.log")
    report["unattenuated_true_peak"] = true_peak(mix_path, ffmpeg, intermediate / "audio-mix-peak.log")
    return mix_path, report


def encode_final_audio(listing, mix_path, output, duration, sid_count, mix_report, ffmpeg, intermediate):
    """Verify the encoded AAC, correcting codec overs with a single static gain."""
    upper = mix_report["unattenuated_true_peak"]["upper_bound_dbtp"]
    attenuation = min(0.0, -1.10 - upper) if upper is not None else 0.0
    for attempt in range(4):
        command = [ffmpeg, "-hide_banner", "-loglevel", "warning", "-y",
                   "-f", "concat", "-safe", "0", "-i", external_path(listing, ffmpeg),
                   "-i", external_path(mix_path, ffmpeg), "-map", "0:v:0", "-map", "1:a:0",
                   "-c:v", "copy", "-af", f"volume={attenuation:.6f}dB", "-c:a", "aac", "-b:a", "192k",
                   "-ar", "44100", "-ac", "2", "-t", str(duration), "-movflags", "+faststart",
                   "-metadata", f"title=H.A.V.O.C. — {sid_count}-SID gameplay showcase",
                   external_path(output, ffmpeg)]
        run(command, intermediate / f"mux-{attempt}.log")
        measured = true_peak(output, ffmpeg, intermediate / f"encoded-peak-{attempt}.log")
        encoded_upper = measured["upper_bound_dbtp"]
        if encoded_upper is None or encoded_upper <= -1.0:
            mix_report.update(global_attenuation_db=attenuation, encoded_true_peak=measured,
                              true_peak_ceiling_dbtp=-1.0, aac_encode_attempts=attempt + 1)
            return
        attenuation += -1.10 - encoded_upper
    raise RuntimeError("Encoded AAC still exceeds -1 dBTP after headroom correction")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--ffmpeg")
    parser.add_argument("--ffprobe")
    parser.add_argument("--music", type=Path, help="continuous original soundtrack, starting at zero")
    parser.add_argument("--sfx-gain-db", type=float, default=-12.0,
                        help="captured effects gain after normalization when --music is used (default: -12)")
    args = parser.parse_args()
    if not math.isfinite(args.sfx_gain_db) or not -120 <= args.sfx_gain_db <= 0:
        parser.error("--sfx-gain-db must be finite and between -120 and 0 dB")
    ffmpeg = executable(args.ffmpeg, "ffmpeg")
    ffprobe = executable(args.ffprobe, "ffprobe")
    manifest_path = args.manifest.resolve()
    manifest = json.loads(manifest_path.read_text())
    if manifest["schema"] != 1 or manifest["sid_count"] not in (2, 4):
        raise ValueError("Expected a two- or four-SID capture manifest, schema 1")
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
        native_width = scene.get("native_width", 160)
        if native_width not in (160, 320):
            raise ValueError(f"Unsupported capture width: {native_width}")
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
                   "-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", f"{native_width}x200",
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
                        if len(pixels) != native_width * 200 * 3:
                            raise ValueError("PNG width differs from its scene manifest")
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
    mix_path, mix_report = mix_audio(listing, args.music, duration, source_peak, args.sfx_gain_db,
                                    ffmpeg, ffprobe, intermediate)
    master = output.with_name(output.stem + "-master.mp4") if args.music else output
    encode_final_audio(listing, mix_path, master, duration, manifest["sid_count"], mix_report, ffmpeg, intermediate)
    if master != output:
        shutil.copyfile(master, output)
        report["master"] = {"path": str(master), "bytes": master.stat().st_size,
                            "sha256": hashlib.sha256(master.read_bytes()).hexdigest(),
                            "video_crf": 16, "probe": probe(master, ffprobe)}
    if output.stat().st_size >= 9_800_000:
        bitrate = int(9_400_000 * 8 / duration - 192_000 - 30_000)
        if bitrate <= 0:
            raise RuntimeError("Reel is too long for the GitHub attachment budget")
        bounded_source = master
        if master == output:
            bounded_source = intermediate / "unbounded.mp4"
            shutil.copyfile(output, bounded_source)
        run([ffmpeg, "-hide_banner", "-loglevel", "warning", "-y", "-i", external_path(bounded_source, ffmpeg),
             "-map", "0:v:0", "-map", "0:a:0", "-c:v", "libx264", "-preset", "slow",
             "-b:v", str(bitrate), "-maxrate", str(bitrate), "-bufsize", str(bitrate * 2),
             "-c:a", "copy", "-t", str(duration), "-movflags", "+faststart", external_path(output, ffmpeg)],
            intermediate / "bounded-encode.log")
    details = probe(output, ffprobe)
    video = next(s for s in details["streams"] if s["codec_type"] == "video")
    audio = next(s for s in details["streams"] if s["codec_type"] == "audio")
    if (video["codec_name"], video["width"], video["height"], audio["codec_name"], audio["channels"]) != ("h264", 640, 480, "aac", 2):
        raise RuntimeError("Unexpected output streams")
    if abs(float(video["duration"]) - duration) > .0201 or abs(float(audio["duration"]) - duration) > .0201:
        raise RuntimeError("Output duration drift exceeds one 20 ms frame")
    if output.stat().st_size >= 9_800_000:
        raise RuntimeError("Video exceeds GitHub attachment budget")
    final_peak = true_peak(output, ffmpeg, intermediate / "final-peak.log")
    if final_peak["upper_bound_dbtp"] is not None and final_peak["upper_bound_dbtp"] > -1.0:
        raise RuntimeError("Final upload audio exceeds -1 dBTP")
    mix_report["final_true_peak"] = final_peak
    report.update(passed=True, duration_seconds=duration, bytes=output.stat().st_size,
                  audio_gain_db=mix_report["native_normalization_db"], audio_mix=mix_report,
                  sha256=hashlib.sha256(output.read_bytes()).hexdigest(), probe=details)
    (intermediate / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"VERIFIED {output}: {duration:.2f}s, {output.stat().st_size:,} bytes", flush=True)


if __name__ == "__main__":
    main()
