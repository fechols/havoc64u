#!/usr/bin/env python3
"""Smoke-test an existing H.A.V.O.C. package in calibrated C64U-flavoured VICE.

Run from the configured WSL SDK: python3 tools/vice-smoke.py --video pal
The default emulator and ROMs are in the sibling doomc64u checkout. No package
files are modified. Timing reports describe this emulator, not physical C64U
performance. Only Python's standard library and LLVM-MOS llvm-nm are required.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import struct
import subprocess
import sys
import time
import zlib


ROOT = Path(__file__).resolve().parents[1]
PROMPT = re.compile(rb"\(C:\$[0-9a-fA-F]{4}\) $")
PALETTE = ((0, 0, 0), (255, 255, 255), (136, 0, 0), (170, 255, 238),
           (204, 68, 204), (0, 204, 85), (0, 0, 170), (238, 238, 119),
           (221, 136, 85), (102, 68, 0), (255, 119, 119), (51, 51, 51),
           (119, 119, 119), (170, 255, 102), (0, 136, 255), (187, 187, 187))


class Monitor:
    """Synchronous client for VICE's text remote monitor."""
    def __init__(self, port, timeout):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        self.socket.settimeout(timeout)
        self.command("r", r"\.;[0-9a-fA-F]{4}")

    def command(self, command, required=None):
        self.socket.sendall((command + "\n").encode())
        output = bytearray()
        while True:
            part = self.socket.recv(65536)
            if not part:
                raise RuntimeError("VICE closed its monitor connection")
            output.extend(part)
            if PROMPT.search(output):
                text = output.decode(errors="replace")
                if required is None or re.search(required, text):
                    if "ERROR --" in text or "Unknown command" in text:
                        raise RuntimeError(f"Monitor command failed: {command}\n{text}")
                    return text

    def registers(self):
        text = self.command("r", r"\.;[0-9a-fA-F]{4}")
        rows = re.findall(r"\.;([0-9a-fA-F]{4})\s+.*?\s+(\d+)\s*\n", text)
        if not rows:
            raise RuntimeError(f"Cannot parse VICE registers: {text}")
        pc, cycles = rows[-1]
        return {"pc": int(pc, 16), "bus_cycles": int(cycles)}

    def read(self, address, length):
        text = self.command(f"m {address:04x} {address+length-1:04x}", r">C:")
        values = bytearray()
        for line in text.splitlines():
            match = re.search(r">C:([0-9a-fA-F]{4})\s+((?:[0-9a-fA-F]{2}\s+){1,16})", line)
            if match:
                values.extend(bytes.fromhex(match[2]))
        if len(values) < length:
            raise RuntimeError(f"Short monitor read at ${address:04x}: {len(values)}/{length}")
        return bytes(values[:length])

    def integer(self, address, length=2):
        return int.from_bytes(self.read(address, length), "little")

    def run_until(self, target, failures):
        current = self.registers()
        if current["pc"] == target:
            self.command("step")
        deadline = time.monotonic() + self.socket.gettimeout()
        self.command(f"until {target:04x}")
        while True:
            current = self.registers()
            if current["pc"] in failures:
                raise RuntimeError(f"Target entered {failures[current['pc']]} at ${current['pc']:04x}")
            if current["pc"] == target:
                return current
            if time.monotonic() >= deadline:
                raise RuntimeError(f"Frame breakpoint timeout; PC=${current['pc']:04x}")
            self.socket.sendall(b"x\n")
            time.sleep(0.02)


def symbols(elf, llvm_nm):
    result = {}
    for line in subprocess.check_output([str(llvm_nm), "-n", str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            try:
                result[fields[2]] = int(fields[0], 16)
            except ValueError:
                pass
    for name in ("frame_complete", "FRAME_SERIAL", "FRONT_BANK", "GAME_MODE", "GAME_DISTANCE"):
        if name not in result:
            raise RuntimeError(f"Missing target symbol {name}; use the matching unstripped ELF")
    return result


def png(path, rows, width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    image = b"\x89PNG\r\n\x1a\n"
    image += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    image += chunk(b"IDAT", zlib.compress(b"".join(b"\0" + row for row in rows), 9))
    path.write_bytes(image + chunk(b"IEND", b""))


def capture(mon, syms, output, name):
    bank = mon.integer(syms["FRONT_BANK"], 1)
    if bank not in (2, 3):
        raise RuntimeError(f"Unexpected VIC front bank {bank}")
    parts = []
    mon.command("bank ram")
    for suffix, address, length in (("bitmap", bank * 0x4000 + 0x2000, 8000),
                                     ("screen", bank * 0x4000, 1000)):
        path = output / f"{name}-{suffix}.bin"
        mon.command(f'bsave "{path.as_posix()}" 0 {address:04x} {address+length-1:04x}')
        data = path.read_bytes()
        if len(data) != length:
            raise RuntimeError(f"Unexpected snapshot size for {path}: {len(data)}")
        parts.append(data)
    mon.command("bank io")
    color_path = output / f"{name}-color.bin"
    mon.command(f'bsave "{color_path.as_posix()}" 0 d800 dbe7')
    parts.append(bytes(value & 15 for value in color_path.read_bytes()))
    background = mon.integer(0xd021, 1) & 15
    mon.command("bank cpu")
    native = b"".join(parts)
    if len(native) != 10000:
        raise RuntimeError(f"Invalid native screen length: {len(native)}")
    path = output / f"{name}-native.bin"
    path.write_bytes(native)
    bitmap, screen, colors = parts
    rows = []
    for y in range(200):
        row = bytearray()
        for x in range(160):
            cell = (y // 8) * 40 + x // 4
            packed = bitmap[cell * 8 + y % 8]
            slot = (packed >> (6 - 2 * (x % 4))) & 3
            color = (background, screen[cell] >> 4, screen[cell] & 15, colors[cell])[slot]
            row.extend(PALETTE[color])
        rows.append(bytes(row))
    png(output / f"{name}-native.png", rows, 160, 200)
    # VIC multicolor pixels are wide on a 4:3 display; retain both the exact
    # native grid and an aspect-correct preview for visual review.
    wide_rows = [b"".join(row[x:x+3] * 4 for x in range(0, 480, 3)) for row in rows]
    png(output / f"{name}-preview.png", [wide_rows[y*200//480] for y in range(480)], 640, 480)
    (output / f"{name}-native.ppm").write_bytes(b"P6\n160 200\n255\n" + b"".join(rows))
    mon.command(f'screenshot "{(output / (name+"-vice.png")).as_posix()}" 2')
    return {"bank": bank, "native": str(path), "sha256": hashlib.sha256(native).hexdigest(),
            "background": background, "nonzero_bitmap_bytes": sum(value != 0 for value in bitmap)}


def counters(mon, syms):
    result = {}
    for name, width in (("FRAME_SERIAL", 2), ("VIDEO_FRAMES", 2), ("AUDIO_TICKS", 2),
                        ("FRAME_TRIANGLES", 2), ("FRAME_DROPPED", 2), ("STREAM_BYTES", 2),
                        ("INPUT_OVERFLOWS", 2), ("GAME_MODE", 1), ("GAME_DISTANCE", 4),
                        ("FRONT_BANK", 1), ("TEST_INPUT", 1), ("RENDER_SEGMENT", 1),
                        ("ENEMY_CODE_PROFILE", 1), ("ENEMY_MODEL_PROFILE", 1), ("MODEL_STREAM_BYTES", 2),
                        ("MUSIC_CUE", 1), ("MUSIC_ACTIVE_CUE", 1), ("MUSIC_PAGE", 1),
                        ("MUSIC_UNDERRUNS", 2)):
        if name in syms:
            result[name.lower()] = mon.integer(syms[name], width)
    mon.command("bank io")
    result["joystick_port2"] = mon.integer(0xdc00, 1) & 31
    result["turbo_register"] = mon.integer(0xd031, 1)
    mon.command("bank cpu")
    result.update(mon.registers())
    return result


def stores(log_path, initial_values=None):
    text = log_path.read_text(errors="replace") if log_path.exists() else ""
    result = []
    values = dict(initial_values or {})
    tracked = set(values)
    # VICE trace records pair an effective store address with the instruction's
    # registers. A store's value comes from A, X or Y according to its opcode.
    pattern = r"(?:Trace|trace)\s+(?:on\s+)?store\s+\$?([0-9a-fA-F]{4})[^\n]*\n([^\n]*)"
    for match in re.finditer(pattern, text):
        address = int(match[1], 16)
        instruction = match[2]
        register = re.search(r"\bST([AXY])\b", instruction, re.I)
        value = re.search(rf"\b{register[1]}:([0-9a-fA-F]{{2}})\b", instruction, re.I) if register else None
        cycles = re.search(r"\s(\d+)\s*$", instruction)
        if cycles is None:
            continue
        if value is not None:
            updated = int(value[1], 16)
        elif address in tracked and re.search(r"\b(?:INC|DEC)\b", instruction, re.I):
            # Counters may compile to read/modify/write instructions rather
            # than STA. Only infer them when the caller supplies known state.
            updated = (values[address] + (1 if re.search(r"\bINC\b", instruction, re.I) else -1)) & 255
        else:
            continue
        values[address] = updated
        result.append({"address": address, "value": updated, "bus_cycles": int(cycles[1])})
    return result


def sid_summary(log_path, events, sid_count=2):
    chips = [{"writes": 0, "gate_on": 0, "gate_off": 0, "registers": {},
              "voices": [{"gate_on": 0, "gate_off": 0} for _ in range(3)]} for _ in range(sid_count)]
    for event in events:
        address = event["address"]
        chip, reg = (address - 0xd400) // 32, (address - 0xd400) % 32
        if not 0 <= chip < sid_count or reg > 24:
            continue
        entry = chips[chip]
        entry["writes"] += 1
        key = f"{reg:02x}"
        entry["registers"][key] = entry["registers"].get(key, 0) + 1
        if reg in (4, 11, 18):
            gate = "gate_on" if event["value"] & 1 else "gate_off"
            entry[gate] += 1
            entry["voices"][reg // 7][gate] += 1
    names = ("left_d400", "right_d420", "socket1_d440", "socket2_d460")
    return dict(zip(names, chips), source=str(log_path), sid_count=sid_count)


def phase_profile(events, address, title, frames):
    names = {1: "simulation", 2: "geometry", 3: "pixels", 4: "publish",
             5: "simulation", 6: "geometry", 7: "pixels", 8: "audio", 9: "audio",
             10: "scenery", 11: "scenery"}
    transitions = [event for event in events if event["address"] == address and event["value"] in names]
    previous = title["bus_cycles"]
    for frame in frames:
        end = frame["bus_cycles"]
        selected = [event for event in transitions if previous <= event["bus_cycles"] < end]
        costs = {name: 0 for name in names.values()}
        loads = {name: 0 for name in ("simulation", "geometry", "pixels", "audio", "scenery")}
        work = {name: 0 for name in ("simulation", "geometry", "pixels", "audio", "scenery")}
        for index, event in enumerate(selected):
            next_cycle = selected[index+1]["bus_cycles"] if index+1 < len(selected) else end
            duration = next_cycle - event["bus_cycles"]
            name = names[event["value"]]
            costs[name] += duration
            if event["value"] in (1, 2, 3, 8, 10):
                loads[name] += duration
            elif event["value"] in (5, 6, 7, 9, 11):
                work[name] += duration
        frame["phase_bus_cycles"] = costs
        if any(event["value"] in (5, 6, 7) for event in selected):
            frame["phase_load_bus_cycles"] = loads
            frame["phase_work_bus_cycles"] = work
        frame["unprofiled_bus_cycles"] = frame["bus_cycles_delta"] - sum(costs.values())
        previous = end
    return transitions


def pixel_profile(events, address, title, frames):
    names = {1: "raster", 2: "hud", 3: "pack", 4: "copy"}
    transitions = [event for event in events if event["address"] == address and event["value"] in (0, 1, 2, 3, 4)]
    previous = title["bus_cycles"]
    for frame in frames:
        end = frame["bus_cycles"]
        selected = [event for event in transitions if previous <= event["bus_cycles"] < end]
        costs = {name: 0 for name in names.values()}
        for index, event in enumerate(selected):
            if event["value"] == 0:
                continue  # Explicit end marker excludes frame publication.
            next_cycle = selected[index+1]["bus_cycles"] if index+1 < len(selected) else end
            costs[names[event["value"]]] += next_cycle - event["bus_cycles"]
        frame["pixel_bus_cycles_detail"] = costs
        if "phase_bus_cycles" in frame:
            frame["pixel_unprofiled_bus_cycles"] = frame["phase_bus_cycles"]["pixels"] - sum(costs.values())
        frame["pixel_profile_markers"] = len(selected)
        previous = end
    return len(transitions)


def dwarf_members(description, label):
    fields = {}
    for block in re.split(r"(?=^0x[0-9a-fA-F]+:)", description, flags=re.M):
        if not re.match(r"0x[0-9a-fA-F]+:\s+DW_TAG_member\b", block):
            continue
        name = re.search(r'DW_AT_name\s+\("([^"]+)"\)', block)
        offset = re.search(r"DW_AT_data_member_location\s+\((0x[0-9a-fA-F]+|\d+)\)", block)
        kind = re.search(r'DW_AT_type\s+\([^\n]*"([^"]+)"\)', block)
        if not (name and offset and kind):
            raise RuntimeError(f"Incomplete DWARF {label} member: {block}")
        field = {"offset": int(offset[1], 0), "type": kind[1]}
        if name[1] in fields and fields[name[1]] != field:
            raise RuntimeError(f"Conflicting DWARF {label} layouts for {name[1]}")
        fields[name[1]] = field
    return fields


def resident_symbol(syms, name):
    candidates = [(symbol, address) for symbol, address in syms.items()
                  if symbol == name or ("havoc_c64" in symbol and
                     re.search(rf"{len(name)}{name}17h[0-9a-fA-F]+E(?:\.\d+)?$", symbol))]
    if len(candidates) != 1:
        raise RuntimeError(f"Expected one resident {name} ELF symbol, got {candidates}")
    return candidates[0]


def dwarf_game_layout(elf, tool, syms, output, controls=False):
    """Read Rust field offsets and enum values from this exact target build."""
    descriptions = {}
    for name in (("Game", "Mode", "Vec3", "Controls", "InputSample") if controls else ("Game", "Mode")):
        descriptions[name] = subprocess.check_output(
            [str(tool), f"--name={name}", "--show-children", str(elf)], text=True)
        (output / f"dwarf-{name.lower()}.txt").write_text(descriptions[name])
    fields = dwarf_members(descriptions["Game"], "Game")
    modes = {}
    for block in re.split(r"(?=^0x[0-9a-fA-F]+:)", descriptions["Mode"], flags=re.M):
        if not re.match(r"0x[0-9a-fA-F]+:\s+DW_TAG_enumerator\b", block):
            continue
        name = re.search(r'DW_AT_name\s+\("([^"]+)"\)', block)
        value = re.search(r"DW_AT_const_value\s+\((0x[0-9a-fA-F]+|\d+)\)", block)
        if name and value:
            modes[name[1]] = int(value[1], 0)
    required = {"rail_ticks": "u16", "boss_z": "i16", "mode": "havoc::game::Mode",
                "boss_started": "bool", "boss_defeated": "bool", "score": "u32",
                "shield": "u8", "lives": "u8", "bombs": "u8", "respawn_ticks": "u8",
                "menu_ticks": "u16"}
    if "Transition" in modes:
        required.update(transition_ticks="u16", ticks="u16")
    sizes = {"u8": 1, "i8": 1, "u16": 2, "u32": 4, "i16": 2, "bool": 1, "havoc::game::Mode": 1}
    sample = None
    if controls:
        required.update(ticks="u16", roll_ticks="u8", roll_cooldown="u8", roll_direction="i8")
        for outer, name, members, expected in (("player", "Vec3", ("x", "y", "z"), "havoc::Vec3"),
                                               ("controls", "Controls", ("bits",), "havoc::game::Controls")):
            if fields.get(outer, {}).get("type") != expected:
                raise RuntimeError(f"Unsupported Game::{outer} DWARF type")
            inner = dwarf_members(descriptions[name], name)
            for member in members:
                kind = "i16" if name == "Vec3" else "u8"
                if inner.get(member, {}).get("type") != kind:
                    raise RuntimeError(f"Unsupported {name}::{member} DWARF type")
                key = f"{outer}.{member}"
                fields[key] = dict(inner[member], offset=fields[outer]["offset"] + inner[member]["offset"])
                required[key] = kind
        sample_fields = dwarf_members(descriptions["InputSample"], "InputSample")
        byte_size = re.search(r"DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)", descriptions["InputSample"])
        if not byte_size:
            raise RuntimeError("Missing InputSample DWARF byte size")
        sample = {"size": int(byte_size[1], 0), "fields": sample_fields,
                  "address": resident_symbol(syms, "INPUTS")[1],
                  "head_address": resident_symbol(syms, "INPUT_HEAD")[1]}
        for field, kind in (("ms", "u32"), ("bits", "u8"), ("pressed", "u8")):
            if sample_fields.get(field, {}).get("type") != kind:
                raise RuntimeError(f"Unsupported InputSample::{field} DWARF type")
            sample_fields[field]["size"] = sizes[kind]
            if sample_fields[field]["offset"] + sizes[kind] > sample["size"]:
                raise RuntimeError("InputSample field exceeds its DWARF size")
    for name, kind in required.items():
        if name not in fields or fields[name]["type"] != kind:
            raise RuntimeError(f"Unsupported or missing target Game::{name} DWARF type")
        fields[name]["size"] = sizes[kind]
    mode_size = re.search(r"DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)", descriptions["Mode"])
    expected_modes = {"Title": 0, "Playing": 1, "Respawning": 2, "Victory": 3, "GameOver": 4, "Attract": 5}
    if "Transition" in modes:
        expected_modes["Transition"] = 6
    if not mode_size or int(mode_size[1], 0) != 1 or modes != expected_modes:
        raise RuntimeError("Unsupported target Mode representation")
    name, address = resident_symbol(syms, "GAME")
    end = syms.get("__game_state_end")
    if end is None or any(address+fields[field]["offset"]+fields[field]["size"] > end for field in required):
        raise RuntimeError("DWARF Game fields exceed the linked game-state memory range")
    return {"symbol": name, "address": address, "fields": fields, "modes": modes, "input_sample": sample,
            "source": "llvm-dwarfdump from the matching packaged ELF; no assumed Rust field offsets"}


def fletcher16(data):
    a = b = 0
    for value in data:
        a = (a + value) % 255
        b = (b + a) % 255
    return a | (b << 8)


def active_section(rail_ticks, mode):
    """A bridge holds the incoming boundary while rendering its outgoing area."""
    return min((rail_ticks - (mode == 6)) // 3750, 3)


def package_model_layout(package):
    """Keep baseline benchmarks compatible while checking the current format."""
    version = int.from_bytes(package[4:8], "little")
    formats = {0x534d0300: (b"SMA2", 80), 0x534d0400: (b"SMA3", 88)}
    if package[:4] != b"SM64" or version not in formats or package[16] != 4 or package[17] not in (2, 4):
        raise RuntimeError("Expected a four-roster version 3.0 or 4.0 package")
    magic, descriptors = formats[version]
    return {"version": version, "magic": magic, "descriptors": descriptors,
            "medal": version == 0x534d0400}


def enemy_bank_check(mon, syms, package, section):
    """Prove the selected executable and every resident model byte agree."""
    required = ("ENEMY_CODE_PROFILE", "ENEMY_MODEL_PROFILE", "MODEL_STREAM_BYTES")
    if any(name not in syms for name in required):
        raise RuntimeError("Enemy streaming requires code/model identity telemetry")
    format = package_model_layout(package)
    reference = package[0xb0000+section*2048:0xb0000+(section+1)*2048]
    if len(reference) != 2048 or reference[:4] != format["magic"] or reference[5] != section:
        raise RuntimeError(f"Invalid packaged level {section} model reference")
    mon.command("bank ram")
    cache = mon.read(0x8400, 2048)
    code = mon.integer(syms["ENEMY_CODE_PROFILE"], 1)
    models = mon.integer(syms["ENEMY_MODEL_PROFILE"], 1)
    mode = mon.integer(syms["GAME_MODE"], 1)
    expected_code = 4 if format["medal"] and mode in (3, 4) else section
    mon.command("bank cpu")
    if code != expected_code or models != section or cache != reference:
        raise RuntimeError(f"Stale enemy bank: expected code {expected_code}, model {section}; code {code}, models {models}, bytes match {cache == reference}")
    return {"section": section, "code_profile": code, "model_profile": models,
            "all_2048_bytes_match": True, "sha256": hashlib.sha256(cache).hexdigest()}


def streaming_checks(mon, syms, failures, elf, reu, dwarf_tool, output, result):
    """Monitor-assisted boundary scenarios, deliberately separate from timing."""
    layout = dwarf_game_layout(elf, dwarf_tool, syms, output, controls=True)
    result.update(passed=False, monitor_assisted=True,
                  scope="Seeded section boundaries and final winning condition; not a full native playthrough. Crossings check both effects voices alongside music. Victory checks the natural 400-tick reveal and held/fresh physical FIRE. All restart input uses the physical joystick port.",
                  excluded_from_timing=True, render_segment_telemetry="RENDER_SEGMENT" in syms,
                  layout=layout, scenarios=[])
    data = reu.read_bytes()
    format = package_model_layout(data)
    segment_base = int.from_bytes(data[12:16], "little")
    references = [data[segment_base+i*2048:segment_base+i*2048+512] for i in range(4)]
    checksums = [int.from_bytes(data[20+i*2:22+i*2], "little") for i in range(4)]
    for index, reference in enumerate(references):
        if len(reference) != 512 or reference[:4] != b"SMS1" or reference[4] != index or fletcher16(reference) != checksums[index]:
            raise RuntimeError(f"Invalid packaged segment {index} reference")
    for name in ("__stack_bottom", "__irq_stack_bottom"):
        if name not in syms:
            raise RuntimeError(f"Missing {name} guard symbol")

    def field_read(name):
        field = layout["fields"][name]
        raw = mon.read(layout["address"] + field["offset"], field["size"])
        return int.from_bytes(raw, "little", signed=field["type"].startswith("i"))

    def seed(scenario, values):
        mon.command("bank ram")
        patches = []
        for name, value in values.items():
            field = layout["fields"][name]
            address = layout["address"] + field["offset"]
            previous = field_read(name)
            raw = value.to_bytes(field["size"], "little", signed=field["type"].startswith("i"))
            mon.command(f"> {address:04x} " + " ".join(f"{byte:02x}" for byte in raw))
            if field_read(name) != value:
                raise RuntimeError(f"Monitor fixture write failed for Game::{name}")
            patches.append({"field": name, "address": address, "previous": previous, "seeded": value})
        scenario["monitor_patches"] = patches
        mon.command("bank cpu")

    def caches(snapshot_name=None):
        # These are published platform cache addresses, not Rust struct offsets.
        mon.command("bank ram")
        views = []
        for slot, address in enumerate((0x9500, 0x9700)):
            cache = mon.read(address, 512)
            index = cache[4]
            if cache[:4] != b"SMS1" or index >= 4:
                raise RuntimeError(f"Invalid SMS1 header in cache slot {slot}")
            checksum = fletcher16(cache)
            if checksum != checksums[index] or cache != references[index]:
                raise RuntimeError(f"Cache slot {slot}, segment {index} differs from the packaged 512-byte prefix")
            views.append({"slot": slot, "address": address, "section": index, "magic": "SMS1",
                          "checksum": checksum, "expected_checksum": checksums[index],
                          "all_512_bytes_match": True, "sha256": hashlib.sha256(cache).hexdigest()})
            if snapshot_name:
                path = output / f"{snapshot_name}-cache{slot}.bin"
                path.write_bytes(cache)
                views[-1]["snapshot"] = str(path)
        mon.command("bank cpu")
        return views

    def observe(scenario):
        mon.run_until(syms["frame_complete"], failures)
        frame = counters(mon, syms)
        mon.command("bank ram")
        observed = ["mode", "rail_ticks", "boss_z", "boss_started", "boss_defeated", "score", "shield", "lives", "bombs", "menu_ticks",
                    "player.x", "player.y", "player.z"]
        if "Transition" in layout["modes"]:
            observed += ["ticks", "transition_ticks"]
        frame["game"] = {name: field_read(name) for name in observed}
        frame["stack_guards"] = {name: mon.integer(syms[name], 1) for name in ("__stack_bottom", "__irq_stack_bottom")}
        mon.command("bank cpu")
        frame["cache_views"] = caches()
        frame["enemy_bank"] = enemy_bank_check(mon, syms, data,
            active_section(frame["game"]["rail_ticks"], frame["game_mode"]))
        scenario.setdefault("frames", []).append(frame)
        if frame["frame_dropped"] or frame["input_overflows"] or any(value != 0xa5 for value in frame["stack_guards"].values()):
            raise RuntimeError("Seeded scenario dropped input/geometry or changed a stack guard")
        if frame["test_input"] != 255 or frame["game_mode"] != frame["game"]["mode"]:
            raise RuntimeError("Seeded scenario did not retain physical input or consistent exported game mode")
        if frame.get("music_underruns", 0):
            raise RuntimeError("Music underrun during section streaming")
        if "render_segment" in frame and frame["render_segment"] != active_section(frame["game"]["rail_ticks"], frame["game_mode"]):
            raise RuntimeError("Geometry rendered a stale segment after a section transition or restart")
        if frame["game_mode"] == layout["modes"].get("Transition"):
            progress = frame["game"]["transition_ticks"]
            label = "late" if progress >= 320 else "middle" if progress >= 180 else "entrance"
            captures = scenario.setdefault("transition_captures", {})
            if label not in captures:
                captures[label] = capture(mon, syms, output, f"{scenario['name']}-{label}")
            if scenario.get("expected_section") == 3 and 300 <= progress < 330 and "peak" not in captures:
                captures["peak"] = dict(capture(mon, syms, output, f"{scenario['name']}-peak"),
                                        transition_ticks=progress)
        return frame

    def wait_ready(scenario, mode, section, desired):
        for _ in range(160 if "Transition" in layout["modes"] else 4):
            frame = observe(scenario)
            current = active_section(frame["game"]["rail_ticks"], frame["game_mode"])
            indices = {view["section"] for view in frame["cache_views"]}
            if frame["game_mode"] == mode and current == section and indices == {section, desired}:
                return frame
        raise RuntimeError(f"{scenario['name']} did not reach its expected state and prefetched caches")

    def finish_capture(scenario, name):
        scenario["capture"] = capture(mon, syms, output, name)
        scenario["cache_views"] = caches(name)
        scenario["passed"] = True
        print(f"STREAM {scenario['name']}: passed ({len(scenario.get('frames', []))} observed frames)", flush=True)

    # Ordinary traces and metrics have already been finalized. Trace only the
    # core SID gate registers here; the two third voices belong to effects.
    # This catches silent crossings without adding full per-register logs.
    mon.command("delete")
    for address in failures:
        mon.command(f"break exec {address:04x}")
    for base in (0xd400, 0xd420):
        for register in (4, 11, 18):
            address = base + register
            mon.command(f"trace store {address:04x} if @cpu:$01 == $35")
    path = output / "streaming-monitor.log"
    path.unlink(missing_ok=True)
    mon.command(f'logname "{path.as_posix()}"')
    mon.command("log on")
    mon.command("jpdb 1 $1f")
    result["initial_cache_views"] = caches("streaming-initial")
    for section, label in ((1, "canyon"), (2, "base"), (3, "carrier-approach")):
        scenario = {"name": f"section-{section}-boundary", "expected_section": section,
                    "expected_prefetch": (section+1)&3}
        result["scenarios"].append(scenario)
        seed(scenario, {"rail_ticks": section*3750-1})
        wait_ready(scenario, layout["modes"]["Playing"], section, (section+1)&3)
        if "Transition" in layout["modes"]:
            bridge = [frame for frame in scenario["frames"] if frame["game_mode"] == layout["modes"]["Transition"]]
            if not bridge:
                raise RuntimeError("Section boundary skipped its scenic flight")
            # Account for catch-up ticks before/after each presented frame:
            # elapsed ticks since bridge entry = transition progress, then
            # exactly 400 plus the resumed local combat ticks.
            epoch = (bridge[0]["game"]["ticks"] - bridge[0]["game"]["transition_ticks"]) & 0xffff
            initial_resources = {name: bridge[0]["game"][name] for name in ("shield", "lives", "bombs", "score")}
            for frame in bridge:
                game = frame["game"]
                if game["rail_ticks"] != section*3750 or ((game["ticks"]-epoch) & 0xffff) != game["transition_ticks"]:
                    raise RuntimeError("Bridge encounter clock or animation clock advanced incorrectly")
                if any(game[name] != value for name, value in initial_resources.items()):
                    raise RuntimeError("Enemy-free scenic flight changed player resources")
            final = scenario["frames"][-1]["game"]
            duration = ((final["ticks"]-epoch) & 0xffff) - (final["rail_ticks"]-section*3750)
            if duration != 400:
                raise RuntimeError(f"Scenic flight lasted {duration} ticks instead of 400")
            scenario["transition"] = {"duration_ticks": duration, "observed_frames": len(bridge), "resources_preserved": True}
            # Exclude entry/exit and any effects left over from the preceding
            # gameplay sample. No physical FIRE is held during these flights.
            sounding = [frame for frame in bridge if 100 <= frame["game"]["transition_ticks"] <= 350]
            if len(sounding) < 2 or any(frame["joystick_port2"] != 31 for frame in sounding):
                raise RuntimeError("Scenic sound check needs neutral input across its middle interval")
            mon.command("log off")
            events = [event for event in stores(path)
                      if sounding[0]["bus_cycles"] < event["bus_cycles"] <= sounding[-1]["bus_cycles"]]
            summary = sid_summary(path, events, data[17])
            for chip in ("left_d400", "right_d420"):
                if not summary[chip]["voices"][2]["gate_on"]:
                    raise RuntimeError(f"Scenic flight never sounded the reserved effects voice on {chip}")
                if not any(voice["gate_on"] for voice in summary[chip]["voices"][:2]):
                    raise RuntimeError(f"Scenic flight interrupted music gates on {chip}")
            scenario["transition"]["sound"] = {
                "from_tick": sounding[0]["game"]["transition_ticks"],
                "through_tick": sounding[-1]["game"]["transition_ticks"],
                "scope": "Core SID control-register traces; both effects voices attack while music continues.",
                "sid": summary,
            }
            mon.command("log on")
        if not any(frame["stream_bytes"] for frame in scenario["frames"]):
            raise RuntimeError(f"No actual segment DMA observed for section {section}")
        if not any(frame["model_stream_bytes"] for frame in scenario["frames"]):
            raise RuntimeError(f"No model-bank DMA observed for section {section}")
        finish_capture(scenario, label)

    scenario = {"name": "visible-carrier", "note": "Monitor advances to boss approach and places the carrier at its normal settled depth."}
    result["scenarios"].append(scenario)
    seed(scenario, {"rail_ticks": 11400, "boss_z": 680})
    frame = wait_ready(scenario, layout["modes"]["Playing"], 3, 0)
    if not frame["game"]["boss_started"] or frame["game"]["boss_z"] != 680:
        raise RuntimeError("Seeded carrier encounter did not activate at its settled depth")
    finish_capture(scenario, "carrier")

    for section in range(4):
        scenario = {"name": f"checkpoint-{section}-respawn", "expected_section": section,
                    "note": "Seed the final respawn tick; normal simulation restores the section checkpoint."}
        result["scenarios"].append(scenario)
        seed(scenario, {"rail_ticks": section*3750+625, "mode": layout["modes"]["Respawning"],
                        "respawn_ticks": 1, "shield": 0, "lives": 2, "score": 1234})
        frame = wait_ready(scenario, layout["modes"]["Playing"], section, (section+1)&3)
        if any(frame["game"][field] != value for field, value in {"shield": 100, "lives": 2, "score": 1234}.items()):
            raise RuntimeError("Checkpoint respawn failed to restore health and preserve lives/score")
        finish_capture(scenario, f"checkpoint-{section}")

    if format["medal"]:
        scenario = {"name": "natural-gold-medal-reveal", "captures": {},
                    "note": "Seed the last mission tick with the carrier defeated; the normal win path and all 400 reveal ticks run in native code. Joystick FIRE is pressed during the reveal, held through arrival, then released and freshly pressed to restart."}
        result["scenarios"].append(scenario)
        mon.command("jpdb 1 $1f")
        observe(scenario)
        observe(scenario)
        seed(scenario, {"rail_ticks": 14999, "mode": layout["modes"]["Playing"],
                        "boss_started": 1, "boss_defeated": 1,
                        "score": 24000, "shield": 76, "lives": 2, "bombs": 1})
        mon.command("jpdb 1 $0f")
        frozen = epoch = None
        music_pages = []
        repress = 0
        for _ in range(200):
            frame = observe(scenario)
            game = frame["game"]
            if game["mode"] != layout["modes"]["Victory"]:
                raise RuntimeError("Natural victory entry failed or FIRE skipped the medal reveal")
            if epoch is None:
                if game["menu_ticks"] >= 400:
                    raise RuntimeError("First victory frame skipped the reveal")
                epoch = (game["ticks"] - game["menu_ticks"]) & 0xffff
                frozen = {name: game[name] for name in
                          ("score", "shield", "lives", "bombs", "rail_ticks", "player.x", "player.y", "player.z")}
            elapsed = (game["ticks"] - epoch) & 0xffff
            if game["menu_ticks"] != min(elapsed, 400):
                raise RuntimeError("Victory reveal did not advance exactly 400 simulation ticks")
            if any(game[name] != value for name, value in frozen.items()):
                raise RuntimeError("Victory reveal changed the frozen ship, mission state, or resources")
            if frame.get("music_cue") != 5 or ("music_active_cue" in frame and frame["music_active_cue"] != 5):
                raise RuntimeError("Victory reveal interrupted the homecoming music")
            page = frame.get("music_page", 0)
            if music_pages and page < music_pages[-1]:
                raise RuntimeError("Victory reveal restarted its music before the full cue loop")
            music_pages.append(page)
            progress = game["menu_ticks"]
            label = "hold" if progress == 400 else "approach" if progress >= 100 else "fade" if progress >= 40 else "entry"
            if label not in scenario["captures"]:
                saved = capture(mon, syms, output, f"victory-reveal-{label}")
                saved.update(reveal_ticks=progress, elapsed_ticks=elapsed)
                scenario["captures"][label] = saved
                if label == "approach":
                    # Only the gold mesh remains after blackout; cockpit white,
                    # gray, cyan and green must all be absent before results.
                    native = Path(saved["native"]).read_bytes()
                    palette = set()
                    for y in range(200):
                        for x in range(160):
                            cell = y//8*40 + x//4
                            slot = native[cell*8+y%8] >> (6 - 2*(x%4)) & 3
                            palette.add((saved["background"], native[8000+cell] >> 4,
                                         native[8000+cell] & 15, native[9000+cell] & 15)[slot])
                    if not palette <= {0, 7, 8, 9}:
                        raise RuntimeError(f"Cinematic retained battlefield or cockpit ink after blackout: {palette}")
                    scenario["cinematic_cockpit_hidden"] = True
            if progress >= 100 and repress == 0:
                mon.command("jpdb 1 $1f")
                repress = 1
            elif progress >= 120 and repress == 1:
                mon.command("jpdb 1 $0f")
                repress = 2
            if elapsed >= 450:
                if frame["joystick_port2"] != 15 or repress != 2:
                    raise RuntimeError("Victory results were not exercised with held physical FIRE")
                break
        else:
            raise RuntimeError("Victory reveal did not reach its settled results")
        scenario.update(duration_ticks=400, held_fire_consumed=True, frozen_state=frozen,
                        uninterrupted_music_pages=music_pages)
        mon.command("jpdb 1 $1f")
        observe(scenario)
        released = observe(scenario)
        if released["game_mode"] != layout["modes"]["Victory"] or released["joystick_port2"] != 31:
            raise RuntimeError("Releasing FIRE dismissed the settled medal")
        mon.command("jpdb 1 $0f")
        restarted = wait_ready(scenario, layout["modes"]["Playing"], 0, 1)
        expected = {"shield": 100, "lives": 3, "bombs": 3, "score": 0, "boss_started": 0, "boss_defeated": 0}
        if any(restarted["game"][name] != value for name, value in expected.items()):
            raise RuntimeError("Fresh FIRE failed to reset the mission after the medal reveal")
        scenario["fresh_fire_restarted"] = True
        finish_capture(scenario, "victory-reveal-restarted")

    for mode_name in ("Victory", "GameOver"):
        scenario = {"name": f"restart-from-{mode_name.lower()}",
                    "note": "Results mode is a monitor fixture; release and fresh FIRE use joystick port 2."}
        result["scenarios"].append(scenario)
        mon.command("jpdb 1 $1f")
        # Two complete frames drain the previous held-fire sample and its edges.
        observe(scenario)
        observe(scenario)
        values = {"rail_ticks": 11400, "mode": layout["modes"][mode_name]}
        if mode_name == "Victory" and format["medal"]:
            values["menu_ticks"] = 400
        if mode_name == "GameOver":
            values.update(shield=0, lives=0)
        seed(scenario, values)
        frame = wait_ready(scenario, layout["modes"][mode_name], 3, 0)
        if frame["joystick_port2"] != 31:
            raise RuntimeError("Results fixture did not observe a released physical joystick")
        scenario["results_capture"] = capture(mon, syms, output, f"results-{mode_name.lower()}")
        mon.command("jpdb 1 $0f")
        frame = wait_ready(scenario, layout["modes"]["Playing"], 0, 1)
        expected = {"shield": 100, "lives": 3, "bombs": 3, "score": 0, "boss_started": 0, "boss_defeated": 0}
        if frame["joystick_port2"] != 15 or any(frame["game"][field] != value for field, value in expected.items()):
            raise RuntimeError(f"Fresh joystick FIRE did not fully reset the {mode_name} fixture")
        finish_capture(scenario, f"restart-{mode_name.lower()}")
    mon.command("jpdb 1 $1f")
    mon.command("log off")
    result["passed"] = True


def attract_checks(mon, syms, failures, elf, reu, dwarf_tool, output, result):
    """Count native simulation calls through a complete, naturally timed tour."""
    layout = dwarf_game_layout(elf, dwarf_tool, syms, output, controls=True)
    fields = layout["fields"]
    for name, kind, size in (("menu_ticks", "u16", 2), ("menu_activity", "bool", 1)):
        if fields.get(name, {}).get("type") != kind:
            raise RuntimeError(f"Unsupported attract Game::{name} field")
        fields[name]["size"] = size
    modes = layout["modes"]
    game_size = syms["__game_state_end"] - layout["address"]
    observed = ("mode", "ticks", "rail_ticks", "menu_ticks", "menu_activity", "score",
                "shield", "lives", "bombs", "boss_started", "boss_defeated", "controls.bits")
    result.update(passed=False, monitor_assisted=True, excluded_from_timing=True, layout=layout,
                  scope="One natural 15-second title, four 15-second maps, and another 15-second title, verified by counting native 20-ms Game::tick calls. Monitor writes establish a clean timer baseline and per-map dismissal fixtures; all dismiss/relaunch input uses joystick port 2.",
                  scenarios=[])
    data = reu.read_bytes()
    segment_base = int.from_bytes(data[12:16], "little")
    references = [data[segment_base+i*2048:segment_base+i*2048+512] for i in range(4)]
    if any(len(part) != 512 or part[:4] != b"SMS1" or part[4] != i
           or fletcher16(part) != int.from_bytes(data[20+i*2:22+i*2], "little")
           for i, part in enumerate(references)):
        raise RuntimeError("Attract checks require four valid packaged segment references")

    def reset_checkpoints():
        mon.command("delete")
        for address in failures:
            mon.command(f"break exec {address:04x}")

    reset_checkpoints()
    path = output / "attract-monitor.log"
    path.unlink(missing_ok=True)
    mon.command(f'logname "{path.as_posix()}"')
    mon.command("log on")
    trace_offset = trace_total = 0
    tracing = False
    tick_candidates = [address for name, address in syms.items()
                       if re.search(r"5havoc4game4Game4tick17h[0-9a-fA-F]+E$", name)]
    if len(tick_candidates) != 1 or "PROF_PHASE" not in syms:
        raise RuntimeError("Attract checks require Game::tick and SIM phase telemetry")
    tick_address = tick_candidates[0]

    def tick_count():
        nonlocal trace_offset, trace_total
        # Flush complete trace records before reading; monitor commands do not
        # advance the emulated clock. The phase condition excludes other code
        # overlays that happen to occupy the same CPU address as Game::tick.
        mon.command("log off")
        with path.open("rb") as log:
            log.seek(trace_offset)
            text = log.read().decode(errors="replace")
            trace_offset = log.tell()
        trace_total += len(re.findall(
            rf"Trace\s+(?:on\s+)?exec\s+\$?{tick_address:04x}\b", text, re.I))
        mon.command("log on")
        return trace_total

    def read_game():
        mon.command("bank ram")
        raw = mon.read(layout["address"], game_size)
        game = {name: int.from_bytes(raw[fields[name]["offset"]:
                                         fields[name]["offset"]+fields[name]["size"]],
                                    "little", signed=fields[name]["type"].startswith("i"))
                for name in observed}
        mon.command("bank cpu")
        return game

    def seed(scenario, values):
        previous = read_game()
        mon.command("bank ram")
        for name, value in values.items():
            field = fields[name]
            address = layout["address"] + field["offset"]
            raw = value.to_bytes(field["size"], "little")
            mon.command(f"> {address:04x} " + " ".join(f"{byte:02x}" for byte in raw))
            scenario.setdefault("monitor_patches", []).append(
                {"field": name, "previous": previous[name], "seeded": value})
        mon.command("bank cpu")
        actual = read_game()
        if any(actual[name] != value for name, value in values.items()):
            raise RuntimeError("Attract monitor fixture did not retain its values")

    def observe(scenario):
        mon.run_until(syms["frame_complete"], failures)
        frame = counters(mon, syms)
        frame["game"] = read_game()
        mon.command("bank ram")
        frame["stack_guards"] = {name: mon.integer(syms[name], 1)
                                 for name in ("__stack_bottom", "__irq_stack_bottom")}
        frame["cache_sections"] = []
        for address in (0x9500, 0x9700):
            cache = mon.read(address, 512)
            if cache[4] >= 4 or cache != references[cache[4]]:
                raise RuntimeError("Attract section cache differs from packaged bytes")
            frame["cache_sections"].append(cache[4])
        mon.command("bank cpu")
        if tracing:
            frame["native_tick_calls"] = tick_count()
        scenario.setdefault("frames", []).append(frame)
        if (frame["input_overflows"] or frame["frame_dropped"]
                or any(value != 0xa5 for value in frame["stack_guards"].values())
                or frame["test_input"] != 255):
            raise RuntimeError("Attract check changed a stack guard, dropped input/geometry, or bypassed physical input")
        section = min(frame["game"]["rail_ticks"] // 3750, 3)
        frame["enemy_bank"] = enemy_bank_check(mon, syms, data, section)
        if (frame["game_mode"] != frame["game"]["mode"]
                or frame.get("render_segment") != section
                or section not in frame["cache_sections"]):
            raise RuntimeError("Attract rendered a stale section or exported an inconsistent mode")
        return frame

    def expect_mode(scenario, mode, section=0):
        for _ in range(4):
            frame = observe(scenario)
            if frame["game_mode"] == modes[mode] and frame["game"]["rail_ticks"] // 3750 == section:
                return frame
        raise RuntimeError(f"{scenario['name']} did not reach {mode}, section {section}")

    def finish(scenario):
        scenario["passed"] = True
        print(f"ATTRACT {scenario['name']}: passed ({len(scenario['frames'])} observed frames)", flush=True)

    # Use the real dismissal path to clear the gameplay fixture left by earlier
    # smoke checks, then let the physical release drain through the input ring.
    setup = {"name": "clean-title-setup"}
    result["setup"] = setup
    mon.command("jpdb 1 $1f")
    observe(setup)
    observe(setup)
    seed(setup, {"mode": modes["Attract"], "rail_ticks": 0,
                 "menu_ticks": 0, "menu_activity": 0})
    mon.command("jpdb 1 $1b")  # LEFT dismisses, without starting the game.
    expect_mode(setup, "Title")
    mon.command("jpdb 1 $1f")
    observe(setup)
    observe(setup)
    seed(setup, {"menu_ticks": 0, "menu_activity": 0})
    finish(setup)

    cycle = {"name": "natural-750-tick-tour-and-repeat", "captures": {},
             "expected_order": ["Title", "Lowlands", "Canyon", "Base", "Carrier", "Title", "Lowlands"]}
    result["scenarios"].append(cycle)
    mon.command(f"trace exec {tick_address:04x} if @cpu:${syms['PROF_PHASE']:04x} == $05")
    tracing = True
    baseline = tick_count()
    for _ in range(1000):
        frame = observe(cycle)
        elapsed = frame["native_tick_calls"] - baseline
        phase, remainder = divmod(elapsed, 750)
        if phase > 6:
            raise RuntimeError("Attract check skipped more than one complete presentation interval")
        section = phase - 1 if 1 <= phase <= 4 else 0
        expected_mode = modes["Title"] if phase in (0, 5) else modes["Attract"]
        expected_rail = 0 if expected_mode == modes["Title"] else section * 3750 + remainder
        game = frame["game"]
        if (game["mode"] != expected_mode or game["menu_ticks"] != remainder
                or game["rail_ticks"] != expected_rail):
            raise RuntimeError(f"Attract 750-tick boundary mismatch after {elapsed} native steps: {game}")
        if any(game[name] != value for name, value in
               (("score", 0), ("shield", 100), ("lives", 3), ("bombs", 3))):
            raise RuntimeError("Attract preview changed player resources or score")
        if expected_mode == modes["Attract"] and remainder >= 250 and str(section) not in cycle["captures"]:
            cycle["captures"][str(section)] = capture(mon, syms, output, f"attract-section-{section}")
            print(f"ATTRACT natural tour: section {section}, {elapsed} native ticks", flush=True)
        if phase == 5 and "title-return" not in cycle["captures"]:
            cycle["captures"]["title-return"] = capture(mon, syms, output, "attract-title-return")
        if phase == 6:
            cycle["native_ticks_observed"] = elapsed
            break
    else:
        raise RuntimeError("Natural attract tour did not repeat within 1000 rendered frames")
    if any(str(section) not in cycle["captures"] for section in range(4)):
        raise RuntimeError("Natural attract tour did not capture every map")
    finish(cycle)
    tracing = False
    reset_checkpoints()

    # Cover every direction and FIRE, including FIRE on the carrier map, while
    # keeping scene setup explicit and all activation/release edges physical.
    for section, bits, label in ((0, 1, "up"), (1, 2, "down"), (2, 4, "left"),
                                 (3, 8, "right"), (0, 16, "fire"), (3, 16, "fire")):
        scenario = {"name": f"section-{section}-{label}-dismiss-and-relaunch"}
        result["scenarios"].append(scenario)
        mon.command("jpdb 1 $1f")
        observe(scenario)
        observe(scenario)
        seed(scenario, {"mode": modes["Attract"], "rail_ticks": section * 3750 + 300,
                        "menu_ticks": 300, "menu_activity": 0})
        expect_mode(scenario, "Attract", section)
        mon.command(f"jpdb 1 ${31 ^ bits:02x}")
        expect_mode(scenario, "Title")
        for _ in range(3):
            held = observe(scenario)
            if held["game_mode"] != modes["Title"] or held["game"]["menu_ticks"] != 0:
                raise RuntimeError("Held attract dismissal input escaped title or advanced its idle timer")
        scenario["title_capture"] = capture(mon, syms, output, f"attract-dismiss-{section}-{label}")
        mon.command("jpdb 1 $1f")
        observe(scenario)
        released = observe(scenario)
        if released["game_mode"] != modes["Title"] or released["game"]["controls.bits"] != 0:
            raise RuntimeError("Releasing attract dismissal input started gameplay")
        mon.command("jpdb 1 $0f")
        launched = expect_mode(scenario, "Playing")
        expected = {"shield": 100, "lives": 3, "bombs": 3, "score": 0,
                    "boss_started": 0, "boss_defeated": 0}
        if any(launched["game"][name] != value for name, value in expected.items()):
            raise RuntimeError("Fresh FIRE after attract did not start a clean mission")
        finish(scenario)
    mon.command("jpdb 1 $1f")
    mon.command("log off")
    result["passed"] = True


def control_checks(mon, syms, failures, elf, dwarf_tool, output, result):
    """Exercise real input sampling between rendered frames, without state writes."""
    layout = dwarf_game_layout(elf, dwarf_tool, syms, output, controls=True)
    result.update(passed=False, excluded_from_timing=True, state_patches=[], layout=layout,
                  scope="Physical joystick-port input, synchronized to accepted raster IRQ samples; no GAME or TEST_INPUT writes.",
                  scenarios=[])
    sample_layout = layout["input_sample"]
    if "irq_entry" not in syms:
        raise RuntimeError("Control checks require the resident irq_entry symbol")
    mon.command("delete")
    for address in failures:
        mon.command(f"break exec {address:04x}")
    path = output / "controls-monitor.log"
    path.unlink(missing_ok=True)
    mon.command(f'logname "{path.as_posix()}"')
    mon.command("log on")

    def observe(scenario):
        mon.run_until(syms["frame_complete"], failures)
        frame = counters(mon, syms)
        mon.command("bank ram")
        frame["game"] = {}
        for name in ("mode", "ticks", "rail_ticks", "player.x", "player.y", "player.z",
                     "controls.bits", "roll_ticks", "roll_cooldown", "roll_direction"):
            field = layout["fields"][name]
            raw = mon.read(layout["address"] + field["offset"], field["size"])
            frame["game"][name] = int.from_bytes(raw, "little", signed=field["type"].startswith("i"))
        frame["stack_guards"] = {name: mon.integer(syms[name], 1)
                                 for name in ("__stack_bottom", "__irq_stack_bottom")}
        mon.command("bank cpu")
        scenario.setdefault("frames", []).append(frame)
        if frame["input_overflows"] or frame["frame_dropped"] or any(v != 0xa5 for v in frame["stack_guards"].values()):
            raise RuntimeError("Control check dropped input/geometry or changed a stack guard")
        if frame["test_input"] != 255 or frame["game"]["mode"] != layout["modes"]["Playing"]:
            raise RuntimeError("Control check lost physical-input mode or active gameplay")
        if "render_segment" in frame and frame["render_segment"] != active_section(frame["game"]["rail_ticks"], frame["game_mode"]):
            raise RuntimeError("Control check rendered a stale segment")
        return frame

    def pulse(scenario, bits, count=1):
        mon.command(f"jpdb 1 ${31 ^ bits:02x}")
        for _ in range(count):
            head = mon.integer(sample_layout["head_address"], 1)
            # Stopping at the *next* IRQ entry sees the preceding IRQ complete,
            # including its ring-buffer write. Timer-only interrupts are skipped.
            for _ in range(8):
                mon.run_until(syms["irq_entry"], failures)
                next_head = mon.integer(sample_layout["head_address"], 1)
                if next_head != head:
                    break
            else:
                raise RuntimeError("No accepted joystick sample within eight IRQs")
            if (next_head - head) & 63 != 1:
                raise RuntimeError("Control pulse skipped an input-ring sample")
            address = sample_layout["address"] + ((next_head - 1) & 63) * sample_layout["size"]
            raw = mon.read(address, sample_layout["size"])
            sample = {name: int.from_bytes(raw[field["offset"]:field["offset"]+field["size"]], "little")
                      for name, field in sample_layout["fields"].items()}
            sample.update(requested_bits=bits, ring_head=next_head)
            scenario.setdefault("samples", []).append(sample)
            if sample["bits"] != bits:
                raise RuntimeError(f"Physical joystick sample {sample['bits']} differs from requested {bits}")

    def finish(scenario, capture_name):
        scenario["capture"] = capture(mon, syms, output, capture_name)
        scenario["passed"] = True
        print(f"CONTROL {scenario['name']}: passed", flush=True)

    setup = {"name": "release-input"}
    mon.command("jpdb 1 $1f")
    observe(setup)
    baseline = observe(setup)
    result["setup"] = setup
    for name, bits, direction in (("up-climbs-world-y", 1, 1), ("down-dives-world-y", 2, -1)):
        scenario = {"name": name, "before": baseline["game"]}
        result["scenarios"].append(scenario)
        pulse(scenario, bits, 6)
        pulse(scenario, 0)
        observe(scenario)
        baseline = observe(scenario)
        delta = baseline["game"]["player.y"] - scenario["before"]["player.y"]
        scenario["world_y_displacement"] = delta
        if delta * direction <= 0 or baseline["game"]["controls.bits"] != 0:
            raise RuntimeError(f"{name} did not produce the expected vertical movement")
        finish(scenario, "controls-" + name)

    for label, bits, direction in (("left", 4, -1), ("right", 8, 1)):
        scenario = {"name": f"double-{label}-dodge", "before": baseline["game"]}
        result["scenarios"].append(scenario)
        pulse(scenario, bits)
        # Twenty neutral display samples put the second edge around 350 ms
        # on NTSC or 420 ms on PAL, exercising the newly extended window.
        pulse(scenario, 0, 20)
        pulse(scenario, bits)
        pulse(scenario, 0)
        edges = [sample for sample in scenario["samples"] if sample["pressed"] & bits]
        if len(edges) != 2:
            raise RuntimeError(f"Double-{label} did not capture two physical press edges")
        gap = (edges[1]["ms"] - edges[0]["ms"]) & 0xffffffff
        scenario["double_tap_gap_ms"] = gap
        if not 250 < gap <= 500:
            raise RuntimeError(f"Double-{label} gesture must exceed 250 ms and stay within 500 ms: {gap}")
        for _ in range(4):
            active = observe(scenario)
            if active["game"]["roll_ticks"] > 0 and active["game"]["controls.bits"] == 0:
                break
        else:
            raise RuntimeError(f"Double-{label} did not activate a roll with released input")
        game = active["game"]
        if game["roll_direction"] != direction or not 10 < game["roll_ticks"] <= 25:
            raise RuntimeError(f"Double-{label} roll direction or initial countdown is invalid")
        if (game["player.x"] - scenario["before"]["player.x"]) * direction <= 0:
            raise RuntimeError(f"Double-{label} did not displace the player in the tapped direction")
        scenario["active_capture"] = capture(mon, syms, output, f"controls-{label}-dodge-active")
        released = observe(scenario)
        drift = released["game"]["player.x"] - game["player.x"]
        scenario["released_input_displacement"] = drift
        if released["game"]["controls.bits"] != 0 or drift * direction <= 0:
            raise RuntimeError(f"Double-{label} has no physical dodge displacement after release")
        previous = active
        for _ in range(8):
            elapsed = (released["game"]["ticks"] - previous["game"]["ticks"]) & 0xffff
            if released["game"]["roll_ticks"] != max(0, previous["game"]["roll_ticks"] - elapsed):
                raise RuntimeError(f"Double-{label} roll did not expire at its simulation-tick deadline")
            if released["game"]["roll_cooldown"] != max(0, previous["game"]["roll_cooldown"] - elapsed):
                raise RuntimeError(f"Double-{label} cooldown did not count down correctly")
            if released["game"]["roll_ticks"] == 0 and released["game"]["roll_cooldown"] == 0:
                break
            previous = released
            released = observe(scenario)
        else:
            raise RuntimeError(f"Double-{label} roll/cooldown did not expire")
        baseline = released
        scenario["world_x_displacement"] = baseline["game"]["player.x"] - scenario["before"]["player.x"]
        finish(scenario, f"controls-{label}-dodge-expired")
    mon.command("jpdb 1 $1f")
    mon.command("log off")
    result["passed"] = True


def weapon_checks(mon, syms, failures, elf, dwarf_tool, output, result, video):
    """Seed encounter fixtures; exercise damage, charging and FIRE through game code."""
    layout = dwarf_game_layout(elf, dwarf_tool, syms, output, controls=True)
    scalar_sizes = {"u8": 1, "i8": 1, "bool": 1, "u16": 2, "i16": 2, "u32": 4}
    structs = {}
    for name in ("Actor", "Shot", "Pickup", "Explosion", "Renderer", "Triangle", "Vec3", "Controls", "BombEffect"):
        description = subprocess.check_output(
            [str(dwarf_tool), f"--name={name}", "--show-children", str(elf)], text=True)
        (output / f"dwarf-{name.lower()}.txt").write_text(description)
        size = re.search(r"DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)", description)
        if size is None:
            raise RuntimeError(f"Missing {name} DWARF byte size")
        structs[name] = {"size": int(size[1], 0), "fields": dwarf_members(description, name)}

    fields = layout["fields"]
    for name in ("invulnerability", "hit_flash_ticks", "spawn_cursor", "spawn_segment", "bank", "kills", "bomb_kills"):
        field = fields.get(name)
        if not field or field["type"] not in scalar_sizes:
            raise RuntimeError(f"Unsupported weapon-fixture Game::{name}")
        field["size"] = scalar_sizes[field["type"]]
    charge = structs["Controls"]["fields"].get("cached_charge")
    if charge is None or charge["type"] != "u8":
        raise RuntimeError("Missing Controls::cached_charge byte")
    fields["controls.cached_charge"] = dict(charge, size=1,
        offset=fields["controls"]["offset"] + charge["offset"])
    arrays = {}
    for field_name, struct_name in (("actors", "Actor"), ("shots", "Shot"),
                                     ("pickups", "Pickup"), ("explosions", "Explosion")):
        field = fields[field_name]
        match = re.fullmatch(rf"havoc::game::{struct_name}\[(\d+)\]", field["type"])
        if match is None:
            raise RuntimeError(f"Unsupported Game::{field_name} array type")
        arrays[field_name] = dict(structs[struct_name], count=int(match[1]),
                                 address=layout["address"] + field["offset"])
        array = arrays[field_name]
        if array["address"] + array["size"] * array["count"] > syms["__game_state_end"]:
            raise RuntimeError(f"Weapon fixture {field_name} exceeds reserved Game state")
        if array["fields"]["pos"]["type"] != "havoc::Vec3":
            raise RuntimeError(f"Unsupported {struct_name}::pos type")
        for axis in ("x", "y", "z"):
            component = structs["Vec3"]["fields"][axis]
            if component["type"] != "i16":
                raise RuntimeError("Unsupported Vec3 component type")
            array["fields"][f"pos.{axis}"] = dict(component,
                offset=array["fields"]["pos"]["offset"] + component["offset"])
    renderer = structs["Renderer"]
    renderer["address"] = resident_symbol(syms, "RENDERER")[1]
    triangle = structs["Triangle"]
    if (renderer["fields"]["triangle_count"]["type"] != "u16"
            or renderer["fields"]["triangles"]["type"] != "havoc::render::Triangle[256]"
            or any(triangle["fields"][axis]["type"] != "i16[3]" for axis in ("x", "y"))
            or triangle["fields"]["depth"]["type"] != "i16"
            or triangle["fields"]["color"]["type"] != "u8"):
        raise RuntimeError("Unsupported Renderer/Triangle DWARF layout")
    effect = structs["BombEffect"]
    effect["address"] = resident_symbol(syms, "BOMB_EFFECT")[1]
    if (effect["size"] != 1 or effect["fields"].get("remaining") != {"offset": 0, "type": "u8"}
            or "irq_entry" not in syms):
        raise RuntimeError("Bomb checks require the one-byte BombEffect and resident irq_entry")
    result.update(passed=False, monitor_assisted=True, excluded_from_timing=True,
                  scope="Seeded encounters and cleared actors/projectiles; damage runs through hostile-shot collision. Charge timers, missiles and bombs use real elapsed time and physical joystick FIRE. No hit-flash, bomb-effect timer or shot-pending writes.",
                  layout=layout, arrays=arrays, renderer=renderer, triangle=triangle, bomb_effect=effect, scenarios=[])

    def read_field(name):
        field = fields[name]
        return int.from_bytes(mon.read(layout["address"] + field["offset"], field["size"]),
                              "little", signed=field["type"].startswith("i"))

    def write_bytes(scenario, label, address, raw):
        previous = mon.read(address, len(raw))
        # The text monitor has a short command buffer; never send a whole
        # projectile array as one line, even though its memory range is valid.
        for offset in range(0, len(raw), 16):
            mon.command(f"> {address+offset:04x} " + " ".join(f"{byte:02x}" for byte in raw[offset:offset+16]))
        if mon.read(address, len(raw)) != raw:
            raise RuntimeError(f"Weapon fixture write failed: {label}")
        scenario.setdefault("monitor_patches", []).append({"field": label, "address": address,
            "length": len(raw), "previous_hex": previous.hex(), "seeded_hex": raw.hex()})

    def seed(scenario, values=None, clear=()):
        mon.command("bank ram")
        for name, value in (values or {}).items():
            field = fields[name]
            write_bytes(scenario, name, layout["address"] + field["offset"],
                        value.to_bytes(field["size"], "little", signed=field["type"].startswith("i")))
        for name in clear:
            array = arrays[name]
            write_bytes(scenario, name, array["address"], bytes(array["size"] * array["count"]))
        mon.command("bank cpu")

    def seed_element(scenario, name, index, values):
        array = arrays[name]
        if not 0 <= index < array["count"]:
            raise RuntimeError(f"Invalid {name} fixture index")
        mon.command("bank ram")
        for member, value in values.items():
            field = array["fields"][member]
            size = scalar_sizes[field["type"]]
            write_bytes(scenario, f"{name}[{index}].{member}",
                        array["address"] + index * array["size"] + field["offset"],
                        value.to_bytes(size, "little", signed=field["type"].startswith("i")))
        mon.command("bank cpu")

    def read_array(name):
        array = arrays[name]
        raw = mon.read(array["address"], array["size"] * array["count"])
        return [{member: int.from_bytes(raw[index*array["size"]+field["offset"]:
                     index*array["size"]+field["offset"]+scalar_sizes[field["type"]]],
                     "little", signed=field["type"].startswith("i"))
                 for member, field in array["fields"].items() if field["type"] in scalar_sizes}
                for index in range(array["count"])]

    def read_triangles():
        count = mon.integer(renderer["address"] + renderer["fields"]["triangle_count"]["offset"])
        if count > 256:
            raise RuntimeError("Invalid native triangle count")
        raw = mon.read(renderer["address"] + renderer["fields"]["triangles"]["offset"], count*triangle["size"])
        commands = []
        for index in range(count):
            data = raw[index*triangle["size"]:(index+1)*triangle["size"]]
            command = {}
            for axis in ("x", "y"):
                offset = triangle["fields"][axis]["offset"]
                command[axis] = list(struct.unpack_from("<3h", data, offset))
            command["depth"] = struct.unpack_from("<h", data, triangle["fields"]["depth"]["offset"])[0]
            command["color"] = data[triangle["fields"]["color"]["offset"]]
            commands.append(command)
        return commands

    observed_fields = ("mode", "ticks", "rail_ticks", "player.x", "player.y", "player.z", "shield",
                       "invulnerability", "hit_flash_ticks", "score", "kills", "bomb_kills", "bombs",
                       "controls.bits", "controls.cached_charge")

    def observe(scenario, geometry=False):
        commands = None
        if geometry and "geometry_complete" in syms:
            mon.run_until(syms["geometry_complete"], failures)
            mon.command("bank ram")
            commands = read_triangles()
            mon.command("bank cpu")
        mon.run_until(syms["frame_complete"], failures)
        frame = counters(mon, syms)
        mon.command("bank ram")
        frame["game"] = {name: read_field(name) for name in observed_fields}
        frame["actors"], frame["shots"] = read_array("actors"), read_array("shots")
        frame["stack_guards"] = {name: mon.integer(syms[name], 1)
                                 for name in ("__stack_bottom", "__irq_stack_bottom")}
        if geometry:
            frame["triangles"] = commands if commands is not None else read_triangles()
        mon.command("bank cpu")
        scenario.setdefault("frames", []).append(frame)
        if (frame["input_overflows"] or frame["frame_dropped"]
                or any(value != 0xa5 for value in frame["stack_guards"].values())
                or frame["test_input"] != 255 or frame["game"]["mode"] != layout["modes"]["Playing"]):
            raise RuntimeError("Weapon fixture lost gameplay/physical input, dropped data, or damaged a stack guard")
        return frame

    def finish(scenario):
        scenario["passed"] = True
        print(f"WEAPON {scenario['name']}: passed ({len(scenario.get('frames', []))} observed frames)", flush=True)

    mon.command("delete")
    for address in failures:
        mon.command(f"break exec {address:04x}")
    path = output / "weapons-monitor.log"
    path.unlink(missing_ok=True)
    mon.command(f'logname "{path.as_posix()}"')
    mon.command("log on")
    mon.command("jpdb 1 $1f")
    setup = {"name": "isolated-encounter-setup"}
    result["setup"] = setup
    observe(setup)
    observe(setup)
    seed(setup, {"rail_ticks": 0, "spawn_segment": 0, "spawn_cursor": 255,
                 "player.x": 0, "player.y": 0, "bank": 0, "shield": 100, "score": 0, "kills": 0, "bomb_kills": 0,
                 "roll_ticks": 0, "roll_cooldown": 0, "invulnerability": 0,
                 "boss_started": 0, "boss_defeated": 0}, clear=("actors", "shots", "pickups", "explosions"))

    flash = {"name": "actual-hit-white-hull-flashes", "captures": []}
    result["scenarios"].append(flash)
    baselines = [observe(flash, geometry=True) for _ in range(3)]
    def signature(command):
        return tuple(command["x"] + command["y"] + [command["depth"]])
    # Separate the centered exhaust by its screen region: a frame interval of
    # eight ticks can alias its animation, so frame intersection alone cannot
    # identify it. The orange rear hull face extends above this region and must
    # still be checked for a white flash.
    def exhaust_region(command):
        return (command["depth"] < 160
                and all(60 <= x <= 100 for x in command["x"])
                and all(135 <= y <= 175 for y in command["y"]))
    # Intersect the remaining near-depth geometry to discard moving terrain.
    def candidates(frame):
        return {signature(command): command["color"] for command in frame["triangles"]
                if 150 <= command["depth"] <= 220
                and all(40 <= x <= 120 for x in command["x"])
                and all(100 <= y <= 175 for y in command["y"])
                and not exhaust_region(command)}
    normal = candidates(baselines[0])
    for frame in baselines[1:]:
        current = candidates(frame)
        normal = {key: color for key, color in normal.items() if current.get(key) == color}
    if len(normal) < 4 or not any(color != 1 for color in normal.values()):
        raise RuntimeError("Cannot identify the stable centered hull from native baseline geometry")
    flash["hull_reference"] = [{"signature": key, "color": color} for key, color in normal.items()]
    # A genuine hostile projectile crosses the player on the next simulation
    # tick; damage_player owns all immunity and flash-timer changes.
    seed_element(flash, "shots", 0, {"kind": 2, "damage": 10, "dx": 0, "dy": 0,
                                    "pos.x": 0, "pos.y": 0, "pos.z": 198})
    colors_seen = set()
    pixel_counts = {"white": [], "normal": []}
    left = min(key[i] for key in normal for i in range(3))
    right = max(key[i] for key in normal for i in range(3))
    top = min(key[i] for key in normal for i in range(3, 6))
    bottom = max(key[i] for key in normal for i in range(3, 6))
    for index in range(8):
        frame = observe(flash, geometry=True)
        timer = frame["game"]["hit_flash_ticks"]
        if frame["game"]["shield"] != 90:
            raise RuntimeError("Seeded hostile shot did not cause exactly one real ten-point hit")
        white = timer > 0 and (35 - timer) // 10 % 2 == 0
        current = {signature(command): command["color"] for command in frame["triangles"]}
        if any(current.get(key) != (1 if white else color) for key, color in normal.items()):
            raise RuntimeError("Hit flash hid hull geometry or did not preserve the expected white/normal colors")
        exhaust = [command for command in frame["triangles"] if command["color"] == 8
                   and exhaust_region(command)]
        if len(exhaust) != 2:
            raise RuntimeError("Hit flash hid or recolored the two animated exhaust triangles")
        phase = "white" if white else "normal"
        shot = capture(mon, syms, output, f"weapons-hit-{index}-{phase}")
        native = Path(shot["native"]).read_bytes()
        white_pixels = 0
        for y in range(top, bottom + 1):
            for x in range(left, right + 1):
                cell = y // 8 * 40 + x // 4
                slot = native[cell*8+y%8] >> (6 - 2*(x%4)) & 3
                screen = native[8000+cell]
                color = (shot["background"], screen >> 4, screen & 15, native[9000+cell])[slot]
                white_pixels += color == 1
        shot.update(phase=phase, timer=timer, hull_commands=len(normal), exhaust_commands=len(exhaust), hull_bounds=[left, top, right, bottom],
                    white_pixels_in_hull_bounds=white_pixels)
        flash["captures"].append(shot)
        pixel_counts[phase].append(white_pixels)
        if timer:
            colors_seen.add(phase)
        if timer == 0:
            break
    if colors_seen != {"white", "normal"} or not pixel_counts["white"] or max(pixel_counts["white"]) <= max(pixel_counts["normal"]):
        raise RuntimeError("Native hit sequence did not expose both visible flash phases with increased white pixels")
    if flash["frames"][-1]["game"]["hit_flash_ticks"] != 0:
        raise RuntimeError("Hit flash did not expire")
    finish(flash)

    def await_charge(scenario):
        mon.command("jpdb 1 $1f")
        for _ in range(25):
            frame = observe(scenario)
            if frame["game"]["controls.cached_charge"] == 255 and frame["game"]["controls.bits"] == 0:
                scenario["fully_charged"] = frame["game"]
                return
        raise RuntimeError("Released physical FIRE did not fully recharge within 25 rendered frames")

    def launch(scenario):
        mon.command("jpdb 1 $0f")
        for _ in range(3):
            frame = observe(scenario)
            active = [(index, shot) for index, shot in enumerate(frame["shots"]) if shot["kind"] == 3]
            if active:
                if len(active) != 1 or active[0][1]["damage"] != 4 or frame["game"]["controls.cached_charge"] != 0:
                    raise RuntimeError("Fully charged FIRE did not launch exactly one four-damage missile and consume charge")
                scenario["launch_capture"] = capture(mon, syms, output, scenario["name"] + "-launch")
                mon.command("jpdb 1 $1f")
                return frame, active[0][0]
        raise RuntimeError("Fresh physical FIRE did not launch a fully charged missile")

    impact = {"name": "homing-missile-tracks-and-hits"}
    result["scenarios"].append(impact)
    await_charge(impact)
    seed(impact, {"kills": 4, "bomb_kills": 4, "bombs": 0},
         clear=("actors", "shots", "pickups", "explosions"))
    seed_element(impact, "actors", 0, {"kind": 3, "hp": 4, "flags": 0, "age": 0,
                 "origin_x": 100, "origin_y": 20, "pos.x": 100, "pos.y": 20, "pos.z": 1400})
    launched, slot = launch(impact)
    if launched["shots"][slot]["pos.x"] <= 0 or launched["shots"][slot]["dx"] <= 0:
        raise RuntimeError("Missile did not turn toward the off-axis turret")
    for _ in range(8):
        frame = observe(impact)
        if frame["actors"][0]["kind"] == 0:
            break
    else:
        raise RuntimeError("Homing missile failed to intercept the off-axis turret")
    if frame["game"]["score"] != launched["game"]["score"] + 250 or frame["shots"][slot]["kind"] != 0:
        raise RuntimeError("Missile impact did not consume the missile and award exactly one turret kill")
    reward = {"kills": 5, "bomb_kills": 0, "bombs": 1}
    if any(frame["game"][name] != expected for name, expected in reward.items()):
        raise RuntimeError("Fifth enemy kill did not immediately award one bomb and reset reward progress")
    impact["five_kill_bomb_reward"] = reward
    impact["impact_capture"] = capture(mon, syms, output, "weapons-missile-impact")
    finish(impact)

    missing = {"name": "homing-missile-without-target"}
    result["scenarios"].append(missing)
    await_charge(missing)
    seed(missing, clear=("actors", "shots", "pickups", "explosions"))
    previous, slot = launch(missing)
    initial_score = previous["game"]["score"]
    for _ in range(12):
        shot = previous["shots"][slot]
        if shot["kind"] != 3 or any(shot[field] != 0 for field in ("pos.x", "pos.y", "dx", "dy")):
            raise RuntimeError("A targetless missile did not retain a straight trajectory")
        frame = observe(missing)
        current = frame["shots"][slot]
        if current["kind"] == 0:
            if current["pos.z"] <= 2400 or any(current[field] != 0 for field in ("pos.x", "pos.y", "dx", "dy")):
                raise RuntimeError("Targetless missile expired before crossing its forward world bound")
            break
        if current["kind"] != 3 or current["pos.z"] <= shot["pos.z"]:
            raise RuntimeError("Targetless missile failed to advance")
        previous = frame
    else:
        raise RuntimeError("Targetless missile did not expire at its forward world bound")
    if frame["game"]["score"] != initial_score:
        raise RuntimeError("Targetless missile unexpectedly changed score")
    missing["expired_capture"] = capture(mon, syms, output, "weapons-missile-no-target-expired")
    finish(missing)

    # Keep this separate from rendered-frame observations: a bomb's complete
    # impulse can run between a handful of frame publications. Read the exact
    # resident countdown from DWARF and trace every actual VIC register write.
    sample_layout = layout["input_sample"]
    bomb_samples = ((6, 6, 1), (0, 0, 1), (6, 1, 7), (0, 6, 7),
                    (4, 5, 8), (0, 1, 8), (4, 2, 2), (0, 5, 2),
                    (2, 4, 0), (0, 2, 0), (2, 2, 0), (0, 4, 0),
                    (2, 3, 0), (0, 2, 0), (2, 3, 0), (0, 4, 0))
    bus_hz = 985248 if video == "pal" else 1022727
    video_ms = 1000 * (19656 if video == "pal" else 17095) / bus_hz

    def pulse_fire(scenario, bits, count=1):
        mon.command(f"jpdb 1 ${31 ^ bits:02x}")
        for _ in range(count):
            head = mon.integer(sample_layout["head_address"], 1)
            for _ in range(8):
                mon.run_until(syms["irq_entry"], failures)
                next_head = mon.integer(sample_layout["head_address"], 1)
                if next_head != head:
                    break
            else:
                raise RuntimeError("Bomb gesture missed an accepted raster input sample")
            if (next_head - head) & 63 != 1:
                raise RuntimeError("Bomb gesture skipped an input-ring sample")
            address = sample_layout["address"] + ((next_head - 1) & 63) * sample_layout["size"]
            raw = mon.read(address, sample_layout["size"])
            sample = {name: int.from_bytes(raw[field["offset"]:field["offset"]+field["size"]], "little")
                      for name, field in sample_layout["fields"].items()}
            if sample["bits"] != bits:
                raise RuntimeError("Bomb gesture did not reach the physical joystick input ring")
            scenario.setdefault("samples", []).append(sample)

    def double_fire(scenario):
        start = len(scenario.get("samples", []))
        for bits in (16, 0, 16, 0):
            pulse_fire(scenario, bits)
        edges = [sample for sample in scenario["samples"][start:] if sample["pressed"] & 16]
        if len(edges) != 2 or not 0 < (edges[1]["ms"] - edges[0]["ms"]) & 0xffffffff <= 500:
            raise RuntimeError("Bomb requires two real FIRE edges within the double-tap window")

    def observe_effect(scenario):
        mon.run_until(syms["irq_entry"], failures)
        state = {"remaining": mon.integer(effect["address"], 1),
                 "audio_ticks": mon.integer(syms["AUDIO_TICKS"]),
                 "front_bank": mon.integer(syms["FRONT_BANK"], 1),
                 "frame_serial": mon.integer(syms["FRAME_SERIAL"])}
        mon.command("bank io")
        state.update(x=mon.integer(0xd016, 1) & 7, y=mon.integer(0xd011, 1) & 7,
                     border=mon.integer(0xd020, 1) & 15)
        mon.command("bank cpu")
        scenario.setdefault("irq_samples", []).append(state)
        return state

    def await_effect(scenario, predicate):
        for _ in range(120):
            state = observe_effect(scenario)
            if predicate(state):
                return state
        raise RuntimeError("Bomb effect did not reach its expected countdown state within 120 IRQs")

    def verify_bomb_trace(scenario, trace_path, expected_triggers, initial_bank):
        mon.command("log off")  # Flush complete records before parsing.
        events = stores(trace_path, {effect["address"]: 0})
        remaining, applied, bank, publications = 0, (0, 3, 0), initial_bank, 0
        triggers, durations, phases = [], [], set()
        changing_between_frames = shaken_publications = 0
        previous_motion = None
        for event in events:
            address, value = event["address"], event["value"]
            if address == effect["address"]:
                if value == 32:
                    triggers.append(event["bus_cycles"])
                elif value != max(0, remaining - 1):
                    raise RuntimeError(f"Bomb countdown changed from {remaining} to {value} without a trigger")
                if value == 0 and remaining > 0:
                    durations.append(1000 * (event["bus_cycles"] - triggers[-1]) / bus_hz)
                remaining = value
            elif address == 0xd016:
                applied = bomb_samples[(32-remaining)//2] if remaining else (0, 3, 0)
                if value != 0x18 | applied[0]:
                    raise RuntimeError("Raster bomb X scroll differs from the native countdown phase")
                if remaining:
                    phases.add((32-remaining)//2)
                    if previous_motion and previous_motion[0] == publications and previous_motion[1] != applied[:2]:
                        changing_between_frames += 1
                    previous_motion = (publications, applied[:2])
            elif address == 0xd011:
                if value != 0x38 | applied[1]:
                    raise RuntimeError("Raster/publication changed bomb Y scroll, bitmap/display mode, or IRQ compare bit")
            elif address == 0xd020:
                if value != applied[2]:
                    raise RuntimeError("Bomb border differs from its white/yellow/orange/red/black phase")
            elif address == syms["FRONT_BANK"]:
                if value not in (2, 3) or value == bank:
                    raise RuntimeError("Bomb effect interrupted alternating bitmap publication")
                bank, publications = value, publications + 1
                shaken_publications += applied[:2] != (0, 3)
        if len(triggers) != expected_triggers or remaining != 0 or applied != (0, 3, 0):
            raise RuntimeError("Bomb trigger count or final neutral display state is incorrect")
        if expected_triggers:
            if (len(durations) != 1 or abs(durations[0] - 640) > 20 + video_ms
                    or phases != set(range(16)) or not changing_between_frames or not shaken_publications):
                raise RuntimeError("Bomb duration, complete shake phases, between-frame motion, or active bank publication failed")
        elif phases or durations:
            raise RuntimeError("Empty-inventory double FIRE activated bomb presentation")
        scenario["presentation"] = {"trace": str(trace_path), "triggers": len(triggers),
            "completed_duration_ms": durations, "duration_tolerance_ms": 20 + video_ms,
            "shake_phases": sorted(phases), "motion_changes_between_publications": changing_between_frames,
            "shaken_bank_publications": shaken_publications, "final_scroll_and_border": applied}

    mon.command("log off")
    for address in (0xd011, 0xd016, 0xd020):
        mon.command(f"trace store {address:04x} if @cpu:$01 == $35")
    for address in (effect["address"], syms["FRONT_BANK"]):
        mon.command(f"trace store {address:04x}")
    for name, inventory, retrigger in (("bomb-display-impulse", 1, False),
                                        ("bomb-display-retrigger", 2, True),
                                        ("bomb-empty-inventory", 0, False)):
        scenario = {"name": name}
        result["scenarios"].append(scenario)
        # Let any earlier FIRE edge age out through actual released samples.
        pulse_fire(scenario, 0, 35)
        observe(scenario)
        seed(scenario, {"bombs": inventory}, clear=("actors", "shots", "pickups", "explosions"))
        initial_bank = mon.integer(syms["FRONT_BANK"], 1)
        if mon.integer(effect["address"], 1) != 0:
            raise RuntimeError("Bomb scenario did not start with a neutral presentation")
        trace_path = output / f"weapons-{name}-monitor.log"
        trace_path.unlink(missing_ok=True)
        mon.command(f'logname "{trace_path.as_posix()}"')
        mon.command("log on")
        double_fire(scenario)
        if inventory:
            await_effect(scenario, lambda state: 24 <= state["remaining"] <= 30)
            scenario["active_capture"] = capture(mon, syms, output, f"weapons-{name}-active")
            if retrigger:
                await_effect(scenario, lambda state: 0 < state["remaining"] <= 24)
                double_fire(scenario)
                await_effect(scenario, lambda state: state["remaining"] >= 28)
            await_effect(scenario, lambda state: state["remaining"] == 0)
        else:
            first = observe_effect(scenario)
            await_effect(scenario, lambda state: ((state["audio_ticks"] - first["audio_ticks"]) & 0xffff) >= 36)
        pulse_fire(scenario, 0, 2)  # Publish idle registers after the final CIA tick.
        final = observe(scenario)
        if final["game"]["bombs"] != 0:
            raise RuntimeError("Physical bomb gesture did not spend the expected inventory")
        verify_bomb_trace(scenario, trace_path, 2 if retrigger else int(inventory > 0), initial_bank)
        scenario["expired_capture"] = capture(mon, syms, output, f"weapons-{name}-expired")
        finish(scenario)
    mon.command("jpdb 1 $1f")
    mon.command("log off")
    result["passed"] = True


def music_checks(mon, syms, failures, elf, reu, dwarf_tool, output, result, sid_count, video):
    """Run every complete cue with native IRQ playback and foreground refills."""
    required = ("MUSIC_CUE", "MUSIC_PAGE", "MUSIC_UNDERRUNS", "AUDIO_TICKS")
    if any(name not in syms for name in required):
        raise RuntimeError("Music checks require exported cue, page, underrun and audio-tick diagnostics")
    layout = dwarf_game_layout(elf, dwarf_tool, syms, output, controls=True)
    fields, modes = layout["fields"], layout["modes"]
    if fields.get("menu_ticks", {}).get("type") != "u16":
        raise RuntimeError("Music fixtures require the DWARF Game::menu_ticks field")
    fields["menu_ticks"]["size"] = 2
    data = reu.read_bytes()
    header = data[0x60000:0x60080]
    if header[:4] != b"SMU2" or header[4] != sid_count or header[6] != 7:
        raise RuntimeError("Music checks require a matching seven-cue SMU2 bank")
    page_size = int.from_bytes(header[12:14], "little")
    names = ("title", "lowlands", "canyon", "fortress", "carrier", "victory", "defeat")
    chips = ("left_d400", "right_d420", "socket1_d440", "socket2_d460")[:sid_count]
    bus_hz = 985248 if video == "pal" else 1022727
    result.update(passed=False, monitor_assisted=True, excluded_from_timing=True,
                  scope="All seven cues play one complete native loop plus a page while rendering continues. DWARF-derived game fixtures hold each area/results mode; lowlands uses physical held FIRE for both effects voices.",
                  sid_count=sid_count, scenarios=[], layout=layout)

    def patch(values):
        mon.command("bank ram")
        for name, value in values.items():
            field = fields[name]
            address = layout["address"] + field["offset"]
            raw = value.to_bytes(field["size"], "little", signed=field["type"].startswith("i"))
            mon.command(f"> {address:04x} " + " ".join(f"{byte:02x}" for byte in raw))
        mon.command("bank cpu")

    def observe():
        mon.run_until(syms["frame_complete"], failures)
        state = {name.lower(): mon.integer(syms[name], width)
                 for name, width in (("MUSIC_CUE", 1), ("MUSIC_PAGE", 1), ("MUSIC_UNDERRUNS", 2),
                                     ("AUDIO_TICKS", 2), ("INPUT_OVERFLOWS", 2), ("FRAME_DROPPED", 2))}
        if "MUSIC_ACTIVE_CUE" in syms:
            state["music_active_cue"] = mon.integer(syms["MUSIC_ACTIVE_CUE"], 1)
        if state["music_underruns"] or state["input_overflows"] or state["frame_dropped"]:
            raise RuntimeError(f"Native music fixture underrun or lost frame/input: {state}")
        return state

    # The preceding gameplay smoke already establishes normal timing. This
    # suite has its own traces so monitor fixtures cannot contaminate it.
    mon.command("delete")
    for address in failures:
        mon.command(f"break exec {address:04x}")
    for sid in range(sid_count):
        base = 0xd400 + 32 * sid
        mon.command(f"trace store {base:04x} {base+24:04x} if @cpu:$01 == $35")
    # Trace unmapped enhancement addresses too: the standard edition must never
    # touch them, even when hardware would mirror those writes onto SID one.
    if sid_count == 2:
        mon.command("trace store d440 d478 if @cpu:$01 == $35")
    mon.command("jpdb 1 $1f")
    observe()
    observe()
    for cue, name in enumerate(names):
        at = 16 + cue * 8
        offset = int.from_bytes(header[at:at+4], "little")
        pages, row_ticks = header[at+4], header[at+5]
        if not pages or not 3 <= row_ticks <= 50:
            raise RuntimeError(f"Invalid {name} score descriptor")
        loop_ticks = pages * 16 * row_ticks
        if cue == 0:
            fixture = {"mode": modes["Title"], "rail_ticks": 0, "menu_ticks": 0}
        elif cue < 5:
            fixture = {"mode": modes["Playing"] if cue == 1 else modes["Attract"],
                       "rail_ticks": (cue-1)*3750+100, "menu_ticks": 0, "shield": 100, "lives": 3,
                       "player.x": 0}
        else:
            fixture = {"mode": modes["Victory"] if cue == 5 else modes["GameOver"],
                       "rail_ticks": 11400, "menu_ticks": 400 if cue == 5 else 0}
        scenario = {"name": name, "cue": cue, "pages": pages, "row_ticks": row_ticks,
                    "loop_ticks": loop_ticks, "loop_seconds": loop_ticks/50,
                    "fixture": fixture, "page_transitions": [], "observed_frames": 0}
        result["scenarios"].append(scenario)
        mon.command("jpdb 1 $1f")
        observe()
        observe()
        patch(fixture)
        path = output / f"music-{name}-monitor.log"
        path.unlink(missing_ok=True)
        mon.command(f'logname "{path.as_posix()}"')
        mon.command("log on")
        if cue == 1:
            mon.command("jpdb 1 $0f")
        # Allow the foreground to select/load the opening and the IRQ to
        # consume it. Repeated cue selection must preserve that transport.
        first = None
        for _ in range(8):
            patch(fixture)
            candidate = observe()
            if (candidate["music_cue"] == cue and candidate.get("music_active_cue", cue) == cue
                    and candidate["music_page"] == 0):
                first = candidate
                break
        if first is None:
            raise RuntimeError(f"{name} did not begin on its opening page")
        start_cycles = mon.registers()["bus_cycles"]
        previous_page, wraps = 0, 0
        seen = {0}
        scenario["page_transitions"].append(dict(first, elapsed_ticks=0))
        # A complete loop plus one page ensures the turnaround and continued
        # prefetch after wrapping are both exercised for every composition.
        for _ in range(6000):
            patch(fixture)
            frame = observe()
            scenario["observed_frames"] += 1
            scenario["last_observation"] = frame
            if frame["music_cue"] != cue or frame.get("music_active_cue", cue) != cue:
                raise RuntimeError(f"{name} changed cue unexpectedly during native playback")
            page = frame["music_page"]
            elapsed = (frame["audio_ticks"] - first["audio_ticks"]) & 0xffff
            if page >= pages:
                raise RuntimeError(f"{name} exposed out-of-range page {page}/{pages}")
            seen.add(page)
            if page != previous_page:
                if page != (previous_page + 1) % pages:
                    raise RuntimeError(f"{name} skipped a score page: {previous_page} -> {page}")
                wraps += page == 0
                scenario["page_transitions"].append(dict(frame, elapsed_ticks=elapsed))
                previous_page = page
            if elapsed >= loop_ticks + 16*row_ticks and wraps:
                break
        else:
            raise RuntimeError(f"{name} did not finish its full loop and refill within 6000 rendered frames")
        end_cycles = mon.registers()["bus_cycles"]
        mon.command("log off")
        events = stores(path)
        scenario["sid"] = sid_summary(path, events, sid_count)
        scenario.update(elapsed_ticks=elapsed, loop_wraps=wraps, pages_seen=sorted(seen),
                        bus_cycles=end_cycles-start_cycles,
                        audio_tick_hz_model=elapsed*bus_hz/(end_cycles-start_cycles))
        if seen != set(range(pages)):
            raise RuntimeError(f"{name} did not visit every packaged page")
        if abs(elapsed - (end_cycles-start_cycles)*50/bus_hz) > 2:
            raise RuntimeError(f"{name} audio clock drifted by more than two ticks")
        music_voices = [(0, 0), (1, 0), (0, 1), (1, 1)]
        if sid_count == 4:
            music_voices += [(sid, voice) for sid in (2, 3) for voice in range(3)]
        for lane, (sid, voice) in enumerate(music_voices):
            # Only demand a gate where the composition actually has attacks.
            # Intentional rests and staggered entrances remain legitimate.
            attacked = any(data[0x60000+offset+p*page_size+12+5*len(music_voices)+row*len(music_voices)+lane] > 1
                           for p in range(pages) for row in range(16))
            if attacked and not scenario["sid"][chips[sid]]["voices"][voice]["gate_on"]:
                raise RuntimeError(f"{name} never sounded composed music lane {lane}")
        if sid_count == 2 and any(0xd440 <= event["address"] <= 0xd478 for event in events):
            raise RuntimeError("Two-SID music wrote to an enhancement SID address")
        if cue == 1 and not all(scenario["sid"][chips[sid]]["voices"][2]["gate_on"] for sid in (0, 1)):
            raise RuntimeError("Lowlands did not service both reserved effects voices during its full loop")
        mon.command("bank ram")
        scenario["stack_guards"] = {symbol: mon.integer(syms[symbol], 1)
                                     for symbol in ("__stack_bottom", "__irq_stack_bottom")}
        mon.command("bank cpu")
        if any(value != 0xa5 for value in scenario["stack_guards"].values()):
            raise RuntimeError(f"{name} changed a native stack guard")
        scenario["passed"] = True
        print(f"MUSIC {name}: {elapsed/50:.2f}s, {len(seen)} pages, {wraps} loop(s), no underruns", flush=True)
    mon.command("jpdb 1 $1f")
    result["passed"] = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sibling = ROOT.parent / "doomc64u"
    parser.add_argument("--video", choices=("pal", "ntsc"), default="pal")
    parser.add_argument("--vice", type=Path, default=Path(os.environ.get("VICE_BIN", sibling / "target/audit-fast/x64sc-64x")))
    parser.add_argument("--roms", type=Path, default=sibling / "target/tools/secondreality/vice/data")
    parser.add_argument("--package-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--title-reference", type=Path,
                        help="optional 10,000-byte host snapshot; require an exact title match")
    parser.add_argument("--streaming-checks", action="store_true",
                        help="after timing, seed section boundaries and results modes through ELF/DWARF-derived monitor writes")
    parser.add_argument("--control-checks", action="store_true",
                        help="after timing, verify Up climbs, Down dives, and lateral dodges with IRQ-synchronized physical joystick input")
    parser.add_argument("--weapon-checks", action="store_true",
                        help="after timing, verify damage flashes, homing missiles and raster-clocked bomb shake/flash with physical joystick input")
    parser.add_argument("--attract-checks", action="store_true",
                        help="after timing, count a natural title/map tour and verify physical-input dismissal and fresh-FIRE relaunch")
    parser.add_argument("--music-checks", action="store_true",
                        help="after timing, play all seven full native cue loops with page, clock, effects and underrun checks")
    parser.add_argument("--llvm-dwarfdump", type=Path,
                        help="DWARF reader for optional streaming/control/weapon checks; defaults beside --llvm-nm")
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--frames", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=45)
    parser.add_argument("--llvm-nm", type=Path, default=Path(os.environ.get("LLVM_MOS", Path.home()/".local/llvm-mos"))/"bin/llvm-nm")
    args = parser.parse_args()
    if not 1 <= args.frames <= 1000 or not 1 <= args.timeout <= 60:
        parser.error("frames must be 1..1000 and timeout 1..60 seconds")
    output = (args.out_dir or ROOT / "target/vice" / args.video).resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"passed": False, "video": args.video, "scope": "Calibrated VICE emulation; not physical hardware timing", "frames": []}
    process = mon = log = None
    try:
        package = args.package_dir.resolve()
        prg, reu, elf = (package / f"havoc.{extension}" for extension in ("prg", "reu", "elf"))
        for path in (args.vice, args.roms, args.llvm_nm, prg, reu, elf):
            if not path.exists():
                raise RuntimeError(f"Required path is missing: {path}")
        if reu.stat().st_size != 16 * 1024 * 1024:
            raise RuntimeError("REU package must be exactly 16 MiB")
        sid_count = reu.read_bytes()[17]
        if sid_count not in (2, 4):
            raise RuntimeError("REU package must identify its two- or four-SID profile")
        syms = symbols(elf, args.llvm_nm)
        failures = {address: name for name, address in syms.items()
                    if ("havoc_c64" in name and "4fail" in name) or name == "rust_begin_unwind"}
        if not failures:
            raise RuntimeError("No fail/panic symbol found in the target ELF")
        report.update(package=str(package), sid_count=sid_count, vice=str(args.vice.resolve()),
                      vice_sha256=hashlib.sha256(args.vice.read_bytes()).hexdigest(),
                      prg_sha256=hashlib.sha256(prg.read_bytes()).hexdigest(),
                      elf_sha256=hashlib.sha256(elf.read_bytes()).hexdigest(),
                      reu_sha256=hashlib.sha256(reu.read_bytes()).hexdigest(), failure_breakpoints=failures)
        with socket.socket() as available:
            available.bind(("127.0.0.1", 0))
            port = available.getsockname()[1]
        startup = output / "startup.mon"
        startup.write_text("jpdb 0 $1f\njpdb 1 $1f\n")
        # The calibrated headless companion crashes while parsing -config;
        # doomc64u's own launcher likewise removes that option. Isolate XDG
        # directories instead and specify all test resources on the CLI.
        command = [str(args.vice.resolve()), "-directory", str(args.roms.resolve()), "+logtofile", "+logcolorize",
                   "-"+args.video, "-warp", "+sound", "-jamaction", "5", "-autostartprgmode", "1",
                   "-reu", "-reusize", "16384", "-reuimage", str(reu), "+reuimagerw", "-sidextra", str(sid_count-1), "-sid2address", "0xd420",
                   "-joydev1", "0", "-joydev2", "0", "-controlport1device", "iosim", "-controlport2device", "iosim",
                   "-remotemonitor", "-remotemonitoraddress", f"127.0.0.1:{port}",
                   "-initbreak", "0x080d", "-moncommands", str(startup), "-autostart", str(prg)]
        if sid_count == 4:
            command.extend(["-sid3address", "0xd440", "-sid4address", "0xd460"])
        report["command"] = command
        (output / "config").mkdir(exist_ok=True)
        (output / "cache").mkdir(exist_ok=True)
        env = dict(os.environ, XDG_CONFIG_HOME=str(output/"config"), XDG_CACHE_HOME=str(output/"cache"))
        log = (output / "vice.log").open("w")
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        deadline = time.monotonic() + args.timeout
        while mon is None:
            if process.poll() is not None:
                raise RuntimeError(f"VICE exited with {process.returncode}; see {output/'vice.log'}")
            try:
                mon = Monitor(port, args.timeout)
            except ConnectionRefusedError:
                if time.monotonic() > deadline:
                    raise RuntimeError("VICE remote monitor did not start")
                time.sleep(0.1)
        mon.command("delete")
        mon.command("sidefx off")
        for address in failures:
            mon.command(f"break exec {address:04x}")
        monitor_log = output / "monitor.log"
        monitor_log.unlink(missing_ok=True)  # VICE otherwise appends old sessions.
        mon.command(f'logname "{monitor_log.as_posix()}"')
        mon.command("log on")
        mon.run_until(syms["frame_complete"], failures)
        report["title"] = counters(mon, syms)
        report["title"]["capture"] = capture(mon, syms, output, "title")
        if report["title"]["game_mode"] != 0:
            raise RuntimeError("Expected title mode after boot")
        if args.title_reference:
            expected = args.title_reference.read_bytes()
            actual = (output / "title-native.bin").read_bytes()
            if len(expected) != 10000:
                raise RuntimeError("Title reference must contain exactly 10,000 bytes")
            differences = sum(a != b for a, b in zip(actual, expected))
            report["title"]["reference"] = {"path": str(args.title_reference.resolve()),
                                             "bytes_different": differences}
            if differences:
                raise RuntimeError(f"Native title differs from host reference in {differences} bytes")
        # Capture real CPU stores while I/O is selected, excluding the renderer
        # RAM living beneath $D000. Tracepoints do not stop the emulated CPU.
        for sid in range(sid_count):
            base = 0xd400 + sid*32
            mon.command(f"trace store {base:04x} {base+24:04x} if @cpu:$01 == $35")
        if "PROF_PHASE" in syms:
            mon.command(f"trace store {syms['PROF_PHASE']:04x}")
        if "PROF_DETAIL" in syms:
            mon.command(f"trace store {syms['PROF_DETAIL']:04x}")
        mon.command("jpdb 1 $0f")  # Physical joystick port 2: active-low FIRE.
        previous = report["title"]
        for index in range(args.frames):
            mon.run_until(syms["frame_complete"], failures)
            frame = counters(mon, syms)
            frame["bus_cycles_delta"] = frame["bus_cycles"] - previous["bus_cycles"]
            for counter in ("video_frames", "audio_ticks", "frame_serial"):
                frame[counter+"_delta"] = (frame[counter]-previous[counter]) & 0xffff
            report["frames"].append(frame)
            previous = frame
            print(f"{args.video.upper()} frame {index+1}: mode={frame['game_mode']} distance={frame['game_distance']} cycles={frame['bus_cycles_delta']} video={frame['video_frames_delta']} audio={frame['audio_ticks_delta']}", flush=True)
        report["gameplay_capture"] = capture(mon, syms, output, "gameplay")
        mon.command("jpdb 1 $1f")
        mon.command("log off")
        events = stores(monitor_log)
        report["sid"] = sid_summary(monitor_log, events, sid_count)
        frames = report["frames"]
        if "PROF_PHASE" in syms:
            report["phase_transitions"] = phase_profile(events, syms["PROF_PHASE"], report["title"], frames)
        if "PROF_DETAIL" in syms:
            report["pixel_profile_markers"] = pixel_profile(events, syms["PROF_DETAIL"], report["title"], frames)
        if frames[-1]["game_mode"] != 1 or frames[-1]["game_distance"] == 0:
            raise RuntimeError("Held fire did not start and advance gameplay")
        if any(frame["joystick_port2"] != 15 or frame["test_input"] != 255 for frame in frames):
            raise RuntimeError("Physical joystick port 2 was not sampled as held fire")
        if any(frame["frame_dropped"] or frame["input_overflows"] for frame in frames):
            raise RuntimeError("Geometry commands or input samples were dropped")
        if any("render_segment" in frame and frame["render_segment"] != active_section(frame["game_distance"] // 8, frame["game_mode"])
               for frame in [report["title"]] + frames):
            raise RuntimeError("Rendered segment does not match the current game distance")
        # This companion's monitor I/O peek does not implement $D031 and
        # returns $FF. The target verifies a CPU-bus read during boot; retain
        # monitor values only as diagnostics, not as a false failure signal.
        if not all(report["sid"][name]["writes"]
                   and any(voice["gate_on"] for voice in report["sid"][name]["voices"][:2])
                   and report["sid"][name]["voices"][2]["gate_on"]
                   for name in ("left_d400", "right_d420")):
            raise RuntimeError("Core music and both effects voices did not sound during the gameplay sample")
        if any(frame.get("music_underruns", 0) for frame in [report["title"]] + frames):
            raise RuntimeError("Score prefetch underrun during ordinary gameplay")
        bus_hz = 985248 if args.video == "pal" else 1022727
        total_cycles = sum(frame["bus_cycles_delta"] for frame in frames)
        report["timing"] = {"bus_hz_model": bus_hz, "mean_frame_bus_cycles": total_cycles/len(frames),
                            "rendered_fps_model": len(frames)*bus_hz/total_cycles,
                            "audio_ticks": sum(frame["audio_ticks_delta"] for frame in frames),
                            "video_frames": sum(frame["video_frames_delta"] for frame in frames)}
        report["timing"]["audio_tick_hz_model"] = report["timing"]["audio_ticks"]*bus_hz/total_cycles
        if abs(report["timing"]["audio_ticks"] - total_cycles*50/bus_hz) > 2:
            raise RuntimeError("The independent audio clock drifted by more than two 50-Hz ticks")
        if args.control_checks:
            report["control_checks"] = {}
            control_checks(mon, syms, failures, elf,
                           args.llvm_dwarfdump or args.llvm_nm.with_name("llvm-dwarfdump"),
                           output, report["control_checks"])
        if args.weapon_checks:
            report["weapon_checks"] = {}
            weapon_checks(mon, syms, failures, elf,
                          args.llvm_dwarfdump or args.llvm_nm.with_name("llvm-dwarfdump"),
                          output, report["weapon_checks"], args.video)
        if args.streaming_checks:
            report["streaming_checks"] = {}
            streaming_checks(mon, syms, failures, elf, reu,
                             args.llvm_dwarfdump or args.llvm_nm.with_name("llvm-dwarfdump"),
                             output, report["streaming_checks"])
        if args.attract_checks:
            report["attract_checks"] = {}
            attract_checks(mon, syms, failures, elf, reu,
                           args.llvm_dwarfdump or args.llvm_nm.with_name("llvm-dwarfdump"),
                           output, report["attract_checks"])
        if args.music_checks:
            report["music_checks"] = {}
            music_checks(mon, syms, failures, elf, reu,
                         args.llvm_dwarfdump or args.llvm_nm.with_name("llvm-dwarfdump"),
                         output, report["music_checks"], sid_count, args.video)
        for extension, path in (("prg", prg), ("reu", reu), ("elf", elf)):
            if hashlib.sha256(path.read_bytes()).hexdigest() != report[extension+"_sha256"]:
                raise RuntimeError(f"Packaged {extension.upper()} changed while the emulator test ran")
        report["package_files_unchanged"] = True
        report["passed"] = True
    except (OSError, RuntimeError, subprocess.CalledProcessError) as exc:
        report["error"] = str(exc)
        print(f"vice-smoke: {exc}", file=sys.stderr)
        if mon is not None:
            try:
                debug = mon.command("r") + mon.command("bt")
                for address, length in ((0, 64), (0x100, 256), (0x7200, 64), (0xc400, 64),
                                        (0x7600, 32), (0x3000, 32), (0x8400, 32)):
                    debug += mon.command(f"m {address:04x} {address+length-1:04x}")
                (output / "failure-monitor.txt").write_text(debug)
                mon.command("step 8000")
                mon.command("bank ram")
                for address in (0x0451, 0x8051):
                    values = mon.read(address, 80)
                    report[f"failure_screen_{address:04x}"] = "".join(chr(v+64 if 0<v<32 else v) for v in values).rstrip()
                mon.command("log off")
            except (OSError, RuntimeError) as debug_error:
                report["diagnostic_error"] = str(debug_error)
    finally:
        if mon is not None:
            mon.socket.close()
        if process is not None and process.poll() is None:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGTERM)
            else:
                process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        if log is not None:
            log.close()
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{'PASS' if report['passed'] else 'FAIL'}: {output/'report.json'}", flush=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
