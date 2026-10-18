"""Audit phase overlays and four alternative tails of a common SIM prefix.

Unlike a disassembly's numeric call addresses, ELF relocation symbols retain
the destination section even when all six phases execute at $3000. This is a
static reference audit; runtime-computed pointers still require hardware tests.
Only Python's standard library is needed. Link with --emit-relocs and do not
strip the ELF before running this checker.
"""

from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
import re
import struct
import sys


class AuditError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise AuditError(message)


def string_at(data, offset):
    require(0 <= offset < len(data), "ELF string offset outside its table")
    end = data.find(b"\0", offset)
    require(end >= 0, "unterminated ELF string")
    return data[offset:end].decode("utf-8", errors="replace")


def readable_name(name):
    """Decode the simple Rust legacy names used for the entry functions."""
    if not (name.startswith("_ZN") and name.endswith("E")):
        return name
    remaining = name[3:-1]
    parts = []
    while remaining:
        length = re.match(r"\d+", remaining)
        if length is None:
            return name
        count = int(length.group())
        remaining = remaining[len(length.group()):]
        if len(remaining) < count:
            return name
        parts.append(remaining[:count])
        remaining = remaining[count:]
    if parts and re.fullmatch(r"h[0-9a-f]{16}", parts[-1]):
        parts.pop()
    return "::".join(parts)


@dataclass
class Section:
    index: int
    name: str
    kind: int
    flags: int
    address: int
    offset: int
    size: int
    link: int
    info: int
    entry_size: int

    @property
    def executable(self):
        return bool(self.flags & 4)


@dataclass
class Symbol:
    name: str
    address: int
    size: int
    kind: int
    binding: int
    section: int


@dataclass(frozen=True)
class Function:
    section: int
    address: int
    end: int
    name: str


class Elf:
    def __init__(self, path):
        self.data = path.read_bytes()
        require(self.data[:6] == b"\x7fELF\x01\x01", "expected ELF32 little-endian input")
        require(len(self.data) >= 52, "truncated ELF header")
        offset = struct.unpack_from("<I", self.data, 32)[0]
        stride, count, names_index = struct.unpack_from("<HHH", self.data, 46)
        require(stride == 40 and count > 0 and names_index < count, "unsupported section table")
        require(offset + count * stride <= len(self.data), "truncated ELF section table")
        headers = [struct.unpack_from("<10I", self.data, offset + i * stride) for i in range(count)]
        names_header = headers[names_index]
        names = self.data[names_header[4]:names_header[4] + names_header[5]]
        self.sections = []
        for i, h in enumerate(headers):
            self.sections.append(Section(i, string_at(names, h[0]), h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[9]))
        self.symbol_tables = {}
        self.symbols = []
        for section in self.sections:
            if section.kind != 2:  # SHT_SYMTAB
                continue
            require(section.entry_size == 16, "unexpected ELF symbol size")
            strings = self.contents(self.sections[section.link])
            symbols = []
            for name, address, size, info, _other, owner in struct.iter_unpack("<IIIBBH", self.contents(section)):
                symbol = Symbol(string_at(strings, name), address, size, info & 15, info >> 4, owner)
                symbols.append(symbol)
            self.symbol_tables[section.index] = symbols
            self.symbols.extend(symbols)
        require(self.symbols, "ELF symbol table is missing; preserve the unstripped ELF")
        self.functions = defaultdict(list)
        by_address = {}
        for symbol in self.symbols:
            if symbol.section >= len(self.sections) or not symbol.name:
                continue
            section = self.sections[symbol.section]
            if not section.executable or not section.address <= symbol.address < section.address + section.size:
                continue
            # LLVM-MOS assembly routine labels can be STT_NOTYPE with size 0.
            if symbol.kind != 2 and (symbol.kind != 0 or symbol.name.startswith((".L", "__phase_"))):
                continue
            key = (symbol.section, symbol.address)
            previous = by_address.get(key)
            if previous is None or symbol.kind == 2:
                by_address[key] = symbol
        grouped = defaultdict(list)
        for symbol in by_address.values():
            grouped[symbol.section].append(symbol)
        for index, symbols in grouped.items():
            symbols.sort(key=lambda symbol: symbol.address)
            for i, symbol in enumerate(symbols):
                next_address = symbols[i + 1].address if i + 1 < len(symbols) else self.sections[index].address + self.sections[index].size
                end = symbol.address + symbol.size if symbol.size else next_address
                self.functions[index].append(Function(index, symbol.address, end, readable_name(symbol.name)))

    def contents(self, section):
        require(section.offset + section.size <= len(self.data), f"truncated {section.name}")
        return self.data[section.offset:section.offset + section.size]

    def function_at(self, section, address):
        return next((function for function in self.functions[section] if function.address <= address < function.end), None)

    def relocations(self):
        for section in self.sections:
            if section.kind not in (4, 9):  # SHT_RELA / SHT_REL
                continue
            require(section.kind == 4, "MOS audit requires explicit RELA addends")
            require(section.entry_size == 12, "unexpected ELF relocation size")
            require(section.link in self.symbol_tables, "relocation has no symbol table")
            require(section.info < len(self.sections), "relocation has no source section")
            source = self.sections[section.info]
            symbols = self.symbol_tables[section.link]
            for address, info, addend in struct.iter_unpack("<IIi", self.contents(section)):
                require(info >> 8 < len(symbols), "relocation symbol index outside its table")
                yield source, address, info & 255, symbols[info >> 8], addend


ENTRIES = {
    ".phase.boot": "havoc_c64::boot_init",
    ".phase.sim": "havoc_c64::simulate",
    ".phase.geometry": "havoc_c64::prepare_geometry",
    ".phase.pixels": "havoc_c64::draw_pixels",
    ".phase.audio": "havoc_c64::service_audio",
    ".phase.scenery": "havoc_c64::prepare_scenery",
}
ENEMY_ENTRIES = {
    f".phase.enemy.{level}": f"enemy_{level}"
    for level in ("lowlands", "canyon", "fortress", "carrier")
}
STARTUP_SECTION = ".startup_checks"
STARTUP_ENTRY = "havoc_c64::check_code_package"


def audit_startup(elf):
    """Fail closed if a future SDK adds a ROM hook before main.

    Low BSS deliberately owns the KERNAL vector area. Only register setup,
    BSS clearing, our data relocation, and the final call to main are valid.
    Keep this small instruction whitelist explicit when changing the CRT.
    """
    symbols = {symbol.name: symbol for symbol in elf.symbols if symbol.name}
    names = ("_start", "__after_main", "__zero_bss", "__havoc_copy_data", "main")
    require(all(name in symbols for name in names), "startup audit symbols are missing")
    start, end = symbols["_start"], symbols["__after_main"]
    require(start.section == end.section and start.address < end.address, "invalid startup range")
    section = elf.sections[start.section]
    code = elf.contents(section)[start.address - section.address:end.address - section.address]
    require(code[0] == 0x78, "startup must mask IRQs before any other instruction")
    lengths = {0x78: 1, 0xD8: 1, 0xA2: 2, 0x9A: 1, 0xA9: 2,
               0x8D: 3, 0xAD: 3, 0x85: 2, 0x20: 3}
    offset, previous, roms_disabled = 0, None, False
    calls = []
    while offset < len(code):
        opcode = code[offset]
        require(opcode in lengths, f"unreviewed startup opcode ${opcode:02X} at ${start.address + offset:04X}")
        width = lengths[opcode]
        require(offset + width <= len(code), "truncated startup instruction")
        instruction = code[offset:offset + width]
        if opcode == 0x85 and instruction[1] == 1:
            require(previous == b"\xA9\x35", "startup must keep both ROMs disabled when setting $01")
            roms_disabled = True
        if opcode == 0x20:
            target = int.from_bytes(instruction[1:], "little")
            require(roms_disabled, "startup calls code before disabling ROMs")
            require(target < 0xA000, f"startup calls ROM/I/O at ${target:04X}")
            calls.append(target)
        previous = instruction
        offset += width
    expected = [symbols[name].address for name in ("__zero_bss", "__havoc_copy_data", "main")]
    require(calls == expected, "startup calls must be exactly BSS clear, data copy, main; reject SDK ROM hooks")


def audit(path):
    elf = Elf(path)
    audit_startup(elf)
    phases = {section.name: section for section in elf.sections if section.name.startswith(".phase.")}
    require(set(phases) == set(ENTRIES) | set(ENEMY_ENTRIES), "ELF must contain the six common phases and four enemy tails")
    entry_functions = {}
    for name, expected in {**ENTRIES, **ENEMY_ENTRIES}.items():
        matches = [function for function in elf.functions[phases[name].index] if function.name == expected]
        require(len(matches) == 1, f"missing or ambiguous overlay entry {expected}")
        entry_functions[name] = matches[0]
    symbols = {symbol.name: symbol for symbol in elf.symbols if symbol.name}
    require("enemy_step" in symbols and "__phase_sim_base_end" in symbols, "missing opaque enemy ABI boundary")
    enemy_address = symbols["enemy_step"].address
    require(enemy_address == symbols["__phase_sim_base_end"].address
            == phases[".phase.sim"].address + phases[".phase.sim"].size,
            "enemy ABI must begin immediately after the shared SIM prefix")
    for name in ENEMY_ENTRIES:
        require(phases[name].address == enemy_address == entry_functions[name].address,
                f"{name} must begin with its ABI entry")
        require(enemy_address + phases[name].size <= 0x7200, f"{name} exceeds the execution window")
    partial_phases = []
    for name, caller_name, tail_name in (
        ("scenery", "havoc::scenery::emit", "havoc::scenery::wildlife"),
        ("geometry", "havoc::game::Game::render_foreground", "havoc::game::Game::render_combat"),
    ):
        phase = phases[f".phase.{name}"]
        boundary_name = f"__phase_{name}_base_end"
        require(boundary_name in symbols, f"missing common {name} prefix boundary")
        boundary = symbols[boundary_name].address
        require(phase.address < boundary < phase.address + phase.size,
                f"invalid common {name} prefix boundary")
        require(entry_functions[phase.name].end <= boundary,
                f"{name} entry must be entirely within the common prefix")
        tail = [function for function in elf.functions[phase.index] if function.name == tail_name]
        require(len(tail) == 1 and tail[0].address >= boundary,
                f"{tail_name} must be the mode-dependent {name} tail entry")
        partial_phases.append((phase, boundary, caller_name, tail[0]))
    startup_sections = [section for section in elf.sections if section.name == STARTUP_SECTION]
    require(len(startup_sections) == 1, "missing or ambiguous startup-check section")
    startup_section = startup_sections[0]
    require(startup_section.address == 0x8C00 and 0 < startup_section.size <= 0x500,
            "startup checks must fit the retired $8C00-$90FF workspace")
    startup_matches = [function for function in elf.functions[startup_section.index] if function.name == STARTUP_ENTRY]
    require(len(startup_matches) == 1, "missing or ambiguous startup-check entry")
    startup_entry = startup_matches[0]

    sources = Counter()
    local_references = Counter()
    entry_references = Counter()
    entry_addresses = defaultdict(list)
    graph = defaultdict(set)
    errors = []
    partial_tail_calls = Counter()
    enemy_calls = 0
    relocations = list(elf.relocations())
    # LLVM places switch tables in resident .rodata and drops their local
    # symbol names. Recover their contiguous relocation runs, then require
    # every table targeting an overlay to be read by that same function.
    tables = []
    table_references = []
    for source, address, kind, symbol, addend in relocations:
        if source.executable or not source.flags & 2 or symbol.section >= len(elf.sections):
            continue
        target = elf.sections[symbol.section]
        if not target.executable:
            continue
        callee = elf.function_at(target.index, symbol.address + addend)
        require(callee is not None, f"unresolved code pointer in {source.name}:${address:04X}")
        if kind not in (3, 4, 5):
            # The BASIC SYS header has a decimal startup-address relocation.
            require(target.name not in phases and target.name != STARTUP_SECTION, f"unsupported transient-code table relocation {kind}")
            continue
        table_references.append((source.index, address, 2 if kind == 3 else 1, callee))
    for section, address, width, callee in sorted(table_references, key=lambda item: (item[0], item[1])):
        if tables and tables[-1][0] == section and tables[-1][2] == address and tables[-1][3] == callee:
            tables[-1][2] += width
        else:
            tables.append([section, address, address + width, callee, set()])
    for source, address, kind, symbol, addend in relocations:
        if not source.flags & 2:  # Ignore debug metadata; it is not loaded.
            continue
        sources[source.name] += 1
        if symbol.name == "enemy_step":
            caller = elf.function_at(source.index, address) if source.executable else None
            instruction = address - source.address - 1
            require(source.name == ".phase.sim" and caller is not None
                    and caller.name == "havoc::game::Game::update_actors"
                    and addend == 0 and kind == 3 and 0 <= instruction < source.size
                    and elf.contents(source)[instruction] == 0x20,
                    "opaque enemy ABI may only be called directly by SIM actor update")
            for name in ENEMY_ENTRIES:
                graph[caller].add(entry_functions[name])
            enemy_calls += 1
            continue
        if symbol.section >= len(elf.sections):
            continue  # Absolute constants such as __phase_boot_size.
        target = elf.sections[symbol.section]
        destination = symbol.address + addend
        caller = elf.function_at(source.index, address) if source.executable else None
        callee = elf.function_at(target.index, destination) if target.executable else None
        for phase, boundary, caller_name, tail in partial_phases:
            if (source.index == phase.index and target.index == phase.index
                    and address < boundary <= destination):
                # Reviewed guards: scenery calls wildlife only in Transition;
                # foreground calls combat only outside Transition. main selects
                # those same full/prefix loads using the unchanged simulation HUD.
                instruction = address - source.address - 1
                direct = (kind == 3 and 0 <= instruction < source.size
                          and elf.contents(source)[instruction] in (0x20, 0x4C))
                if (caller and caller.name == caller_name and callee == tail
                        and destination == tail.address and direct):
                    partial_tail_calls[phase.name] += 1
                else:
                    origin = caller.name if caller else f"{source.name}:${address:04X}"
                    errors.append(f"common {phase.name} prefix references unloaded tail: {origin} -> ${destination:04X}")
        if caller and callee:
            graph[caller].add(callee)
        for table in tables:
            owner, start, end, table_target, consumers = table
            if target.index == owner and start <= destination < end:
                consumers.add(caller)
                if caller:
                    graph[caller].add(table_target)
        if target.name not in phases and target.name != STARTUP_SECTION:
            continue
        origin = caller.name if caller else f"{source.name}+${address - source.address:X}"
        if source.name in phases or source.name == STARTUP_SECTION:
            if source.name in ENEMY_ENTRIES and target.name == ".phase.sim":
                # Only the active tail and this same immutable prefix coexist.
                local_references[source.name] += 1
            elif source.name != target.name:
                errors.append(f"{origin} in {source.name} references {target.name} at ${destination:04X}")
            else:
                local_references[source.name] += 1
            continue
        if not source.executable:
            # Validated as an owned switch table below.
            continue
        if target.name in ENEMY_ENTRIES:
            errors.append(f"unapproved direct enemy-tail reference: {origin} -> {target.name}")
            continue
        expected = startup_entry if target.name == STARTUP_SECTION else entry_functions[target.name]
        # Require the exact JSR instruction and function entry, not merely an
        # address that happens to fall inside a whitelisted entry function.
        instruction = address - source.address - 1
        is_jsr = kind == 3 and 0 <= instruction < source.size and elf.contents(source)[instruction] == 0x20
        if source.name == ".text" and caller and caller.name == "main" and destination == expected.address and callee == expected and is_jsr:
            entry_references[target.name] += 1
            entry_addresses[target.name].append(address)
        else:
            errors.append(f"unapproved resident/data reference: {origin} -> {target.name}:${destination:04X}")
    phase_tables = 0
    for section, start, end, target, consumers in tables:
        if elf.sections[target.section].name in phases or elf.sections[target.section].name == STARTUP_SECTION:
            phase_tables += 1
            if consumers != {target}:
                names = sorted(consumer.name if consumer else "unknown/data" for consumer in consumers)
                errors.append(f"overlay switch table {elf.sections[section].name}:${start:04X}-${end-1:04X} targets {target.name}, consumed by {names}")
    for name in [".text", *ENTRIES, STARTUP_SECTION]:
        require(sources[name] > 0, f"no retained relocations for {name}; link with --emit-relocs")
    require(not errors, "unsafe overlay references:\n  " + "\n  ".join(errors))
    require(enemy_calls == 1, f"expected exactly one opaque SIM enemy call, got {enemy_calls}")
    require(all(partial_tail_calls[phase.name] == 1 for phase, *_ in partial_phases),
            f"expected one guarded call to each mode-dependent tail, got {dict(partial_tail_calls)}")
    require(all(entry_references[name] == 1 for name in ENTRIES), f"expected one main JSR to each overlay entry, got {dict(entry_references)}")
    require(entry_references[STARTUP_SECTION] == 1, "startup checker must have exactly one direct main call")
    require(entry_addresses[STARTUP_SECTION][0] < entry_addresses[".phase.boot"][0],
            "startup checks must precede BOOT's first asset load")

    roots = [function for functions in elf.functions.values() for function in functions if function.name in ("irq_entry", "irq_body")]
    require({function.name for function in roots} == {"irq_entry", "irq_body"}, "IRQ audit roots are missing")
    reached = set()
    pending = list(roots)
    while pending:
        function = pending.pop()
        if function in reached:
            continue
        reached.add(function)
        require(elf.sections[function.section].name not in phases, f"IRQ reference graph reaches overlay function {function.name}")
        require(elf.sections[function.section].name != STARTUP_SECTION, f"IRQ reference graph reaches retired startup function {function.name}")
        require("call_indir" not in function.name, f"IRQ calls an indirect-call trampoline: {function.name}")
        pending.extend(graph[function])
    print(f"Overlay audit: {sum(local_references.values())} phase-local references, {phase_tables} owned switch tables; zero cross-phase references")
    for phase, boundary, _, tail in partial_phases:
        print(f"{phase.name} prefix: {boundary-phase.address} bytes; one reviewed mode-dependent call to {tail.name}")
    print("Resident entry calls: " + ", ".join(ENTRIES.values()))
    print("Enemy ABI: one opaque SIM call; four entries at the shared prefix boundary")
    print(f"IRQ relocation graph: {len(reached)} functions, all resident")
    print("Startup: ROMs disabled before BSS; only BSS clear, data copy, main are called")
    print("Startup checks: one main call before BOOT; no phase or IRQ references into retired workspace")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: python3 tools/check-overlays.py path/to/havoc.elf")
    try:
        audit(Path(sys.argv[1]))
    except (AuditError, OSError, ValueError, struct.error, IndexError) as error:
        raise SystemExit(f"overlay audit failed: {error}")
