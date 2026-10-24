# Digital soundtrack catalog

`catalog.json` is the version 2 authoring source for the DLT4 soundtrack, its
order, semantic music cues, and stage overrides. Append new tracks to preserve
numeric track IDs. Galactic Dash and Hammond Rocker remain at IDs 0 and 1.
Mission Failed uses retired fixture slot 2; existing stage recordings retain
IDs 3 through 16. Citadel Core, Orbital Shipyard, Space Carrier, and Jump Gate
occupy IDs 17 through 20 in that order.

William Echols composed the twenty-one supplied recordings for H.A.V.O.C.; they and
their encoded versions are released under the [MIT License](LICENSE).

Each track names its `.dlt` and adjacent `.json` metadata, a Rust identifier,
and either `stereo20k3` (20 kHz, two channels, three bits per channel) or
`mono16k3` (16 kHz, one channel, three bits per sample). `default_gameplay`
supplies every cue not explicitly listed under `cues`; cue names match
`MusicState` in `src/audio.rs`. Galactic Dash is stored once, as mono, and is
the default cue fallback, including ascent. Title selects Hammond Rocker,
Victory selects Galactic Dash, and Defeat selects Mission Failed. No catalog
entry is a temporary fixture.

The `stages` array has exactly 18 entries in campaign order. Its first seven
entries select Lowland Approach, Crystal Cave, Iron Canyon, Canyon Gate,
Ashen Wastes, Foundry Belt, and Fortress Run. Stage indices 8 through 14 select
Storm Front, Meteor Reach, Carrier Coast, Carrier Assault, Asteroid Belt,
Lunar Awe (for Shattered Moon), and Ship Graveyard. Indices 7 and 15 through 17
(Citadel Core, Orbital Shipyard, Space Carrier, and Jump Gate) have their own
recordings too. Each stage recording plays throughout its stage and its boss.
Title and attract music, victory, defeat, and the ascent cue use their cue mapping,
regardless of stage.
This preserves the existing semantic cue assignments, including the cues shared
by Iron Canyon/Canyon Gate, Ashen Wastes/Foundry Belt, and Fortress Run/Citadel
Core. Fortress Run remains track ID 9. All eighteen stage entries name a
recording; invalid stage indices use the cue mapping.

Hammond Rocker uses true stereo. Galactic Dash, Mission Failed, and all eighteen
stage recordings use mono decoding with the right output delayed by 160 samples
(10 ms at 16 kHz). Songs retain their full lengths and loop until the selected
track changes.

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
bytes. The twenty-one-track directory occupies 400 bytes. Assets follow
in manifest order at 16-byte alignment. All remaining bytes before `$FE0000`
must be zero. `$FE0000–$FFFFFF` is mutable playback memory and is excluded from
immutable soundtrack validation.

The complete directory, encoded assets, checksums and alignment must fit
15,597,568 bytes. The twenty-one stored assets occupy 14,647,076 bytes, including
the directory and alignment, leaving 950,492 bytes free (about 928 KiB). The
eighteen stage recordings total 27:04.963; all twenty-one unique recordings
total 34:37.8835. The twenty mono recordings total 31:07.950. The capacity check
uses actual encoded extents, including codebooks, CRC checkpoints, stage
mappings and alignment; it does not rely on a duration projection or omit a
stored duplicate. Storage fit does not establish listening quality.
