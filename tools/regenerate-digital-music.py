"""Reproduce the DLT5 catalog in staging; install only after all checks pass.

Requires an already-built encoder and FFmpeg/FFprobe on PATH. Allocation is
explicit: run digital_format.py --allocate-codebooks before this command.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import math
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import wave

import digital_format as digital

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def overlaps(first, second):
    return first == second or first in second.parents or second in first.parents


def decoded_pcm_digest(path):
    """Hash source-rate, source-channel signed16 PCM without retaining the waveform."""
    command = ["ffmpeg", "-v", "error", "-i", str(path), "-map", "0:a:0", "-vn",
               "-f", "s16le", "-c:a", "pcm_s16le", "-"]
    checksum = hashlib.sha256()
    with tempfile.TemporaryFile() as errors:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=errors) as process:
            for block in iter(lambda: process.stdout.read(1024 * 1024), b""):
                checksum.update(block)
            result = process.wait()
        if result:
            errors.seek(0)
            detail = errors.read(16384).decode("utf-8", errors="replace")
            raise ValueError(f"{path.name}: PCM decoding failed: {detail}")
    return checksum.hexdigest()


def source_layout(path):
    probe = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
        "stream=sample_rate,channels:format_tags", "-of", "json", str(path)], text=True))
    streams = probe.get("streams", [])
    if len(streams) != 1:
        raise ValueError(f"{path.name}: expected one selected audio stream")
    stream = streams[0]
    return (int(stream["sample_rate"]), int(stream["channels"])), probe.get("format", {}).get("tags", {})


def authenticate_source(source, flac, provenance, name):
    """Accept container-only changes after proving layout and authenticated PCM equality."""
    recording = provenance["source"]
    current_hash = digest(source)
    container_changed = current_hash != recording["sha256"]
    recorded_pcm = recording.get("pcm_sha256")
    if not flac.exists():
        if container_changed and not recorded_pcm:
            raise ValueError(f"{name}: changed source has no authenticated intermediate or PCM hash")
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(source),
                        "-map", "0:a:0", "-vn", "-sample_fmt", "s16", "-c:a", "flac",
                        "-threads", "1", str(flac)], check=True)
    flac_hash = digest(flac)
    intermediate_changed = flac_hash != provenance["conversion"]["flac_sha256"]
    # An old FLAC container hash is the trust anchor until a PCM hash has been recorded.
    if intermediate_changed and not recorded_pcm:
        raise ValueError(f"{name}: source intermediate hash changed without authenticated PCM")
    if container_changed or intermediate_changed:
        expected_layout = (int(recording["sample_rate"]), int(recording["channels"]))
        source_format, tags = source_layout(source)
        flac_format, _ = source_layout(flac)
        if source_format != expected_layout or flac_format != expected_layout:
            raise ValueError(f"{name}: source sample rate or channel layout changed")
        flac_pcm = decoded_pcm_digest(flac)
        trusted_pcm = recorded_pcm or flac_pcm
        if flac_pcm != trusted_pcm:
            raise ValueError(f"{name}: intermediate PCM differs from authenticated recording")
        if container_changed and decoded_pcm_digest(source) != trusted_pcm:
            raise ValueError(f"{name}: original recording PCM changed")
        if container_changed:
            recording.setdefault("original_container_sha256", recording["sha256"])
            recording.update(sha256=current_hash, pcm_sha256=trusted_pcm,
                             current_container_metadata=tags)
        provenance["conversion"]["flac_sha256"] = flac_hash
    return flac_hash


def validate_previews(preview, metadata):
    """Bind cached listening artifacts to their recorded PCM, including mono delay."""
    rate, channels, frames = (metadata[key] for key in ("sample_rate", "channels", "sample_count"))

    def read(path, expected_channels, expected_frames):
        with wave.open(str(path), "rb") as wav:
            if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth(), wav.getnframes(), wav.getcomptype()) != (
                    rate, expected_channels, 2, expected_frames, "NONE"):
                raise ValueError(f"{path.name}: preview format mismatch")
            pcm = wav.readframes(expected_frames)
        if len(pcm) != expected_frames * expected_channels * 2:
            raise ValueError(f"{path.name}: truncated preview PCM")
        return pcm

    decoded = read(preview, channels, frames)
    reference = read(preview.with_suffix(".reference.wav"), channels, frames)
    for label, pcm in (("decoded", decoded), ("reference", reference)):
        if hashlib.sha256(pcm).hexdigest() != metadata[f"{label}_pcm_sha256"]:
            raise ValueError(f"{preview.name}: {label} preview PCM hash mismatch")
    if channels == 1:
        delay = rate // 100
        delayed = read(preview.with_suffix(".delayed.wav"), 2, frames + delay)
        left, right = decoded + bytes(delay * 2), bytes(delay * 2) + decoded
        if any(delayed[offset::4] != expected[byte::2]
               for offset, expected, byte in ((0, left, 0), (1, left, 1), (2, right, 0), (3, right, 1))):
            raise ValueError(f"{preview.name}: delayed preview differs from decoded mono")


def validate_quality(metadata, name):
    channels = metadata["channels"]
    labels = ["mono"] if channels == 1 else ["left", "right"]
    metrics = metadata.get("channel_metrics", [])
    if (not isinstance(metrics, list) or any(not isinstance(metric, dict) for metric in metrics)
            or [metric.get("channel") for metric in metrics] != labels):
        raise ValueError(f"{name}: channel metrics do not match the audio channels")
    values = [metadata.get(key) for key in ("matched_level_snr_db", "normalization_gain_db")]
    values += [metric.get(key) for metric in metrics for key in
               ("matched_level_snr_db", "decoded_sample_peak_dbfs", "decoded_true_peak_upper_bound_dbtp")]
    if any(not isinstance(value, (int, float)) or isinstance(value, bool) or not math.isfinite(value)
           for value in values):
        raise ValueError(f"{name}: quality metrics must be finite numbers")


def load_authenticated_recording(directory, name, label):
    metadata = json.loads((directory / f"{name}.json").read_text())
    provenance = json.loads((directory / f"{name}.provenance.json").read_text())
    if digest(directory / f"{name}.dlt") != metadata["asset_sha256"] or any(
            metadata[key] != provenance[key] for key in
            ("asset_sha256", "decoded_pcm_sha256", "reference_pcm_sha256")):
        raise ValueError(f"{name}: {label} asset, metadata and provenance disagree")
    return metadata, provenance


def install_files(files):
    """Prepare all files, then copy into existing destinations to preserve their ACLs.

    Caught write failures roll back every attempted destination, including a
    partially written file. This is not atomic against power loss or process kill.
    """
    directory = (ROOT / "target/digital-music").resolve()
    temporary = Path(tempfile.mkdtemp(prefix="dlt5-install-", dir=directory)).resolve()
    assert temporary.parent == directory
    retain_recovery_files = False
    try:
        backups, replacements = [], []
        for index, (source, destination) in enumerate(files):
            backup = temporary / f"{index}.old"
            replacement = temporary / f"{index}.new"
            if destination.exists():
                shutil.copyfile(destination, backup)
            shutil.copyfile(source, replacement)
            backups.append((destination, backup))
            replacements.append(replacement)
        attempted = 0
        try:
            for (destination, _), replacement in zip(backups, replacements):
                attempted += 1
                # Replacing a file from tempfile's private Windows directory would
                # transfer its restrictive ACL. Writing preserves destination identity,
                # permissions, and inherited ACLs; new files inherit their own parent.
                shutil.copyfile(replacement, destination)
        except BaseException:
            try:
                for destination, backup in reversed(backups[:attempted]):
                    if backup.exists():
                        shutil.copyfile(backup, destination)
                    else:
                        destination.unlink(missing_ok=True)
            except BaseException as error:
                retain_recovery_files = True
                raise RuntimeError(f"Installation rollback failed; recovery files remain at {temporary}") from error
            raise
    finally:
        if not retain_recovery_files:
            shutil.rmtree(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--encoder", type=Path, required=True)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/digital-music/dlt5")
    parser.add_argument("--baseline", type=Path, default=ROOT / "target/digital-music/dlt5-baseline")
    parser.add_argument("--install", action="store_true")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    encoder = args.encoder.resolve(strict=True)
    work = args.work_dir.resolve()
    baseline = args.baseline.resolve()
    music = ROOT / "assets/music"
    staged = work / "assets/music"
    if any(overlaps(first.resolve(), second.resolve()) for first, second in
           ((music, staged), (music, baseline), (staged, baseline))):
        parser.error("live music, staged music, and baseline directories must not overlap")
    # Intermediates and installation recovery files use this fixed directory,
    # even when both configurable output directories are elsewhere.
    (ROOT / "target/digital-music").mkdir(parents=True, exist_ok=True)
    staged.mkdir(parents=True, exist_ok=True)
    baseline.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((music / "catalog.json").read_text())
    write_json(staged / "catalog.json", manifest)
    for track in manifest["tracks"]:
        if "codebook_count" not in track:
            parser.error("allocate codebooks before encoding")
        for suffix in ("dlt", "json", "provenance.json"):
            name = f'{track["name"]}.{suffix}'
            if not (baseline / name).exists():
                shutil.copyfile(music / name, baseline / name)
    sources = ROOT / "tools/encode-digital-music/src"
    source_hashes = {path.name: digest(path) for path in sorted(sources.glob("*.rs"))}
    binary_hash = digest(encoder)
    ffmpeg_version = subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0]

    def encode(track):
        name = track["name"]
        old, _ = load_authenticated_recording(baseline, name, "baseline")
        validate_quality(old, f"{name} baseline")
        # The baseline fixes the quality comparison. Source authentication and
        # provenance must retain information accepted by later installations,
        # including PCM identity recorded after a container-only retag.
        _, provenance = load_authenticated_recording(music, name, "live")
        source = music / "source" / provenance["source"]["filename"]
        flac = ROOT / "target/digital-music" / f"{name}-source.flac"
        flac_hash = authenticate_source(source, flac, provenance, name)
        asset = staged / f"{name}.dlt"
        preview = work / f"{name}-dlt5.wav"
        fingerprint = dict(source=flac_hash, encoder=binary_hash, encoder_sources=source_hashes,
                           ffmpeg=ffmpeg_version, profile=track["profile"], books=track["codebook_count"])
        stamp = work / f"{name}.inputs.json"
        cached = False
        try:
            if json.loads(stamp.read_text()) == fingerprint:
                metadata = json.loads(asset.with_suffix(".json").read_text())
                digital.validate_blob(asset.read_bytes(), metadata, track["profile"])
                validate_previews(preview, metadata)
                cached = True
        except (OSError, ValueError, KeyError, TypeError, AssertionError, EOFError, wave.Error):
            pass
        if not cached:
            started = time.monotonic()
            print(f'Encoding {name}: {track["codebook_count"]} books', flush=True)
            with (work / f"{name}.log").open("w", encoding="utf-8") as log:
                subprocess.run([str(encoder), "--profile", track["profile"], "--codebooks",
                                str(track["codebook_count"]), str(flac), str(asset), str(preview)],
                               stdout=log, stderr=subprocess.STDOUT, check=True)
            write_json(stamp, fingerprint)
            print(f"Encoded {name} in {time.monotonic() - started:.1f}s", flush=True)
        metadata = json.loads(asset.with_suffix(".json").read_text())
        digital.validate_blob(asset.read_bytes(), metadata, track["profile"])
        validate_previews(preview, metadata)
        validate_quality(metadata, name)
        if metadata["codebook_count"] != track["codebook_count"]:
            raise ValueError(f"{name}: encoder changed requested book count")
        for key in ("sample_count", "sample_rate", "channels", "bits_per_sample"):
            if metadata[key] != old[key]:
                raise ValueError(f"{name}: changed {key}")
        if metadata["flac_sha256"] != flac_hash:
            raise ValueError(f"{name}: metadata does not identify the authenticated source intermediate")
        channels = []
        for before, after in zip(old["channel_metrics"], metadata["channel_metrics"]):
            gain = after["matched_level_snr_db"] - before["matched_level_snr_db"]
            if gain < -0.000001:
                raise ValueError(f'{name}: {after["channel"]} SNR regressed {gain:.6f}dB')
            if max(after["decoded_sample_peak_dbfs"], after["decoded_true_peak_upper_bound_dbtp"]) > -1:
                raise ValueError(f"{name}: decoded peak exceeds -1dB")
            channels.append(dict(channel=after["channel"], snr_gain_db=gain,
                                 true_peak_dbtp=after["decoded_true_peak_upper_bound_dbtp"]))
        conversion = provenance["conversion"]
        conversion.update(format="DLT5", codebook_entry_bytes=2,
                          codebook_count=metadata["codebook_count"],
                          samples_per_segment=metadata["samples_per_segment"],
                          long_segment_count=metadata["long_segment_count"],
                          encoder_files_sha256=source_hashes, encoder_binary_sha256=binary_hash,
                          ffmpeg_version=ffmpeg_version,
                          normalization_gain_db=metadata["normalization_gain_db"],
                          normalization_attempts=metadata["normalization_attempts"],
                          packing="Continuous LSB-first three-bit codes; stereo alternates left/right; "
                                  "balanced ceil(frame_count/alignment) groups, first remainder books "
                                  "one group longer; final segment clipped; zero final padding",
                          codebooks="Little-endian signed-16 deltas; eight entries per channel")
        provenance["tool_source"]["note"] = (
            "Original source hashes identify the imported Second Reality algorithms. The local "
            "encoder adds DLT5 compact signed-16 books, balanced segments, closed-loop retraining, "
            "beam search, floating-point mono downmix, rounded output and loop-aware peak validation; "
            "conversion.encoder_files_sha256 identifies this implementation.")
        for key in ("asset_sha256", "decoded_pcm_sha256", "reference_pcm_sha256"):
            provenance[key] = metadata[key]
        provenance["reproduction_commands"]["encode"] = (
            "cargo run --release --manifest-path tools/encode-digital-music/Cargo.toml "
            "--target-dir target/digital-music/tool-build -- "
            f'--profile {track["profile"]} --codebooks {track["codebook_count"]} '
            f"target/digital-music/{name}-source.flac assets/music/{name}.dlt "
            f"target/digital-music/{name}-dlt5.wav")
        write_json(staged / f"{name}.provenance.json", provenance)
        return dict(name=name, old_books=old["codebook_count"], books=metadata["codebook_count"],
                    average_interval_ms=metadata["sample_count"] / metadata["sample_rate"]
                    / metadata["codebook_count"] * 1000,
                    old_snr_db=old["matched_level_snr_db"],
                    snr_db=metadata["matched_level_snr_db"],
                    snr_gain_db=metadata["matched_level_snr_db"] - old["matched_level_snr_db"],
                    old_normalization_gain_db=old["normalization_gain_db"],
                    normalization_gain_db=metadata["normalization_gain_db"],
                    channels=channels, preview=str(preview))

    results = {}
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for future in as_completed([pool.submit(encode, track) for track in manifest["tracks"]]):
            result = future.result()
            results[result["name"]] = result
    catalog = digital.load_catalog(work)
    generated = digital.rust_catalog(catalog)
    report = dict(passed=True, used_bytes=catalog["used_bytes"],
                  remaining_bytes=digital.MUSIC_CAPACITY - catalog["used_bytes"],
                  encoder_sha256=binary_hash, listening_review="not performed by this script",
                  snr_reference="Each encoding's own normalized reference at the unchanged profile; "
                                "normalization gains may differ, so this is not a fixed-reference comparison.",
                  tracks=[results[track["name"]] for track in manifest["tracks"]])
    write_json(work / "quality-report.json", report)
    if args.install:
        # All tracks, quality gates and the complete staged catalog passed first.
        files = []
        for track in manifest["tracks"]:
            for suffix in ("dlt", "json", "provenance.json"):
                name = f'{track["name"]}.{suffix}'
                files.append((staged / name, music / name))
        generated_path = work / "digital_catalog.rs"
        generated_path.write_text(generated, encoding="utf-8")
        files.append((generated_path, ROOT / "src/digital_catalog.rs"))
        install_files(files)
        print("Installed validated DLT5 soundtrack and generated catalog.", flush=True)
    print(f'Catalog: {report["used_bytes"]:,} bytes; {report["remaining_bytes"]} free. '
          f'Quality report: {work / "quality-report.json"}', flush=True)


if __name__ == "__main__":
    main()
