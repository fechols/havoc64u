"""Reject unsafe packed REU reservations before the packer writes an image."""
import importlib.util
from pathlib import Path
import unittest
import json


spec = importlib.util.spec_from_file_location('packer', Path(__file__).with_name('pack-overlays.py'))
packer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packer)


def layout():
    addresses = (0x10000, 0x20000, 0x30000, 0x40000, 0x50000, 0x70000,
                 0x80000, 0x90000, 0xa0000, 0x15000, 0x1a000, 0x25000,
                 0x2a000, 0x35000, 0x3a000, 0x55000, 0x5a000, 0x60000,
                 0x65000, 0x6a000, 0x75000, 0x7a000, 0x85000, 0x8a000)
    symbols = {'__reu_size': 0x1000000, '__reu_code_slot_size': 0x4200,
               '__reu_models': 0xb0000, '__reu_music': 0xc0000,
               '__reu_digital_music': 0x100000, '__reu_digital_ring': 0xfe0000,
               '__reu_digital_victory': 0x500000,
               '__reu_digital_ring_size': 0x20000,
               '__reu_music_kernels': 0xb9000, '__music_kernel_size': 0x300}
    symbols.update((f'__reu_{symbol}', address)
                   for (_, symbol, _), address in zip(packer.CODE_IMAGES, addresses))
    return symbols


class ReuLayout(unittest.TestCase):
    def test_digital_asset_integrity_and_mutable_ring(self):
        root = Path(__file__).resolve().parents[1]
        reu = bytearray(0x1000000)
        tracks = (('galactic-dash', 0x100000), ('hammond-rocker', 0x500000))
        for name, start in tracks:
            asset = (root / f'assets/music/{name}.dlt').read_bytes()
            reu[start:start + len(asset)] = asset
        result = packer.validate_digital_payload(reu)
        self.assertEqual(set(result), {name for name, _ in tracks})
        for name, start in tracks:
            metadata = result[name]
            for offset in (0, 4, 21, 22, 32, 80, metadata['stream_offset'], metadata['crc_offset']):
                with self.subTest(track=name, offset=offset):
                    reu[start + offset] ^= 1
                    with self.assertRaises(AssertionError):
                        packer.validate_digital_payload(reu)
                    reu[start + offset] ^= 1
        reu[0xfe0000] = 127
        reu[0xffffff] = 127
        with self.assertRaisesRegex(AssertionError, 'PCM ring'):
            packer.validate_digital_payload(reu)
        packer.validate_digital_payload(reu, empty_ring=False)

    def test_complete_package_fits_with_full_music_and_code_reservations(self):
        offsets, allocations = packer.checked_layout(layout())
        self.assertEqual(len(offsets), 24)
        # Code, models and music as before, plus the SMK1 directory and
        # twenty-one 768-byte kernel images between the models and the score.
        self.assertEqual(sum(end - start for start, end, _ in allocations), 744832 + 256 + 21 * 0x300 + 0xf00000)
        self.assertFalse(any(start < 0xc0000 and end > 0xbfc00 for start, end, _ in allocations))
        self.assertIn((0xb9000, 0xb9000 + 256 + 21 * 0x300, 'music kernels'), allocations)

    def test_rejects_aliases_bank_crossings_and_out_of_bounds_reservations(self):
        for symbol, value, message in (
            ('__reu_scenery_gate', 0x10000, 'overlaps'),
            ('__reu_scenery_gate', 0x1c000, 'crosses a 64 KiB bank'),
            ('__reu_scenery_gate', 0x100000, 'exceeds REU'),
            ('__reu_scenery_gate', 0x8a100, '4 KiB aligned'),
            ('__reu_scenery_gate', 0x45000, 'exclusive REU bank'),
            ('__reu_music', 0xd0000, 'overlaps'),
            ('__reu_music', 0xb0000, 'overlaps'),
            ('__reu_models', 0xa0000, 'overlaps'),
            ('__reu_music_kernels', 0xb8000, 'overlaps'),
            ('__reu_music_kernels', 0xbd000, 'overlaps'),
            ('__music_kernel_size', 0x800, 'bounded DMA'),
            ('__reu_digital_music', 0xf0000, 'digital music address'),
            ('__reu_digital_victory', 0x200000, 'digital victory address'),
            ('__reu_digital_ring', 0x3e0000, 'digital ring address'),
            ('__reu_digital_ring_size', 0x10000, 'digital ring size'),
        ):
            with self.subTest(symbol=symbol, value=value):
                symbols = layout()
                symbols[symbol] = value
                with self.assertRaisesRegex(AssertionError, message):
                    packer.checked_layout(symbols)


if __name__ == '__main__':
    unittest.main()
