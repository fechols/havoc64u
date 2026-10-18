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
    if len(original["reu"]) != 16*1024*1024:
        parser.error("input REU image must contain exactly 16 MiB")
    before = {extension: hashlib.sha256(data).hexdigest() for extension, data in original.items()}
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
        ("boot-code", 0x10010, "REU CODE CHECKSUM FAILED"),
        ("audio-code", 0x50010, "REU CODE CHECKSUM FAILED"),
        ("scenery-code", 0x70010, "REU CODE CHECKSUM FAILED"),
        # These tails are omitted from some frame transfers, but boot must
        # validate their complete executable bytes before any frame is drawn.
        ("wildlife-tail", 0x70000 + int.from_bytes(original["reu"][0x122:0x124], "little") - 1,
         "REU CODE CHECKSUM FAILED"),
        ("enemy-drawing-tail", 0x30000 + int.from_bytes(original["reu"][0x108:0x10a], "little") - 1,
         "REU CODE CHECKSUM FAILED"),
        ("old-code-directory", 0x103, "REU CODE DIRECTORY MISSING OR INVALID"),
        ("common-assets", 0x1010, "INVALID COMMON ASSET BANK"),
        ("resident-pair", 0x118, "PRG AND REU DO NOT MATCH"),
        ("directory-profile", 0x120, "PRG AND REU DO NOT MATCH"),
        ("music-page", 0x60080+12+5*(10 if original["reu"][17] == 4 else 4), "INVALID MUSIC PAGE"),
    ]
    for name, offset, expected in cases:
        case_output = output/name
        private_package = case_output/"package"
        private_package.mkdir(parents=True, exist_ok=True)
        for extension in ("prg", "elf", "cfg"):
            (private_package/f"havoc.{extension}").write_bytes(original[extension])
        damaged = bytearray(original["reu"])
        damaged[offset] ^= 1
        if name == "old-code-directory":
            damaged[0x100:0x104] = b"OVL2"
        if name == "directory-profile":
            damaged[offset] = 2 if original["reu"][17] == 4 else 4
        if name in ("resident-pair", "directory-profile", "old-code-directory"):
            # A valid directory with the wrong resident checksum must get the
            # distinct pairing error, rather than failing directory validation.
            damaged[0x13e:0x140] = checksum(damaged[0x100:0x13e]).to_bytes(2, "little")
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
