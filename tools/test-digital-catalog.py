"""Independent DLT4 bit/extent checks and manifest capacity rejection cases."""
import copy
import hashlib
import json
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

import digital_format as digital


def fixture(frames, channels):
    rate = 16000 if channels == 1 else 20000
    codes = [(index * 5 + 3) & 7 for index in range(frames * channels)]
    # Independently construct the entire bit string, then partition into bytes.
    bits = ''.join(f'{code:03b}'[::-1] for code in codes)
    payload = bytes(int(bits[i:i + 8].ljust(8, '0')[::-1], 2) for i in range(0, len(bits), 8))
    books = b''.join(struct.pack('<HB', i * 1024, 0) for _ in range(channels) for i in range(8))
    header = b'DLT4' + struct.pack('<IIIHHBB', rate, frames, len(payload), 8, 1, 3, channels) + bytes(10)
    stream_offset = len(header) + len(books)
    crc = 0
    crc_bytes = []
    for begin in range(0, len(payload), 1024):
        for byte in payload[begin:begin + 1024]:
            # Bitwise oracle, independent of the validator's table.
            crc ^= byte
            for _ in range(8):
                crc = ((crc << 1) ^ (7 if crc & 128 else 0)) & 255
        crc_bytes.append(crc)
    blob = header + books + payload + bytes(crc_bytes)
    metadata = dict(sample_rate=rate, sample_count=frames, channels=channels,
                    bits_per_sample=3, codebook_entries=8, codebook_count=1,
                    samples_per_segment=frames, stream_bytes=len(payload),
                    stream_offset=stream_offset, crc_offset=stream_offset + len(payload),
                    crc_count=len(crc_bytes), asset_bytes=len(blob),
                    asset_sha256=hashlib.sha256(blob).hexdigest(),
                    header_checksum=digital.checksum(header), codebook_checksum=digital.checksum(books),
                    crc_table_checksum=digital.checksum(crc_bytes), asset_checksum=digital.checksum(blob))
    return blob, metadata


class DigitalCatalogTests(unittest.TestCase):
    def test_every_partial_byte_and_crc_boundary(self):
        for channels in (1, 2):
            for frames in (*range(1, 34), 1365, 1366, 2730, 2731):
                with self.subTest(channels=channels, frames=frames):
                    blob, metadata = fixture(frames, channels)
                    profile = 'mono16k3' if channels == 1 else 'stereo20k3'
                    digital.validate_blob(blob, metadata, profile)
                    used = frames * channels * 3 % 8
                    if used:
                        damaged = bytearray(blob)
                        damaged[metadata['crc_offset'] - 1] |= 1 << used
                        with self.assertRaisesRegex(AssertionError, 'padding'):
                            digital.validate_blob(damaged, metadata, profile)
                    with self.assertRaisesRegex(AssertionError, 'extent'):
                        digital.validate_blob(blob[:-1], metadata, profile)

    def test_rejects_profiles_and_metadata_disagreement(self):
        blob, metadata = fixture(11, 1)
        with self.assertRaisesRegex(AssertionError, 'profile'):
            digital.validate_blob(blob, metadata, 'stereo20k3')
        for field in ('sample_count', 'channels', 'codebook_count', 'samples_per_segment',
                      'stream_offset', 'crc_offset', 'asset_bytes'):
            wrong = dict(metadata)
            wrong[field] += 1
            with self.subTest(field=field), self.assertRaisesRegex(AssertionError, 'metadata'):
                digital.validate_blob(blob, wrong, 'mono16k3')

    def test_manifest_rejects_duplicates_unknown_mappings_and_overflow(self):
        manifest_path = digital.ROOT / 'assets/music/catalog.json'
        original_read = Path.read_text
        manifest = json.loads(original_read(manifest_path))

        def check(changed, expected, oversized=False):
            def read(path, *args, **kwargs):
                if path == manifest_path:
                    return json.dumps(changed)
                value = original_read(path, *args, **kwargs)
                if oversized and path.suffix == '.json':
                    metadata = json.loads(value)
                    metadata['asset_bytes'] = digital.MUSIC_CAPACITY - 32
                    return json.dumps(metadata)
                return value
            with patch.object(Path, 'read_text', read), self.assertRaisesRegex(AssertionError, expected):
                digital.load_catalog(validate=False)

        changed = copy.deepcopy(manifest)
        changed['tracks'][1] = changed['tracks'][0]
        check(changed, 'duplicate')
        changed = copy.deepcopy(manifest)
        changed['cues']['NotACue'] = changed['default_gameplay']
        check(changed, 'unknown soundtrack cue')
        changed = copy.deepcopy(manifest)
        changed['cues']['Title'] = 'missing-song'
        check(changed, 'unknown cue track')
        for stages in (None, {}, [], manifest['stages'][:-1], manifest['stages'] + [None]):
            changed = copy.deepcopy(manifest)
            changed['stages'] = stages
            check(changed, 'stage count')
        for track in ('missing-song', 255, [], {}):
            changed = copy.deepcopy(manifest)
            changed['stages'][0] = track
            check(changed, 'unknown stage track')
        check(manifest, 'music budget', oversized=True)

    def test_stage_directory_layout_and_corruption_are_bound_to_manifest(self):
        catalog = digital.load_catalog(validate=False)
        directory = catalog['directory']
        cue_at = 16 + 16 * len(catalog['tracks'])
        stage_at = cue_at + len(digital.CUES)
        self.assertEqual(directory[:8], b'DMC1' + bytes((2, 10, 21, 16)))
        self.assertEqual(struct.unpack_from('<H', directory, 8)[0], 224)
        self.assertEqual(directory[10:14], bytes((18, 0, 0, 0)))
        self.assertEqual(list(directory[cue_at:stage_at]), catalog['cues'])
        self.assertEqual(list(directory[stage_at:stage_at + 18]), [3, 4, 5, 6, 7, 8, 9] + [255] * 11)
        self.assertFalse(any(directory[stage_at + 18:]))
        unsigned = bytearray(directory)
        unsigned[14:16] = b'\0\0'
        self.assertEqual(struct.unpack_from('<H', directory, 14)[0], digital.checksum(unsigned))
        reu = bytearray(digital.REU_BYTES)
        reu[digital.DIGITAL_OFFSET:digital.DIGITAL_OFFSET + len(directory)] = directory
        for track in catalog['tracks']:
            reu[track['offset']:track['end']] = track['blob']
        with patch.object(digital, 'load_catalog', return_value=catalog):
            digital.validate_digital_payload(reu)
            for offset in (4, 10, 11, 12, 13, 14, stage_at, stage_at + 6, stage_at + 7, len(directory) - 1):
                with self.subTest(offset=offset):
                    reu[digital.DIGITAL_OFFSET + offset] ^= 1
                    with self.assertRaisesRegex(AssertionError, 'directory mismatch'):
                        digital.validate_digital_payload(reu)
                    reu[digital.DIGITAL_OFFSET + offset] ^= 1

    def test_projection_includes_books_crc_directory_and_alignment(self):
        # Eventual complete soundtrack, excluding the temporary mono duplicate:
        # twenty-one tracks, two actual stereo songs plus 27 mono minutes.
        stereo = [json.loads((digital.ROOT / f'assets/music/{name}.json').read_text())
                  for name in ('galactic-dash', 'hammond-rocker')]
        directory = (16 + 21 * 16 + len(digital.CUES) + digital.STAGE_COUNT + 15) & ~15
        mono_frames = 27 * 60 * 16000
        # Nineteen separate bitstreams can each have a partial final byte.
        mono_payload = (mono_frames * 3 + 7) // 8 + 18
        # Conservative per-track CRC ceiling and twenty inter-asset gaps.
        complete = (sum(m['asset_bytes'] for m in stereo) + mono_payload
                    + 19 * (32 + 521 * 8 * 3)
                    + (mono_payload + 1023) // 1024 + 18 + directory + 20 * 15)
        self.assertLessEqual(complete, digital.MUSIC_CAPACITY)
        self.assertGreater(digital.MUSIC_CAPACITY - complete, 120000)


if __name__ == '__main__':
    unittest.main()
