#!/usr/bin/env python3
"""Execute the production sprite publisher and shake placement with a VIC model.

Reuses the bitmap publisher's cycle-counted 6502 interpreter. All VIC accesses
are charged an extra full 1-MHz bus cycle; this checks guard margins and state
ownership, not physical Ultimate timing.
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

spec = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("test-publish-kernel.py"))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
ROOT, SDK = publisher.ROOT, publisher.SDK
C, Z, I, D, B, U, V, N = (1 << n for n in range(8))
BUILD = ROOT / "target/hud-kernel-test"
WRITES, COMMITS, SCROLL = 0x7800, 0x7802, 0x7804


class Machine(publisher.Machine):
    def __init__(self, code, symbols, corner, lines, position, changed="all", flags=U, injection=None):
        aliases = dict(symbols, publish_staged_band=symbols["publish_hud_corner"],
                       publish_wait=-1, publish_guard=-1, publish_commit=-1, publish_done=-1)
        super().__init__(code, aliases, corner, lines, position, flags, injection)
        self.hud_symbols = symbols
        self.corner = corner
        self.scroll_call = False
        self.start, self.end = (44, 84) if corner < 2 else (212, 254)
        self.destination = 0x8c00 + corner * 128
        self.mem[0x8e80] = 3
        self.mem[WRITES:WRITES + 2] = (255).to_bytes(2, "little")
        self.mem[COMMITS:COMMITS + 2] = (65535).to_bytes(2, "little")
        self.changed = set(range(128)) if changed == "all" else ({0, 63, 64, 127} if changed == "sparse" else set())
        for n in range(128):
            self.mem[self.destination + n] = self.mem[0x8e00 + n] ^ (255 if n in self.changed else 0)
        for n in range(2):
            # Upper bits in VIC color reads must be ignored for dirty checks.
            self.mem[0xd027 + corner * 2 + n] = 0xf3 if changed == "none" else (4 + n)
        self.before = self.mem[:]

    def read(self, address):
        if 0xd000 <= address < 0xd040 and address not in (0xd011, 0xd012):
            self.now += 64
            self.io += 1
        return super().read(address)

    def write(self, address, value):
        if 0xd000 <= address < 0xd040:
            self.now += 64
            self.io += 1
        if 0x8c00 <= address < 0x8e00 or 0xd000 <= address < 0xd040:
            if not self.scroll_call:
                assert self.p & I, "sprite publication with IRQs enabled"
                base_y = 54 if self.corner < 2 else 225
                for displacement in range(-3, 5):
                    assert not base_y + displacement - 1 <= self.line < base_y + displacement + 22, (
                        "sprite store during possible DMA", self.corner, self.line, hex(address))
            self.writes.append((address, value, self.now))
        self.mem[address] = value

    def step(self):
        if not self.scroll_call:
            if self.pc == self.hud_symbols["hud_publish_wait"]:
                self.wait_visits += 1
                self.wait_irqs_enabled &= not bool(self.p & I)
                if self.injection == "waiting" and not self.injected and self.wait_visits == 3:
                    self.irq(180)
                    self.injected = True
            if self.pc == self.hud_symbols["hud_publish_guard"]:
                self.final_checks += 1
                assert self.p & I
            if self.pc == self.hud_symbols["hud_publish_commit"]:
                self.commit_start = self.now
                assert not self.start <= self.line < self.end
                if self.injection == "deferred_251":
                    self.irq_due = 251 * self.line_cycles
                    assert self.now < self.irq_due
            if self.pc == self.hud_symbols["hud_publish_done"]:
                self.commit_end = self.now
        op = self.mem[self.pc]
        extra = (0xa2, 0xa6, 0xbd, 0x99, 0x9d, 0xc9, 0xcd, 0xd1, 0xe0,
                 0x4a, 0x6a, 0xee, 0xe8, 0xf0, 0x48, 0x68, 0xe9)
        if op not in extra:
            super().step()
            return
        self.byte()
        if op in (0xa2, 0xa6):
            self.x = self.nz(self.byte() if op == 0xa2 else self.read(self.byte()))
            cycles = 2 if op == 0xa2 else 3
        elif op == 0xbd:
            base = self.word()
            address = (base + self.x) & 65535
            self.a = self.nz(self.read(address))
            cycles = 4 + ((base & 0xff00) != (address & 0xff00))
        elif op in (0x99, 0x9d):
            self.write((self.word() + (self.y if op == 0x99 else self.x)) & 65535, self.a)
            cycles = 5
        elif op in (0xc9, 0xcd, 0xd1, 0xe0):
            if op == 0xd1:
                zp = self.byte()
                base = self.mem[zp] | self.mem[(zp + 1) & 255] << 8
                address = (base + self.y) & 65535
                right = self.read(address)
                cycles = 5 + ((base & 0xff00) != (address & 0xff00))
            else:
                right = self.read(self.word()) if op == 0xcd else self.byte()
                cycles = 4 if op == 0xcd else 2
            self.compare(self.x if op == 0xe0 else self.a, right)
        elif op in (0x4a, 0x6a):
            carry = self.a & 1
            self.a = self.nz((self.a >> 1) | ((self.p & C) << 7 if op == 0x6a else 0))
            self.p = (self.p & ~C) | carry
            cycles = 2
        elif op == 0xee:
            address = self.word()
            self.write(address, self.nz(self.read(address) + 1))
            cycles = 6
        elif op == 0xe8:
            self.x = self.nz(self.x + 1)
            cycles = 2
        elif op == 0xf0:
            delta = self.byte()
            cycles = 2
            if self.p & Z:
                target = self.pc + (delta if delta < 128 else delta - 256)
                cycles += 1 + ((target & 0xff00) != (self.pc & 0xff00))
                self.pc = target
        elif op == 0x48:
            self.push(self.a)
            cycles = 3
        elif op == 0x68:
            self.a = self.nz(self.pop())
            cycles = 4
        elif op == 0xe9:
            right = self.byte()
            assert not self.p & D
            result = self.a - right - (not self.p & C)
            overflow = ((self.a ^ right) & (self.a ^ result) & 128) != 0
            self.p = (self.p & ~(C | V)) | (C if result >= 0 else 0) | (V if overflow else 0)
            self.a = self.nz(result)
            cycles = 2
        self.now += cycles
        self.steps += 1
        assert self.steps < 100000, "HUD kernel did not terminate"

    def run(self):
        while self.pc != 0xffff:
            self.step()
        expected = {self.destination + n: self.before[0x8e00 + n] for n in self.changed}
        for n in range(2):
            address = 0xd027 + self.corner * 2 + n
            if self.before[address] & 15 != self.before[0x8e80]:
                expected[address] = self.before[0x8e80]
        assert len(self.writes) == len(expected), "unchanged sprite bytes were written"
        assert {address: value for address, value, _ in self.writes} == expected
        assert self.mem[self.destination:self.destination + 128] == self.before[0x8e00:0x8e80]
        assert int.from_bytes(self.mem[WRITES:WRITES + 2], "little") == 255 + len(self.changed)
        assert int.from_bytes(self.mem[COMMITS:COMMITS + 2], "little") == 0
        allowed = set(expected) | set(range(4, 18)) | set(range(0x100, 0x200)) | set(range(WRITES, COMMITS + 2))
        assert all(address in allowed or old == new for address, (old, new) in enumerate(zip(self.before, self.mem)))
        assert self.wait_irqs_enabled and self.s == 0xfd
        assert self.commit_start is not None and self.commit_end is not None
        assert self.max_mask < 2 * self.line_cycles, "HUD publication exceeds two raster lines"
        return self


def main():
    BUILD.mkdir(parents=True, exist_ok=True)
    linker = "ENTRY(publish_hud_corner)\n__rc0 = 2; INCLUDE imag-regs.ld\n"
    linker += "SECTIONS { . = 0x3000; .text : { *(.phase.pixels.hud_publish) *(.text.hud_apply_scroll) *(.rodata.hud_positions) } /DISCARD/ : { *(.comment) } }\n"
    linker += f"HUD_IMAGE_WRITES = {WRITES}; HUD_COMMITS = {COMMITS}; HUD_SCROLL = {SCROLL};\n"
    (BUILD / "kernel.ld").write_text(linker)
    source = ROOT / "target-c64/asm/hud.S"
    subprocess.run([SDK / "mos-sim-clang", "-c", source, "-o", BUILD / "kernel.o"], check=True)
    subprocess.run([SDK / "ld.lld", "-T", BUILD / "kernel.ld", "-L", SDK.parent / "mos-platform/common/lib",
                    BUILD / "kernel.o", "-o", BUILD / "kernel.elf"], check=True)
    subprocess.run([SDK / "llvm-objcopy", "-O", "binary", "--only-section=.text", BUILD / "kernel.elf", BUILD / "kernel.bin"], check=True)
    symbols = {}
    for line in subprocess.check_output([SDK / "llvm-nm", BUILD / "kernel.elf"], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    code = (BUILD / "kernel.bin").read_bytes()
    cases = max_mask = rollover_retries = 0
    for lines in (312, 263):
        for corner in range(4):
            start, end = (44, 84) if corner < 2 else (212, 254)
            positions = {0, lines - .01, 255.98, 256.02, 250.8, 251.01}
            positions.update(boundary + delta for boundary in (start, end - 1, end) for delta in (-.02, .02, .98))
            for position in sorted(positions):
                for changed in ("all", "sparse", "none"):
                    flags = U | (N | D | V | C | Z if cases & 1 else 0) | (I if cases % 5 == 0 else 0)
                    machine = Machine(code, symbols, corner, lines, position, changed, flags).run()
                    assert machine.p & ~(B | U) == flags & ~(B | U)
                    max_mask = max(max_mask, machine.max_mask)
                    cases += 1
            machine = Machine(code, symbols, corner, lines, start - .8, injection="before_final").run()
            assert machine.injected and machine.final_checks >= 2
            machine = Machine(code, symbols, corner, lines, start + 1, injection="waiting").run()
            assert machine.injected and machine.irq_count == 1
            cases += 2
        for boundary in (256, lines):
            for phase in range(32):
                machine = Machine(code, symbols, 3, lines, boundary - .15 + phase / 200).run()
                for n in range(0, len(machine.raster_reads) - 2, 3):
                    first, _, last = machine.raster_reads[n:n + 3]
                    rollover_retries += (first[1] >= 256) != (last[1] >= 256)
                cases += 1
        machine = Machine(code, symbols, 0, lines, 250.8, injection="deferred_251").run()
        assert machine.injected and machine.irq_count == 1 and machine.irq_deferred
        cases += 1
    assert rollover_retries >= 4, "high-bit rollover paths were not exercised"
    # Reuse one memory image through every shake sample, then in reverse, to
    # prove placement derives from fixed bases instead of accumulating offsets.
    machine = Machine(code, symbols, 0, 312, 0)
    machine.scroll_call = True
    scroll_cases = 0
    for packed in [*range(64), *reversed(range(64)), 24]:
        machine.a, machine.p = packed, U | I
        machine.pc = symbols["hud_apply_scroll"]
        machine.s = 0xfd
        machine.push(0xff)
        machine.push(0xfe)
        before = machine.mem[:]
        while machine.pc != 0xffff:
            machine.step()
        expected = {SCROLL: packed, 0xd010: 0xcc}
        for sprite, base_x in enumerate((30, 54, 34, 58, 30, 54, 34, 58)):
            expected[0xd000 + sprite * 2] = base_x + (packed & 7)
            expected[0xd001 + sprite * 2] = (54 if sprite < 4 else 225) + (packed >> 3) - 3
        assert all(machine.mem[address] == value for address, value in expected.items())
        allowed = set(expected) | {4, 5} | set(range(0x100, 0x200))
        assert all(address in allowed or old == new for address, (old, new) in enumerate(zip(before, machine.mem)))
        assert machine.s == 0xfd
        scroll_cases += 1
    report = dict(passed=True, publication_cases=cases, scroll_cases=scroll_cases,
                  rollover_retries=rollover_retries, max_mask_cycles=max_mask,
                  max_mask_pal_lines=max_mask / (63 * 64),
                  source_sha256=hashlib.sha256(source.read_bytes()).hexdigest())
    (BUILD / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS {cases} HUD publication cases, {scroll_cases} scroll samples; masked bound {report['max_mask_pal_lines']:.3f} PAL lines (<2)")


if __name__ == "__main__":
    main()
