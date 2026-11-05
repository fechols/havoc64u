#!/usr/bin/env python3
"""Run the linked raster control sampler against a CIA/POTX register model.

The timer values are supplied in Phi2 cycles independently of CPU instruction
cycles, including turbo-like no-tick reads and timer wrap. This verifies the
production instructions and bounded work, not physical controller wiring.
"""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path

from native_cpu import Cpu, I, KernelError, load_elf, RESIDENT_SECTIONS, SENTINEL

ROOT = Path(__file__).resolve().parents[1]
KEYS = {"w": (1, 1), "a": (1, 2), "d": (2, 2), "y": (3, 1), "z": (1, 4), "r": (2, 1),
        "ctrl": (7, 2), "cbm": (7, 5), "del": (0, 0),
        "space": (7, 4), "f": (2, 5), "s": (1, 5),
        "shift": (1, 7), "x": (2, 7)}


def require(condition, message):
    if not condition:
        raise KernelError(message)


class ControlsCpu(Cpu):
    def __init__(self, elf, symbols):
        memory = bytearray(65536)
        sections = {section.name: section for section in elf.sections}
        for name in RESIDENT_SECTIONS:
            section = sections.get(name)
            if section is not None:
                memory[section.address:section.address + section.size] = elf.contents(section)
        self.since = symbols["controls_pot_since"].address
        self.held = symbols["controls_pot_held"].address
        self.scratch = symbols["controls_tmp"].address
        super().__init__(memory, [(0x100, 0x200), (self.since, self.since + 2),
                                  (self.held, self.held + 1), (self.scratch, self.scratch + 1),
                                  (0xdc00, 0xdc01), (0xdc02, 0xdc03)])
        self.entries = {name: symbols[name].address
                        for name in ("read_controls", "restore_controls")}
        self.mem[0xdc00], self.mem[0xdc02] = 0x80, 0xc0
        self.timer, self.pot = 0x8000, 255
        self.joy1 = self.joy2 = 0
        self.keys = frozenset()
        self.timer_script = deque()
        self.pot_reads = 0
        self.calls = self.maximum_cycles = 0

    def matrix(self):
        rows = ((~self.mem[0xdc00] & self.mem[0xdc02]) | self.joy2) & 255
        columns = self.joy1
        while True:
            before = rows, columns
            for key in self.keys:
                row, column = KEYS[key]
                if rows & (1 << row):
                    columns |= 1 << column
                if columns & (1 << column):
                    rows |= 1 << row
            if before == (rows, columns):
                return rows, columns

    def read(self, address):
        if address == 0xdc00:
            require(self.mem[0xdc02] == 0xc0 and self.mem[0xdc00] == 0x80,
                    "digital joystick sampled with wrong port direction/selection")
            # Only real joystick pins are modeled for this read. The complete
            # matrix is modeled below where the selected row is read.
            return ((self.joy2 ^ 255) & 0x3f) | 0x80
        if address == 0xdc01:
            require(self.mem[0xdc02] == 255 and self.mem[0xdc00] == 0xfd,
                    "Shift read without row 1 selected")
            return self.matrix()[1] ^ 255
        if address in (0xdc06, 0xdc07):
            if self.timer_script:
                expected, value = self.timer_script.popleft()
                require(address == expected, "unexpected coherent timer read order")
                return value
            return self.timer >> (8 * (address - 0xdc06)) & 255
        if address == 0xd419:
            require(self.mem[0xdc00] == 0x80 and self.mem[0xdc02] == 0xc0,
                    "POTX read before paddle port 2 restored")
            self.pot_reads += 1
            return self.pot
        return super().read(address)

    def write(self, address, value):
        require(self.p & I, "control sampler enabled interrupts")
        super().write(address, value)

    def call(self, name):
        require(self.s == 0xfd and self.p & I, "invalid sampler entry state")
        self.push((SENTINEL - 1) >> 8)
        self.push((SENTINEL - 1) & 255)
        self.pc = self.entries[name]
        start = self.cycles
        limit = 600 if name == "read_devices" else 200
        for _ in range(limit):
            if self.pc == SENTINEL:
                break
            self.step()
        else:
            raise KernelError(f"control sampler did not return within {limit} instructions")
        require(self.s == 0xfd and self.p & I, "sampler changed stack/interrupt state")
        require((self.mem[0xdc00], self.mem[0xdc02], self.mem[0xdc03]) == (0x80, 0xc0, 0),
                "sampler did not restore joystick inputs/paddle port 2")
        require(int.from_bytes(self.mem[self.since:self.since + 2], "little") == self.timer,
                "sampler did not timestamp the final selection")
        self.calls += 1
        self.maximum_cycles = max(self.maximum_cycles, self.cycles - start)
        return self.a

    def sample(self, *, elapsed=2048, timer=0x8000, pot=255, held=0,
               joy1=0, joy2=0, keys=(), expected=0, read=True):
        self.timer, self.pot = timer, pot
        self.mem[self.since:self.since + 2] = ((timer + elapsed) & 65535).to_bytes(2, "little")
        self.mem[self.held] = held
        self.joy1, self.joy2, self.keys = joy1, joy2, frozenset(keys)
        before = self.pot_reads
        actual = self.call("read_controls")
        require(actual == expected, f"input mismatch: actual ${actual:02x}, expected ${expected:02x}")
        require(self.pot_reads - before == int(read), "POTX read during settling or not read when ready")


class DevicesCpu(ControlsCpu):
    """read_devices: port 2, port 1 (idle columns) and the masked keyboard."""
    # (key, row, column bit, logical bit) in read_devices' order.
    GAME_KEYS = (("w", 1, 0x02, 0x01), ("s", 1, 0x20, 0x02), ("a", 1, 0x04, 0x04),
                 ("shift", 1, 0x80, 0x20), ("d", 2, 0x04, 0x08), ("space", 7, 0x10, 0x10))

    def __init__(self, elf, symbols):
        super().__init__(elf, symbols)
        self.devices = symbols["device_bits"].address
        self.writable.append((self.devices, self.devices + 7))
        self.entries["read_devices"] = symbols["read_devices"].address

    def read(self, address):
        if address == 0xdc01 and self.mem[0xdc02] == 255:
            return self.matrix()[1] ^ 255
        return super().read(address)

    def columns(self, row_select):
        saved = self.mem[0xdc00], self.mem[0xdc02]
        self.mem[0xdc00], self.mem[0xdc02] = row_select, 255
        columns = self.matrix()[1]
        self.mem[0xdc00], self.mem[0xdc02] = saved
        return columns

    def reference(self, previous):
        grounded = self.joy2 & 0x1f
        idle = self.columns(0xff)
        rows = {1: self.columns(0xfd), 2: self.columns(0xfb), 7: self.columns(0x7f)}
        keys = 0
        for _, row, column, bit in self.GAME_KEYS:
            row_bit = 1 << row
            if grounded & row_bit:
                if grounded == row_bit:
                    keys |= bit if idle & column else 0
                else:
                    keys |= previous & bit
            elif idle & column:
                keys |= previous & bit
            else:
                keys |= bit if rows[row] & column else 0
        return idle & 0x1f, keys

    def devices_sample(self, joy1=0, joy2=0, keys=(), pot=255, previous=0):
        self.timer, self.pot = 0x8000, pot
        self.mem[self.since:self.since + 2] = ((0x8000 + 2048) & 65535).to_bytes(2, "little")
        self.mem[self.held] = 0
        self.joy1, self.joy2, self.keys = joy1, joy2, frozenset(keys)
        self.mem[self.devices + 2] = previous
        self.call("read_devices")
        port2, port1, board = self.mem[self.devices:self.devices + 3]
        want_port1, want_board = self.reference(previous)
        require(port2 == (joy2 & 0x1f) | (32 if pot < 128 else 0),
                f"port 2 ${port2:02x} for joy2 ${joy2:02x}: Left Shift must not boost it")
        require(port1 == want_port1, f"port 1 ${port1:02x}, expected ${want_port1:02x}")
        require(board == want_board,
                f"keyboard ${board:02x}, expected ${want_board:02x} (keys {sorted(keys)}, "
                f"joy1 ${joy1:02x}, joy2 ${joy2:02x}, previous ${previous:02x})")
        return port1, board


def exercise_devices(elf, symbols):
    cpu = DevicesCpu(elf, symbols)
    game = [key for key, *_ in DevicesCpu.GAME_KEYS]
    truth_bits = {key: bit for key, _, _, bit in DevicesCpu.GAME_KEYS}
    checks = 0
    # Without joysticks every combination of game keys reads exactly.
    for mask in range(64):
        keys = [key for n, key in enumerate(game) if mask & (1 << n)]
        for previous in (0, 0x3f):
            _, board = cpu.devices_sample(keys=keys, previous=previous)
            require(board == sum(truth_bits[k] for k in keys), f"unmasked keys {keys} misread")
            checks += 1
    # With joysticks: the freeze rule, and no false key from a joystick alone.
    for joy1 in (0, 1, 2, 4, 8, 16, 31):
        for joy2 in (0, 1, 2, 4, 8, 16, 6, 31):
            for keys in ((), *((k,) for k in game), ("w", "d"), ("a", "space"), ("s", "shift"),
                         ("z",), ("y", "r")):
                for previous in (0, 0x3f):
                    port1, board = cpu.devices_sample(joy1=joy1, joy2=joy2, keys=keys,
                                                      previous=previous)
                    checks += 1
                    if not keys:
                        require(port1 == joy1, "an idle keyboard changed port 1")
                    # With both joysticks held, port 1 can fake a key in the one
                    # row port 2 grounds; the keyboard cannot hold a seat then.
                    if len(keys) <= 1 and previous == 0 and not (joy1 and joy2):
                        # A single key cannot ghost: a joystick can freeze keys
                        # (which then stay released here) but never fake one.
                        truth = sum(truth_bits.get(k, 0) for k in keys)
                        require(board & ~truth == 0,
                                f"phantom key with {keys}, joysticks {joy1}/{joy2}")
    # POTX second fire still reaches port 2 only.
    cpu.devices_sample(pot=0, keys=("shift",))
    require(cpu.maximum_cycles < 900, "device sampling exceeded its CPU-cycle budget")
    return checks


def exercise(elf, symbols):
    cpu = ControlsCpu(elf, symbols)
    for initial_latch in (0, 0xfd, 0xfe, 255):
        cpu.mem[0xdc00], cpu.mem[0xdc02] = initial_latch, 255
        cpu.call("restore_controls")
    for digital in range(32):
        for pot in (0, 1, 127, 128, 254, 255):
            for shift in (False, True):
                boost = pot < 128 or shift
                cpu.sample(pot=pot, joy2=digital, keys=("shift",) if shift else (),
                           expected=digital | (32 if boost else 0))
    for timer in (0, 1, 255, 256, 0x8000, 0xfffe, 0xffff):
        for elapsed in (0, 1, 1023, 1024, 1025, 16384, 20000, 65535):
            for old, pot in ((0, 0), (32, 255)):
                ready = elapsed >= 1024
                cpu.sample(timer=timer, elapsed=elapsed, pot=pot, held=old,
                           expected=(32 if pot < 128 else 0) if ready else old, read=ready)
    cpu.sample(elapsed=0, pot=255, keys=("shift",), expected=32, read=False)

    ordinary = ("ctrl", "cbm", "del", "space", "f", "s")
    for joy1, joy2 in ((0, 0), (31, 31), (31, 0), (0, 31),
                       *((0, 1 << n) for n in range(5))):
        for mask in range(64):
            keys = tuple(key for n, key in enumerate(ordinary) if mask & (1 << n))
            cpu.sample(joy1=joy1, joy2=joy2, keys=keys, expected=joy2)
            cpu.sample(joy1=joy1, joy2=joy2, keys=keys + ("shift",), expected=joy2 | 32)

    # Overlapping physical sources are one held button; releasing either one
    # cannot create a release/repress while the other remains held.
    for pot, keys, expected in ((255, (), 0), (0, (), 32), (0, ("shift",), 32),
                                (255, ("shift",), 32), (255, (), 0)):
        cpu.sample(pot=pot, keys=keys, expected=expected)

    # A high-byte change retries the timer read rather than using torn time.
    cpu.timer_script.extend(((0xdc07, 0x12), (0xdc06, 0xff), (0xdc07, 0x11)))
    cpu.sample(timer=0x11f0, pot=0, expected=32)
    require(not cpu.timer_script, "timer rollover fixture was not consumed")
    require(cpu.maximum_cycles < 350, "input polling exceeded its CPU-cycle budget")
    device_checks = exercise_devices(elf, symbols)
    return {"calls": cpu.calls, "maximum_cpu_cycles": cpu.maximum_cycles,
            "device_checks": device_checks,
            "timer_units": "unaccelerated Phi2 cycles, independent of instruction cycles",
            "scenarios": ["all digital controls and POTX threshold endpoints",
                          "1024-cycle settling boundary and 16-bit timer wrap",
                          "held state retained during invalid POTX sampling",
                          "Left Shift during settling and simultaneous game shortcuts",
                          "two boost sources merge into one held button",
                          "coherent timer read retry and bounded IRQ work",
                          "read_devices: port 1 idle columns, exact unmasked WASD/Space/Shift, "
                          "the freeze rule under every joystick combination, no phantom keys"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", type=Path, default=ROOT / "dist/havoc.elf")
    parser.add_argument("--report", type=Path, default=ROOT / "target/input-kernel/report.json")
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    report = {"elf_sha256": hashlib.sha256(args.elf.read_bytes()).hexdigest(),
              "scope": "Linked input helpers; modeled CIA matrix/POTX/timer, not physical wiring",
              "passed": False}
    try:
        report.update(exercise(elf, symbols))
        report["passed"] = True
    finally:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(f"native input integration passed: {report['calls']} calls; "
          f"maximum {report['maximum_cpu_cycles']} CPU cycles; {args.report}")


if __name__ == "__main__":
    main()
