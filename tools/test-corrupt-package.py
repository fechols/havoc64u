#!/usr/bin/env python3
"""Check boot rejection of private corrupted REU copies in calibrated VICE.

Run in Linux/WSL: python3 tools/test-corrupt-package.py
Uses the existing package and dynamic ELF symbols; it performs no builds and
never changes the input package. Defaults match vice-smoke.py's sibling VICE
installation. Fixtures, error screenshots, logs, and a summary stay in target.
These checks establish emulator behavior, not physical C64 Ultimate acceptance.
"""
import argparse
import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]


def checksum(data):
    a = b = 0
    for value in data:
        a = (a + value) % 255
        b = (b + a) % 255
    return a | b << 8


def main():
    sibling = ROOT.parent / "doomc64u"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", type=Path, default=ROOT/"dist")
    parser.add_argument("--out-dir", type=Path, default=ROOT/"target/corrupt-package-test")
    parser.add_argument("--vice", type=Path, default=Path(os.environ.get("VICE_BIN", sibling/"target/audit-fast/x64sc-64x")))
    parser.add_argument("--roms", type=Path, default=sibling/"target/tools/secondreality/vice/data")
    parser.add_argument("--llvm-nm", type=Path, default=Path(os.environ.get("LLVM_MOS", Path.home()/".local/llvm-mos"))/"bin/llvm-nm")
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--case", action="append", help="run only this named case; repeat to select several")
    args = parser.parse_args()
    package, output = args.package_dir.resolve(), args.out_dir.resolve()
    if output == package or package in output.parents:
        parser.error("private test output must be outside the input package directory")
    output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location("vice_smoke", ROOT/"tools/vice-smoke.py")
    vice = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(vice)
    original = {extension: (package/f"havoc.{extension}").read_bytes()
                for extension in ("prg", "elf", "reu", "cfg")}
    before = {extension: hashlib.sha256(data).hexdigest() for extension, data in original.items()}
    image = original["reu"]
    package_format = vice.package_model_layout(image, validate_size=True)
    space = package_format["space"]
    if (not package_format["campaign"]
            or image[0x100:0x104] != (b"OVL7" if space else b"OVL6")):
        parser.error("input must be a matched OVL6/OVL7 campaign package")
    cue_count = 21 if space else 13
    overlay_offsets = package_format["overlay_offsets"]
    summary = {"passed": False, "scope": "Private corrupt REU copies in calibrated VICE; not physical hardware acceptance",
               "package": str(package), "original_sha256": before, "cases": []}
    native_command = vice.Monitor.command
    case_output = None

    def command(self, command, required=None):
        if command == "step 8000":
            # Complete error text, then let the accelerated CPU run long enough
            # for several video refreshes before taking the diagnostic PNG.
            result = native_command(self, "step 1000000", required)
            native_command(self, f'screenshot "{(case_output/"error-screen.png").as_posix()}" 2')
            return result
        return native_command(self, command, required)

    vice.Monitor.command = command
    cases = [
        ("directory-magic", 0x100, "REU CODE DIRECTORY MISSING OR INVALID"),
        ("old-code-directory", 0x103, "REU CODE DIRECTORY MISSING OR INVALID"),
        ("common-assets", 0x1010, "INVALID COMMON ASSET BANK"),
        ("resident-pair", 0x118, "PRG AND REU DO NOT MATCH"),
        ("directory-profile", 0x120, "PRG AND REU DO NOT MATCH"),
        ("model-directory", 0x40, "INVALID MODEL DIRECTORY"),
        ("model-hp-sentinel", 0xb0000 + 2*2048 + 88, "INVALID LEVEL MODEL BANK"),
        ("sim-helpers-second-chunk", overlay_offsets[23] + 1024, "REU CODE CHECKSUM FAILED"),
        ("transport-directory-size", 0x14c, "INVALID REU CODE SIZE"),
        ("sim-helpers-directory-size", 0x154, "INVALID REU CODE SIZE"),
    ]
    overlays = (
        ("boot", 1, 4), ("lowlands", 2, 6), ("geometry", 3, 8),
        ("pixels", 4, 10), ("audio", 5, 12), ("wildlife", 7, 34),
        ("canyon", 8, 38), ("fortress", 9, 42), ("carrier", 10, 46),
        ("victory", 12, 50), ("terminal", 13, 54),
        ("warden", 18, 64), ("citadel", 19, 68), ("flagship", 20, 72),
        ("transport", 21, 76), ("meteors", 22, 80),
        ("sim-helpers", 23, 84),
    )
    if space:
        overlays += (("ascent", 24, 88), ("asteroid", 25, 92), ("moon", 26, 96),
                     ("graveyard", 27, 100), ("shipyard", 28, 104),
                     ("space-carrier", 29, 108), ("gate", 30, 112))
    if package_format['version'] >= 0x534d0c00:
        overlays += (("shadow", 31, 116),
                     ("scenery-helpers", 33, 58))
        cases += [("shared-projection-code", overlay_offsets[3] + 0x3f20, "REU CODE CHECKSUM FAILED"),
                  ("scenery-extra-code", overlay_offsets[33] + 0x500, "REU CODE CHECKSUM FAILED")]
    # The split PIXELS image authenticates its unused gap as well as its
    # separately loaded helper tail. Neither region enters the main PRG.
    if int.from_bytes(image[0x10a:0x10c], "little") > 0x3f20:
        cases += [
            ("pixels-raster-gap", 0x43000, "REU CODE CHECKSUM FAILED"),
            ("pixels-helper-code", 0x43f20, "REU CODE CHECKSUM FAILED"),
        ]
    for name, page, directory_offset in overlays:
        size_at = 0x100 + directory_offset
        size = int.from_bytes(image[size_at:size_at + 2], "little")
        if not 16 < size <= 0x4200:
            parser.error(f"invalid {name} overlay size")
        cases.append((f"{name}-code", overlay_offsets[page] + 16, "REU CODE CHECKSUM FAILED"))
        # Boot authenticates complete padded images, including inactive tails.
        cases.append((f"{name}-tail", overlay_offsets[page] + size - 1, "REU CODE CHECKSUM FAILED"))
    for level in range(package_format["count"]):
        cases.append((f"level-{level}-models", 0xb0000 + level*2048 + 200, "INVALID LEVEL MODEL BANK"))
        cases.append((f"level-{level}-encounters", 0x2000 + level*2048 + 20, "INVALID MISSION SEGMENT"))
    metadata = {
        "enemy-profile": (8, 4), "music-cue": (9, cue_count),
        "bridge-profile": (10, 5 if space else 3), "boss-kind": (11, 6 if space else 4),
        "theme": (12, 16 if space else 10), "next-theme": (13, 16 if space else 10),
        "previous-theme": (14, 16 if space else 10), "reserved": (15, 1),
    }
    for field, (offset, _value) in metadata.items():
        cases.append((f"metadata-{field}", 0x2800 + offset, "INVALID MISSION SEGMENT"))
    cases += [("sfx-bank-size", 0x178, "PRG AND REU DO NOT MATCH"),
              ("sfx-bank-checksum", 0x17a, "PRG AND REU DO NOT MATCH"),
              ("sfx-reserved", 0x17c, "PRG AND REU DO NOT MATCH"),
              ("sfx-prg-first", 2 + 0x7200 - 0x801, "PRG AND REU DO NOT MATCH"),
              ("sfx-prg-padding", 2 + 0x74ff - 0x801, "PRG AND REU DO NOT MATCH")]
    if args.case:
        unknown = set(args.case) - {name for name, _, _ in cases}
        if unknown:
            parser.error(f"unknown corruption case(s): {sorted(unknown)}")
        cases = [case for case in cases if case[0] in args.case]
    for name, offset, expected in cases:
        case_output = output/name
        private_package = case_output/"package"
        private_package.mkdir(parents=True, exist_ok=True)
        for extension in ("prg", "elf", "cfg"):
            (private_package/f"havoc.{extension}").write_bytes(original[extension])
        damaged = bytearray(original["reu"])
        if name.startswith("sfx-prg-"):
            damaged_prg = bytearray(original["prg"])
            damaged_prg[offset] ^= 1
            (private_package/"havoc.prg").write_bytes(damaged_prg)
        else:
            damaged[offset] ^= 1
        if name.endswith("-directory-size"):
            size = int.from_bytes(original["reu"][offset:offset+2], "little")
            damaged[offset:offset+2] = (size - 1).to_bytes(2, "little")
            # Authenticate the altered directory so startup must compare this
            # composed image length with its independent startup size table.
            damaged[0x17e:0x180] = checksum(damaged[0x100:0x17e]).to_bytes(2, "little")
        if name == "model-hp-sentinel":
            # HP 255 belongs to indestructible scenery, never an enemy. Repair
            # both checksums so only structural model validation can reject it.
            damaged[offset] = 255
            model_start = 0xb0000 + 2*2048
            damaged[0x50:0x52] = checksum(damaged[model_start:model_start+2048]).to_bytes(2, "little")
            damaged[0x7e:0x80] = checksum(damaged[0x40:0x7e]).to_bytes(2, "little")
        if name == "old-code-directory":
            damaged[0x100:0x104] = b"OVL6" if space else b"OVL5"
        if name == "directory-profile":
            damaged[offset] = 2 if original["reu"][17] == 4 else 4
        if name in ("resident-pair", "directory-profile", "old-code-directory"):
            # A valid directory with the wrong resident checksum must get the
            # distinct pairing error, rather than failing directory validation.
            damaged[0x17e:0x180] = checksum(damaged[0x100:0x17e]).to_bytes(2, "little")
        if name.startswith("metadata-"):
            damaged[offset] = metadata[name.removeprefix("metadata-")][1]
            # Valid hashes isolate the structural checks on each metadata byte.
            damaged[22:24] = checksum(damaged[0x2800:0x2a00]).to_bytes(2, "little")
            damaged[62:64] = checksum(damaged[:62]).to_bytes(2, "little")
        if name.startswith("sfx-"):
            damaged[0x17e:0x180] = checksum(damaged[0x100:0x17e]).to_bytes(2, "little")
        damaged_hash = hashlib.sha256(damaged).hexdigest()
        (private_package/"havoc.reu").write_bytes(damaged)
        sys.argv = ["vice-smoke.py", "--video", "pal", "--frames", "1", "--timeout", str(args.timeout),
                    "--package-dir", str(private_package), "--out-dir", str(case_output),
                    "--vice", str(args.vice), "--roms", str(args.roms), "--llvm-nm", str(args.llvm_nm)]
        # The ordinary success smoke deliberately returns failure here. Keep its
        # detailed diagnostics and report the expected-rejection verdict below.
        with (case_output/"runner.log").open("w") as log, contextlib.redirect_stdout(log), contextlib.redirect_stderr(log):
            result = vice.main()
        report = json.loads((case_output/"report.json").read_text())
        text = report.get("failure_screen_8051", "")
        failure = report.get("error", "")
        reached_fail = "Target entered" in failure and "4fail" in failure
        passed = result == 1 and reached_fail and text.startswith(expected) and not report["frames"]
        summary["cases"].append({"name": name, "changed_reu_offset": hex(offset), "expected": expected,
                                 "displayed": text, "expected_failure_observed": passed, "target_error": failure,
                                 "private_reu_sha256": damaged_hash, "report": str(case_output/"report.json")})
        print(f"CORRUPTION {'PASS' if passed else 'FAIL'} {name}: {text}", flush=True)
        if not passed:
            break
    after = {extension: hashlib.sha256((package/f"havoc.{extension}").read_bytes()).hexdigest()
             for extension in original}
    summary["input_package_unchanged"] = before == after
    summary["passed"] = len(summary["cases"]) == len(cases) and all(case["expected_failure_observed"] for case in summary["cases"]) and before == after
    (output/"summary.json").write_text(json.dumps(summary, indent=2)+"\n")
    print("PASS" if summary["passed"] else "FAIL", output/"summary.json", flush=True)
    return 0 if summary["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
