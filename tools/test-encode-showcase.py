#!/usr/bin/env python3
"""Exercise soundtrack mixing, silent captures and native timing with tiny reels."""
import array
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import wave
import zlib


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("encode_showcase", ROOT / "tools/encode-showcase.py")
encoder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(encoder)


def png(path, color):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 160, 200, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress((b"\0" + bytes(color) * 160) * 200)) + chunk(b"IEND", b""))


def wav(path, seconds, amplitude=0, frequency=997):
    samples = array.array("h")
    for index in range(round(seconds * 44100)):
        sample = round(amplitude * 32767 * math.sin(2 * math.pi * frequency * index / 44100))
        samples.extend((sample, sample))
    if sys.byteorder != "little":
        samples.byteswap()
    with wave.open(str(path), "wb") as stream:
        stream.setparams((2, 2, 44100, 0, "NONE", "not compressed"))
        stream.writeframes(samples.tobytes())


def fixture(directory, amplitude):
    scenes = []
    for index, name in enumerate(("first", "second")):
        scene_dir = directory / name
        scene_dir.mkdir()
        png(scene_dir / "frame.png", (index * 120, 80, 100))
        wav(scene_dir / "audio.wav", .4, amplitude)
        scene = {"passed": True, "trim_start_seconds": 0, "duration_seconds": .4,
                 "audio": {"path": "audio.wav", "retained_channels": [{"peak": round(amplitude * 32767)}] * 2},
                 "frames": [{"path": "frame.png", "seconds": time} for time in (0, .2, .4)]}
        (scene_dir / "scene.json").write_text(json.dumps(scene))
        scenes.append({"name": name, "manifest": f"{name}/scene.json", "included_in_video": True})
    manifest = directory / "manifest.json"
    manifest.write_text(json.dumps({"schema": 1, "sid_count": 4, "passed": True,
                                    "package_files_unchanged": True, "scenes": scenes}))
    return manifest


class GainTests(unittest.TestCase):
    def test_silence_and_legacy_gain(self):
        self.assertEqual(encoder.effects_normalization_db(0), 0)
        self.assertEqual(encoder.effects_normalization_db(1000), 6)
        self.assertAlmostEqual(encoder.effects_normalization_db(32768), 20 * math.log10(.7))


class EncoderIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            cls.ffmpeg = encoder.executable(os.environ.get("FFMPEG"), "ffmpeg")
            cls.ffprobe = encoder.executable(os.environ.get("FFPROBE"), "ffprobe")
        except RuntimeError as error:
            raise unittest.SkipTest(str(error))
        (ROOT / "target").mkdir(exist_ok=True)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="showcase-encoder-test-", dir=ROOT / "target")
        self.directory = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def encode(self, amplitude, music=None):
        manifest = fixture(self.directory, amplitude)
        command = [sys.executable, str(ROOT / "tools/encode-showcase.py"), str(manifest),
                   "--ffmpeg", self.ffmpeg, "--ffprobe", self.ffprobe]
        if music:
            command += ["--music", str(music)]
        result = subprocess.run(command, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        report = json.loads((self.directory / "encoded/report.json").read_text())
        self.assertTrue(report["passed"])
        self.assertAlmostEqual(report["duration_seconds"], .8)
        self.assertEqual([scene["output_frames"] for scene in report["scenes"]], [20, 20])
        self.assertLess(report["bytes"], 9_800_000)
        return report

    def test_silent_capture_without_overlay(self):
        report = self.encode(0)
        self.assertEqual(report["audio_gain_db"], 0)
        self.assertEqual(report["audio_mix"]["mode"], "native captured audio")
        self.assertNotIn("music", report["audio_mix"])
        self.assertNotIn("master", report)
        self.assertIsNone(report["audio_mix"]["final_true_peak"]["dbtp"])

    def test_native_audio_without_overlay_retains_legacy_balance(self):
        report = self.encode(.5)
        mix = report["audio_mix"]
        self.assertAlmostEqual(mix["native_normalization_db"], 20 * math.log10(.7 / .5))
        self.assertEqual(mix["effects_gain_db"], 0)
        self.assertNotIn("music", mix)
        self.assertEqual(mix["global_attenuation_db"], 0)
        self.assertLessEqual(mix["final_true_peak"]["upper_bound_dbtp"], -1)

    def test_silent_effects_allow_longer_music_trim(self):
        music = self.directory / "longer.wav"
        wav(music, 1.2, .5, 443)
        report = self.encode(0, music)
        mix = report["audio_mix"]
        self.assertEqual(mix["native_normalization_db"], 0)
        self.assertFalse(mix["music"]["full_source"])
        self.assertEqual(mix["music"]["retained_duration_seconds"], .8)
        self.assertEqual(mix["music"]["source_duration_seconds"], 1.2)
        self.assertEqual(mix["global_attenuation_db"], 0)
        self.assertIsNotNone(mix["final_true_peak"]["dbtp"])

    def test_continuous_full_music_and_quieter_effects_with_headroom(self):
        music = self.directory / "music.wav"
        wav(music, .8, 1.0, 443)
        report = self.encode(.5, music)
        mix = report["audio_mix"]
        self.assertEqual(mix["music"]["sha256"], hashlib.sha256(music.read_bytes()).hexdigest())
        self.assertTrue(mix["music"]["full_source"])
        self.assertEqual(mix["music"]["start_seconds"], 0)
        self.assertEqual(mix["music_gain_db"], 0)
        self.assertEqual(mix["effects_gain_db"], -12)
        self.assertFalse(mix["time_stretch"])
        self.assertLess(mix["global_attenuation_db"], 0)
        self.assertLessEqual(mix["final_true_peak"]["upper_bound_dbtp"], -1)
        self.assertTrue(Path(report["master"]["path"]).is_file())
        # At the scene boundary native effects fade out/in, but the soundtrack
        # must continue at its original phase and unity amplitude.
        pcm = subprocess.check_output([self.ffmpeg, "-v", "error", "-i",
             encoder.external_path(self.directory / "encoded/audio-mix.wav", self.ffmpeg),
             "-f", "f32le", "-acodec", "pcm_f32le", "-"])
        samples = array.array("f", pcm)
        if sys.byteorder != "little":
            samples.byteswap()
        boundary = round(.4 * 44100)
        for index in range(boundary - 2, boundary + 3):
            expected = math.sin(2 * math.pi * 443 * index / 44100)
            self.assertAlmostEqual(samples[index * 2], expected, delta=.01)

    def test_short_soundtrack_is_rejected(self):
        music = self.directory / "short.wav"
        wav(music, .2, .2)
        with self.assertRaisesRegex(ValueError, "too short"):
            encoder.mix_audio(self.directory / "unused.ffconcat", music, .8, 0, -12,
                              self.ffmpeg, self.ffprobe, self.directory)


if __name__ == "__main__":
    unittest.main()
