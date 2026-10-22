#!/usr/bin/env python3
"""Execute the assembled publisher with a cycle-driven VIC/IRQ test model.

The tiny instruction interpreter executes LLVM-MOS's production machine code;
it does not substitute a Python implementation of the publication predicate.
VIC reads and color stores each cost an extra complete 1-MHz bus cycle, while
ordinary instructions use their 6502 cycle counts at 64x. This deliberately
overcharges I/O. It checks publication deadlines, not Ultimate FPGA timing.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target/publish-test"
SDK = Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin"
ORIGIN = 0x3000
PROFILE = 0x7800
C, Z, I, D, B, U, V, N = (1 << n for n in range(8))


class Machine:
    def __init__(self, code, symbols, band, lines, position, flags=0x20, injection=None):
        self.mem = bytearray((i * 37 + (i >> 8) * 13) & 255 for i in range(65536))
        self.mem[ORIGIN:ORIGIN + len(code)] = code
        self.a, self.x, self.y, self.p, self.s = band, 0x57, 0x93, flags, 0xfd
        self.pc = symbols["publish_staged_band"]
        self.push(0xff)
        self.push(0xfe)
        self.before = self.mem[:]
        self.band, self.lines, self.symbols = band, lines, symbols
        self.line_cycles = (63 if lines == 312 else 65) * 64
        self.now = int(position * self.line_cycles)
        self.started = self.now
        self.io = self.steps = self.irq_count = self.wait_visits = self.final_checks = 0
        self.injection = injection
        self.injected = False
        self.irq_due = None
        self.irq_deferred = False
        self.writes = []
        self.raster_reads = []
        self.commit_start = self.commit_end = self.mask_start = None
        self.max_mask = 0
        self.wait_irqs_enabled = True

    @property
    def line(self):
        return (self.now // self.line_cycles) % self.lines

    def push(self, value):
        self.mem[0x100 + self.s] = value
        self.s = (self.s - 1) & 255

    def pop(self):
        self.s = (self.s + 1) & 255
        return self.mem[0x100 + self.s]

    def read(self, address):
        if address in (0xd011, 0xd012):
            # The access may wait a full bus cycle before observing the VIC.
            self.now += 64
            self.io += 1
            line = self.line
            self.raster_reads.append((address, line))
            return (0x38 | (0x80 if line >= 256 else 0)) if address == 0xd011 else line & 255
        return self.mem[address]

    def write(self, address, value):
        if 0xd800 <= address < 0xdc00:
            self.now += 64
            self.io += 1
        if 0xa000 <= address < 0xbf40 or 0x8000 <= address < 0x83e8 or 0xd800 <= address < 0xdbe8:
            assert self.p & I, "visible store with IRQs enabled"
            assert not 48 + 8 * self.band <= self.line < 63 + 8 * self.band, (
                "visible store during a possible VIC fetch", self.band, self.line, hex(address))
            self.writes.append((address, value, self.now))
        self.mem[address] = value

    def nz(self, value):
        value &= 255
        self.p = (self.p & ~(N | Z)) | (N if value & 128 else 0) | (Z if value == 0 else 0)
        return value

    def byte(self):
        value = self.mem[self.pc]
        self.pc += 1
        return value

    def word(self):
        return self.byte() | self.byte() << 8

    def compare(self, left, right):
        self.p = (self.p & ~C) | (C if left >= right else 0)
        self.nz(left - right)

    def irq(self, bus_cycles):
        assert not self.p & I
        self.irq_count += 1
        self.now += bus_cycles * 64

    def step(self):
        pc = self.pc
        if pc == self.symbols["publish_wait"]:
            self.wait_visits += 1
            self.wait_irqs_enabled &= not bool(self.p & I)
            if self.injection == "waiting" and not self.injected and self.wait_visits == 3:
                self.irq(180)
                self.injected = True
        if pc == self.symbols["publish_guard"]:
            self.final_checks += 1
            assert self.p & I
        if pc == self.symbols["publish_commit"]:
            self.commit_start = self.now
            assert self.line < 40 + 8 * self.band or self.line >= 64 + 8 * self.band
            if self.injection == "deferred_251":
                self.irq_due = 251 * self.line_cycles
                assert self.now < self.irq_due
        if pc == self.symbols["publish_done"]:
            self.commit_end = self.now
        if self.irq_due is not None and self.now >= self.irq_due and not self.injected:
            if self.p & I:
                self.irq_deferred = True
            else:
                self.irq(180)
                self.injected = True
        op = self.byte()
        # Deliver a pending IRQ immediately before SEI, after the first safe
        # poll. A fresh masked sample must reject the now-unsafe position.
        if op == 0x78 and self.injection == "before_final" and not self.injected:
            self.irq(12 * (self.line_cycles // 64))
            self.injected = True
        cycles = 0
        if op in (0xa9, 0xa0):
            value = self.nz(self.byte())
            if op == 0xa9:
                self.a = value
            else:
                self.y = value
            cycles = 2
        elif op == 0xa5:
            self.a = self.nz(self.read(self.byte()))
            cycles = 3
        elif op == 0xad:
            self.a = self.nz(self.read(self.word()))
            cycles = 4
        elif op == 0xb9:
            base = self.word()
            address = (base + self.y) & 65535
            self.a = self.nz(self.read(address))
            cycles = 4 + ((base & 0xff00) != (address & 0xff00))
        elif op == 0x85:
            self.write(self.byte(), self.a)
            cycles = 3
        elif op == 0x8d:
            self.write(self.word(), self.a)
            cycles = 4
        elif op == 0x91:
            zp = self.byte()
            address = self.mem[zp] | self.mem[(zp + 1) & 255] << 8
            self.write((address + self.y) & 65535, self.a)
            cycles = 6
        elif op in (0x69, 0x65):
            right = self.byte() if op == 0x69 else self.read(self.byte())
            assert not self.p & D, "address arithmetic ran in decimal mode"
            result = self.a + right + bool(self.p & C)
            overflow = (~(self.a ^ right) & (self.a ^ result) & 128) != 0
            self.p = (self.p & ~(C | V)) | (C if result > 255 else 0) | (V if overflow else 0)
            self.a = self.nz(result)
            cycles = 2 if op == 0x69 else 3
        elif op == 0x29:
            self.a = self.nz(self.a & self.byte())
            cycles = 2
        elif op in (0xc5, 0xc0):
            self.compare(self.a if op == 0xc5 else self.y,
                         self.read(self.byte()) if op == 0xc5 else self.byte())
            cycles = 3 if op == 0xc5 else 2
        elif op == 0x0a:
            carry = self.a >> 7
            self.a = self.nz(self.a << 1)
            self.p = (self.p & ~C) | carry
            cycles = 2
        elif op == 0x26:
            address = self.byte()
            value = self.read(address)
            carry = value >> 7
            self.write(address, self.nz(value << 1 | (self.p & C)))
            self.p = (self.p & ~C) | carry
            cycles = 5
        elif op == 0xe6:
            address = self.byte()
            self.write(address, self.nz(self.read(address) + 1))
            cycles = 5
        elif op == 0xc8:
            self.y = self.nz(self.y + 1)
            cycles = 2
        elif op in (0xd0, 0x90, 0xb0):
            delta = self.byte()
            take = not self.p & (Z if op == 0xd0 else C) if op != 0xb0 else self.p & C
            cycles = 2
            if take:
                target = self.pc + (delta if delta < 128 else delta - 256)
                cycles += 1 + ((target & 0xff00) != (self.pc & 0xff00))
                self.pc = target
        elif op == 0x20:
            target = self.word()
            self.push((self.pc - 1) >> 8)
            self.push((self.pc - 1) & 255)
            self.pc = target
            cycles = 6
        elif op == 0x60:
            self.pc = (self.pop() | self.pop() << 8) + 1
            cycles = 6
        elif op == 0x08:
            self.push(self.p | B | U)
            cycles = 3
        elif op == 0x28:
            self.p = self.pop()
            cycles = 4
            if self.mask_start is not None:
                self.max_mask = max(self.max_mask, self.now + cycles - self.mask_start)
                self.mask_start = None
        elif op in (0x18, 0x38, 0x58, 0x78, 0xd8):
            if op in (0x18, 0x58, 0xd8):
                self.p &= ~{0x18: C, 0x58: I, 0xd8: D}[op]
            else:
                self.p |= C if op == 0x38 else I
            if op == 0x78:
                self.mask_start = self.now
            if op == 0x58 and self.mask_start is not None:
                self.max_mask = max(self.max_mask, self.now + 2 - self.mask_start)
                self.mask_start = None
            cycles = 2
        else:
            raise AssertionError(f"unsupported opcode ${op:02x} at ${pc:04x}")
        self.now += cycles
        self.steps += 1
        assert self.steps < 100000, "publisher did not terminate"

    def run(self):
        while self.pc != 0xffff:
            self.step()
        expected = {}
        for source, dest, length in ((0x9540, 0xa000 + 320 * self.band, 320),
                                     (0x7700, 0x8000 + 40 * self.band, 40),
                                     (0x7728, 0xd800 + 40 * self.band, 40)):
            expected.update((dest + n, self.before[source + n]) for n in range(length))
            assert self.mem[source:source + length] == self.before[source:source + length]
        assert len(self.writes) == 400
        assert {address: value for address, value, _ in self.writes} == expected
        assert self.wait_irqs_enabled
        assert self.s == 0xfd
        allowed = set(expected) | set(range(4, 18)) | set(range(0x100, 0x200)) | {PROFILE}
        assert all(a in allowed or x == y for a, (x, y) in enumerate(zip(self.before, self.mem)))
        assert self.commit_start is not None and self.commit_end is not None
        assert self.max_mask < 7 * self.line_cycles
        return self


def main():
    BUILD.mkdir(parents=True, exist_ok=True)
    linker = "ENTRY(publish_staged_band)\n__rc0 = 2; INCLUDE imag-regs.ld\n"
    linker += "SECTIONS { . = 0x3000; .text : { *(.phase.pixels.publish) } /DISCARD/ : { *(.comment) } }\n"
    linker += f"\nPROF_DETAIL = {PROFILE};\n"
    (BUILD / "kernel.ld").write_text(linker)
    source = ROOT / "target-c64/asm/publish.S"
    subprocess.run([SDK / "mos-sim-clang", "-c", source, "-o", BUILD / "kernel.o"], check=True)
    subprocess.run([SDK / "ld.lld", "-T", BUILD / "kernel.ld", "-L",
                    SDK.parent / "mos-platform/common/lib", BUILD / "kernel.o",
                    "-o", BUILD / "kernel.elf"], check=True)
    subprocess.run([SDK / "llvm-objcopy", "-O", "binary", "--only-section=.text",
                    BUILD / "kernel.elf", BUILD / "kernel.bin"], check=True)
    symbols = {}
    for line in subprocess.check_output([SDK / "llvm-nm", BUILD / "kernel.elf"], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    code = (BUILD / "kernel.bin").read_bytes()
    cases = 0
    max_mask = max_commit = max_wait = 0
    rollover_retries = 0
    for lines in (312, 263):
        for band in range(25):
            start, end = 40 + 8 * band, 64 + 8 * band
            positions = {0, lines - .01, 255.98, 256.02, 250.99, 251.01}
            positions.update(x + delta for x in (start, start + 8, end - 1, end)
                             for delta in (-.02, .02, .98))
            for position in sorted(positions):
                flags = U | (N | D | V | C | Z if cases & 1 else 0)
                machine = Machine(code, symbols, band, lines, position, flags).run()
                assert machine.p & ~(B | U) == flags & ~(B | U)
                max_mask = max(max_mask, machine.max_mask)
                max_commit = max(max_commit, machine.commit_end - machine.commit_start)
                max_wait = max(max_wait, machine.commit_start - machine.started)
                cases += 1
            machine = Machine(code, symbols, band, lines, start - .8, injection="before_final").run()
            assert machine.injected and machine.irq_count == 1 and machine.final_checks >= 2
            machine = Machine(code, symbols, band, lines, start + 1, injection="waiting").run()
            assert machine.injected and machine.irq_count == 1 and machine.wait_visits > 3
            cases += 2
        # Slide the polling instructions across both ninth-bit transitions,
        # including an actual mismatched high/low/high sample that must retry.
        for boundary in (256, lines):
            for phase in range(32):
                machine = Machine(code, symbols, 24, lines, boundary - .15 + phase / 200).run()
                for n in range(0, len(machine.raster_reads) - 2, 3):
                    first, _, last = machine.raster_reads[n:n + 3]
                    rollover_retries += (first[1] >= 256) != (last[1] >= 256)
                cases += 1
        machine = Machine(code, symbols, 10, lines, 250.8, injection="deferred_251").run()
        assert machine.injected and machine.irq_count == 1 and machine.irq_deferred
        cases += 1
    assert rollover_retries >= 4, "raster high-bit rollover paths were not exercised"
    # Static conservative bound from the final sample through PLP: at most two
    # coherent sampling attempts, branch-page penalties on every copy branch,
    # page penalties on every indexed source load, and a full EXTRA bus cycle
    # for each of six VIC reads and each of forty color stores. No IRQ can run.
    copy_bound = (2 + 32 * (8 * (5 + 6 + 2) + 4) + 5
                  + 8 * (8 * (5 + 6 + 2) + 2 + 4) + 2
                  + 10 * (4 * (4 + 6 + 4 + 6 + 2) + 2 + 4) + 4)
    sample_bound = 2 * 100 + 40
    static_bound = copy_bound + sample_bound + 46 * 64
    assert max_mask <= static_bound < 7 * 63 * 64
    report = {"cases": cases, "kernel_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
              "assembled_bytes": len(code), "max_mask_cpu_cycles": max_mask,
              "max_commit_cpu_cycles": max_commit, "static_mask_bound_cpu_cycles": static_bound,
              "static_mask_bound_pal_lines": static_bound / (63 * 64),
              "coherent_raster_rollover_retries": rollover_retries,
              "max_wait_pal_line_units": max_wait / (63 * 64),
              "io_model": "64x CPU plus one extra bus cycle per VIC read/color store",
              "physical_hardware_timing": "not measured"}
    (BUILD / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS {cases} publication cases; {len(code)} assembled bytes; "
          f"masked bound {static_bound / (63 * 64):.3f} PAL lines (<7)")


if __name__ == "__main__":
    main()
