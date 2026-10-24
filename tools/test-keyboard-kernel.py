#!/usr/bin/env python3
"""Execute the linked PIXELS keyboard scanner against a bounded CIA matrix.

Uses the production 6502 instructions and ELF/DWARF field locations. The model
checks digital keyboard/joystick interference, IRQ exclusion and memory writes;
it does not model contact bounce or claim physical keyboard timing. No emulator
boot or Ultimate Audio sampler is needed. Run after scripts/build-target.sh.
"""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import runpy

ROOT = Path(__file__).resolve().parents[1]
KERNEL = runpy.run_path(str(ROOT / "tools/test-music-kernel.py"))
SMOKE = runpy.run_path(str(ROOT / "tools/vice-smoke.py"))
Cpu, KernelError, load_elf = (KERNEL[name] for name in ("Cpu", "KernelError", "load_elf"))
IRQ_MASK, UNUSED, SENTINEL = 4, 32, 0xffff
SELECTOR = "menu_music_galactic"
STORY = "story_mode"
# Standard C64 matrix: CTRL and C= share row 7; DEL is row 0, column 0.
KEYS = {"ctrl": (7, 2), "cbm": (7, 5), "del": (0, 0),
        "space": (7, 4), "f": (2, 5), "d": (2, 2), "five": (2, 0),
        "s": (1, 5), "shift": (1, 7)}
CHORD_KEYS = ("ctrl", "cbm", "del")
CHORD = frozenset(CHORD_KEYS)


def require(condition, message):
    if not condition:
        raise KernelError(message)


class Machine(Cpu):
    def __init__(self, elf, symbols, layout, budget):
        sections = {section.name: section for section in elf.sections}
        memory = bytearray(65536)
        for name in (*KERNEL["RESIDENT_SECTIONS"], ".phase.pixels"):
            section = sections[name]
            memory[section.address:section.address + section.size] = elf.contents(section)
        self.raw_symbols = {symbol.name: symbol.address for symbol in elf.symbols}
        self.addresses = {name: SMOKE["resident_symbol"](self.raw_symbols, name)[1]
                          for name in ("KEYS_HELD", "PAUSED", "FPS_SHOWN")}
        self.entry = symbols["havoc_c64::update_fps_overlay"].address
        self.layout = layout
        self.game_start = layout["address"]
        self.game_end = symbols["__game_state_end"].address
        self.selector = self.game_start + layout["fields"][SELECTOR]["offset"]
        self.story = self.game_start + layout["fields"][STORY]["offset"]
        self.activity = self.game_start + layout["fields"]["menu_activity"]["offset"]
        self.mode_address = self.game_start + layout["fields"]["mode"]["offset"]
        self.stack_bottom = symbols["__stack_bottom"].address
        self.stack_top = symbols["__stack"].address
        writable = [(0, 0x200), (self.stack_bottom, self.stack_top),
                    (self.selector, self.selector + 1), (self.story, self.story + 1),
                    (self.activity, self.activity + 1), (0xdc00, 0xdc01), (0xdc02, 0xdc03)]
        for name in (".data", ".bss", ".noinit"):
            section = sections[name]
            writable.append((section.address, section.address + section.size))
        super().__init__(memory, writable)
        self.mem[2:4] = self.stack_top.to_bytes(2, "little")
        self.mem[0xdc00] = 255
        self.mem[0xdc02] = self.mem[0xdc03] = 0
        # Only mode and the menu selection/activity fields are used here. Their
        # offsets and valid discriminants come from this exact native build.
        self.mem[self.game_start:self.game_end] = bytes(self.game_end - self.game_start)
        self.p = UNUSED  # The caller enters with interrupts enabled.
        self.keys = frozenset()
        self.joy1 = self.joy2 = 0
        self.budget = budget
        self.calls = self.max_cycles = self.max_steps = 0
        self.scan_reads = []

    def read(self, address):
        if address != 0xdc01:
            return super().read(address)
        require(self.p & IRQ_MASK, "keyboard read with IRQ sampling enabled")
        require(self.mem[0xdc02] == 255 and self.mem[0xdc03] == 0,
                "keyboard scan requires port A output and port B input")
        # Joystick 2 grounds rows; joystick 1 grounds columns. Closed keys
        # connect both directions, including multi-key matrix ghost paths.
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
                break
        self.scan_reads.append(self.mem[0xdc00])
        return columns ^ 255

    def write(self, address, value):
        if address in (0xdc00, 0xdc02):
            require(self.p & IRQ_MASK, "keyboard port changed with IRQ sampling enabled")
        super().write(address, value)

    def flag(self, name):
        return self.mem[self.addresses[name]]

    def scan(self, keys=(), mode="Title", expected=False, joy1=0, joy2=0,
             pause=False, fps=False, story=False, label="scan"):
        self.keys = frozenset(keys)
        self.joy1, self.joy2 = joy1, joy2
        self.mem[self.mode_address] = self.layout["modes"][mode]
        before = bytes(self.mem[self.game_start:self.game_end])
        paused, shown = self.flag("PAUSED"), self.flag("FPS_SHOWN")
        stack = bytes(self.mem[2:4])
        require(self.s == 0xfd and not self.p & IRQ_MASK, f"{label}: invalid call state")
        self.scan_reads = []
        self.push((SENTINEL - 1) >> 8)
        self.push((SENTINEL - 1) & 255)
        self.pc = self.entry
        self.a = self.layout["modes"][mode]  # MOS ABI: the first u8 argument is A.
        start = self.cycles
        for steps in range(self.budget):
            if self.pc == SENTINEL:
                break
            self.step()
        else:
            raise KernelError(f"{label}: scanner exceeded {self.budget} instructions at ${self.pc:04x}")
        require(self.s == 0xfd and self.mem[2:4] == stack, f"{label}: stack imbalance")
        require(not self.p & IRQ_MASK, f"{label}: IRQs left disabled")
        require(self.mem[0xdc02] == 0 and self.mem[0xdc03] == 0,
                f"{label}: joystick ports not restored to input")
        require(255 in self.scan_reads, f"{label}: missing idle-column baseline")
        after = bytes(self.mem[self.game_start:self.game_end])
        offset = self.selector - self.game_start
        want = bytearray(before)
        want[offset] ^= int(expected)
        want[self.story - self.game_start] ^= int(story)
        if story:
            want[self.activity - self.game_start] = 1
        require(after == want, f"{label}: wrong menu toggle/activity or unrelated Game mutation")
        require(self.flag("PAUSED") == paused ^ int(pause), f"{label}: wrong pause edge")
        require(self.flag("FPS_SHOWN") == shown ^ int(fps), f"{label}: wrong FPS edge")
        self.calls += 1
        self.max_cycles = max(self.max_cycles, self.cycles - start)
        self.max_steps = max(self.max_steps, steps)


def exercise(elf, symbols, layout, budget):
    machines = []
    scenarios = []

    def fresh():
        cpu = Machine(elf, symbols, layout, budget)
        machines.append(cpu)
        return cpu

    for mode in ("Title", "Attract"):
        for mask in range(7):
            cpu = fresh()
            keys = [key for bit, key in enumerate(CHORD_KEYS) if mask & (1 << bit)]
            cpu.scan(keys, mode, label=f"{mode}: incomplete chord {mask}")
        for order in itertools.permutations(CHORD_KEYS):
            cpu = fresh()
            down = set()
            for key in order:
                down.add(key)
                cpu.scan(down, mode, expected=down == CHORD, label=f"{mode}: order {order}")
            for _ in range(3):
                cpu.scan(CHORD, mode, label=f"{mode}: held chord")
            # Releasing any member rearms the complete chord, in either song.
            for key in CHORD_KEYS:
                cpu.scan(CHORD - {key}, mode, label=f"{mode}: release {key}")
                cpu.scan(CHORD, mode, expected=True, label=f"{mode}: repress {key}")
    scenarios.append("partial chords, all press orders, held chord and individual-key rearming in Title/Attract")

    for mode in layout["modes"]:
        for selected in (0, 1):
            cpu = fresh()
            cpu.mem[cpu.selector] = selected
            cpu.scan(CHORD, mode, expected=mode in ("Title", "Attract"), label=f"mode {mode}, song {selected}")
            cpu.scan(CHORD, "Title", label=f"{mode}: held into Title")
            cpu.scan((), "Title")
            cpu.scan(CHORD, "Title", expected=True, label=f"{mode}: fresh title press")
    cpu = fresh()
    cpu.scan(CHORD, "Title", expected=True)
    cpu.scan(CHORD, "Attract", label="held across title/attract cut")
    cpu.scan((), "Attract")
    cpu.scan(CHORD, "Attract", expected=True)
    scenarios.append("all mode guards and no synthetic edge while held across scene changes")

    # Every individual joystick line, and all lines together, must be unable
    # to supply a missing chord key. Use both ports and both together.
    for joy in (1, 2, 4, 8, 16, 31):
        for joy1, joy2 in ((joy, 0), (0, joy), (joy, joy)):
            for missing in (*CHORD_KEYS, None):
                cpu = fresh()
                keys = CHORD - {missing} if missing else ()
                cpu.scan(keys, joy1=joy1, joy2=joy2,
                         label=f"joystick {joy1}/{joy2}, missing {missing}")
    # Port 1 UP/LEFT share DEL/CTRL columns. Port 2 UP selects DEL's row;
    # these genuine chords are suppressed by the existing idle filter.
    for joy1, joy2 in ((1, 0), (4, 0), (0, 1)):
        fresh().scan(CHORD, joy1=joy1, joy2=joy2, label="real chord with contaminated baseline")
    fresh().scan(CHORD, joy2=16, expected=True, label="real chord with unrelated port 2 FIRE")
    # A joystick-selected row with a different key cannot impersonate CTRL,
    # C= or DEL; it already pulls that column low during the idle read.
    for missing, substitute in (("ctrl", "d"), ("cbm", "f"), ("del", "five")):
        fresh().scan((CHORD - {missing}) | {substitute}, joy2=4,
                     label=f"joystick-selected {substitute} cannot fake {missing}")
    scenarios.append("both joystick ports, partial chords, idle filtering and cross-row impostors")

    # Idle-column qualification can disappear while the physical chord stays
    # down. Clearing a joystick must not manufacture another keyboard edge.
    contaminants = ((1, 0), (4, 0), (5, 0), (0, 1),
                    (1, 1), (4, 1), (5, 1), (31, 31))
    for mode in ("Title", "Attract"):
        for joy1, joy2 in contaminants:
            label = f"{mode}: held chord, joystick {joy1}/{joy2}"
            cpu = fresh()
            cpu.scan(CHORD, mode, expected=True, label=label + " initial edge")
            cpu.scan(CHORD, mode, joy1=joy1, joy2=joy2, label=label + " contamination")
            cpu.scan(CHORD, mode, joy1=joy1, joy2=joy2, label=label + " held contamination")
            cpu.scan(CHORD, mode, label=label + " joystick release")
            cpu.scan(CHORD, mode, label=label + " still held")
            cpu.scan(CHORD - {"del"}, mode, label=label + " physical DEL release")
            cpu.scan(CHORD, mode, expected=True, label=label + " physical DEL repress")

            cpu = fresh()
            cpu.scan(CHORD, mode, joy1=joy1, joy2=joy2, label=label + " initially contaminated")
            cpu.scan(CHORD, mode, label=label + " initially contaminated joystick release")
            cpu.scan(CHORD, mode, label=label + " initially contaminated still held")
            cpu.scan((), mode, label=label + " physical release")
            cpu.scan(CHORD, mode, expected=True, label=label + " new physical press")
    scenarios.append("joystick interference cannot rearm a held chord or trigger an initially contaminated chord")

    for mode in layout["modes"]:
        cpu = fresh()
        pause = mode in ("Playing", "Transition")
        cpu.scan(("space",), mode, pause=pause, label=f"{mode}: SPACE")
        cpu.scan(("space",), mode, label=f"{mode}: held SPACE")
        cpu.scan((), mode)
        cpu.scan(("space",), mode, pause=pause, label=f"{mode}: fresh SPACE")
        cpu.scan(("f",), mode, fps=True, label=f"{mode}: F")
        cpu.scan(("f",), mode, label=f"{mode}: held F")
        cpu.scan((), mode)
        cpu.scan(("f",), mode, fps=True, label=f"{mode}: fresh F")
    cpu = fresh()
    cpu.scan(CHORD | {"f"}, expected=True, fps=True, label="chord plus F")
    cpu.scan(CHORD | {"f"}, label="held chord plus F")
    cpu.scan(())
    cpu.scan(CHORD | {"space"}, expected=True, label="title chord plus SPACE")
    scenarios.append("existing SPACE/F edge behavior and simultaneous shortcuts")

    for mode in layout["modes"]:
        for selected in (0, 1):
            cpu = fresh()
            cpu.mem[cpu.story] = selected
            cpu.scan(("s",), mode, story=mode == "Title", label=f"{mode}: S, selected {selected}")
            # SIM consumes activity; holding S must not continually defer attract.
            cpu.mem[cpu.activity] = 0
            cpu.scan(("s",), mode, label=f"{mode}: held S")
            cpu.scan(("s",), "Title", label=f"{mode}: held into Title")
            cpu.scan((), "Title")
            cpu.scan(("s",), "Title", story=True, label=f"{mode}: fresh Title S")
    fresh().scan(("s", "shift"), story=True, label="shifted S selects story")
    scenarios.append("Title-only S toggles both ways, activity once, held-key and mode-change guards")

    for joy in (1, 2, 4, 8, 16, 31):
        for joy1, joy2 in ((joy, 0), (0, joy), (joy, joy)):
            fresh().scan((), joy1=joy1, joy2=joy2, label=f"joystick {joy1}/{joy2} cannot fake S")
            # S shares column 5 with F and C=. Joystick-selected rows must not
            # let either real key impersonate S through the matrix.
            for substitute, row in (("f", 4), ("cbm", 128)):
                fresh().scan((substitute,), joy1=joy1, joy2=joy2 | row,
                             label=f"joystick-selected {substitute} cannot fake S")
    for joy1, joy2 in ((0, 2), (31, 31)):
        cpu = fresh()
        cpu.scan(("s",), story=True)
        cpu.scan(("s",), joy1=joy1, joy2=joy2, label="S held with contaminated idle")
        cpu.scan(("s",), label="joystick release does not rearm S")
        cpu.scan(())
        cpu.scan(("s",), story=True)
        cpu = fresh()
        cpu.scan(("s",), joy1=joy1, joy2=joy2, label="S initially contaminated")
        cpu.scan(("s",), label="initial contamination clears without an edge")
        cpu.scan(())
        cpu.scan(("s",), story=True)
    fresh().scan(("s",), joy2=16, story=True, label="S and joystick FIRE may coexist")
    cpu = fresh()
    cpu.scan(CHORD | {"s", "f", "space"}, expected=True, fps=True, story=True,
             label="S with existing shortcuts on Title")
    cpu.scan(CHORD | {"s", "f", "space"}, label="held simultaneous shortcuts")
    scenarios.append("S idle filtering, both joystick ports, cross-row impostors and simultaneous shortcuts")

    return {"scenarios": scenarios, "fixtures": len(machines),
            "scanner_calls": sum(cpu.calls for cpu in machines),
            "maximum_cpu_cycles_per_call": max(cpu.max_cycles for cpu in machines),
            "maximum_instructions_per_call": max(cpu.max_steps for cpu in machines),
            "invariants": ["IRQ exclusion during CIA scan", "CIA ports restored to input",
                           "balanced native stacks", "only menu selectors/activity may change in Game"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", type=Path, default=ROOT / "dist/havoc.elf")
    parser.add_argument("--llvm-dwarfdump", type=Path,
                        default=Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin/llvm-dwarfdump")
    parser.add_argument("--budget", type=int, default=100_000)
    parser.add_argument("--report", type=Path, default=ROOT / "target/keyboard-kernel/report.json")
    args = parser.parse_args()
    require(args.budget > 0, "instruction budget must be positive")
    args.report.parent.mkdir(parents=True, exist_ok=True)
    elf, symbols = load_elf(args.elf)
    raw_symbols = {symbol.name: symbol.address for symbol in elf.symbols}
    layout = SMOKE["dwarf_game_layout"](args.elf, args.llvm_dwarfdump, raw_symbols, args.report.parent)
    for name in (SELECTOR, STORY, "menu_activity"):
        field = layout["fields"].get(name)
        require(field is not None and field["type"] == "bool", f"missing native Game::{name} bool")
        require(layout["address"] + field["offset"] < symbols["__game_state_end"].address,
                f"menu field {name} exceeds native Game storage")
    report = {"elf_sha256": hashlib.sha256(args.elf.read_bytes()).hexdigest(),
              "scope": "Linked PIXELS scanner; digital CIA matrix and joystick model, not physical keyboard timing",
              "layout": layout, "passed": False}
    try:
        report.update(exercise(elf, symbols, layout, args.budget))
        report["passed"] = True
    except (KernelError, RuntimeError) as error:
        report["error"] = str(error)
        raise
    finally:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(f"native keyboard integration passed: {report['scanner_calls']} scans, "
          f"{report['fixtures']} fixtures; {args.report}")


if __name__ == "__main__":
    main()
