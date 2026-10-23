"""Validate both authored stereo DLT3 tracks inside a v10 REU package."""
import hashlib
import json
from pathlib import Path
import struct


ROOT = Path(__file__).resolve().parents[1]
DIGITAL_OFFSET = 0x100000
VICTORY_OFFSET = 0x500000
RING_OFFSET = 0xfe0000
RING_BYTES = 0x20000
REU_BYTES = 0x1000000
TRACKS = (("galactic-dash", DIGITAL_OFFSET, VICTORY_OFFSET),
          ("hammond-rocker", VICTORY_OFFSET, RING_OFFSET))


def validate_digital_payload(reu, *, empty_ring=True):
    """Match checked source metadata and every payload byte, excluding mutable RAM.

    The source hash covers the header, every codebook, compressed stream and
    cumulative CRC table. Runtime uses the corresponding smaller checksums.
    """
    assert len(reu) == REU_BYTES, "digital music requires a 16 MiB REU"
    metadata = {}
    for name, start, end in TRACKS:
        metadata[name] = validate_track(reu, name, start, end)
    if empty_ring:
        assert not any(reu[RING_OFFSET:]), "PCM ring must start zero-filled"
    return metadata


def validate_track(reu, name, start, end):
    metadata = json.loads((ROOT / f"assets/music/{name}.json").read_text())
    length = metadata["asset_bytes"]
    assert 32 < length <= end - start, "digital asset exceeds reservation"
    blob = reu[start:start + length]
    assert blob[:4] == b"DLT3", "invalid digital music magic"
    rate, samples, stream_bytes = struct.unpack_from("<III", blob, 4)
    entries, books = struct.unpack_from("<HH", blob, 16)
    assert (rate, samples, stream_bytes, entries, books, blob[20], blob[21]) == (
        24000, metadata["sample_count"], metadata["stream_bytes"], 16,
        metadata["codebook_count"], 4, 2), "invalid digital music header"
    assert metadata["channels"] == 2, "digital music metadata must describe stereo"
    assert not any(blob[22:32]), "digital header reserved bytes must be zero"
    stream_offset = 32 + books * entries * 3 * 2
    crc_offset = stream_offset + stream_bytes
    assert stream_bytes == samples, "invalid compressed stereo frame count"
    assert stream_offset == metadata["stream_offset"], "invalid digital stream offset"
    assert crc_offset == metadata["crc_offset"], "invalid digital CRC offset"
    assert length == crc_offset + (stream_bytes + 1023) // 1024, "invalid digital payload extent"
    assert hashlib.sha256(blob).hexdigest() == metadata["asset_sha256"], "digital music payload checksum mismatch"
    assert not any(reu[start + length:end]), "digital reservation padding must be zero"
    return metadata
