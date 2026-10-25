"""Exercise regeneration failures, cache integrity, and transactional installation."""
from contextlib import redirect_stderr, redirect_stdout
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import wave

import digital_format as digital

SPEC = importlib.util.spec_from_file_location("regenerate_music", Path(__file__).with_name("regenerate-digital-music.py"))
regen = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(regen)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fixture(books=2, legacy=False):
    frames, rate, channels = 16, 16000, 1
    book_data = bytes(books * 8 * (3 if legacy else 2))
    payload = bytes(6)
    header = (b"DLT4" if legacy else b"DLT5") + struct.pack("<IIIHHBB", rate, frames, len(payload), 8, books, 3, channels) + bytes(10)
    blob = header + book_data + payload + b"\0"
    metadata = dict(sample_count=frames, sample_rate=rate, channels=channels, bits_per_sample=3,
                    codebook_entries=8, codebook_count=books, samples_per_segment=frames // books,
                    long_segment_count=0, codebook_entry_bytes=3 if legacy else 2,
                    header_bytes=32, codebook_bytes=len(book_data), stream_bytes=len(payload),
                    stream_offset=32 + len(book_data), crc_offset=len(blob) - 1, crc_count=1,
                    asset_bytes=len(blob), asset_sha256=sha(blob), header_checksum=digital.checksum(header),
                    codebook_checksum=0, crc_table_checksum=0, asset_checksum=digital.checksum(blob),
                    flac_sha256=sha(b"flac"), decoded_pcm_sha256=sha(bytes(frames * 2)),
                    reference_pcm_sha256=sha(bytes(frames * 2)), matched_level_snr_db=20 if legacy else 22,
                    normalization_gain_db=0, normalization_attempts=1,
                    channel_metrics=[dict(channel="mono", matched_level_snr_db=20 if legacy else 22,
                                          decoded_sample_peak_dbfs=-2, decoded_true_peak_upper_bound_dbtp=-1.5)])
    return blob, metadata


def write_wav(path, channels, frames):
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(16000)
        wav.writeframes(bytes(frames * channels * 2))


class RegenerateMusicTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="test-dlt5-regeneration-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.music = self.root / "assets/music"
        self.work = self.root / "target/digital-music/work"
        self.baseline = self.root / "target/digital-music/baseline"
        self.music.mkdir(parents=True)
        (self.music / "source").mkdir()
        (self.root / "target/digital-music").mkdir(parents=True)
        (self.root / "tools/encode-digital-music/src").mkdir(parents=True)
        (self.root / "tools/encode-digital-music/src/main.rs").write_text("source")
        (self.root / "src").mkdir()
        (self.root / "src/digital_catalog.rs").write_text("original catalog")
        self.encoder = self.root / "encoder.exe"
        self.encoder.write_bytes(b"encoder")
        regen.write_json(self.music / "catalog.json", dict(tracks=[
            dict(name=name, profile="mono16k3", codebook_count=2) for name in ("alpha", "beta")]))
        for name in ("alpha", "beta"):
            blob, metadata = fixture(1, legacy=True)
            (self.music / f"{name}.dlt").write_bytes(blob)
            regen.write_json(self.music / f"{name}.json", metadata)
            regen.write_json(self.music / f"{name}.provenance.json", dict(
                source=dict(filename=f"{name}.m4a", sha256=sha(b"original"), sample_rate=48000,
                            channels=2, original_metadata={"comment": "historical comment"}),
                conversion=dict(flac_sha256=sha(b"flac")), tool_source=dict(note="legacy"),
                reproduction_commands=dict(encode="old command"),
                **{key: metadata[key] for key in ("asset_sha256", "decoded_pcm_sha256", "reference_pcm_sha256")}))
            (self.music / "source" / f"{name}.m4a").write_bytes(b"original")
            (self.root / "target/digital-music" / f"{name}-source.flac").write_bytes(b"flac")
        self.original = self.live_files()
        self.calls = []
        self.fail = None
        self.mutate_metadata = None
        self.flac_bytes = b"flac"

    def live_files(self):
        paths = list(self.music.glob("*.*")) + [self.root / "src/digital_catalog.rs"]
        return {str(path): path.read_bytes() for path in paths}

    def encode(self, command, **kwargs):
        if command[0] == "ffmpeg":
            Path(command[-1]).write_bytes(self.flac_bytes)
            return
        asset, preview = map(Path, command[-2:])
        name = asset.stem
        self.calls.append(name)
        if name == self.fail:
            raise subprocess.CalledProcessError(1, command)
        blob, metadata = fixture()
        metadata["flac_sha256"] = sha(Path(command[-3]).read_bytes())
        if "--training" in command:
            metadata.update(encoder_training=command[command.index("--training") + 1],
                            encoder_beam_width=int(command[command.index("--beam-width") + 1]),
                            fixed_reference=True)
        if self.mutate_metadata:
            self.mutate_metadata(metadata)
        asset.write_bytes(blob)
        regen.write_json(asset.with_suffix(".json"), metadata)
        write_wav(preview, 1, 16)
        write_wav(preview.with_suffix(".reference.wav"), 1, 16)
        write_wav(preview.with_suffix(".delayed.wav"), 2, 176)

    def run_command(self, *extra, work=None, baseline=None):
        arguments = ["regenerate", "--encoder", str(self.encoder), "--jobs", "1",
                     "--work-dir", str(work or self.work), "--baseline", str(baseline or self.baseline), *extra]
        with patch.object(regen, "ROOT", self.root), patch.object(sys, "argv", arguments), \
                patch.object(regen.subprocess, "check_output", return_value="FFmpeg test\n"), \
                patch.object(regen.subprocess, "run", side_effect=self.encode), \
                patch.object(digital, "load_catalog", return_value={"used_bytes": 512}), \
                patch.object(digital, "rust_catalog", return_value="new catalog"), \
                redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            regen.main()

    def test_custom_directories_create_intermediate_parent_on_clean_checkout(self):
        intermediates = self.root / "target/digital-music"
        for path in intermediates.glob("*-source.flac"):
            path.unlink()
        intermediates.rmdir()
        intermediates.parent.rmdir()
        self.run_command("--install", work=self.root / "alternate-work",
                         baseline=self.root / "alternate-baseline")
        self.assertEqual(self.calls, ["alpha", "beta"])
        self.assertEqual((intermediates / "alpha-source.flac").read_bytes(), b"flac")
        self.assertEqual((intermediates / "beta-source.flac").read_bytes(), b"flac")
        self.assertEqual((self.root / "src/digital_catalog.rs").read_text(), "new catalog")

    def test_container_retagging_refreshes_provenance_without_reencoding(self):
        self.run_command()
        original_baseline = {path.name: path.read_bytes() for path in self.baseline.iterdir()}
        source = self.music / "source/alpha.m4a"
        source.write_bytes(b"retagged original")
        with patch.object(regen, "source_layout", return_value=((48000, 2), {"artist": "Composer"})), \
                patch.object(regen, "decoded_pcm_digest", return_value=sha(b"same PCM")):
            self.run_command("--install")
        self.assertEqual(self.calls, ["alpha", "beta"])
        self.assertEqual({path.name: path.read_bytes() for path in self.baseline.iterdir()}, original_baseline)
        provenance = json.loads((self.music / "alpha.provenance.json").read_text())["source"]
        self.assertEqual(provenance["original_container_sha256"], sha(b"original"))
        self.assertEqual(provenance["sha256"], sha(b"retagged original"))
        self.assertEqual(provenance["pcm_sha256"], sha(b"same PCM"))
        self.assertEqual(provenance["original_metadata"], {"comment": "historical comment"})
        self.assertEqual(provenance["current_container_metadata"], {"artist": "Composer"})

    def test_changed_source_pcm_rejects_install_and_preserves_live_files(self):
        self.run_command()
        (self.music / "source/alpha.m4a").write_bytes(b"changed audio")
        with patch.object(regen, "source_layout", return_value=((48000, 2), {})), \
                patch.object(regen, "decoded_pcm_digest", side_effect=lambda path: sha(path.suffix.encode())):
            with self.assertRaisesRegex(ValueError, "recording PCM changed"):
                self.run_command("--install")
        self.assertEqual(self.live_files(), self.original)
        self.assertEqual(self.calls, ["alpha", "beta"])

    def test_missing_intermediate_uses_live_pcm_identity_after_retagging(self):
        self.run_command()
        original_baseline = {path.name: path.read_bytes() for path in self.baseline.iterdir()}
        (self.music / "source/alpha.m4a").write_bytes(b"retagged original")
        with patch.object(regen, "source_layout", return_value=((48000, 2), {"artist": "Composer"})), \
                patch.object(regen, "decoded_pcm_digest", return_value=sha(b"same PCM")):
            self.run_command("--install")
            flac = self.root / "target/digital-music/alpha-source.flac"
            flac.unlink()
            self.flac_bytes = b"recreated FLAC container, same PCM"
            self.run_command("--install")
        provenance = json.loads((self.music / "alpha.provenance.json").read_text())
        self.assertEqual(self.calls, ["alpha", "beta", "alpha"])
        self.assertEqual(provenance["source"]["sha256"], sha(b"retagged original"))
        self.assertEqual(provenance["source"]["original_container_sha256"], sha(b"original"))
        self.assertEqual(provenance["source"]["pcm_sha256"], sha(b"same PCM"))
        self.assertEqual(provenance["conversion"]["flac_sha256"], sha(self.flac_bytes))
        self.assertEqual({path.name: path.read_bytes() for path in self.baseline.iterdir()}, original_baseline)

    def test_inconsistent_live_provenance_is_rejected_before_source_authentication(self):
        self.run_command()
        path = self.music / "alpha.provenance.json"
        provenance = json.loads(path.read_text())
        provenance["asset_sha256"] = sha(b"wrong asset")
        regen.write_json(path, provenance)
        before = self.live_files()
        with patch.object(regen, "authenticate_source", wraps=regen.authenticate_source) as authenticate:
            with self.assertRaisesRegex(ValueError, "live asset, metadata and provenance disagree"):
                self.run_command("--install")
        self.assertNotIn("alpha", [call.args[3] for call in authenticate.call_args_list])
        self.assertEqual(self.live_files(), before)

    def test_changed_source_layout_rejects_even_equal_pcm_bytes(self):
        self.run_command()
        (self.music / "source/alpha.m4a").write_bytes(b"changed layout")
        with patch.object(regen, "source_layout", return_value=((44100, 2), {})), \
                patch.object(regen, "decoded_pcm_digest", return_value=sha(b"same PCM")) as decoder:
            with self.assertRaisesRegex(ValueError, "sample rate or channel layout changed"):
                self.run_command("--install")
        decoder.assert_not_called()
        self.assertEqual(self.live_files(), self.original)

    def test_recreated_intermediate_uses_recorded_pcm_authentication(self):
        # Model a fresh checkout whose committed provenance records the verified PCM.
        path = self.music / "alpha.provenance.json"
        provenance = json.loads(path.read_text())
        provenance["source"]["pcm_sha256"] = sha(b"same PCM")
        regen.write_json(path, provenance)
        flac = self.root / "target/digital-music/alpha-source.flac"
        flac.write_bytes(b"new FLAC container, same PCM")
        with patch.object(regen, "source_layout", return_value=((48000, 2), {})), \
                patch.object(regen, "decoded_pcm_digest", return_value=sha(b"same PCM")):
            self.run_command("--install")
        installed = json.loads(path.read_text())
        self.assertEqual(installed["conversion"]["flac_sha256"], sha(flac.read_bytes()))
        metadata = json.loads((self.music / "alpha.json").read_text())
        self.assertEqual(metadata["flac_sha256"], sha(flac.read_bytes()))
        old_metadata = json.loads((self.baseline / "alpha.json").read_text())
        self.assertEqual(old_metadata["flac_sha256"], sha(b"flac"))

    def test_untrusted_intermediate_fails_before_source_pcm_comparison(self):
        (self.music / "source/alpha.m4a").write_bytes(b"retagged original")
        (self.root / "target/digital-music/alpha-source.flac").write_bytes(b"untrusted intermediate")
        with patch.object(regen, "decoded_pcm_digest") as decoder:
            with self.assertRaisesRegex(ValueError, "without authenticated PCM"):
                self.run_command("--install")
        decoder.assert_not_called()
        self.assertEqual(self.live_files(), self.original)

    def test_encoder_failure_leaves_all_live_files_unchanged(self):
        self.fail = "beta"
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_command("--install")
        self.assertEqual(self.live_files(), self.original)
        self.assertTrue((self.work / "assets/music/alpha.dlt").exists())

    def test_quality_failure_leaves_all_live_files_unchanged(self):
        self.mutate_metadata = lambda metadata: metadata["channel_metrics"][0].update(matched_level_snr_db=19)
        with self.assertRaisesRegex(ValueError, "SNR regressed"):
            self.run_command("--install")
        self.assertEqual(self.live_files(), self.original)

    def test_valid_cache_is_reused_and_provenance_is_installed(self):
        self.run_command()
        self.run_command("--install")
        self.assertEqual(self.calls, ["alpha", "beta"])
        self.assertEqual((self.root / "src/digital_catalog.rs").read_text(), "new catalog")
        provenance = json.loads((self.music / "alpha.provenance.json").read_text())
        self.assertEqual(provenance["conversion"]["format"], "DLT5")
        self.assertEqual(provenance["conversion"]["codebook_count"], 2)
        self.assertEqual(provenance["source"]["sha256"], sha(b"original"))

    def test_corrupt_cache_assets_and_previews_are_regenerated(self):
        self.run_command()
        for path in (self.work / "assets/music/alpha.dlt", self.work / "alpha-dlt5.wav",
                     self.work / "alpha-dlt5.reference.wav", self.work / "alpha-dlt5.delayed.wav"):
            with self.subTest(path=path.name):
                data = bytearray(path.read_bytes())
                data[-1] ^= 1
                path.write_bytes(data)
                previous = len(self.calls)
                self.run_command()
                self.assertEqual(self.calls[previous:], ["alpha"])
                self.assertEqual(self.live_files(), self.original)

    def test_missing_channel_metrics_and_nonfinite_values_are_rejected(self):
        for mutate in (lambda metadata: metadata.update(channel_metrics=[]),
                       lambda metadata: metadata["channel_metrics"][0].update(matched_level_snr_db=float("nan")),
                       lambda metadata: metadata.update(normalization_gain_db=float("inf"))):
            _, metadata = fixture()
            mutate(metadata)
            with self.assertRaises(ValueError):
                regen.validate_quality(metadata, "broken")

    def test_overlapping_live_and_staged_paths_are_rejected_before_writing(self):
        with self.assertRaises(SystemExit):
            self.run_command("--install", work=self.root)
        self.assertEqual(self.live_files(), self.original)

    def test_install_failure_restores_completed_replacements(self):
        first, second = self.music / "alpha.dlt", self.music / "beta.dlt"
        source = self.root / "new.dlt"
        source.write_bytes(b"new")
        original_copy = regen.shutil.copyfile
        failed = False

        def fail_second(source_path, destination):
            nonlocal failed
            if destination == second and not failed:
                failed = True
                destination.write_bytes(b"partial write")
                raise OSError("simulated install failure")
            return original_copy(source_path, destination)

        with patch.object(regen, "ROOT", self.root), patch.object(regen.shutil, "copyfile", side_effect=fail_second):
            with self.assertRaisesRegex(OSError, "simulated"):
                regen.install_files([(source, first), (source, second)])
        self.assertEqual(self.live_files(), self.original)

    def test_failed_rollback_preserves_original_recovery_files(self):
        first, second = self.music / "alpha.dlt", self.music / "beta.dlt"
        source = self.root / "new.dlt"
        source.write_bytes(b"new")
        original_copy = regen.shutil.copyfile
        calls = 0

        def fail_after_first(source_path, destination):
            nonlocal calls
            if destination in (first, second):
                calls += 1
                if calls > 1:
                    raise OSError("storage unavailable")
            return original_copy(source_path, destination)

        with patch.object(regen, "ROOT", self.root), patch.object(regen.shutil, "copyfile", side_effect=fail_after_first):
            with self.assertRaisesRegex(RuntimeError, "recovery files remain"):
                regen.install_files([(source, first), (source, second)])
        recovery = list((self.root / "target/digital-music").glob("dlt5-install-*"))
        self.assertEqual(len(recovery), 1)
        self.assertEqual((recovery[0] / "0.old").read_bytes(), self.original[str(first)])
        self.assertEqual((recovery[0] / "1.old").read_bytes(), self.original[str(second)])

    def test_install_preserves_existing_destination_identity(self):
        destination = self.music / "alpha.dlt"
        identity = destination.stat().st_ino
        source = self.root / "new.dlt"
        source.write_bytes(b"new content")
        with patch.object(regen, "ROOT", self.root):
            regen.install_files([(source, destination)])
        self.assertEqual(destination.read_bytes(), b"new content")
        self.assertEqual(destination.stat().st_ino, identity)

    @unittest.skipIf(os.name == "nt", "POSIX mode bits are not supported on Windows")
    def test_install_preserves_posix_destination_permissions(self):
        destination = self.music / "alpha.dlt"
        destination.chmod(0o640)
        source = self.root / "new.dlt"
        source.write_bytes(b"new content")
        source.chmod(0o600)
        with patch.object(regen, "ROOT", self.root):
            regen.install_files([(source, destination)])
        self.assertEqual(destination.stat().st_mode & 0o777, 0o640)

    def test_inconsistent_baseline_is_rejected_without_installing(self):
        self.run_command()
        (self.baseline / "alpha.dlt").write_bytes(b"corrupt baseline")
        with self.assertRaisesRegex(ValueError, "baseline asset, metadata and provenance disagree"):
            self.run_command("--install")
        self.assertEqual(self.live_files(), self.original)

    def select_experimental_recipe(self):
        path = self.music / "catalog.json"
        manifest = json.loads(path.read_text())
        _, metadata = fixture()
        manifest["tracks"][0]["encoder_options"] = dict(
            training="som", beam_width=64,
            reference=dict(training="baseline", beam_width=32, codebook_count=2,
                           pcm_sha256=metadata["reference_pcm_sha256"], asset_sha256=metadata["asset_sha256"]))
        regen.write_json(path, manifest)
        return manifest

    def test_selected_recipe_reconstructs_reference_from_source_and_records_settings(self):
        self.select_experimental_recipe()
        self.run_command("--install")
        self.assertEqual(self.calls, ["alpha", "alpha", "beta"])
        provenance = json.loads((self.music / "alpha.provenance.json").read_text())
        self.assertEqual(provenance["conversion"]["encoder_training"], "som")
        self.assertEqual(provenance["conversion"]["encoder_beam_width"], 64)
        self.assertIn("--reference-wav", provenance["reproduction_commands"]["encode"])
        self.assertIn("reference", provenance["reproduction_commands"])
        # Rebuilding a missing reference does not require a preserved binary or WAV.
        reference = self.work / "reference-inputs/alpha.reference.wav"
        reference.unlink()
        self.run_command()
        self.assertEqual(self.calls, ["alpha", "alpha", "beta", "alpha"])
        self.assertTrue(reference.exists())

    def test_selected_recipe_rejects_a_reference_with_different_bytes(self):
        manifest = self.select_experimental_recipe()
        manifest["tracks"][0]["encoder_options"]["reference"]["pcm_sha256"] = sha(b"different reference")
        regen.write_json(self.music / "catalog.json", manifest)
        before = self.live_files()
        with self.assertRaisesRegex(ValueError, "regenerated reference differs"):
            self.run_command("--install")
        self.assertEqual(self.live_files(), before)

    def test_invalid_selected_recipe_is_rejected(self):
        manifest = self.select_experimental_recipe()
        manifest["tracks"][0]["encoder_options"]["beam_width"] = 256
        regen.write_json(self.music / "catalog.json", manifest)
        with self.assertRaisesRegex(ValueError, "invalid encoder options"):
            self.run_command()

    def test_report_identifies_authenticated_fixed_references(self):
        self.select_experimental_recipe()
        self.run_command()
        report = json.loads((self.work / "quality-report.json").read_text())
        self.assertTrue(report["all_tracks_same_reference"])
        self.assertIn("identical authenticated normalized PCM", report["snr_reference"])
        for track in report["tracks"]:
            self.assertTrue(track["same_reference"])
            self.assertEqual(track["old_reference_pcm_sha256"], track["reference_pcm_sha256"])
            self.assertIn("old_snr_db", track)
            self.assertIn("snr_gain_db", track)

    def test_report_keeps_caveat_when_even_one_reference_differs(self):
        # Equal gain metadata alone cannot prove that normalized PCM is identical.
        changed_hash = sha(b"different normalized baseline PCM")
        for suffix in ("json", "provenance.json"):
            path = self.music / f"alpha.{suffix}"
            metadata = json.loads(path.read_text())
            metadata["reference_pcm_sha256"] = changed_hash
            regen.write_json(path, metadata)
        self.run_command()
        report = json.loads((self.work / "quality-report.json").read_text())
        self.assertFalse(report["all_tracks_same_reference"])
        self.assertIn("not a fixed-reference comparison for every track", report["snr_reference"])
        alpha, beta = report["tracks"]
        self.assertFalse(alpha["same_reference"])
        self.assertTrue(beta["same_reference"])
        self.assertEqual(alpha["old_normalization_gain_db"], alpha["normalization_gain_db"])
        self.assertEqual(alpha["old_reference_pcm_sha256"], changed_hash)
        self.assertNotEqual(alpha["old_reference_pcm_sha256"], alpha["reference_pcm_sha256"])


if __name__ == "__main__":
    unittest.main()
