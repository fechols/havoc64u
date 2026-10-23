"""Validate DLT4 and generate manifest-owned DMC1 soundtrack bindings.

Run with --write-rust after encoding assets or editing catalog.json. Normal
builds check the generated Rust rather than silently changing tracked files.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct

ROOT = Path(__file__).resolve().parents[1]
DIGITAL_OFFSET = 0x100000
RING_OFFSET = 0xfe0000
RING_BYTES = 0x20000
REU_BYTES = 0x1000000
MUSIC_CAPACITY = RING_OFFSET - DIGITAL_OFFSET
STAGE_COUNT = 18
PROFILES = {"stereo20k3": (20000, 2), "mono16k3": (16000, 1)}
CUES = ("Title", "Lowlands", "Canyon", "Fortress", "Carrier", "Victory",
        "Defeat", "CrystalCurrent", "AshesAndSteel", "TempestDrive",
        "GorgeWarden", "CitadelHeart", "CarrierOverdrive", "AsteroidBelt",
        "ShatteredMoon", "ShipGraveyard", "OrbitalShipyard", "SpaceCarrier",
        "JumpGate", "Ascent", "GateGuardian")


def checksum(data):
    a = b = 0
    for value in data:
        a = (a + value) % 255
        b = (b + a) % 255
    return a | b << 8


def _crc_entry(value):
    for _ in range(8):
        value = ((value << 1) ^ (7 if value & 128 else 0)) & 255
    return value


CRC_TABLE = tuple(map(_crc_entry, range(256)))


def validate_blob(blob, metadata, profile):
    """Check actual extents before indexing, then independently verify integrity."""
    assert len(blob) >= 32 and blob[:4] == b"DLT4", "invalid digital music magic"
    rate, frames, stream_bytes, entries, books, bits, channels = struct.unpack_from("<IIIHHBB", blob, 4)
    assert (rate, channels) == PROFILES[profile], "invalid digital music profile"
    assert entries == 8 and bits == 3 and frames > 0, "invalid digital music quantizer"
    alignment = 8 // channels
    assert 1 <= books <= min(1024, max(1, frames // alignment)), "invalid digital codebook count"
    assert not any(blob[22:32]), "digital header reserved bytes must be zero"
    stream_offset = 32 + books * entries * 3 * channels
    assert stream_bytes == (frames * channels * 3 + 7) // 8, "invalid compressed frame count"
    crc_offset = stream_offset + stream_bytes
    crc_count = (stream_bytes + 1023) // 1024
    assert len(blob) == crc_offset + crc_count, "invalid digital payload extent"
    segment = frames if books == 1 else frames // books // alignment * alignment
    expected = dict(sample_rate=rate, sample_count=frames, channels=channels,
                    bits_per_sample=bits, codebook_entries=entries, codebook_count=books,
                    samples_per_segment=segment, stream_bytes=stream_bytes,
                    stream_offset=stream_offset, crc_offset=crc_offset,
                    crc_count=crc_count, asset_bytes=len(blob))
    for key, value in expected.items():
        assert metadata[key] == value, f"invalid digital metadata {key}"
    used = (frames * channels * 3) & 7
    assert not used or blob[crc_offset - 1] >> used == 0, "nonzero final packed padding"
    assert hashlib.sha256(blob).hexdigest() == metadata["asset_sha256"], "digital music payload checksum mismatch"
    for key, data in (("header_checksum", blob[:32]),
                      ("codebook_checksum", blob[32:stream_offset]),
                      ("crc_table_checksum", blob[crc_offset:]),
                      ("asset_checksum", blob)):
        assert checksum(data) == metadata[key], f"invalid digital {key}"
    crc = 0
    for chunk, begin in enumerate(range(stream_offset, crc_offset, 1024)):
        for value in blob[begin:min(begin + 1024, crc_offset)]:
            crc = CRC_TABLE[crc ^ value]
        assert crc == blob[crc_offset + chunk], "digital stream CRC mismatch"
    return expected


def load_catalog(root=ROOT, *, validate=True):
    manifest = json.loads((root / "assets/music/catalog.json").read_text())
    assert manifest["version"] == 2, "unsupported soundtrack manifest"
    tracks = manifest["tracks"]
    assert 1 <= len(tracks) <= 255, "invalid soundtrack track count"
    stages = manifest.get("stages")
    assert isinstance(stages, list) and len(stages) == STAGE_COUNT, "invalid soundtrack stage count"
    names, variants = set(), set()
    directory_bytes = (16 + 16 * len(tracks) + len(CUES) + STAGE_COUNT + 15) & ~15
    offset = DIGITAL_OFFSET + directory_bytes
    result = []
    for index, entry in enumerate(tracks):
        name, variant, profile = entry["name"], entry["rust_name"], entry["profile"]
        assert re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name), "invalid soundtrack asset name"
        assert re.fullmatch(r"[A-Z][A-Za-z0-9]*", variant), "invalid soundtrack Rust name"
        assert name not in names and variant not in variants, "duplicate soundtrack track"
        assert profile in PROFILES, "invalid soundtrack profile"
        names.add(name)
        variants.add(variant)
        metadata = json.loads((root / f"assets/music/{name}.json").read_text())
        blob = (root / f"assets/music/{name}.dlt").read_bytes()
        if validate:
            validate_blob(blob, metadata, profile)
        length = metadata["asset_bytes"]
        assert isinstance(length, int) and 32 < length <= MUSIC_CAPACITY, "invalid soundtrack asset extent"
        offset = (offset + 15) & ~15
        assert offset + length <= RING_OFFSET, "soundtrack exceeds 15,597,568-byte music budget"
        result.append(dict(entry, index=index, offset=offset, end=offset + length,
                           metadata=metadata, blob=blob))
        offset += length
    assert [entry["rust_name"] for entry in result[:2]] == ["GalacticDash", "HammondRocker"], "preserve the two established soundtrack IDs"
    by_name = {entry["name"]: entry for entry in result}
    assert manifest["default_gameplay"] in by_name, "unknown gameplay track"
    assert set(manifest["cues"]).issubset(CUES), "unknown soundtrack cue"
    cue_names = [manifest["cues"].get(cue, manifest["default_gameplay"]) for cue in CUES]
    assert all(name in by_name for name in cue_names), "unknown cue track"
    cue_ids = [by_name[name]["index"] for name in cue_names]
    assert all(name is None or isinstance(name, str) and name in by_name for name in stages), "unknown stage track"
    stage_ids = [255 if name is None else by_name[name]["index"] for name in stages]
    directory = bytearray(directory_bytes)
    directory[:4] = b"DMC1"
    directory[4:8] = bytes((2, len(result), len(CUES), 16))
    struct.pack_into("<H", directory, 8, directory_bytes)
    directory[10] = STAGE_COUNT
    for entry in result:
        metadata = entry["metadata"]
        at = 16 + entry["index"] * 16
        rate, channels = PROFILES[entry["profile"]]
        struct.pack_into("<BBHIIHH", directory, at, int(bool(entry.get("fixture"))), channels, rate,
                         entry["offset"], metadata["asset_bytes"],
                         metadata["asset_checksum"], metadata["header_checksum"])
    cue_offset = 16 + 16 * len(result)
    stage_offset = cue_offset + len(CUES)
    directory[cue_offset:stage_offset] = bytes(cue_ids)
    directory[stage_offset:stage_offset + STAGE_COUNT] = bytes(stage_ids)
    struct.pack_into("<H", directory, 14, checksum(directory))
    return dict(tracks=result, cues=cue_ids, stages=stage_ids, directory=bytes(directory), end=offset,
                used_bytes=offset - DIGITAL_OFFSET)


def validate_digital_payload(reu, *, empty_ring=True, root=ROOT):
    assert len(reu) == REU_BYTES, "digital music requires a 16 MiB REU"
    catalog = load_catalog(root)
    directory = catalog["directory"]
    assert reu[DIGITAL_OFFSET:DIGITAL_OFFSET + len(directory)] == directory, "soundtrack directory mismatch"
    previous = DIGITAL_OFFSET + len(directory)
    metadata = {}
    for entry in catalog["tracks"]:
        start, end = entry["offset"], entry["end"]
        assert not any(reu[previous:start]), "soundtrack alignment padding must be zero"
        blob = reu[start:end]
        validate_blob(blob, entry["metadata"], entry["profile"])
        assert blob == entry["blob"], "digital music payload differs from source"
        metadata[entry["name"]] = entry["metadata"]
        previous = end
    assert not any(reu[previous:RING_OFFSET]), "soundtrack trailing padding must be zero"
    if empty_ring:
        assert not any(reu[RING_OFFSET:]), "PCM ring must start zero-filled"
    return metadata


def pack_catalog(reu, root=ROOT):
    assert len(reu) == REU_BYTES, "digital music requires a 16 MiB REU"
    catalog = load_catalog(root)
    reu[DIGITAL_OFFSET:RING_OFFSET] = bytes(MUSIC_CAPACITY)
    reu[DIGITAL_OFFSET:DIGITAL_OFFSET + len(catalog["directory"])] = catalog["directory"]
    for entry in catalog["tracks"]:
        reu[entry["offset"]:entry["end"]] = entry["blob"]
    return catalog


def rust_catalog(catalog):
    tracks = catalog["tracks"]
    lines = ["// Generated by tools/digital_format.py --write-rust; do not edit.",
             "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]", "#[repr(u8)]", "pub enum Track {"]
    for entry in tracks:
        if entry["index"] == 0:
            lines.append("    #[default]")
        lines.append(f'    {entry["rust_name"]} = {entry["index"]},')
    lines += ["}", "impl Track {", f"    pub const ALL: [Self; {len(tracks)}] = [" +
              ", ".join("Self::" + e["rust_name"] for e in tracks) + "];",
              "    pub const fn from_id(id: u8) -> Option<Self> {", "        match id {"]
    lines += [f'            {e["index"]} => Some(Self::{e["rust_name"]}),' for e in tracks]
    lines += ["            _ => None,", "        }", "    }", "    #[inline(always)]",
              "    pub fn descriptor(self) -> &'static Descriptor { descriptor(self) }", "}"]
    for variant, name in (("GalacticDash", "GALACTIC"), ("HammondRocker", "HAMMOND")):
        entry = next(e for e in tracks if e["rust_name"] == variant)
        lines.append(f'pub const {name}_OFFSET: u32 = 0x{entry["offset"]:06x};')
    metadata = next(e for e in tracks if e["rust_name"] == "GalacticDash")["metadata"]
    for name, key, typ in (("SAMPLE_COUNT", "sample_count", "u32"), ("SAMPLES_PER_SEGMENT", "samples_per_segment", "u32"),
                           ("STREAM_BYTES", "stream_bytes", "u32"), ("STREAM_OFFSET", "stream_offset", "u32"),
                           ("CRC_OFFSET", "crc_offset", "u32"), ("CRC_COUNT", "crc_count", "usize"),
                           ("ASSET_BYTES", "asset_bytes", "u32"), ("HEADER_CHECKSUM", "header_checksum", "u16"),
                           ("CODEBOOK_CHECKSUM", "codebook_checksum", "u16"), ("CRC_TABLE_CHECKSUM", "crc_table_checksum", "u16"),
                           ("ASSET_CHECKSUM", "asset_checksum", "u16")):
        lines.append(f"pub const {name}: {typ} = {metadata[key]};")
    lines += [f'pub const CATALOG_END: u32 = 0x{catalog["end"]:06x};',
              f'pub const DIRECTORY_BYTES: usize = {len(catalog["directory"])};',
              f'pub const DIRECTORY_CHECKSUM: u16 = {checksum(catalog["directory"])};',
              f"const TRACK_DESCRIPTORS: [Descriptor; {len(tracks)}] = ["]
    for entry in tracks:
        m = entry["metadata"]
        lines += ["    Descriptor {", f'        asset_offset: 0x{entry["offset"]:06x},',
                  f'        asset_bytes: {m["asset_bytes"]},']
        lines += [f"        {key}: {m[key]}," for key in ("header_checksum", "codebook_checksum", "crc_table_checksum", "asset_checksum")]
        lines.append("        metadata: Metadata {")
        for field, key in (("total_samples", "sample_count"), ("stream_bytes", "stream_bytes"),
                           ("samples_per_segment", "samples_per_segment"), ("stream_offset", "stream_offset"),
                           ("crc_offset", "crc_offset"), ("codebook_count", "codebook_count"),
                           ("sample_rate", "sample_rate"), ("channels", "channels")):
            lines.append(f"            {field}: {m[key]},")
        lines += ["        },", "    },"]
    lines += ["];", '#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.data")]',
              f"static TRACK_DATA: [Descriptor; {len(tracks)}] = TRACK_DESCRIPTORS;",
              '#[cfg_attr(target_arch = "mos", link_section = ".phase.boot.data")]',
              f"static BOOT_DATA: [Descriptor; {len(tracks)}] = TRACK_DESCRIPTORS;",
              "#[inline(always)]", "pub fn descriptor_startup(track: Track) -> &'static Descriptor { &BOOT_DATA[track as usize] }",
              "#[inline(always)]", "pub fn descriptor(track: Track) -> &'static Descriptor {",
              "    &TRACK_DATA[track as usize]", "}",
              '#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.data")]',
              f"static CUE_TRACKS: [Track; {len(CUES)}] = [" + ", ".join("Track::" + tracks[i]["rust_name"] for i in catalog["cues"]) + "];",
              "#[inline(always)]", "pub fn track_for_cue(cue: crate::audio::MusicState) -> Track { CUE_TRACKS[cue as usize] }",
              f"pub const STAGE_COUNT: usize = {STAGE_COUNT};",
              '#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.data")]',
              "static STAGE_TRACKS: [u8; STAGE_COUNT] = [" + ", ".join(map(str, catalog["stages"])) + "];",
              "#[inline(always)]", "pub fn track_for_scene(cue: crate::audio::MusicState, stage: u8) -> Track {",
              "    use crate::audio::MusicState;",
              "    if !matches!(cue, MusicState::Title | MusicState::Victory | MusicState::Defeat | MusicState::Ascent) {",
              "        if let Some(&id) = STAGE_TRACKS.get(stage as usize) {",
              "            if let Some(track) = Track::from_id(id) { return track; }",
              "        }", "    }", "    track_for_cue(cue)", "}",
              '#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.data")]',
              "static DIRECTORY: [u8; DIRECTORY_BYTES] = [" + ", ".join(map(str, catalog["directory"])) + "];",
              "#[inline(always)]", "pub fn catalog_directory() -> &'static [u8; DIRECTORY_BYTES] { &DIRECTORY }",
              '#[cfg(feature = "host-tools")]', f"pub const TRACK_ASSETS: [(Track, &[u8]); {len(tracks)}] = ["]
    lines += [f'    (Track::{e["rust_name"]}, include_bytes!("../assets/music/{e["name"]}.dlt")),' for e in tracks]
    lines += ["];", "const _: () = assert!(CATALOG_END <= RING_OFFSET);",
              "const _: () = assert!(STAGE_COUNT == crate::assets::SEGMENT_COUNT as usize);",
              "const _: () = assert!(CUE_TRACKS.len() == crate::music::CUE_COUNT);"]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-rust", action="store_true")
    args = parser.parse_args()
    catalog = load_catalog()
    generated = rust_catalog(catalog)
    path = ROOT / "src/digital_catalog.rs"
    if args.write_rust:
        path.write_text(generated, encoding="utf-8", newline="\n")
    else:
        assert path.read_text(encoding="utf-8") == generated, "stale digital catalog; run tools/digital_format.py --write-rust"
    print(f'DLT4 catalog: {len(catalog["tracks"])} tracks; {catalog["used_bytes"]:,} / {MUSIC_CAPACITY:,} bytes')


if __name__ == "__main__":
    main()
