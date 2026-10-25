#!/usr/bin/env python3
"""Execute the linked PRG startup with poisoned RAM before any REU access.

This checks the production 6502 instructions, memory ownership and publication
order. VIC raster timing and physical display quality remain emulator/hardware
checks rather than claims made by the instruction interpreter.
"""
import argparse
from collections import Counter
from pathlib import Path

from native_cpu import Cpu, D, I, U, V, load_elf


ROOT = Path(__file__).resolve().parents[1]
VISIBLE = ((0xA000, 0xBF40), (0x8000, 0x83E8), (0xD800, 0xDBE8))
HARDWARE = (0, 1, 0xD011, 0xD015, 0xD016, 0xD018, 0xD019, 0xD01A,
            0xD020, 0xD021, 0xD031, 0xDC0D, 0xDD00, 0xDD02, 0xDD0D,
            0xFFFA, 0xFFFB, 0xFFFE, 0xFFFF)


class StartupCpu(Cpu):
    def __init__(self, memory, payload):
        super().__init__(memory, [(0, 0x600), *VISIBLE,
                                 *((address, address + 1) for address in HARDWARE)])
        self.payload = payload
        self.visible = False
        self.visible_stores = Counter()

    def read(self, address):
        assert not 0xDF00 <= address <= 0xDF1F, 'startup read REU before entering main'
        return super().read(address)

    def check_picture(self):
        assert self.mem[0xA000:0xBF40] == self.payload[:8000], 'bitmap copy differs'
        assert self.mem[0x8000:0x83E8] == self.payload[8000:9000], 'screen copy differs'
        assert self.mem[0xD800:0xDBE8] == self.payload[9000:10000], 'color RAM copy differs'

    def write(self, address, value):
        assert not 0xDF00 <= address <= 0xDF1F, 'startup wrote REU before entering main'
        assert self.p & I, 'startup enabled IRQs before runtime initialization'
        if not self.visible:
            assert not 0x22 <= address < 0x100 and not 0x200 <= address < 0x600, (
                'runtime state initialized before splash publication', hex(address))
        for start, end in VISIBLE:
            if start <= address < end:
                assert not self.visible, 'startup overwrote the published splash'
                assert self.mem[1] == 0x35, 'splash copy requires RAM with I/O visible'
                assert not self.mem[0xD011] & 0x10, 'splash copied with display enabled'
                self.visible_stores[address] += 1
        if address == 0xD011 and value & 0x10:
            self.check_picture()
            assert value == 0x3B, 'splash must enable standard multicolor bitmap display'
            assert not self.visible, 'startup enabled splash more than once'
            assert self.mem[1] == 0x35 and self.mem[0xDD02] & 3 == 3 and self.mem[0xDD00] & 3 == 1, (
                'splash publication requires the correct CPU and VIC bank mapping')
            assert tuple(self.mem[address] for address in (0xD015, 0xD016, 0xD018, 0xD020, 0xD021)) == (
                0, 0x18, 8, 0, self.payload[-1]), 'splash video setup must precede display enable'
            self.visible = True
        super().write(address, value)


def run_case(elf, symbols, image, seed, synthetic=False):
    memory = bytearray((index * 37 + (index >> 8) * 13 + seed) & 255 for index in range(65536))
    load = int.from_bytes(image[:2], 'little')
    assert load == 0x801 and load + len(image) - 2 == 0xA000, 'unexpected PRG span'
    memory[load:load + len(image) - 2] = image[2:]
    data = next(section for section in elf.sections if section.name == '.splash')
    assert data.address == 0x3000 and data.size == 10001
    if synthetic:
        payload = bytearray((index * 53 + (index >> 8)) & 255 for index in range(10001))
        payload[9000:] = bytes(index & 15 for index in range(1001))
        memory[data.address:data.address + data.size] = payload
    payload = memory[data.address:data.address + data.size]
    original = memory[:]
    cpu = StartupCpu(memory, payload)
    cpu.pc = symbols['_start'].address
    cpu.a, cpu.x, cpu.y, cpu.p, cpu.s = 0x79, 0xB3, 0x4D, U | D | V, 0x67
    ready = symbols['__havoc_splash_ready'].address
    runtime_entries = {symbols[name].address for name in ('__zero_bss', '__havoc_copy_data')}
    saw_ready = False
    steps = 0
    while cpu.pc != symbols['main'].address:
        if cpu.pc == ready:
            assert cpu.visible, 'ready marker precedes display enable'
            cpu.check_picture()
            saw_ready = True
        if cpu.pc in runtime_entries:
            assert saw_ready, 'BSS/data initialization precedes the splash'
        cpu.step()
        steps += 1
        assert steps < 200000, 'startup failed to reach main'
    assert saw_ready and cpu.visible
    assert cpu.p & I and not cpu.p & D, 'startup CPU flags are unsafe'
    assert cpu.mem[0] == 0x2F and cpu.mem[1] == 0x35, 'runtime ROM/I/O mapping changed'
    assert cpu.mem[0xDD00] == (original[0xDD00] & 0xFC) | 1, 'CIA2 port bits were lost'
    assert cpu.mem[0xDD02] == original[0xDD02] | 3, 'CIA2 direction bits were lost'
    for address, value in ((0xD011, 0x3B), (0xD015, 0), (0xD016, 0x18),
                           (0xD018, 8), (0xD020, 0), (0xD021, payload[-1]), (0xD031, 0x8F)):
        assert cpu.mem[address] == value, f'incorrect splash register ${address:04X}'
    cpu.check_picture()
    expected_stores = {address for start, end in VISIBLE for address in range(start, end)}
    assert set(cpu.visible_stores) == expected_stores and set(cpu.visible_stores.values()) == {1}, (
        'splash bytes must each be copied exactly once')
    for address in range(0x600, 65536):
        if address in HARDWARE or any(start <= address < end for start, end in VISIBLE):
            continue
        assert memory[address] == original[address], f'startup changed unrelated RAM ${address:04X}'
    return steps, cpu.cycles


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('elf', nargs='?', type=Path, default=ROOT / 'dist/havoc.elf')
    parser.add_argument('prg', nargs='?', type=Path)
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    image = (args.prg or args.elf.with_suffix('.prg')).read_bytes()
    for seed, synthetic in ((0, False), (85, False), (255, False), (17, True)):
        steps, cycles = run_case(elf, symbols, image, seed, synthetic)
        print(f'Splash startup seed {seed}, synthetic={synthetic}: {steps:,} instructions, '
              f'{cycles:,} CPU cycles; exact picture before BSS/data and no REU access')


if __name__ == '__main__':
    main()
