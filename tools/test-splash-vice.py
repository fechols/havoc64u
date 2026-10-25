#!/usr/bin/env python3
"""Check the boot splash and its first-frame overwrite in calibrated VICE.

Use the vice-smoke.py arguments, for example --video pal or --video ntsc.
This wrapper uses its explicit effects-only emulator override, verifies the
startup display, then lets the ordinary gameplay smoke checks finish. Native
digital initialization/priming is covered separately by the digital tests.
All captures, traces and the splash report remain under --out-dir.
Review the decoded *-preview.png captures: this companion's known $D031
matrix-fetch defect affects its direct *-vice.png screenshots (hardware.md).
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys


SPEC = importlib.util.spec_from_file_location("vice_smoke", Path(__file__).with_name("vice-smoke.py"))
VICE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VICE)
ORIGINAL_AUDIO = VICE.configure_emulator_audio
ORIGINAL_INPUTS = VICE.configure_emulator_inputs
CONTEXT = {}


def function(syms, name):
    if name in syms:
        return syms[name]
    token = f"{len(name)}{name}"
    matches = [address for symbol, address in syms.items() if token in symbol]
    if len(matches) != 1:
        raise RuntimeError(f"Missing or ambiguous splash checkpoint {name}: {matches}")
    return matches[0]


def configure_audio(mon, syms, failures, package_format, backend="sfx"):
    if backend != "sfx":
        raise RuntimeError("Splash VICE checks require --audio-backend sfx; native audio has separate tests")
    CONTEXT.update(syms=syms, failures=failures)
    return ORIGINAL_AUDIO(mon, syms, failures, package_format, backend)


def splash_checks(mon, syms, failures, output):
    expected = (VICE.ROOT / "assets/splash/havoc_splash.bin").read_bytes()
    if len(expected) != 10001 or expected[-1] != 6:
        raise RuntimeError("Splash fixture must contain 10001 bytes with background 6")
    result = dict(passed=False,
        scope="PAL/NTSC calibrated VICE with effects-only audio; display and production boot checks, not Ultimate Audio playback or physical C64U timing.",
        capture_note="Decoded previews use the verified display bytes. Direct VICE screenshots retain the known $D031 matrix-fetch defect documented in docs/hardware.md.",
        expected_sha256=hashlib.sha256(expected[:10000]).hexdigest(), checkpoints=[])
    report = output / "splash-report.json"
    trace = output / "splash-monitor.log"
    trace.unlink(missing_ok=True)
    mon.command("log off")
    mon.command(f'logname "{trace.as_posix()}"')
    mon.command("log on")
    for address in (0xd011, 0xd021, 0xd015):
        mon.command(f"trace store {address:04x} if @cpu:$01 == $35")

    def snapshot(name, capture=False):
        parts = []
        for suffix, address, length, bank in (
                ("bitmap", 0xa000, 8000, "ram"),
                ("screen", 0x8000, 1000, "ram"),
                ("color", 0xd800, 1000, "io")):
            mon.command(f"bank {bank}")
            path = output / f"splash-{name}-{suffix}.bin"
            mon.command(f'bsave "{path.as_posix()}" 0 {address:04x} {address+length-1:04x}')
            raw = path.read_bytes()
            if len(raw) != length:
                raise RuntimeError(f"Short splash snapshot {name}/{suffix}")
            parts.append(bytes(value & 15 for value in raw) if suffix == "color" else raw)
        mon.command("bank io")
        registers = {f"{address:04x}": mon.integer(address, 1)
                     for address in (0xd011, 0xd016, 0xd018, 0xd015, 0xd021, 0xdd00)}
        mon.command("bank cpu")
        if ((registers["d011"] & 0x7f) != 0x3b
                or registers["d016"] & 0x3f != 0x18 or registers["d018"] & 0xfe != 0x08
                or registers["d021"] & 15 != 6 or registers["dd00"] & 3 != 1):
            raise RuntimeError(f"Splash display changed mode/background at {name}: {registers}")
        native = b"".join(parts)
        item = dict(name=name, registers=registers, **mon.registers(),
                    sha256=hashlib.sha256(native).hexdigest(),
                    bytes_different=sum(a != b for a, b in zip(native, expected)))
        if capture:
            item["capture"] = VICE.capture(mon, syms, output, f"splash-{name}")
        result["checkpoints"].append(item)
        report.write_text(json.dumps(result, indent=2) + "\n")
        return native

    def unchanged(name, address=None, capture=False):
        if address is not None:
            mon.run_until(address, failures)
        if snapshot(name, capture) != expected[:10000]:
            raise RuntimeError(f"Splash display memory changed before first publication: {name}")
        if result["checkpoints"][-1]["registers"]["d015"] != 0:
            raise RuntimeError(f"HUD sprites appeared over the boot splash: {name}")

    def publisher():
        # Several overlays share the address. Stop only in the loaded PIXELS
        # phase, as the ordinary native reticle integration checks also do.
        for _ in range(200):
            mon.run_until(syms["publish_staged_band"], failures)
            if mon.integer(syms["PROF_PHASE"], 1) == 7:
                registers = mon.command("r")
                rows = re.findall(r"\.;[0-9a-fA-F]{4}\s+([0-9a-fA-F]{2})\b", registers)
                if not rows:
                    raise RuntimeError("Cannot read publisher's band argument")
                return int(rows[-1], 16)
        raise RuntimeError("Splash test did not reach PIXELS publisher")

    try:
        unchanged("main", capture=True)
        unchanged("code-checks", function(syms, "check_code_package"))
        unchanged("boot-init", function(syms, "boot_init"))
        unchanged("boot-complete", function(syms, "load_sim"))
        unchanged("geometry-complete", syms["geometry_complete"])
        if publisher() != 0:
            raise RuntimeError("First splash overwrite did not start at band zero")
        unchanged("before-first-band", capture=True)
        for band in range(1, 13):
            if publisher() != band:
                raise RuntimeError(f"Splash handoff skipped or reordered band {band}")
        partial = snapshot("before-band-12", capture=True)
        changed = 0
        for start, width, count in ((0, 320, 25), (8000, 40, 25), (9000, 40, 25)):
            boundary, end = start + 12 * width, start + count * width
            if partial[boundary:end] != expected[boundary:end]:
                raise RuntimeError("First-frame handoff modified an unpublished splash band")
            changed += sum(a != b for a, b in zip(partial[start:boundary], expected[start:boundary]))
        if changed == 0:
            raise RuntimeError("First-frame bands did not overwrite the splash")
        if result["checkpoints"][-1]["registers"]["d015"] != 0:
            raise RuntimeError("HUD sprites appeared before first-frame publication completed")
        mon.run_until(syms["frame_complete"], failures)
        completed = snapshot("first-frame", capture=True)
        if completed == expected[:10000] or mon.integer(syms["FRAME_SERIAL"], 2) != 0:
            raise RuntimeError("Splash was not replaced by the first completed frame")
        mon.command("log off")
        events = VICE.stores(trace)
        counts = {f"{address:04x}": sum(event["address"] == address for event in events)
                  for address in (0xd011, 0xd021, 0xd015)}
        if not all(counts.values()):
            raise RuntimeError(f"Missing splash display trace events: {counts}")
        for event in events:
            address, value = event["address"], event["value"]
            if address == 0xd011 and value != 0x3b:
                raise RuntimeError(f"Display blanked or changed mode during splash handoff: {event}")
            if address == 0xd021 and value != 6:
                raise RuntimeError(f"Splash background changed before handoff completed: {event}")
            if address == 0xd015 and value != 0:
                raise RuntimeError(f"HUD sprites enabled before first completed frame: {event}")
        result.update(passed=True, partial_prefix_bytes_changed=changed,
                      untouched_suffix_bands=13, trace=str(trace), register_store_counts=counts)
        print("SPLASH: intact through boot/geometry; 12 bands overwrite while 13 remain intact; first frame visible with no blank/background change", flush=True)
        return result
    except Exception as exc:
        result["error"] = str(exc)
        raise
    finally:
        report.write_text(json.dumps(result, indent=2) + "\n")
        # Restore the smoke driver's normal failure checkpoints and trace.
        mon.command("delete")
        for address in failures:
            mon.command(f"break exec {address:04x}")
        mon.command("log off")
        mon.command(f'logname "{(output / "monitor.log").as_posix()}"')
        mon.command("log on")


def configure_inputs(mon, output):
    result = ORIGINAL_INPUTS(mon, output)
    result["splash"] = splash_checks(mon, CONTEXT["syms"], CONTEXT["failures"], output)
    return result


if __name__ == "__main__":
    VICE.configure_emulator_audio = configure_audio
    VICE.configure_emulator_inputs = configure_inputs
    sys.exit(VICE.main())
