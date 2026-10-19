#!/usr/bin/env python3
"""Capture a spoiler-free, four-SID native README showcase in calibrated VICE.

Run under the configured WSL SDK. Scenes are independently monitor-seeded;
after preparation only physical port-2 joystick input changes. Native bitmap
publications and the simultaneous stereo SID mix share emulated-cycle time.
No game, clock, health, or benchmark writes occur during recording. Bombs and
endings are excluded because bitmap reconstruction omits raster scroll/border
effects. This is emulator footage, not a physical-hardware recording.

Outputs manifest.json and per-scene scene.json, 160x200 native PNG frames,
640x480 aspect-correct screenshots, and 44.1 kHz stereo WAV. Paths in manifests
are relative to the directory containing that manifest. Encoding is separate.
"""

import argparse
import array
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
import wave


ROOT = Path(__file__).resolve().parents[1]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "tools" / filename)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


vice = module("showcase_vice", "vice-smoke.py")
bench = module("showcase_layouts", "bench-native.py")

# Local positions are simulation ticks, eight world units each. Each stage is
# 3750 ticks; carrier footage begins during its active encounter, well before
# the final boundary. Iron Canyon is captured for the still gallery only.
SCENES = (
    dict(name="title", label="H.A.V.O.C.", stage=None, local=0, seconds=4),
    dict(name="lowlands", label="Lowland Approach", stage=0, local=525, seconds=12),
    dict(name="crystal-cave", label="Crystal Cave", stage=1, local=525, seconds=10),
    dict(name="foundry-belt", label="Foundry Belt", stage=5, local=725, seconds=10),
    dict(name="storm-front", label="Storm Front", stage=8, local=525, seconds=10),
    dict(name="crossing", label="Meteor crossing", stage=9, local=3650, seconds=8),
    dict(name="carrier-combat", label="Carrier Assault", stage=11, local=1050, seconds=18),
    dict(name="title-outro", label="H.A.V.O.C.", stage=None, local=0, seconds=8),
    dict(name="iron-canyon", label="Iron Canyon", stage=2, local=625, seconds=6, gallery=True),
)
BUS_HZ = {"pal": 985248, "ntsc": 1022727}
PREROLL = 2.0
POSTROLL = 0.30


def json_write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def sha256(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def wav_metrics(path, trim_start, duration):
    """Validate only the retained mix, streaming bounded PCM chunks."""
    with wave.open(str(path), "rb") as stream:
        channels, rate, width = stream.getnchannels(), stream.getframerate(), stream.getsampwidth()
        total = stream.getnframes()
        if (channels, rate, width) != (2, 44100, 2):
            raise RuntimeError(f"Unexpected WAV format: {(channels, rate, width)}")
        first, count = round(trim_start * rate), round(duration * rate)
        if total < first + count:
            raise RuntimeError(f"Incomplete WAV: {total/rate:.6f}s; need {(first+count)/rate:.6f}s")
        stream.setpos(first)
        metrics = [dict(peak=0, square_sum=0, clipping_samples=0) for _ in range(channels)]
        remaining = count
        while remaining:
            wanted = min(remaining, 16384)
            samples = array.array("h", stream.readframes(wanted))
            if sys.byteorder != "little":
                samples.byteswap()
            if len(samples) != wanted * channels:
                raise RuntimeError("Short PCM read")
            for index, value in enumerate(samples):
                channel = metrics[index % channels]
                channel["peak"] = max(channel["peak"], abs(value))
                channel["square_sum"] += value * value
                channel["clipping_samples"] += abs(value) >= 32767
            remaining -= wanted
        for channel in metrics:
            channel["rms"] = math.sqrt(channel.pop("square_sum") / count)
            if channel["rms"] < 1:
                raise RuntimeError("A retained stereo channel is silent")
            if channel["clipping_samples"]:
                raise RuntimeError("Clipped native stereo mix")
    return dict(path=path.name, sample_rate=rate, channels=channels, sample_width=width,
                duration_seconds=total/rate, sample_frames=total,
                retained_channels=metrics, sha256=sha256(path))


def pixels(native, background):
    bitmap, screen, colors = native[:8000], native[8000:9000], native[9000:]
    rows = []
    for y in range(200):
        row = bytearray()
        for x in range(160):
            cell = (y // 8) * 40 + x // 4
            slot = (bitmap[cell * 8 + y % 8] >> (6 - (x % 4) * 2)) & 3
            color = (background, screen[cell] >> 4, screen[cell] & 15, colors[cell] & 15)[slot]
            row.extend(vice.PALETTE[color])
        rows.append(bytes(row))
    return rows


def screenshot(path, rows):
    wide = [b"".join(row[x:x+3] * 4 for x in range(0, 480, 3)) for row in rows]
    vice.png(path, [wide[y * 200 // 480] for y in range(480)], 640, 480)


class Capture:
    def __init__(self, args, scene, package, syms, layout, layouts, failures):
        self.args, self.scene, self.package = args, scene, package
        self.syms, self.layout, self.layouts, self.failures = syms, layout, layouts, failures
        self.output = args.out_dir / scene["name"]
        self.output.mkdir(parents=True, exist_ok=True)
        self.frames_dir = self.output / "frames"
        self.frames_dir.mkdir(exist_ok=True)
        self.hz = BUS_HZ[args.video]
        self.process = self.mon = None
        self.last_bits = None
        self.game_address = vice.resident_symbol(syms, "GAME")[1]
        self.game_size = layouts.size("Game")
        self.report = dict(schema=1, name=scene["name"], label=scene["label"], passed=False,
                           video=args.video, sid_count=4, sid_model="8580", bus_hz=self.hz,
                           scope="Native emulator; independently seeded scene; physical joystick only after preparation",
                           included_in_video=not scene.get("gallery", False),
                           trim_start_seconds=PREROLL, duration_seconds=args.duration or scene["seconds"],
                           native_width=160, native_height=200, display_width=640, display_height=480,
                           frames=[], screenshot="screenshot.png", setup_patches=[])

    def boot(self):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        startup = self.output / "startup.mon"
        startup.write_text("jpdb 0 $1f\njpdb 1 $1f\n")
        command = [str(self.args.vice), "-directory", str(self.args.roms), "+logtofile", "+logcolorize",
                   "-" + self.args.video, "-warp", "-sound", "-sounddev", "alsa", "-soundarg", "null",
                   "-soundoutput", "2", "-soundrate", "44100", "-soundfragsize", "0", "-soundwarpmode", "1",
                   "-jamaction", "5", "-autostartprgmode", "1", "-reu", "-reusize", "16384",
                   "-reuimage", str(self.args.package_dir / "havoc.reu"), "+reuimagerw",
                   "-sidmodel", "1", "-sidextra", "3", "-sid2address", "0xd420", "-sid3address", "0xd440", "-sid4address", "0xd460",
                   "-joydev1", "0", "-joydev2", "0", "-controlport1device", "iosim", "-controlport2device", "iosim",
                   "-remotemonitor", "-remotemonitoraddress", f"127.0.0.1:{port}",
                   "-initbreak", "0x080d", "-moncommands", str(startup), "-autostart", str(self.args.package_dir / "havoc.prg")]
        for name in ("config", "cache"):
            (self.output / name).mkdir(exist_ok=True)
        env = dict(os.environ, XDG_CONFIG_HOME=str(self.output / "config"), XDG_CACHE_HOME=str(self.output / "cache"))
        self.report["command"] = command
        with (self.output / "vice.log").open("w") as log:
            self.process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log,
                                            stderr=subprocess.STDOUT, start_new_session=True)
        deadline = time.monotonic() + self.args.timeout
        while self.mon is None:
            if self.process.poll() is not None:
                raise RuntimeError(f"VICE exited {self.process.returncode}; see {self.output/'vice.log'}")
            try:
                self.mon = vice.Monitor(port, self.args.timeout)
            except ConnectionRefusedError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("VICE monitor startup timeout")
                time.sleep(.1)
        self.mon.command("delete")
        self.mon.command("sidefx off")
        for address in self.failures:
            self.mon.command(f"break exec {address:04x}")
        self.mon.run_until(self.syms["frame_complete"], self.failures)

    def game(self):
        self.mon.command("bank ram")
        raw = self.mon.read(self.game_address, self.game_size)
        self.mon.command("bank cpu")
        return self.layouts.decode("Game", raw)

    def patch(self, values):
        if "audio_start_cycle" in self.report:
            raise RuntimeError("Game writes are forbidden after recording starts")
        values = vice.stage_fixture(values, self.layout, self.package)
        self.mon.command("bank ram")
        for name, value in values.items():
            offset, kind = self.layouts.member("Game", name)
            size = self.layouts.size(kind)
            if size > 4:
                raise RuntimeError(f"Not a scalar setup field: {name}")
            raw = int(value).to_bytes(size, "little", signed=kind.startswith("i"))
            address = self.game_address + offset
            self.mon.command(f"> {address:04x} " + " ".join(f"{v:02x}" for v in raw))
            if self.mon.read(address, size) != raw:
                raise RuntimeError(f"Failed scene setup field {name}")
        self.mon.command("bank cpu")
        self.report["setup_patches"].append(values)

    def joystick(self, bits):
        if bits != self.last_bits:
            self.mon.command(f"jpdb 1 ${31 ^ bits:02x}")
            self.last_bits = bits

    def pilot(self, state, frame):
        if self.scene["stage"] is None or self.scene["name"] == "crossing" or state["mode"] != 1:
            return 0
        target_x = target_y = 0
        if state["boss_started"] and not state["boss_defeated"]:
            # Show the opening fight without deliberately finishing its core.
            target_x = -100 if state["boss_left"] else 100
            target_y = 30
        else:
            candidates = [actor for actor in state["actors"] if actor["kind"] and actor["hp"] != 255
                          and 300 < actor["pos"]["z"] < 2400]
            if candidates:
                actor = min(candidates, key=lambda candidate: candidate["pos"]["z"])
                entry = 0xb0000 + state["stage_id"] * 2048 + 88 + (actor["kind"] - 1) * 8
                center = int.from_bytes(self.package[entry+1:entry+2], "little", signed=True)
                target_x, target_y = actor["pos"]["x"], actor["pos"]["y"] + center
        dx, dy = target_x - state["player"]["x"], target_y - state["player"]["y"]
        bits = 16 | (8 if dx > 5 else 4 if dx < -5 else 0) | (1 if dy > 5 else 2 if dy < -5 else 0)
        # Lateral double taps exercise the native dodge; FIRE stays held to
        # avoid both bomb gestures and a synthetic repeated-fire soundtrack.
        if frame % 12 in (0, 2):
            bits &= ~12
        elif frame % 12 in (1, 3):
            bits = (bits & ~8) | 4
        return bits

    def prepare(self):
        if self.scene["stage"] is None:
            return
        self.joystick(16)
        for _ in range(12):
            self.mon.run_until(self.syms["frame_complete"], self.failures)
            if self.game()["mode"] == 1:
                break
        else:
            raise RuntimeError("Physical FIRE did not start the game")
        self.joystick(0)
        stage, local = self.scene["stage"], self.scene["local"]
        # Fresh boot supplies empty actor/shot pools. Native encounter spawning
        # advances normally from this checkpoint; no actors are fabricated.
        setup = dict(mode=1, stage_id=stage, rail_ticks=stage*3750+local,
                     spawn_segment=stage, spawn_cursor=0, menu_ticks=0, ticks=local,
                     boss_started=0, boss_defeated=0, boss_z=1800,
                     boss_left=20, boss_right=20, boss_core=60)
        if self.scene["name"] == "crossing":
            setup["rail_ticks"] -= 100  # Leave room for cache/gesture warmup.
        self.patch(setup)
        before = self.mon.registers()["bus_cycles"]
        for _ in range(80):
            self.mon.run_until(self.syms["frame_complete"], self.failures)
            counters = vice.counters(self.mon, self.syms)
            if (counters.get("render_segment") == stage and counters["game_stage"] == stage
                    and counters["bus_cycles"] - before >= self.hz):
                vice.enemy_bank_check(self.mon, self.syms, self.package, stage, self.game())
                break
        else:
            raise RuntimeError("Native stage/model warmup did not settle")
        if self.scene["name"] == "crossing":
            # The two-second audio preroll leads naturally into the crossing.
            self.patch(dict(rail_ticks=(stage+1)*3750-100, stage_id=stage))

    def capture_frame(self, index, state):
        counters = vice.counters(self.mon, self.syms)
        if any(counters.get(name, 0) for name in ("frame_dropped", "input_overflows", "music_underruns")):
            raise RuntimeError(f"Capture lost native data: {counters}")
        if counters["test_input"] != 255:
            raise RuntimeError("Capture unexpectedly uses synthetic benchmark input")
        if state["mode"] in (3, 4) or state["boss_defeated"]:
            raise RuntimeError("Spoiler-free capture reached a result or boss defeat")
        if self.mon.integer(self.syms["BOMB_EFFECT"], 1):
            raise RuntimeError("Bomb raster effect cannot be represented by native bitmap capture")
        bank = counters["front_bank"]
        if bank not in (2, 3):
            raise RuntimeError(f"Invalid front bank {bank}")
        parts = []
        self.mon.command("bank ram")
        for suffix, address, length in (("bitmap", bank*0x4000+0x2000, 8000), ("screen", bank*0x4000, 1000)):
            path = self.output / f"scratch-{suffix}.bin"
            self.mon.command(f'bsave "{path.as_posix()}" 0 {address:04x} {address+length-1:04x}')
            raw = path.read_bytes()
            if len(raw) != length:
                raise RuntimeError("Incomplete native framebuffer dump")
            parts.append(raw)
        self.mon.command("bank io")
        path = self.output / "scratch-color.bin"
        self.mon.command(f'bsave "{path.as_posix()}" 0 d800 dbe7')
        parts.append(path.read_bytes())
        background = self.mon.integer(0xd021, 1) & 15
        scroll = (self.mon.integer(0xd016, 1) & 7, self.mon.integer(0xd011, 1) & 7)
        border = self.mon.integer(0xd020, 1) & 15
        self.mon.command("bank cpu")
        if scroll != (0, 3) or border != 0:
            raise RuntimeError(f"Unexpected raster presentation: scroll={scroll}, border={border}")
        native = b"".join(parts)
        if len(native) != 10000:
            raise RuntimeError("Incomplete native bitmap/screen/color data")
        rows = pixels(native, background)
        path = self.frames_dir / f"{index:06d}.png"
        vice.png(path, rows, 160, 200)
        elapsed = (counters["bus_cycles"] - self.report["audio_start_cycle"]) / self.hz
        item = dict(path=path.relative_to(self.output).as_posix(), bus_cycles=counters["bus_cycles"],
                    seconds=elapsed, stage=state["stage_id"], mode=state["mode"],
                    rail_ticks=state["rail_ticks"], frame_serial=counters["frame_serial"],
                    music_cue=counters["music_cue"], music_page=counters["music_page"],
                    shield=state["shield"], lives=state["lives"], score=state["score"],
                    boss_left=state["boss_left"], boss_right=state["boss_right"], boss_core=state["boss_core"],
                    native_sha256=hashlib.sha256(native).hexdigest())
        self.report["frames"].append(item)
        return elapsed, rows

    def run(self):
        try:
            self.boot()
            self.prepare()
            self.mon.command("warp off")
            trace_path = self.output / "sid-gates.log"
            trace_path.unlink(missing_ok=True)
            self.mon.command(f'logname "{trace_path.as_posix()}"')
            for chip in range(4):
                for register in (4, 11, 18):
                    self.mon.command(f"trace store {0xd400 + chip*32 + register:04x} if @cpu:$01 == $35")
            self.mon.command("log on")
            wav = self.output / "audio.wav"
            self.mon.command(f'resourceset "SoundRecordDeviceArg" "{wav.as_posix()}"')
            self.mon.command('resourceset "SoundRecordDeviceName" "wav"')
            self.mon.command('resourceset "Sound" "1"')
            self.report["audio_start_cycle"] = self.mon.registers()["bus_cycles"]
            self.report["audio_origin_uncertainty_seconds"] = 65 / self.hz
            endpoint = PREROLL + self.report["duration_seconds"]
            self.report["retained_end_seconds"] = endpoint
            chosen = None
            candidate_names = set()
            index = 0
            while True:
                state = self.game()
                elapsed, rows = self.capture_frame(index, state)
                # Save several genuine candidates for visual selection. The
                # default screenshot is near the midpoint of retained footage.
                if PREROLL <= elapsed <= endpoint:
                    midpoint_distance = abs(elapsed - (PREROLL + self.report["duration_seconds"] / 2))
                    if chosen is None or midpoint_distance < chosen:
                        screenshot(self.output / "screenshot.png", rows)
                        self.report["screenshot_frame"] = index
                        chosen = midpoint_distance
                    for fraction in (.2, .4, .6, .8):
                        name = f"candidate-{round(fraction*100):02d}.png"
                        if elapsed >= PREROLL + fraction * self.report["duration_seconds"] and name not in candidate_names:
                            screenshot(self.output / name, rows)
                            candidate_names.add(name)
                if elapsed >= endpoint + POSTROLL:
                    break
                self.joystick(self.pilot(state, index))
                self.mon.run_until(self.syms["frame_complete"], self.failures)
                index += 1
                if index > 5000:
                    raise RuntimeError("Capture exceeded its native frame limit")
                if index % 25 == 0:
                    print(f"CAPTURE {self.scene['name']}: {elapsed:.2f}s, {index} native frames", flush=True)
            self.joystick(0)
            self.mon.command('resourceset "Sound" "0"')
            self.mon.run_until(self.syms["frame_complete"], self.failures)
            self.mon.command("log off")
            self.report["audio"] = wav_metrics(wav, PREROLL, self.report["duration_seconds"])
            first_cycle = self.report["audio_start_cycle"] + round(PREROLL * self.hz)
            last_cycle = self.report["audio_start_cycle"] + round(endpoint * self.hz)
            events = [event for event in vice.stores(trace_path) if first_cycle <= event["bus_cycles"] < last_cycle]
            activity = vice.sid_summary(trace_path.name, events, 4)
            if any(not activity[name]["gate_on"] for name in ("left_d400", "right_d420", "socket1_d440", "socket2_d460")):
                raise RuntimeError("A SID chip had no active voice gates during retained footage")
            self.report["sid_activity"] = activity
            if chosen is None:
                raise RuntimeError("No retained screenshot frame")
            self.report.update(passed=True, game_writes_during_recording=0,
                               input_source="physical port 2", bombs_used=0, result_screens=0,
                               native_frame_count=len(self.report["frames"]))
            print(f"RECORDED {self.scene['name']}: {self.report['duration_seconds']}s + preroll/tail; "
                  f"{len(self.report['frames'])} native frames", flush=True)
            return self.report
        except Exception as error:
            self.report["error"] = str(error)
            raise
        finally:
            if self.mon is not None:
                self.mon.socket.close()
            if self.process is not None and self.process.poll() is None:
                os.killpg(self.process.pid, signal.SIGTERM)
                self.process.wait(timeout=5)
            json_write(self.output / "scene.json", self.report)


def main():
    sibling = ROOT.parent / "doomc64u"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", type=Path, default=ROOT / "dist/4sid")
    parser.add_argument("--video", choices=tuple(BUS_HZ), default="pal")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "target/readme-showcase")
    parser.add_argument("--scene", action="append", choices=[scene["name"] for scene in SCENES],
                        help="capture selected scene(s), retaining storyboard order")
    parser.add_argument("--duration", type=float, help="override each selected duration for short probes")
    parser.add_argument("--capture-only", action="store_true", help="explicitly request native frames/WAV (encoding is always separate)")
    parser.add_argument("--vice", type=Path, default=Path(os.environ.get("VICE_BIN", sibling / "target/audit-fast/x64sc-64x")))
    parser.add_argument("--roms", type=Path, default=sibling / "target/tools/secondreality/vice/data")
    parser.add_argument("--llvm-nm", type=Path, default=Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin/llvm-nm")
    parser.add_argument("--timeout", type=float, default=45)
    args = parser.parse_args()
    if args.duration is not None and (not math.isfinite(args.duration) or not .5 <= args.duration <= 60
                                      or abs(args.duration * 50 - round(args.duration * 50)) > 1e-7):
        parser.error("--duration must be 0.5..60 seconds in multiples of 20 milliseconds")
    if not 1 <= args.timeout <= 60:
        parser.error("--timeout must be between 1 and 60 seconds")
    for name in ("package_dir", "out_dir", "vice", "roms", "llvm_nm"):
        setattr(args, name, getattr(args, name).resolve())
    elf = args.package_dir / "havoc.elf"
    for path in (elf, args.package_dir / "havoc.prg", args.package_dir / "havoc.reu", args.vice, args.roms, args.llvm_nm):
        if not path.exists():
            raise RuntimeError(f"Required path missing: {path}")
    package = (args.package_dir / "havoc.reu").read_bytes()
    if len(package) != 16 * 1024 * 1024 or package[16:18] != bytes((12, 4)):
        raise RuntimeError("README showcase requires the twelve-stage four-SID package")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    syms = vice.symbols(elf, args.llvm_nm)
    for name in ("BOMB_EFFECT", "MUSIC_UNDERRUNS", "GAME_STAGE", "RENDER_SEGMENT"):
        if name not in syms:
            raise RuntimeError(f"Missing native capture diagnostic: {name}")
    failures = {address: name for name, address in syms.items()
                if ("havoc_c64" in name and "4fail" in name) or name == "rust_begin_unwind"}
    if not failures:
        raise RuntimeError("Missing native fail/panic breakpoints")
    dwarf = args.llvm_nm.with_name("llvm-dwarfdump")
    layout = vice.dwarf_game_layout(elf, dwarf, syms, args.out_dir)
    layouts = bench.Layouts(elf, dwarf, args.out_dir)
    hashes = {name: sha256(args.package_dir / name) for name in ("havoc.prg", "havoc.reu", "havoc.elf")}
    report = dict(schema=1, passed=False, video=args.video, sid_count=4, sid_model="8580", bus_hz=BUS_HZ[args.video],
                  scope="Selected native-emulator scenes at real emulated pacing; simultaneous four-SID stereo mix",
                  package=str(args.package_dir), package_sha256=hashes, vice_sha256=sha256(args.vice), scenes=[])
    try:
        for scene in SCENES:
            if args.scene and scene["name"] not in args.scene:
                continue
            result = Capture(args, scene, package, syms, layout, layouts, failures).run()
            report["scenes"].append(dict(name=scene["name"], label=scene["label"], directory=scene["name"],
                manifest=f"{scene['name']}/scene.json", included_in_video=result["included_in_video"],
                duration_seconds=result["duration_seconds"], passed=result["passed"]))
            json_write(args.out_dir / "manifest.json", report)
        if hashes != {name: sha256(args.package_dir / name) for name in hashes}:
            raise RuntimeError("Input package changed during capture")
        report.update(passed=True, package_files_unchanged=True)
    finally:
        json_write(args.out_dir / "manifest.json", report)


if __name__ == "__main__":
    main()
