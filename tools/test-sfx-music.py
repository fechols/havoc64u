"""Check offline DLT5 auditions against independently authored PCM sequences."""
from pathlib import Path
import runpy
import struct
import tempfile
import unittest


PREVIEW = runpy.run_path(str(Path(__file__).with_name("sfx-previews.py")))


def fixture(channels):
    # Three packing groups split into two books: the first book gets two.
    # Both profiles end in a partial group and loop through a predictor reset.
    rate, frames = (16000, 21) if channels == 1 else (20000, 11)
    deltas = ((256,), (-512,)) if channels == 1 else ((256, -256), (-256, 512))
    payload = bytes((frames * channels * 3 + 7) // 8)
    header = b"DLT5" + struct.pack("<IIIHHBB", rate, frames, len(payload), 8, 2, 3, channels) + bytes(10)
    books = b"".join(struct.pack("<8h", delta, *([0] * 7)) for segment in deltas for delta in segment)
    if channels == 1:
        pcm = [(value,) for value in (*range(1, 17), 14, 12, 10, 8, 6)]
    else:
        pcm = [(value, -value) for value in range(1, 9)] + [(7, -6), (6, -4), (5, -2)]
    return header + books + payload + b"\0", rate, pcm


class MusicAuditionTests(unittest.TestCase):
    def test_balanced_books_partial_tails_loops_and_mono_delay(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "music.dlt"
            for channels in (1, 2):
                blob, rate, pcm = fixture(channels)
                path.write_bytes(blob)
                for offset in (0, 320):
                    with self.subTest(channels=channels, offset=offset):
                        actual, metadata = PREVIEW["music_excerpt"](
                            path, offset / rate, seconds=200 / rate, rate=rate)
                        expected = []
                        for frame in range(offset, offset + 200):
                            if channels == 1:
                                left = pcm[frame % len(pcm)][0]
                                right = 0 if frame < 160 else pcm[(frame - 160) % len(pcm)][0]
                            else:
                                left, right = pcm[frame % len(pcm)]
                            expected.extend((left * 256, right * 256))
                        self.assertEqual(list(actual), expected)
                        self.assertEqual(metadata["mono_right_delay_samples"], 160 if channels == 1 else 0)

    def test_old_storage_formats_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "legacy.dlt"
            blob, _, _ = fixture(1)
            for version in b"234":
                path.write_bytes(blob[:3] + bytes((version,)) + blob[4:])
                with self.subTest(version=version), self.assertRaisesRegex(RuntimeError, "DLT5"):
                    PREVIEW["music_excerpt"](path, 0)


if __name__ == "__main__":
    unittest.main()
