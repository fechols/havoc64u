# Digital soundtrack catalog

`catalog.json` is the version 2 authoring source for the DLT5 soundtrack, its
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

Each track's `codebook_count` is an explicit encoding input. DLT5 stores eight
little-endian signed-16 deltas per channel and balances packing groups across
segments, retaining predictor continuity. The catalog allocator gives tracks
approximately equal adaptation intervals and includes every header, CRC and
alignment byte in its budget. Reallocate and regenerate the soundtrack with:

```text
python3 tools/digital_format.py --allocate-codebooks
cargo build --release --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build
python3 tools/regenerate-digital-music.py --encoder target/digital-music/tool-build/release/encode-digital-music --jobs 4 --work-dir target/digital-music/dlt5-512k --baseline target/digital-music/dlt5-512k-baseline --install
```

On Windows, append `.exe` to the encoder path. Regeneration stages all assets,
checks original/intermediate hashes, validates the complete catalog, and rejects
per-channel SNR regressions or decoded peaks above -1 dB before installing. The
previous DLT5 assets and authenticated listening previews remain in
`target/digital-music/dlt5-512k-baseline`; new previews and the quality report are
under `target/digital-music/dlt5-512k`. Use a fresh baseline directory for each
subsequent encoding revision so quality gates compare against the current
soundtrack. The older `dlt5-baseline` preserves DLT4 and is not the baseline for
this allocation. Physical listening remains a separate check. Normal builds
validate existing assets without re-encoding.
The 512 KiB expansion improves every channel's SNR against the preceding DLT5
encoding by 0.100–0.219 dB (mean 0.151 dB). Rescaling both decodes to the same
preserved reference gain confirms the improvement; the fixed-reference report
and authenticated A/B excerpts are in `target/digital-music/dlt5-512k`.
These are coding-error measurements, not a listening verdict.
Container-only metadata edits are accepted only after checking the original
sample rate, channel count and decoded PCM against an authenticated intermediate
or recorded PCM hash. Provenance retains the original container hash and tags,
and records the current container hash and tags separately.

### Offline quality experiments

The selected combined encoder improves all 21 tracks over the expanded DLT5
baseline. Arithmetic mean track SNR rises from **22.827077 to 23.016140 dB**
(**+0.189063 dB**); pooled signal/error SNR rises from **22.825633 to 23.008737 dB**.
Every channel improves by **0.133186 to 0.236906 dB**, and all selected outputs
pass the existing sample/true-peak limits at the exact original reference gain.
Eighteen tracks select beam width 128, Hammond Rocker and Lowland Approach select
64, and Lunar Awe selects 32. The complete catalog remains 16,121,852 bytes.

| Encoder experiment | Mean track SNR | Gain over baseline | Tracks qualifying |
|---|---:|---:|---:|
| Existing training, beam 32 | 22.827077 dB | 0 dB | Baseline |
| Beam-residual retraining, beam 32 | 22.940984 dB | +0.113906 dB | 21 |
| SOM candidate, beam 32 | 22.911009 dB | +0.083931 dB | 19 |
| Existing training, beam 64 | 22.864327 dB | +0.037249 dB | 18 |
| Existing training, beam 128 | 22.889002 dB | +0.061925 dB | 18 |
| Combined, selected width per track | 23.016140 dB | +0.189063 dB | 21 |

Method averages retain baseline results when a candidate fails the peak or
non-regression gate. The SOM-only trials for Lowland Approach and Foundry Belt
exceeded the peak limit. The combined result does not isolate SOM's incremental
contribution on top of the other improvements; the SOM-only comparison does.
Full measurements are in `target/digital-music/quality-experiments/quality-report.json`.

`tools/quality-digital-music.py` compares the existing encoder with beam-residual
retraining, an eight-node scalar self-organizing map (SOM), wider beam searches,
and their combination. All candidates keep the track's profile, frame count,
codebook count, and complete asset size. Their signed-16 predictors and rounded
8-bit publication use the existing DLT5 decoder.

```text
python3 tools/quality-digital-music.py --encoder target/digital-music/quality-tool-build/release/encode-digital-music --reference-dir target/digital-music/dlt5-512k --jobs 8 --install
```

The first run freezes the baseline assets and authenticates the reference WAVs
against their recorded PCM hashes. Subsequent runs reuse that frozen baseline
and validate cached candidates. `--tracks galactic-dash hammond-rocker` runs a
pilot; installation requires the complete catalog. The default work directory
is `target/digital-music/quality-experiments`.

Selection uses independently decoded, full-track integer squared error against
the same normalized reference for every candidate. Total error must decrease,
neither stereo channel may regress, and sample/true peaks must remain at or below
-1 dB. Candidates that exceed peaks are rejected at the frozen gain. Unchanged
tracks retain their baseline bytes. The report includes every configuration,
rejection, selected result, encoding time, and preview path; it does not claim a
physical listening result.

Selected tracks store `encoder_options` in the catalog and provenance. Normal
regeneration first reproduces and authenticates the original baseline reference
from the source, then applies the selected training/search settings. It therefore
does not depend on retaining experiment WAVs. These recipes bind the profile and
codebook count; a different allocation needs a new measured reference and
experiment. Use a new work directory to compare a later soundtrack revision.

After independently encoding or changing the manifest, run:

```text
python3 tools/digital_format.py --write-rust
python3 tools/test-digital-catalog.py
```

The generator validates each asset and updates `src/digital_catalog.rs` with
bounded descriptors, asset inclusions, scene mappings, and exact directory bytes.
Normal builds verify this generated file. Host baking and package validation
reject mismatched assets, changed directory bytes, nonzero padding, and extents
beyond the soundtrack region.

The DMC1 directory starts at REU `$080000`. Its 16-byte header contains magic
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
16,121,856 bytes. The twenty-one stored assets occupy 16,121,852 bytes, including
the directory and alignment, leaving four bytes free. Compacting the preceding
REU code and model region adds 524,288 bytes of codebooks. Codebooks occupy
1,749,920 bytes, with average adaptation intervals of 20.915–20.920 ms. Galactic
Dash uses 7,341 books, Hammond Rocker 10,036, and the other tracks 4,242–4,743. The
eighteen stage recordings total 27:04.963; all twenty-one unique recordings
total 34:37.8835. The twenty mono recordings total 31:07.950. The capacity check
uses actual encoded extents, including codebooks, CRC checkpoints, stage
mappings and alignment; it does not rely on a duration projection or omit a
stored duplicate. Storage fit does not establish listening quality.
