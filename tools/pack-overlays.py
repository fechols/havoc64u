"""Pair the PRG, composed campaign phases and shared SIM helpers with the REU."""
import os
import pathlib
import subprocess
import sys

def checksum(data):
    a = b = 0
    for value in data:
        a = (a + value) % 255
        b = (b + a) % 255
    return a | b << 8

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
    assert len(reu) == 16*1024*1024
    directory = bytearray(128)
    directory[:4] = b'OVL7'
    allocations = [(0, 0x180, 'directories'), (0x1000, 0x1c00, 'common assets'),
                   (0x2000, 0xb000, 'encounters'), (0xb0000, 0xb9000, 'models'),
                   (0x200000, 0x240000, 'music')]
    def put(at, value): directory[at:at+2] = value.to_bytes(2, 'little')
    def extract(name):
        output = elf.parent / f'phase-{name}.bin'
        subprocess.run([str(sdk/'llvm-objcopy'), '--dump-section', f'.phase.{name}={output}', str(elf)], check=True)
        return output.read_bytes()
    def package(name, code, offset, size_at, hash_at):
        assert 0 < len(code) <= 0x4200
        end = offset + 0x4200
        assert 0 <= offset < end <= len(reu), f'{name}: allocation exceeds REU'
        for start, stop, owner in allocations:
            assert end <= start or offset >= stop, f'{name}: overlaps {owner}'
        allocations.append((offset, end, name))
        reu[offset:offset+0x4200] = bytes(0x4200)
        reu[offset:offset+len(code)] = code
        put(size_at, len(code))
        put(hash_at, checksum(code))
        destination = 0x8c00 if name == 'sim-helpers' else 0x3000
        print(f'{name:12} REU ${offset:06X} -> RAM ${destination:04X}: {len(code):5} bytes')
    for index, name in enumerate(('boot', 'sim', 'geometry', 'pixels', 'audio')):
        if name == 'sim':
            continue
        code = extract(name)
        assert len(code) == symbols[f'__phase_{name}_size'] and 0 < len(code) <= 0x4200
        if name in ('scenery', 'geometry'):
            assert 0 < symbols[f'__phase_{name}_base_size'] < len(code)
        offset = 0x70000 if name == 'scenery' else (index+1)*0x10000
        package(name, code, offset, 34 if name == 'scenery' else 4+index*2,
                36 if name == 'scenery' else 14+index*2)
    prefix = extract('sim')
    assert len(prefix) == symbols['__phase_sim_base_size']
    for level, name in enumerate(('lowlands', 'canyon', 'fortress', 'carrier', 'warden', 'citadel', 'flagship')):
        tail = extract(f'enemy.{name}')
        assert 0 < len(tail) <= symbols['__phase_enemy_size']
        code = (prefix + tail).ljust(symbols['__phase_sim_size'], b'\0')
        (elf.parent / f'phase-sim-{name}.bin').write_bytes(code)
        package(f'sim-{name}', code, (2 if level == 0 else level+7 if level < 4 else level+14)*0x10000,
                6 if level == 0 else 38+(level-1)*4 if level < 4 else 64+(level-4)*4,
                16 if level == 0 else 40+(level-1)*4 if level < 4 else 66+(level-4)*4)
    terminal = (prefix + extract('terminal')).ljust(symbols['__phase_sim_size'], b'\0')
    assert len(terminal) == symbols['__phase_sim_size']
    package('sim-terminal', terminal, 0xD0000, 54, 56)
    geometry_prefix = extract('geometry')[:symbols['__phase_geometry_base_size']]
    victory = geometry_prefix + extract('victory')
    assert len(victory) == symbols['__phase_victory_size']
    package('victory', victory, 0xC0000, 50, 52)
    scenery_prefix = extract('scenery')
    assert len(scenery_prefix) == symbols['__phase_scenery_base_size']
    for name, page, at in (('wildlife', 7, 34), ('transport', 0x15, 76), ('meteors', 0x16, 80),
                           ('ascent', 0x18, 88), ('asteroid', 0x19, 92), ('moon', 0x1a, 96),
                           ('graveyard', 0x1b, 100), ('shipyard', 0x1c, 104),
                           ('carrier', 0x1d, 108), ('gate', 0x1e, 112)):
        code = (scenery_prefix + extract(f'scenery.{name}')).ljust(symbols['__phase_scenery_size'], b'\0')
        assert len(code) == symbols['__phase_scenery_size']
        package(f'scenery-{name}', code, page * 0x10000, at, at+2)
    auxiliary = extract('sim_aux')
    assert len(auxiliary) == symbols['__phase_sim_aux_size'] and len(auxiliary) <= 0x500
    package('sim-helpers', auxiliary, 0x170000, 84, 86)
    def span(start, end): return image[2+start-0x801:2+end-0x801]
    put(24, checksum(span(0x801, symbols['__resident_end'])))
    put(26, checksum(span(0x7800, 0x8000)))
    put(28, checksum(span(0x9900, 0xa000)))
    put(30, checksum(span(symbols['__startup_checks_start'], symbols['__startup_checks_end'])))
    directory[32] = 4 if symbols['__music_page_end'] > symbols['__music_page_start'] else 2
    put(126, checksum(directory[:126]))
    reu[0x100:0x180] = directory
    reu_path.write_bytes(reu)

if __name__ == '__main__': main()
