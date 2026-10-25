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
STATE = dict(HUD_SPRITE0_COLOR=0x7805, HUD_ENABLE_PENDING=0x7806,
             HUD_ENABLE_ACTIVE=0x7807, RETICLE_PENDING_X=0x7808,
             RETICLE_PENDING_Y=0x780a, RETICLE_PENDING_VISIBLE=0x780b,
             RETICLE_ACTIVE_X=0x780c, RETICLE_ACTIVE_Y=0x780e,
             RETICLE_ACTIVE_VISIBLE=0x780f, RETICLE_MISSES=0x7810,
             SPRITE_PHASE=0x7812, IRQ_FRAME_EVENT=0x7813,
             TEST_FRAMES=0x7814, TEST_CIA_TICKS=0x7815)


class Machine(publisher.Machine):
    def __init__(self, code, symbols, corner, lines, position, changed="all", flags=U, injection=None):
        aliases = dict(symbols, publish_staged_band=symbols["publish_hud_corner"],
                       publish_wait=-1, publish_guard=-1, publish_commit=-1, publish_done=-1)
        super().__init__(code, aliases, corner, lines, position, flags, injection)
        self.hud_symbols = symbols
        self.corner = corner
        self.scroll_call = False
        self.irq_call = False
        self.cia_reads = 0
        self.late_raster = False
        self.start, self.end = (52, 92) if corner < 2 else (212, 254)
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
        if self.irq_call and address == 0xdc0d:
            self.cia_reads += 1
            value = self.mem[address]
            self.mem[address] = 0
            return value
        if 0xd000 <= address < 0xd040 and address not in (0xd011, 0xd012):
            self.now += 64
            self.io += 1
        return super().read(address)

    def write(self, address, value):
        if 0xd000 <= address < 0xd040:
            assert self.p & I, "VIC register write with IRQs enabled"
            self.now += 64
            self.io += 1
        if 0x8c00 <= address < 0x8e00 or 0xd000 <= address < 0xd040:
            if not self.scroll_call:
                assert self.p & I, "sprite publication with IRQs enabled"
                base_y = 62 if self.corner < 2 else 225
                for displacement in range(-4, 4):
                    assert not base_y + displacement - 1 <= self.line < base_y + displacement + 22, (
                        "sprite store during possible DMA", self.corner, self.line, hex(address))
            self.writes.append((address, value, self.now))
        if self.irq_call and address == 0xd019:
            self.mem[address] &= ~value
        else:
            self.mem[address] = value

    def step(self):
        if self.late_raster and self.pc == self.hud_symbols.get("mock_audio_end"):
            self.mem[0xd019] |= 1
            self.late_raster = False
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
                 0x4a, 0x6a, 0xee, 0xe8, 0xf0, 0x48, 0x68, 0xe9, 0xe5,
                 0x6d, 0xac, 0x98, 0xaa, 0x8a, 0xa8, 0x09, 0xb5,
                 0x95, 0xca, 0x10, 0x30, 0xc4, 0x40, 0x4c)
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
        elif op in (0xf0, 0x10, 0x30):
            delta = self.byte()
            cycles = 2
            take = bool(self.p & Z) if op == 0xf0 else bool(self.p & N) == (op == 0x30)
            if take:
                target = self.pc + (delta if delta < 128 else delta - 256)
                cycles += 1 + ((target & 0xff00) != (self.pc & 0xff00))
                self.pc = target
        elif op == 0x48:
            self.push(self.a)
            cycles = 3
        elif op == 0x68:
            self.a = self.nz(self.pop())
            cycles = 4
        elif op in (0xe9, 0xe5):
            right = self.byte() if op == 0xe9 else self.read(self.byte())
            assert not self.p & D
            result = self.a - right - (not self.p & C)
            overflow = ((self.a ^ right) & (self.a ^ result) & 128) != 0
            self.p = (self.p & ~(C | V)) | (C if result >= 0 else 0) | (V if overflow else 0)
            self.a = self.nz(result)
            cycles = 2 if op == 0xe9 else 3
        elif op == 0x6d:
            right = self.read(self.word())
            result = self.a + right + bool(self.p & C)
            overflow = (~(self.a ^ right) & (self.a ^ result) & 128) != 0
            self.p = (self.p & ~(C | V)) | (C if result > 255 else 0) | (V if overflow else 0)
            self.a = self.nz(result)
            cycles = 4
        elif op == 0xac:
            self.y = self.nz(self.read(self.word()))
            cycles = 4
        elif op in (0x98, 0xaa, 0x8a, 0xa8):
            if op == 0x98:
                self.a = self.nz(self.y)
            elif op == 0xaa:
                self.x = self.nz(self.a)
            elif op == 0x8a:
                self.a = self.nz(self.x)
            else:
                self.y = self.nz(self.a)
            cycles = 2
        elif op == 0x09:
            self.a = self.nz(self.a | self.byte())
            cycles = 2
        elif op == 0xb5:
            self.a = self.nz(self.read((self.byte() + self.x) & 255))
            cycles = 4
        elif op == 0x95:
            self.write((self.byte() + self.x) & 255, self.a)
            cycles = 4
        elif op == 0xca:
            self.x = self.nz(self.x - 1)
            cycles = 2
        elif op == 0xc4:
            self.compare(self.y, self.read(self.byte()))
            cycles = 3
        elif op == 0x40:
            self.p = self.pop()
            self.pc = self.pop() | self.pop() << 8
            cycles = 6
        elif op == 0x4c:
            self.pc = self.word()
            cycles = 3
        self.now += cycles
        self.steps += 1
        assert self.steps < 100000, "HUD kernel did not terminate"

    def run(self):
        while self.pc != 0xffff:
            self.step()
        expected = {self.destination + n: self.before[0x8e00 + n] for n in self.changed}
        for n in range(2):
            address = 0xd027 + self.corner * 2 + n
            if self.corner == 0 and n == 0:
                assert self.mem[STATE["HUD_SPRITE0_COLOR"]] == self.before[0x8e80]
                assert self.mem[address] == self.before[address], "foreground changed borrowed sprite color"
                continue
            if self.before[address] & 15 != self.before[0x8e80]:
                expected[address] = self.before[0x8e80]
        assert len(self.writes) == len(expected), "unchanged sprite bytes were written"
        assert {address: value for address, value, _ in self.writes} == expected
        assert self.mem[self.destination:self.destination + 128] == self.before[0x8e00:0x8e80]
        assert int.from_bytes(self.mem[WRITES:WRITES + 2], "little") == 255 + len(self.changed)
        assert int.from_bytes(self.mem[COMMITS:COMMITS + 2], "little") == 0
        allowed = set(expected) | set(range(4, 18)) | set(range(0x100, 0x200)) | set(range(WRITES, COMMITS + 2))
        if self.corner == 0:
            allowed.add(STATE["HUD_SPRITE0_COLOR"])
        assert all(address in allowed or old == new for address, (old, new) in enumerate(zip(self.before, self.mem)))
        assert self.wait_irqs_enabled and self.s == 0xfd
        assert self.commit_start is not None and self.commit_end is not None
        assert self.max_mask < 2 * self.line_cycles, "HUD publication exceeds two raster lines"
        return self


def call(machine, symbol, value=0, flags=U | I):
    machine.a, machine.p, machine.s = value, flags, 0xfd
    machine.pc = machine.hud_symbols[symbol]
    machine.push(0xff)
    machine.push(0xfe)
    start = machine.now
    while machine.pc != 0xffff:
        machine.step()
    assert machine.s == 0xfd
    return machine.now - start


def mux_machine(code, symbols, lines, position=92):
    machine = Machine(code, symbols, 0, lines, position)
    machine.scroll_call = True
    for address in STATE.values():
        machine.mem[address] = 0
    for name in ("RETICLE_PENDING_X", "RETICLE_ACTIVE_X", "RETICLE_MISSES"):
        machine.mem[STATE[name] + 1] = 0
    machine.mem[STATE["HUD_SPRITE0_COLOR"]] = 3
    machine.mem[STATE["HUD_ENABLE_PENDING"]] = 0xff
    machine.mem[STATE["HUD_ENABLE_ACTIVE"]] = 0xff
    machine.mem[SCROLL] = 24
    machine.mem[0xd015] = machine.mem[0xd01c] = 0xff
    machine.mem[0xd010] = 0xcc
    machine.mem[0xd017] = machine.mem[0xd01d] = 0
    machine.mem[0x83f8:0x8400] = bytes(range(0x30, 0x38))
    machine.mem[STATE["RETICLE_ACTIVE_X"]] = 170
    machine.mem[STATE["RETICLE_ACTIVE_Y"]] = 140
    machine.mem[STATE["RETICLE_ACTIVE_VISIBLE"]] = 1
    return machine


def enter_irq(machine, raster, cia, phase):
    machine.irq_call = True
    machine.mem[0xd019], machine.mem[0xdc0d] = raster, cia
    machine.mem[STATE["SPRITE_PHASE"]] = phase
    machine.a, machine.x, machine.y, machine.s = 0xa6, 0x52, 0x39, 0xfd
    machine.p = U | N | V | D | C
    machine.mem[1] = 0x34
    before_regs = bytes(machine.mem[2:34])
    machine.push(0xff)
    machine.push(0xff)
    machine.push(machine.p)
    machine.p |= I
    machine.pc = machine.hud_symbols["irq_entry"]
    while machine.pc != 0xffff:
        machine.step()
    assert (machine.a, machine.x, machine.y, machine.s, machine.p, machine.mem[1]) == (
        0xa6, 0x52, 0x39, 0xfd, U | N | V | D | C, 0x34), "IRQ corrupted interrupted CPU state"
    assert bytes(machine.mem[2:34]) == before_regs, "IRQ corrupted imaginary registers"


def test_multiplexer(code, symbols):
    cases = irq_cases = max_arm = 0
    for lines in (312, 263):
        # All fine-scroll phases, including high-X carries beyond gameplay's
        # current range, preserve the other seven sprites and shared colors.
        for packed in range(64):
            for x in (140, 202, 255, 256):
                machine = mux_machine(code, symbols, lines)
                machine.mem[SCROLL] = packed
                machine.mem[STATE["RETICLE_ACTIVE_X"]:STATE["RETICLE_ACTIVE_X"] + 2] = x.to_bytes(2, "little")
                y = 137 if packed & 1 else 163
                machine.mem[STATE["RETICLE_ACTIVE_Y"]] = y
                before = machine.mem[:]
                max_arm = max(max_arm, call(machine, "reticle_arm"))
                expected_x = x + (packed & 7)
                assert machine.mem[0xd000] == expected_x & 255
                assert machine.mem[0xd001] == y + (packed >> 3) - 3
                assert machine.mem[0xd010] == (before[0xd010] & 0xfe) | ((expected_x >> 8) & 1)
                assert machine.mem[0xd015] == before[0xd015] | 1
                assert machine.mem[0xd01c] == before[0xd01c] & 0xfe
                assert machine.mem[0xd027] == 1 and machine.mem[0x83f8] == 0xff
                allowed = {0xd000, 0xd001, 0xd010, 0xd015, 0xd01c, 0xd027, 0x83f8} | set(range(0x100, 0x200))
                assert all(address in allowed or old == new for address, (old, new) in enumerate(zip(before, machine.mem))), "arm modified unrelated state"
                assert machine.mem[STATE["RETICLE_MISSES"]:STATE["RETICLE_MISSES"] + 2] == bytes(2)
                assert machine.now / machine.line_cycles < machine.mem[0xd001] - 7
                cases += 1

        # Captions retire an already armed sight immediately. Before line 92
        # the same physical slot still belongs to the HUD and must stay on.
        for phase, position in ((0, 60), (0, 91.9), (1, 92.1), (1, 100), (1, 150)):
            for enabled in (0, 1, 3, 0xaa, 0xff):
                for flags in (U, U | D | C | V, U | I):
                    machine = mux_machine(code, symbols, lines, position)
                    machine.mem[STATE["SPRITE_PHASE"]] = phase
                    machine.mem[STATE["RETICLE_PENDING_VISIBLE"]] = 1
                    machine.mem[0xd015] = enabled
                    before = machine.mem[:]
                    call(machine, "reticle_hide", flags=flags)
                    assert machine.mem[0xd015] == (enabled & 0xfe if phase else enabled)
                    assert machine.mem[STATE["RETICLE_ACTIVE_VISIBLE"]] == 0
                    assert machine.mem[STATE["RETICLE_PENDING_VISIBLE"]] == 0
                    assert machine.p & ~(B | U) == flags & ~(B | U)
                    allowed = {0xd015, STATE["RETICLE_ACTIVE_VISIBLE"], STATE["RETICLE_PENDING_VISIBLE"]} | set(range(0x100, 0x200))
                    assert all(address in allowed or old == new for address, (old, new) in enumerate(zip(before, machine.mem)))
                    cases += 1

        # Deadline edges, ninth-bit/frame rollover and counter carry. A missed
        # occurrence clears active visibility but leaves the pending sample.
        for position, visible in ((92, 0), (131.5, 1), (132.5, 1), (133.5, 1), (139, 1),
                                  (251, 1), (255.99, 1), (256.01, 1), (lines - .01, 1)):
            machine = mux_machine(code, symbols, lines, position)
            machine.mem[STATE["RETICLE_ACTIVE_VISIBLE"]] = visible
            machine.mem[STATE["RETICLE_PENDING_VISIBLE"]] = 1
            machine.mem[STATE["RETICLE_MISSES"]:STATE["RETICLE_MISSES"] + 2] = (255).to_bytes(2, "little")
            call(machine, "reticle_arm")
            missed = visible and position >= 133
            assert bool(machine.mem[0xd015] & 1) == bool(visible and not missed), (lines, position, visible, missed, machine.line, machine.mem[0xd001], machine.mem[0xd015], machine.raster_reads)
            assert machine.mem[STATE["RETICLE_ACTIVE_VISIBLE"]] == (visible and not missed)
            assert int.from_bytes(machine.mem[STATE["RETICLE_MISSES"]:STATE["RETICLE_MISSES"] + 2], "little") == 255 + missed
            assert machine.mem[STATE["RETICLE_PENDING_VISIBLE"]] == 1
            cases += 1

        for packed in range(64):
            for enabled, visible in ((0xff, 1), (0xff, 0), (3, 0), (0, 0)):
                machine = mux_machine(code, symbols, lines, 251)
                machine.mem[SCROLL] = packed
                machine.mem[0xd01c] &= 0xfe
                machine.mem[STATE["HUD_ENABLE_PENDING"]] = enabled
                machine.mem[STATE["RETICLE_PENDING_X"]:STATE["RETICLE_PENDING_X"] + 2] = (202).to_bytes(2, "little")
                machine.mem[STATE["RETICLE_PENDING_Y"]] = 163
                machine.mem[STATE["RETICLE_PENDING_VISIBLE"]] = visible
                call(machine, "hud_apply_scroll", packed)
                call(machine, "reticle_restore")
                assert machine.mem[0xd000] == 30 - (packed & 7)
                assert machine.mem[0xd001] == 62 - ((packed >> 3) - 3)
                assert machine.mem[0xd010] == 0xcc and machine.mem[0xd01c] == 0xff
                assert machine.mem[0xd027] == 3 and machine.mem[0x83f8] == 0x30
                assert machine.mem[0xd015] == machine.mem[STATE["HUD_ENABLE_ACTIVE"]] == enabled
                assert machine.mem[STATE["RETICLE_ACTIVE_X"]:STATE["RETICLE_ACTIVE_X"] + 2] == (202).to_bytes(2, "little")
                assert machine.mem[STATE["RETICLE_ACTIVE_Y"]] == 163
                assert machine.mem[STATE["RETICLE_ACTIVE_VISIBLE"]] == visible
                cases += 1

        # A middle raster + CIA edge must use the short path without reading
        # CIA ICR, then service the still-pending audio without a frame sample.
        machine = mux_machine(code, symbols, lines)
        enter_irq(machine, 1, 1, 0)
        assert machine.cia_reads == 0 and machine.mem[0xdc0d] == 1
        assert machine.mem[0xd019] == 0 and machine.mem[0xd012] == 251
        assert machine.mem[STATE["SPRITE_PHASE"]] == 1
        assert machine.mem[STATE["TEST_FRAMES"]] == machine.mem[STATE["TEST_CIA_TICKS"]] == 0
        enter_irq(machine, 0, 1, 1)
        assert machine.cia_reads == 1 and machine.mem[STATE["TEST_CIA_TICKS"]] == 1
        assert machine.mem[STATE["TEST_FRAMES"]] == 0
        irq_cases += 2

        machine = mux_machine(code, symbols, lines, 251)
        enter_irq(machine, 1, 1, 1)
        assert machine.mem[STATE["TEST_FRAMES"]] == machine.mem[STATE["TEST_CIA_TICKS"]] == 1
        assert machine.mem[0xd012] == 92 and machine.mem[STATE["SPRITE_PHASE"]] == 0
        assert machine.mem[0x83f8] == 0x30
        irq_cases += 1

        # A middle raster becoming pending during the full CIA handler cannot
        # be misclassified as a second line-251 frame event by Rust.
        machine = mux_machine(code, symbols, lines)
        machine.late_raster = True
        enter_irq(machine, 0, 1, 0)
        assert machine.mem[0xd019] == 1 and machine.mem[STATE["TEST_FRAMES"]] == 0
        enter_irq(machine, 1, 0, 0)
        assert machine.mem[0xd019] == 0 and machine.mem[STATE["TEST_FRAMES"]] == 0
        assert machine.mem[STATE["TEST_CIA_TICKS"]] == 1
        irq_cases += 2
    return cases, irq_cases, max_arm


def main():
    BUILD.mkdir(parents=True, exist_ok=True)
    linker = "ENTRY(publish_hud_corner)\n__rc0 = 2; INCLUDE imag-regs.ld\n"
    linker += "SECTIONS { . = 0x3000; .text : { *(.phase.pixels.hud_publish) *(.phase.pixels.reticle_hide) *(.text.hud_apply_scroll) *(.text.reticle*) *(.text.irq_entry) *(.text.__havoc_copy_bytes) *(.text.mock_irq_body) *(.text.hud_positions) } /DISCARD/ : { *(.comment) *(.init.*) *(.text.*) } }\n"
    linker += f"HUD_IMAGE_WRITES = {WRITES}; HUD_COMMITS = {COMMITS}; HUD_SCROLL = {SCROLL};\n"
    linker += "".join(f"{name} = {address};\n" for name, address in STATE.items())
    linker += "__irq_stack = 0x7600;\n"
    (BUILD / "kernel.ld").write_text(linker)
    source = ROOT / "target-c64/asm/hud.S"
    wrapper = ROOT / "target-c64/asm/platform.S"
    # The real wrapper is tested with a tiny full-handler stand-in. It reads
    # CIA ICR once and honors only the explicitly passed frame flag, matching
    # irq_body's contract without executing the Rust audio engine here.
    (BUILD / "mock.S").write_text(""".section .text.mock_irq_body,"ax",@progbits
.globl irq_body
irq_body:
    pha
    lda $dc0d
    and #1
    beq mock_audio_end
    inc TEST_CIA_TICKS
.globl mock_audio_end
mock_audio_end:
    pla
    beq mock_done
    inc TEST_FRAMES
    lda HUD_SCROLL
    jsr hud_apply_scroll
    jsr reticle_restore
mock_done:
    rts
""")
    subprocess.run([SDK / "mos-sim-clang", "-c", source, "-o", BUILD / "kernel.o"], check=True)
    subprocess.run([SDK / "mos-sim-clang", "-c", wrapper, "-o", BUILD / "wrapper.o"], check=True)
    subprocess.run([SDK / "mos-sim-clang", "-c", BUILD / "mock.S", "-o", BUILD / "mock.o"], check=True)
    subprocess.run([SDK / "ld.lld", "-T", BUILD / "kernel.ld", "-L", SDK.parent / "mos-platform/common/lib",
                    BUILD / "kernel.o", BUILD / "wrapper.o", BUILD / "mock.o", "-o", BUILD / "kernel.elf"], check=True)
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
            start, end = (52, 92) if corner < 2 else (212, 254)
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
            expected[0xd000 + sprite * 2] = base_x - (packed & 7)
            expected[0xd001 + sprite * 2] = (62 if sprite < 4 else 225) - ((packed >> 3) - 3)
        assert all(machine.mem[address] == value for address, value in expected.items())
        allowed = set(expected) | {4, 5} | set(range(0x100, 0x200))
        assert all(address in allowed or old == new for address, (old, new) in enumerate(zip(before, machine.mem)))
        assert machine.s == 0xfd
        scroll_cases += 1
    mux_cases, irq_cases, max_arm = test_multiplexer(code, symbols)
    report = dict(passed=True, publication_cases=cases, scroll_cases=scroll_cases,
                  multiplex_cases=mux_cases, irq_cases=irq_cases,
                  max_arm_pal_lines=max_arm / (63 * 64),
                  rollover_retries=rollover_retries, max_mask_cycles=max_mask,
                  max_mask_pal_lines=max_mask / (63 * 64),
                  source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                  irq_source_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest())
    (BUILD / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS {cases} HUD publication cases, {scroll_cases} scroll samples, {mux_cases} multiplex cases, {irq_cases} IRQ cases; masked bound {report['max_mask_pal_lines']:.3f} PAL lines (<2), arm {report['max_arm_pal_lines']:.3f} PAL lines")


if __name__ == "__main__":
    main()
