#!/usr/bin/env python3
"""Execute the linked overlay loader across compact REU bank boundaries.

Checks the production 6502 instructions, exact transfer addresses/data, bounded
DMA sizes and calling convention. CPU cycle totals are interpreter counts,
not C64 Ultimate bus timing or frame-rate measurements.
"""
import argparse
from pathlib import Path

from native_cpu import Cpu, I, U, SENTINEL, RESIDENT_SECTIONS, load_elf
from reu_layout import CODE_IMAGES, image_size_symbol


ROOT = Path(__file__).resolve().parents[1]


class LoaderCpu(Cpu):
    def __init__(self, memory, source):
        super().__init__(memory, [(0, 0x600), (0xc400, 0xc800), (0xdf00, 0xdf0b)])
        self.source = source
        self.transfers = []

    def write(self, address, value):
        super().write(address, value)
        if address != 0xdf01:
            return
        assert value == 0x91, 'loader must fetch immediately without autoload'
        cpu = int.from_bytes(self.mem[0xdf02:0xdf04], 'little')
        reu = int.from_bytes(self.mem[0xdf04:0xdf07], 'little')
        count = int.from_bytes(self.mem[0xdf07:0xdf09], 'little')
        assert 0 < count <= 1024, f'unbounded DMA size: {count}'
        assert 0x3000 <= cpu < cpu + count <= 0x7200, 'DMA escaped overlay RAM'
        assert reu + count <= len(self.source), 'DMA escaped compact code region'
        assert (reu & 0xffff) + count <= 0x10000, 'individual DMA crossed a bank'
        assert self.mem[0xdf09:0xdf0b] == bytes(2), 'unexpected DMA interrupt/control'
        self.transfers.append((reu, cpu, count))
        self.mem[cpu:cpu + count] = self.source[reu:reu + count]
        # The hardware advances its address registers after each transfer.
        self.mem[0xdf02:0xdf04] = (cpu + count).to_bytes(2, 'little')
        self.mem[0xdf04:0xdf07] = (reu + count).to_bytes(3, 'little')
        self.mem[0xdf07:0xdf09] = bytes(2)


def exercise(elf, symbols):
    memory = bytearray([0xa5]) * 65536
    for section in elf.sections:
        if section.name in RESIDENT_SECTIONS:
            memory[section.address:section.address + section.size] = elf.contents(section)
    source = bytes((at * 37 + (at >> 8) * 13 + (at >> 16) * 71) & 255
                   for at in range(0x80000))
    table = symbols['__code_reu_pages'].address
    entry = symbols['havoc_c64::load_phase'].address
    assert symbols['__code_reu_pages_end'].address - table == 34
    cases = []
    crossed_images = []
    for name, symbol, logical_id in CODE_IMAGES:
        start = symbols[f'__reu_{symbol}'].address
        length = symbols[image_size_symbol(name)].address
        assert memory[table + logical_id] << 11 == start, f'{name}: table scale'
        if start >> 16 != (start + length - 1) >> 16:
            crossed_images.append(name)
        sizes = {0, 1, 255, 256, 1023, 1024, 1025, 2047, 2048, length - 1, length}
        for size in sorted(sizes):
            if size <= length:
                cases.append((name, logical_id, start, size))
    assert len(crossed_images) >= 4, 'compact images must exercise real bank crossings'
    # Relocate one test table entry to both sides of every bank transition,
    # including the last encodable unit at the music boundary.
    for unit in (0, 1, 31, 32, 63, 95, 127, 159, 191, 223, 247, 255):
        for size in (0, 1, 1023, 1024, 1025, 2048, 0x4200):
            if (unit << 11) + size <= len(source):
                cases.append((f'unit-{unit}', 1, unit << 11, size))
    total_cycles = 0
    for name, logical_id, start, size in cases:
        for interrupt_mask in (0, I):
            state = bytearray(memory)
            state[table + logical_id] = start >> 11
            state[2:4] = (0xc800).to_bytes(2, 'little')
            state[18:34] = bytes(range(0xc0, 0xd0))
            preserved = bytes(state[18:34])
            cpu = LoaderCpu(state, source)
            # LLVM-MOS byte argument A, followed by size in X / __rc2.
            cpu.a, cpu.x, cpu.y = logical_id, size & 255, 0x79
            state[4] = size >> 8
            cpu.p = U | interrupt_mask
            cpu.push(SENTINEL >> 8)
            cpu.push((SENTINEL - 1) & 255)
            cpu.pc = entry
            for _ in range(20000):
                if cpu.pc == SENTINEL:
                    break
                cpu.step()
            else:
                raise AssertionError(f'{name}: loader did not return')
            expected = [(start + offset, 0x3000 + offset, min(1024, size - offset))
                        for offset in range(0, size, 1024)]
            assert cpu.transfers == expected, f'{name}, size {size}: {cpu.transfers}'
            assert state[0x3000:0x3000 + size] == source[start:start + size], name
            assert state[0x3000 + size:0x7200] == memory[0x3000 + size:0x7200], name
            assert state[0x2fff] == memory[0x2fff] and state[0x7200] == memory[0x7200]
            assert cpu.s == 0xfd and state[2:4] == (0xc800).to_bytes(2, 'little')
            assert state[18:34] == preserved, f'{name}: callee-saved registers changed'
            assert cpu.p & I == interrupt_mask, f'{name}: interrupt mask changed'
            total_cycles += cpu.cycles
    return len(cases) * 2, crossed_images, total_cycles


class PairingCpu(Cpu):
    """Permit only the directory DMA while executing the native pairing gate."""

    def __init__(self, memory, package):
        super().__init__(memory, [(0, 0x600), (0xc400, 0xc800), (0xdf00, 0xdf0b)])
        self.package = package
        self.transfers = []

    def write(self, address, value):
        super().write(address, value)
        if address == 0xdf01:
            destination = int.from_bytes(self.mem[0xdf02:0xdf04], 'little')
            source = int.from_bytes(self.mem[0xdf04:0xdf07], 'little')
            count = int.from_bytes(self.mem[0xdf07:0xdf09], 'little')
            transfer = (source, destination, count)
            assert value == 0x91 and transfer == (0x100, 0x7600, 128), (
                'pairing gate fetched code before rejecting a mixed package', transfer)
            assert not self.transfers, 'pairing gate fetched more than one directory'
            self.transfers.append(transfer)
            self.mem[destination:destination + count] = self.package[source:source + count]
            self.mem[0xdf02:0xdf04] = (destination + count).to_bytes(2, 'little')
            self.mem[0xdf04:0xdf07] = (source + count).to_bytes(3, 'little')
            self.mem[0xdf07:0xdf09] = bytes(2)


def check_pairing(elf_path, package, matched):
    elf, symbols = load_elf(elf_path)
    image = elf_path.with_suffix('.prg').read_bytes()
    memory = bytearray(65536)
    load = int.from_bytes(image[:2], 'little')
    assert load == 0x801 and load + len(image) - 2 == 0xa000, 'unexpected PRG span'
    memory[load:load + len(image) - 2] = image[2:]
    for section in elf.sections:
        if section.name == '.data':
            memory[section.address:section.address + section.size] = elf.contents(section)
    memory[2:4] = (0xc800).to_bytes(2, 'little')
    cpu = PairingCpu(memory, package)
    cpu.pc = symbols['havoc_c64::check_code_package'].address
    cpu.push(SENTINEL >> 8)
    cpu.push((SENTINEL - 1) & 255)
    load_phase = symbols['havoc_c64::load_phase'].address
    fail = symbols['havoc_c64::fail'].address
    for _ in range(1000000):
        if cpu.pc in (load_phase, fail):
            break
        cpu.step()
    else:
        raise AssertionError('native pairing gate did not finish')
    assert cpu.transfers == [(0x100, 0x7600, 128)]
    if matched:
        assert cpu.pc == load_phase, 'matching PRG/REU rejected before overlay validation'
    else:
        assert cpu.pc == fail, 'mixed PRG/REU reached the overlay loader'
        # LLVM-MOS passes this slice's length in A/X and pointer in __rc2/3.
        pointer = int.from_bytes(memory[4:6], 'little')
        expected = symbols['havoc_c64::ERROR_RESIDENT_PAIR']
        assert (pointer, cpu.a | cpu.x << 8) == (expected.address, expected.size), (
            'mixed PRG/REU selected the wrong diagnostic')
        assert memory[pointer:pointer + expected.size] == b'PRG AND REU DO NOT MATCH'
    return cpu.cycles


def exercise_pairing(current_elf, previous_package):
    previous_elf = previous_package / 'havoc.elf'
    current_reu = current_elf.with_suffix('.reu').read_bytes()
    previous_reu = (previous_package / 'havoc.reu').read_bytes()
    versions = tuple(int.from_bytes(image[4:8], 'little') for image in (previous_reu, current_reu))
    assert versions == (0x534d1000, 0x534d1100), 'pairing fixtures must be v16 and v17'
    for elf_path, own, other in ((previous_elf, previous_reu, current_reu),
                                 (current_elf, current_reu, previous_reu)):
        check_pairing(elf_path, own, matched=True)
        check_pairing(elf_path, other, matched=False)
    print('PRG/REU pairing: both matched v16/v17 controls pass; both mixed pairs reject '
          'with PRG AND REU DO NOT MATCH after directory DMA and before any overlay fetch')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('elf', nargs='?', type=Path, default=ROOT / 'dist/havoc.elf')
    parser.add_argument('--previous-package', type=Path,
                        help='also verify v16/v17 mixed pairs using this preserved v16 package')
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    cases, crossed, cycles = exercise(elf, symbols)
    print(f'Compact REU loader: {cases} native cases; exact bytes and <=1024-byte transfers; '
          f'{cycles:,} interpreter CPU cycles')
    print('Real images crossing 64 KiB banks: ' + ', '.join(crossed))
    if args.previous_package:
        exercise_pairing(args.elf, args.previous_package)


if __name__ == '__main__':
    main()
