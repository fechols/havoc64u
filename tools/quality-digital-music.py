"""Compare fixed-size DLT5 encoders against authenticated, fixed-reference PCM.

The full matrix is resumable. --tracks runs a pilot; --install requires every
catalog track and installs only complete-track winners after catalog validation.
"""
import argparse
from array import array
from concurrent.futures import ThreadPoolExecutor, as_completed
import copy
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import time
import wave

import digital_format as digital

SPEC = importlib.util.spec_from_file_location("regenerate_music", Path(__file__).with_name("regenerate-digital-music.py"))
regen = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(regen)
ROOT = Path(__file__).resolve().parents[1]
PEAK_REJECTION = "FROZEN_REFERENCE_PEAK_LIMIT"
CONFIGURATIONS = (("baseline", 32), ("beam-refine", 32), ("som", 32),
                  ("baseline", 64), ("baseline", 128))
GEOMETRY = ("sample_count", "sample_rate", "channels", "bits_per_sample", "output_bits_per_sample",
            "codebook_count", "codebook_entries", "asset_bytes", "codebook_bytes", "stream_bytes")


def pcm_bytes(pcm):
    if sys.byteorder == "little":
        return pcm.tobytes()
    little = array("h", pcm)
    little.byteswap()
    return little.tobytes()


def read_pcm(path, metadata, expected_hash):
    with wave.open(str(path), "rb") as wav:
        if (wav.getnchannels(), wav.getframerate(), wav.getsampwidth(), wav.getnframes(), wav.getcomptype()) != (
                metadata["channels"], metadata["sample_rate"], 2, metadata["sample_count"], "NONE"):
            raise ValueError(f"{path}: reference/preview WAV format mismatch")
        raw = wav.readframes(metadata["sample_count"])
    if len(raw) != metadata["sample_count"] * metadata["channels"] * 2:
        raise ValueError(f"{path}: truncated PCM")
    if hashlib.sha256(raw).hexdigest() != expected_hash:
        raise ValueError(f"{path}: PCM hash mismatch")
    result = array("h")
    result.frombytes(raw)
    if sys.byteorder != "little":
        result.byteswap()
    return result


def decode_pcm(blob):
    """Independent scalar DLT5 oracle; output is actual signed8 DAC PCM * 256."""
    if blob[:4] != b"DLT5":
        raise ValueError("quality experiments require DLT5")
    _, frames, stream_bytes, entries, books, bits, channels = struct.unpack_from("<IIIHHBB", blob, 4)
    if entries != 8 or bits != 3 or channels not in (1, 2):
        raise ValueError("unsupported DLT5 quantizer")
    base, longer = digital.segment_layout(frames, books, channels)
    alignment = 8 // channels
    stream_start = 32 + books * channels * 16
    stream = memoryview(blob)[stream_start:stream_start + stream_bytes]
    predictions = [0] * channels
    output = array("h")
    reservoir = available = position = produced = 0
    for segment in range(books):
        count = min(base + (alignment if segment < longer else 0), frames - produced)
        tables = [struct.unpack_from("<8h", blob, 32 + (segment * channels + channel) * 16)
                  for channel in range(channels)]
        for _ in range(count):
            for channel in range(channels):
                if available < 3:
                    reservoir |= stream[position] << available
                    position += 1
                    available += 8
                code = reservoir & 7
                reservoir >>= 3
                available -= 3
                predictor = max(-32768, min(32767, predictions[channel] + tables[channel][code]))
                predictions[channel] = predictor
                output.append(max(-128, min(127, (predictor + 128) // 256)) * 256)
        produced += count
    if produced != frames:
        raise ValueError("independent decoder produced the wrong length")
    return output


def measure_error(reference, decoded, channels):
    if len(reference) != len(decoded) or len(reference) % channels:
        raise ValueError("PCM lengths differ")
    signal, error = [0] * channels, [0] * channels
    for index, (target, sample) in enumerate(zip(reference, decoded)):
        channel = index % channels
        signal[channel] += target * target
        error[channel] += (target - sample) ** 2
    return dict(signal_sum_squares=signal, error_sum_squares=error,
                snr_db=[snr(s, e) for s, e in zip(signal, error)], total_snr_db=snr(sum(signal), sum(error)))


def snr(signal, error):
    # Exact integers govern selection; JSON null represents undefined/infinite SNR.
    return 10 * math.log10(signal / error) if signal and error else None


def eligible(baseline, candidate):
    if candidate.get("rejected"):
        return False
    before, after = baseline["metrics"], candidate["metrics"]
    return (before["signal_sum_squares"] == after["signal_sum_squares"]
            and len(before["error_sum_squares"]) == len(after["error_sum_squares"])
            and all(a <= b for a, b in zip(after["error_sum_squares"], before["error_sum_squares"]))
            and sum(after["error_sum_squares"]) < sum(before["error_sum_squares"]))


def choose_winner(baseline, candidates):
    winner = baseline
    for candidate in candidates:
        if eligible(baseline, candidate) and sum(candidate["metrics"]["error_sum_squares"]) < sum(winner["metrics"]["error_sum_squares"]):
            winner = candidate
    return winner


def snr_value(signal, error):
    """Represent infinite/undefined SNR explicitly, without nonstandard JSON floats."""
    if signal == 0:
        return dict(value_db=None, status="undefined" if error == 0 else "negative_infinity")
    if error == 0:
        return dict(value_db=None, status="positive_infinity")
    return dict(value_db=10 * math.log10(signal / error), status="finite")


def mean_values(values):
    if not values:
        return dict(value_db=None, status="undefined")
    statuses = {value["status"] for value in values}
    if "undefined" in statuses or {"positive_infinity", "negative_infinity"} <= statuses:
        return dict(value_db=None, status="undefined")
    for status in ("positive_infinity", "negative_infinity"):
        if status in statuses:
            return dict(value_db=None, status=status)
    return dict(value_db=math.fsum(value["value_db"] for value in values) / len(values), status="finite")


def gain_value(before, after):
    if before["status"] == after["status"] == "finite":
        return dict(value_db=after["value_db"] - before["value_db"], status="finite")
    if "undefined" in (before["status"], after["status"]) or before["status"] == after["status"]:
        return dict(value_db=None, status="undefined")
    positive = after["status"] == "positive_infinity" or before["status"] == "negative_infinity"
    return dict(value_db=None, status="positive_infinity" if positive else "negative_infinity")


def summarize_metrics(results):
    signals = [sum(result["metrics"]["signal_sum_squares"]) for result in results]
    errors = [sum(result["metrics"]["error_sum_squares"]) for result in results]
    values = [snr_value(signal, error) for signal, error in zip(signals, errors)]
    mean = mean_values(values)
    pooled = snr_value(sum(signals), sum(errors))
    return dict(arithmetic_mean_snr_db=mean["value_db"], arithmetic_mean_snr_status=mean["status"],
                pooled_snr_db=pooled["value_db"], pooled_snr_status=pooled["status"],
                track_count=len(results), finite_track_count=sum(value["status"] == "finite" for value in values),
                signal_sum_squares=sum(signals), error_sum_squares=sum(errors))


def summarize_experiments(tracks):
    """Equal-track means and energy-weighted pooling; failed methods use the baseline."""
    baselines = [track["baseline"] for track in tracks]
    winners = [track["winner"] for track in tracks]
    baseline_values = [snr_value(sum(result["metrics"]["signal_sum_squares"]),
                                sum(result["metrics"]["error_sum_squares"])) for result in baselines]

    def compare(results):
        values = [snr_value(sum(result["metrics"]["signal_sum_squares"]),
                            sum(result["metrics"]["error_sum_squares"])) for result in results]
        gain = mean_values([gain_value(before, after) for before, after in zip(baseline_values, values)])
        return dict(summarize_metrics(results), mean_gain_db=gain["value_db"], mean_gain_status=gain["status"])

    strategies = {}
    for result in winners:
        config = result["configuration"]
        key = f'{config["training"]}-{config["beam_width"]}'
        strategies[key] = strategies.get(key, 0) + 1
    methods = {}
    keys = [f"{training}-{width}" for training, width in CONFIGURATIONS] + ["combined"]
    for key in keys:
        effective, widths, fallback_count, attempted = [], {}, 0, 0
        for track in tracks:
            matched = [candidate for candidate in track["candidates"]
                       if (candidate["configuration"]["training"] == "combined" if key == "combined"
                           else f'{candidate["configuration"]["training"]}-{candidate["configuration"]["beam_width"]}' == key)]
            trial = matched[0] if matched else None
            attempted += trial is not None
            if trial is not None:
                width = str(trial["configuration"]["beam_width"])
                widths[width] = widths.get(width, 0) + 1
            if trial is not None and eligible(track["baseline"], trial):
                effective.append(trial)
            else:
                effective.append(track["baseline"])
                fallback_count += 1
        methods[key] = dict(compare(effective), attempted_tracks=attempted, baseline_fallback_tracks=fallback_count,
                            beam_width_counts=widths)
    return dict(baseline=summarize_metrics(baselines), selected=compare(winners),
                improved_tracks=sum(eligible(track["baseline"], track["winner"]) for track in tracks),
                selected_strategy_counts=strategies, methods=methods,
                method_policy="Each method falls back to the baseline unless complete-track error improves with no channel regression; combined uses each track's best standalone beam width.")


def validate_candidate(asset, preview, track, baseline_metadata, reference, configuration, flac_hash):
    metadata = json.loads(asset.with_suffix(".json").read_text())
    blob = asset.read_bytes()
    digital.validate_blob(blob, metadata, track["profile"])
    regen.validate_quality(metadata, track["name"])
    for key in GEOMETRY:
        if metadata.get(key) != baseline_metadata.get(key):
            raise ValueError(f'{track["name"]}: candidate changed {key}')
    if (metadata["reference_pcm_sha256"] != baseline_metadata["reference_pcm_sha256"]
            or metadata["normalization_gain_db"] != baseline_metadata["normalization_gain_db"]):
        raise ValueError(f'{track["name"]}: candidate changed fixed reference or gain')
    if (metadata.get("encoder_training") != configuration["training"]
            or metadata.get("encoder_beam_width") != configuration["beam_width"]
            or metadata.get("fixed_reference") is not True or metadata["flac_sha256"] != flac_hash):
        raise ValueError(f'{track["name"]}: candidate settings/source mismatch')
    if any(max(channel["decoded_sample_peak_dbfs"], channel["decoded_true_peak_upper_bound_dbtp"]) > -1
           for channel in metadata["channel_metrics"]):
        raise ValueError(f'{track["name"]}: candidate exceeds peak limit')
    regen.validate_previews(preview, metadata)
    decoded = decode_pcm(blob)
    if hashlib.sha256(pcm_bytes(decoded)).hexdigest() != metadata["decoded_pcm_sha256"]:
        raise ValueError(f'{track["name"]}: preview differs from independent DLT5 decoding')
    metrics = measure_error(reference, decoded, metadata["channels"])
    for index, channel in enumerate(metadata["channel_metrics"]):
        for key in ("signal_sum_squares", "error_sum_squares"):
            if channel.get(key) != metrics[key][index]:
                raise ValueError(f'{track["name"]}: encoder {key} differs from independent measurement')
    return metadata, metrics


def run_candidate(encoder, track, baseline_metadata, reference, reference_path, reference_metadata_path,
                  flac, configuration, directory, fingerprint):
    directory.mkdir(parents=True, exist_ok=True)
    name = track["name"]
    asset, preview = directory / f"{name}.dlt", directory / f"{name}-dlt5.wav"
    stamp, log_path = directory / "result.json", directory / "encoder.log"
    expected = dict(fingerprint, configuration=configuration)
    cached = None
    try:
        value = json.loads(stamp.read_text())
        if value["fingerprint"] == expected:
            if value["result"].get("rejected"):
                if regen.digest(log_path) == value["log_sha256"] and PEAK_REJECTION in log_path.read_text():
                    return value["result"]
            else:
                metadata, metrics = validate_candidate(asset, preview, track, baseline_metadata, reference,
                                                       configuration, fingerprint["source_sha256"])
                cached = dict(value["result"], metrics=metrics)
    except (OSError, ValueError, KeyError, AssertionError, EOFError, wave.Error):
        pass
    if cached is not None:
        return cached
    print(f'{name}: {configuration["training"]}, beam {configuration["beam_width"]}', flush=True)
    command = [str(encoder), "--profile", track["profile"], "--codebooks", str(track["codebook_count"]),
               "--training", configuration["training"], "--beam-width", str(configuration["beam_width"]),
               "--reference-wav", str(reference_path), "--reference-metadata", str(reference_metadata_path),
               str(flac), str(asset), str(preview)]
    started = time.monotonic()
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    elapsed = time.monotonic() - started
    result = dict(configuration=configuration, seconds=elapsed, asset=str(asset), preview=str(preview))
    if process.returncode:
        if PEAK_REJECTION not in log_path.read_text(encoding="utf-8", errors="replace"):
            raise RuntimeError(f"{name}: encoder failed ({process.returncode}); see {log_path}")
        result.update(rejected="peak_limit", log=str(log_path))
    else:
        metadata, metrics = validate_candidate(asset, preview, track, baseline_metadata, reference,
                                               configuration, fingerprint["source_sha256"])
        result.update(metrics=metrics, asset_sha256=metadata["asset_sha256"],
                      decoded_pcm_sha256=metadata["decoded_pcm_sha256"],
                      peaks=[dict(sample_dbfs=c["decoded_sample_peak_dbfs"],
                                  true_peak_dbtp=c["decoded_true_peak_upper_bound_dbtp"])
                             for c in metadata["channel_metrics"]])
    regen.write_json(stamp, dict(fingerprint=expected, result=result, log_sha256=regen.digest(log_path)))
    return result


def snapshot_baseline(music, baseline, reference_dir):
    baseline.mkdir(parents=True, exist_ok=True)
    manifest_path = baseline / "catalog.json"
    if not manifest_path.exists():
        shutil.copyfile(music / "catalog.json", manifest_path)
    manifest = json.loads(manifest_path.read_text())
    for track in manifest["tracks"]:
        name = track["name"]
        for suffix in ("dlt", "json", "provenance.json"):
            destination = baseline / f"{name}.{suffix}"
            if not destination.exists():
                shutil.copyfile(music / destination.name, destination)
        metadata, _ = regen.load_authenticated_recording(baseline, name, "frozen baseline")
        digital.validate_blob((baseline / f"{name}.dlt").read_bytes(), metadata, track["profile"])
        if track["codebook_count"] != metadata["codebook_count"]:
            raise ValueError(f"{name}: frozen manifest book count differs")
        destination = baseline / f"{name}-dlt5.reference.wav"
        if not destination.exists():
            shutil.copyfile(reference_dir / destination.name, destination)
        read_pcm(destination, metadata, metadata["reference_pcm_sha256"])
    return manifest


def stage_winners(work, baseline, manifest, results, encoder, source_hashes, ffmpeg_version):
    staged_root = work / "selected"
    music = staged_root / "assets/music"
    music.mkdir(parents=True, exist_ok=True)
    selected_manifest = copy.deepcopy(manifest)
    for track in selected_manifest["tracks"]:
        name = track["name"]
        result = results[name]
        winner = result["winner"]
        source = Path(winner["asset"])
        if not result["improved"]:
            for suffix in ("dlt", "json", "provenance.json"):
                shutil.copyfile(baseline / f"{name}.{suffix}", music / f"{name}.{suffix}")
            continue
        metadata = json.loads(source.with_suffix(".json").read_text())
        before, provenance = regen.load_authenticated_recording(baseline, name, "frozen baseline")
        track["encoder_options"] = dict(winner["configuration"], reference=dict(
            training="baseline", beam_width=32, codebook_count=track["codebook_count"],
            pcm_sha256=before["reference_pcm_sha256"], asset_sha256=before["asset_sha256"]))
        for suffix in ("dlt", "json"):
            shutil.copyfile(source.with_suffix(f".{suffix}"), music / f"{name}.{suffix}")
        provenance["conversion"].update(
            encoder_files_sha256=source_hashes, encoder_binary_sha256=regen.digest(encoder),
            ffmpeg_version=ffmpeg_version, flac_sha256=metadata["flac_sha256"],
            normalization_gain_db=metadata["normalization_gain_db"],
            normalization_attempts=metadata["normalization_attempts"])
        for key in ("asset_sha256", "decoded_pcm_sha256", "reference_pcm_sha256"):
            provenance[key] = metadata[key]
        regen.record_encoder_options(provenance, track, metadata)
        provenance["tool_source"]["note"] += (
            " Offline quality experiment selected this configuration using whole-track integer "
            "squared error against authenticated fixed-reference PCM, with no channel regression.")
        regen.write_json(music / f"{name}.provenance.json", provenance)
    regen.write_json(music / "catalog.json", selected_manifest)
    catalog = digital.load_catalog(staged_root)
    generated = staged_root / "digital_catalog.rs"
    generated.write_text(digital.rust_catalog(catalog), encoding="utf-8")
    return staged_root, catalog


def install_selection(staged, manifest):
    """Install the complete selection, including baseline fallbacks on reruns."""
    files = []
    for track in manifest["tracks"]:
        for suffix in ("dlt", "json", "provenance.json"):
            name = f'{track["name"]}.{suffix}'
            files.append((staged / "assets/music" / name, ROOT / "assets/music" / name))
    files.extend(((staged / "assets/music/catalog.json", ROOT / "assets/music/catalog.json"),
                  (staged / "digital_catalog.rs", ROOT / "src/digital_catalog.rs")))
    regen.install_files(files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--encoder", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/digital-music/quality-experiments")
    parser.add_argument("--reference-dir", type=Path)
    parser.add_argument("--tracks", nargs="+")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--install", action="store_true")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    encoder, work = args.encoder.resolve(strict=True), args.work_dir.resolve()
    music, baseline = ROOT / "assets/music", work / "baseline"
    if regen.overlaps(work, music.resolve()):
        parser.error("experiment staging must not overlap live assets")
    manifest = snapshot_baseline(music, baseline, (args.reference_dir or baseline).resolve())
    names = [track["name"] for track in manifest["tracks"]]
    requested = set(args.tracks or names)
    if requested - set(names):
        parser.error(f"unknown tracks: {sorted(requested - set(names))}")
    if args.install and requested != set(names):
        parser.error("--install requires every catalog track")
    source_hashes = {path.name: regen.digest(path) for path in sorted((ROOT / "tools/encode-digital-music/src").glob("*.rs"))}
    ffmpeg_version = subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0]
    common = dict(encoder_sha256=regen.digest(encoder), encoder_sources=source_hashes,
                  pipeline_sha256=regen.digest(Path(__file__)),
                  format_validator_sha256=regen.digest(Path(digital.__file__)), ffmpeg=ffmpeg_version)

    def experiment(track):
        name = track["name"]
        before, provenance = regen.load_authenticated_recording(baseline, name, "frozen baseline")
        reference_path = baseline / f"{name}-dlt5.reference.wav"
        reference = read_pcm(reference_path, before, before["reference_pcm_sha256"])
        old_asset = baseline / f"{name}.dlt"
        old_pcm = decode_pcm(old_asset.read_bytes())
        if hashlib.sha256(pcm_bytes(old_pcm)).hexdigest() != before["decoded_pcm_sha256"]:
            raise ValueError(f"{name}: independent baseline decode hash differs")
        base = dict(configuration=dict(training="baseline", beam_width=32), asset=str(old_asset),
                    asset_sha256=before["asset_sha256"], decoded_pcm_sha256=before["decoded_pcm_sha256"],
                    metrics=measure_error(reference, old_pcm, before["channels"]))
        flac = ROOT / "target/digital-music" / f"{name}-source.flac"
        flac.parent.mkdir(parents=True, exist_ok=True)
        flac_hash = regen.authenticate_source(music / "source" / provenance["source"]["filename"], flac, provenance, name)
        directory = work / "candidates" / name
        directory.mkdir(parents=True, exist_ok=True)
        # A different FLAC container is acceptable only after source PCM authentication.
        bound_metadata = dict(before, flac_sha256=flac_hash)
        reference_metadata_path = directory / "reference.json"
        regen.write_json(reference_metadata_path, bound_metadata)
        fingerprint = dict(common, profile=track["profile"], books=track["codebook_count"],
                           source_sha256=flac_hash, baseline_asset_sha256=before["asset_sha256"],
                           baseline_metadata_sha256=regen.digest(baseline / f"{name}.json"),
                           reference_pcm_sha256=before["reference_pcm_sha256"])
        candidates = []
        for training, width in CONFIGURATIONS:
            configuration = dict(training=training, beam_width=width)
            candidate = run_candidate(encoder, track, before, reference, reference_path, reference_metadata_path,
                                      flac, configuration, directory / f"{training}-{width}", fingerprint)
            if training == "baseline" and width == 32:
                if candidate.get("rejected") or candidate["asset_sha256"] != before["asset_sha256"]:
                    raise ValueError(f"{name}: baseline encoder no longer reproduces frozen DLT5 bytes")
            candidates.append(candidate)
        width_winner = choose_winner(base, [c for c in candidates if c["configuration"]["training"] == "baseline"])
        width = width_winner["configuration"]["beam_width"]
        candidates.append(run_candidate(encoder, track, before, reference, reference_path, reference_metadata_path,
                                        flac, dict(training="combined", beam_width=width),
                                        directory / f"combined-{width}", fingerprint))
        winner = choose_winner(base, candidates)
        gains = [after - old if after is not None and old is not None else None
                 for old, after in zip(base["metrics"]["snr_db"], winner["metrics"]["snr_db"])]
        print(f'{name}: selected {winner["configuration"]}, channel SNR gains {gains}', flush=True)
        return dict(name=name, baseline=base, candidates=candidates, winner=winner,
                    improved=winner is not base, channel_snr_gain_db=gains,
                    reference_pcm_sha256=before["reference_pcm_sha256"],
                    normalization_gain_db=before["normalization_gain_db"])

    results = {}
    tracks = [track for track in manifest["tracks"] if track["name"] in requested]
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for future in as_completed([pool.submit(experiment, track) for track in tracks]):
            result = future.result()
            results[result["name"]] = result
    report = dict(passed=True, complete_catalog=requested == set(names),
                  comparison="Exact whole-track squared error against the same authenticated normalized PCM; no channel regressions",
                  listening_review="not performed; numerical improvement is the adoption criterion",
                  inputs=common, tracks=[results[track["name"]] for track in tracks])
    report["summary"] = summarize_experiments(report["tracks"])
    if report["complete_catalog"]:
        staged, catalog = stage_winners(work, baseline, manifest, results, encoder, source_hashes, ffmpeg_version)
        report.update(used_bytes=catalog["used_bytes"], remaining_bytes=digital.MUSIC_CAPACITY - catalog["used_bytes"],
                      selected_root=str(staged))
        if args.install:
            install_selection(staged, manifest)
            report["installed"] = True
    regen.write_json(work / "quality-report.json", report)
    print(f'Improved {sum(result["improved"] for result in results.values())}/{len(results)} tracks; '
          f'report: {work / "quality-report.json"}', flush=True)


if __name__ == "__main__":
    main()
