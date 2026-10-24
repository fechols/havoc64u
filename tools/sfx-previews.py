"""Record all quad-SID effects and directional variants through native VICE playback.
Emulator synthesis only; DAC music and physical hardware mixing are outside scope.
"""
import argparse
import array
import hashlib
import importlib.util
import itertools
import json
import math
import os
from pathlib import Path
import re
import runpy
import signal
import socket
import subprocess
import time
import wave


ROOT = Path(__file__).resolve().parents[1]
CUES = {0: 'fire', 1: 'enemy-fire', 2: 'missile-launch', 3: 'enemy-hit', 4: 'pickup', 5: 'roll', 6: 'bomb', 7: 'enemy-explosion', 8: 'footstep', 9: 'bird-call', 10: 'transport', 11: 'transport-away', 12: 'meteor', 13: 'boss-hit', 14: 'player-hit', 15: 'boss-explosion', 16: 'player-death', 17: 'missile-impact', 18: 'boss-arrival', 19: 'missile-ready', 20: 'low-shield'}
LEAD_TICKS = 20  # 400 ms after recording begins; discard the recorder transient.
KEEP_LEAD_SECONDS = 0.1


def read_effects_layout(elf, dwarf):
    """Read the exact target ABI; no duplicated Rust offsets or instance stride."""
    spec = importlib.util.spec_from_file_location("sfx_layout_smoke", ROOT / "tools/vice-smoke.py")
    smoke = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(smoke)
    result = {}
    for label in ("EffectsEngine", "Instance", "AudioEvent"):
        description = subprocess.check_output(
            [str(dwarf), f"--name={label}", "--show-children", str(elf)], text=True)
        sizes = {int(value, 0) for value in re.findall(
            rf'DW_TAG_structure_type\s+DW_AT_name\s+\("{label}"\)\s+DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)',
            description)}
        if len(sizes) != 1:
            raise RuntimeError(f"Missing or conflicting DWARF size for {label}: {sizes}")
        result[label] = dict(size=sizes.pop(), fields=smoke.dwarf_members(description, label))
    engine, instance, event = (result[name] for name in ("EffectsEngine", "Instance", "AudioEvent"))
    fields = engine["fields"]
    if not (0 < engine["size"] <= 224
            and fields["instances"]["type"].endswith("::Instance[4]")
            and fields["events"]["type"].endswith("::AudioEvent[8]")
            and fields["event_count"]["type"] == "u8"
            and fields["instances"]["offset"] + instance["size"] * 4 <= fields["registers"]["offset"]
            and fields["events"]["offset"] + event["size"] * 8 <= fields["event_count"]["offset"] < engine["size"]
            and instance["fields"]["remaining"]["type"] == "u8"
            and event["fields"]["pan"]["type"] == "i8"
            and event["fields"]["effect"]["type"].endswith("::Effect")
            and event["size"] == 2):
        raise RuntimeError("Unsupported effects DWARF layout")
    return result


def window_metrics(values, channels, rate, start, end):
    first, last = round(start * rate) * channels, round(end * rate) * channels
    if last <= first or first < 0 or last > len(values):
        raise RuntimeError(f"Invalid audio analysis window {start:.3f}..{end:.3f}s")
    result = []
    for channel in range(channels):
        lane = values[first + channel:last:channels]
        mean = sum(lane) / len(lane)
        result.append(dict(peak=max(abs(value) for value in lane),
                           rms=math.sqrt(sum(value * value for value in lane) / len(lane)),
                           ac_rms=math.sqrt(sum((value - mean) ** 2 for value in lane) / len(lane)),
                           clipping_samples=sum(abs(value) >= 32767 for value in lane)))
    return dict(start_seconds=start, end_seconds=end, channels=result)


def wav_metrics(path, seconds, schedule, *, lead_seconds=LEAD_TICKS / 50):
    with wave.open(str(path), "rb") as stream:
        channels, rate, width = stream.getnchannels(), stream.getframerate(), stream.getsampwidth()
        frames = stream.getnframes()
        samples = stream.readframes(frames)
    if (channels, rate, width) != (2, 44100, 2):
        raise RuntimeError(f"Unexpected WAV format: {(channels, rate, width)}")
    expected = round((lead_seconds + seconds) * rate)
    if frames < expected:
        raise RuntimeError(f"Incomplete recording: {frames / rate:.3f}s, need {expected / rate:.3f}s")
    values = array.array("h", samples[:expected * channels * width])
    if os.sys.byteorder != "little":
        values.byteswap()
    # Analysis precedes cropping. Neither the startup click nor DC offset can
    # make a silent effect pass. A 20 ms gate-low tick precedes the SID attack.
    quiet = window_metrics(values, channels, rate, lead_seconds - 0.15, lead_seconds - 0.05)
    attack = window_metrics(values, channels, rate, lead_seconds + 0.02, lead_seconds + 0.16)
    body = window_metrics(values, channels, rate, lead_seconds + 0.16, lead_seconds + 0.4)
    floor = max(channel["ac_rms"] for channel in quiet["channels"])
    attack_level = max(channel["ac_rms"] for channel in attack["channels"])
    # Short effects need not sustain through the body window; require a real
    # attack at least 12 dB above the quiet floor (and >8 PCM16 counts RMS).
    if attack_level < max(8.0, floor * 4):
        raise RuntimeError(f"No distinct effect attack after recorder startup: {attack_level:.2f} RMS, floor {floor:.2f}")
    windows = {"quiet": quiet, "attack": attack, "body": body}
    events = []
    for tick, effect, pan in schedule:
        onset = lead_seconds + tick / 50
        event_attack = window_metrics(values, channels, rate, onset + 0.02, onset + 0.16)
        events.append(dict(tick=tick, effect=CUES[effect], pan=pan, attack=event_attack))
    begin = round((lead_seconds - KEEP_LEAD_SECONDS) * rate)
    samples = samples[begin * channels * width:expected * channels * width]
    with wave.open(str(path), "wb") as stream:
        stream.setnchannels(channels)
        stream.setsampwidth(width)
        stream.setframerate(rate)
        stream.writeframes(samples)
    values = array.array("h", samples)
    if os.sys.byteorder != "little":
        values.byteswap()
    retained_frames = len(values) // channels
    metrics = window_metrics(values, channels, rate, 0, retained_frames / rate)["channels"]
    if any(channel["clipping_samples"] for channel in metrics):
        raise RuntimeError("Clipped effect recording")
    return {"duration_seconds": retained_frames / rate, "sample_rate": rate,
            "samples": retained_frames, "channels": metrics,
            "trigger_seconds": KEEP_LEAD_SECONDS, "raw_lead_seconds": lead_seconds,
            "analysis_windows_raw_time": windows, "event_attacks_raw_time": events,
            "attack_over_floor_db": 20 * math.log10(attack_level / max(floor, 1)),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def music_excerpt(path, offset_seconds, seconds=8, rate=44100):
    """Decode the shipped DLT PCM, retaining the game's 10 ms mono delay."""
    blob = path.read_bytes()
    if blob[:4] != b"DLT4" or blob[21] not in (1, 2):
        raise RuntimeError("Offline music audition requires a DLT4 mono/stereo asset")
    source_rate, channels = int.from_bytes(blob[4:8], "little"), blob[21]
    start = round(offset_seconds * source_rate)
    delay = 160 if channels == 1 else 0
    count = math.ceil(seconds * source_rate) + 2
    decoder = runpy.run_path(str(ROOT / "tools/test-digital-kernel.py"))["pcm_reference"]
    decoded = ([(0,) * channels] * max(0, delay - start)
               + list(itertools.islice(decoder(blob), max(0, start - delay), start + count)))
    values = array.array("h")
    for frame in range(round(seconds * rate)):
        position = frame * source_rate / rate
        index, fraction = int(position), position % 1
        for channel in range(2):
            at = index + (delay if channels == 1 and channel == 0 else 0)
            lane = channel if channels == 2 else 0
            first, second = decoded[at][lane], decoded[at + 1][lane]
            values.append(round((first + (second - first) * fraction) * 256))
    return values, dict(path=str(path), sha256=hashlib.sha256(blob).hexdigest(),
                        source_rate=source_rate, channels=channels, offset_seconds=offset_seconds,
                        mono_right_delay_samples=delay, output_rate=rate)


def mix_audition(path, music, music_db, sfx_db):
    """An explicit listening aid, not a calibrated model of the Ultimate mixer."""
    with wave.open(str(path), "rb") as stream:
        if (stream.getnchannels(), stream.getsampwidth(), stream.getframerate()) != (2, 2, 44100):
            raise RuntimeError("Unsupported offline mix input")
        samples = array.array("h", stream.readframes(stream.getnframes()))
    if os.sys.byteorder != "little":
        samples.byteswap()
    if len(samples) > len(music):
        raise RuntimeError("Offline music excerpt is shorter than the audition")
    music_gain, sfx_gain = 10 ** (music_db / 20), 10 ** (sfx_db / 20)
    mixed = [sample * sfx_gain + music[index] * music_gain for index, sample in enumerate(samples)]
    peak = max(abs(value) for value in mixed)
    attenuation = min(1.0, (32767 * 10 ** (-1 / 20)) / max(1, peak))
    values = array.array("h", (round(value * attenuation) for value in mixed))
    if os.sys.byteorder != "little":
        values.byteswap()
    output = path.with_name(path.stem + "-with-music.wav")
    with wave.open(str(output), "wb") as stream:
        stream.setnchannels(2)
        stream.setsampwidth(2)
        stream.setframerate(44100)
        stream.writeframes(values.tobytes())
    return dict(path=str(output), music_gain_db=music_db, sfx_gain_db=sfx_db,
                common_peak_attenuation_db=20 * math.log10(attenuation),
                scope="Offline DLT PCM plus VICE SID synthesis at explicit listening gains; not calibrated hardware balance.",
                sha256=hashlib.sha256(output.read_bytes()).hexdigest())


def main():
    sibling = ROOT.parent / "doomc64u"
    spec = importlib.util.spec_from_file_location("havoc_smoke", ROOT / "tools/vice-smoke.py")
    smoke = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(smoke)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--video", choices=("pal", "ntsc"), default="pal")
    parser.add_argument("--cues", type=int, nargs="+", choices=tuple(CUES), default=list(CUES))
    parser.add_argument("--stress-only", action="store_true", help="record only repeated-fire and crowded hit scenarios")
    parser.add_argument("--music-dlt", type=Path, help="also create uncalibrated offline mixes with this shipped music asset")
    parser.add_argument("--music-offset", type=float, default=30, help="music excerpt start in seconds (default30)")
    parser.add_argument("--mix-music-db", type=float, default=-12, help="offline music listening gain, not a hardware mixer value")
    parser.add_argument("--mix-sfx-db", type=float, default=0, help="offline SID listening gain, not a hardware mixer value")
    parser.add_argument("--vice", type=Path, default=Path(os.environ.get("VICE_BIN", sibling / "target/audit-fast/x64sc-64x")))
    parser.add_argument("--roms", type=Path, default=sibling / "target/tools/secondreality/vice/data")
    parser.add_argument("--llvm-nm", type=Path, default=Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin/llvm-nm")
    smoke.add_emulator_audio_argument(parser)
    args = parser.parse_args()
    if not math.isfinite(args.music_offset) or args.music_offset < 0 or not all(
            math.isfinite(value) and -60 <= value <= 12 for value in (args.mix_music_db, args.mix_sfx_db)):
        parser.error("music offset must be nonnegative and listening gains must be -60..12 dB")
    package = args.package_dir.resolve()
    elf, reu, prg = (package / f"havoc.{suffix}" for suffix in ("elf", "reu", "prg"))
    data = reu.read_bytes()
    package_format = smoke.package_model_layout(data, validate_size=True)
    sid_count = 4
    output = (args.out_dir or ROOT / "target/sfx-previews" / f"{sid_count}sid").resolve()
    output.mkdir(parents=True, exist_ok=True)
    syms = smoke.symbols(elf, args.llvm_nm)
    required = ("frame_complete", "MUSIC_CUE", "MUSIC_UNDERRUNS", "AUDIO_TICKS", "INPUT_OVERFLOWS", "FRAME_DROPPED")
    if any(name not in syms for name in required):
        raise RuntimeError("Music previews require an instrumented native package")
    failures = {address: name for name, address in syms.items()
                if ("havoc_c64" in name and "4fail" in name) or name == "rust_begin_unwind"}
    audio_address = smoke.resident_symbol(syms, "EFFECTS")[1]
    layout = read_effects_layout(elf, args.llvm_nm.with_name("llvm-dwarfdump"))
    fields = layout["EffectsEngine"]["fields"]
    event_fields = layout["AudioEvent"]["fields"]
    event_size = layout["AudioEvent"]["size"]
    with socket.socket() as available:
        available.bind(("127.0.0.1", 0))
        port = available.getsockname()[1]
    startup = output / "startup.mon"
    startup.write_text("jpdb 0 $1f\njpdb 1 $1f\n")
    command = [str(args.vice.resolve()), "-directory", str(args.roms.resolve()), "+logtofile", "+logcolorize",
               "-" + args.video, "-warp", "-sound", "-sounddev", "alsa", "-soundarg", "null",
               "-soundoutput", "2", "-soundrate", "44100", "-soundwarpmode", "1",
               "-sidmodel", "1", "-jamaction", "5", "-autostartprgmode", "1", "-reu", "-reusize", str(package_format["reu_size"] // 1024),
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
              "scope": "Native IRQ effects in a paused title fixture; VICE stereo synthesis, not physical hardware audio, DAC mix or timing evidence.",
              "effects_layout": layout,
              "cues": []}
    music = None
    if args.music_dlt:
        music, report["offline_music"] = music_excerpt(args.music_dlt.resolve(), args.music_offset)
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
            report["emulator_audio"] = smoke.configure_emulator_audio(
                mon, syms, failures, package_format, args.audio_backend)
            mon.run_until(syms["frame_complete"], failures)
            # Freeze foreground gameplay so attract/scene transitions cannot
            # cancel a scenic audition. The 50 Hz SID IRQ continues normally.
            mon.command("bank ram")
            mon.command(f'> {smoke.resident_symbol(syms, "PAUSED")[1]:04x} 01')
            mon.command("bank cpu")

            tick = smoke.resident_symbol(syms, "audio_tick")[1]
            auditions = [] if args.stress_only else [(CUES[cue] + "-" + direction, 150, [(0, cue, pan)])
                         for cue in args.cues
                         for pan, direction in ((-100, "left"), (0, "center"), (100, "right"))]
            auditions += [("laser-ten-per-second", 200, [(at, 0, 0) for at in range(0, 150, 5)]),
                          ("laser-with-hits", 250,
                           [(at, 0, 0) for at in range(0, 150, 5)]
                           + [(20, 3, -100), (40, 13, 100), (60, 14, 0), (100, 20, 0)]),
                          ("bomb-with-enemy-hit", 150, [(0, 6, 0), (20, 3, 0)]),
                          ("bomb-with-player-hit", 150, [(0, 6, 0), (20, 14, 0)]),
                          ("bomb-with-hit-same-tick", 150, [(0, 6, 0), (0, 14, 0)]),
                          ("bomb-with-left-right-explosions", 150,
                           [(0, 6, 0), (0, 7, -100), (0, 7, 100), (20, 7, -100), (30, 7, 100)]),
                          ("repeated-bomb-with-explosions", 180,
                           [(0, 6, 0), (20, 7, -100), (20, 7, 100), (25, 6, 0)]),
                          ("boss-explosion-with-low-shield", 150, [(0, 15, 0), (20, 20, 0)]),
                          ("crowded-combat", 350,
                [(at, 0 if at % 10 else 1, -100 if at % 10 else 100) for at in range(0, 200, 5)]
                + [(20, 2, 0), (40, 7, -100), (60, 17, 100), (80, 20, 0),
                   (100, 6, 0), (110, 3, -100), (120, 14, 0),
                   (180, 15, 0), (190, 13, 100), (220, 16, 0)])]
            for name, ticks, schedule in auditions:
                path = output / f"{name}.wav"
                mon.command("warp off")
                mon.command(f'resourceset "SoundRecordDeviceArg" "{path.as_posix()}"')
                mon.command('resourceset "SoundRecordDeviceName" "wav"')
                mon.command('resourceset "Sound" "1"')
                pending = sorted((at + LEAD_TICKS, effect, pan) for at, effect, pan in schedule)
                start = mon.integer(syms["AUDIO_TICKS"], 2)
                for _ in range(ticks + LEAD_TICKS + 50):
                    elapsed = (mon.integer(syms["AUDIO_TICKS"], 2) - start) & 0xffff
                    # Publish one batch at a time using the engine's eight-event ABI.
                    batch = []
                    while pending and pending[0][0] <= elapsed:
                        _, effect, pan = pending.pop(0)
                        raw = bytearray(event_size)
                        raw[event_fields["effect"]["offset"]] = effect
                        raw[event_fields["pan"]["offset"]] = pan & 255
                        batch.extend(raw)
                    if batch:
                        if len(batch) > event_size * 8:
                            raise RuntimeError("Audition event queue overflow")
                        mon.command("bank ram")
                        mon.command(f'> {audio_address + fields["events"]["offset"]:04x} ' + " ".join(f"{b:02x}" for b in batch))
                        mon.command(f'> {audio_address + fields["event_count"]["offset"]:04x} {len(batch) // event_size:02x}')
                        mon.command("bank cpu")
                    if elapsed >= ticks + LEAD_TICKS:
                        break
                    mon.run_until(tick, failures)
                else:
                    raise RuntimeError("Audio clock did not advance")
                mon.command('resourceset "Sound" "0"')
                mon.run_until(tick, failures)
                mon.command("warp on")
                result = {"name": name, "schedule": schedule,
                          "path": str(path), **wav_metrics(path, ticks / 50 - 0.2, schedule)}
                if music is not None:
                    result["offline_music_mix"] = mix_audition(path, music, args.mix_music_db, args.mix_sfx_db)
                report["cues"].append(result)
                print(f"RECORDED {name}", flush=True)
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

