"""Pair the PRG, composed campaign phases and shared SIM helpers with the REU."""
import os
import pathlib
import subprocess
import sys
from digital_format import validate_digital_payload
from reu_layout import CODE_IMAGES, PACKAGE_ID, PAGE_SHIFT, checked_layout

def checksum(data):
    a = b = 0
    for value in data:
        a = (a + value) % 255
        b = (b + a) % 255
    return a | b << 8


def write_code_image(reu, symbols, name, code):
    """Clear only this image's reservation, including its unused storage tail."""
    symbol = next(symbol for entry, symbol, _ in CODE_IMAGES if entry == name)
    offset, end = symbols[f'__reu_{symbol}'], symbols[f'__reu_{symbol}_end']
    assert 0 <= offset < end <= len(reu), f'{name}: invalid REU reservation'
    assert 0 < len(code) <= 0x4200, f'{name}: code exceeds execution window'
    assert len(code) <= end - offset, f'{name}: code exceeds REU reservation'
    reu[offset:end] = bytes(end - offset)
    reu[offset:offset + len(code)] = code


def main():
    elf, prg, reu_path = map(pathlib.Path, sys.argv[1:4])
    sdk = pathlib.Path(os.environ.get('LLVM_MOS', str(pathlib.Path.home()/'.local/llvm-mos')))/'bin'
    symbols = {}
    for line in subprocess.check_output([str(sdk/'llvm-nm'), '-n', str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            try: symbols[fields[2]] = int(fields[0], 16)
            except ValueError: pass
    image = prg.read_bytes()
    assert image[:2] == b'\x01\x08'
    reu = bytearray(reu_path.read_bytes())
    offsets, allocations = checked_layout(symbols)
    assert len(reu) == symbols['__reu_size'], 'REU image must contain exactly 16 MiB'
    assert int.from_bytes(reu[4:8], 'little') == PACKAGE_ID, 'expected v17 package'
    digital = validate_digital_payload(reu)
    for name, metadata in digital.items():
        print(f'digital {name}: {metadata["asset_bytes"]:,} bytes, {metadata["sample_rate"]} Hz, {metadata["channels"]} channels; two 64 KiB mutable PCM slots')
    table_at = 2 + symbols['__code_reu_pages'] - 0x801
    expected_pages = bytearray(34)
    for name, _, logical_id in CODE_IMAGES:
        expected_pages[logical_id] = offsets[name] >> PAGE_SHIFT
    assert image[table_at:table_at + 34] == expected_pages, 'REU code address table disagrees with linker'
    directory = bytearray(128)
    directory[:4] = b'OVL7'
    packaged = set()
    def put(at, value): directory[at:at+2] = value.to_bytes(2, 'little')
    def extract(name):
        output = elf.parent / f'phase-{name}.bin'
        subprocess.run([str(sdk/'llvm-objcopy'), '--dump-section', f'.phase.{name}={output}', str(elf)], check=True)
        return output.read_bytes()
    def package(name, code, size_at, hash_at):
        offset = offsets[name]
        assert name not in packaged, f'{name}: duplicate code image'
        packaged.add(name)
        write_code_image(reu, symbols, name, code)
        put(size_at, len(code))
        put(hash_at, checksum(code))
        destination = 0x8e00 if name in ('sim-helpers', 'scenery-helpers') else 0x3000
        print(f'{name:12} REU ${offset:06X} -> RAM ${destination:04X}: {len(code):5} bytes')
    for index, name in enumerate(('boot', 'sim', 'geometry', 'pixels', 'audio')):
        if name == 'sim':
            continue
        code = extract(name)
        if name == 'geometry':
            assert len(code) == symbols['__phase_geometry_prefix_size'] <= 0x3f20
            auxiliary = extract('render_aux')
            assert len(auxiliary) == symbols['__phase_render_aux_size'] <= 0x2e0
            assert symbols['__phase_render_aux_start'] == 0x6f20
            assert symbols['__phase_render_aux_load'] == offsets['geometry'] + 0x3f20
            code = code.ljust(0x3f20, b'\0') + auxiliary
            (elf.parent / 'phase-geometry-composed.bin').write_bytes(code)
        if name == 'pixels':
            assert len(code) == symbols['__phase_pixels_prefix_size'] <= 0x3f20
            auxiliary = extract('pixels_aux')
            assert len(auxiliary) == symbols['__phase_pixels_aux_size'] <= 0x2e0
            assert symbols['__phase_pixels_aux_start'] == 0x6f20
            assert symbols['__phase_pixels_aux_load'] == offsets['pixels'] + 0x3f20
            code = code.ljust(0x3f20, b'\0') + auxiliary
            (elf.parent / 'phase-pixels-composed.bin').write_bytes(code)
        assert len(code) == symbols[f'__phase_{name}_size'] and 0 < len(code) <= 0x4200
        if name in ('scenery', 'geometry'):
            assert 0 < symbols[f'__phase_{name}_base_size'] < len(code)
        package(name, code, 34 if name == 'scenery' else 4+index*2,
                36 if name == 'scenery' else 14+index*2)
    prefix = extract('sim')
    assert len(prefix) == symbols['__phase_sim_base_size']
    for level, name in enumerate(('lowlands', 'canyon', 'fortress', 'carrier', 'warden', 'citadel', 'flagship')):
        tail = extract(f'enemy.{name}')
        assert 0 < len(tail) <= symbols['__phase_enemy_size']
        code = (prefix + tail).ljust(symbols['__phase_sim_size'], b'\0')
        (elf.parent / f'phase-sim-{name}.bin').write_bytes(code)
        package(f'sim-{name}', code,
                6 if level == 0 else 38+(level-1)*4 if level < 4 else 64+(level-4)*4,
                16 if level == 0 else 40+(level-1)*4 if level < 4 else 66+(level-4)*4)
    terminal = (prefix + extract('terminal')).ljust(symbols['__phase_sim_size'], b'\0')
    assert len(terminal) == symbols['__phase_sim_size']
    package('sim-terminal', terminal, 54, 56)
    geometry_prefix = extract('geometry')[:symbols['__phase_geometry_base_size']]
    victory = geometry_prefix + extract('victory')
    assert len(victory) == symbols['__phase_victory_size']
    package('victory', victory, 50, 52)
    scenery_prefix = extract('scenery')
    assert len(scenery_prefix) == symbols['__phase_scenery_base_size']
    for name, at in (('wildlife', 34), ('transport', 76), ('meteors', 80),
                     ('ascent', 88), ('asteroid', 92), ('moon', 96),
                     ('graveyard', 100), ('shipyard', 104), ('carrier', 108), ('gate', 112)):
        code = (scenery_prefix + extract(f'scenery.{name}')).ljust(symbols['__phase_scenery_size'], b'\0')
        assert len(code) == symbols['__phase_scenery_size']
        package(f'scenery-{name}', code, at, at+2)
    auxiliary = extract('sim_aux')
    assert len(auxiliary) == symbols['__phase_sim_aux_prefix_size'] <= 0x500
    extra = extract('sim_extra')
    assert len(extra) == symbols['__phase_sim_extra_size'] <= 0x1800
    auxiliary = auxiliary.ljust(0x500, b'\0') + extra
    assert len(auxiliary) == symbols['__phase_sim_aux_size']
    package('sim-helpers', auxiliary, 84, 86)
    package('shadow', extract('shadow'), 116, 118)
    auxiliary = extract('scenery_aux')
    assert len(auxiliary) == symbols['__phase_scenery_aux_prefix_size'] <= 0x500
    extra = extract('scenery_extra')
    assert len(extra) == symbols['__phase_scenery_extra_size'] <= 0x800
    auxiliary = auxiliary.ljust(0x500, b'\0') + extra
    assert len(auxiliary) == symbols['__phase_scenery_aux_size']
    package('scenery-helpers', auxiliary, 58, 60)
    assert packaged == set(offsets), 'missing code image'
    def span(start, end): return image[2+start-0x801:2+end-0x801]
    put(24, checksum(span(0x801, symbols['__resident_end'])))
    put(26, checksum(span(0x7800, 0x8000)))
    put(28, checksum(span(0x9900, 0xa000)))
    put(30, checksum(span(symbols['__startup_checks_start'], symbols['__startup_checks_end'])))
    directory[32] = 4
    put(120, 768)
    put(122, checksum(span(0x7200, 0x7500)))
    put(126, checksum(directory[:126]))
    reu[0x100:0x180] = directory
    reu_path.write_bytes(reu)
    reserved = sum(end - start for start, end, _ in allocations)
    print(f'REU reservations: {reserved:,} bytes; {len(reu) - reserved:,} bytes unreserved')

if __name__ == '__main__': main()
