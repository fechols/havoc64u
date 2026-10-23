# Digital soundtrack catalog

`catalog.json` is the version 2 authoring source for the DLT4 soundtrack, its
order, semantic music cues, and stage overrides. Append new tracks to preserve
numeric track IDs. Galactic Dash, Hammond Rocker, and the mono Galactic Dash
placeholder remain at IDs 0, 1, and 2.

William Echols composed the eight supplied recordings for H.A.V.O.C.; they and
their encoded versions are released under the [MIT License](LICENSE).

Each track names its `.dlt` and adjacent `.json` metadata, a Rust identifier,
and either `stereo20k3` (20 kHz, two channels, three bits per channel) or
`mono16k3` (16 kHz, one channel, three bits per sample). `default_gameplay`
supplies every cue not explicitly listed under `cues`; cue names match
`MusicState` in `src/audio.rs`. The mono Galactic Dash duplicate is marked
`fixture` and remains the fallback for stages without their own recordings.
It is included in the actual packed catalog's size check.

The `stages` array has exactly 18 entries in campaign order. Its first six
entries select Lowland Approach, Crystal Cave, Iron Canyon, Canyon Gate,
Ashen Wastes, and Foundry Belt. The remaining twelve entries are `null`, which
uses the cue mapping. Each stage recording plays throughout its stage; Canyon
Gate keeps playing when its boss starts. Title and attract music, victory,
defeat, and the ascent cue always use their cue mapping, regardless of stage.
This preserves the existing SID cue assignments, including the cues shared
by Iron Canyon/Canyon Gate and Ashen Wastes/Foundry Belt.

The two menu/reward recordings use true stereo. All six stage recordings and
the placeholder use mono decoding with the right output delayed by 160 samples
(10 ms at 16 kHz).

After encoding or changing the manifest, run:

```text
python3 tools/digital_format.py --write-rust
python3 tools/test-digital-catalog.py
```

The generator validates each asset and updates `src/digital_catalog.rs` with
bounded descriptors, asset inclusions, scene mappings, and exact directory bytes.
Normal builds verify this generated file. Host baking and package validation
reject mismatched assets, changed directory bytes, nonzero padding, and extents
beyond the soundtrack region.

The DMC1 directory starts at REU `$100000`. Its 16-byte header contains magic
at bytes 0-3, version 2 at byte 4, track count at byte 5, cue count at byte 6,
entry size 16 at byte 7, little-endian directory length at bytes 8-9, stage
count 18 at byte 10, three reserved zero bytes at 11-13, and a Fletcher checksum
at bytes 14-15 computed with its own field zeroed.
Each entry is `<BBHIIHH>`: flags (bit zero means temporary fixture), channel
count, sample rate, absolute REU address, byte length, whole-asset Fletcher,
and header Fletcher. The entry index is its track ID. One byte per cue follows
the entries, then one byte per stage. Stage ID 255 means cue fallback; every
other ID must name a catalog track. Zero padding aligns the directory to 16
bytes. The nine-track directory occupies 208 bytes. Assets follow
in manifest order at 16-byte alignment. All remaining bytes before `$FE0000`
must be zero. `$FE0000–$FFFFFF` is mutable playback memory and is excluded from
immutable soundtrack validation.

The complete directory, encoded assets, checksums and alignment must fit
15,597,568 bytes. A separate future estimate for 21 final tracks uses both
stereo songs plus 27 additional minutes of mono music, excluding the temporary
mono duplicate once all recordings are available. Its capacity test includes
codebooks, CRC rounding, stage mappings and worst-case alignment. The actual
catalog, including the retained placeholder, must pass the packer's size check.
Storage fit does not establish listening quality.
