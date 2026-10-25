#!/usr/bin/env python3
"""Deterministic native renderer benchmarks; never writes the input package.

Build with scripts/build-benchmark.sh. Fixtures use matching ELF/DWARF layouts,
live 50 Hz audio, fixed simulation work, and a common VIC raster starting line.
Sprite HUDs are warmed with the same fixture before every measured frame;
the cold/retained palette choice applies only to bitmap attributes.
The separate real-time pass retains ordinary scheduling and physical FIRE.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import socket
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("vice_smoke", ROOT / "tools/vice-smoke.py")
vice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vice)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def semantic_hash(value):
    return digest(json.dumps(value, sort_keys=True, separators=(",", ":")).encode())


def checkout_metadata():
    command = ["git"]
    pointer = ROOT / ".git"
    if sys.platform == "linux" and pointer.is_file():
        text = pointer.read_text().strip()
        match = re.fullmatch(r"gitdir: ([A-Za-z]):/(.+)", text)
        if match:
            command += ["--git-dir=/mnt/" + match[1].lower() + "/" + match[2],
                        "--work-tree=" + str(ROOT)]
    return dict(driver_checkout_revision=subprocess.check_output(
                    command + ["rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                driver_checkout_status=subprocess.check_output(
                    command + ["status", "--short"], cwd=ROOT, text=True))


class Layouts:
    """Resolve named fields, including arrays, without assuming Rust offsets."""
    primitive = {"u8": 1, "i8": 1, "bool": 1, "u16": 2, "i16": 2, "u32": 4, "i32": 4}

    def __init__(self, elf, tool, output):
        self.elf, self.tool, self.output, self.structs = elf, tool, output, {}

    def array(self, name):
        return re.fullmatch(r"(.+)\[(\d+)\]", name)

    def load(self, name):
        short = name.rsplit("::", 1)[-1]
        if short not in self.structs:
            text = subprocess.check_output([str(self.tool), f"--name={short}", "--show-children", str(self.elf)], text=True)
            (self.output / f"dwarf-{short}.txt").write_text(text)
            size = re.search(r"DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)", text)
            if not size:
                raise RuntimeError(f"Missing DWARF type {name}")
            self.structs[short] = {"size": int(size[1], 0), "fields": vice.dwarf_members(text, short)}
        return self.structs[short]

    def size(self, name):
        if name in self.primitive:
            return self.primitive[name]
        array = self.array(name)
        return self.size(array[1]) * int(array[2]) if array else self.load(name)["size"]

    def member(self, name, path):
        if name.rsplit("::", 1)[-1] == "Game" and "bosses" in self.load(name)["fields"]:
            path = vice.BOSS_FIELD_PATHS.get(path, path)
        offset = 0
        for part in re.findall(r"[a-zA-Z_][a-zA-Z_0-9]*|\d+", path):
            array = self.array(name)
            if array:
                index = int(part)
                if not 0 <= index < int(array[2]):
                    raise RuntimeError(f"Fixture array index out of bounds: {path}")
                name = array[1]
                offset += index * self.size(name)
            else:
                fields = self.load(name)["fields"]
                if part not in fields:
                    raise RuntimeError(f"Fixture field missing: {name}::{part}")
                field = fields[part]
                offset += field["offset"]
                name = field["type"]
        return offset, name

    def patch(self, data, name, path, value):
        offset, kind = self.member(name, path)
        size = self.size(kind)
        if size > 4:
            raise RuntimeError(f"Fixture value is not a scalar: {path}")
        data[offset:offset+size] = int(value).to_bytes(size, "little", signed=kind.startswith("i"))

    def decode(self, name, raw):
        if len(raw) != self.size(name):
            raise RuntimeError(f"Wrong byte size decoding {name}")
        if name in self.primitive:
            return int.from_bytes(raw, "little", signed=name.startswith("i"))
        array = self.array(name)
        if array:
            size = self.size(array[1])
            return [self.decode(array[1], raw[n:n+size]) for n in range(0, len(raw), size)]
        fields = self.load(name)["fields"]
        if not fields:  # Fieldless enums such as Mode and Effect.
            return int.from_bytes(raw, "little")
        return {key: self.decode(field["type"], raw[field["offset"]:field["offset"]+self.size(field["type"])])
                for key, field in fields.items()}


STAGE_FIELDS = ("enemy_profile", "music_cue", "bridge_profile", "boss_kind",
                "theme", "next_theme", "previous_theme", "reserved")
SKY_COLORS = (6, 8, 11, 4, 0, 11, 8, 11, 6, 4, 0, 0, 0, 0, 0, 0)
STAGE_LABELS = ("lowlands", "crystal", "canyon", "gorge", "ash", "foundry",
                "fortress", "citadel", "storm", "meteor", "coast", "carrier",
                "asteroid-belt", "shattered-moon", "ship-graveyard", "orbital-shipyard",
                "space-carrier", "jump-gate")


# The same semantic events are injected at native 50 Hz tick entries in both
# packages. Debugger stops consume no emulated time; all actual SFX work and IRQ
# entry/exit remain inside the measured frame. No engine layout is hardcoded.
SFX_SCENARIOS = (
    ("quiet", "title", ()),
    ("firing", "lowlands", tuple((at, 0, 0) for at in range(0, 50, 5))),
    ("dense-combat", "full-pools", ((0, 6, 0), (2, 3, -96), (4, 14, 0),
        (6, 13, 96), (8, 7, -96), (10, 2, 0), (12, 17, 96),
        (14, 0, 0), (16, 20, 0), (18, 3, -96))),
    ("boss", "boss-gorge-warden", ((0, 18, 0), (2, 13, -96),
        (4, 2, 0), (6, 15, 0), (8, 14, 0), (10, 13, 96),
        (12, 16, 0))),
    ("scenic", "bridge-0-200", ((0, 10, -96), (0, 11, 96),
        (2, 9, 0), (4, 12, 96), (6, 8, -96), (8, 10, 0))),
)


def sustained_schedule(name, ticks):
    """A complete, frame-rate-independent 50 Hz request stream."""
    events = []
    for tick in range(ticks):
        if name in ("sfx-firing", "sfx-dense-combat", "sfx-boss") and tick % 5 == 0:
            events.append((tick, 0, 0))
        if name == "sfx-dense-combat":
            if tick % 7 == 0:
                events.append((tick, 1, -100 if tick // 7 % 2 == 0 else 100))
            if tick % 11 == 0:
                events.append((tick, 3, -100 if tick // 11 % 2 == 0 else 100))
            for at, effect, pan in ((15, 6, 0), (35, 6, 0), (15, 7, -100),
                                    (16, 7, 100), (36, 7, -100), (37, 7, 100),
                                    (70, 14, 0), (110, 17, 100), (160, 20, 0)):
                if tick % 250 == at:
                    events.append((tick, effect, pan))
        elif name == "sfx-boss":
            if tick % 9 == 0:
                events.append((tick, 13, -100 if tick // 9 % 2 == 0 else 100))
            for at, effect, pan in ((0, 18, 0), (25, 6, 0), (45, 6, 0),
                                    (26, 7, -100), (27, 7, 100), (80, 14, 0),
                                    (160, 15, 0), (165, 7, -100), (166, 7, 100)):
                if tick % 250 == at:
                    events.append((tick, effect, pan))
        elif name == "sfx-scenic":
            for at, effect, pan in ((0, 10, -100), (0, 11, 100), (25, 9, 0),
                                    (45, 12, 100), (60, 8, -100), (90, 10, 0),
                                    (120, 12, -100), (145, 9, 100)):
                if tick % 200 == at:
                    events.append((tick, effect, pan))
        elif name not in ("sfx-quiet", "sfx-firing"):
            if name not in ("sfx-dense-combat", "sfx-boss"):
                raise ValueError(f"Unknown sustained workload: {name}")
    return events


def schedule_batches(schedule):
    batches = {}
    for tick, effect, pan in schedule:
        batches.setdefault(tick, []).append([effect, pan])
    if any(len(batch) > 8 for batch in batches.values()):
        raise ValueError("Sustained request schedule exceeds the bounded queue")
    return batches


def validate_sustained_schedule(schedule, injections, completed_ticks, expected_ticks):
    if completed_ticks != expected_ticks:
        raise RuntimeError(f"Sustained clock stopped at {completed_ticks}, expected {expected_ticks}")
    expected = [dict(tick=tick, events=batch) for tick, batch in schedule_batches(schedule).items()]
    actual = [dict(tick=entry["tick"], events=entry["events"]) for entry in injections]
    if actual != expected:
        raise RuntimeError("Sustained request schedule was skipped, duplicated, reordered, or truncated")


def distribution(values):
    ordered = sorted(values)
    if not ordered:
        raise ValueError("Cannot summarize an empty measurement")
    return dict(count=len(ordered), median=statistics.median(ordered),
                p95=ordered[(len(ordered) * 95 + 99) // 100 - 1], max=ordered[-1])


def sustained_statistics(frames):
    result = {"frame_bus_cycles": distribution(frame["bus_cycles_delta"] for frame in frames),
              "dma_bytes": distribution(frame["dma"]["bytes"] for frame in frames),
              "wait_bus_cycles": distribution(frame["pixel_bus_cycles_detail"]["wait"] for frame in frames)}
    for field in ("phase_bus_cycles", "phase_load_bus_cycles", "phase_work_bus_cycles"):
        result[field] = {name: distribution(frame[field][name] for frame in frames)
                         for name in frames[0][field]}
    return result


class SustainedBoundary(Exception):
    """The next IRQ entry is the exact end of the requested tick window."""


def selected_fixtures(args, package_format, boss_health=None, split_boss=False):
    available = fixtures(package_format["medal"], package_format["campaign"],
                         package_format["space"], boss_health, split_boss)
    if args.sfx_workloads:
        by_name = {fixture["name"]: fixture for fixture in available}
        return [dict(by_name[source], name="sfx-" + name, sfx_schedule=list(schedule))
                for name, source, schedule in SFX_SCENARIOS
                if args.fixtures == "all" or "sfx-" + name in args.fixtures.split(",")]
    return [fixture for fixture in available
            if args.fixtures == "all" or fixture["name"] in args.fixtures.split(",")]


def gameplay_identity(state):
    # These are sound-delivery state, intentionally changed by the revised hit
    # policy. Preserve their raw hash separately; compare every gameplay field.
    return {key: value for key, value in state.items()
            if key not in ("audio", "audio_head", "audio_len")}


def stage_metadata(package, stage):
    start = int.from_bytes(package[12:16], "little") + stage * 2048 + 8
    return dict(zip(STAGE_FIELDS, package[start:start+8]))


def fixtures(include_victory=True, campaign=False, space=False, boss_health=None, split_boss=False):
    boss_health = boss_health or dict(boss_left=20, boss_right=20, boss_core=60)
    count = 18 if space else 12 if campaign else 4
    matching = (0, 2, 6, 11) if campaign else (0, 1, 2, 3)
    result = []
    for name, rail in (("title", 0), ("lowlands", 625), ("canyon", 4375),
                       ("base", 8125), ("carrier", 12025), ("missiles", 4375),
                       ("full-pools", 12025), ("section-boundary", 3749)):
        old_section, local = divmod(rail, 3750)
        baseline_scene_ticks = rail + old_section * 400
        section = matching[old_section]
        if campaign and name in ("canyon", "base", "missiles"):
            # Terrain has a 1024-tick shape/scroll period. Align that actual
            # geometry with its old checkpoint while keeping the new stage
            # identity and both metadata/model banks truthful.
            local -= ((section - old_section) * 4150) % 1024
        rail = section * 3750 + local
        patch = {"mode": 0 if name == "title" else 1, "rail_ticks": rail, "ticks": 0,
                 "player.x": 0, "player.y": 0, "player.z": 180,
                 "spawn_segment": rail // 3750, "spawn_cursor": 255,
                 "shield": 100, "lives": 3, "bombs": 3, "invulnerability": 255}
        if name in ("carrier", "full-pools"):
            patch.update(boss_started=1, boss_z=680, **boss_health)
        if name == "missiles":
            for index in range(8):
                x = (index % 4) * 70 - 105
                for field, value in {"kind": 1 + index % 3, "hp": 8, "origin_x": x,
                                     "origin_y": index // 4 * 40, "pos.x": x,
                                     "pos.y": index // 4 * 40, "pos.z": 1000 + index * 80}.items():
                    patch[f"actors[{index}].{field}"] = value
                for field, value in {"kind": 3, "damage": 4, "pos.x": index * 20 - 70,
                                     "pos.y": 0, "pos.z": 300 + index * 40}.items():
                    patch[f"shots[{index}].{field}"] = value
            patch["hit_flash_ticks"] = 35
            patch["controls.cached_charge"] = 255
            # Released 2.1 s before the 16-bit millisecond clock wraps to the
            # fixture's zero: the charge must read as saturated across the wrap.
            patch["controls.released_at"] = (1 << 16) - 2100
        if name == "full-pools":
            for array, pool_count, fields in (
                ("actors", 12, {"kind": 3, "hp": 6, "pos.z": 100}),
                ("shots", 24, {"kind": 2, "damage": 6, "pos.z": 300}),
                ("pickups", 4, {"kind": 1, "pos.z": 400}),
                ("explosions", 6, {"ticks": 9, "color": 7, "pos.z": 500}),
            ):
                for index in range(pool_count):
                    for field, value in fields.items():
                        if array == "actors" and field == "kind":
                            value = 1 + index % 2
                        patch[f"{array}[{index}].{field}"] = value
        result.append({"name": name, "patch": patch, "section": rail // 3750,
                       "cache_miss": name == "section-boundary", "input_bits": 0})
        if campaign and name in ("canyon", "base", "missiles"):
            result[-1]["visual_clock_alignment"] = {
                "terrain_period_ticks": 1024, "baseline_scene_ticks": baseline_scene_ticks,
                "campaign_scene_ticks": rail + section*400,
                "reason": "Compare identical terrain geometry at the corresponding new checkpoint"}
    for section in range(count - 1):
        # Four final meteor streaks overlap at tick 306, before the oldest
        # expires at tick 310; midpoint alone misses this geometry peak.
        progress_ticks = (0, 200, 306, 399) if section >= 2 else (0, 200, 399)
        if space and section == 11:
            progress_ticks = (0, 100, 175, 200, 306, 350, 399)
        for progress in progress_ticks:
            patch = dict(result[1]["patch"], mode=6, rail_ticks=(section+1)*3750,
                         transition_ticks=progress, spawn_segment=section)
            result.append({"name": f"bridge-{section}-{progress}", "patch": patch,
                           "section": section, "cache_miss": False, "input_bits": 16})
    # Exercise the real per-level enemies, including simultaneous escorts and
    # carrier weak points. Warm and cold model-bank fixtures are distinct.
    labels = STAGE_LABELS[:count] if campaign else ("lowlands", "canyon", "fortress", "carrier")
    for section, label in enumerate(labels):
        for saturated in (False, True):
            patch = dict(result[1]["patch"], rail_ticks=section*3750+625,
                         spawn_segment=section)
            if section == count - 1:
                patch.update(boss_started=1, boss_z=680, **boss_health)
            actor_count = 12 if saturated else (2 if section == count - 1 else 3)
            for index in range(actor_count):
                kind = 1 + index % (2 if section == count - 1 else 3)
                x, y = (index % 3-1)*75, -90 if kind == 3 else (index % 2)*50-25
                for field, value in {"kind": kind, "hp": 6, "age": 54,
                        "origin_x": x, "origin_y": y, "pos.x": x, "pos.y": y,
                        "pos.z": 650 + index%3*160}.items():
                    patch[f"actors[{index}].{field}"] = value
            fixture = {"name": f"enemies-{label}{'-full' if saturated else ''}",
                       "patch": patch, "section": section, "cache_miss": False, "input_bits": 0}
            result.append(fixture)
            if not saturated:
                result.append(dict(fixture, name=f"enemies-{label}-cold", enemy_cache_miss=True))
    if campaign:
        bosses = [(3, "gorge-warden"), (7, "citadel-heart")]
        if space:
            bosses += [(16, "dreadnought"), (17, "gate-guardian")]
        for section, label in bosses:
            patch = dict(result[1]["patch"], rail_ticks=section*3750+(2600 if section >= 16 else 1200),
                         spawn_segment=section, boss_started=1, boss_z=680,
                         **boss_health)
            result.append({"name": f"boss-{label}", "patch": patch,
                           "section": section, "cache_miss": False, "input_bits": 0})
        if space and split_boss:
            for phase in (1, 2):
                for exposed in (False, True):
                    patch = dict(result[1]["patch"], rail_ticks=17*3750+3000,
                                 spawn_segment=17, boss_started=1, boss_defeated=0,
                                 boss_phase=phase, boss_split_ticks=0)
                    for index in range(1 << phase):
                        x = (-74 if index == 0 else 74) if phase == 1 else (-55 if index & 1 == 0 else 55)
                        y = 0 if phase == 1 else (-40 if index < 2 else 40)
                        for member, value in {"pos.x": x, "pos.y": y, "pos.z": 500,
                                              "left": 0 if exposed else 40 >> phase,
                                              "right": 0 if exposed else 40 >> phase,
                                              "core": 120 >> phase, "flash": 0}.items():
                            patch[f"bosses[{index}].{member}"] = value
                    for index in range(24):
                        for member, value in {"kind": 1 if index & 1 else 2, "damage": 4,
                                              "pos.x": (index % 6) * 30 - 75,
                                              "pos.y": (index // 6) * 25 - 40,
                                              "pos.z": 250 + index * 4}.items():
                            patch[f"shots[{index}].{member}"] = value
                    result.append({"name": f"boss-gate-guardian-phase-{phase+1}-{'exposed' if exposed else 'protected'}",
                                   "patch": patch, "section": 17, "cache_miss": False, "input_bits": 0})
    if space:
        for section in (12, 14, 15, 16):
            patch = dict(result[1]["patch"], rail_ticks=section*3750+625, spawn_segment=section)
            for index in range(12):
                small = index % 2 == 0
                for field, value in {"kind": 7, "flags": 2 if small else 0,
                                     "hp": 2 if small else 255,
                                     "approach_speed": 8,
                                     "pos.x": (index % 4)*70-105,
                                     "pos.y": (index // 4)*60-60,
                                     "pos.z": 500+(index % 3)*220}.items():
                    patch[f"actors[{index}].{field}"] = value
            result.append({"name": f"hazards-{STAGE_LABELS[section]}", "patch": patch,
                           "section": section, "cache_miss": False, "input_bits": 0})
    if include_victory:
        for name, elapsed in (("start", 0), ("midfade", 50), ("arrival", 400), ("hold", 600)):
            patch = dict(result[1]["patch"], mode=3, rail_ticks=count*3750, ticks=elapsed,
                         menu_ticks=min(elapsed, 400), spawn_segment=count-1,
                         boss_started=1, boss_defeated=1, score=24000,
                         shield=76, lives=2, bombs=1, invulnerability=0)
            result.append({"name": f"victory-{name}", "patch": patch,
                           "section": count-1, "cache_miss": False, "input_bits": 0})
    if campaign:
        # Copy identity from the package during seed(); fixtures only choose
        # checkpoints, never duplicate the checked stage metadata table.
        for fixture in result:
            fixture["patch"]["stage_id"] = fixture["section"]
    return result


def dma_events(events, phase_address):
    registers, transfers = {}, []
    phase = 0
    for event in events:
        address, value = event["address"], event["value"]
        if address == phase_address:
            phase = value
        elif address == 0xdf01 and value & 0x80:
            missing = [n for n in range(2, 11) if n not in registers]
            if missing:
                raise RuntimeError(f"Incomplete REU setup before command: {missing}")
            count = registers.get(7, 0) | registers.get(8, 0) << 8
            transfers.append({"cycles": event["bus_cycles"], "command": value,
                              "phase_marker": phase,
                              "bytes": count or 65536,
                              "cpu": registers[2] | registers[3] << 8,
                              "reu": registers[4] | registers[5] << 8 | registers[6] << 16})
        elif 0xdf02 <= address <= 0xdf0a:
            registers[address - 0xdf00] = value
    return transfers


def execution_cycles(text, address):
    cycles = []
    pattern = r"(?:Trace|trace)\s+(?:on\s+)?exec\s+\$?([0-9a-fA-F]{4})[^\n]*\n([^\n]*)"
    for match in re.finditer(pattern, text):
        tail = re.search(r"\s(\d+)\s*$", match[2])
        if int(match[1], 16) == address and tail:
            cycles.append(int(tail[1]))
    return cycles


def scanout_commit_profile(text, symbols, cycles_per_line):
    """Pair every protected commit entry/exit; reject missing or overlong copies."""
    names = ("publish_guard", "publish_commit", "publish_done")
    if not any(name in symbols for name in names):
        return None  # Earlier benchmark packages publish without these labels.
    if not all(name in symbols for name in names):
        raise RuntimeError("Scan-out commit trace labels are incomplete")
    guard, begin, done = (symbols[name] for name in names)
    if len({guard, begin, done}) != 3:
        raise RuntimeError("Scan-out commit trace labels share an address")
    durations, starts, protected_durations, protected_starts = [], [], [], []
    pending_guard = pending = None
    attempts = 0
    previous = -1
    pattern = r"(?:Trace|trace)\s+(?:on\s+)?exec\s+\$?([0-9a-fA-F]{4})[^\n]*\n([^\n]*)"
    for match in re.finditer(pattern, text):
        address = int(match[1], 16)
        if address not in (guard, begin, done):
            continue
        tail = re.search(r"\s(\d+)\s*$", match[2])
        if tail is None:
            raise RuntimeError("Scan-out commit trace has no cycle timestamp")
        cycle = int(tail[1])
        if cycle < previous:
            raise RuntimeError("Scan-out commit trace timestamps run backward")
        previous = cycle
        if address == guard:
            if pending is not None:
                raise RuntimeError("Scan-out commit was rechecked before the preceding copy ended")
            # A rejected final sample returns to the interruptible wait. Its
            # next attempt supersedes that guard, but may not discard a copy.
            pending_guard = cycle
            attempts += 1
        elif address == begin:
            if pending is not None:
                raise RuntimeError("Scan-out commit began before the preceding copy ended")
            if pending_guard is None:
                raise RuntimeError("Scan-out commit began without a protected raster check")
            pending = cycle
        else:
            if pending is None:
                raise RuntimeError("Scan-out commit ended without a matching entry")
            starts.append(pending)
            durations.append(cycle - pending)
            protected_starts.append(pending_guard)
            protected_durations.append(cycle - pending_guard)
            pending_guard = None
            pending = None
    if pending_guard is not None or pending is not None or len(durations) != 25:
        raise RuntimeError(f"Scan-out commit trace must contain 25 complete copies, got {len(durations)}")
    limit = 7 * cycles_per_line
    if any(duration <= 0 for duration in durations) or any(duration >= limit for duration in protected_durations):
        raise RuntimeError(f"Scan-out commit exceeded the seven-scanline bound ({limit} bus cycles): {protected_durations}")
    return {"copies": len(durations), "start_bus_cycles": starts,
            "duration_bus_cycles": durations, "max_bus_cycles": max(durations),
            "max_scanlines": max(durations) / cycles_per_line,
            "guard_attempts": attempts, "guard_retries": attempts - len(durations),
            "protected_start_bus_cycles": protected_starts,
            "protected_duration_bus_cycles": protected_durations,
            "max_protected_bus_cycles": max(protected_durations),
            "max_protected_scanlines": max(protected_durations) / cycles_per_line,
            "limit_bus_cycles_exclusive": limit}


def validate_pixels_transfers(transfers, symbols):
    """Prove runtime DMA skips the live edge/mask gap in split PIXELS images."""
    if "__phase_pixels_aux_size" not in symbols:
        return None  # Preserve measurements of the earlier contiguous image.
    prefix = symbols["__phase_pixels_prefix_size"]
    helper = symbols["__phase_pixels_aux_size"]
    # geometry_complete marks the subsequent PIXELS transfer as phase 3.
    expected = [(0x40000 + offset, 0x3000 + offset, min(1024, prefix - offset), 3)
                for offset in range(0, prefix, 1024)]
    expected.append((symbols["__phase_pixels_aux_load"],
                     symbols["__phase_pixels_aux_start"], helper, 7))
    actual = [(item["reu"], item["cpu"], item["bytes"], item["phase_marker"])
              for item in transfers if item["reu"] >> 16 == 4]
    if actual != expected:
        raise RuntimeError(f"PIXELS split DMA differs from linked prefix/helper ranges: {actual} != {expected}")
    return {"prefix_bytes": prefix, "helper_bytes": helper, "gap_skipped": True}


def hud_commit_profile(text, symbols, cycles_per_line):
    """Dirty-corner copies may be absent; each observed copy must stay bounded."""
    names = ("hud_publish_guard", "hud_publish_commit", "hud_publish_done")
    if not all(name in symbols for name in names):
        return None
    guard, begin, done = (symbols[name] for name in names)
    events = sorted((cycle, label) for label, address in (("guard", guard), ("begin", begin), ("done", done))
                    for cycle in execution_cycles(text, address))
    guarded = started = None
    durations, protected, attempts = [], [], 0
    for cycle, label in events:
        if label == "guard":
            if started is not None:
                raise RuntimeError("HUD guard interrupted an unfinished image copy")
            guarded = cycle
            attempts += 1
        elif label == "begin":
            if guarded is None or started is not None:
                raise RuntimeError("HUD image copy lacks its final protected raster check")
            started = cycle
        else:
            if started is None:
                raise RuntimeError("HUD image copy ended without an entry")
            durations.append(cycle - started)
            protected.append(cycle - guarded)
            guarded = started = None
    if guarded is not None or started is not None or len(durations) > 4:
        raise RuntimeError("HUD trace has an incomplete copy or more than four dirty corners")
    limit = 2 * cycles_per_line
    if any(duration <= 0 or duration >= limit for duration in protected):
        raise RuntimeError(f"HUD image commit exceeded its two-scanline bound ({limit} cycles): {protected}")
    return dict(copies=len(durations), duration_bus_cycles=durations,
                protected_duration_bus_cycles=protected, guard_attempts=attempts,
                guard_retries=attempts - len(durations), max_protected_bus_cycles=max(protected, default=0),
                max_protected_scanlines=max(protected, default=0) / cycles_per_line,
                limit_bus_cycles_exclusive=limit)


class BinaryInspection:
    """Read bulk snapshots without thousands of monitor text reply fragments."""
    def __init__(self, mon, directory):
        self.mon, self.directory = mon, directory
        directory.mkdir(parents=True, exist_ok=True)
        self.verified = set()
        self.checks = []

    def read(self, address, length, bank, mask=255):
        path = self.directory / f"{bank}-{address:04x}-{length}.bin"
        self.mon.command(f'bsave "{path.as_posix()}" 0 {address:04x} {address+length-1:04x}')
        raw = path.read_bytes()
        if len(raw) != length:
            raise RuntimeError(f"Binary inspection has wrong length at${address:04x}: {len(raw)}/{length}")
        value = bytes(byte & mask for byte in raw) if mask != 255 else raw
        key = (bank, address, length, mask)
        if key not in self.verified:
            text = self.mon.read(address, length)
            expected = bytes(byte & mask for byte in text) if mask != 255 else text
            if value != expected:
                raise RuntimeError(f"Binary/text inspection differs at{bank}:${address:04x}")
            self.verified.add(key)
            self.checks.append(dict(bank=bank, address=address, bytes=length, mask=mask,
                                    sha256=digest(value), text_matches_binary=True))
        return value


def native_bytes(mon, syms, inspection=None):
    bank = mon.integer(syms["FRONT_BANK"], 1)
    bitmap, screen = (0xe000, 0xc000) if bank == 3 else (0xa000, 0x8000)
    mon.command("bank ram")
    result = (inspection.read(bitmap, 8000, "ram") + inspection.read(screen, 1000, "ram")
              if inspection else mon.read(bitmap, 8000) + mon.read(screen, 1000))
    # Color RAM is written in place; its upper nibble is undefined. Byte
    # 10,000 is the VIC background ($D021): what code 0 shows.
    mon.command("bank io")
    result += (inspection.read(0xd800, 1000, "io", mask=15) if inspection
               else bytes(value & 15 for value in mon.read(0xd800, 1000)))
    result += bytes([mon.integer(0xd021, 1) & 15])
    mon.command("bank cpu")
    return result


def decoded_pixels(native):
    """The 32,000 visible pixel colours of a native capture (bitmap, screen,
    color RAM and, when recorded, the background byte; captures from before
    the background was recorded showed black there), independent of which
    hardware slot each cell gives a colour."""
    bitmap, screen, colors = native[:8000], native[8000:9000], native[9000:10000]
    background = native[10000] & 15 if len(native) > 10000 else 0
    pixels = bytearray(32000)
    for cell in range(1000):
        base, high, low, extra = cell * 8, screen[cell] >> 4, screen[cell] & 15, colors[cell] & 15
        row0 = (cell // 40) * 8 * 160 + (cell % 40) * 4
        for row in range(8):
            byte = bitmap[base + row]
            at = row0 + row * 160
            for column in range(4):
                code = (byte >> (6 - column * 2)) & 3
                pixels[at + column] = background if code == 0 else high if code == 1 else low if code == 2 else extra
    return bytes(pixels)


def pixel_digest_of_capture(folder):
    """Decoded-pixel hash of a case's captured frame (for baselines recorded
    before the hash existed)."""
    path = folder / "frame-native.bin"
    if not path.exists():
        return None
    return digest(decoded_pixels(path.read_bytes()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sibling = ROOT.parent / "doomc64u"
    parser.add_argument("--package-dir", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--video", choices=("pal", "ntsc"), default="pal")
    parser.add_argument("--vice", type=Path, default=sibling / "target/audit-fast/x64sc-64x")
    parser.add_argument("--roms", type=Path, default=sibling / "target/tools/secondreality/vice/data")
    parser.add_argument("--llvm-nm", type=Path, default=Path(os.environ.get("LLVM_MOS", Path.home()/".local/llvm-mos"))/"bin/llvm-nm")
    parser.add_argument("--samples", type=int, choices=(1, 3, 9), default=9)
    parser.add_argument("--fixtures", default="all", help="comma-separated fixture names; all by default")
    parser.add_argument("--ticks", default="0,10")
    parser.add_argument("--sfx-workloads", action="store_true", help="five matched audio workloads with native tick event schedules")
    parser.add_argument("--sustained-seconds", type=int, default=0,
                        help="continuous controlled SFX run at each of three CIA phases; minimum30 seconds")
    parser.add_argument("--max-frame-regression", type=float, default=5.0, help="maximum median frame-time increase in percent")
    parser.add_argument("--palettes", default="cold,retained")
    parser.add_argument("--real-time-ticks", type=int, default=400)
    # Accepted for older command lines: the game no longer caches sky bands.
    parser.add_argument("--skip-cache-checks", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--compare", type=Path, help="matching baseline report.json")
    parser.add_argument("--pixel-identity", action="store_true",
                        help="compare decoded pixels instead of raw bitmap/attribute bytes (cell slot order may differ)")
    vice.add_emulator_audio_argument(parser)
    args = parser.parse_args()
    if args.sustained_seconds and (args.sustained_seconds < 30 or not args.sfx_workloads):
        parser.error("--sustained-seconds requires --sfx-workloads and at least30 seconds")
    if args.sustained_seconds and (args.ticks != "10" or args.palettes != "retained" or args.compare):
        parser.error("sustained runs require --ticks10 --palettes retained; compare with bench-sfx.py")
    output, package = args.out_dir.resolve(), args.package_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    package_image = (package/"havoc.reu").read_bytes()
    package_format = vice.package_model_layout(package_image, validate_size=True)
    if package_format["digital"] and args.audio_backend == "native":
        parser.error("digital-package benchmark fixtures require --audio-backend sfx; "
                     "their EffectsEngine snapshots cannot restore DAC state")
    sid_count = package_image[17]
    chosen = selected_fixtures(args, package_format, split_boss=True)
    ticks_values = list(map(int, args.ticks.split(",")))
    palettes = args.palettes.split(",")
    if not 0 <= args.max_frame_regression < 100:
        parser.error("frame regression limit must be 0..100 percent")
    if not chosen or any(n not in (0, 10) for n in ticks_values) or any(p not in ("cold", "retained") for p in palettes):
        parser.error("invalid fixture, ticks, or palette selection")
    report = {"passed": False, "protocol": "havoc-sustained-sfx-v1" if args.sustained_seconds else "havoc-fixed-frame-v1", "video": args.video,
              "scope": "Fixed semantic workload in calibrated VICE; effects only, excludes DAC decoding; not physical hardware",
              "retained_palette_setup": "One warmup frame of the identical fixture fills the single visible VIC bank and color RAM",
              "geometry_hash_policy": "Ordered coordinates, colors and depths; renderer-private sky/orientation/terrain flags (0x66) excluded. Raw geometry and full native pixel hashes are recorded separately.",
              "cases": [], "fixtures": chosen, "campaign": package_format["campaign"],
              "stage_count": package_format["count"], "sid_count": sid_count,
              "sfx_workloads": args.sfx_workloads, "max_frame_regression_percent": args.max_frame_regression}
    if args.sustained_seconds:
        report["sustained"] = dict(seconds=args.sustained_seconds, ticks=args.sustained_seconds * 50,
            cia_phases=[0, 1, 2], audio_hz=50,
            scope="Controlled repeating render/gameplay work with persistent SFX state and50Hz clock; excludes DAC and physical hardware",
            natural_cues="Game and scene publication work executes; pending native cues are discarded at each IRQ before only the explicit schedule is injected",
            frame_window="Complete frames before the exact terminal IRQ entry are scored; the unfinished boundary frame and inter-frame gaps are separate",
            scene_history="Warmed TransitionAudio state persists throughout each run",
            render_clock="FRAME_SERIAL is restored to32 to keep HUD/title animation work identical; native CLOCK_MS and AUDIO_TICKS continue",
            inter_frame_gaps="Existing benchmark raster250 alignment remains active and is recorded separately; no live-game FPS claim")
    process = mon = log = None
    try:
        prg, reu, elf = [package / f"havoc.{extension}" for extension in ("prg", "reu", "elf")]
        hashes = {extension: digest(path.read_bytes()) for extension, path in (("prg", prg), ("reu", reu), ("elf", elf))}
        report.update(package=str(package), hashes=hashes, vice_sha256=digest(args.vice.read_bytes()),
                      driver_sha256=digest(Path(__file__).read_bytes()),
                      smoke_tool_sha256=digest((ROOT/"tools/vice-smoke.py").read_bytes()),
                      fixture_sha256=semantic_hash(chosen), samples=3 if args.sustained_seconds else args.samples,
                      command=sys.argv, **checkout_metadata())
        syms = vice.symbols(elf, args.llvm_nm)
        has_sprite_hud = "HUD_COMMITS" in syms
        report["sprite_hud_setup"] = "matching-fixture-warmup-v1" if has_sprite_hud else "bitmap-only"
        for name in ("BENCH_TICKS", "BENCH_RUNTIME", "benchmark_frame_begin", "benchmark_frame_ready", "geometry_complete"):
            if name not in syms:
                raise RuntimeError(f"Package is not an instrumented benchmark build: missing {name}")
        failures = {address: name for name, address in syms.items() if ("havoc_c64" in name and "4fail" in name) or name == "rust_begin_unwind"}
        layouts = Layouts(elf, args.llvm_nm.with_name("llvm-dwarfdump"), output)
        for name in ("Game", "Runtime", "EffectsEngine", "Renderer", "Triangle"):
            layouts.load(name)
        game_address = vice.resident_symbol(syms, "GAME")[1]
        audio_address = vice.resident_symbol(syms, "EFFECTS")[1]
        renderer_address = vice.resident_symbol(syms, "RENDERER")[1]
        audio_tick_address = vice.resident_symbol(syms, "audio_tick")[1]
        bus_hz, period = (985248, 19705) if args.video == "pal" else (1022727, 20455)
        report.update(bus_hz=bus_hz, audio_period_cycles=period, layouts=layouts.structs)
        with socket.socket() as available:
            available.bind(("127.0.0.1", 0))
            port = available.getsockname()[1]
        startup = output / "startup.mon"
        startup.write_text("jpdb 0 $1f\njpdb 1 $1f\n")
        for name in ("config", "cache"):
            (output / name).mkdir(exist_ok=True)
        command = [str(args.vice.resolve()), "-directory", str(args.roms.resolve()), "+logtofile", "+logcolorize", "-"+args.video,
                   "-warp", "+sound", "-jamaction", "5", "-autostartprgmode", "1", "-reu", "-reusize", str(package_format["reu_size"] // 1024),
                   "-reuimage", str(reu), "+reuimagerw", "-sidextra", str(sid_count-1), "-sid2address", "0xd420",
                   "-joydev1", "0", "-joydev2", "0", "-controlport1device", "iosim", "-controlport2device", "iosim",
                   "-remotemonitor", "-remotemonitoraddress", f"127.0.0.1:{port}", "-initbreak", "0x080d",
                   "-moncommands", str(startup), "-autostart", str(prg)]
        if sid_count == 4:
            command.extend(["-sid3address", "0xd440", "-sid4address", "0xd460"])
        log = (output / "vice.log").open("w")
        process = subprocess.Popen(command, cwd=ROOT, env=dict(os.environ, XDG_CONFIG_HOME=str(output/"config"), XDG_CACHE_HOME=str(output/"cache")),
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        deadline = time.monotonic() + 45
        while mon is None:
            if process.poll() is not None:
                raise RuntimeError("VICE exited before its monitor started")
            try:
                mon = vice.Monitor(port, 60)
            except ConnectionRefusedError:
                if time.monotonic() > deadline:
                    raise RuntimeError("VICE monitor start timeout")
                time.sleep(0.1)
        mon.command("delete")
        mon.command("sidefx off")
        for address in failures:
            # High-RAM benchmark panic code aliases KERNAL ROM during autostart.
            condition = " if @cpu:$01 == $35 || @cpu:$01 == $34" if address >= 0xe000 else ""
            mon.command(f"break exec {address:04x}" + condition)
        report["emulator_audio"] = vice.configure_emulator_audio(
            mon, syms, failures, package_format, args.audio_backend)
        mon.run_until(syms["benchmark_frame_begin"], failures)
        mon.command("bank ram")
        initial_game = mon.read(game_address, layouts.size("Game"))
        initial_audio = mon.read(audio_address, layouts.size("EffectsEngine"))
        runtime_address = mon.integer(syms["BENCH_RUNTIME"])
        initial_runtime = mon.read(runtime_address, layouts.size("Runtime"))
        helper_size = syms.get("__benchmark_size", 0)
        if helper_size:
            helper_start = syms["__benchmark_start"]
            helper_load = syms["__benchmark_load"]
            raw_prg = prg.read_bytes()
            load_base = int.from_bytes(raw_prg[:2], "little")
            expected_helpers = raw_prg[2 + helper_load - load_base:2 + helper_load - load_base + helper_size]
            if mon.read(helper_start, helper_size) != expected_helpers:
                raise RuntimeError("BOOT did not copy the benchmark helper bank exactly")
            report["benchmark_helpers"] = dict(source=helper_load, destination=helper_start,
                size=helper_size, sha256=digest(expected_helpers), verified_before_first_frame=True)
        mon.command("bank cpu")
        report["initial_game"] = layouts.decode("Game", initial_game)
        boss_health = vice.full_boss_health(report["initial_game"])
        chosen = selected_fixtures(args, package_format, boss_health, "bosses" in report["initial_game"])
        if not chosen:
            raise RuntimeError("Selected boss-phase fixtures require the four-boss game layout")
        report.update(full_boss_health=boss_health, fixtures=chosen,
                      fixture_sha256=semantic_hash(chosen))
        inspection = BinaryInspection(mon, output / "inspection")
        report["inspection_transport"] = dict(method="VICE bsave; no emulated execution",
                                             first_range_text_checks=inspection.checks)

        def write(address, data):
            # VICE's command buffer is256 bytes;64-byte deposits stay below it.
            for offset in range(0, len(data), 64):
                mon.command(f"> {address+offset:04x} " + " ".join(f"{byte:02x}" for byte in data[offset:offset+64]))
            if mon.read(address, len(data)) != bytes(data):
                raise RuntimeError(f"Monitor write failed at ${address:04x}")

        def word(address, value, size=2):
            write(address, int(value).to_bytes(size, "little"))

        if report["emulator_audio"]["sfx_only"]:
            # Digital builds initialize the independent effects engine in
            # AUDIO, after the first SIM pass. Keep the pristine Game/Runtime
            # snapshots above, but let one zero-tick title frame initialize SID.
            word(syms["BENCH_TICKS"], 0, 1)
            mon.run_until(syms["frame_complete"], failures)
            mon.command("bank ram")
            initial_audio = bytearray(mon.read(audio_address, layouts.size("EffectsEngine")))
            initialized_audio = layouts.decode("EffectsEngine", initial_audio)
            if not initialized_audio["enabled"]:
                raise RuntimeError("Benchmark warmup did not initialize effects")
            mon.command("bank cpu")
            word(syms["BENCH_TICKS"], 255, 1)
            mon.run_until(syms["benchmark_frame_begin"], failures)
            report["audio_seed"] = "initialized effects-only snapshot; excludes DAC decoding cost"
        else:
            report["audio_seed"] = "startup EffectsEngine snapshot"

        if args.real_time_ticks:
            mon.command("jpdb 1 $0f")
            frames = []
            previous = vice.counters(mon, syms)
            while previous["audio_ticks"] < args.real_time_ticks:
                mon.run_until(syms["frame_complete"], failures)
                frame = vice.counters(mon, syms)
                frame["bus_cycles_delta"] = frame["bus_cycles"] - previous["bus_cycles"]
                frames.append(frame)
                if frame["input_overflows"] or frame["frame_dropped"]:
                    raise RuntimeError("Real-time pass dropped input or geometry")
                previous = frame
            selected = [frame for frame in frames if 50 <= frame["audio_ticks"] <= args.real_time_ticks]
            report["real_time"] = {"input": "physical port-2 held FIRE", "frames": frames,
                                   "measurement": "completed frames after the first second through the requested audio-tick deadline",
                                   "fps": len(selected)*bus_hz/sum(frame["bus_cycles_delta"] for frame in selected),
                                   "audio_hz": (frames[-1]["audio_ticks"]-frames[0]["audio_ticks"])*bus_hz/(frames[-1]["bus_cycles"]-frames[0]["bus_cycles"])}
            print(f"REALTIME {args.video}: {report['real_time']['fps']:.3f} FPS", flush=True)
            mon.command("jpdb 1 $1f")
        word(syms["BENCH_TICKS"], 0, 1)
        mon.run_until(syms["benchmark_frame_ready"], failures)
        mon.command(f"trace store {syms['PROF_PHASE']:04x}")
        mon.command(f"trace store {syms['PROF_DETAIL']:04x}")
        mon.command("trace store df01 df0a if @cpu:$01 == $35")
        mon.command("trace store d400 d418 if @cpu:$01 == $35")
        mon.command("trace store d420 d438 if @cpu:$01 == $35")
        if sid_count == 4:
            mon.command("trace store d440 d478 if @cpu:$01 == $35")
        mon.command(f"trace exec {audio_tick_address:04x}")
        for name in ("publish_guard", "publish_commit", "publish_done", "hud_publish_guard", "hud_publish_commit", "hud_publish_done"):
            if name in syms:
                # These addresses belong to the shared overlay window.
                mon.command(f"trace exec {syms[name]:04x} if @cpu:${syms['PROF_PHASE']:04x} == $07")
        segment_image = package_image
        baseline = None
        if args.compare:
            baseline = json.loads(args.compare.read_text())
            if not baseline.get("passed"):
                raise RuntimeError("Baseline report did not pass")
            if any(baseline.get(key) != report[key] for key in ("video", "protocol")):
                raise RuntimeError("Baseline video/protocol differs")
            if bool(baseline.get("sfx_workloads", False)) != args.sfx_workloads:
                raise RuntimeError("Baseline SFX workload mode differs")
            if baseline.get("sid_count", 2) != sid_count:
                raise RuntimeError("Baseline SID profile differs")
            cross_campaign = (baseline.get("campaign", False) != report["campaign"]
                              or baseline.get("stage_count", 4) != report["stage_count"])
            if not cross_campaign and baseline.get("fixture_sha256") != report["fixture_sha256"]:
                raise RuntimeError("Baseline fixture identity differs")
            report["comparison"] = {"cross_campaign": cross_campaign,
                "required_equality": ["geometry_sha256", "native_pixels_sha256" if args.pixel_identity else "native_sha256"] +
                    ([] if cross_campaign else ["game_sha256"]),
                "pixel_identity": args.pixel_identity,
                "game_state_note": "Campaign stage identity fields differ; geometry and complete native pixels remain exact requirements" if cross_campaign else "Identical gameplay state excluding sound queue, which changes intentionally" if args.sfx_workloads else "Identical decoded state required"}
        baseline_cases = {(case["fixture"], case["ticks"], case["palette"]): case for case in baseline["cases"]} if baseline else {}
        if baseline and args.pixel_identity:
            # Baselines recorded before the decoded-pixel hash existed still
            # hold their first sample's capture; hash that.
            for (name, ticks, palette), case in baseline_cases.items():
                if case["samples"] and "native_pixels_sha256" not in case["samples"][0]:
                    recovered = pixel_digest_of_capture(args.compare.parent / f"{name}-{ticks}-{palette}" / "0")
                    if recovered is None:
                        raise RuntimeError(f"Baseline {name}/{ticks}/{palette} has neither a pixel hash nor a capture")
                    case["samples"][0]["native_pixels_sha256"] = recovered

        active_run = None

        def seed(fixture, ticks, cold, phase, continuous=False, warm_history=False):
            mon.command("bank ram")
            history = None
            if continuous or warm_history:
                offset, kind = layouts.member("Runtime", "transition_audio")
                history = (offset, mon.read(runtime_address + offset, layouts.size(kind)))
            game = bytearray(initial_game)
            values = dict(fixture["patch"])
            if package_format["campaign"]:
                info = stage_metadata(package_image, fixture["section"])
                values["stage_id"] = fixture["section"]
                values.update((f"stage_info.{field}", value) for field, value in info.items())
            else:
                info = {"enemy_profile": fixture["section"], "boss_kind": 0}
            values = vice.clock_fixture(values, layouts.load("Game")["fields"], package_format["count"])
            for field, value in values.items():
                layouts.patch(game, "Game", field, value)
            # Speeds are initialized exactly as spawning would do, including
            # zero-tick missile fixtures that have not run enemy movement yet.
            if "approach_speed" in layouts.load("Actor")["fields"]:
                for index in range(12):
                    kind = fixture["patch"].get(f"actors[{index}].kind", 0)
                    if 1 <= kind <= 3:
                        descriptor = 0xb0000 + fixture["section"]*2048 + package_format["descriptors"] + (kind-1)*8
                        layouts.patch(game, "Game", f"actors[{index}].approach_speed", segment_image[descriptor+6])
            write(game_address, game)
            if not continuous:
                write(audio_address, initial_audio)
            runtime = bytearray(initial_runtime)
            if history:
                runtime[history[0]:history[0] + len(history[1])] = history[1]
            sections = [fixture["section"], (fixture["section"] + (2 if fixture["cache_miss"] else 1)) % package_format["count"]]
            for field, value in (("sim_ms", 0), ("current", 0), ("loaded[0]", sections[0]), ("loaded[1]", sections[1])):
                layouts.patch(runtime, "Runtime", field, value)
            if "profile" in layouts.load("Runtime")["fields"]:
                profile = (fixture["section"] + int(fixture.get("enemy_cache_miss", False))) % package_format["count"]
                if package_format["campaign"]:
                    code_profile = vice.boss_sim_profile(info["enemy_profile"], info["boss_kind"]) if fixture["patch"]["mode"] in (1, 5) else 7
                    if fixture.get("enemy_cache_miss"):
                        code_profile = (code_profile + 1) % 8
                else:
                    code_profile = 4 if package_format["medal"] and fixture["patch"]["mode"] in (3, 4) else profile
                layouts.patch(runtime, "Runtime", "profile", code_profile)
                write(0x8400, segment_image[0xb0000+profile*2048:0xb0000+(profile+1)*2048])
                word(syms["ENEMY_MODEL_PROFILE"], profile, 1)
                word(syms["ENEMY_CODE_PROFILE"], code_profile, 1)
            write(runtime_address, runtime)
            for slot, section in enumerate(sections):
                write(syms.get(f"__segment{slot}_start", 0x9500 + slot * 512), segment_image[0x2000+section*2048:0x2200+section*2048])
            for name, size, value in (("CLOCK_MS", 2, 0), ("INPUT_HEAD", 1, 0), ("INPUT_TAIL", 1, 0), ("LAST_BITS", 1, fixture["input_bits"])):
                if continuous and name == "CLOCK_MS":
                    continue
                word(vice.resident_symbol(syms, name)[1], value, size)
            for name, size, value in (("AUDIO_TICKS", 2, 0), ("INPUT_OVERFLOWS", 2, 0), ("FRAME_SERIAL", 2, 32),
                                      ("TEST_INPUT", 1, fixture["input_bits"]), ("BENCH_TICKS", 1, ticks)):
                if continuous and name in ("AUDIO_TICKS", "INPUT_OVERFLOWS"):
                    continue
                word(syms[name], value, size)
            if cold:
                write(0x8000, bytes(1000))
                # "bank ram" addresses the RAM under I/O; color RAM needs the
                # I/O bank. Its upper nibble reads back undefined.
                mon.command("bank io")
                mon.command("fill d800 dbe7 00")
                if any(value & 15 for value in mon.read(0xd800, 1000)):
                    raise RuntimeError("Color RAM clear failed")
                mon.command("bank ram")
            mon.command("bank cpu")
            seeded = {"game_sha256": semantic_hash(layouts.decode("Game", bytes(game)))}
            if continuous:
                return seeded
            # Force a known first CIA deadline, then restore the ordinary latch.
            # Monitor reads have side effects disabled and consume no CPU time.
            first_period = (period, period*2//3, period//3)[phase]
            mon.command("> dc0e 00")
            mon.command(f"> dc04 {(first_period-1)&255:02x} {(first_period-1)>>8:02x}")
            mon.command("> dc0e 11")
            # Reading ICR through the CPU is necessary to clear any previous
            # pending underflow. sidefx applies only to monitor peeks.
            mon.command("sidefx on")
            mon.read(0xdc0d, 1)
            mon.command("sidefx off")
            return dict(seeded, first_audio_period_cycles=first_period)

        def finish_timer_seed():
            # VICE applies CRA force-load on a later CIA cycle. Run the first
            # normal instructions of this frame before restoring the 50 Hz
            # reload latch; otherwise that write can replace the phase offset.
            # This execution is inside the measured interval and trace log.
            mon.command("step $40")
            mon.command(f"> dc04 {(period-1)&255:02x} {(period-1)>>8:02x}")
            remaining = mon.integer(0xdc04)
            if not 0 < remaining < period:
                raise RuntimeError(f"CIA timer seed did not take effect: {remaining}")
            return mon.registers()["bus_cycles"] + remaining + 1

        def read_geometry():
            # The depth sort leaves the commands in emission order and writes
            # the sorted order to __sort_order; report them in sorted order so
            # the geometry hash is independent of that implementation detail.
            count_offset, _ = layouts.member("Renderer", "triangle_count")
            count = mon.integer(renderer_address+count_offset)
            triangles_offset, _ = layouts.member("Renderer", "triangles")
            triangle_size = layouts.size("Triangle")
            order = list(mon.read(syms["__sort_order"], count)) if count else []
            if sorted(order) != list(range(count)):
                raise RuntimeError(f"Sorted order at __sort_order is not a permutation of {count} commands")
            raw = inspection.read(renderer_address+triangles_offset, count*triangle_size, "ram") if count else b""
            return [layouts.decode("Triangle", raw[n*triangle_size:(n+1)*triangle_size]) for n in order]

        def run_scheduled_until(target, schedule, injections):
            if not args.sfx_workloads:
                return mon.run_until(target, failures)
            response = mon.command(f"break exec {audio_tick_address:04x}")
            match = re.search(r"BREAK:\s*(\d+)", response)
            if not match:
                raise RuntimeError(f"Cannot identify scheduled SFX breakpoint: {response}")
            checkpoint = int(match[1])
            try:
                if mon.registers()["pc"] == target:
                    mon.command("step")
                mon.command(f"until {target:04x}")
                while True:
                    current = mon.registers()
                    if current["pc"] in failures:
                        raise RuntimeError(f"Target entered {failures[current['pc']]}")
                    if current["pc"] == target:
                        return current
                    if current["pc"] == audio_tick_address:
                        tick = mon.integer(syms["AUDIO_TICKS"], 2)
                        batch = [[effect, pan] for at, effect, pan in schedule if at == tick]
                        if batch or active_run is not None:
                            mon.command("bank ram")
                            audio = bytearray(mon.read(audio_address, layouts.size("EffectsEngine")))
                            count_offset, _ = layouts.member("EffectsEngine", "event_count")
                            count = audio[count_offset]
                            if active_run is not None:
                                if tick != active_run["next_tick"]:
                                    raise RuntimeError(f"Sustained IRQ clock skipped or replayed tick{tick}")
                                lateness = current["bus_cycles"] - (active_run["first_deadline_bus_cycles"] + tick * period)
                                if not -2 <= lateness <= bus_hz * 0.002:
                                    raise RuntimeError(f"Sustained IRQ deadline missed at tick{tick}: {lateness} bus cycles")
                                active_run["tick_entries"].append(dict(tick=tick,
                                    bus_cycles=current["bus_cycles"], lateness_cycles=lateness))
                                if tick:
                                    state = layouts.decode("EffectsEngine", bytes(audio))
                                    admitted = sorted({(instance["effect"], instance["mask"])
                                        for instance in state["instances"]
                                        if instance["mask"] and instance["age"] == 0})
                                    active_run["admissions"].append(dict(tick=tick - 1,
                                        groups=[list(group) for group in admitted]))
                                    if admitted and tick - 1 not in active_run["batches"]:
                                        raise RuntimeError("An unscheduled native SFX was admitted")
                                if tick == active_run["ticks"]:
                                    active_run["boundary_bus_cycles"] = current["bus_cycles"]
                                    mon.command("bank cpu")
                                    raise SustainedBoundary()
                                active_run["next_tick"] = tick + 1
                                active_run["suppressed_native_cues"] += count
                                count = 0
                                audio[count_offset] = 0
                            if count + len(batch) > 8:
                                raise RuntimeError("Scheduled SFX exceed bounded event queue")
                            if batch:
                                requested = dict(tick=tick, events=batch)
                                injections.append(requested)
                                if active_run is not None:
                                    active_run["injections"].append(requested)
                            for effect, pan in batch:
                                layouts.patch(audio, "EffectsEngine", f"events[{count}].effect", effect)
                                layouts.patch(audio, "EffectsEngine", f"events[{count}].pan", pan)
                                count += 1
                            layouts.patch(audio, "EffectsEngine", "event_count", count)
                            write(audio_address, audio)
                            mon.command("bank cpu")
                        mon.command("step")
                    mon.command("x")
            finally:
                mon.command(f"delete {checkpoint}")

        def measured_frame(folder, capture_image, schedule, first_deadline=None):
            path = folder / "monitor.log"
            path.unlink(missing_ok=True)
            mon.command(f'logname "{path.as_posix()}"')
            mon.command("log on")
            start = vice.counters(mon, syms)
            if first_deadline is None:
                first_deadline = finish_timer_seed()
                if active_run is not None:
                    active_run["first_deadline_bus_cycles"] = first_deadline
            injections = []
            try:
                run_scheduled_until(syms["geometry_complete"], schedule, injections)
            except SustainedBoundary:
                mon.command("log off")
                end = vice.counters(mon, syms)
                return dict(partial=True, start_bus_cycles=start["bus_cycles"],
                            bus_cycles_delta=end["bus_cycles"] - start["bus_cycles"],
                            audio_ticks=end["audio_ticks"], sfx_injections=injections)
            mon.command("bank ram")
            clear_offset, _ = layouts.member("Renderer", "clear_color")
            clear_color = mon.integer(renderer_address+clear_offset, 1)
            geometry = read_geometry()
            mon.command("bank cpu")
            try:
                run_scheduled_until(syms["frame_complete"], schedule, injections)
            except SustainedBoundary:
                mon.command("log off")
                end = vice.counters(mon, syms)
                return dict(partial=True, start_bus_cycles=start["bus_cycles"],
                            bus_cycles_delta=end["bus_cycles"] - start["bus_cycles"],
                            audio_ticks=end["audio_ticks"], sfx_injections=injections)
            end = vice.counters(mon, syms)
            mon.command("log off")
            end["bus_cycles_delta"] = end["bus_cycles"] - start["bus_cycles"]
            end["start_bus_cycles"] = start["bus_cycles"]
            end["start_audio_ticks"] = start["audio_ticks"]
            end["sfx_injections"] = injections
            end["first_audio_deadline_cycles"] = first_deadline
            events = vice.stores(path)
            vice.phase_profile(events, syms["PROF_PHASE"], start, [end])
            vice.pixel_profile(events, syms["PROF_DETAIL"], start, [end])
            transfers = dma_events(events, syms["PROF_PHASE"])
            end["pixels_transfers"] = validate_pixels_transfers(transfers, syms)
            end["dma"] = {"bytes": sum(item["bytes"] for item in transfers), "commands": len(transfers),
                          "max_bytes": max((item["bytes"] for item in transfers), default=0), "transfers": transfers,
                          "bytes_by_phase_marker": {str(phase): sum(item["bytes"] for item in transfers if item["phase_marker"] == phase)
                                                    for phase in sorted({item["phase_marker"] for item in transfers})}}
            trace = path.read_text(errors="replace")
            end["audio_service_cycles"] = execution_cycles(trace, audio_tick_address)
            end["scanout_commits"] = scanout_commit_profile(trace, syms, 63 if args.video == "pal" else 65)
            end["hud_image_commits"] = hud_commit_profile(trace, syms, 63 if args.video == "pal" else 65)
            if has_sprite_hud:
                for counter in ("hud_commits", "hud_image_writes"):
                    end[counter + "_delta"] = (end[counter] - start[counter]) & 0xffff
                if (end["hud_commits_delta"] or end["hud_image_writes_delta"]
                        or end["hud_image_commits"]["copies"]):
                    raise RuntimeError("Measured frame changed the warmed sprite HUD")
            end["sid"] = vice.sid_summary(path, events, sid_count)
            # Quiet fixtures legitimately have no SID gates: music is DAC-only.
            mon.command("bank ram")
            state = vice.logical_game_state(layouts.decode("Game", mon.read(game_address, layouts.size("Game"))))
            end["simulation_ticks"] = state["ticks"]
            if "ENEMY_CODE_PROFILE" in syms:
                section = vice.active_section(state["rail_ticks"], state["mode"], state.get("stage_id"))
                end["enemy_bank"] = vice.enemy_bank_check(mon, syms, segment_image, section, state)
                mon.command("bank ram")
            end["stack_guards"] = {name: mon.integer(syms[name], 1) for name in ("__stack_bottom", "__irq_stack_bottom")}
            mon.command("bank cpu")
            native = native_bytes(mon, syms, inspection)
            sprites = vice.sprite_state(mon, syms)
            # Renderer-private classification flags are not geometry: bits
            # 2/4 identify cached sky/orientation, 32/64 identify the terrain
            # visibility pass. Compare every coordinate, color, depth and
            # triangle order, and separately require identical native pixels.
            comparable_geometry = [dict(triangle, reserved=triangle["reserved"] & ~0x66) for triangle in geometry]
            end.update(geometry_sha256=semantic_hash(comparable_geometry), raw_geometry_sha256=semantic_hash(geometry),
                       static_background_sha256=semantic_hash([triangle for triangle in geometry if triangle["reserved"] & 2]),
                       static_background_triangles=sum(bool(triangle["reserved"] & 2) for triangle in geometry),
                       game_sha256=semantic_hash(gameplay_identity(state) if args.sfx_workloads else state),
                       raw_game_sha256=semantic_hash(state), native_sha256=digest(native),
                       native_pixels_sha256=digest(decoded_pixels(native)),
                       native_screen_sha256=digest(native[8000:9000]), native_color_sha256=digest(native[9000:10000]),
                       native_background=native[10000],
                       sprite_state_sha256=semantic_hash(vice.sprite_visual_state(sprites)),
                       sprite_images_sha256=digest(vice.sprite_images(sprites)),
                       composited_pixels_sha256=digest(vice.composite_pixels(decoded_pixels(native), sprites)),
                       clear_color=clear_color)
            if capture_image:
                end["capture"] = vice.capture(mon, syms, folder, "frame")
                (folder/"game.json").write_text(json.dumps(state, indent=2)+"\n")
                (folder/"geometry.json").write_text(json.dumps(geometry, indent=2)+"\n")
            if end["input_overflows"] or end["frame_dropped"] or any(value != 0xa5 for value in end["stack_guards"].values()):
                raise RuntimeError("Fixed-work frame dropped data or changed a stack guard")
            if end.get("music_underruns", 0):
                raise RuntimeError("Fixed-work frame reported an audio underrun")
            if end["dma"]["max_bytes"] > 1024:
                raise RuntimeError("Runtime DMA exceeded the 1024-byte IRQ latency bound")
            service = end["audio_service_cycles"]
            if not service:
                raise RuntimeError("No traced audio-service entries in measured frame")
            lateness = [cycle - (first_deadline+(start["audio_ticks"]+index)*period)
                        for index, cycle in enumerate(service)]
            end["audio_lateness_cycles"] = lateness
            end["max_audio_lateness_ms"] = max(lateness)*1000/bus_hz
            if min(lateness) < -2 or max(lateness) > bus_hz*0.002:
                raise RuntimeError(f"Audio deadline/latency failure: {lateness}")
            if end["audio_ticks"] - start["audio_ticks"] != len(service):
                raise RuntimeError("Audio counter does not match traced services")
            return end

        if args.sustained_seconds:
            target_ticks = args.sustained_seconds * 50
            for fixture in chosen:
                schedule = sustained_schedule(fixture["name"], target_ticks)
                case = dict(fixture=fixture["name"], ticks=10, palette="retained",
                            schedule=schedule, schedule_sha256=semantic_hash(schedule), runs=[])
                report["cases"].append(case)
                reference_identity = None
                for phase in range(3):
                    root_folder = output / fixture["name"] / f"phase-{phase}"
                    root_folder.mkdir(parents=True, exist_ok=True)
                    # Warm pixels/HUD and scenic history, then restart the SID
                    # engine and CIA once. All following frames retain both.
                    seed(fixture, 10, True, phase)
                    finish_timer_seed()
                    mon.run_until(syms["frame_complete"], failures)
                    mon.run_until(syms["benchmark_frame_ready"], failures)
                    seeded = seed(fixture, 10, False, phase, warm_history=True)
                    run = dict(cia_phase=phase, ticks=target_ticks, next_tick=0,
                               batches=schedule_batches(schedule), injections=[], admissions=[],
                               suppressed_native_cues=0, frames=[], tick_entries=[], inter_frame_bus_cycles=0,
                               initial=seeded, start_bus_cycles=mon.registers()["bus_cycles"])
                    active_run = run
                    frame_index = 0
                    while True:
                        folder = root_folder / str(frame_index)
                        folder.mkdir(exist_ok=True)
                        sample = measured_frame(folder, frame_index == 0, schedule,
                                                run.get("first_deadline_bus_cycles"))
                        if sample.get("partial"):
                            run["boundary_frame"] = sample
                            break
                        identity = {field: sample[field] for field in
                                    ("game_sha256", "geometry_sha256", "native_sha256", "sprite_state_sha256")}
                        if reference_identity is None:
                            reference_identity = identity
                        if identity != reference_identity:
                            changed = [field for field in identity if identity[field] != reference_identity[field]]
                            raise RuntimeError(f"Sustained repeated work changed {changed} at{fixture['name']}/phase{phase}/frame{frame_index}")
                        if fixture["patch"]["mode"] in (1, 3, 6):
                            expected_ticks = (fixture["patch"].get("ticks", 0) + 10) & 0xffff
                            if sample["simulation_ticks"] != expected_ticks:
                                raise RuntimeError("Sustained frame changed its fixed simulation workload")
                        # Full monitor traces remain on disk. Keep the report
                        # compact while retaining every frame's costs/identity.
                        compact = {field: sample[field] for field in (
                            "start_bus_cycles", "bus_cycles", "bus_cycles_delta", "start_audio_ticks", "audio_ticks",
                            "simulation_ticks", "phase_bus_cycles", "phase_load_bus_cycles", "phase_work_bus_cycles",
                            "pixel_bus_cycles_detail", "geometry_bus_cycles_detail", "max_audio_lateness_ms",
                            "audio_lateness_cycles", "sfx_injections", "game_sha256", "geometry_sha256",
                            "native_sha256", "sprite_state_sha256") if field in sample}
                        compact["dma"] = {key: value for key, value in sample["dma"].items() if key != "transfers"}
                        compact["sid_writes"] = sum(chip["writes"] for chip in sample["sid"].values()
                                                   if isinstance(chip, dict) and "writes" in chip)
                        run["frames"].append(compact)
                        gap_start = mon.registers()["bus_cycles"]
                        try:
                            run_scheduled_until(syms["benchmark_frame_ready"], schedule, [])
                        except SustainedBoundary:
                            run["boundary_frame"] = dict(partial=True, inter_frame=True,
                                start_bus_cycles=gap_start,
                                bus_cycles_delta=mon.registers()["bus_cycles"] - gap_start)
                            break
                        run["inter_frame_bus_cycles"] += mon.registers()["bus_cycles"] - gap_start
                        seed(fixture, 10, False, phase, continuous=True)
                        frame_index += 1
                    active_run = None
                    validate_sustained_schedule(schedule, run["injections"], run["next_tick"], target_ticks)
                    if len(run["admissions"]) != target_ticks:
                        raise RuntimeError("Sustained admission trace does not cover every completed tick")
                    if mon.integer(vice.resident_symbol(syms, "CLOCK_MS")[1]) != (target_ticks * 20) & 0xffff:
                        raise RuntimeError("Sustained50Hz clock changed speed")
                    run["completed_audio_ticks"] = target_ticks
                    run["scheduled_window_bus_cycles"] = target_ticks * period
                    run["actual_window_bus_cycles"] = run["boundary_bus_cycles"] - run["first_deadline_bus_cycles"]
                    run["boundary_lateness_cycles"] = run["actual_window_bus_cycles"] - target_ticks * period
                    run["requested_events"] = len(schedule)
                    run["admitted_groups"] = sum(len(entry["groups"]) for entry in run["admissions"])
                    run["statistics"] = sustained_statistics(run["frames"])
                    del run["batches"]
                    case["runs"].append(run)
                    case["identity"] = reference_identity
                    print(f"SUSTAINED {args.video} {fixture['name']} phase={phase}: {len(run['frames'])} complete frames, "
                          f"{target_ticks} ticks, {run['requested_events']} requests/{run['admitted_groups']} admitted groups, "
                          f"median={run['statistics']['frame_bus_cycles']['median']/bus_hz*1000:.3f}ms "
                          f"p95={run['statistics']['frame_bus_cycles']['p95']/bus_hz*1000:.3f}ms", flush=True)
                    (output/"report.json").write_text(json.dumps(report, indent=2)+"\n")
                    # Finish only the unscored boundary frame before the next
                    # independently seeded phase. No scheduled tick is omitted.
                    mon.run_until(syms["benchmark_frame_ready"], failures)
                case["statistics"] = sustained_statistics([frame for run in case["runs"] for frame in run["frames"]])

        for fixture in ([] if args.sustained_seconds else chosen):
            for ticks in ticks_values:
                for palette in palettes:
                    case = {"fixture": fixture["name"], "ticks": ticks, "palette": palette, "samples": []}
                    report["cases"].append(case)
                    for sample_index in range(args.samples):
                        phase = sample_index % 3
                        folder = output / f"{fixture['name']}-{ticks}-{palette}" / str(sample_index)
                        folder.mkdir(parents=True, exist_ok=True)
                        if palette == "retained" or has_sprite_hud:
                            # Warm the identical post-simulation HUD for every
                            # sample, independent of earlier fixtures. The final
                            # seed still clears bitmap attributes for cold cases.
                            seed(fixture, ticks, True, phase)
                            finish_timer_seed()
                            mon.run_until(syms["frame_complete"], failures)
                            mon.run_until(syms["benchmark_frame_ready"], failures)
                        seeded = seed(fixture, ticks, palette == "cold", phase)
                        sample = measured_frame(folder, sample_index == 0, fixture.get("sfx_schedule", ()))
                        if fixture["patch"]["mode"] in (1, 3, 6):
                            expected_ticks = (fixture["patch"].get("ticks", 0) + ticks) & 0xffff
                            if sample["simulation_ticks"] != expected_ticks:
                                raise RuntimeError("Code/model reentry skipped or duplicated fixed simulation ticks")
                        sample.update(initial=seeded, cia_phase=phase)
                        if case["samples"] and any(sample[key] != case["samples"][0][key] for key in ("geometry_sha256", "game_sha256", "native_sha256", "sprite_state_sha256")):
                            raise RuntimeError("Identical fixture repeats changed geometry, game state, bitmap pixels, or visible sprites")
                        case["samples"].append(sample)
                        mon.run_until(syms["benchmark_frame_ready"], failures)
                    case.update(median_cycles=statistics.median(sample["bus_cycles_delta"] for sample in case["samples"]),
                                worst_cycles=max(sample["bus_cycles_delta"] for sample in case["samples"]),
                                max_audio_lateness_ms=max(sample["max_audio_lateness_ms"] for sample in case["samples"]))
                    reference = baseline_cases.get((fixture["name"], ticks, palette))
                    if reference:
                        if (palette == "cold" and reference["samples"][0].get("hud_image_commits") is not None
                                and baseline.get("sprite_hud_setup") != "matching-fixture-warmup-v1"):
                            raise RuntimeError("Cold baseline has unseeded sprite HUD state; rerun it with the current driver")
                        comparison_failures = []
                        equality = report["comparison"]["required_equality"] + (["sprite_state_sha256"] if "sprite_state_sha256" in reference["samples"][0] else [])
                        for key in equality:
                            if case["samples"][0][key] != reference["samples"][0][key]:
                                comparison_failures.append(f"Baseline comparison failed for {fixture['name']}/{ticks}/{palette}: {key}")
                        if not comparison_failures:
                            case["speedup"] = reference["median_cycles"]/case["median_cycles"]
                            case["frame_time_change_percent"] = (case["median_cycles"] / reference["median_cycles"] - 1) * 100
                            if case["frame_time_change_percent"] >= args.max_frame_regression:
                                comparison_failures.append(f"Matched-work frame-time regression reaches {args.max_frame_regression}% for {fixture['name']}/{ticks}/{palette}: {case['frame_time_change_percent']:.4f}%")
                        if comparison_failures:
                            case["comparison_failures"] = comparison_failures
                            report.setdefault("comparison_failures", []).extend(comparison_failures)
                    print(f"BENCH {args.video} {fixture['name']} ticks={ticks} palette={palette}: {case['median_cycles']/bus_hz*1000:.3f}ms DMA={case['samples'][0]['dma']['bytes']}B", flush=True)
                    (output/"report.json").write_text(json.dumps(report, indent=2)+"\n")
        report["passed"] = not bool(report.get("comparison_failures"))
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        report["error"] = str(error)
        print(f"BENCH FAIL: {error}", file=sys.stderr, flush=True)
    finally:
        if mon is not None:
            mon.socket.close()
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
        if log is not None:
            log.close()
        (output/"report.json").write_text(json.dumps(report, indent=2)+"\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
