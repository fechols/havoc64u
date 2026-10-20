"""Reject incorrect transfer lengths and IRQ-unsafe music-directory placement."""
from pathlib import Path
import runpy
import struct
import subprocess
import sys
import tempfile


def main():
    source = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/havoc.elf")
    prg = Path(sys.argv[2]) if len(sys.argv) > 2 else source.with_suffix(".prg")
    checker = Path(__file__).with_name("check-layout.py")
    overlay = runpy.run_path(str(checker.with_name("check-overlays.py")))
    elf = overlay["Elf"](source)
    original = prg.read_bytes()
    tables = (("__sim_load_sizes", 8, "SIM load table disagrees", "SIM profile"),
              ("__code_sizes", 24, "Code size table disagrees", "OVL7 image"))

    def check(path, image=source):
        return subprocess.run([sys.executable, str(checker), str(image), str(path)],
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
        directory_entry = next(
            table.offset + index * table.entry_size
            for table in elf.sections if table.kind == 2
            for index, symbol in enumerate(elf.symbol_tables[table.index])
            if overlay["readable_name"](symbol.name) == "havoc_c64::MUSIC_DIRECTORY")
        compiler_frames = next(section for section in elf.sections if section.name == ".noinit")
        damaged_elf = Path(directory) / "damaged.elf"
        for label, offset, form, value, expected in (
            ("music-directory-IRQ-save", 14, "<H", compiler_frames.index, "music directory must not enter IRQ-saved"),
            ("music-directory-size", 8, "<I", 167, "music directory must hold all 21"),
        ):
            data = bytearray(elf.data)
            struct.pack_into(form, data, directory_entry + offset, value)
            damaged_elf.write_bytes(data)
            result = check(prg, damaged_elf)
            assert result.returncode != 0, f"accepted {label}"
            assert expected in result.stderr, result.stdout + result.stderr
            cases += 1
            print(f"Rejected {label}")
        for name, value, expected in (
            ("__phase_pixels_prefix_size", 0x3001, "PIXELS prefix overlaps raster tails"),
            ("__phase_pixels_aux_start", 0x6f1f, "PIXELS helpers overlap raster masks"),
            ("__phase_pixels_aux_size", 0x2e1, "PIXELS helpers exceed workspace"),
            ("__phase_pixels_aux_load", 0x43f21, "incorrect PIXELS helper REU address"),
            ("__phase_pixels_size", 0x3000, "incorrect composed PIXELS size"),
        ):
            entry = next(table.offset + index * table.entry_size
                         for table in elf.sections if table.kind == 2
                         for index, symbol in enumerate(elf.symbol_tables[table.index])
                         if symbol.name == name)
            data = bytearray(elf.data)
            struct.pack_into("<I", data, entry + 4, value)
            damaged_elf.write_bytes(data)
            result = check(prg, damaged_elf)
            assert result.returncode != 0, f"accepted {name}={value}"
            assert expected in result.stderr, result.stdout + result.stderr
            cases += 1
            print(f"Rejected {name}={value}")
    assert prg.read_bytes() == original, "input PRG changed"
    assert source.read_bytes() == elf.data, "input ELF changed"
    print(f"Layout audit negative fixtures: {cases} passed; original package unchanged")


if __name__ == "__main__":
    main()
