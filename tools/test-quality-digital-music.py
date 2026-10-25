"""Independent decoder, full-track selection, fixed-reference and cache checks."""
from array import array
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import wave

SPEC = importlib.util.spec_from_file_location("quality_music", Path(__file__).with_name("quality-digital-music.py"))
quality = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(quality)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def wav(path, pcm, channels=1):
    with wave.open(str(path), "wb") as output:
        output.setnchannels(channels)
        output.setsampwidth(2)
        output.setframerate(16000 if channels == 1 else 20000)
        output.writeframes(quality.pcm_bytes(array("h", pcm)))


def candidate(reference, decoded, channels=1, **extra):
    return dict(metrics=quality.measure_error(reference, decoded, channels), **extra)


def fixture(channels=1, frames=17, deltas=(256, -512)):
    books = 2
    tables = b"".join(struct.pack("<8h", *([deltas[segment] * (channel + 1)] * 8))
                      for segment in range(books) for channel in range(channels))
    stream = bytes((frames * channels * 3 + 7) // 8)
    rate = 16000 if channels == 1 else 20000
    header = b"DLT5" + struct.pack("<IIIHHBB", rate, frames, len(stream), 8, books, 3, channels) + bytes(10)
    # All packed codes are zero and their CRC is zero.
    blob = header + tables + stream + bytes((len(stream) + 1023) // 1024)
    base, longer = quality.digital.segment_layout(frames, books, channels)
    metadata = dict(sample_rate=rate, sample_count=frames, channels=channels, bits_per_sample=3,
                    output_bits_per_sample=8, codebook_entries=8, codebook_count=books,
                    samples_per_segment=base, long_segment_count=longer, header_bytes=32,
                    codebook_bytes=len(tables), stream_bytes=len(stream), stream_offset=32 + len(tables),
                    crc_offset=32 + len(tables) + len(stream), crc_count=1, asset_bytes=len(blob),
                    asset_sha256=sha(blob), header_checksum=quality.digital.checksum(header),
                    codebook_checksum=quality.digital.checksum(tables), crc_table_checksum=0,
                    asset_checksum=quality.digital.checksum(blob), flac_sha256=sha(b"source"),
                    normalization_gain_db=0, matched_level_snr_db=20,
                    encoder_training="som", encoder_beam_width=32, fixed_reference=True)
    return blob, metadata


class QualityMusicTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="test-digital-quality-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_independent_decoder_keeps_predictor_across_balanced_segments(self):
        blob, metadata = fixture()
        quality.digital.validate_blob(blob, metadata, "mono16k3")
        # ceil(17/8)=3 packing groups: first book covers16 frames, second one.
        expected = [256 * (i + 1) for i in range(16)] + [3584]
        self.assertEqual(list(quality.decode_pcm(blob)), expected)

    def test_independent_decoder_stereo_channels_and_partial_tail(self):
        blob, metadata = fixture(channels=2)
        quality.digital.validate_blob(blob, metadata, "stereo20k3")
        # ceil(17/4)=5 groups: the first book covers12 frames.
        expected = []
        left = right = 0
        for frame in range(17):
            delta = 256 if frame < 12 else -512
            left += delta
            right += delta * 2
            expected.extend((left, right))
        self.assertEqual(list(quality.decode_pcm(blob)), expected)

    def test_independent_decoder_saturates_and_rounds_signed_extrema(self):
        blob, _ = fixture(deltas=(32767, -32768))
        samples = quality.decode_pcm(blob)
        self.assertEqual(samples[0], 32512)
        self.assertEqual(samples[15], 32512)
        self.assertEqual(samples[16], 0)  # predictor -1 rounds to zero

    def test_exact_error_totals_select_real_improvement_and_retain_ties(self):
        reference = [1000, 2000, -500]
        baseline = candidate(reference, [900, 1900, -400], tag="baseline")
        worse = candidate(reference, [850, 1850, -350])
        better = candidate(reference, [950, 1950, -450], tag="first")
        tie = candidate(reference, [1050, 2050, -550], tag="tie")
        self.assertIs(quality.choose_winner(baseline, [worse, better, tie]), better)
        self.assertIs(quality.choose_winner(baseline, [copy.deepcopy(baseline)]), baseline)

    def test_stereo_total_gain_cannot_hide_one_channel_regression(self):
        reference = [1000, 1000] * 4
        baseline = candidate(reference, [900, 900] * 4, channels=2)
        skewed = candidate(reference, [1000, 860] * 4, channels=2)
        self.assertLess(sum(skewed["metrics"]["error_sum_squares"]), sum(baseline["metrics"]["error_sum_squares"]))
        self.assertFalse(quality.eligible(baseline, skewed))

    def test_local_improvement_does_not_override_full_track_regression(self):
        reference = [1000] * 8
        baseline = candidate(reference, [900] * 8)
        # First four frames improve; a different ending predictor damages the rest.
        locally_better = candidate(reference, [990] * 4 + [800] * 4)
        self.assertIs(quality.choose_winner(baseline, [locally_better]), baseline)

    def test_changed_reference_is_not_an_snr_win(self):
        baseline = candidate([1000] * 4, [900] * 4)
        quieter = candidate([500] * 4, [490] * 4)
        self.assertFalse(quality.eligible(baseline, quieter))
        self.assertIs(quality.choose_winner(baseline, [dict(rejected="peak_limit")]), baseline)

    def test_summary_means_pooling_and_method_fallbacks(self):
        baseline_a = candidate([1000] * 2, [900] * 2, configuration=dict(training="baseline", beam_width=32))
        baseline_b = candidate([2000] * 2, [1800] * 2, configuration=dict(training="baseline", beam_width=32))
        som_a = candidate([1000] * 2, [950] * 2, configuration=dict(training="som", beam_width=32))
        som_b = dict(configuration=dict(training="som", beam_width=32), rejected="peak_limit")
        combined_a = candidate([1000] * 2, [975] * 2, configuration=dict(training="combined", beam_width=64))
        combined_b = candidate([2000] * 2, [1900] * 2, configuration=dict(training="combined", beam_width=128))
        tracks = [dict(baseline=baseline_a, winner=combined_a, candidates=[som_a, combined_a]),
                  dict(baseline=baseline_b, winner=combined_b, candidates=[som_b, combined_b])]
        summary = quality.summarize_experiments(tracks)
        self.assertAlmostEqual(summary["baseline"]["arithmetic_mean_snr_db"], 20)
        self.assertAlmostEqual(summary["selected"]["arithmetic_mean_snr_db"], (32.04119982655925 + 26.020599913279625) / 2)
        self.assertAlmostEqual(summary["selected"]["mean_gain_db"], 9.030899869919436)
        self.assertAlmostEqual(summary["selected"]["pooled_snr_db"], quality.snr(10000000, 21250))
        self.assertEqual(summary["improved_tracks"], 2)
        self.assertEqual(summary["selected_strategy_counts"], {"combined-64": 1, "combined-128": 1})
        self.assertAlmostEqual(summary["methods"]["som-32"]["arithmetic_mean_snr_db"], 23.010299956639813)
        self.assertEqual(summary["methods"]["som-32"]["baseline_fallback_tracks"], 1)
        self.assertEqual(summary["methods"]["combined"]["beam_width_counts"], {"64": 1, "128": 1})

    def test_summary_rejects_method_channel_regression(self):
        baseline = candidate([1000, 1000] * 4, [900, 900] * 4, channels=2,
                             configuration=dict(training="baseline", beam_width=32))
        trial = candidate([1000, 1000] * 4, [1000, 860] * 4, channels=2,
                          configuration=dict(training="som", beam_width=32))
        summary = quality.summarize_experiments([dict(baseline=baseline, winner=baseline, candidates=[trial])])
        self.assertEqual(summary["methods"]["som-32"]["mean_gain_db"], 0)
        self.assertEqual(summary["methods"]["som-32"]["baseline_fallback_tracks"], 1)

    def test_summary_explicitly_represents_perfect_silent_and_undefined_values(self):
        baseline = candidate([1000], [900], configuration=dict(training="baseline", beam_width=32))
        perfect = candidate([1000], [1000], configuration=dict(training="som", beam_width=32))
        summary = quality.summarize_experiments([dict(baseline=baseline, winner=perfect, candidates=[perfect])])
        self.assertIsNone(summary["selected"]["arithmetic_mean_snr_db"])
        self.assertEqual(summary["selected"]["arithmetic_mean_snr_status"], "positive_infinity")
        self.assertEqual(summary["selected"]["mean_gain_status"], "positive_infinity")
        self.assertEqual(summary["selected"]["finite_track_count"], 0)
        silent = candidate([0], [0], configuration=dict(training="baseline", beam_width=32))
        summary = quality.summarize_experiments([dict(baseline=silent, winner=silent, candidates=[])])
        self.assertEqual(summary["selected"]["arithmetic_mean_snr_status"], "undefined")
        self.assertEqual(summary["selected"]["pooled_snr_status"], "undefined")
        self.assertEqual(quality.snr_value(0, 1)["status"], "negative_infinity")
        self.assertEqual(quality.mean_values([quality.snr_value(1, 0), quality.snr_value(0, 1)])["status"], "undefined")

    def test_reference_authentication_checks_pcm_not_container_bytes(self):
        path = self.root / "reference.wav"
        pcm = array("h", [1, -2, 32767])
        wav(path, pcm)
        metadata = dict(channels=1, sample_rate=16000, sample_count=3)
        self.assertEqual(quality.read_pcm(path, metadata, sha(quality.pcm_bytes(pcm))), pcm)
        with self.assertRaisesRegex(ValueError, "PCM hash mismatch"):
            quality.read_pcm(path, metadata, sha(b"wrong"))
        with self.assertRaisesRegex(ValueError, "format mismatch"):
            quality.read_pcm(path, dict(metadata, sample_count=4), sha(quality.pcm_bytes(pcm)))

    def prepared_candidate(self):
        blob, metadata = fixture()
        decoded = quality.decode_pcm(blob)
        reference = array("h", (sample + 10 for sample in decoded))
        metrics = quality.measure_error(reference, decoded, 1)
        metadata.update(decoded_pcm_sha256=sha(quality.pcm_bytes(decoded)),
                        reference_pcm_sha256=sha(quality.pcm_bytes(reference)),
                        channel_metrics=[dict(channel="mono", matched_level_snr_db=20,
                                              decoded_sample_peak_dbfs=-10, decoded_true_peak_upper_bound_dbtp=-9,
                                              signal_sum_squares=metrics["signal_sum_squares"][0],
                                              error_sum_squares=metrics["error_sum_squares"][0])])
        asset, preview = self.root / "alpha.dlt", self.root / "alpha-dlt5.wav"
        asset.write_bytes(blob)
        quality.regen.write_json(asset.with_suffix(".json"), metadata)
        wav(preview, decoded)
        wav(preview.with_suffix(".reference.wav"), reference)
        # The production delayed-WAV validator is covered by regeneration tests.
        return asset, preview, metadata, reference

    def test_candidate_rejects_gain_size_peak_and_error_claim_changes(self):
        asset, preview, metadata, reference = self.prepared_candidate()
        track = dict(name="alpha", profile="mono16k3")
        config = dict(training="som", beam_width=32)
        with patch.object(quality.regen, "validate_previews"):
            quality.validate_candidate(asset, preview, track, metadata, reference, config, metadata["flac_sha256"])
            mutations = (
                (lambda m: m.update(normalization_gain_db=-1), "fixed reference or gain"),
                (lambda m: m.update(output_bits_per_sample=16), "changed output_bits"),
                (lambda m: m["channel_metrics"][0].update(decoded_true_peak_upper_bound_dbtp=-0.9), "peak limit"),
                (lambda m: m["channel_metrics"][0].update(error_sum_squares=1), "independent measurement"),
            )
            for mutate, message in mutations:
                changed = copy.deepcopy(metadata)
                mutate(changed)
                quality.regen.write_json(asset.with_suffix(".json"), changed)
                with self.subTest(message=message), self.assertRaisesRegex(ValueError, message):
                    quality.validate_candidate(asset, preview, track, metadata, reference, config, metadata["flac_sha256"])

    def test_cache_reuses_only_exact_fingerprint_and_authenticated_outputs(self):
        directory = self.root / "trial"
        directory.mkdir()
        log = directory / "encoder.log"
        log.write_text(quality.PEAK_REJECTION)
        config = dict(training="som", beam_width=32)
        fingerprint = dict(source_sha256="source", encoder_sha256="one")
        result = dict(configuration=config, rejected="peak_limit")
        quality.regen.write_json(directory / "result.json", dict(
            fingerprint=dict(fingerprint, configuration=config), result=result, log_sha256=quality.regen.digest(log)))
        args = (self.root / "encoder", dict(name="alpha", profile="mono16k3", codebook_count=2), {}, [],
                self.root / "reference.wav", self.root / "metadata.json", self.root / "source.flac", config, directory)
        with patch.object(quality.subprocess, "run") as run:
            self.assertEqual(quality.run_candidate(*args, fingerprint), result)
            run.assert_not_called()

        def reject(command, stdout, **kwargs):
            stdout.write(quality.PEAK_REJECTION)
            return subprocess.CompletedProcess(command, 1)

        with patch.object(quality.subprocess, "run", side_effect=reject) as run:
            quality.run_candidate(*args, dict(fingerprint, encoder_sha256="two"))
            self.assertEqual(run.call_count, 1)
            log.write_text("modified cached evidence")
            quality.run_candidate(*args, dict(fingerprint, encoder_sha256="two"))
            self.assertEqual(run.call_count, 2)

    def test_actual_encoder_failure_is_not_classified_as_peak_rejection(self):
        def fail(command, stdout, **kwargs):
            stdout.write("out of memory")
            return subprocess.CompletedProcess(command, 1)
        with patch.object(quality.subprocess, "run", side_effect=fail), self.assertRaisesRegex(RuntimeError, "encoder failed"):
            quality.run_candidate(self.root / "encoder", dict(name="alpha", profile="mono16k3", codebook_count=2),
                                  {}, [], self.root / "r.wav", self.root / "r.json", self.root / "s.flac",
                                  dict(training="som", beam_width=32), self.root / "failed", dict(source_sha256="hash"))

    def test_install_restores_baseline_fallback_after_a_previous_winner(self):
        staged = self.root / "staged"
        live_music = self.root / "assets/music"
        staged_music = staged / "assets/music"
        live_music.mkdir(parents=True)
        staged_music.mkdir(parents=True)
        (self.root / "src").mkdir()
        (self.root / "target/digital-music").mkdir(parents=True)
        for suffix in ("dlt", "json", "provenance.json"):
            (live_music / f"alpha.{suffix}").write_bytes(b"previous winner")
            (staged_music / f"alpha.{suffix}").write_bytes(b"preserved baseline")
        (staged_music / "catalog.json").write_bytes(b"selected catalog")
        (staged / "digital_catalog.rs").write_bytes(b"selected descriptors")
        with patch.object(quality, "ROOT", self.root), patch.object(quality.regen, "ROOT", self.root):
            quality.install_selection(staged, dict(tracks=[dict(name="alpha")]))
        for suffix in ("dlt", "json", "provenance.json"):
            self.assertEqual((live_music / f"alpha.{suffix}").read_bytes(), b"preserved baseline")
        self.assertEqual((self.root / "src/digital_catalog.rs").read_bytes(), b"selected descriptors")


if __name__ == "__main__":
    unittest.main()
