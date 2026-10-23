"""Pair the PRG, composed campaign phases and shared SIM helpers with the REU."""
import os
import pathlib
import subprocess
import sys
from digital_format import validate_digital_payload

# Directory order and logical IDs remain stable; the linker owns REU addresses.
CODE_IMAGES = (
    ('boot', 'boot', 1), ('sim-lowlands', 'sim_lowlands', 2),
    ('geometry', 'geometry', 3), ('pixels', 'pixels', 4), ('audio', 'audio', 5),
    ('scenery-wildlife', 'scenery_wildlife', 7),
    ('sim-canyon', 'sim_canyon', 8), ('sim-fortress', 'sim_fortress', 9),
    ('sim-carrier', 'sim_carrier', 10), ('victory', 'victory', 12),
    ('sim-terminal', 'sim_terminal', 13), ('sim-warden', 'sim_warden', 18),
    ('sim-citadel', 'sim_citadel', 19), ('sim-flagship', 'sim_flagship', 20),
    ('scenery-transport', 'scenery_transport', 21),
    ('scenery-meteors', 'scenery_meteors', 22), ('sim-helpers', 'sim_helpers', 23),
    ('scenery-ascent', 'scenery_ascent', 24), ('scenery-asteroid', 'scenery_asteroid', 25),
    ('scenery-moon', 'scenery_moon', 26), ('scenery-graveyard', 'scenery_graveyard', 27),
    ('scenery-shipyard', 'scenery_shipyard', 28),
    ('scenery-carrier', 'scenery_carrier', 29), ('scenery-gate', 'scenery_gate', 30),
)

# Per-song music kernels in cue order; Lost Wing (cue 6) plays without one.
KERNEL_CUES = (
    'title', 'lowlands', 'canyon', 'fortress', 'carrier', 'victory', None,
    'crystal_current', 'ashes_and_steel', 'tempest_drive', 'gorge_warden',
    'citadel_heart', 'carrier_overdrive', 'asteroid_belt', 'shattered_moon',
    'ship_graveyard', 'orbital_shipyard', 'space_carrier', 'jump_gate',
    'ascent', 'gate_guardian',
)
KERNEL_DIRECTORY_SIZE = 256
KERNEL_HEADER_SIZE = 16


def kernel_reservation(symbols):
    """The SMK1 directory plus one slot-sized image per cue."""
    base = symbols['__reu_music_kernels']
    slot = symbols['__music_kernel_size']
    assert 0 < slot <= 0x400, 'music kernel slot must be a bounded DMA'
    return base, base + KERNEL_DIRECTORY_SIZE + len(KERNEL_CUES) * slot, slot


def checked_layout(symbols):
    """Check full reservations, including padding, before writing any payload."""
    size = symbols['__reu_size']
    slot_size = symbols['__reu_code_slot_size']
    assert size == 0x1000000 and slot_size == 0x4200, 'unsupported REU layout'
    assert symbols['__reu_digital_music'] == 0x100000, 'incorrect digital music address'
    assert symbols['__reu_digital_victory'] == 0x500000, 'incorrect digital victory address'
    assert symbols['__reu_digital_ring'] == 0xfe0000, 'incorrect digital ring address'
    assert symbols['__reu_digital_ring_size'] == 0x20000, 'incorrect digital ring size'
    offsets = {name: symbols[f'__reu_{symbol}'] for name, symbol, _ in CODE_IMAGES}
    kernels_start, kernels_end, _ = kernel_reservation(symbols)
    allocations = [(0, 0x180, 'directories'), (0x1000, 0x1c00, 'common assets'),
                   (0x2000, 0xb000, 'encounters'),
                   (symbols['__reu_models'], symbols['__reu_models'] + 0x9000, 'models'),
                   (kernels_start, kernels_end, 'music kernels'),
                   (symbols['__reu_music'], symbols['__reu_music'] + 0x40000, 'music'),
                   (symbols['__reu_digital_music'], symbols['__reu_digital_victory'], 'digital title music'),
                   (symbols['__reu_digital_victory'], symbols['__reu_digital_ring'], 'digital victory music'),
                   (symbols['__reu_digital_ring'], size, 'mutable PCM ring')]
    for name, offset in offsets.items():
        assert offset + slot_size <= 0x100000, f'{name}: code allocation exceeds REU code region'
        assert offset & 0xfff == 0, f'{name}: code base must be 4 KiB aligned'
        assert (offset & 0xffff) + slot_size <= 0x10000, f'{name}: code slot crosses a 64 KiB bank'
        assert name == 'pixels' or offset >> 16 != 4, 'PIXELS must retain its exclusive REU bank'
        allocations.append((offset, offset + slot_size, name))
    previous_end = 0
    previous_name = ''
    for start, end, name in sorted(allocations):
        assert 0 <= start < end <= size, f'{name}: allocation exceeds REU'
        assert start >= previous_end, f'{name}: overlaps {previous_name}'
        previous_end, previous_name = end, name
    return offsets, allocations


def checksum(data):
    a = b = 0
    for value in data:
        a = (a + value) % 255
        b = (b + a) % 255
    return a | b << 8


def pack_music_kernels(elf, sdk, symbols, reu):
    """Write the SMK1 directory and every linked per-cue kernel image.

    Without the music-kernels feature no kernel section exists and the
    directory records twenty-one empty entries; the shipping AUDIO overlay then
    never reads it. With the feature, every image must fit the resident slot and
    carries the checksum the loader re-verifies at each cue change.
    """
    base, end, slot = kernel_reservation(symbols)
    directory = bytearray(KERNEL_DIRECTORY_SIZE)
    directory[:4] = b'SMK1'
    directory[4] = len(KERNEL_CUES)
    directory[6:8] = slot.to_bytes(2, 'little')
    # The header checksum treats its own two bytes as zero, like page checksums.
    directory[14:16] = checksum(directory[:14] + b'\0\0').to_bytes(2, 'little')
    reu[base:end] = bytes(end - base)
    linked = 0
    for cue, name in enumerate(KERNEL_CUES):
        if name is None:
            continue
        output = elf.parent / f'kernel-{name}.bin'
        output.unlink(missing_ok=True)
        result = subprocess.run([str(sdk/'llvm-objcopy'), '--dump-section', f'.music.kernel.{name}={output}', str(elf)],
                                capture_output=True)
        if result.returncode != 0 or not output.exists():
            continue
        code = output.read_bytes()
        if not code:
            continue
        assert len(code) <= slot, f'{name}: kernel image exceeds the {slot}-byte slot'
        at = base + KERNEL_DIRECTORY_SIZE + cue * slot
        reu[at:at + len(code)] = code
        entry = KERNEL_HEADER_SIZE + cue * 4
        directory[entry:entry + 2] = len(code).to_bytes(2, 'little')
        directory[entry + 2:entry + 4] = checksum(code).to_bytes(2, 'little')
        linked += 1
        print(f'kernel {name:18} REU ${at:06X} -> slot ${symbols["__music_kernel_start"]:04X}: {len(code):4} of {slot} bytes')
    directory[-2:] = checksum(directory[:-2]).to_bytes(2, 'little')
    reu[base:base + KERNEL_DIRECTORY_SIZE] = directory
    print(f'music kernels: {linked} images, {slot}-byte slot, directory at ${base:06X}')

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
    assert int.from_bytes(reu[4:8], 'little') == 0x534d0a00, 'expected v10 package'
    digital = validate_digital_payload(reu)
    for name, metadata in digital.items():
        print(f'digital {name}: {metadata["asset_bytes"]:,} bytes, {metadata["sample_rate"]} Hz stereo; shared 128 KiB mutable PCM ring')
    table_at = 2 + symbols['__code_reu_pages'] - 0x801
    expected_pages = bytearray(31)
    for name, _, logical_id in CODE_IMAGES:
        expected_pages[logical_id] = offsets[name] >> 12
    assert image[table_at:table_at + 31] == expected_pages, 'REU code address table disagrees with linker'
    assert reu[symbols['__reu_music']:symbols['__reu_music'] + 4] == b'SMU3', 'music address disagrees with baker'
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
        assert 0 < len(code) <= 0x4200
        assert name not in packaged, f'{name}: duplicate code image'
        packaged.add(name)
        reu[offset:offset+0x4200] = bytes(0x4200)
        reu[offset:offset+len(code)] = code
        put(size_at, len(code))
        put(hash_at, checksum(code))
        destination = symbols['__phase_sim_aux_start'] if name == 'sim-helpers' else 0x3000
        print(f'{name:12} REU ${offset:06X} -> RAM ${destination:04X}: {len(code):5} bytes')
    for index, name in enumerate(('boot', 'sim', 'geometry', 'pixels', 'audio')):
        if name == 'sim':
            continue
        code = extract(name)
        if name == 'pixels':
            assert len(code) == symbols['__phase_pixels_prefix_size'] <= 0x3f20
            auxiliary = extract('pixels_aux')
            assert len(auxiliary) == symbols['__phase_pixels_aux_size'] <= 0x2e0
            assert symbols['__phase_pixels_aux_start'] == 0x6f20
            assert symbols['__phase_pixels_aux_load'] == 0x43f20
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
    assert len(auxiliary) == symbols['__phase_sim_aux_size'] and len(auxiliary) <= 0x500
    package('sim-helpers', auxiliary, 84, 86)
    assert packaged == set(offsets), 'missing code image'
    def span(start, end): return image[2+start-0x801:2+end-0x801]
    put(24, checksum(span(0x801, symbols['__resident_end'])))
    put(26, checksum(span(0x7800, 0x8000)))
    put(28, checksum(span(0x9900, 0xa000)))
    put(30, checksum(span(symbols['__startup_checks_start'], symbols['__startup_checks_end'])))
    directory[32] = 4 if symbols['__music_page_end'] > symbols['__music_page_start'] else 2
    put(126, checksum(directory[:126]))
    reu[0x100:0x180] = directory
    pack_music_kernels(elf, sdk, symbols, reu)
    reu_path.write_bytes(reu)
    reserved = sum(end - start for start, end, _ in allocations)
    print(f'REU reservations: {reserved:,} bytes; {len(reu) - reserved:,} bytes unreserved')

if __name__ == '__main__': main()
