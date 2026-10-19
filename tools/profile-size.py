"""Compare native function sizes from linked ELF files or a failed linker's map.

Uses only the Python standard library; packages and build outputs are read-only.
Example:
  python3 tools/profile-size.py --baseline target/campaign-baseline/dist/havoc.elf \
      --current target-c64/target/mos-c64-none/release/build/.../out/havoc.map
"""

import argparse
import json
from pathlib import Path
import re
import struct


def name_of(name):
    """Normalize LLVM-MOS's Rust legacy names and trailing compiler hashes."""
    if name.startswith("_ZN") and name.endswith("E"):
        remaining, parts = name[3:-1], []
        while remaining:
            match = re.match(r"\d+", remaining)
            if not match:
                break
            length = int(match[0])
            remaining = remaining[len(match[0]):]
            parts.append(remaining[:length])
            remaining = remaining[length:]
        if not remaining:
            name = "::".join(parts)
    return re.sub(r"::h[0-9a-f]{16}$", "", name)


def deduplicate(records):
    """Aliases at one address share bytes; equal addresses in overlays do not."""
    groups = {}
    for record in records:
        if not record["size"]:
            continue
        key = (record["section"], record["address"])
        old = groups.get(key)
        if old is None:
            groups[key] = record
        else:
            old["aliases"].append(record["name"])
            old["size"] = max(old["size"], record["size"])
    return list(groups.values())


def read_elf(path):
    data = path.read_bytes()
    if data[:6] != b"\x7fELF\x01\x01":
        raise ValueError(f"{path}: expected ELF32 little-endian")
    table = struct.unpack_from("<I", data, 32)[0]
    stride, count, names_index = struct.unpack_from("<HHH", data, 46)
    if stride != 40:
        raise ValueError("unsupported ELF section size")
    headers = [struct.unpack_from("<10I", data, table + index * stride) for index in range(count)]

    def contents(header):
        return data[header[4]:header[4] + header[5]]

    def string(strings, offset):
        return strings[offset:strings.index(0, offset)].decode("utf-8", errors="replace")

    section_names = contents(headers[names_index])
    sections = [string(section_names, header[0]) for header in headers]
    sizes = {sections[index]: header[5] for index, header in enumerate(headers) if header[2] & 4}
    records = []
    for header in headers:
        if header[1] != 2:
            continue
        strings = contents(headers[header[6]])
        for offset, address, size, info, _, owner in struct.iter_unpack("<IIIBBH", contents(header)):
            if owner >= len(headers) or not headers[owner][2] & 4 or info & 15 != 2:
                continue
            records.append(dict(name=name_of(string(strings, offset)), section=sections[owner],
                                address=address, size=size, aliases=[]))
    return deduplicate(records), sizes


def read_map(path):
    records, sections = [], {}
    section, input_section = "", ""
    for line in path.read_text().splitlines():
        match = re.match(r"\s*([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+\d+ (\.[^\s]+)$", line)
        if match:
            section = match[4]
            input_section = section
            sections[section] = int(match[3], 16)
            continue
        match = re.search(r":\((\.[^)]+)\)$", line)
        if match:
            input_section = match[1]
            continue
        match = re.match(r"\s*([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s{8,}(\S.*)$", line)
        if not match or not section.startswith((".phase.", ".text", ".startup_checks", ".tables")):
            continue
        name = match[3]
        if any(token in input_section for token in (".data", ".rodata")) or " = " in name:
            continue
        if name.startswith(("__", ".L", "PROVIDE(", "ASSERT(")):
            continue
        records.append(dict(name=name_of(name), section=section, address=int(match[1], 16),
                            size=int(match[2], 16), aliases=[]))
    relevant = {name: size for name, size in sections.items()
                if name.startswith((".phase.", ".text", ".startup_checks", ".tables"))}
    return deduplicate(records), relevant


def read(path):
    return read_elf(path) if path.read_bytes()[:4] == b"\x7fELF" else read_map(path)


def compare(current, baseline):
    previous = {}
    for row in baseline:
        previous.setdefault(row["name"], []).append(row)
    output = []
    for row in current:
        candidates = previous.get(row["name"], [])
        same_section = [candidate for candidate in candidates if candidate["section"] == row["section"]]
        old = next((candidate for candidate in same_section if candidate["address"] == row["address"]), None)
        if old is None and len(same_section) == 1:
            old = same_section[0]
        if old is None and len(candidates) == 1:
            old = candidates[0]
        output.append({**row, "baseline_size": old["size"] if old else 0,
                       "baseline_section": old["section"] if old else None,
                       "delta": row["size"] - (old["size"] if old else 0),
                       "status": "changed" if old else "ambiguous" if candidates else "new"})
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--current", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, default=Path("target/campaign-size"))
    parser.add_argument("--limit", type=int, default=15)
    args = parser.parse_args()
    current, current_sections = read(args.current)
    baseline, baseline_sections = read(args.baseline)
    rows = compare(current, baseline)
    new_names = {row["name"] for row in current}
    removed = [row for row in baseline if row["name"] not in new_names]
    sections = [dict(section=name, size=size, baseline_size=baseline_sections.get(name, 0),
                     delta=size - baseline_sections.get(name, 0))
                for name, size in current_sections.items()]
    images = []
    for name, size in current_sections.items():
        prefix = ".phase.sim" if name.startswith(".phase.enemy.") or name == ".phase.terminal" else ".phase.scenery" if name.startswith(".phase.scenery.") else None
        if prefix is not None:
            total = current_sections.get(prefix, 0) + size
            images.append(dict(image=name, prefix_size=current_sections.get(prefix, 0),
                               tail_size=size, size=total, headroom=0x4200 - total))
    notes = [
        "Overlay addresses overlap intentionally: totals are per section, never summed as resident RAM.",
        "A newly named composed entry can include formerly separate helpers; its delta is not net package growth.",
        "Absent helper symbols can indicate inlining, elimination, or renaming; inspect callsites before changing code.",
        "Prefer restoring shared bounded helpers before assembly. Recheck stack, cycles, and relocation ownership after each change.",
        "ELF input uses sized STT_FUNC symbols; map input includes sized code symbols and excludes explicit data input sections.",
        "Ambiguous repeated generic names are not assigned a speculative baseline; inspect their section/address entries in JSON.",
    ]
    removed_waves = [row for row in removed if row["name"].endswith("_wave")]
    entries = [row for row in rows if row["name"].startswith("encounter_")]
    if removed_waves and entries:
        notes.append("Inlining candidate: old wave helpers occupied " +
                     ", ".join(f'{row["name"]}={row["size"]} B' for row in removed_waves) +
                     "; composed encounter entries now range " +
                     f'{min(row["size"] for row in entries)}-{max(row["size"] for row in entries)} B. ' +
                     "Inspect signed constant divisions and repeated origin loads before writing assembly.")
    report = dict(baseline=str(args.baseline), current=str(args.current), sections=sections, composed_images=images,
                  functions=sorted(rows, key=lambda row: row["size"], reverse=True), removed=removed, notes=notes)
    lines = [f"Native size profile: {args.current}", f"Baseline: {args.baseline}", "", "Largest functions (bytes):"]
    for row in report["functions"][:args.limit]:
        lines.append(f'{row["size"]:6} {row["delta"]:+6}  {row["section"]:26} {row["name"]}')
    lines += ["", "Largest growth in matched functions (bytes; moved functions retain their baseline):"]
    matched = [row for row in rows if row["status"] == "changed"]
    for row in sorted(matched, key=lambda row: row["delta"], reverse=True)[:args.limit]:
        lines.append(f'{row["delta"]:+6} {row["baseline_size"]:6} -> {row["size"]:6}  {row["name"]}')
    lines += ["", "Code sections (bytes):"]
    for row in sections:
        lines.append(f'{row["size"]:6} {row["delta"]:+6}  {row["section"]}')
    lines += ["", "Composed SIM/SCENERY images (bytes; negative headroom is overflow):"]
    for row in images:
        lines.append(f'{row["size"]:6} headroom {row["headroom"]:+6}  {row["image"]}')
    lines += ["", *notes]
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "profile.json").write_text(json.dumps(report, indent=2) + "\n")
    (args.out_dir / "README.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
