"""Reject unsafe packed REU reservations before the packer writes an image."""
import importlib.util
from pathlib import Path
import unittest
from digital_format import load_catalog, pack_catalog
from reu_layout import CODE_BASE, CODE_END, CODE_IMAGES, MUSIC_OFFSET, OFFSETS, RESERVATIONS, image_size_symbol


spec = importlib.util.spec_from_file_location('packer', Path(__file__).with_name('pack-overlays.py'))
packer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packer)


def layout():
    symbols = {'__reu_size': 0x1000000, '__reu_code_slot_size': 0x4200,
               '__reu_models': 0xb000, '__reu_models_end': CODE_BASE,
               '__reu_probe_start': 0xc00, '__reu_probe_end': 0x1000,
               '__reu_digital_music': MUSIC_OFFSET, '__reu_digital_ring': 0xfe0000,
               '__reu_digital_victory': MUSIC_OFFSET,
               '__reu_digital_ring_size': 0x20000}
    for name, symbol, _ in CODE_IMAGES:
        symbols[f'__reu_{symbol}'] = OFFSETS[name]
        symbols[f'__reu_{symbol}_end'] = OFFSETS[name] + RESERVATIONS[name]
        symbols[image_size_symbol(name)] = min(RESERVATIONS[name], 0x4200)
    return symbols


class ReuLayout(unittest.TestCase):
    def test_digital_asset_integrity_and_mutable_ring(self):
        root = Path(__file__).resolve().parents[1]
        reu = bytearray(0x1000000)
        catalog = pack_catalog(reu)
        self.assertEqual(catalog['end'], 0xfe0000 - 4)
        self.assertEqual(reu[catalog['end']:0xfe0000], bytes(4))
        tracks = [(entry['name'], entry['offset']) for entry in catalog['tracks']]
        result = packer.validate_digital_payload(reu)
        self.assertEqual(set(result), {name for name, _ in tracks})
        for name, start in tracks:
            metadata = result[name]
            for offset in (0, 4, 21, 22, 32, 80, metadata['stream_offset'] - 1,
                           metadata['stream_offset'], metadata['crc_offset']):
                with self.subTest(track=name, offset=offset):
                    reu[start + offset] ^= 1
                    with self.assertRaises(AssertionError):
                        packer.validate_digital_payload(reu)
                    reu[start + offset] ^= 1
        reu[catalog['end']] = 1
        with self.assertRaisesRegex(AssertionError, 'trailing padding'):
            packer.validate_digital_payload(reu)
        reu[catalog['end']] = 0
        reu[0xfe0000] = 127
        reu[0xffffff] = 127
        with self.assertRaisesRegex(AssertionError, 'PCM ring'):
            packer.validate_digital_payload(reu)
        packer.validate_digital_payload(reu, empty_ring=False)

    def test_complete_package_fits_with_full_music_and_code_reservations(self):
        offsets, allocations = packer.checked_layout(layout())
        self.assertEqual(len(offsets), 26)
        self.assertEqual((CODE_BASE, CODE_END), (0x14000, 0x7e800))
        self.assertEqual(MUSIC_OFFSET, 0x80000)
        self.assertEqual(sum(RESERVATIONS.values()), CODE_END - CODE_BASE)
        ordered = sorted(allocations)
        self.assertTrue(all(end <= following[0]
                            for (_, end, _), following in zip(ordered, ordered[1:])))
        self.assertTrue(any(start >> 16 != (end - 1) >> 16
                            for start, end, name in allocations if name in offsets))
        pixels_bank = offsets['pixels'] >> 16
        self.assertTrue(any(name != 'pixels' and offset >> 16 == pixels_bank
                            for name, offset in offsets.items()))

    def test_rejects_invalid_reservations_and_image_overflow(self):
        for symbol, value, message in (
            ('__reu_scenery_gate', 0x80000, 'exceeds image'),
            ('__reu_scenery_gate', OFFSETS['scenery-gate'] + 1, '2 KiB aligned'),
            ('__reu_scenery_gate_end', OFFSETS['scenery-gate'] + 0x4000, 'reservation size'),
            ('__reu_models', 0xa0000, 'model bank address'),
            ('__reu_models_end', CODE_BASE + 1, 'model bank end'),
            ('__reu_probe_start', 0x100, 'probe reservation'),
            ('__reu_probe_end', 0x1001, 'probe reservation'),
            ('__phase_boot_size', 0x3001, 'exceeds REU reservation'),
            ('__phase_scenery_size', 0x4201, 'exceeds execution window'),
            ('__reu_digital_music', 0xf0000, 'digital music address'),
            ('__reu_digital_victory', 0x200000, 'digital victory alias'),
            ('__reu_digital_ring', 0x3e0000, 'digital ring address'),
            ('__reu_digital_ring_size', 0x10000, 'digital ring size'),
        ):
            with self.subTest(symbol=symbol, value=value):
                symbols = layout()
                symbols[symbol] = value
                with self.assertRaisesRegex(AssertionError, message):
                    packer.checked_layout(symbols)

        symbols = layout()
        symbols['__reu_scenery_gate'] = OFFSETS['scenery-carrier']
        symbols['__reu_scenery_gate_end'] = OFFSETS['scenery-carrier'] + RESERVATIONS['scenery-gate']
        with self.assertRaisesRegex(AssertionError, 'overlaps scenery-carrier'):
            packer.checked_layout(symbols)

    def test_packing_short_reservations_preserves_neighbors_and_rejects_overflow(self):
        symbols = layout()
        for name in ('boot', 'audio', 'sim-helpers', 'scenery-helpers'):
            start, size = OFFSETS[name], RESERVATIONS[name]
            reu = bytearray([0xa5]) * 0x80000
            code = bytes([0x5a]) * (size - 1)
            packer.write_code_image(reu, symbols, name, code)
            self.assertEqual(reu[start:start + size], code + b'\0')
            self.assertEqual((reu[start - 1], reu[start + size]), (0xa5, 0xa5))
            before = reu[:]
            with self.assertRaisesRegex(AssertionError, 'exceeds REU reservation'):
                packer.write_code_image(reu, symbols, name, bytes(size + 1))
            self.assertEqual(reu, before)


if __name__ == '__main__':
    unittest.main()
