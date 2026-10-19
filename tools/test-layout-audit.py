"""Reject incorrect startup image sizes and runtime SIM transfer lengths."""
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile


def main():
    source = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/havoc.elf")
    prg = Path(sys.argv[2]) if len(sys.argv) > 2 else source.with_suffix(".prg")
    checker = Path(__file__).with_name("check-layout.py")
    elf = runpy.run_path(str(checker.with_name("check-overlays.py")))["Elf"](source)
    original = prg.read_bytes()
    tables = (("__sim_load_sizes", 8, "SIM load table disagrees", "SIM profile"),
              ("__code_sizes", 17, "Code size table disagrees", "OVL6 image"))

    def check(path):
        return subprocess.run([sys.executable, str(checker), str(source), str(path)],
                              capture_output=True, text=True)

    baseline = check(prg)
    assert baseline.returncode == 0, baseline.stdout + baseline.stderr
    cases = 0
    with tempfile.TemporaryDirectory(prefix="havoc-layout-audit-") as directory:
        damaged = Path(directory) / "damaged.prg"
        for table, count, expected, label in tables:
            symbol = next(symbol for symbol in elf.symbols if symbol.name == table)
            offset = symbol.address - int.from_bytes(original[:2], "little") + 2
            for profile in range(count):
                at = offset + profile * 2
                length = int.from_bytes(original[at:at+2], "little")
                for delta in (-1, 1):
                    data = bytearray(original)
                    data[at:at+2] = (length + delta).to_bytes(2, "little")
                    damaged.write_bytes(data)
                    result = check(damaged)
                    assert result.returncode != 0, f"accepted {label} {profile} length {length + delta}"
                    assert expected in result.stderr, result.stdout + result.stderr
                    cases += 1
                    print(f"Rejected {label} {profile} length {length + delta}")
    assert prg.read_bytes() == original, "input PRG changed"
    assert source.read_bytes() == elf.data, "input ELF changed"
    print(f"Layout audit negative fixtures: {cases} passed; original package unchanged")


if __name__ == "__main__":
    main()
