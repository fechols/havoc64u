#!/usr/bin/env python3
"""Package an already validated native build as the single player download.

Run scripts/build.sh for the complete build and validation first. This helper
uses Python 3.11+ standard-library modules and never rebuilds the game.
"""
import argparse
import configparser
from pathlib import Path
import re
import tempfile
import tomllib
import zipfile
from reu_layout import PACKAGE_ID


ROOT = Path(__file__).resolve().parents[1]


def archive_name(root=ROOT):
    with (root / "Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(
            r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
        raise ValueError("Cargo package version must be a safe semantic version")
    return f"havoc-v{version}.zip"


def release_contents(package_dir, root=ROOT):
    sources = {
        "havoc.cfg": package_dir / "havoc.cfg",
        "havoc.prg": package_dir / "havoc.prg",
        "havoc.reu": package_dir / "havoc.reu",
        "README.txt": root / "docs/launch.txt",
        "LICENSE": root / "LICENSE",
        "MUSIC-LICENSE.txt": root / "assets/music/LICENSE",
    }
    contents = {name: path.read_bytes() for name, path in sources.items()}
    if any(not data for data in contents.values()):
        raise ValueError("Release files must not be empty")
    if contents["havoc.cfg"] != (root / "config/havoc.cfg").read_bytes():
        raise ValueError("Package configuration differs from config/havoc.cfg; rebuild the package")
    config = configparser.ConfigParser(interpolation=None)
    config.read_string(contents["havoc.cfg"].decode("utf-8"))
    expected = {
        "C64 and Cartridge Settings": {
            "RAM Expansion Unit": "Enabled", "REU Size": "16 MB",
            "Map Ultimate Audio $DF20-DFFF": "Enabled",
        },
        "SID Addressing": {
            "SID Socket 1 Address": "Unmapped", "SID Socket 2 Address": "Unmapped",
            "UltiSID 1 Address": "$D400", "UltiSID 2 Address": "$D420",
            "UltiSID Range Split": "1/2 (A6)", "Auto Address Mirroring": "Disabled",
        },
        "Audio Mixer": {"Vol Socket 1": "OFF", "Vol Socket 2": "OFF",
                        "Vol Sampler L": "+6 dB", "Vol Sampler R": "+6 dB",
                        "Pan Sampler L": "Left 3", "Pan Sampler R": "Right 3"},
    }
    for section, entries in expected.items():
        for key, value in entries.items():
            if config.get(section, key, fallback=None) != value:
                raise ValueError(f"Ultimate audio configuration requires {key}={value}")
    prg, reu = contents["havoc.prg"], contents["havoc.reu"]
    if len(prg) <= 2 or prg[:2] != b"\x01\x08":
        raise ValueError("Expected a native PRG loading at $0801")
    if (len(reu) != 16 * 1024 * 1024 or reu[:4] != b"SM64" or reu[17] != 4
            or int.from_bytes(reu[4:8], "little") != PACKAGE_ID):
        raise ValueError("Expected a v17 16 MiB four-SID REU package")
    return contents


def verify_archive(path, contents):
    with zipfile.ZipFile(path) as archive:
        if archive.namelist() != list(contents) or archive.testzip() is not None:
            raise ValueError("Release archive has invalid entries or checksums")
        for name, expected in contents.items():
            if archive.read(name) != expected:
                raise ValueError(f"Release archive differs from validated input: {name}")


def package_release(package_dir, root=ROOT):
    package_dir = Path(package_dir)
    output = package_dir / archive_name(root)
    contents = release_contents(package_dir, root)
    with tempfile.NamedTemporaryFile(prefix=f".{output.name}.", suffix=".tmp",
                                     dir=package_dir, delete=False) as temporary:
        temporary_path = Path(temporary.name)
    try:
        with zipfile.ZipFile(temporary_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, data in contents.items():
                archive.writestr(name, data)
        verify_archive(temporary_path, contents)
        temporary_path.replace(output)
    finally:
        temporary_path.unlink(missing_ok=True)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package_dir", nargs="?", type=Path, default=ROOT / "dist")
    parser.add_argument("--print-name", action="store_true",
                        help="print the versioned archive filename without packaging")
    args = parser.parse_args()
    try:
        output = archive_name() if args.print_name else package_release(args.package_dir.resolve())
    except (OSError, ValueError, KeyError, configparser.Error, zipfile.BadZipFile) as error:
        parser.exit(1, f"Release packaging failed: {error}\n")
    print(output)


if __name__ == "__main__":
    main()
