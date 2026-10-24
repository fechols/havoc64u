#!/usr/bin/env python3
"""Reject unsafe references through the benchmark-only resident audio publisher.

Build with scripts/build-benchmark.sh, then pass that package's havoc.elf.
Only private temporary copies are corrupted; the input ELF stays unchanged.
"""
import argparse
import contextlib
import io
import json
from pathlib import Path
import runpy
import struct
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    checker = runpy.run_path(str(Path(__file__).with_name("check-overlays.py")))
    elf = checker["Elf"](args.elf)
    with contextlib.redirect_stdout(io.StringIO()):
        checker["audit"](args.elf)
    entries = []
    for section in elf.sections:
        if section.kind != 4:
            continue
        owner = elf.sections[section.info]
        if not owner.flags & 2:
            continue
        for position in range(section.offset, section.offset + section.size, 12):
            address, info, addend = struct.unpack_from("<IIi", elf.data, position)
            symbol = elf.symbol_tables[section.link][info >> 8]
            caller = elf.function_at(owner.index, address) if owner.executable else None
            target = (elf.function_at(symbol.section, symbol.address + addend)
                      if symbol.section < len(elf.sections) else None)
            entries.append(dict(position=position, owner=owner, address=address,
                                info=info, addend=addend, caller=caller,
                                target=target, table=section.link))

    def call_from(entry, caller):
        return entry["caller"] and entry["caller"].name == caller and entry["info"] & 255 == 3

    def call_to(entry, callee):
        return entry["target"] and entry["target"].name == callee and entry["info"] & 255 == 3

    incoming = next(entry for entry in entries
                    if call_from(entry, "havoc_c64::simulate")
                    and call_to(entry, "havoc_c64::send_audio"))
    outgoing = next(entry for entry in entries
                    if call_from(entry, "havoc_c64::send_audio")
                    and call_to(entry, "havoc::game::Game::music_state"))
    resident = next(entry for entry in entries if call_from(entry, "havoc::game::Game::sound"))
    wrong = next(entry for entry in entries if call_to(entry, "havoc::game::Game::bind_stage"))

    def shift_target(data, entry):
        struct.pack_into("<i", data, entry["position"] + 8, entry["addend"] + 1)

    def replace_target(data, entry, reference):
        assert entry["table"] == reference["table"]
        start, original = entry["position"] + 4, reference["position"] + 4
        data[start:start + 8] = elf.data[original:original + 8]

    def replace_jsr(data):
        section = outgoing["owner"]
        data[section.offset + outgoing["address"] - section.address - 1] = 0x4c

    cases = [
        ("caller-enters-mid-publisher", lambda data: shift_target(data, incoming),
         "benchmark audio publisher may only be called directly from SIM simulate"),
        ("publisher-enters-mid-SIM", lambda data: shift_target(data, outgoing),
         "unsafe overlay references"),
        ("publisher-jump-instead-of-call", replace_jsr, "unsafe overlay references"),
        ("wrong-SIM-target", lambda data: replace_target(data, outgoing, wrong),
         "unsafe overlay references"),
        ("resident-caller-outside-SIM", lambda data: replace_target(data, resident, incoming),
         "benchmark audio publisher may only be called directly from SIM simulate"),
    ]
    results = []
    with tempfile.TemporaryDirectory(prefix="havoc-benchmark-audit-") as temporary:
        damaged = Path(temporary) / "damaged.elf"
        for name, mutate, expected in cases:
            data = bytearray(elf.data)
            mutate(data)
            damaged.write_bytes(data)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    checker["audit"](damaged)
            except checker["AuditError"] as error:
                assert expected in str(error), f"{name}: unexpected rejection: {error}"
                results.append(dict(case=name, rejected=True, reason=str(error)))
            else:
                raise AssertionError(f"audit accepted {name}")
    assert args.elf.read_bytes() == elf.data, "input ELF changed"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(dict(passed=True, cases=results), indent=2) + "\n")
    print(f"PASS {len(results)} benchmark audio publisher negative fixtures; input ELF unchanged")


if __name__ == "__main__":
    main()
