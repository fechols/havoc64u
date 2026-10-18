#!/usr/bin/env python3
"""Deterministic native renderer benchmarks; never writes the input package.

Build with scripts/build-benchmark.sh. Fixtures use matching ELF/DWARF layouts,
live 50 Hz audio, fixed simulation work, and a common VIC raster starting line.
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


def fixtures():
    result = []
    for name, rail in (("title", 0), ("lowlands", 625), ("canyon", 4375),
                       ("base", 8125), ("carrier", 12025), ("missiles", 4375),
                       ("full-pools", 12025), ("section-boundary", 3749)):
        patch = {"mode": 0 if name == "title" else 1, "rail_ticks": rail, "ticks": 0,
                 "player.x": 0, "player.y": 0, "player.z": 180,
                 "spawn_segment": rail // 3750, "spawn_cursor": 255,
                 "shield": 100, "lives": 3, "bombs": 3, "invulnerability": 255}
        if name in ("carrier", "full-pools"):
            patch.update(boss_started=1, boss_z=680, boss_left=20, boss_right=20, boss_core=60)
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
            patch["controls.released_at"] = (1 << 32) - 2100
        if name == "full-pools":
            for array, count, fields in (
                ("actors", 12, {"kind": 3, "hp": 6, "pos.z": 100}),
                ("shots", 24, {"kind": 2, "damage": 12, "pos.z": 300}),
                ("pickups", 4, {"kind": 1, "pos.z": 400}),
                ("explosions", 6, {"ticks": 9, "color": 7, "pos.z": 500}),
            ):
                for index in range(count):
                    for field, value in fields.items():
                        if array == "actors" and field == "kind":
                            value = 1 + index % 2
                        patch[f"{array}[{index}].{field}"] = value
        result.append({"name": name, "patch": patch, "section": rail // 3750,
                       "cache_miss": name == "section-boundary", "input_bits": 0})
    for section in range(3):
        # Four final meteor streaks overlap at tick 306, before the oldest
        # expires at tick 310; midpoint alone misses this geometry peak.
        progress_ticks = (0, 200, 306, 399) if section == 2 else (0, 200, 399)
        for progress in progress_ticks:
            patch = dict(result[1]["patch"], mode=6, rail_ticks=(section+1)*3750,
                         transition_ticks=progress, spawn_segment=section)
            result.append({"name": f"bridge-{section}-{progress}", "patch": patch,
                           "section": section, "cache_miss": False, "input_bits": 16})
    # Exercise the real per-level enemies, including simultaneous escorts and
    # carrier weak points. Warm and cold model-bank fixtures are distinct.
    for section, label in enumerate(("lowlands", "canyon", "fortress", "carrier")):
        for saturated in (False, True):
            patch = dict(result[1]["patch"], rail_ticks=section*3750+625,
                         spawn_segment=section)
            if section == 3:
                patch.update(boss_started=1, boss_z=680, boss_left=20, boss_right=20, boss_core=60)
            count = 12 if saturated else (2 if section == 3 else 3)
            for index in range(count):
                kind = 1 + index % (2 if section == 3 else 3)
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
    return result


def cache_sequence():
    """Entry/exit histories that can leave stale pixels in either VIC bank."""
    base = next(fixture for fixture in fixtures() if fixture["name"] == "lowlands")

    def variant(name, patch, **metadata):
        return dict(base, name=name, patch=dict(base["patch"], **patch), **metadata)

    occluder = variant("sky-occluder", {"actors[0].kind": 1, "actors[0].hp": 6,
        "actors[0].origin_x": 0, "actors[0].origin_y": 340,
        "actors[0].pos.x": 0, "actors[0].pos.y": 340, "actors[0].pos.z": 1100})
    banner = variant("sector-banner", {"rail_ticks": 25})
    boss = variant("boss-hud", {"boss_started": 1, "boss_z": 680,
        "boss_left": 20, "boss_right": 20, "boss_core": 60})
    title = variant("title-transition", {"mode": 0})
    sequence = [base]*3 + [occluder]*2 + [base]*3 + [banner]*2 + [base]*3 + [boss]*2 + [base]*3 + [title] + [base]*3
    themes = {fixture["section"]: fixture for fixture in fixtures()
              if fixture["name"] in ("lowlands", "canyon", "base", "carrier")}

    def theme_frame(section, kind, stable=False):
        theme = themes[section]
        return variant(f"theme-{theme['name']}-{kind}",
                       {"rail_ticks": theme["patch"]["rail_ticks"], "spawn_segment": section},
                       section=section, theme_check=True, stable_check=stable)

    # Four frames allow both VIC banks to repaint, then demonstrate sustained
    # reuse. Returning to lowlands exercises a complete 0 -> 1 -> 2 -> 3 -> 0 loop.
    for section in (0, 1, 2, 3, 0):
        sequence.extend(theme_frame(section, "stable", frame == 3) for frame in range(4))
    # Switch every frame in both directions, including returning to the theme
    # last present in the back bank. Neither bank may retain that old validity.
    sequence.extend(theme_frame(section, "rapid") for section in (1, 2, 3, 0, 3, 2, 1, 0))
    sequence.extend(theme_frame(0, "recovery", frame == 2) for frame in range(3))
    # Append so the original occluder/HUD/transition indices remain stable.
    # Sweep both camera directions while terrain scrolls across row boundaries;
    # finish with an identical state in both banks to prove reuse recovers.
    heights = (-65, -48, -32, -16, 0, 16, 32, 48, 65,
               48, 32, 16, 0, -16, -32, -48, -65)
    for section in range(4):
        rail = themes[section]["patch"]["rail_ticks"]
        for frame, height in enumerate(heights):
            sequence.append(variant(f"camera-{section}-{frame}",
                {"rail_ticks": rail + frame*7, "spawn_segment": section,
                 "player.y": height}, section=section, theme_check=True, camera_check=True))
        for frame in range(3):
            sequence.append(variant(f"camera-{section}-recovery-{frame}",
                {"rail_ticks": rail + (len(heights)-1)*7, "spawn_segment": section,
                 "player.y": heights[-1]}, section=section, theme_check=True,
                camera_check=True, stable_check=frame == 2))
    for section in range(3):
        progress_ticks = (0, 100, 200, 300, 306, 399) if section == 2 else (0, 100, 200, 300, 399)
        for progress in progress_ticks:
            sequence.append(variant(f"bridge-{section}-{progress}",
                {"mode": 6, "rail_ticks": (section+1)*3750,
                 "transition_ticks": progress, "spawn_segment": section},
                section=section, theme_check=True, bridge_check=True))
        for frame in range(3):
            sequence.append(theme_frame(section+1, f"bridge-recovery-{frame}", frame == 2))
    return sequence


def cloud_only_bands(bands, geometry):
    """Reference cloud-only eligibility using the same fixed backdrop geometry."""
    background = [triangle for triangle in geometry if triangle["reserved"] & 2]
    horizons = [triangle for triangle in background if triangle["depth"] == 2350]
    if len(background) != 14 or len(horizons) != 5:
        raise RuntimeError("Cloud-only reference requires all 14 fixed backdrop triangles")
    for triangle in horizons:
        top = max(0, min(200, min(triangle["y"])))
        bottom = max(0, min(200, max(triangle["y"])))
        if top < bottom:
            first, end = top // 8, (bottom + 7) // 8
            bands &= ~(((1 << (end-first))-1) << first)
    return bands


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


def native_bytes(mon, syms):
    bank = mon.integer(syms["FRONT_BANK"], 1)
    bitmap, screen = (0xe000, 0xc000) if bank == 3 else (0xa000, 0x8000)
    mon.command("bank ram")
    result = mon.read(bitmap, 8000) + mon.read(screen, 1000) + mon.read(0x9100, 1000)
    mon.command("bank cpu")
    return result


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
    parser.add_argument("--palettes", default="cold,retained")
    parser.add_argument("--real-time-ticks", type=int, default=400)
    cache_selection = parser.add_mutually_exclusive_group()
    cache_selection.add_argument("--skip-cache-checks", action="store_true", help="omit native cache transition A/B replay in quick timing runs")
    cache_selection.add_argument("--cache-checks-only", action="store_true", help="run only cache transition A/B replay, skipping timing and real-time passes")
    parser.add_argument("--cloud-only-cache", action="store_true",
                        help="reference policy: exclude horizon bands from caching without changing geometry or target code; skips real-time pass")
    parser.add_argument("--compare", type=Path, help="matching baseline report.json")
    args = parser.parse_args()
    output, package = args.out_dir.resolve(), args.package_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    chosen = [fixture for fixture in fixtures() if args.fixtures == "all" or fixture["name"] in args.fixtures.split(",")]
    ticks_values = list(map(int, args.ticks.split(",")))
    palettes = args.palettes.split(",")
    if not chosen or any(n not in (0, 10) for n in ticks_values) or any(p not in ("cold", "retained") for p in palettes):
        parser.error("invalid fixture, ticks, or palette selection")
    report = {"passed": False, "protocol": "havoc-fixed-frame-v1", "video": args.video,
              "scope": "Fixed semantic workload in calibrated VICE; not physical hardware",
              "cache_policy": "cloud-only" if args.cloud_only_cache else "full-background",
              "retained_palette_setup": "Two warmup frames of the identical fixture fill both VIC banks",
              "cases": [], "fixtures": chosen}
    process = mon = log = None
    try:
        prg, reu, elf = [package / f"havoc.{extension}" for extension in ("prg", "reu", "elf")]
        hashes = {extension: digest(path.read_bytes()) for extension, path in (("prg", prg), ("reu", reu), ("elf", elf))}
        report.update(package=str(package), hashes=hashes, vice_sha256=digest(args.vice.read_bytes()),
                      driver_sha256=digest(Path(__file__).read_bytes()),
                      smoke_tool_sha256=digest((ROOT/"tools/vice-smoke.py").read_bytes()),
                      fixture_sha256=semantic_hash(chosen), samples=args.samples,
                      command=sys.argv, driver_checkout_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                      driver_checkout_status=subprocess.check_output(["git", "status", "--short"], cwd=ROOT, text=True))
        syms = vice.symbols(elf, args.llvm_nm)
        if (args.cache_checks_only or args.cloud_only_cache) and "SKY_CACHE" not in syms:
            raise RuntimeError("Selected cache validation requires a package containing SKY_CACHE")
        for name in ("BENCH_TICKS", "BENCH_RUNTIME", "benchmark_frame_begin", "benchmark_frame_ready", "geometry_complete"):
            if name not in syms:
                raise RuntimeError(f"Package is not an instrumented benchmark build: missing {name}")
        failures = {address: name for name, address in syms.items() if ("havoc_c64" in name and "4fail" in name) or name == "rust_begin_unwind"}
        layouts = Layouts(elf, args.llvm_nm.with_name("llvm-dwarfdump"), output)
        for name in ("Game", "Runtime", "AudioEngine", "Renderer", "Triangle"):
            layouts.load(name)
        game_address = vice.resident_symbol(syms, "GAME")[1]
        audio_address = vice.resident_symbol(syms, "AUDIO")[1]
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
                   "-warp", "+sound", "-jamaction", "5", "-autostartprgmode", "1", "-reu", "-reusize", "16384",
                   "-reuimage", str(reu), "+reuimagerw", "-sidextra", "1", "-sid2address", "0xd420",
                   "-joydev1", "0", "-joydev2", "0", "-controlport1device", "iosim", "-controlport2device", "iosim",
                   "-remotemonitor", "-remotemonitoraddress", f"127.0.0.1:{port}", "-initbreak", "0x080d",
                   "-moncommands", str(startup), "-autostart", str(prg)]
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
            mon.command(f"break exec {address:04x}")
        mon.run_until(syms["benchmark_frame_begin"], failures)
        mon.command("bank ram")
        initial_game = mon.read(game_address, layouts.size("Game"))
        initial_audio = mon.read(audio_address, layouts.size("AudioEngine"))
        runtime_address = mon.integer(syms["BENCH_RUNTIME"])
        initial_runtime = mon.read(runtime_address, layouts.size("Runtime"))
        mon.command("bank cpu")
        report["initial_game"] = layouts.decode("Game", initial_game)

        def write(address, data):
            for offset in range(0, len(data), 16):
                mon.command(f"> {address+offset:04x} " + " ".join(f"{byte:02x}" for byte in data[offset:offset+16]))
            if mon.read(address, len(data)) != bytes(data):
                raise RuntimeError(f"Monitor write failed at ${address:04x}")

        def word(address, value, size=2):
            write(address, int(value).to_bytes(size, "little"))

        if args.cloud_only_cache and args.real_time_ticks and not args.cache_checks_only:
            report["real_time_skipped"] = "Cloud-only reference applies at fixed-frame geometry stops; use the native policy for ordinary scheduling"
        if args.real_time_ticks and not args.cache_checks_only and not args.cloud_only_cache:
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
        mon.command(f"trace exec {audio_tick_address:04x}")
        segment_image = reu.read_bytes()
        baseline = None
        if args.compare:
            baseline = json.loads(args.compare.read_text())
            if not baseline.get("passed"):
                raise RuntimeError("Baseline report did not pass")
            if any(baseline.get(key) != report[key] for key in ("video", "protocol", "fixture_sha256")):
                raise RuntimeError("Baseline video/protocol/fixture identity differs")
        baseline_cases = {(case["fixture"], case["ticks"], case["palette"]): case for case in baseline["cases"]} if baseline else {}

        def seed(fixture, ticks, cold, phase):
            mon.command("bank ram")
            game = bytearray(initial_game)
            for field, value in fixture["patch"].items():
                layouts.patch(game, "Game", field, value)
            # Speeds are initialized exactly as spawning would do, including
            # zero-tick missile fixtures that have not run enemy movement yet.
            if "approach_speed" in layouts.load("Actor")["fields"]:
                for index in range(12):
                    kind = fixture["patch"].get(f"actors[{index}].kind", 0)
                    if 1 <= kind <= (2 if fixture["section"] == 3 else 3):
                        descriptor = 0xb0000 + fixture["section"]*2048 + 80 + (kind-1)*8
                        layouts.patch(game, "Game", f"actors[{index}].approach_speed", segment_image[descriptor+6])
            write(game_address, game)
            write(audio_address, initial_audio)
            runtime = bytearray(initial_runtime)
            sections = [fixture["section"], (fixture["section"] + (2 if fixture["cache_miss"] else 1)) % 4]
            for field, value in (("sim_ms", 0), ("current", 0), ("loaded[0]", sections[0]), ("loaded[1]", sections[1])):
                layouts.patch(runtime, "Runtime", field, value)
            if "profile" in layouts.load("Runtime")["fields"]:
                profile = (fixture["section"] + int(fixture.get("enemy_cache_miss", False))) % 4
                layouts.patch(runtime, "Runtime", "profile", profile)
                write(0x8400, segment_image[0xb0000+profile*2048:0xb0000+(profile+1)*2048])
                word(syms["ENEMY_MODEL_PROFILE"], profile, 1)
                word(syms["ENEMY_CODE_PROFILE"], profile, 1)
            write(runtime_address, runtime)
            for slot, section in enumerate(sections):
                write(0x9500+slot*512, segment_image[0x2000+section*2048:0x2200+section*2048])
            for name, size, value in (("CLOCK_MS", 4, 0), ("INPUT_HEAD", 1, 0), ("INPUT_TAIL", 1, 0), ("LAST_BITS", 1, fixture["input_bits"])):
                word(vice.resident_symbol(syms, name)[1], value, size)
            for name, size, value in (("AUDIO_TICKS", 2, 0), ("INPUT_OVERFLOWS", 2, 0), ("FRAME_SERIAL", 2, 32),
                                      ("TEST_INPUT", 1, fixture["input_bits"]), ("BENCH_TICKS", 1, ticks)):
                word(syms[name], value, size)
            if cold:
                for address in (0x8000, 0xc000, 0x9100):
                    write(address, bytes(1000))
                if "SKY_CACHE" in syms:
                    write(syms["SKY_CACHE"], bytes(8))
            mon.command("bank cpu")
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
            return {"game_sha256": semantic_hash(layouts.decode("Game", bytes(game))),
                    "first_audio_period_cycles": first_period}

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
            count_offset, _ = layouts.member("Renderer", "triangle_count")
            count = mon.integer(renderer_address+count_offset)
            triangles_offset, _ = layouts.member("Renderer", "triangles")
            triangle_size = layouts.size("Triangle")
            return [layouts.decode("Triangle", mon.read(renderer_address+triangles_offset+n*triangle_size, triangle_size)) for n in range(count)]

        def apply_cache_policy(geometry, sky_bands):
            if args.cloud_only_cache:
                sky_bands = cloud_only_bands(sky_bands, geometry)
                # Monitor writes consume no emulated CPU cycles. Both policies
                # run identical geometry/audio code; only cache eligibility differs.
                word(syms["SKY_BANDS"], sky_bands, 4)
            return sky_bands

        def measured_frame(folder, capture_image):
            path = folder / "monitor.log"
            path.unlink(missing_ok=True)
            mon.command(f'logname "{path.as_posix()}"')
            mon.command("log on")
            start = vice.counters(mon, syms)
            cache_before = mon.read(syms["SKY_CACHE"], 8).hex() if "SKY_CACHE" in syms else None
            first_deadline = finish_timer_seed()
            mon.run_until(syms["geometry_complete"], failures)
            mon.command("bank ram")
            clear_offset, _ = layouts.member("Renderer", "clear_color")
            clear_color = mon.integer(renderer_address+clear_offset, 1)
            geometry = read_geometry()
            sky_bands = mon.integer(syms["SKY_BANDS"], 4) if "SKY_BANDS" in syms else None
            sky_bands = apply_cache_policy(geometry, sky_bands)
            cache_prepared = mon.read(syms["SKY_CACHE"], 8) if "SKY_CACHE" in syms else None
            mon.command("bank cpu")
            mon.run_until(syms["frame_complete"], failures)
            end = vice.counters(mon, syms)
            mon.command("log off")
            end["bus_cycles_delta"] = end["bus_cycles"] - start["bus_cycles"]
            end["first_audio_deadline_cycles"] = first_deadline
            events = vice.stores(path)
            vice.phase_profile(events, syms["PROF_PHASE"], start, [end])
            vice.pixel_profile(events, syms["PROF_DETAIL"], start, [end])
            transfers = dma_events(events, syms["PROF_PHASE"])
            end["dma"] = {"bytes": sum(item["bytes"] for item in transfers), "commands": len(transfers),
                          "max_bytes": max((item["bytes"] for item in transfers), default=0), "transfers": transfers,
                          "bytes_by_phase_marker": {str(phase): sum(item["bytes"] for item in transfers if item["phase_marker"] == phase)
                                                    for phase in sorted({item["phase_marker"] for item in transfers})}}
            end["audio_service_cycles"] = execution_cycles(path.read_text(errors="replace"), audio_tick_address)
            end["sid"] = vice.sid_summary(path, events)
            if any(not end["sid"][chip]["gate_on"] for chip in ("left_d400", "right_d420")):
                raise RuntimeError("Measured frame did not service live music on both SID chips")
            if cache_before is not None:
                cache_after = mon.read(syms["SKY_CACHE"], 8)
                front = end["front_bank"]
                end["sky_cache"] = {"before_hex": cache_before, "after_hex": cache_after.hex(),
                                    "prepared_hex": cache_prepared.hex(),
                                    "geometry_bands": sky_bands,
                                    "eligible_bands": int.from_bytes(cache_after[(front-2)*4:(front-1)*4], "little"),
                                    # Count actual skipped paint/pack operations;
                                    # pre-geometry masks can be invalidated by a
                                    # theme switch and do not prove reuse.
                                    "reused_bands": 25 - sum(event["address"] == syms["PROF_DETAIL"] and event["value"] == 2 for event in events)}
            mon.command("bank ram")
            state = layouts.decode("Game", mon.read(game_address, layouts.size("Game")))
            end["simulation_ticks"] = state["ticks"]
            if "ENEMY_CODE_PROFILE" in syms:
                section = vice.active_section(state["rail_ticks"], state["mode"])
                end["enemy_bank"] = vice.enemy_bank_check(mon, syms, segment_image, section)
                mon.command("bank ram")
            end["stack_guards"] = {name: mon.integer(syms[name], 1) for name in ("__stack_bottom", "__irq_stack_bottom")}
            mon.command("bank cpu")
            native = native_bytes(mon, syms)
            # Bits 2/4 cache static-sky classification and orientation for
            # PIXELS. The original renderer discarded both metadata values.
            comparable_geometry = [dict(triangle, reserved=triangle["reserved"] & ~6) for triangle in geometry]
            end.update(geometry_sha256=semantic_hash(comparable_geometry), raw_geometry_sha256=semantic_hash(geometry),
                       static_background_sha256=semantic_hash([triangle for triangle in geometry if triangle["reserved"] & 2]),
                       static_background_triangles=sum(bool(triangle["reserved"] & 2) for triangle in geometry),
                       game_sha256=semantic_hash(state), native_sha256=digest(native),
                       native_screen_sha256=digest(native[8000:9000]), native_color_sha256=digest(native[9000:]),
                       clear_color=clear_color)
            if capture_image:
                end["capture"] = vice.capture(mon, syms, folder, "frame")
                (folder/"game.json").write_text(json.dumps(state, indent=2)+"\n")
                (folder/"geometry.json").write_text(json.dumps(geometry, indent=2)+"\n")
            if end["input_overflows"] or end["frame_dropped"] or any(value != 0xa5 for value in end["stack_guards"].values()):
                raise RuntimeError("Fixed-work frame dropped data or changed a stack guard")
            if end.get("music_underruns", 0):
                raise RuntimeError("Fixed-work frame underrun the score prefetch")
            if end["dma"]["max_bytes"] > 1024:
                raise RuntimeError("Runtime DMA exceeded the 1024-byte IRQ latency bound")
            service = end["audio_service_cycles"]
            if not service:
                raise RuntimeError("No traced audio-service entries in measured frame")
            lateness = [cycle - (first_deadline+index*period) for index, cycle in enumerate(service)]
            end["audio_lateness_cycles"] = lateness
            end["max_audio_lateness_ms"] = max(lateness)*1000/bus_hz
            if min(lateness) < -2 or max(lateness) > bus_hz*0.002:
                raise RuntimeError(f"Audio deadline/latency failure: {lateness}")
            if end["audio_ticks"] != len(service):
                raise RuntimeError("Audio counter does not match traced services")
            return end

        for fixture in ([] if args.cache_checks_only else chosen):
            for ticks in ticks_values:
                for palette in palettes:
                    case = {"fixture": fixture["name"], "ticks": ticks, "palette": palette, "samples": []}
                    report["cases"].append(case)
                    for sample_index in range(args.samples):
                        phase = sample_index % 3
                        folder = output / f"{fixture['name']}-{ticks}-{palette}" / str(sample_index)
                        folder.mkdir(parents=True, exist_ok=True)
                        if palette == "retained":
                            # Each VIC bank keeps its own palette. Render the
                            # same post-simulation state into both banks first.
                            for warmup in range(2):
                                seed(fixture, ticks, warmup == 0, phase)
                                finish_timer_seed()
                                if args.cloud_only_cache:
                                    mon.run_until(syms["geometry_complete"], failures)
                                    mon.command("bank ram")
                                    apply_cache_policy(read_geometry(), mon.integer(syms["SKY_BANDS"], 4))
                                    mon.command("bank cpu")
                                mon.run_until(syms["frame_complete"], failures)
                                mon.run_until(syms["benchmark_frame_ready"], failures)
                        seeded = seed(fixture, ticks, palette == "cold", phase)
                        sample = measured_frame(folder, sample_index == 0)
                        if fixture["patch"]["mode"] in (1, 6):
                            expected_ticks = (fixture["patch"].get("ticks", 0) + ticks) & 0xffff
                            if sample["simulation_ticks"] != expected_ticks:
                                raise RuntimeError("Code/model reentry skipped or duplicated fixed simulation ticks")
                        sample.update(initial=seeded, cia_phase=phase)
                        if case["samples"] and any(sample[key] != case["samples"][0][key] for key in ("geometry_sha256", "game_sha256", "native_sha256")):
                            raise RuntimeError("Identical fixture repeats changed geometry, game state, or pixels")
                        case["samples"].append(sample)
                        mon.run_until(syms["benchmark_frame_ready"], failures)
                    case.update(median_cycles=statistics.median(sample["bus_cycles_delta"] for sample in case["samples"]),
                                worst_cycles=max(sample["bus_cycles_delta"] for sample in case["samples"]),
                                max_audio_lateness_ms=max(sample["max_audio_lateness_ms"] for sample in case["samples"]))
                    reference = baseline_cases.get((fixture["name"], ticks, palette))
                    if reference:
                        for key in ("geometry_sha256", "game_sha256", "native_sha256"):
                            if case["samples"][0][key] != reference["samples"][0][key]:
                                raise RuntimeError(f"Baseline comparison failed for {fixture['name']}/{ticks}/{palette}: {key}")
                        case["speedup"] = reference["median_cycles"]/case["median_cycles"]
                    print(f"BENCH {args.video} {fixture['name']} ticks={ticks} palette={palette}: {case['median_cycles']/bus_hz*1000:.3f}ms DMA={case['samples'][0]['dma']['bytes']}B", flush=True)
                    (output/"report.json").write_text(json.dumps(report, indent=2)+"\n")
        if "SKY_CACHE" in syms and not args.skip_cache_checks:
            cache_report = {"passed": False, "sequence": cache_sequence(), "runs": {}}
            report["cache_checks"] = cache_report
            for forced in (False, True):
                name = "forced-misses" if forced else "normal-cache"
                samples = []
                cache_report["runs"][name] = samples
                for index, fixture in enumerate(cache_report["sequence"]):
                    folder = output / "cache-checks" / name / str(index)
                    folder.mkdir(parents=True, exist_ok=True)
                    if index == 0:
                        word(syms["FRONT_BANK"], 3, 1)
                    seed(fixture, 0, index == 0, 0)
                    if forced:
                        write(syms["SKY_CACHE"], bytes(8))
                    sample = measured_frame(folder, False)
                    samples.append(sample)
                    mon.run_until(syms["benchmark_frame_ready"], failures)
            normal, forced = [cache_report["runs"][name] for name in ("normal-cache", "forced-misses")]
            theme_age = 0
            camera_backgrounds, camera_geometry = {}, {}
            for index, (cached, uncached) in enumerate(zip(normal, forced)):
                fixture = cache_report["sequence"][index]
                switched = index > 0 and fixture["section"] != cache_report["sequence"][index-1]["section"]
                theme_age = 0 if switched else theme_age + 1
                for key in ("geometry_sha256", "game_sha256", "native_sha256", "native_screen_sha256", "native_color_sha256", "clear_color"):
                    if cached[key] != uncached[key]:
                        raise RuntimeError(f"Cache transition {index} changed {key}")
                if uncached["sky_cache"]["reused_bands"] != 0:
                    raise RuntimeError("Forced cache miss unexpectedly reused a band")
                if fixture.get("theme_check"):
                    cache = cached["sky_cache"]
                    expected_sky = (6, 8, 11, 4)[fixture["section"]]
                    if fixture.get("bridge_check"):
                        expected_sky = ((6, 14, 10, 8), (8, 9, 12, 11), (11, 6, 14, 4))[fixture["section"]][fixture["patch"]["transition_ticks"] // 100]
                        if cache["eligible_bands"] or cache["reused_bands"] or cache["prepared_hex"] != bytes(8).hex():
                            raise RuntimeError(f"Scenic flight {index} retained static sky cache data")
                    if cached["clear_color"] != expected_sky:
                        raise RuntimeError(f"Theme transition {index} selected the wrong sky color")
                    if switched and cache["prepared_hex"] != bytes(8).hex():
                        raise RuntimeError(f"Theme transition {index} did not invalidate both VIC banks")
                    if theme_age < 2 and cache["reused_bands"]:
                        raise RuntimeError(f"Theme transition {index} reused sky before both VIC banks repainted")
                    if not cache["eligible_bands"] and cache["reused_bands"]:
                        raise RuntimeError(f"Theme transition {index} reused an ineligible sky band")
                    if fixture.get("stable_check") and cache["eligible_bands"] and cache["reused_bands"] <= 0:
                        raise RuntimeError(f"Stable theme {fixture['section']} never reused its eligible native sky")
                if fixture.get("camera_check"):
                    section = fixture["section"]
                    if cached["static_background_triangles"] != 14:
                        raise RuntimeError(f"Camera sweep {index} did not retain all 14 backdrop triangles")
                    reference = camera_backgrounds.setdefault(section, cached["static_background_sha256"])
                    if cached["static_background_sha256"] != reference:
                        raise RuntimeError(f"Camera sweep {index} moved the static backdrop")
                    camera_geometry.setdefault(section, set()).add(cached["geometry_sha256"])
            if any(len(camera_geometry.get(section, ())) < 2 for section in range(4)):
                raise RuntimeError("Camera sweeps did not move foreground geometry in all four scenes")
            if normal[2]["sky_cache"]["reused_bands"] <= 0:
                raise RuntimeError("Stable native sky never exercised cache reuse")
            stable = normal[2]["sky_cache"]["eligible_bands"]
            if stable & ~normal[3]["sky_cache"]["eligible_bands"] == 0:
                raise RuntimeError("Native occluder fixture did not invalidate a cached sky band")
            if not stable & 0x78 or normal[8]["sky_cache"]["eligible_bands"] & 0x78:
                raise RuntimeError("Sector banner cache exclusion was not exercised")
            if not stable & 0x6 or normal[13]["sky_cache"]["eligible_bands"] & 0x6:
                raise RuntimeError("Boss HUD cache exclusion was not exercised")
            if normal[18]["sky_cache"]["eligible_bands"] or normal[19]["sky_cache"]["reused_bands"]:
                raise RuntimeError("Title-to-playing transition reused stale sky")
            cache_report["passed"] = True
            print(f"CACHE {args.video}: {len(normal)} transition frames match forced misses exactly", flush=True)
        report["package_unchanged"] = all(digest(path.read_bytes()) == hashes[extension] for extension, path in (("prg", prg), ("reu", reu), ("elf", elf)))
        if not report["package_unchanged"]:
            raise RuntimeError("Input package changed during benchmarking")
        report["passed"] = True
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
