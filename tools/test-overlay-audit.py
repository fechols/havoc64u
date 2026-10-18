"""Prove enemy overlay audit guards reject private, deliberately damaged ELFs."""
from pathlib import Path
import runpy
import struct
import sys
import tempfile


def main():
    source = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/havoc.elf")
    checker = runpy.run_path(str(Path(__file__).with_name("check-overlays.py")))
    elf = checker["Elf"](source)
    checker["audit"](source)
    entries = []
    for section in elf.sections:
        if section.kind != 4:
            continue
        symbols = elf.symbol_tables[section.link]
        owner = elf.sections[section.info]
        for position in range(section.offset, section.offset + section.size, 12):
            address, info, addend = struct.unpack_from("<IIi", elf.data, position)
            entries.append((position, owner, address, info & 255, symbols[info >> 8], section.link))

    def symbol_index(table, name):
        return next(i for i, symbol in enumerate(elf.symbol_tables[table]) if symbol.name == name)

    def replace_target(data, entry, name):
        position, _owner, _address, kind, _symbol, table = entry
        struct.pack_into("<Ii", data, position + 4, (symbol_index(table, name) << 8) | kind, 0)

    opaque = next(entry for entry in entries if entry[4].name == "enemy_step")
    geometry = next(entry for entry in entries if entry[1].name == ".phase.geometry" and entry[3] == 3)
    lowlands = next(entry for entry in entries if entry[1].name == ".phase.enemy.lowlands" and entry[3] == 3)
    pointer = next(entry for entry in entries
                   if not entry[1].executable and entry[1].flags & 2 and entry[3] in (3, 4, 5)
                   and entry[4].section < len(elf.sections)
                   and elf.sections[entry[4].section].name.startswith(".phase."))
    irq = next(entry for entry in entries
               if entry[1].name == ".text" and entry[3] == 3
               and (function := elf.function_at(entry[1].index, entry[2]))
               and function.name == "irq_body")

    def non_jsr(data):
        _position, owner, address, _kind, _symbol, _table = opaque
        data[owner.offset + address - owner.address - 1] = 0x4c

    def nonzero_addend(data):
        struct.pack_into("<i", data, opaque[0] + 8, 1)

    cases = [
        ("opaque-tail-jump", non_jsr, "opaque enemy ABI"),
        ("opaque-interior-entry", nonzero_addend, "opaque enemy ABI"),
        ("direct-common-to-tail", lambda data: replace_target(data, opaque, "enemy_lowlands"), "unsafe overlay references"),
        ("cross-tail-call", lambda data: replace_target(data, lowlands, "enemy_canyon"), "unsafe overlay references"),
        ("geometry-calls-enemy", lambda data: replace_target(data, geometry, "enemy_step"), "opaque enemy ABI"),
        ("irq-calls-enemy", lambda data: replace_target(data, irq, "enemy_step"), "opaque enemy ABI"),
        ("escaped-enemy-pointer", lambda data: replace_target(data, pointer, "enemy_step"), None),
        ("foreign-switch-table", lambda data: replace_target(data, pointer, "enemy_canyon"), "unsafe overlay references"),
    ]
    with tempfile.TemporaryDirectory(prefix="havoc-overlay-audit-") as temporary:
        target = Path(temporary) / "damaged.elf"
        for name, mutate, expected in cases:
            data = bytearray(elf.data)
            mutate(data)
            target.write_bytes(data)
            try:
                checker["audit"](target)
            except checker["AuditError"] as error:
                if expected is not None and expected not in str(error):
                    raise AssertionError(f"{name}: unexpected rejection: {error}") from error
                print(f"Rejected {name}")
            else:
                raise AssertionError(f"audit accepted {name}")
    assert source.read_bytes() == elf.data, "input ELF changed"
    print(f"Overlay audit negative fixtures: {len(cases)} passed; original ELF unchanged")


if __name__ == "__main__":
    main()
