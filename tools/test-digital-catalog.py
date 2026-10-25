"""Independent DLT5 bit/extent checks and manifest capacity rejection cases."""
import copy
import hashlib
import json
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

import digital_format as digital


def fixture(frames, channels, count=1):
    rate = 16000 if channels == 1 else 20000
    codes = [(index * 5 + 3) & 7 for index in range(frames * channels)]
    # Independently construct the entire bit string, then partition into bytes.
    bits = ''.join(f'{code:03b}'[::-1] for code in codes)
    payload = bytes(int(bits[i:i + 8].ljust(8, '0')[::-1], 2) for i in range(0, len(bits), 8))
    books = b''.join(struct.pack('<h', delta) for _ in range(count * channels)
                     for delta in (-32768, -8192, -1024, -1, 0, 1, 8192, 32767))
    header = b'DLT5' + struct.pack('<IIIHHBB', rate, frames, len(payload), 8, count, 3, channels) + bytes(10)
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
    alignment = 8 // channels
    groups = (frames + alignment - 1) // alignment
    base, longer = divmod(groups, count)
    metadata = dict(sample_rate=rate, sample_count=frames, channels=channels,
                    bits_per_sample=3, codebook_entries=8, codebook_count=count,
                    samples_per_segment=base * alignment, long_segment_count=longer,
                    header_bytes=32, codebook_bytes=len(books), stream_bytes=len(payload),
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
        for field in ('sample_count', 'channels', 'codebook_count', 'samples_per_segment', 'long_segment_count',
                      'header_bytes', 'codebook_bytes',
                      'stream_offset', 'crc_offset', 'asset_bytes'):
            wrong = dict(metadata)
            wrong[field] += 1
            with self.subTest(field=field), self.assertRaisesRegex(AssertionError, 'metadata'):
                digital.validate_blob(blob, wrong, 'mono16k3')

    def test_large_compact_tables_and_legacy_format_rejection(self):
        for channels, books in ((1, 1025), (1, 4097), (2, 2049), (2, 65535)):
            alignment = 8 // channels
            frames = (books - 1) * alignment + 1
            blob, metadata = fixture(frames, channels, books)
            profile = 'mono16k3' if channels == 1 else 'stereo20k3'
            with self.subTest(channels=channels, books=books):
                digital.validate_blob(blob, metadata, profile)
                self.assertEqual(metadata['codebook_bytes'], books * channels * 16)
                self.assertEqual(metadata['samples_per_segment'], alignment)
                self.assertEqual(metadata['long_segment_count'], 0)
                self.assertEqual(struct.unpack_from('<h', blob, 32)[0], -32768)
                self.assertEqual(struct.unpack_from('<h', blob, 46)[0], 32767)
                legacy = b'DLT4' + blob[4:]
                with self.assertRaisesRegex(AssertionError, 'magic'):
                    digital.validate_blob(legacy, metadata, profile)
        blob, metadata = fixture(8, 1)
        for count in (0, 2, 65535):
            damaged = bytearray(blob)
            struct.pack_into('<H', damaged, 18, count)
            with self.subTest(count=count), self.assertRaisesRegex(AssertionError, 'codebook count'):
                digital.validate_blob(damaged, metadata, 'mono16k3')

    def test_balanced_segments_cover_frames_without_long_final_tail(self):
        for channels in (1, 2):
            alignment = 8 // channels
            for frames in range(1, 90):
                groups = (frames + alignment - 1) // alignment
                for books in range(1, groups + 1):
                    base, longer = digital.segment_layout(frames, books, channels)
                    spans = [base + alignment * (index < longer) for index in range(books)]
                    spans[-1] -= groups * alignment - frames
                    self.assertEqual(sum(spans), frames)
                    self.assertGreater(min(spans), 0)
                    self.assertLessEqual(max(spans) - min(spans), 2 * alignment - 1)
                    self.assertTrue(all(span % alignment == 0 for span in spans[:-1]))
                    self.assertLessEqual(spans[-1], max(spans[:-1], default=spans[-1]))

    def test_allocation_ties_limits_and_last_affordable_book(self):
        mono = dict(sample_count=16, sample_rate=16000, channels=1)
        stereo = dict(sample_count=20, sample_rate=20000, channels=2)
        # Two mono streams occupy 87 fixed bytes and at least 32 book bytes.
        result = digital.allocate_codebooks([mono, mono], capacity=135, directory_bytes=0)
        self.assertEqual(result, dict(counts=[2, 1], used_bytes=135, free_bytes=0, fixed_bytes=87))
        self.assertEqual(digital.allocate_codebooks([mono, mono], capacity=251, directory_bytes=0)['counts'], [2, 2])
        with self.assertRaisesRegex(AssertionError, 'music budget'):
            digital.allocate_codebooks([mono, mono], capacity=118, directory_bytes=0)
        # With only sixteen bytes left, a stereo book cannot consume the tail.
        mixed = digital.allocate_codebooks([stereo, mono], capacity=160, directory_bytes=0)
        self.assertEqual(mixed['counts'], [1, 2])
        self.assertLess(mixed['free_bytes'], 16)

    def test_current_allocation_uses_exact_budget_and_equal_intervals(self):
        manifest = json.loads((digital.ROOT / 'assets/music/catalog.json').read_text())
        metadata = [json.loads((digital.ROOT / f"assets/music/{track['name']}.json").read_text())
                    for track in manifest['tracks']]
        result = digital.allocate_codebooks(metadata)
        self.assertEqual(result, digital.allocate_codebooks(metadata))
        self.assertEqual(result['fixed_bytes'], 14371932)
        self.assertEqual(result['used_bytes'], digital.MUSIC_CAPACITY - 4)
        self.assertEqual(result['counts'], [track['codebook_count'] for track in manifest['tracks']])
        self.assertEqual(digital.MUSIC_CAPACITY, 16121856)
        self.assertEqual(result['counts'][:2], [7341, 10036])
        self.assertEqual(sum(result['counts']), 99334)
        self.assertEqual(sum(count * 16 * track['channels']
                             for track, count in zip(metadata, result['counts'])), 1749920)
        previous = digital.allocate_codebooks(metadata, capacity=digital.MUSIC_CAPACITY - 0x80000)
        self.assertEqual(result['used_bytes'] - previous['used_bytes'], 0x80000)
        intervals = [track['sample_count'] / track['sample_rate'] / count
                     for track, count in zip(metadata, result['counts'])]
        self.assertGreater(min(intervals), 0.0209)
        self.assertLess(max(intervals), 0.0210)

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

    def test_manifest_rejects_stale_authoring_count(self):
        manifest_path = digital.ROOT / 'assets/music/catalog.json'
        original_read = Path.read_text
        manifest = json.loads(original_read(manifest_path))
        metadata = json.loads(original_read(digital.ROOT / f"assets/music/{manifest['tracks'][0]['name']}.json"))
        manifest['tracks'][0]['codebook_count'] = metadata['codebook_count'] + 1

        def read(path, *args, **kwargs):
            return json.dumps(manifest) if path == manifest_path else original_read(path, *args, **kwargs)

        with patch.object(Path, 'read_text', read), patch.object(digital, 'validate_blob'):
            with self.assertRaisesRegex(AssertionError, 'codebook count differs from allocation'):
                digital.load_catalog()

    def test_stage_directory_layout_and_corruption_are_bound_to_manifest(self):
        catalog = digital.load_catalog(validate=False)
        directory = catalog['directory']
        cue_at = 16 + 16 * len(catalog['tracks'])
        stage_at = cue_at + len(digital.CUES)
        self.assertEqual(directory[:8], b'DMC1' + bytes((2, 21, 21, 16)))
        self.assertEqual(struct.unpack_from('<H', directory, 8)[0], 400)
        self.assertEqual(directory[10:14], bytes((18, 0, 0, 0)))
        self.assertEqual(list(directory[cue_at:stage_at]), catalog['cues'])
        self.assertEqual(list(directory[stage_at:stage_at + 18]),
                         [3, 4, 5, 6, 7, 8, 9, 17, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20])
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

    def test_complete_soundtrack_has_one_encoding_per_song_and_fits(self):
        catalog = digital.load_catalog()
        tracks = catalog['tracks']
        self.assertEqual(len(tracks), 21)
        self.assertFalse(any(track.get('fixture') for track in tracks))
        self.assertEqual([track['name'] for track in tracks[:3]],
                         ['galactic-dash', 'hammond-rocker', 'mission-failed'])
        self.assertEqual([track['name'] for track in tracks
                          if track['metadata']['channels'] == 2], ['hammond-rocker'])
        self.assertNotIn('galactic-dash-mono', [track['name'] for track in tracks])
        self.assertEqual(catalog['cues'][digital.CUES.index('Victory')], 0)
        self.assertEqual(catalog['cues'][digital.CUES.index('Defeat')], 2)
        self.assertEqual(catalog['cues'][digital.CUES.index('Ascent')], 0)
        self.assertEqual(len(set(catalog['stages'])), digital.STAGE_COUNT)
        self.assertTrue(all(0 <= stage < len(tracks) for stage in catalog['stages']))
        # Independently size every actual stream, book table, CRC and alignment gap.
        complete = 400
        for track in tracks:
            metadata = track['metadata']
            payload = (metadata['sample_count'] * metadata['channels'] * 3 + 7) // 8
            size = (32 + metadata['codebook_count'] * 8 * 2 * metadata['channels']
                    + payload + (payload + 1023) // 1024)
            self.assertEqual(size, len(track['blob']))
            complete = (complete + 15) // 16 * 16 + size
        self.assertEqual(complete, catalog['used_bytes'])
        self.assertLessEqual(complete, digital.MUSIC_CAPACITY)
        self.assertEqual(digital.MUSIC_CAPACITY - complete, 4)


if __name__ == '__main__':
    unittest.main()
