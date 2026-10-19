"""Record full campaign cues through native VICE SID playback as stereo WAV.

Requires an instrumented package with the normal frame/audio diagnostics. The
monitor holds a harmless respawn scene and rewinds the opening score page once
before recording; IRQ playback and foreground page refill remain native. These
listening previews are emulator audio, not a physical SID recording or a frame
rate benchmark. No third-party Python packages are required.
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
import time
import wave


ROOT = Path(__file__).resolve().parents[1]
SCORE_OFFSET = 0x100000
CUES = {
    7: ("crystal-current", 1, False),
    8: ("ashes-and-steel", 4, False),
    9: ("tempest-drive", 8, False),
    10: ("gorge-warden", 3, True),
    11: ("citadel-heart", 7, True),
    12: ("carrier-overdrive", 11, True),
}


def wav_metrics(path, seconds):
    with wave.open(str(path), "rb") as stream:
        channels, rate, width = stream.getnchannels(), stream.getframerate(), stream.getsampwidth()
        frames = stream.getnframes()
        samples = stream.readframes(frames)
    if (channels, rate, width) != (2, 44100, 2):
        raise RuntimeError(f"Unexpected WAV format: {(channels, rate, width)}")
    expected = round(seconds * rate)
    if frames < expected:
        raise RuntimeError(f"Incomplete recording: {frames / rate:.3f}s, need {seconds:.3f}s")
    # The monitor stops on rendered frames. Remove its final partial score page
    # so each downloadable file has exactly one authored loop's duration.
    samples = samples[:expected * channels * width]
    with wave.open(str(path), "wb") as stream:
        stream.setnchannels(channels)
        stream.setsampwidth(width)
        stream.setframerate(rate)
        stream.writeframes(samples)
    values = array.array("h", samples)
    if os.sys.byteorder != "little":
        values.byteswap()
    metrics = []
    for channel in range(channels):
        lane = values[channel::channels]
        metrics.append({"peak": max(abs(value) for value in lane),
                        "rms": math.sqrt(sum(value * value for value in lane) / len(lane)),
                        "clipping_samples": sum(abs(value) >= 32767 for value in lane)})
    if any(lane["rms"] < 1 for lane in metrics):
        raise RuntimeError("Silent recording channel")
    return {"duration_seconds": expected / rate, "sample_rate": rate,
            "samples": expected, "channels": metrics,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def main():
    sibling = ROOT.parent / "doomc64u"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--video", choices=("pal", "ntsc"), default="pal")
    parser.add_argument("--cues", type=int, nargs="+", choices=tuple(CUES), default=list(CUES))
    parser.add_argument("--vice", type=Path, default=Path(os.environ.get("VICE_BIN", sibling / "target/audit-fast/x64sc-64x")))
    parser.add_argument("--roms", type=Path, default=sibling / "target/tools/secondreality/vice/data")
    parser.add_argument("--llvm-nm", type=Path, default=Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin/llvm-nm")
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("havoc_smoke", ROOT / "tools/vice-smoke.py")
    smoke = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(smoke)
    package = args.package_dir.resolve()
    elf, reu, prg = (package / f"havoc.{suffix}" for suffix in ("elf", "reu", "prg"))
    data = reu.read_bytes()
    header = data[SCORE_OFFSET:SCORE_OFFSET + 128]
    if header[:4] != b"SMU2" or header[6] != 13 or header[4] not in (2, 4):
        raise RuntimeError("Expected the thirteen-cue campaign SMU2 bank")
    sid_count = header[4]
    output = (args.out_dir or ROOT / "target/music-previews" / f"{sid_count}sid").resolve()
    output.mkdir(parents=True, exist_ok=True)
    syms = smoke.symbols(elf, args.llvm_nm)
    required = ("frame_complete", "MUSIC_CUE", "MUSIC_PAGE", "MUSIC_UNDERRUNS", "AUDIO_TICKS", "INPUT_OVERFLOWS", "FRAME_DROPPED")
    if any(name not in syms for name in required):
        raise RuntimeError("Music previews require an instrumented native package")
    failures = {address: name for name, address in syms.items()
                if ("havoc_c64" in name and "4fail" in name) or name == "rust_begin_unwind"}
    dwarf = args.llvm_nm.with_name("llvm-dwarfdump")
    layout = smoke.dwarf_game_layout(elf, dwarf, syms, output)
    fields = layout["fields"]
    for name, kind, size in (("stage_id", "u8", 1), ("invulnerability", "u8", 1)):
        if fields.get(name, {}).get("type") != kind:
            raise RuntimeError(f"Unsupported Game::{name} DWARF field")
        fields[name]["size"] = size
    description = subprocess.check_output([str(dwarf), "--name=AudioEngine", "--show-children", str(elf)], text=True)
    audio_fields = smoke.dwarf_members(description, "AudioEngine")
    for name in ("row", "row_ticks"):
        if audio_fields.get(name, {}).get("type") != "u8":
            raise RuntimeError(f"Unsupported AudioEngine::{name} DWARF field")
    audio_address = smoke.resident_symbol(syms, "AUDIO")[1]
    with socket.socket() as available:
        available.bind(("127.0.0.1", 0))
        port = available.getsockname()[1]
    startup = output / "startup.mon"
    startup.write_text("jpdb 0 $1f\njpdb 1 $1f\n")
    command = [str(args.vice.resolve()), "-directory", str(args.roms.resolve()), "+logtofile", "+logcolorize",
               "-" + args.video, "-warp", "-sound", "-sounddev", "alsa", "-soundarg", "null",
               "-soundoutput", "2", "-soundrate", "44100", "-soundwarpmode", "1",
               "-jamaction", "5", "-autostartprgmode", "1", "-reu", "-reusize", "16384",
               "-reuimage", str(reu), "+reuimagerw", "-sidextra", str(sid_count - 1), "-sid2address", "0xd420",
               "-joydev1", "0", "-joydev2", "0", "-controlport1device", "iosim", "-controlport2device", "iosim",
               "-remotemonitor", "-remotemonitoraddress", f"127.0.0.1:{port}",
               "-initbreak", "0x080d", "-moncommands", str(startup), "-autostart", str(prg)]
    if sid_count == 4:
        command += ["-sid3address", "0xd440", "-sid4address", "0xd460"]
    for folder in ("config", "cache"):
        (output / folder).mkdir(exist_ok=True)
    env = dict(os.environ, XDG_CONFIG_HOME=str(output / "config"), XDG_CACHE_HOME=str(output / "cache"))
    report = {"passed": False, "video": args.video, "sid_count": sid_count, "command": command,
              "package": str(package), "reu_sha256": hashlib.sha256(data).hexdigest(),
              "scope": "Native IRQ score playback and refill in a monitor-held respawn scene; VICE stereo synthesis, not physical hardware audio or timing evidence.",
              "cues": []}
    process = mon = None
    try:
        with (output / "vice.log").open("w") as log:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            deadline = time.monotonic() + 45
            while mon is None:
                if process.poll() is not None:
                    raise RuntimeError(f"VICE exited: {process.returncode}")
                try:
                    mon = smoke.Monitor(port, 45)
                except ConnectionRefusedError:
                    if time.monotonic() > deadline:
                        raise RuntimeError("Monitor startup timeout")
                    time.sleep(0.1)
            mon.command("delete")
            mon.command("sidefx off")
            for address in failures:
                mon.command(f"break exec {address:04x}")
            mon.run_until(syms["frame_complete"], failures)

            def patch(values):
                values = smoke.stage_fixture(values, layout, data)
                mon.command("bank ram")
                for name, value in values.items():
                    field = fields[name]
                    address = layout["address"] + field["offset"]
                    raw = value.to_bytes(field["size"], "little", signed=field["type"].startswith("i"))
                    mon.command(f"> {address:04x} " + " ".join(f"{byte:02x}" for byte in raw))
                mon.command("bank cpu")

            def observe():
                mon.run_until(syms["frame_complete"], failures)
                state = {name: mon.integer(syms[name], 1 if name in ("MUSIC_CUE", "MUSIC_PAGE") else 2)
                         for name in required if name != "frame_complete"}
                if any(state[name] for name in ("MUSIC_UNDERRUNS", "INPUT_OVERFLOWS", "FRAME_DROPPED")):
                    raise RuntimeError(f"Native preview lost audio, input or frames: {state}")
                return state

            for cue in args.cues:
                name, stage, boss = CUES[cue]
                fixture = {"mode": layout["modes"]["Respawning"], "stage_id": stage,
                           "rail_ticks": stage * 3750 + 1100, "respawn_ticks": 255,
                           "shield": 100, "lives": 3, "invulnerability": 255,
                           "boss_started": int(boss), "boss_defeated": 0, "menu_ticks": 0}
                for _ in range(12):
                    patch(fixture)
                    first = observe()
                    if first["MUSIC_CUE"] == cue and first["MUSIC_PAGE"] == 0:
                        break
                else:
                    raise RuntimeError(f"{name} did not start its opening page")
                entry = 16 + cue * 8
                pages, row_ticks = header[entry + 4:entry + 6]
                loop_ticks = pages * 16 * row_ticks
                path = output / f"{name}-{sid_count}sid.wav"
                mon.command("warp off")
                mon.command(f'resourceset "SoundRecordDeviceArg" "{path.as_posix()}"')
                mon.command('resourceset "SoundRecordDeviceName" "wav"')
                mon.command('resourceset "Sound" "1"')
                # Rewind only the already validated opening page's transport;
                # the same queued next page and normal player remain in use.
                mon.command("bank ram")
                for field in ("row", "row_ticks"):
                    mon.command(f'> {audio_address + audio_fields[field]["offset"]:04x} 00')
                mon.command("bank cpu")
                start = mon.integer(syms["AUDIO_TICKS"], 2)
                seen = {0}
                for _ in range(3000):
                    patch(fixture)
                    frame = observe()
                    if frame["MUSIC_CUE"] != cue:
                        raise RuntimeError(f"{name} changed cue during recording")
                    seen.add(frame["MUSIC_PAGE"])
                    elapsed = (frame["AUDIO_TICKS"] - start) & 0xffff
                    if elapsed >= loop_ticks + 16 * row_ticks:
                        break
                else:
                    raise RuntimeError(f"{name} did not complete its loop")
                mon.command('resourceset "Sound" "0"')
                patch(fixture)
                observe()  # Flush and close the WAV header.
                mon.command("warp on")
                if seen != set(range(pages)):
                    raise RuntimeError(f"{name} skipped score pages: {sorted(seen)}")
                result = {"cue": cue, "name": name, "path": str(path), "pages_seen": sorted(seen),
                          "fixture": fixture, **wav_metrics(path, loop_ticks / 50)}
                report["cues"].append(result)
                print(f'RECORDED {name}: {result["duration_seconds"]:.2f}s, {sid_count} SIDs, {pages} pages', flush=True)
            report["passed"] = True
    finally:
        if mon is not None:
            mon.socket.close()
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=5)
        (output / "recordings.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
