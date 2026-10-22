# Native performance benchmarks

`tools/bench-native.py` measures complete frames in the calibrated C64 Ultimate
VICE build used by `tools/vice-smoke.py`. These are emulator measurements, not
claims about physical C64 Ultimate frame rates. The package is read-only during
measurement; its PRG, REU, and ELF hashes are recorded and checked afterward.

Current package ID `$534D0800` uses a 1 MiB REU image and four built-in UltiSIDs,
with music at `$0C0000-$0FFFFF` and code overlays packed into the lower banks.
Recorded measurements below retain their original package layouts and hashes;
earlier 16 MiB results do not measure the relocated loader. Use the configuration
and REU capacity matching the package when reproducing a historical run.

Current span summaries require only four-SID PAL and NTSC results. Use
`--historical-dual-sid` to validate archived matrices containing both SID profiles.

## Ground obstacle visibility: 2026-10-22

The terrain-coverage rule now includes terrestrial rocks, buildings and ordinary
mounted enemies, including Canyon wall batteries. It applies to full meshes and
distance/budget proxies, intentionally exposing shallow embedded bases. The
painter remains the default; the optional span backends still reproduce painter
ordering and do not provide per-pixel depth correction.

The actual first Lowlands rock at stage tick 458, position `(-70, -110, 336)`,
loses all 259 of its visible pixels to terrain without protection. Native
captures restore all 259 pixels on PAL and NTSC; a Foundry building fixture
restores 176. The host regression follows the first authored rock from spawn to
retirement, and the campaign sweep covers 165,888 combinations of ground actors,
camera heights, scroll phases, depths and proxy choices. Silhouette checks use
coverage rather than nonblack ink, so black rock facets are included.

Both untouched production packages pass memory, overlay, IRQ and package
checks. Nine fixtures, cold/retained palettes, three CIA phases, both video
standards and before/after builds produce 216 native captures. Every capture
matches all 10,000 display bytes and its background against independent Python
Q16 edge walking followed by the production Rust cell writer and HUD. Ordered
geometry matches after removing visibility metadata; gameplay state is identical
apart from live paused-input timestamps in 72 frame pairs.

| Measurement | PAL | NTSC |
| --- | ---: | ---: |
| Mean render-frame time change | +0.400% | +0.334% |
| Largest case render-frame time change | +1.746% | +2.541% |

All 36 paired fixture/palette medians retain at least 95% of baseline throughput.
These are calibrated VICE measurements of paused production render frames with
live audio and three CIA phases. They exclude simulation work, and the starting
raster line is uncontrolled. The standard instrumented benchmark cannot link
even the clean baseline because instrumentation exceeds resident memory; no
runtime or layout workaround was introduced. These results are not physical
hardware measurements or a full simulation benchmark.

Host checks pass 362 tests with music kernels, 184 no-default-feature library
tests, and both optional span renderer suites. Each library configuration retains
its intentionally ignored native fixture exporter. Native negative audits pass
142 layout and 76 overlay cases. Resident headroom stays at 226 bytes; GEOMETRY
grows by 13 bytes and PIXELS is unchanged.

The frozen source, production packages, captures and validation scripts are under
`target/ground-visibility`. `production-comparison.json` records paired timing,
geometry, state and package hashes; `native-oracle.json` records image matches.
Generated evidence is not committed.

## Span-renderer optimizations: 2026-10-21

Both optimized renderers are correct in the measured matrix, but neither beats
the standard painter. They remain **disabled by default**. The deferred backend
improves its own unoptimized version by 0.33–0.47% in mean full-frame time;
front-to-back costs more than the optimized deferred backend despite visiting
fewer intervals in the host model.

This experiment freezes commit `70f7d6e716329f586f0b5bfe4aae1571b12570e0`,
including its revised stereo audio assets. It does not reuse the older packages
from the original span experiment below. `target/span-opt` contains the source
archive, isolated packages, reports and provenance. The variants are:

- `original`: frozen painter before the HUD fix.
- `corrected`: the same source with the shared per-pixel HUD fix.
- `unoptimized`: original deferred compositor with that same HUD fix.
- `spans`: contained no-op, uniform bulk splice and prepared row pointers.
- `unhoisted`: optimized deferred compositor with only pointer hoisting disabled.
- `front`: noncommitting terrain-coverage prepass, reverse stable traversal,
  shrinking uncovered intervals, direct cell writes and dust last.

All use matching assets, music kernels, SID profiles and benchmark settings.
The same twenty fixtures, zero/ten ticks, cold/retained palettes and three CIA
phases give **320 cases / 960 measured samples per variant**. Every comparison
matches raw ordered geometry, game state and decoded displayed pixels exactly.
No palette-history mismatch was waived. The matrix summary makes 7,680 paired
comparisons by reusing those samples across its eight variant pairs.

| Profile | Corrected painter | Optimized deferred | Front-to-back | Deferred / front change vs corrected |
| --- | ---: | ---: | ---: | ---: |
| Two SID PAL | 126.898 ms | 128.997 ms | 131.357 ms | +1.65% / +3.51% |
| Two SID NTSC | 122.600 ms | 124.480 ms | 126.678 ms | +1.53% / +3.33% |
| Four SID PAL | 128.114 ms | 129.987 ms | 132.384 ms | +1.46% / +3.33% |
| Four SID NTSC | 123.683 ms | 125.544 ms | 127.798 ms | +1.50% / +3.33% |

Values are means of each case's three-sample median full-frame time. Against the
frozen original painter, deferred is 2.03–2.18% slower and front-to-back is
3.83–4.05% slower. The shared HUD correction itself costs 0.48–0.69%. A winner
must improve all four aggregate means against **both** painters and have no
case below 0.95 times either painter's FPS. Deferred fails that per-case bound
in six comparisons against the original and two against the corrected painter;
front fails in 92 and 66 respectively. Both also fail the aggregate speed gate.
Held-FIRE scheduling is reported separately and also favors the painter.
Maximum measured audio lateness among the optimized variants is 1.191 ms.

Pointer hoisting is retained: the isolated `unhoisted -> spans` comparison
improves every aggregate mean by 0.07–0.09%, with exact state and pixels. It
prepares pointers once per triangle band, advances them on clipped rows too,
and adds no runtime selection test. Set `HAVOC_SPAN_PREPARED_POINTERS=0` when
building a deferred package to reproduce the control.

The 900-band host census records 241,263 painter cell-row writes, 185,463
deferred writes and 190,153 front-to-back writes. Both span paths reduce spills
from 414 to 109. Deferred takes 2,855 contained no-ops and 33,641 uniform bulk
paths. Front visits 47,366 intervals versus deferred's 129,388, but emits 28,284
fragments versus 21,617 and performs another 4,140 coverage spans. Its native
fragment iterator restarts from the row head, unlike the host iterator, so the
host visit census is not a native cycle model. Native timings include that
bookkeeping, the coverage prepass, cell writes, packing, publication waits and
the existing code transfers. Saved overdraw does not recover those costs here.

Host suites pass for both backends, including 900 raw/decoded scene bands,
144 consecutive frames with independently evolved palettes, maximal holes,
clear-color ownership, spill reconstruction and persistent edge state. The
shared HUD tests cover overwrites before and after spill, including background
ink. Native harnesses pass 17,300 cell checks, 16,708 deferred checks and 896
front checks. Front checks include 256 Q16 fixtures in 800 multi-triangle bands,
immutable coverage walks, exact quantized output, ABI guards and no duplicate
scene-pixel submissions. This does not count band initialization, spill
expansion or later HUD work as scene overdraw.

All six production packages pass unchanged memory and overlay limits; no
generic 32-bit arithmetic helpers return. Four-SID resident headroom is 1,230
bytes for the painter, 904 for deferred and 824 for front. PIXELS prefix
headroom is respectively 840, 618 and 493 bytes; helper headroom is 37, 135 and
135 bytes. Front reuses eight preparation-scratch bytes for row heads, and both
backends reuse exactly 2,560 retired command bytes. No framebuffer, memory
boundary or extra DMA stage was added. Default packages contain no span backend.
Negative fixtures pass 121/69 layout/overlay cases for the painter, 124/75 for
deferred and 128/79 for front; twenty summary-tool tests cover missing evidence,
hash/state/CIA mismatches and the exact 5% boundary.

All twelve production renderer/SID/video configurations pass exact decoded
title pixels, eight gameplay frames and 69 smoke scenarios each: four controls,
22 weapon cases and 43 streaming cases. Package hashes remain unchanged during
testing. These production binaries are separate from the instrumented timing
packages, and all artifacts remain outside `dist`. Reports and the effective
configuration audit live under `target/span-opt`.

A stale smoke-control assertion was also corrected. A right roll moved from
x=-70 to x=-68, but the old test compared against x=-68 from before the tap gap,
while the previous left roll was still coasting. The test now uses the existing
active-to-released-frame directional displacement check; activation, direction,
countdown, cooldown and released-input checks remain mandatory. Three focused
regressions reject stationary or wrong-direction motion and held input. The
original failure, reproduction and old smoke tool are retained alongside the
corrected retry. Timing reports all use the unchanged, frozen original tool.

Build instrumented packages with
`bash scripts/build-benchmark.sh OUTPUT RENDERER`,
where `RENDERER` is `painter`,
`spans` or `front`. Each benchmark report records the complete fixture list and
invocation. Run
`python3 tools/summarize-span-benchmarks.py target/span-opt --optimization-matrix --historical-dual-sid`
to verify all package hashes and comparisons and produce
`summary.json`; exit status 1 correctly reports the failed speed gate. The
separate pointer-hoist retention gate passes. Reproduce kernel checks with
`tools/test-cells-kernel.py`, `tools/test-spans-kernel.py` and
`tools/test-front-spans-kernel.py` in the configured SDK. These measurements use
calibrated VICE; they make no physical C64 Ultimate timing claim.

## Span-buffer experiment: 2026-10-21

The optional `span-buffer` renderer resolves each scene band into disjoint runs
before cell painting. It preserves the current painter ordering and Q16 edges;
the HUD still follows the scene. It is **disabled by default**: correctness
passes, but the measured implementation does not pass the performance gate.

The baseline is the clean post-refactor commit
`2e2522536520bfc6560cbbec85175606ababd444`, archived beneath
`target/span-buffer/baseline-source` before implementation. Its archive hash and
shell-file newline normalization are recorded in `baseline-source-manifest.json`.
Both renderers were separately packaged with identical assets and sound features.
No comparison uses the older `s2` or pre-refactor binaries.

The matrix contains twenty fixtures: title, four terrestrial scenes, missiles,
full pools, section loading, ascent/orbital crossings, four crowded orbital
scenes, four bosses, and two victory phases. Zero/ten simulation ticks and
cold/retained palettes make 80 cases per video/SID combination, with three CIA
phases each. All **320 cases / 960 paired samples** retain exact projected
geometry (including visibility flags), decoded game state and displayed pixels.
Raw VIC slot assignments can differ. No palette-history exception was needed.

| Profile | Mean full frame, before -> spans | Change | Mean raster, before -> spans | Cases losing >5% FPS |
| --- | ---: | ---: | ---: | ---: |
| Two SID PAL | 126.210 -> 128.970 ms | +2.19% | 26.850 -> 30.027 ms | 2 |
| Two SID NTSC | 121.950 -> 124.272 ms | +1.90% | 25.867 -> 29.021 ms | 0 |
| Four SID PAL | 127.185 -> 129.907 ms | +2.14% | 26.906 -> 30.120 ms | 0 |
| Four SID NTSC | 122.913 -> 125.420 ms | +2.04% | 25.874 -> 29.092 ms | 4 |

These are means of per-case three-sample medians. Ordinary held-FIRE scheduling
also slows: 9.333 -> 9.120 FPS (two-SID PAL), 9.676 -> 9.454 (two-SID NTSC),
7.867 -> 7.683 (four-SID PAL), and 8.204 -> 8.023 (four-SID NTSC). The six
threshold failures are cold/retained zero-tick `enemies-orbital-shipyard-full`
under two-SID PAL and four-SID NTSC, plus zero-tick `missiles` under four-SID NTSC.
The worst FPS ratio is 0.943935x. Maximum audio lateness remains below 1.188 ms;
the matrix reports no dropped geometry, input overflow, score underrun, changed
stack guard, or publication-check failure.

The host census explains the tradeoff: across 900 scene bands, cell-row submissions
fall from 241,251 to 185,453 (23.13%) and spills from 415 to 109 (73.73%).
Composition visits 129,324 runs for 36,595 incoming spans, then emits 21,606
final spans; the maximum observed row has 25 runs. The saved cell work does not
offset run-list maintenance and flushing. All 900 bands match raw pixels and
coverage, and the 144-frame independent-palette-history test has zero decoded
differences. A constructed test still demonstrates why unused palette slots can
change a later equal-frequency quantizer tie; both histories are cross-checked
under identical preceding palettes rather than waiving arbitrary differences.

The independent native harness passes 16,708 checks, including 256 raster
fixtures over all 25 bands, maximal fragmentation, protected background ink,
register/memory guards and nonoverlapping emitted pixels. Negative audits reject
122 layout and 70 overlay corruptions. Both production profiles pass the memory,
ownership and package checks. The four-SID production build retains 1,003 bytes
of resident headroom, 668 bytes in PIXELS and 197 in its helper tail. The three
removed generic 32-bit arithmetic helpers remain absent. The compositor borrows
2,560 retired command bytes and adds no framebuffer or separate code transfer;
the existing PIXELS transfers grow by 103 bytes per frame.

All four production SID/video combinations also pass exact title pixels and
69 smoke scenarios each: four controls, twenty-two weapons, and forty-three
streaming scenarios. Their `smoke-PROFILE-VIDEO/report.json` files record intact
input package hashes and no observed music underruns. These production packages
are distinct from the instrumented timing packages; both sets remain isolated
from `dist`.

Feature-off rebuilds pass both profiles' checks with the compositor absent.
Their PIXELS prefix/helper are byte-identical to the frozen baseline; all 325
executable functions match after relocation normalization. Whole-package hashes
differ because two unchanged resident iterator specializations exchanged link
order and their call addresses changed. `default-parity.json` records this
comparison rather than claiming whole-package byte identity.

Reproduce the native test with `python3 tools/test-spans-kernel.py`. Build each
variant using `bash scripts/build-benchmark.sh OUTPUT PROFILE painter` or
`bash scripts/build-benchmark.sh OUTPUT PROFILE spans`. Measure with
`tools/bench-native.py --samples 3 --compare BASELINE/report.json --pixel-identity`
and matching fixture/video settings; each report records its complete invocation.
`python3 tools/summarize-span-benchmarks.py target/span-buffer` verifies package
hashes, every case and sample, and writes `summary.json`. Its nonzero exit here
correctly reports the failed performance gate. Reports, frozen packages, native
captures, and `host-census.log` remain under `target/span-buffer`; native-kernel
and negative-audit evidence is under `target/spans-test`. These are calibrated
VICE measurements, not physical C64 Ultimate timing.

## Guarded single-framebuffer publication: 2026-10-21

PIXELS now prepares each complete band before publishing any of its bytes.
`fast_cell_stage` consumes the coverage/HUD-slot/overflow tables as a 400-byte
staging buffer, preserving the preceding palette inputs for spilled cells.
The new `publish_staged_band` copies those bytes only outside the target
band's raster exclusion interval. It waits with IRQs enabled and masks only
the final coherent raster recheck and fixed copy. The interval is
`[40 + 8*band, 64 + 8*band)`, covering all Y-scroll positions and timing
margins; it does not wait for v-sync. Global background changes remain
progressive, as documented in the hardware guide.

The final publisher is 308 bytes, with eight-byte bitmap loops and four-cell
attribute loops. PIXELS grows from 15,117 to 15,419 bytes; its 637-byte helper
tail and total checksummed composition remain unchanged. There is no new
framebuffer allocation. Coverage must be cleared after staging, so its cost
is included in frame measurements. The benchmark's geometry checkpoint now
uses ordinary resident code to keep the four-SID instrumented constants within
their existing limit.

Validation includes both host feature suites, 16,366 native cell/staging
checks, 256 quantizer bands, 1,076 assembled publisher cases (57 observed
raster-rollover retries), and both production layouts/overlay audits. The
publisher model charges an extra bus cycle for every VIC read and color
store: its conservative masked bound is 8,473 CPU-cycle units, or 2.102 PAL
lines, against a seven-line requirement. This is a model bound, not a
physical-machine measurement. Native benchmark traces independently require
25 complete copies per frame and measure the successful masked guard through
the final visible store, including any coherent-read retry.

All four production SID/video combinations pass the controls, weapons and
streaming suites (67 scenarios each). After the loop unroll, all four pass
another 20 weapon scenarios and exact title comparison; there are no music
underruns. Reports are in `target/scanout/smoke-*` and
`target/scanout/final-smoke-*`. The final 18-case NTSC matrix, with three CIA
phases per case, has identical bitmap/screen/color bytes, geometry and game
state against the unchanged `s2-pkg` baseline. Mean FPS ratio is 0.9672, with
0.944 ms of polling and 2.142 ms of copy/scheduling work per frame. Its longest
successful protected operation is 81 bus cycles (0.079 ms, 1.25 NTSC lines).
The 12-case four-SID NTSC matrix also meets this bound. These are calibrated
emulator figures; physical scan-out acceptance remains outstanding.

The complete PAL matrix passes all correctness checks: 572 cases, three CIA
phases each (1,716 samples), and 42,900 protected commits, with exact native,
game-state and geometry hashes against `s2-pal`. The aggregate report
`target/scanout/full-pal-report.json` retains the 271 completed cases from an
interrupted sequential run and three passed parallel shards; it verifies the
complete key set, identical package hashes and all per-sample guards. Its
summary is `target/scanout/full-pal-summary.json`.

Mean frame time is 115.453 -> 119.104 ms (+3.16%); the mean matched-case FPS
ratio is 0.96886. Summed phase work is 49.425 -> 53.660 ms, including 0.693 ms
polling and 2.208 ms copying/scheduling per frame. Staging averages 4.024 ms
against the former 4.087 ms finish; raster time is 22.020 -> 22.575 ms,
including the additional coverage clears. The longest successful protected
operation is 84 bus cycles (0.085 ms, 1.33 PAL lines), with one observed final
guard retry and maximum audio lateness of 1.191 ms. The opening real-time
sample changes from 9.692 to 9.343 FPS.

Correctness passes do not waive the existing performance warning: four cases
exceed its 5% FPS-loss threshold. They are `bridge-11-100` and `bridge-16-0`,
both at ten simulation ticks with cold and retained palettes. The largest
loss is 5.66% FPS (6.00% more frame time). These are recorded separately in
the aggregate as the cost of the chosen publication strategy.

## The sky as the VIC background: 2026-10-21

A census of the cells that spill into the pixel form (session tool
`spill_census.py`, a patched `cells.S` logging every spill) showed what they had
in common: the band's clear colour sat in slot 1 of nearly every one of them.
Terrain is painted far to near, so a cell crossed by several terrain spans
passes through "sky + three terrain colours" while its sky pixels are still
waiting to be overdrawn; the fourth colour forces the spill although the cell
ends with three. Lowlands spilled 44 cells per frame, the missiles frame 59,
the carrier 57 — about 5–7 ms each (expansion 2.1 k cycles, pixel-form rows
140 each, the quantizer 3.8 k).

The VIC's global background register is the natural fourth slot, and no
geometry triangle in any fixture uses black: PIXELS now writes the frame's
clear colour to `$D021` and code 0 means "the clear colour". The cell writer
stores slot *keys* (colour XOR the clear colour), so the clear colour is key 0
like a free slot and every zero test in the span fast paths is unchanged — the
raster and the HUD entries XOR once per span, the finish XORs the attribute
nibbles back (a free slot shows the clear colour), the spill expands slot 0 to
the clear colour, and `fast_pack_cell` takes the background from the same
zero-page byte (code 0, never a slot, the nearest-colour fallback). Black HUD
panels become ordinary slot colours in their three bands, where they replace
the sky slot one for one. The unused band packer (`fast_pack_band` and its
`$7700` sampling table) went with it: PIXELS 15,495 → 15,117 bytes. The host
reference (`cells.rs`, `pack_cell` with a background argument) mirrors the
representation; `tests/cells.rs` checks the cell band against the classic pixel
path over 925 bands of real frames with their sky colours (identical pixels;
427 spills instead of about 1,600). `tools/test-cells-kernel.py` (16,306
checks) and `tools/test-pack-kernel.py` (256 bands, with backgrounds) pass.

Pixels change only where a cell used to be quantized: the quick PAL matrix
shows 0–29 changed pixels per frame (none on the black-background boss stage),
geometry and game state identical everywhere, the cells in question now showing
all their colours exactly (`tools/compare-benchmarks.py`,
`target/bench/bg-quick-diff`). Work on the quick matrix: Lowlands 41.3 → 37.0
ms, missiles 68.3 → 62.5, carrier 55.0 → 51.9, title 55.8 → 52.6, dreadnought
unchanged. Full matrices (`target/bench/bg-pal`, `bg-ntsc`, difference reports
in `bg-pal-diff` / `bg-ntsc-diff`): 572 cases each, geometry and game state
identical in every case, 10.3 changed pixels per frame on average (at most 51,
240 frames unchanged), work 50.4 → 49.1 ms (PAL) and 50.3 → 48.9 (NTSC), raster
22.7 → 21.7, pack 4.4 → 4.0, canyon frames −11 %, total 115.5 → 113.8, opening
real-time sample 9.23 → 9.58 FPS; in `mos-sim` the Lowlands frame's PIXELS
23.5 → 19.4 ms (cell writes 662 → 470 cycles per span, finish 14.4 k → 10.4 k
per band), the missiles frame 43.7 → 37.5, the carrier 31.0 → 27.5. Both smoke
suites pass, and the host title snapshot now matches the native title capture
pixel for pixel through the background-aware decoder (`--title-reference`).
Frozen as `opt-baseline-v5-pal` / `-ntsc` / `-pkg`; later steps are
pixel-identical against v5. Native captures carry `$D021` as byte 10,000, and
the host snapshot writes the same byte, so every decoder shows code 0 in the
frame's background colour.

## Phase F: the flight model, and the 16-bit audit: 2026-10-21

The player now flies havoc2's assisted model in Q8 fixed point at the 50 Hz
tick (`src/game/flight.rs`, native body `fast_flight_step` in
`target-c64/asm/flight.S`, resident): the stick commands a target velocity
(3.0 units per tick sideways, 2.75 vertically, a 2.5 push in the roll's
direction while the hull spins), the velocity follows it by 3/32 per tick
(213 ms time constant), a soft wall `v ≤ (edge + 7 − q) >> 3` eases the ship
into the former ±100 × ±65 hard limits without ever crossing them, the hull
banks with −16 × its lateral acceleration (±28°, so it rolls into a turn,
levels while sliding and rolls back while braking) and pitches with the
commanded climb (±15°); both angles lag by an eighth per tick. All deltas use
the error's magnitude, so left and right are mirrored within a unit and every
state settles exactly (`tools/test-flight-kernel.py`: 147,850 steps identical
to the C transcription of the host reference). The kernel is 638 bytes and
1.9 k cycles per tick (0.03 ms; 0.15 ms per frame at five ticks) and replaced
the 493-byte compiled `move_player`; the resident Q8 multiply and bounded
division kernels moved to the `$9900` table page (20 bytes spare there) to
make room. The mesh rotation's identity shortcut still applies whenever the
ship is level, which it is exactly whenever it has settled.

The audit of 32-bit arithmetic then removed every `u32`/`i32` from the target
build (details in [hardware.md](hardware.md#build-and-verification)): a 16-bit
millisecond clock with window clamps and paused-frame sampling, the
section-local HUD distance, 16-bit HUD bars, victory easing, wildlife slope
and music-kernel offsets. `__udivsi3` (380 bytes), `__divsi3` (219) and
`__mulsi3` (103) are no longer linked; `computed_charge` 202 → 105 bytes,
`Game::hud` 533 → 309, the IRQ body 532 → 488. Resident code: 10,018 →
8,759 bytes (1,453 bytes free before `$3000`); SIM prefix 13,731 → 13,542
(largest profile 16,655 of 16,896); PIXELS prefix 15,438 → 15,291; `Game`
622 → 621 bytes with the ten-byte flight record included. The score stays
32-bit (boss bonuses alone pass 65,535 in a full run; its adds and digit
conversion link no helpers).

Measurement note: a parallel session landed the per-cue music kernels, the
scan-out publish markers and the 1 MiB REU layout in the same tree while this
work was in progress, so the `s2`/`v5` matrices are no longer like-for-like
baselines (their packages predate the missile-lock change too: `Game` 609
bytes, no `missile_targets`). A quick PAL matrix of the combined tree
(`target/bench/f1-quick-pal`) keeps every idle fixture's geometry and pixel
hash equal to `s2` except `missiles` (whose missiles are steered by the newer
lock logic); its +11 % of work is the music kernels' IRQ time and the new
`copy`/`wait` publish accounting, not the flight step (SIM slices moved by
≤ 0.4 ms at ten ticks, `title` meshes unchanged at 1.33 ms). The next full
PAL/NTSC matrix of this tree becomes the baseline for later steps.

## SIM: the shot collision kernel: 2026-10-21

`Game::update_shots` tested every moved player shot and missile against the
twelve actor slots with compiled code: a descriptor lookup, the `fast_swept_z`
call and the outlined `actor_center_y` and `close_xy` for each live slot, about
400 cycles per pair, so a fight with ten shots in flight spent 40–50 k cycles
per tick on misses. The scan is now `fast_shot_scan` (`target-c64/asm/shots.S`),
the native body of `Game::shot_target`: it takes its arguments in registers,
keeps its state in caller-saved zero page, tests the lateral distance before
selecting a radius (a slot more than 255 units to the side costs about 70
cycles), then the depth sweep and the centred vertical distance with the
release build's wrapping arithmetic, and returns the first hit slot in slot
order; the Rust keeps the hit's consequences. `tools/test-shots-kernel.py`
compares it with a C transcription of the host loop over 56,564 scenes (every
edge of every kind class's hit volume, both sweep directions, slot-order ties,
wrapping corners, random descriptors). In `mos-sim` a shot costs 0.7 k cycles
against twelve empty slots, 1.6 k against twelve distant live slots and 2.6 k
against twelve within 256 units. `update_shots` shrank 2,481 → 2,197 bytes and
the kernel adds 441, so the largest SIM profile keeps 528 bytes of room.

Quick PAL matrix against the v5 run: every native, pixel, geometry and game
hash identical; the missiles fixture's SIM slice 21.7 → 16.8 ms at ten ticks
(work 81.0 → 76.3), the mean SIM slice 3.8 → 3.4 ms, other fixtures within
noise. Full PAL matrix (`target/bench/s2-pal`, `--pixel-identity` against
`opt-baseline-v5-pal`): 572 cases, no comparison failure, every hash identical,
work 49.1 → 49.0 ms, SIM 3.9 → 3.7, missiles −6.3 %, opening real-time sample
9.58 → 9.69 FPS. The NTSC matrix, smoke suites and the s2 baseline freeze are
still to run.

## SIM: the missile guidance kernel: 2026-10-21

The missiles fixture spent 29 ms of its 95 ms frame in SIM because
`missile_target` (compiled Rust, 1,466 bytes) scans every actor slot for every
live missile on every catch-up tick — about 38 k cycles per missile per tick.
`target-c64/asm/missile.S` (`fast_steer_missile`) is the native body of
`Game::steer_missile`: the search in the host's stable order (slots, left
turret, right turret, core), then `missile_velocity` per axis with the release
build's wrapping 16-bit product (two quarter-square partials), a bounded
twelve-round subtraction for the truncating quotient and the ±1 steering step.
It takes a 19-byte argument record and uses GEOMETRY's `$9140–$917F` scratch,
idle during SIM. 1,024 bytes of assembly replace about 1,600 bytes of Rust: the
shared SIM image shrank 13,678 → 13,098 bytes and the largest profile now has
671 bytes of room (it had 105). The kernel costs 1.1 k cycles with no live
slot, 4.8 k with six, 6.7 k with twelve and an active boss.
`tools/test-missile-kernel.py` compares it with a C transcription of the host
functions over 3,494 scenes (the host tests' scenarios, the error-clamp and
depth extremes, random scenes with wrapping coordinates), checking the steered
velocity, the chosen target and its closing speed. Writing it exposed two tool
gaps, both fixed: the branch audit could not see a wrapped branch whose target
fell before its section start (objdump prints `beq -$31`, which the pattern did
not match), and the assembler's `<`/`>` operators on a negative constant give
the bytes of its magnitude, so negative immediates are now written as bytes.

PAL and NTSC fixed matrices (`target/bench/s1-pal`, `s1-ntsc`), 572 cases each,
decoded pixels and game state identical to the v4 baselines: work 50.3 → 50.4 /
50.3 ms (the kernel touches only frames with missiles in flight), total
116.2 → 115.5 on both (the smaller SIM image saves 0.8 ms of transfer), opening
real-time sample 9.23 FPS.
The missiles fixture's tick-10 frame: SIM 28.9 → 21.7 ms, work 94.5 → 86.9 ms;
the remaining SIM time is `update_shots`'s per-shot collision scan, the next SIM
candidate. (Fixtures run ten 20-ms ticks in the measured frame; a real 90-ms
frame runs four or five, so their SIM slice is about twice what play pays.)
Frozen as `opt-baseline-s1-pal` / `-ntsc` / `-pkg`; both smoke suites pass;
deployed to the card.

## 2x on work: order-table sort, prep fast path, walker, clip kernel: 2026-10-21

Replaying captured command lists through the PIXELS kernels in `mos-sim`
(`files/frame_cycles.py` in the session workspace splits a frame into
preparation, clear, raster and finish; `files/cell_paths.py` counts the cell
writer's path events) showed where the raster time really went: edge
preparation cost 10.6 k cycles per triangle (three 32-bit divisions and a
shift-add multiply per edge — 7 ms on Lowlands, 31 ms on the 182-triangle
citadel frame), a span cost 740–890 cycles (first use of a colour went through
the general path at about 220 cycles; the 0.6 % of cells that overflow into
the pixel form cost about 270 per row plus a 3.3 k expansion), and the walker
about 460 per painted row. The replay was 5–7 k cycles per scenery record,
of which the projection-cache lookup was 720 per corner (the arithmetic hash
alone 346) and a near-clipped record about 20 k in compiled Rust.

- `target-c64/asm/sort.S` no longer moves commands: a two-pass 8-bit radix
  sort writes the sorted order (one command index per position) to `$7600`,
  the raster heads moved to `$EF20-$FF1F`, and `scenery_last`'s three
  command reversals became a logical scan order (foreground segment first).
  1,800 → ≈ 230 cycles per command; the reorder slice (0.6–2.3 ms) is gone.
- `preparation.S` takes a fast path for edges with dy ≤ 255 and |dx| ≤ 255
  (nearly all): a 24-bit restoring division by the 8-bit dy with leading-zero
  skip, the row-centre fraction `16384/dy` from a 512-byte table when the
  start row is on screen, two quarter-square products. Positions are stored
  with +32767 so the walker reads a row's column from the high word.
  10.6 k → 4.9 k cycles per triangle.
- `raster.S` was rewritten around that: a band's rows split into the segment
  before the middle row and the segment from it (no per-row middle test),
  one loop per orientation (no per-row left/right swap), steps in zero page
  `$D0-$E0` (the compiler's zero-page region is empty; `check-layout.py`
  asserts it stays so), a walk-only loop for unpainted bands, no head
  write-back from a triangle's last band.
- `cells.S`: every fast path takes a free palette slot inline; a spilled
  cell's palette is poisoned (`$FF`) so the loops need no overflow test;
  whole-row pixel paths and a table-driven expansion for overflowed cells;
  unrolled clears.
- `emit.S`: `fast_clip_triangle` is the renderer's `GeometrySink::triangle`
  in assembly (classification, Sutherland–Hodgman against z = 8 with the
  bounded division and quarter-square partials, emission of the polygon);
  the command depth third uses `85 h + (h + l)/3` with a thirds table instead
  of a division; the cache lookup indexes by a byte mix of the six coordinate
  bytes (720 → ≈ 300 cycles per corner; a host census over 54 frames showed
  misses already equal to the distinct vertices, so a larger cache would not
  have helped).
- Harnesses: `tools/test-sort-kernel.py` (416 sorts, with and without
  segments, through the Rust entry), `tools/test-clip-kernel.py` (1,584
  triangles), `tools/test-raster-prep.py` (biased expectations),
  `tools/test-cells-kernel.py` (biased heads, relocated band state), emit,
  mesh, pack, projection, multiply and preparation-ABI checks all pass. The
  smoke suite's targetless-missile check now allows 24 frames instead of 12:
  faster frames simulate fewer ticks each.

PAL fixed matrix, two-SID, 572 cases, three samples, decoded pixels
identical to `opt-baseline-v3-pal` in every case:

| Slice (mean, ms) | Phase G baseline | Cell writer (v3) | Order sort + walker (`walk-pal`) | + clip, segments, pixel paths (`clip2-pal`) | v4 (`v4-pal`) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Total frame | 191.1 | 135.8 | 123.1 | 117.7 | **116.2** (1.64x) |
| Work (no overlay transfers) | 119.9 | 69.8 | 55.7 | 51.8 | **50.3** (2.38x; median 50.0) |
| GEOMETRY work | 41.0 | 17.1 | 15.0 | 11.8 | 10.8 |
| Raster (prep, clear, walk, cell writes) | 36.0 | 35.9 | 23.9 | 23.2 | 22.6 |
| Band finish (pack) | 30.1 | 4.4 | 4.5 | 4.5 | 4.5 |
| HUD | 2.6 | 3.2 | 3.2 | 3.2 | 3.3 |
| Opening real-time sample | 6.12 FPS | 7.91 FPS | 8.63 FPS | 9.08 FPS | 9.22 FPS |

Work per frame (tick 10): Lowlands 41 ms, base 44, bridge 46 (about 22–24 FPS
on the machine, where the frame time is the work time), carrier 55, the
dreadnought boss 65, the full citadel 81; the missiles fixture 95, of which 29
is SIM (`missile_target` scans every actor for every live missile in compiled
Rust — the next SIM kernel). The 90th percentile of the matrix is 69 ms. The NTSC matrix (`target/bench/v4-ntsc`) is pixel-identical
to `opt-baseline-v3-ntsc`: work 69.7 → 50.3 ms (total 135.9 → 116.2, real-time
8.21 → 9.57 FPS). In `mos-sim` the Lowlands
frame's PIXELS now costs 23.5 ms (was 35.8 at the start of this step):
preparation 3.4, clear 1.4, walk and spans 4.6, cell writes 8.6, finish 5.6.
Frozen as `target/bench/opt-baseline-v4-pal` / `-ntsc` with the package in
`opt-baseline-v4-pkg`; both smoke suites pass; deployed to the card.

## The cell writer: spans paint cells, the pack disappears: 2026-10-20

A host census of 55 frames (`src/cells.rs` prototype; see the previous section)
showed that 81% of the 1,000 cells of a frame end with at most one nonblack
colour and only 0.5% need quantisation, while the pack spent about 1,600
cycles per cell reading 32 pixels and ranking a histogram, because overdraw
marks most cells "partial". PIXELS therefore no longer paints pixels:

- `target-c64/asm/cells.S` (the native body of `havoc::cells::CellBand`) writes
  two-bit colour codes straight into a row-major 8×40 table of cell bytes
  (`$9200`), while a three-slot palette per cell (`$9480`) hands out the
  hardware colours on first use; black is always code 0. A slot whose colour
  no longer appears in any of the cell's eight rows is recycled (one table
  lookup per row); a cell asked for a genuine fourth colour is expanded into
  the pixel band at `$8C00` and painted per pixel from then on, with the
  classic rules. Terrain-clear coverage is a parallel nibble table (`$9340`);
  exterior terrain skips covered pixels; HUD ink marks its slot so expanded
  pixels carry the quantizer's priority bit.
- The raster hands every span to `cell_rows`: the first and last cells merge
  through mask tables, interior cells run a per-mode loop (about 39 cycles
  per cell, against 32 for the old four-pixel fill). The HUD, text, bars and
  ambient dust draw through the same `Canvas` trait (`span`, `plot`, `glyph`)
  on the host and the target, so one Rust implementation serves the classic
  pixel band (host reference), the host cell band and the kernels.
- `fast_cell_finish` transposes the 320 code bytes into the visible bitmap,
  writes the 80 attribute bytes from the palettes and sends only overflowed
  cells (about 21 per frame after recycling, 40-75 in terrain stages) through
  the per-pixel quantizer, now exposed as `fast_pack_cell`. `fast_cell_clear`
  replaces the 1,280-byte band clear with 520 bytes (the coverage table only
  when a terrain-clear span dirtied it).
- Exactness: decoded pixels are identical to the classic pipeline; attribute
  bytes differ in slot order (first use instead of frequency ranking, no
  preceding-frame retention), so comparisons decode the frame
  (`tools/bench-native.py --pixel-identity`, `native_pixels_sha256`).
  `tools/test-cells-kernel.py` checks the kernels, the raster hand-off and
  `fast_pack_cell` against a C transcription of the reference over 16,333
  state checks (random bands, 256 exported triangles over 25 bands, layered
  scenes); the host prototype is pixel-identical to the classic pipeline on
  every band of the 55 census frames, HUD included.
- Two tooling consequences: `tools/branch-audit.py` now re-assembles each
  source with line information and compares every branch's label with its
  encoded target (the object-only audit could not see a wrapped branch to a
  same-section label; this one caught a −147-byte branch in the new kernel
  and finds nothing else in 367 branches), and the kernel harnesses relocate
  every fixed scratch address (`-Wa,-defsym`) and assert that their image ends
  below the kernel state.

PAL fixed matrix, two-SID, 572 cases, three samples (`target/bench/cells-pal`,
frozen as `target/bench/opt-baseline-v3-pal` with its package in
`opt-baseline-v3-pkg`), decoded pixels identical to `opt-baseline-v2-pal` in
every case:

| Slice (mean, ms) | Phase G baseline | Precision step (v2) | GEOMETRY kernels | Cell writer (v3) |
| --- | ---: | ---: | ---: | ---: |
| Total frame | 191.1 | 165.6 | 149.2 | **135.8** (1.41x) |
| Work (no overlay transfers) | 119.9 | 98.8 | 84.1 | **69.8** (1.72x) |
| GEOMETRY work | 41.0 | 31.8 | 17.1 | 17.1 |
| Raster (now including the cell writes) | 36.0 | 30.0 | 30.1 | 35.9 |
| Pack (now the band finish) | 30.1 | 24.9 | 24.9 | 4.4 |
| HUD | 2.6 | 2.9 | 2.9 | 3.2 |
| Opening real-time sample | 6.12 FPS | 6.63 FPS | 7.12 FPS | 7.91 FPS |

The Lowlands opening frame is 56 ms of work (goal: 50 for 20 FPS on the
machine); the 182-triangle citadel scene 114 ms, now raster-bound. The NTSC
matrix (`target/bench/cells-ntsc`, frozen as `opt-baseline-v3-ntsc`) is the
same picture: 572/572 pixel-identical to `opt-baseline-v2-ntsc`, total
165.8 → 135.9 ms, work 98.9 → 69.7 ms. In
`mos-sim` the row walker costs about 225 cycles per row (two Q16 edge steps,
clamps, pointer stepping) plus 140 per triangle band, and a span about 250
plus 39 per interior cell; the raster is where the next step goes. PIXELS is
14,063 of 16,160 bytes; the pack's band driver and row-sampled paths remain
only for `tools/test-pack-kernel.py`.

The PAL two-SID smoke suite (control, weapon and streaming checks,
`target/cells/smoke-2sid-pal-final`) and the NTSC four-SID suite (control and
weapon checks, `target/cells/smoke-4sid-ntsc-final`) pass; the roll check now
allows the countdown enough frames at the new frame rate. Both host test
suites (two- and four-SID features) pass; the build was deployed to the card.

## GEOMETRY in assembly: emit, replay and mesh kernels: 2026-10-20

Three kernels in `target-c64/asm/emit.S` replace the compiled per-triangle
chain (`SceneBudget::triangle` → `camera_point` ×3 → `Renderer::triangle` →
`project_triangle` → `project` ×3, about 8-9 k cycles of glue per face):

- `fast_emit_triangle` is `project_triangle` for the resident renderer: the
  three corners through the 64-entry projection cache (misses call
  `fast_project` in place), area and orientation culling, the screen-edge
  rejections, the 256-command capacity with saturating drops, the exact
  `(z0+z1+z2)/3` depth and the 16-byte command. `tools/test-emit-kernel.py`
  checks 3,184 emissions byte for byte.
- `fast_replay_scenery` consumes the staged scenery records in place (sky
  parallax applied to STATIC_SKY records as it goes) and stops at the first
  record that needs near clipping, which Rust clips and resumes after, so the
  stable sort still sees the authored order.
- `fast_emit_mesh` is the scene sink's mesh path (`SceneBudget::mesh`): every
  vertex of an instance is rotated once by `fast_rotate_vertex`, translated
  with saturating adds and lowered by the chase camera into a 16-byte record at
  `$E000 + 16·index` (the raster tails live there but are idle until PIXELS);
  faces then index the records, take the scene budget in order, project each
  corner on first use and enter the shared emission tail, while a face that
  straddles the near plane is handed back with its indices and colour so the
  caller clips it through the classic path. `tools/test-mesh-kernel.py`
  checks 160 random meshes (3,366 vertex records, 796 commands, 567 clipped
  hand-backs) against a C transcription of the Rust reference, including
  i16 saturation, budget exhaustion and the command capacity. A ten-vertex,
  seventeen-face enemy costs about 67 k cycles (`mos-sim`): 1,900 per vertex
  for the rotation and translation, 1,090 per projected vertex, about 1,500
  per visible face; the Rust chain was about 125 k.

PAL fixed matrix, two-SID, 572 cases, three samples (`target/bench/mesh-pal`),
byte-identical to `opt-baseline-v2-pal` in every case:

| Slice (mean, ms) | Phase G baseline | Precision step (v2) | Emit + replay + mesh kernels |
| --- | ---: | ---: | ---: |
| Total frame | 191.1 | 165.6 | **149.2** (1.28x) |
| Work (no overlay transfers) | 119.9 | 98.8 | **84.1** (1.43x) |
| GEOMETRY work | 41.0 | 31.8 | 17.1 |
| Raster | 36.0 | 30.0 | 30.1 |
| Pack | 30.1 | 24.9 | 24.9 |
| Opening real-time sample | 6.12 FPS | 6.63 FPS | 7.12 FPS |

GEOMETRY is 15,112 of 16,896 bytes (the Rust mesh chain left with it). The
same matrix on NTSC for the precision step (`opt-baseline-v2-ntsc`): total
192.5 → 165.8 ms, work 119.9 → 98.9 ms.

Where the work goes now (Lowlands, ms): raster 22, pack 26, GEOMETRY 11
(scenery replay 6.4, meshes 2.5, sort 1.6), HUD/SIM/SCENERY 10. A host census
of 55 frames (`src/cells.rs` prototype) shows why the pack is next: 81% of the
1,000 cells end the frame with at most one nonblack colour, 18% with two or
three and 0.5% need quantisation, yet overdraw marks most cells "partial" so
the pack reads 32 pixels and builds a histogram for them. Writing two-bit codes
straight from the spans into per-cell bitmap bytes (first-use palettes, dead
slots recycled, a fourth colour spilling the cell to the per-pixel quantizer)
is pixel-identical on every band of those frames and replaces the pack with a
320-byte transpose; that is the next step.

## Hardware frame rate, i8 meshes and the projection kernel: 2026-10-20

### What the real machine says

The space-bar/F frame-rate readout on a physical C64 Ultimate shows **8-10 FPS**
in the opening Lowlands flight, against 6.1-6.6 FPS for the same screens in
the calibrated emulator. The hardware probe (`dist/hwprobe/hwprobe.prg`), run
at stock speed because the turbo configuration was not active, measured REU
transfers at 1,169 µs/KiB — bus speed, slightly slower than the emulator's
1,034 — and ordinary RAM and I/O stores at CPU speed. A 100-125 ms hardware
frame therefore cannot contain the 60-68 ms of bus-speed overlay transfers the
emulator charges: on the C64 Ultimate the REU DMA scales with the turbo clock,
and **the hardware frame is, to first order, the work time** that the reports
record as `phase_work_bus_cycles`. From here every comparison lists both the
emulator's total frame and the work time; the goal of doubling the frame rate
is a work-time goal (about 50 ms per frame for 20 FPS).

### The precision step

This is the one content change of the optimisation programme that moves
pixels, taken deliberately so that everything after it can be held byte-exact
again:

- Model banks are `SMA4` (package `$534D0700`): vertices are `i8` x, y, z in
  units of `1 << shift` with the shift (0-3) and a one-sided flag in the two
  spare descriptor bytes (`assets.rs`, `host.rs`). Every regular enemy, the
  player, the pickups and the medal are exact at shift 0; the ordinary slot-4
  hulls are exact at shift 1 (their doubled coordinates are even); only the
  Dreadnought and the Gate Guardian round to 4-unit steps (shift 2).
- Mesh rotation is a 3×3 Q8 matrix composed once per instance
  (`fast_mesh_matrix`, entries clamped to ±256) and applied by
  `fast_rotate_vertex` as nine quarter-square products with zero entries
  skipped; the former pair sequence rounded each stage separately. Half-size
  debris uses one extra shift instead of a Q8 pre-scale.
- Projection is `80·x/z` and `133·y/z` as one 16×16 product per axis: z is
  normalised to 128..255 with a signed exponent, `PROJECT_X`/`PROJECT_Y` hold
  `scale·32768/d` (128 entries each, the same 512 bytes `RECIPROCAL` used),
  and `>> (15 + e)` restores the scale; y is negated before the shift so both
  axes round the way they always did. The quantisation is 0.003% against the
  former 0.2%, and the former `|y| ≤ 30000` guard is unnecessary.
  `fast_project` (`target-c64/asm/project.S`) is the native body: 1,900 cycles
  in `mos-sim` against 4,383 for the same formula compiled and about 5,250 for
  the previous Rust projection; `tools/test-projection-kernel.py` checks
  183,900 points including z ≤ 0 and every clamp corner.

PAL fixed matrix, two-SID, 572 cases, three samples (`target/bench/precision-pal`,
frozen as `target/bench/opt-baseline-v2-pal` with its package in
`opt-baseline-v2-pkg`), against the Phase G baseline and the previous step:

| Slice (mean, ms) | Phase G baseline | Previous step (`p4a-pal`) | Precision step |
| --- | ---: | ---: | ---: |
| Total frame | 191.1 | 171.9 | **165.6** (1.15x) |
| Work (no overlay transfers) | 119.9 | 105.5 | **98.8** (1.21x) |
| GEOMETRY work | 41.0 | 38.5 | 31.8 |
| Raster | 36.0 | 30.1 | 30.0 |
| Pack | 30.1 | 24.9 | 24.9 |
| Opening real-time sample | 6.12 FPS | 6.30 FPS | 6.63 FPS |

Pixels differ from the Phase G baseline in every case, by construction.
`tools/compare-benchmarks.py` (`target/bench/precision-pal-diff`) classifies
the changes: a mean of 77 and at most 853 of the 32,000 visible pixels per
frame move, almost all along triangle edges (coordinate deltas are 0 or 1 in
38,620 of 41,712 sampled corner comparisons; the larger deltas belong to
off-screen or near-plane vertices, where one model unit is ten pixels). The
"nonlocal" count — pixels whose new colour was not in the old 3×3
neighbourhood — has a median of 0 and a 90th percentile of 6 per play frame,
with 19 in the worst boss scene; the Victory medal reaches 257-286 because its
rotation now rounds once and several of its 116 grazing faces flip in or out.
Twenty-two cases gain or lose one such sliver. The difference images confirm
thin edge outlines and nothing structural.

Also in this build, without any pixel impact on the matrix: SPACE pauses the
flight (a PAUSED caption while the simulation deadline follows the clock), F
toggles the readout, and taking damage runs a 240 ms two-pixel jolt with an
80 ms red border through the bomb impulse sequencer. The key scan accepts a
column only when it also reads high with no row selected, which is what keeps
the calibrated emulator's `iosim` control-port device (it drives `$DC01` bits
5-7 low) from switching the readout on inside every capture.

Two tooling notes: `tools/branch-audit.py` now runs from `target-c64/build.rs`
on every assembled object, so a wrapped conditional branch fails the build
instead of producing a plausible package (it caught two more during this
step); and `tools/test-campaign-tools.py` lost two references to the retired
sky-cache fixture sequence.

Validation of this state: 164 library, 3 host-binary and 133 integration
tests per SID profile; both packages pass the layout, overlay-reference and
package audits; the PAL two-SID smoke suite with control, weapon and streaming
checks passes (`target/precision/smoke-2sid-pal`); the raster, pack, multiply/
matrix/rotation and projection kernel harnesses pass.

## PIXELS kernels and the Q8 multiply: 2026-10-20

Phase 3 of the optimization plan rewrites the hot PIXELS paths in assembly
without changing a pixel. All figures are two-SID PAL, 572 fixed-matrix cases
against `target/bench/opt-baseline-pal/report.json`; native bitmap, screen and
color bytes are identical to the baseline in every case of every run below.

| Step | Change | Mean frame | Raster | Pack |
| --- | --- | ---: | ---: | ---: |
| Phase G baseline | | 191.1 ms | 36.4 ms | 31.4 ms |
| Phase 1 | single framebuffer | 186.3 ms | 36.4 ms | 31.4 ms |
| 3a-3c (`target/bench/p3a-pal`) | zero-page edge stepping with inline clamps, span fill eight stores per pass, absolute-indexed band clear | 180.7 ms | 30.6 ms | 31.5 ms |
| 3d-3e (`target/bench/p3e-pal`) | assembly band dispatcher, partial-cell table and row-sampled packing | 174.5 ms | 30.0 ms | 24.9 ms |

Cumulative speedup 1.095x (cold cases 1.107x, retained 1.084x); the opening
real-time sample reads 6.30 FPS against the baseline's 6.12 and Phase 1's 5.86;
no case regresses beyond the 5% matched-work bound any more. Crossings gain the
most (`bridge-16-306` 0.815x), the busiest enemy scenes the least (1.01-1.02x
for `enemies-storm/crystal/citadel` retained, which had cached sky bands before).

What the kernels do (`target-c64/asm/raster.S`, `pack.S`):

- The row walker copies each triangle's Q16 edge positions into zero page
  (`rc12-rc19`) and their steps into private scratch, clamps both edges inline
  and writes the positions back on exit, so a row costs about 84 cycles of
  stepping instead of 170 through `(zp),y` pointers. Kernel-only cycles over the
  1,024 exported fixtures: 35.0 M to 28.3 M (`tools/test-raster-kernel.py`
  still passes all 25,600 band calls and 48 visibility cases).
- `fast_clear_band` fills the fixed band at `$8C00` with five absolute-indexed
  stores per step (about 6 cycles per byte against 13 for the pointer loop) and
  zeroes the partial-cell table.
- `fast_raster_band` walks a band's ordered 32-byte mask in assembly, deriving
  each triangle's head (`$D000 + 16 i`) and tail (`$E000 + 12 i`) from its
  index; the Rust loop that marshalled a Job per triangle is gone, which also
  made the PIXELS prefix 175 bytes smaller.
- The 160-byte partial-cell table at `$7700` records cells containing a span
  end, a dust speck or HUD ink. Unmarked cells hold one byte per row, so the
  packer compares one pixel per row, histograms four identical pixels per row and
  emits one pattern lookup per row: 524 against 1,131 cycles for a uniform cell
  and 3,156 against 4,856 for a three-color row-uniform cell in `mos-sim`.
  `tools/test-pack-kernel.py` now places the band at `$8C00` with the minimal
  correct marking and checks 216 bands, including 48 new row-uniform fixtures,
  byte for byte against the Rust reference. Overdraw keeps the in-game marking
  conservative (a live title frame leaves 12-40 cells per band unmarked in the
  open bands and none in text or terrain bands), which is why the pack gain is
  6.5 ms rather than the 11 ms a minimal marking would give.
- Raster tails and masks moved from `$6000-$6F1F` to the free `$E000-$EF1F`, so
  the PIXELS prefix may grow to `$6F1F`; it is 12,660 bytes.

Two tooling lessons are recorded in the plan: the llvm-mos assembler wraps an
out-of-range conditional branch silently (a 126-byte `beq` produced a package
that skipped triangles and looked fast; every benchmark now requires hash
identity before its timing counts), and a `bpl` loop cannot count down through
indices of 128 or more.

The exact Q8 multiply (`target-c64/asm/multiply.S`, `tools/test-multiply-kernel.py`)
replaces the resident `mul_q8`'s sign-extended `__mulsi3` call with
quarter-square partial products from a 1 KiB table BOOT builds at `$C000`, and
`fast_rotate_vertex` performs `PreparedRotation::apply`'s exact pair sequence in
one call. Both match their references over 108,226 products and 24,000 vertex
rotations, GEOMETRY shrinks from 16,860 to 16,748 bytes, and the fixed matrix
stays byte-identical. In the full matrix (`target/bench/p4a-pal`, zero
comparison failures) the mean falls to 171.9 ms (1.112x cumulative, cold 1.124x,
retained 1.100x) and GEOMETRY work from 40.6 to 38.5 ms, almost entirely in the
victory scenes whose rotating medal is multiply-bound (`victory-start`
267.8 to 188.1 ms, `victory-hold` 240.9 to 190.4). Play scenes did not move
(`carrier` 173.8 ms before and after, `enemies-citadel-full` 241.7): their
GEOMETRY cost sits in projection's 32-bit products and variable shifts and in
the sink chain, which is where Phase 4 continues. The opaque kernel calls also
cost code size at every `mul_q8` call site, because the compiler can no longer
see which registers the callee leaves alone: SCENERY grew from 13,388 to 13,688
bytes and resident headroom is 344 bytes (332 four-SID).

Remaining profile (final build, PAL mean 171.9 ms): overlay transfers 66.4 ms,
GEOMETRY work 38.5, raster 30.0, pack 24.9, HUD 2.9, SIM 3.9, SCENERY 4.5.

Validation of this state: 161 library, 3 host-binary and 133 integration tests
per SID profile; both packages pass the layout, overlay-reference and package
audits; the PAL two-SID and NTSC four-SID smoke suites pass in
`target/phase4/smoke-*`; the raster, pack and multiply kernel harnesses pass.
Physical hardware acceptance of these changes remains outstanding.

## Single framebuffer, frame-rate readout and sky parallax: 2026-10-20

The renderer now draws into one VIC bank. `draw_pixels` rasterizes each band
and packs it in place into the visible bitmap, screen and live color RAM
(`render::pack_band_in_place`), so the back bank, the color shadow, the
publication raster wait and the whole sky-band cache (`SkyCache`,
`static_sky_bands`, `fast_static_sky_bands`, `prepare_sky`) are gone; every
band is repainted every frame. Freed RAM: `$9100-$94FF`, `$C000-$C3FF` and
`$E000-$FFF9`. Resident headroom grows from 164 to 503 bytes (491 four-SID),
tables shrink from 1,777 to 1,525 bytes, PIXELS from 12,180 to 11,810 bytes
including the new readout, and SCENERY from 13,605 to 13,388 bytes; GEOMETRY
is 16,860 of 16,896 bytes after the parallax replay below.

The space bar toggles a frame-rate readout in tenths beside the boss panel,
measured in PIXELS over windows of at least fifty audio ticks; it is off at
startup, so no capture below includes it.

Fixed matrix, two-SID PAL, 572 cases (143 fixtures, ticks 0/10, cold/retained),
three samples each, in `target/bench/p1-single-fb-pal-v2/report.json` against
the frozen Phase G baseline `target/bench/opt-baseline-pal/report.json`:

| Scope | Baseline | Single framebuffer | Ratio |
| --- | ---: | ---: | ---: |
| All 572 cases, mean | 191.1 ms | 186.3 ms | 0.975 (1.026x) |
| 286 cold-palette cases | 193.3 ms | 186.4 ms | 0.964 |
| 286 retained-palette cases | 189.0 ms | 186.1 ms | 0.985 |
| Best: `bridge-16-306` (publication wait removed) | 138.2 ms | 123.4 ms | 0.893 |
| Worst: `enemies-crystal/0/retained` (sky cache removed) | 160.4 ms | 175.5 ms | 1.094 |
| Opening real-time sample (title into Lowlands) | 6.12 FPS | 5.86 FPS | |

Native bitmap, screen and color bytes are identical to the baseline in all 572
cases, including every retained case: in-place packing reads each cell's
previous bytes before writing it, exactly as the two-bank path saw them. The
23 comparison failures the report lists are retained-palette terrestrial cases
slower than the 5% matched-work bound; every one of them previously reused
cached sky bands (Lowlands retained 11 of 25), which the single-framebuffer
decision deliberately gives up in favor of redrawing the sky each frame. The
publication wait and the per-frame color copy account for the gains, largest in
crossings that used to land on the 20 ms raster sawtooth.

A seeding defect in `tools/bench-native.py` was found and fixed on the way: the
cold-palette zero fill addressed `$D800` while the monitor was in `bank ram`,
which writes the RAM under I/O, so color RAM kept the previous case's palette
slots and the first comparison run showed slot-only byte differences in every
case (and palette-overflow cells resolved differently in 94 of them). The fill
now uses `bank io` and verifies the low nibbles; the earlier run is kept in
`target/bench/p1-single-fb-pal/` for reference and the corrected cold check in
`target/bench/p1-coldcheck-pal/`.

Sky parallax: backdrop faces and the orbital starfield keep the `STATIC_SKY`
flag through crossings, and GEOMETRY shifts flagged records by the ship's
sideways offset and half its height as it replays staged scenery
(`Renderer::project_scenery`, `Game::sky_parallax`); the host reference applies
the same shift before its sink. Applying the offsets inside the SCENERY
overlay instead cost 268 bytes of argument plumbing alone and overflowed it;
the replay form costs about 150 GEOMETRY bytes, recovered by dropping an
unreachable `STATIC_SKY` branch from `SceneBudget` and the saturating camera
subtraction (mesh translation already saturates). Every fixed-matrix fixture
pins the ship at the origin, so the matrix is unaffected and the Phase G
baseline remains the reference for the kernel work; the parallax package's own
PAL matrix is recorded in `target/bench/parallax-pal/report.json`.

Validation: 161 library, 3 host-binary and 133 integration tests per SID
profile (the sky-cache suite is gone; `tests/packing.rs` keeps the packer
tests), 149 library tests without default features and the gameplay suite;
both packages pass the layout, overlay-reference and package audits; PAL
two-SID and NTSC four-SID smoke suites in `target/parallax/smoke-*`.

## Gameplay content before the optimization baseline: 2026-10-20

Three content changes precede the renderer optimization work, so the frozen
performance baseline must be built from this source rather than the earlier
orbital release: regular enemies bake at three halves of their authored size
while bosses keep doubling (`assets::enemy_scaled`, `assets::BOSS_SCALE`),
actors and pickups fly through the camera instead of vanishing in front of it
(`game::ACTOR_RETIRE_Z`, near plane moved from 32 to 8 units), and the bomb
pickup is the HUD's rimmed 4:3 rectangle. These are gameplay and image changes:
fixed-fixture pixels and game-state hashes intentionally differ from every
earlier report, and no timing claim is made for them.

Validation: 160 library, 3 host-binary and 137 integration tests pass per SID
profile, plus 148 library and 14 gameplay tests without host tools. Both shipping
packages pass the layout, overlay-reference and package checks; GEOMETRY grows
from 16,680 to 16,848 of 16,896 bytes (the rectangle, the second rim shape and
the exact near-depth projection guard), PIXELS and SIM are unchanged. The
rebuilt two-SID PAL package passes 67 control, weapon and streaming scenarios and
the four-SID NTSC package 32 control, weapon and attract scenarios in
`target/phase-g/final-smoke-{2sid-pal,4sid-ntsc}/report.json`. Host-rendered
fly-past frames in `target/phase-g/flypast/` show a fighter and a diver sweeping
through the near corners at z 48, 30, 16 and 8 without a visible clip edge.
Physical hardware acceptance of these changes remains outstanding.

## Hardware timing probe: 2026-10-20

`scripts/build-probe.sh` builds `dist/hwprobe/hwprobe.prg`, a standalone
program (`tools/hwprobe/`) that times the operations bounding the game's frame
with CIA 1 as a 1 MHz clock and prints microseconds to the text screen. Import
the game's `havoc.cfg` first, then run the PRG; it issues the game's exact turbo
request (`$D031 = $8F`) after a stock-speed reference loop. The REU stash test
writes 1 KiB into the image's unreserved padding at `$0BFC00` in current builds,
so a loaded `havoc.reu` stays valid. The historical 16 MiB probe used `$FFFC00`;
the measurements below retain that original layout. The probe never returns to
BASIC; reset to exit.

The calibrated emulator reports the model that every fixed-matrix figure in this
document rests on, PAL:

| Measurement | Emulator | Meaning |
| --- | ---: | --- |
| CPU loop, stock (329,229 nominal cycles) | 348,627 us | 0.94 MHz with badline stalls |
| CPU loop, turbo | 5,151 us | 63.9x |
| REU fetch 64 x 1 KiB | 66,184 us | 1,034 us/KiB, about one byte per bus cycle |
| REU stash 64 x 1 KiB | 66,184 us | same |
| 1,024 `STA $D800,X` | 183 us | color RAM is not bus-synchronized here |
| 1,024 `STA $D020` / `LDA $D012` | 1,031 us | one bus cycle per VIC access |
| 1,024 `STA $6000,X` | 167 us | ordinary RAM at turbo speed |
| Copy 8 KiB, to `$6000` or under I/O at `$D000` | 1,808 us | 114,688 nominal cycles at turbo speed |

An ordinary frame transfers about 59 KiB of code and data, which this model
prices at 60 ms of a 160-200 ms frame. The first physical run (2026-10-20),
made with the stock-speed configuration still active (the turbo register read
back `$8F` but the CPU loop measured 329,320 µs, so the "turbo" lines are
stock-speed figures): REU fetch 74,846 µs / 1,169 µs/KiB, REU stash 75,054 µs /
1,172 µs/KiB, 1,024 `STA $D800,X` 11,393 µs, 1,024 `STA $D020` 9,346 µs,
1,024 `LDA $D012` 9,344 µs, 1,024 `STA $6000,X` 10,370 µs, 8 KiB copies
115,380-115,390 µs. The REU therefore moves one byte per bus cycle at stock
speed, as in the emulator. Combined with the 8-10 FPS the in-game readout
shows on the same machine in turbo mode, the overlay transfers cannot cost
bus time there: the C64 Ultimate's DMA follows the turbo clock. A turbo-mode
probe run (import `havoc.cfg` first) remains to be recorded.

## Recorded production validation: 2026-10-18

The visual-refresh production builds pass 264 host tests per SID profile,
with one existing fixture exporter ignored, plus 129 library and 14 gameplay
tests without host tools. Both packages pass memory-layout, overlay-reference,
and baked-asset checks. Final PAL/NTSC smoke checks repeat startup, exact title
output, physical joystick controls, weapons, and runtime guards after the last
text and cave-entry refinements. Reports are under
`target/visual-refresh-validation/final-smoke-{2sid,4sid}-{pal,ntsc}/`;
the initial visual candidate's full streaming and attract checks are under
`smoke-{2sid,4sid}-{pal,ntsc}/` in the same directory. Focused visual and timing
results are documented in [Visual refresh final](#visual-refresh-final-2026-10-18).

Continuous native emulator campaigns also complete all twelve stages and
three bosses: two-SID PAL in 5,630 rendered frames and four-SID NTSC in 5,808.
Both finish with three lives and 64 shield. These runs use only physical
joystick-port-2 input, with no writes to game state or clocks; input package
hashes remain unchanged. Reports and captures are under
`target/native-campaign/{final-2sid-pal,final-4sid-ntsc}/`.
`tools/native-campaign.py` supplies the input-driven campaign scenario through
the existing smoke-test session and accepts its package, video, and output
options. This is distinct from the monitor-seeded checkpoint and boss checks,
and does not establish physical C64 Ultimate performance.

The final production packages pass all thirteen complete cue loops on both
SID profiles and video standards: 52 checks covering 1,696 authored page
visits, 356 music-lane checks, concurrent effects, and zero underruns.
Reports are under `target/music-validation/final-{2sid,4sid}-{pal,ntsc}/`;
unchanged package hashes are recorded in
`target/music-validation/final-shipping-summary.json`. Six native emulator
recordings per profile in `target/music-validation/previews-{2sid,4sid}/`
have zero clipped samples and score banks identical to those final packages.
These checks establish emulator playback for the recorded builds; physical
SID listening and ARMSID mixer balance remain unverified. Generated reports
and recordings are not committed.

## Build and compare

Run these commands from the configured WSL SDK environment:

```sh
bash scripts/build-benchmark.sh target/bench/candidate-instrumented
python3 tools/bench-native.py \
  --package-dir target/bench/candidate-instrumented \
  --out-dir target/bench/candidate-pal --video pal
python3 tools/bench-native.py \
  --package-dir target/bench/candidate-instrumented \
  --out-dir target/bench/candidate-ntsc --video ntsc
```

Build each baseline from its frozen source tree into a separate output directory.
Do not rebuild the baseline from the current checkout. Changes to content or game
behavior must also be applied to that isolated tree before making a performance
comparison. Keep the original frozen package and record the adjusted baseline
separately. In particular, the static-cloud baseline includes the same screen-space
cloud change as the optimized renderer, so that change earns no claimed speedup.

The build script enables the target's optional `benchmarks` feature. Normal
`scripts/build.sh` builds omit the monitor-controlled fixed-work scheduler.
The benchmark script never writes `dist/`. It requires the matching unstripped
ELF, LLVM-MOS DWARF tools, emulator, and ROMs. Their default locations match the
existing native smoke-test setup; `--vice`, `--roms`, and `--llvm-nm` override them.

## Visual refresh final: 2026-10-18

The Crystal Cave, orange Ashen Wastes and persistent level-name HUD pass the
existing five-percent timing gate against the preserved campaign package
`target/bench/campaign-fast11`. The final two-SID instrumented package is
`target/bench/visual-refresh-fast3`. All **72 cases pass**, with three initial
CIA timer phases per case: **216 candidate samples** across PAL and NTSC.

The nine fixtures are Lowlands, Crystal with ordinary and full enemy pools,
Ashen Wastes with ordinary and full enemy pools, and the midpoints of crossings
0, 1, 3 and 4. Every fixture runs with zero and ten simulation ticks and both
cold and retained palettes. Retained samples warm both VIC banks with the same
post-simulation state. The gate requires each baseline/candidate median cycle
ratio to be at least 0.95; no individual sample or publication wait is removed.

| Measurement | PAL | NTSC |
| --- | ---: | ---: |
| Passing fixed cases | 36/36 | 36/36 |
| Lowest baseline/candidate median ratio | 0.980393 | 0.998902 |
| Baseline opening FPS | 6.179145 | 6.326317 |
| Final opening FPS | 6.247815 | 6.492520 |
| Maximum fixed-case audio lateness | 1.169 ms | 1.136 ms |

The separate opening probe uses physical port-2 held FIRE, ordinary simulation
scheduling and a 400-audio-tick deadline. FPS counts completed frames after the
first second. It does not establish a campaign-wide average. Audio lateness
remains below the 2 ms limit. Each measured ordinary frame transfers 58,140
bytes; each crossing frame transfers 59,773 bytes. Every DMA command transfers
at most 1,024 bytes. All checked frames retain intact stack guards, correct
code/model banks, zero geometry drops, zero input overflows and zero music
underruns. Input package hashes remain unchanged.

These are timing comparisons of an intentional visual revision, so the final
images are not required to match the old campaign images. Game-state hashes
remain equal in every case; coordinate/depth changes are confined to the cave
and its two adjacent crossings. Ashen Wastes changes colors while preserving
geometry. The persistent HUD changes native display bytes in every play scene.

The bounded native 3-by-5 glyph helper was checked separately against the first
visual candidate: all 216 samples retain exact geometry, game state, clear
color and native bitmap/screen/color hashes. The final package then preserves
those hashes except for the intended cave-entry lighting change: at transition
tick 200, the clear color becomes black when the backdrop switches to cave
colors. This avoids introducing a fourth nonblack backdrop color while the
roof closes, and removes the remaining PAL publication-window regression.
Geometry and game-state hashes remain equal through that change as well.

Recorded baseline reports are
`target/visual-refresh-validation/baseline-{pal,ntsc}/report.json`; final reports
are `target/visual-refresh-validation/candidate-fast3-{pal,ntsc}/report.json`.
The case ratios, phase deltas and explicit image-comparison exceptions are in
`target/visual-refresh-validation/timing-comparison-fast3.json`. The separate
glyph equivalence comparison is `timing-comparison-fast2.json` in the same
directory. These generated artifacts include package, emulator and driver
hashes and are not committed.

Rerun the final frozen package from the configured WSL SDK environment:

```bash
source scripts/env.sh
for video in pal ntsc; do
  python3 tools/bench-native.py \
    --package-dir target/bench/visual-refresh-fast3 \
    --out-dir "target/visual-refresh-validation/recheck-$video" --video "$video" \
    --fixtures lowlands,enemies-crystal,enemies-crystal-full,enemies-ash,enemies-ash-full,bridge-0-200,bridge-1-200,bridge-3-200,bridge-4-200 \
    --samples 3 --ticks 0,10 --palettes cold,retained \
    --real-time-ticks 400 --skip-cache-checks
done
```

Join baseline and candidate cases by fixture, simulation ticks and palette,
then divide baseline `median_cycles` by candidate `median_cycles`. The normal
`--compare` option also requires old native pixels to match, so it is not used
for this visual revision. Cache replay is a separate check from this matrix.

The final PAL cache replay passes **449 cached-versus-forced-miss pairs**
(898 rendered frames), covering all twelve stages, all eleven crossings,
camera movement, occlusion, HUD exclusions, title changes, and victory.
Each pair retains identical projected geometry, decoded state, native display
bytes, and clear color; the package hashes remain unchanged. Its report is
`target/visual-refresh-validation/cache-pal/report.json`. Reproduce it with:

```bash
python3 tools/bench-native.py \
  --package-dir target/bench/visual-refresh-fast3 \
  --out-dir target/visual-refresh-validation/recheck-cache-pal --video pal \
  --cache-checks-only
```

## Campaign expansion final: 2026-10-18

The twelve-stage campaign passes the five-percent performance regression gate
against the preserved four-stage enemy-motion package. The final instrumented
two-SID package is `target/bench/campaign-fast11`; the baseline is
`target/enemy-motion/bench-2sid`. All 32 fixed cases pass, with three initial CIA
timer phases per case: 96 candidate samples across PAL and NTSC. Every compared
case retains exact projected geometry and all 10,000 native bitmap, screen and
color bytes. The package hashes remain unchanged throughout measurement.

This is a comparison of corresponding scenes, not identical campaign state.
Lowlands, Canyon and Fortress move from zero-based stage IDs 0, 1 and 2 to
0, 2 and 6. The missile fixture uses the corresponding Canyon stage. The fixture
driver keeps the actual stage metadata, model bank and simulation profile, and
aligns the terrain's 1,024-tick shape/scroll period by adjusting the local scene
clock. It freezes the encounter cursor and requests exactly zero or ten
simulation ticks. Retained-palette samples first warm both VIC banks with the
same post-simulation scene.

Cross-campaign comparisons do not require equal `Game` hashes because stage
identity, metadata and scene clocks differ. Repeated samples within each build
must still produce identical decoded state. Geometry comparison preserves
triangle order, coordinates, colors and depths; only renderer-private reserved
bits `0x66` are masked (sky/orientation and terrain-visibility classification).
Raw geometry hashes are also recorded. Requiring identical complete native
pixels separately prevents this metadata mask from hiding a display change.

The table gives median whole-frame milliseconds, **motion baseline -> campaign**.
Each row contributes one PAL and one NTSC case.

| Scene | Simulation ticks | Palette | PAL ms | NTSC ms |
| --- | ---: | --- | ---: | ---: |
| Lowlands | 0 | Cold | 160.182 -> 160.175 | 152.990 -> 152.349 |
| Lowlands | 0 | Retained | 143.580 -> 142.906 | 150.991 -> 150.988 |
| Lowlands | 10 | Cold | 160.181 -> 160.174 | 167.705 -> 167.692 |
| Lowlands | 10 | Retained | 144.469 -> 144.183 | 150.992 -> 150.987 |
| Canyon | 0 | Cold | 182.896 -> 181.970 | 184.422 -> 184.421 |
| Canyon | 0 | Retained | 182.430 -> 181.494 | 184.423 -> 184.420 |
| Canyon | 10 | Cold | 180.132 -> 180.125 | 184.421 -> 184.415 |
| Canyon | 10 | Retained | 180.132 -> 180.126 | 184.423 -> 184.421 |
| Fortress (`base`) | 0 | Cold | 180.133 -> 180.126 | 169.138 -> 168.324 |
| Fortress (`base`) | 0 | Retained | 160.182 -> 160.174 | 150.993 -> 150.986 |
| Fortress (`base`) | 10 | Cold | 180.129 -> 180.125 | 184.422 -> 184.420 |
| Fortress (`base`) | 10 | Retained | 160.179 -> 160.174 | 153.097 -> 152.598 |
| Missiles | 0 | Cold | 220.034 -> 220.024 | 217.853 -> 217.847 |
| Missiles | 0 | Retained | 220.030 -> 220.026 | 217.853 -> 217.846 |
| Missiles | 10 | Cold | 239.982 -> 239.975 | 234.564 -> 234.559 |
| Missiles | 10 | Retained | 239.982 -> 239.975 | 234.564 -> 234.558 |

The lowest baseline/campaign case ratios are 1.000018x PAL and 1.000005x NTSC;
no measured case median regresses. Most whole-frame times are effectively
unchanged because frame publication waits for a permitted raster window.
Earlier candidates missed that window in three selected cases; the final
candidate clears those waits without changing the comparison workload or gate.

The separate 400-audio-tick opening probe uses physical port-2 held FIRE and
ordinary simulation scheduling. Measuring completed frames after its first
second gives **6.175800 -> 6.179146 FPS PAL** (+0.054%) and
**6.281506 -> 6.326317 FPS NTSC** (+0.713%). These opening samples and the fixed
matrix do not establish an average frame rate for the entire campaign.

Maximum fixed-matrix audio-service lateness is 1.153 ms PAL and 1.139 ms NTSC,
inside the existing 2 ms gate. Measured ordinary-frame DMA is 57,662 bytes for
Lowlands, 57,649 for Fortress, and 57,476 for Canyon and Missiles. Every individual
transfer stays at or below 1,024 bytes. Checked frames retain intact stack
guards, correct code/model banks, zero geometry drops, zero input overflows and
zero music underruns.

The PAL cache replay additionally passes **449 cached-versus-forced-miss pairs**
(898 rendered frames). It covers all twelve stages and ten themes, all eleven
crossings, camera movement, occlusion, HUD exclusions, title changes and victory. Each pair
has identical geometry, decoded state, native display bytes and clear color.
Both VIC banks invalidate on theme changes; crossings and victory retain no
stale cached sky. Maximum audio lateness in this replay is 1.047 ms and its
largest DMA transfer is also 1,024 bytes.

Reproduce the comparisons from the configured WSL SDK environment, preserving
the recorded baseline package and reports:

```bash
source scripts/env.sh
for video in pal ntsc; do
  python3 tools/bench-native.py \
    --package-dir target/bench/campaign-fast11 \
    --out-dir "target/campaign-performance/recheck-$video" --video "$video" \
    --samples 3 --fixtures lowlands,canyon,base,missiles \
    --ticks 0,10 --palettes cold,retained --real-time-ticks 400 \
    --skip-cache-checks \
    --compare "target/campaign-performance/motion-baseline-$video/report.json"
done
python3 tools/bench-native.py \
  --package-dir target/bench/campaign-fast11 \
  --out-dir target/campaign-performance/recheck-pal-cache --video pal \
  --cache-checks-only
```

The recorded matrix and cadence reports are
`target/campaign-performance/fast11-{pal,ntsc}-full/report.json`; the cache report
is `target/campaign-performance/fast11-pal-cache/report.json`. Baseline reports
are `target/campaign-performance/motion-baseline-{pal,ntsc}/report.json`.
Each includes package, driver and emulator hashes, fixtures, per-phase timing,
DMA traces and captures. These generated artifacts are not committed. All
numbers here come from calibrated VICE; physical C64 Ultimate acceptance remains
separate.

## Campaign compiler options: 2026-10-18

These isolated builds use the same frozen `campaign-fast9` source, two SIDs and
the `benchmarks` feature. The baseline uses `opt-level="z"`, Rust fat LTO and one
codegen unit. Its resident headroom is 36 bytes and its largest composed SIM
image is 16,348 bytes. These are experiment figures, not the final shipping
memory snapshot. Build logs, exact flag invocations, hashes and section comparisons
are in [the generated report](../target/lto-comparison/README.txt) and
[comparison.json](../target/lto-comparison/comparison.json); those artifacts are
not committed.

| Change from baseline | Result on this source |
| --- | --- |
| Rust `lto="thin"` | Links; every allocated section has the same size. A few resident helpers reorder, changing call addresses. |
| Rust `lto="off"` | Compiler fails in `core::u128::ilog10`: the MOS backend cannot legalize a 128-bit `G_UDIV`; no linked output. |
| `opt-level="s"` | Link fails: PIXELS exceeds its 12,288-byte limit by 49 bytes. |
| Two codegen units | Links; resident headroom rises to 101 bytes, largest SIM falls to 16,325 bytes. |
| Four codegen units | Links; resident headroom rises to 94 bytes, largest SIM falls to 16,325 bytes. |
| `s` globally, `z` for package `havoc` | Link fails: resident code/data exceed their region by 70 bytes. |
| `-Zinline-mir=no` | Loaded sections and PRG are byte-identical to baseline. |
| `-Zlocation-detail=none` | Loaded sections and PRG are byte-identical to baseline. |
| LLVM inline and inline-hint thresholds set to zero in Rust and the final link | Links; all allocated section sizes remain unchanged. |
| MIR/LLVM inlining disabled, inline attributes removed and `noinline` forced in both LLVM stages | Link fails: resident code/data exceed their region by 44,379 bytes; startup and other regions also overflow. |
| Rust ThinLTO plus explicit LLD `--lto=thin` | Link fails because the input modules lack compatible unified-LTO metadata. |

All successful variants pass the layout and overlay audits. Both codegen-unit
variants also pass all 43 negative overlay fixtures. Size alone does not establish
a runtime winner: code placement can change page-crossing costs and
frame-publication timing even when section sizes match.

The release profile now uses **two codegen units**, retaining `opt-level="z"`
and fat LTO. The isolated CGU2 package was also measured before adoption:
`target/bench/campaign-cgu2` passes all 16 PAL cases against the motion baseline,
while two existing NTSC publication-window failures remain among its 16 cases.
The demanding PAL Lowlands ten-tick retained-palette case improves from 157,805
cycles with one codegen unit to 143,411 with two on the same frozen source.
Its ordinary-play probe measures 6.159691 FPS PAL and 6.295775 FPS NTSC. Reports
are `target/campaign-performance/cgu2-{pal,ntsc}-full/report.json`.
The smaller resident/geometry footprint and this measured PAL improvement
support the choice; the final source subsequently passes all 32 cases as
documented above. The isolated compiler figures in the table remain unchanged
and should not be substituted for the final production memory snapshot.

Rust's LTO selection and the SDK's final link are separate controls. The local
`mos-c64-none` target requires LTO; `mos-common.cfg` defaults to `-flto`, and the
driver probe shows `-flto=full`. The isolated Rust ThinLTO library still contains
LLVM bitcode. Changing Cargo's LTO setting alone therefore does not prove that
the final MOS link uses a ThinLTO backend. The explicit linker experiment above
tests that distinction and records its incompatibility with this toolchain.

ELF file length is not loaded code size. Here the raw Thin ELF is 1,147,352 bytes;
the packer's `llvm-objcopy --dump-section` normalizes its string tables and file
layout to 1,146,376 bytes, matching the packaged fat ELF length. A disposable-copy
check proves every allocated section stays byte-identical through extraction,
and the PRG stays unchanged. Both PRGs are 38,913 bytes, although the fat and Thin
PRGs differ because of the helper reorder. Compare sections and function sizes,
then measure execution.

To reproduce one option without changing source or release packages, freeze a
baseline from the same source and use a fresh target directory for each case:

```bash
source scripts/env.sh
case_target="$PWD/target/compiler-options/cgu2"
(
  cd target-c64
  CARGO_TARGET_DIR="$case_target" \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
  CARGO_PROFILE_RELEASE_LTO=fat \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=2 \
  cargo build --release --features benchmarks
)
elf=$(find "$case_target/mos-c64-none/release/deps" \
  -name 'havoc_c64-*.elf' -print -quit)
python3 tools/check-layout.py "$elf" \
  "$case_target/mos-c64-none/release/havoc-c64"
python3 tools/check-overlays.py "$elf"
python3 tools/profile-size.py \
  --baseline target/bench/campaign-fast9/havoc.elf \
  --current "$elf" --out-dir target/compiler-options/cgu2-profile
```

Change the three `CARGO_PROFILE_RELEASE_*` values for the profile experiments.
For the MIR or location experiment, pass its `RUSTFLAGS` value only to that
isolated build. The threshold experiment uses this complete value:

```text
-Cllvm-args=-inline-threshold=0 -Cllvm-args=-inlinehint-threshold=0 -Clink-arg=-Wl,--mllvm=-inline-threshold=0 -Clink-arg=-Wl,--mllvm=-inlinehint-threshold=0
```

The `llvm-args` options reach Rust's LLVM optimizer; the `link-arg` options reach
LLD's final LLVM optimizer. The older `-Cinline-threshold` is deprecated and has
no effect in this compiler. The forced-no-inline case additionally uses
`-Zinline-mir=no -Zinline-llvm=no`, with `-force-remove-attribute=alwaysinline`,
`-force-remove-attribute=inlinehint`, and `-force-attribute=noinline` passed through
both of those LLVM routes. Full invocations are preserved beside the report.

Cargo optimization overrides apply to packages/crates, not `.rs` files or Rust
modules. For example, with global `opt-level="s"`, this build command keeps the
`havoc` library at `z` while the native executable package remains at `s`:

```bash
cargo --config 'profile.release.package.havoc.opt-level="z"' \
  build --release --features benchmarks
```

Run it inside the same isolated `target-c64` build environment shown above.
`#[inline]` and `#[inline(never)]` affect individual functions; `link_section`
controls placement, not optimization level. A separate crate can give a module
its own package optimization level, but LTO can still transform code across that
boundary. Inspect the final ELF before treating a source-file split as a size
improvement.

## Deterministic matrix

For campaign packages, the default catalog contains 92 fixtures. Eight cover
title, lowlands, canyon, base, carrier, missiles, full pools, and a section
boundary with a deliberate segment-cache miss. Forty-two scenic fixtures cover
elapsed ticks 0, 200 and 399 of all eleven crossings, plus tick 306 for crossings
three through eleven; this also captures the four-meteor overlap where that
decoration is used. Ten-tick samples starting at tick 399 exercise the handoff
back to combat.

Thirty-six enemy fixtures add representative, saturated and deliberately
wrong-bank states for each of the twelve stages. Two additional fixtures cover
the Gorge Warden and Citadel Heart; the Carrier is included in the combat/enemy
fixtures. Four victory fixtures cover reveal entry, the midpoint of the fade,
arrival and the settled medal. Legacy four-stage packages retain ten scenic and
twelve enemy fixtures, with victory fixtures included when the format supports
the medal. The final campaign comparison above deliberately selects four
matching scenes from this larger catalog.

Model bytes and enemy profiles are restored along with segment caches; the
driver verifies all 2,048 model bytes after each measured frame. Fixed-tick
checks detect skipped or duplicated work when SIM returns to resident code to
change its enemy bank. Each fixture runs with exactly zero or ten simulation
ticks and either cold or retained
palette state. Cold palettes clear both screen buffers and the color shadow.
Retained palettes render the identical post-simulation state twice before the
measured frame, priming both VIC banks. Cold setup also invalidates the optional
static-sky cache; retained setup preserves its two warmed banks. By default each
case has nine samples: three CIA timer phases, repeated three times. Live stereo
music remains enabled throughout.

Fixture field offsets and sizes come from the matching ELF's DWARF, including
nested structures and arrays. The driver restores Game, Runtime, audio-engine,
input-queue, timestamp, and frame-animation state before each sample. It supplies
the section bytes directly from that package's REU image. Fixed-work frames begin
at VIC raster line 250. The CIA timer's first deadline is deliberately varied;
subsequent deadlines use the ordinary 50 Hz latch. The emulator's deferred CIA
force-load behavior is handled inside the timed interval, without injecting code
or excluding any part of the normal frame.

Timing starts before the SIM overlay load and ends at `frame_complete`, after
publication. Reports separate overlay-loading and phase-work intervals, pixel
subphases, publication, DMA commands/bytes, and audio service times. Geometry is
captured at `geometry_complete`, before PIXELS can reuse its command storage.
Native captures include the bitmap, screen, and color data, with PNG previews.

The separate real-time pass leaves normal scheduling enabled and holds physical
port-2 FIRE for 400 audio ticks. It reports completed-frame throughput after the
first second and measured audio frequency. Its changing workload is useful for
ordinary play behavior; use the fixed matrix for same-work speedup comparisons.

## Enemy movement patterns: 2026-10-18

All eight flying enemy types now have harmonic, crossing, pursuit, or feinting
paths. Production SIM images grow from 16,793 to 16,809 bytes per
profile; each of the five alternatives is padded to that same length. The
instrumented image is 16,879 bytes. All four builds pass layout, overlay, and
package validation; no actor fields, lookup tables, resident code, or constants
were added. Each level shares its own small sine-scaling helper so constant
division does not expand at every movement axis.

The two-SID instrumented package passes 48 roster cases on each video standard:
representative, saturated, and deliberately incorrect-bank fixtures for all four
levels, each with zero/ten simulation ticks and cold/retained palettes. Three
CIA phases give 144 samples per standard. Geometry, exact tick counts, model
selection, deterministic repetitions, input, music, stack guards, and package
hashes all pass. Every DMA transfer remains at most 1,024 bytes.

| Measurement | PAL | NTSC |
| --- | ---: | ---: |
| Maximum audio-service lateness | 1.1916 ms | 1.1264 ms |
| Ordinary-scheduling probe | 6.1758 FPS | 6.2815 FPS |

Reports are under `target/enemy-motion/bench-{pal,ntsc}/`. These timings describe
the new paths, which change the rendered workload; they are not a same-work
renderer speedup measurement. Both host test profiles pass, including the
full-mission pilot. Movement tests cover trajectory bounds, phase wrapping,
late spawns, pursuit commitment, and preserved firing schedules. Saturated
rendering also exercises actual moving poses at all player corners and near
the camera, with matching direct/staged pixels and no dropped commands.

A separate 40-frame held-FIRE production sample compares the preserved
`target/motion-baseline/2sid` package with the new build. PAL changes from
5.8454 to 6.0574 FPS; NTSC changes from 6.1360 to 6.2319 FPS. Neither sample
shows a throughput regression. These changing encounters do not isolate the
cost of the movement arithmetic. Reports are under
`target/enemy-motion/baseline-{pal,ntsc}` and `smoke-2sid-{pal,ntsc}`.

Both production SID profiles also pass PAL and NTSC smoke checks: six weapon
scenarios, eleven streaming/restart scenarios, and seven attract-mode scenarios
plus title setup per run. All sampled telemetry has zero dropped geometry,
input overflows, and music underruns, with package hashes unchanged. The
four-SID reports are `target/enemy-motion/smoke-4sid-{pal,ntsc}/report.json`.
These remain emulator checks; physical C64 Ultimate acceptance is outstanding.

## Gold-medal ending: 2026-10-17

The pre-medal production packages were preserved under `target/medal-baseline/`
before implementation. The same 40-frame, held-FIRE opening-game sample compares
those packages with the final two-SID release, using the calibrated emulator:

| Video standard | Before | After | FPS change |
| --- | ---: | ---: | ---: |
| PAL | 5.8628 FPS | 5.8454 FPS | -0.30% |
| NTSC | 6.1360 FPS | 6.1360 FPS | approximately 0% |

Both remain within the five-percent ordinary-play regression budget. These short
samples include the simulation and memory-layout changes needed for the medal;
they do not measure a complete mission or isolate the cost of any one change.
Reports are `target/medal-baseline/vice-{pal,ntsc}/report.json` and
`target/medal-native/2sid-{pal,ntsc}/report.json`.

The instrumented two-SID build passes 20 fixed-work cases per standard: lowlands
and all four victory fixtures, each with zero/ten simulation ticks and
cold/retained palettes. Every case uses three CIA phases. All geometry, input,
music, stack, and bounded-DMA checks pass; maximum audio-service lateness is
1.177 ms PAL / 1.090 ms NTSC, below the 2 ms gate. The separate 400-audio-tick
ordinary-scheduling probe measures 5.8550 FPS PAL / 6.0816 FPS NTSC, with audio
at 50.099 / 50.048 Hz. This is a different timing protocol from the production
comparison above.

Each standard also passes 169 cached-versus-forced-repaint frame comparisons,
including victory entry, fade, arrival, idle, and restart. Game state, projected
geometry, bitmap, screen, and color data match exactly. Victory invalidates the
sky cache and repaints all 25 bands. Reports and captures are under
`target/medal-bench/check-{pal,ntsc}/`; package hashes remain unchanged.

Both production SID profiles pass PAL and NTSC streaming/reveal checks. A fixture
seeds the mission's final tick, then the complete 400-tick reveal advances
naturally with physical joystick input. Checks cover frozen score/resources and
camera, disappearance of the world, continuing victory music, consumed reveal
inputs, and a fresh-fire restart. The two-SID PAL run additionally passes controls,
weapons, checkpoints, and the full attract tour. Reports are under
`target/medal-native/{2sid,4sid}-{pal,ntsc}/`. Both profiles pass 13 negative overlay
audit cases and 29 startup-corruption cases, including both new code images and
rejection of the old directory. Physical C64 Ultimate testing remains outstanding.

## Distinct transitions and effects: 2026-10-17

The final transition build passes 12 fixed-work cases per video standard: the
wildlife and hovercraft crossings at tick 200 and the meteor overlap at tick 306,
each with zero/ten simulation ticks, cold/retained palettes, and three CIA phases
(36 measured samples per standard). Complete-frame medians are about 220 ms PAL
for wildlife and the hovercraft and 200 ms for meteors; NTSC ranges from 202 to
218 ms for wildlife, 218 ms for the hovercraft, and 184 ms for meteors. All geometry,
input, music, stack, and bounded-DMA checks pass. Maximum audio-service lateness
is 1.191 ms PAL / 1.143 ms NTSC, below the 2 ms gate.

The instrumented opening-game probe measures 5.4250 FPS PAL / 5.5819 FPS NTSC,
compared with the preceding enemy build's 5.4892 / 5.6724 (1.17% / 1.60% lower).
A separate 40-frame production check against the preserved pre-change package
measures 5.4950 to 5.4517 FPS PAL and 5.7664 to 5.6440 FPS NTSC (0.79% / 2.12%
lower). Both remain within the five-percent ordinary-play regression budget.
These comparisons include the current shield-pickup magnet and memory-layout
adjustments, so they do not isolate the cost of scenic sound selection.

The final PAL and NTSC cache replays each compare 158 retained-cache frames
against 158 forced repaints, including the meteor peak and both scene seams.
Every pair matches game state, projected geometry, bitmap, screen, and color
bytes. Reports are in `target/bench/transition-sfx-cache-{pal,ntsc}/report.json`.

Reports are in `target/bench/transition-sfx-{pal,ntsc}/report.json` and
`target/vice/transition-final-2sid-{pal,ntsc}/report.json`; preserved production
reports are in `target/vice/transition-baseline-{pal,ntsc}/report.json`.
The transition fixtures deliberately change their decorations, so their images
are checked for determinism and cache correctness rather than equality to the
previous scenes. These are calibrated-emulator measurements, not hardware results.

Stereo WAV captures of all three final-package transitions are under
`target/transition-audio/{pal,ntsc}/`, alongside `recordings.json`. All six
recordings contain both channels, have zero clipped samples, and retain zero
music underruns, dropped geometry, and input overflows. The recordings include
the surrounding rendered-frame boundary; the gameplay transition itself remains
exactly 400 ticks. This is an objective signal check, not subjective listening
or physical SID mixer validation.

## Per-level enemy measurements

The preceding source and packages were preserved under `target/enemies-baseline/`.
The two-SID instrumented build's eight-second ordinary-play probe measured:

| Video | Preserved build FPS | Enemy build FPS | Change |
| --- | ---: | ---: | ---: |
| PAL | 5.4254 | 5.4892 | +1.17% |
| NTSC | 5.6319 | 5.6724 | +0.72% |

This passes the five-percent regression budget for that opening lowland probe.
It does not establish a speedup for the entire mission: later levels deliberately
change their enemy models, movement, and encounter density. Baseline reports are
`target/enemies-baseline/{pal,ntsc}/report.json`; candidate reports are
`target/enemies-bench-{pal,ntsc}/report.json`.

Each candidate matrix covers 22 fixtures with zero/ten ticks and cold/retained
palettes: 88 cases and 264 measured samples per video standard, spanning all
three CIA phases. It includes all four real rosters, saturated scenes, deliberate
model misses, and all three bridge exits. Repeated game, geometry, and pixel
hashes agree. There are no input overflows, dropped triangles, music underruns,
or stack-guard changes. Maximum audio lateness is 1.195 ms PAL / 1.147 ms NTSC,
below the 2 ms gate; every DMA command stays within 1,024 bytes.

These instrumented fixtures transfer 56,589 bytes for an ordinary frame with
score-page service. A deliberate enemy-bank miss adds one 16,746-byte SIM reload
and 2,048 model bytes, totaling 75,383 bytes. A bridge-exit fixture also fetches
a 512-byte segment, totaling 75,895 bytes. Model telemetry records zero bytes
for warm ordinary frames and 2,048 for these swaps. Representative enemy scenes
take roughly 168–220 ms; saturated scenes reach 300 ms PAL / 285 ms NTSC.
These are complete frame times including publication, rather than isolated AI costs.

Focused reruns at `target/enemies-reentry-{pal,ntsc}/report.json` check all four
bank misses and three bridge exits with ten requested ticks. Every frame completes
exactly ten ticks, keeps code/model selection synchronized, and matches the full
2,048-byte model cache to its packaged bank after the reload.

Cache replay reports at `target/enemies-cache-{pal,ntsc}/report.json` compare
157 retained-cache frames with 157 forced-repaint frames per video standard.
Every pair matches game state, projected geometry, bitmap, screen, and color
bytes. The instrumented builds precede a final BOOT-only validation change that
rejects the scenery health sentinel in enemy metadata; gameplay source is identical.

## Scenic-flight measurements

These measurements describe the earlier scenic-flight build.

The scenic build adds a SCENERY transfer and stages raw geometry before the
GEOMETRY pass. Ordinary frames omit the 3,079-byte wildlife tail; scenic frames
omit the 1,423-byte enemy-drawing tail. Overlay transfers use the same bounded
1,024-byte chunks, with address calculations specialized to the fixed code window.
Startup still checksums all six complete overlays.

The original checkout was preserved and built separately under
`target/transitions-baseline/`. The final ordinary-scheduling probe gives:

| Video | Original FPS | Scenic build FPS | Change |
| --- | ---: | ---: | ---: |
| PAL | 5.7285 | 5.4720 | -4.48% |
| NTSC | 5.9262 | 5.6843 | -4.08% |

This meets the five-percent budget for that eight-second ordinary-play probe;
it does not establish the same bound for every fixed fixture or a whole mission.
The lowlands fixed fixtures retain identical projected geometry and native
pixels. Their timing varies with palette state and video-publication boundaries:
for example, PAL zero-tick cold frames increase from 157,808 to 160,705 bus cycles,
while retained-palette frames increase from 138,846 to 157,809 cycles. Later
sections intentionally change world position because completed bridges add to
the scenery clock, so their outputs are not compared as byte-preserving changes.

The baseline reports are `target/transitions-baseline/bench-{pal,ntsc}/report.json`;
the final loader measurements are `target/bench/scenic-dma-{pal,ntsc}/report.json`.
Both final lowlands matrices use three samples per case across the three CIA
phases. The expanded cache replay contains 157 frames, including all three
flights and recovery of both VIC banks at each destination.

## Gates and interpretation

Every sample must retain both stack guards, avoid input overflow and dropped
triangles, service both SID chips, and keep every runtime DMA command at or below
1,024 bytes. Traced audio-service entries must match the audio counter and occur
within 2 ms of their known CIA deadlines. Repeated fixed fixtures must have exact
semantic Game hashes, projected-triangle hashes, and native output hashes.
The geometry hash excludes only static-sky classification and cached-orientation
bookkeeping bits; positions, depth, and color remain exact. Raw command hashes and JSON remain
available for inspection.

Use `--compare` for byte-preserving optimization iterations. It requires a passing baseline with matching video standard, protocol,
and fixture identity. It checks the same three hashes before reporting each
case's median whole-frame speedup. Reports retain all samples and worst times;
a fast raster subphase alone is not evidence of a whole-frame target being met.
Always report PAL and NTSC separately and distinguish zero-tick rendering from
ten-tick simulation-plus-rendering results.

For an intentional raster-precision change, keep the strict comparison available
and diagnose differences separately:

```sh
python3 tools/compare-benchmarks.py \
  target/bench/baseline-static-pal/report.json \
  target/bench/candidate-pal/report.json \
  --out-dir target/bench/candidate-pal-differences
```

This command still exits nonzero for any native mismatch. Its independent old-Q8
and new-Q16 formulas check every visible row of each captured triangle, require
endpoint shifts no greater than one pixel, and build the exact span-difference
mask. `rounding_qualified` requires unchanged game state and projected geometry,
with every changed visible pixel inside that mask. It does not accept arbitrary
nearby image changes. Difference images mark changed pixels, and JSON records
coordinates, colors, geometry discrepancies, and per-case median speedups.

Validate the same captures against both actual renderer implementations as a
separate quality gate:

```sh
python3 tools/check-raster-quality.py \
  target/bench/baseline-static-pal/report.json \
  target/bench/candidate-pal/report.json \
  --out-dir target/bench/candidate-pal-final-quality
```

Repeat with the NTSC reports. The tool compiles the frozen baseline renderer and
the current renderer into isolated host replay programs. It uses the current
sequential `RasterPass` path, including preparation, ordered band masks, and edge
advancement. Every captured triangle is also rendered individually: its actual
old/new row coverage differences must exactly match the independent formula's
span-difference mask, with endpoint movement limited to one pixel. Game state,
projected geometry, and HUD frame serial must match between captures.

The full-color composited scenes identify the exact 4-by-8 pixel cells affected
by changed coverage. Every changed native bitmap, screen, or color cell must
belong to that set. This allows the cell-local palette consequences of a bounded
edge adjustment while rejecting changes in unaffected cells. The command exits
nonzero for any unexplained change and records source hashes, per-triangle
coverage errors, affected cells, and native-cell differences. It complements the
stricter visible-pixel diagnostic and native kernel tests; it does not silently
replace an exact native-hash check.

When the package contains `SKY_CACHE`, the driver also runs a 133-frame transition
history twice, once normally and once forcing cache misses before every frame.
Stable backgrounds, a sky occluder, sector banners, boss HUD, title/playing
transitions, and rapid scene changes must produce identical geometry, game state,
and native bytes in both runs. Appended camera sweeps move player height from
`-65` to `65` and back while scrolling terrain in all four scenes. These assert
that all 14 backdrop triangles stay identical, foreground geometry moves, and
eligible bands resume reuse after both VIC banks repaint. The checks exercise
each invalidation path. Use `--cache-checks-only` to run this replay separately,
or `--skip-cache-checks` only for preliminary timing runs.

For the fixed-background change, compare caching policies within the **same
instrumented package**, retaining its current geometry and audio implementation:

```sh
python3 tools/bench-native.py \
  --package-dir target/bench/candidate-instrumented \
  --out-dir target/bench/cloud-only-pal --video pal \
  --cloud-only-cache --real-time-ticks 0 --skip-cache-checks
python3 tools/bench-native.py \
  --package-dir target/bench/candidate-instrumented \
  --out-dir target/bench/full-background-pal --video pal \
  --real-time-ticks 0 --skip-cache-checks \
  --compare target/bench/cloud-only-pal/report.json
```

Repeat with `--video ntsc` and separate output directories. The cloud-only
reference intersects `SKY_BANDS` with the inverse bounds of the five horizon
triangles at `geometry_complete`, using the native scanner's clamping and band
rounding. It applies that restriction to warmup frames and measured frames.
Monitor writes consume no emulated CPU cycles and do not alter the package,
triangle commands, or target code. This isolates the gain from reusing horizon
bands; both policies pay for the new geometry scan. `cache_policy` records the
selected policy, and `--compare` still requires exact geometry, game state, and
native hashes. The reference skips the independent real-time pass because its
override requires fixed-frame geometry stops. Retained samples report the
additional reused bands and whole-frame timing; canyon terrain can still exclude
every band.

For a quick harness check, use `--samples 3` or `--samples 1`, select fixtures with
`--fixtures lowlands,full-pools`, and disable the independent real-time pass with
`--real-time-ticks 0`. Final comparison runs use the defaults. The JSON report
records package/emulator/driver hashes, arguments, layouts, and fixture data.

Physical hardware validation remains separate: use the same verified PRG/REU/CFG
on the C64 Ultimate with 64 MHz mode, the package's matching REU capacity
(16 MB for this historical build; 1 MB for current builds), and dual SIDs at
`$D400/$D420`.
Record PAL/NTSC, firmware version, frame-rate observations, and audible continuity
under the dense scene. The VICE model does not establish the hardware's exact DMA
or interrupt timing.

## Fixed-background results: 2026-10-17

The cloud-only and full-background policies passed using identical PRG, REU, and
ELF hashes and the current streamed soundtrack. Each policy ran 48 fixed samples
per video standard: four scene fixtures, zero/ten simulation ticks, cold/retained
palettes, and three CIA phases. This is the four-scene comparison, not the full
eight-fixture matrix. Geometry, game state, and native bitmap/screen/color output
matched across policies. Cold samples showed no meaningful timing change.

The following retained-palette medians are **cloud-only → full-background**, in
milliseconds. Reused bands are out of 25 and were identical at zero and ten ticks.

| Video | Scene | Reused bands | Frame, 0 ticks | Frame, 10 ticks |
| --- | --- | ---: | ---: | ---: |
| PAL | Lowlands | 9 → 11 | 143.265 → 140.893 | 143.485 → 141.114 |
| PAL | Canyon | 0 → 0 | 180.123 → 180.123 | 184.210 → 184.210 |
| PAL | Fortress | 9 → 12 | 160.170 → 160.171 | 161.156 → 160.170 |
| PAL | Carrier coast | 9 → 9 | 180.122 → 180.122 | 180.123 → 180.123 |
| NTSC | Lowlands | 9 → 11 | 150.983 → 135.755 | 150.985 → 135.970 |
| NTSC | Canyon | 0 → 0 | 184.418 → 184.412 | 184.418 → 184.412 |
| NTSC | Fortress | 9 → 12 | 150.985 → 150.983 | 167.696 → 150.984 |
| NTSC | Carrier coast | 9 → 9 | 167.695 → 167.696 | 169.731 → 169.731 |

Median pixel-phase work fell by 2.28–2.40 ms in lowlands and 6.09–6.70 ms in the
fortress. Whole-frame savings depend on the allowed raster window for publishing
a frame: PAL fortress at zero ticks saved 6.61 ms of pixel work with effectively
unchanged frame time, while NTSC fortress at ten ticks saved 16.71 ms overall.
The extra reuse therefore reduces rendering work without guaranteeing the same
frame-rate improvement in every scene. Foreground bounds still exclude all canyon
bands, and this carrier fixture gains no additional eligible bands.

The separate 133-frame camera/occluder/HUD/scene-transition replay passed on both
PAL and NTSC. Each cached output matched its forced-miss counterpart exactly
(266 total frames per standard); camera sweeps kept all 14 backdrop triangles
fixed while foreground geometry moved. Audio, DMA, stack, and dropped-geometry
gates passed.

Reports are under `target/bench/static-background-{cloud-only,full}-{pal,ntsc}/`
and `target/bench/static-background-cache-{pal,ntsc}/`. These generated artifacts
are not committed. The measurements isolate cache reuse within the same new
geometry implementation; they do not measure a complete old-build/new-build or
physical-hardware speedup.

## Verified results: 2026-10-17

These earlier renderer results predate the fixed-horizon change and the expanded
133-frame cache replay described above.

The final matrix passed all 288 fixed-work frames per video standard, plus 44
cache A/B replay frames per standard. Both actual-source quality gates passed all
32 cases. Game state and projected geometry remain exact. The 3–17 changed visible
pixels per 32,000-pixel frame are all inside the independently predicted Q8-to-Q16
coverage-difference mask; no triangle row endpoint moves by more than one pixel.
Every native cell change also passed actual-renderer composite replay.

Each table entry is the median of eight scene speedups, including the title;
each scene time is itself the median of nine native samples. The range gives the
slowest and fastest scene speedup in that group. These are whole-frame results,
including overlays, simulation, rendering, HUD, packing, audio, and publication.

| Video | Simulation ticks | Palette state | Median speedup | Scene range |
| --- | ---: | --- | ---: | ---: |
| PAL | 0 | Cold | 1.408× | 1.269–2.065× |
| PAL | 0 | Retained | 1.570× | 1.332–2.065× |
| PAL | 10 | Cold | 1.545× | 1.249–1.724× |
| PAL | 10 | Retained | 1.554× | 1.332–1.727× |
| NTSC | 0 | Cold | 1.465× | 1.299–2.058× |
| NTSC | 0 | Retained | 1.555× | 1.299–2.057× |
| NTSC | 10 | Cold | 1.453× | 1.298–1.714× |
| NTSC | 10 | Retained | 1.549× | 1.299–1.715× |

Across all 32 cases, median speedup is 1.554× PAL and 1.498× NTSC. The independent
ordinary-scheduling probe improves from 3.845 to 5.854 FPS PAL and from 3.998 to
6.011 FPS NTSC. The dense zero-tick full-pools fixture exceeds 2×; the overall
whole-frame 2× target is not met.

Maximum measured audio-service lateness, including the cache replay, is 1.118 ms
PAL and 1.109 ms NTSC. Stable warm sky reuses 10 of 25 bands. Dynamic occlusion,
sector banners, boss HUD, and title transitions invalidate the expected bands;
all 22 transition outputs are byte-identical to forced cache misses.

The instrumented baseline transfers 44,281 bytes in 45 ordinary-frame REU
commands; the candidate transfers 45,682 bytes in 46 commands. A segment-cache
miss adds 512 bytes and one command. Runtime transfers remain at most 1,024 bytes
each. Faster rendering has not reduced total code-overlay traffic in this build.
The shipping build omits benchmark scheduling and transfers 45,537 code bytes per
ordinary frame. Compression was not adopted: shipping SIM (16,520 bytes) and
GEOMETRY (16,882 bytes) exceed the 16,128-byte prerequisite for the reserved decoder
and safety margin.

Reports are under `target/bench/baseline-static-{pal,ntsc}/`,
`target/bench/candidate-{pal,ntsc}/`, and
`target/bench/candidate-{pal,ntsc}-final-quality/`. The compact numeric summary is
`target/bench/final-summary.json`. These generated artifacts are not committed.

## Native triangle preparation port: 2026-10-17

The native Q16 preparation kernel developed in the Kaiju project now replaces
HAVOC's native Rust preparation path. Host builds retain the reference path.
The comparison uses clean commit `f52ea20` plus the same benchmark-only memory
repair on both sides; the candidate adds the preparation kernel and signed slope
helper. This is a new, byte-exact optimization of the existing Q16 renderer, so
the earlier Q8-to-Q16 pixel-difference allowance does not apply.

Both PAL and NTSC passed 120 fixed cases with three samples each: all 30 current
fixtures, zero/ten simulation ticks, cold/retained palettes, and three initial
CIA phases. All **720 before/after sample pairs** have identical game state,
projected geometry including metadata, static background, and native
bitmap/screen/color hashes. Each case also retained its exact seeded state and
CIA phase. No geometry drops, input overflows, music underruns, or stack-guard
changes occurred. The fixture set includes all three bridge exits and the four
enemy profiles with warm, cold, and full pools.

| Measurement | PAL before -> after | NTSC before -> after |
| --- | ---: | ---: |
| Median whole-frame speedup across 120 cases | 1.091x | 1.083x |
| Ordinary scheduling, 400 audio ticks, held FIRE | 5.503 -> 5.872 FPS | 5.644 -> 6.096 FPS |
| Preparation plus first-band work, median excluding title | 26.40 -> 14.16 ms | 25.42 -> 13.74 ms |
| Full pools, zero ticks, retained palettes, whole frame | 282.82 -> 259.94 ms | 284.72 -> 251.29 ms |

Whole-frame speedups are medians of the 120 per-case ratios; every case time is
the median of its three samples. The separate ordinary-scheduling probe improves
6.7% PAL and 8.0% NTSC. It lets the real scheduler determine simulation work,
whereas the fixed matrix holds that work constant. Preparation timing is the
existing first raster marker through the first HUD marker: it includes triangle
setup and advancement through the opaque, unpainted first band. Title is excluded
because it paints that band. This component improves approximately 1.86x PAL and
1.87x NTSC. It is not an isolated assembly instruction count.

Frame publication still waits for a permitted raster window. For example, NTSC
Lowlands at zero ticks with retained palettes reduces preparation/first-band work
from 16.30 to 8.91 ms while whole-frame time remains approximately 151.00 ms.
The preparation gain therefore does not translate into the same frame-rate gain
in every fixture. The slowest per-case ratios are within four microseconds of
unchanged timing; no material whole-frame regression was measured.

The instrumented PIXELS image shrinks from 12,196 to 10,890 bytes, and ordinary
frame DMA falls from 58,330 to 57,015 bytes. Runtime transfers remain bounded to
1,024 bytes each. Maximum measured audio-service lateness is 1.195 ms PAL and
1.150 ms NTSC, within the existing 2 ms gate. Both two-SID and four-SID benchmark
packages pass the linker, memory, overlay-reference, switch-table, IRQ, and
startup audits. No audit exemption was added.

The old benchmark build no longer fit the current SIM allocation before the
port. Its test-only deadline calculation now runs once in resident `main` before
SIM reentry, preserving the same frozen deadline across an enemy-bank change.
Benchmark-only validation placement and checkpoints use existing resident space;
the checkpoints have a separate executable `.benchmark_resident` section in the
already loaded and checksummed `$7800-$7FFF` region. Keeping that section separate
preserves non-executable jump-table ownership checks. The four-SID build places
its 37-byte frame-start checkpoint in ordinary resident RAM to fit its larger
constant table. Shipping builds omit these benchmark changes. Both shipping
PRG/REU pairs were rebuilt from the final source and remained byte-identical to
the packages used by the completed smoke tests.

The production two-SID PAL and four-SID NTSC smoke tests passed exact host title
bytes, physical controls and weapons, all three streamed section crossings,
carrier, checkpoints, and result restarts. Their short eight-frame timing samples
were 5.797 FPS PAL and 5.909 FPS NTSC; those samples are separate from the fixed
matrix and the 400-tick ordinary-scheduling results above. The native preparation
suite additionally passed 17,728 exact head/tail comparisons and 1,024 ABI-canary
calls, including actual in-place replacement, unaligned buffers, and guards.

Separate candidate cache replays passed all 158 cached-versus-forced-miss pairs
per standard (316 rendered frames each). Every pair had identical native output,
with the dynamic-occlusion, HUD, camera, theme, and transition invalidation checks
passing. Their reports are
`target/bench/native-prep-cache-{pal,ntsc}/report.json`.

The fixed matrix used `--samples 3 --fixtures all --ticks 0,10 --palettes
cold,retained --real-time-ticks 400 --skip-cache-checks`, with the calibrated
`x64sc-64x` executable and matching ROM directory recorded in each report.
Cache replays used `--cache-checks-only --real-time-ticks 0`. This is calibrated
emulator evidence; physical C64 Ultimate timing remains a separate acceptance check.

Candidate reports are `target/bench/native-prep-after-{pal,ntsc}/report.json`;
clean-source baseline reports are copied to
`target/bench/native-prep-before-{pal,ntsc}/report.json`. The complete baseline
source, package, captures, and monitor traces remain in the neighboring Kaiju
workspace at `target/sueme64-prep-baseline`. The all-sample comparison and component
timings are summarized in `target/bench/native-prep-summary.json`, and shipping
hash identity is recorded in `target/bench/native-prep-shipping-verification.json`.
Shipping smoke reports are `target/vice/native-prep-2sid-pal/report.json` and
`target/vice/native-prep-4sid-ntsc/report.json`. Generated reports are not committed.


## Terrain visibility correction: 2026-10-18

Near-clipped terrain can have a smaller average-depth sort key than a ship or
ground enemy that is actually in front of it. The lower-right ship reproduction
also occurs in the frozen pre-port renderer; the preparation port did not cause
it. The correction protects player hull/exhaust, Lowlands tracked guns and
Fortress bunkers from exterior terrain while retaining ordinary foreground
ordering. Canyon wall batteries are unchanged. See the geometric bounds and
coverage-bit lifetime in [hardware.md](hardware.md#terrain-visibility-coverage).

This comparison freezes the complete working source before the correction,
including its existing medal/OVL5 changes. It is not a comparison against HEAD.
Both standards passed 136 cases with three samples each: 816 before/after sample
pairs retain identical game states and projected geometry after masking the new
visibility metadata. Expected display differences repair terrain overlap.

| Measurement | PAL before -> after | NTSC before -> after |
| --- | ---: | ---: |
| Ordinary scheduling, 400 audio ticks, held FIRE | 5.855 -> 5.872 FPS | 6.082 -> 6.082 FPS |
| Change in mean fixed-case frame time | -0.004% | +0.048% |
| Median per-case frame-time change | 0.000% | 0.000% |
| Largest individual frame-time increase | +0.571% | +6.526% |

The dense NTSC Lowlands case at ten ticks with cold palettes is the only case
above the planned 5% per-case limit. Its median increases from 225,200 to 239,897
bus cycles because the changed overlay transfer schedule misses a publication
window. In the middle sample, PIXELS work decreases from 87,585 to 87,372 cycles,
while publication wait rises from 189 to 14,706 cycles. Average frame cost and
ordinary throughput therefore do not imply every fixed case meets that limit.
The constant-color fill also omits a redundant comparison: STA preserves DEY's
zero flag. Coverage-preserving and terrain-skipping loops keep their comparison.

Both SID profiles pass 224 host tests, with 101 no-default-feature library tests
also passing (one intentionally ignored fixture exporter in each suite).
Visibility regressions include 9,800 ship/terrain comparisons and 15,360 ground
enemy/terrain comparisons, actual mesh/proxy bounds, roll extremes, spawn timing,
transitions and exclusion of Canyon batteries and unrelated geometry.
Native checks pass 17,728 exact preparations, 1,024 preparation ABI calls,
25,600 raster-band calls, 48 ordered coverage cases, 256 sky-mask cases and 168
packer bands including coverage/HUD mixtures. The final constant-color loop has
been checked against the independent native raster reference.

Seven targeted scenes captured before and after on both standards pass all 28
independent Q16/coverage-oracle comparisons, each checking all 10,000 native
display bytes. These include the lower-right ship, a tracked gun, mirrored near
and far bunkers, and a Canyon wall battery. The battery's entire display is
byte-identical before and after on both standards. Each standard also passes all
169 cached-versus-forced-miss pairs. Shipping two/four-SID PAL/NTSC smoke tests
pass exact title output, controls, weapons, streaming and result restarts.

Both shipping profiles pass the memory, overlay-reference, IRQ and package
audits. GEOMETRY uses 16,690 of 16,896 bytes; PIXELS 11,713 of 12,288; SCENERY
16,880 of 16,896; resident tables 1,691 of 1,792. Resident headroom is 121 bytes
for two SIDs and 109 for four. There are no added triangles or framebuffers.

The baseline, final instrumented packages, captures, matrix/cache/smoke reports
and comparisons are in the neighboring Kaiju workspace under
`target/terrain-visibility/{before,after-ground}/`,
`target/terrain-visibility/final-comparison.json` and
`target/terrain-visibility/native-oracle.json`. These generated files are not
committed. Timing is measured in calibrated VICE; physical C64 Ultimate
acceptance remains separate.

## Off-planet campaign expansion: 2026-10-20

The campaign now has eighteen stages, including atmospheric ascent after the
original finale and six space missions. The original terrestrial workload is
checked against the frozen twelve-stage package at
`target/bench/visual-refresh-fast3`. The selected instrumented build is
`target/offplanet-fast9/2sid`; shipping packages are separately frozen at
`target/offplanet-release/{2sid,4sid}`. These are distinct binaries, and the
checks below identify which package they exercised.

The matched matrix passes all 96 cases: twelve fixtures, zero/ten simulation
ticks, cold/retained palettes and both video standards. Each case uses three CIA
phases, giving 288 before/after sample pairs. Ordered projected geometry and all
native display bytes match the baseline. Stage identity fields intentionally
differ because the campaign clock is now local to each stage. No case exceeds
the 5% median-FPS regression limit.

| Measurement | PAL | NTSC |
| --- | ---: | ---: |
| Matched fixed cases passed | 48/48 | 48/48 |
| Smallest median-FPS ratio, expanded / baseline | 0.999959x | 0.999955x |
| Change in mean fixed-case frame time | -0.451% | -0.228% |
| Median per-case frame-time change | +0.0020% | +0.0019% |
| Ordinary scheduling, baseline -> expanded | 6.248 -> 6.197 FPS | 6.493 -> 6.349 FPS |
| Largest measured audio lateness | 1.173 ms | 1.144 ms |
| Input queue overflows / music underruns | 0 / 0 | 0 / 0 |

The smallest PAL ratio occurs at `carrier/10/cold`; the NTSC minimum is
`section-boundary/0/retained`. Ordinary scheduling is a separate 400-audio-tick
held-FIRE run, excluding the first second. Reports are
`target/offplanet-validation/bench-fast9-{pal,ntsc}/report.json`; references are
`target/offplanet-validation/baseline-{pal,ntsc}/report.json`. The exact fixture
list and invocation are recorded in each report. The selected benchmark ELF
SHA-256 is
`fa3135cc9af070203f4d6f392179aa7b18d53736bd9afd865ab92d8d53138f71`.

Three transfer reductions keep the expanded campaign within that timing gate.
Terrestrial frames load the 10,726-byte SCENERY prefix, omitting 2,865 bytes of
space helpers. Terrestrial crossings then DMA only their selected decoration
tail to its original fixed address. Space frames retain the complete composed
image. Boot validates all 21 music descriptors and caches their 168 bytes in
BSS; ongoing audio service no longer transfers and parses the directory.
Finally, ongoing AUDIO transfers omit the startup-only validation tail:
1,117 of 3,078 bytes are loaded for two SIDs, or 1,127 of 3,109 for four.
Startup still verifies the complete code image and music bank. Input capture
and the simulation clock begin only after music validation completes, avoiding
boot-time input overflow.

Both shipping profiles pass the positive memory, overlay, IRQ and package
audits. They also pass 66 negative layout fixtures and 55 negative overlay
fixtures each, with the original packages unchanged. These cover malformed
transfer lengths, a music cache incorrectly placed inside IRQ-saved memory,
calls into omitted AUDIO startup code, and terrestrial references into omitted
space helpers. The sole reviewed AUDIO prefix-to-tail call is the initial
`service_audio` call to `validate_music_bank`, guarded by `MUSIC_VALIDATED`.
Logs are `target/offplanet-validation/layout-negative-release-{2sid,4sid}.log`
and `overlay-negative-release-{2sid,4sid}.log`.
Each shipping profile also rejects all 125 private corrupt REU packages with
the expected native error; reports are
`target/offplanet-validation/release-corrupt-{2sid,4sid}/summary.json`.

In the shipping ELFs the cached directory occupies `$0212-$02B9`, outside
the 524-byte IRQ-saved `.noinit` region at `$04B4-$06BF`. Resident headroom is
139 bytes for two SIDs and 127 for four. GEOMETRY uses 16,680 of 16,896 bytes;
PIXELS uses 12,234 of 12,288; the largest composed SCENERY image uses 16,709 of
16,896; resident tables use 1,777 of 1,792. OVL7 verifies all 24 code images.
Shipping ELF SHA-256 values are:

| Profile | SHA-256 |
| --- | --- |
| Two SIDs | `8148e16303cea1df0de24eb6df3f09c001097bcd57adf076209c4f4f408f5083` |
| Four SIDs | `c16ba40e17c075170900acbd4c13e980c2a91684289af06ab13cc4bf5e99f3ce` |

All four shipping-profile/video combinations pass the final native smoke run.
Each checks four control, six weapon, 43 streaming and seven attract scenarios,
including all seventeen crossings, eighteen respawns, five bosses and victory
restarts. Each also plays complete loops plus one page for the eight new cues
(13-20), with full page coverage, clock and pitch audits, intact stack guards
and no music underruns. Reports are
`target/offplanet-validation/release-smoke-{2sid,4sid}-{pal,ntsc}/report.json`.
Their PRG, ELF and REU hashes match the frozen shipping packages, and all four
reports confirm that those files remained unchanged.

Broader campaign evidence was collected on earlier frozen expansion packages
and is retained with its own provenance. Continuous joystick-only pilots of
all eighteen stages passed on `target/offplanet-final/2sid` under PAL and
`target/offplanet-final/4sid` under NTSC; these pilots do not write game state or
clocks. Every one of the 21 cues completed a full loop plus another page on
both SID profiles and both video standards. Reports are
`final-campaign-2sid-pal`, `final-campaign-4sid-ntsc`,
`final-smoke-2sid-{pal,ntsc}` and `final-music-4sid-{pal,ntsc}`, each beneath
`target/offplanet-validation` with filename `report.json`. These complete
pilots and 21-cue loops were not rerun on the later optimized shipping binaries.

The intermediate `target/bench/offplanet-fast` package separately passed 659
cached-versus-forced-miss pairs under PAL, covering all stages and crossings,
camera/HUD changes and victory. Its report is
`target/offplanet-validation/cache-fast-pal/report.json`. Four-SID stress
coverage on `target/offplanet-fast8/4sid` passed 24 space/ascent fixtures with
three CIA phases under each standard, with no input overflow or music underrun;
reports are `space-fast8-4sid-{pal,ntsc}/report.json` in the same directory.
These checks supplement the selected build's matched matrix; they do not
establish binary identity with it.

All reports record their package hashes. Consolidated hashes and matrix
statistics are in
`target/offplanet-validation/expansion-validation-summary.json`. Generated
packages, reports, captures and logs are not committed. Timing is measured in
calibrated VICE; physical C64 Ultimate acceptance remains separate.

## 1 MiB REU layout (2026-10-21)

The compact `$534D0800` package keeps all models, encounters and score bytes
unchanged while relocating code and music into a 1 MiB image. An isolated
same-source 16 MiB baseline restores the original REU addresses and loader;
both builds retain the same benchmark-only segment-checksum placement.
The copied sources and adaptations are recorded in
`target/reu-1m/baseline-source-manifest.json`.

All eighteen four-SID PAL comparison cases pass with identical decoded game
state, ordered geometry and native pixels. Cases cover title, lowlands,
carrier, section loading, terrestrial and ascent crossings, orbital enemies,
the gate boss and victory, with zero or ten simulation ticks and cold palettes.
Frame-time differences range from -0.209% to +0.177%; the largest increase is
0.183 ms in lowlands with ten ticks. The relocated scenery loader adds 48
transferred code bytes per sampled frame. These calibrated VICE results use
one sample per case and do not establish physical-hardware timing.
`target/reu-1m/performance-summary.json` links the complete baseline and compact
reports. Both production and instrumented sound profiles pass the existing RAM
and overlay limits; no limits were enlarged for the smaller REU.

## Floating sprite HUD (2026-10-21)

These measurements cover the initial HUD artwork, before its decorative
backings were removed and the bomb count changed to yellow/gray rectangles.

The four-corner HUD uses eight persistent sprites while all 25 bitmap bands
render scenery. The baseline was built from the pre-change HEAD in
`target/hud-baseline-source`; the candidate is `target/hud-bench-package`.
Both use the same four-SID configuration. These calibrated VICE measurements
use three samples per fixed fixture, zero simulation ticks, and report median
frame times with retained palettes. The changed bitmap content is intentional.

| Scene | PAL before / after | NTSC before / after |
| --- | ---: | ---: |
| Lowlands | 107.131 / 108.343 ms | 103.244 / 103.546 ms |
| Carrier | 122.565 / 122.564 ms | 118.367 / 115.449 ms |
| Asteroid belt enemies | 107.284 / 108.970 ms | 105.980 / 104.522 ms |

The 400-tick held-fire run measured 7.814 to 7.668 FPS on PAL and 8.145 to
8.039 FPS on NTSC. These runs advance with real time and are not identical
per-frame workloads; the fixed fixtures above isolate matching scene state.
This change provides full-screen scenery and independent HUD colors, without
claiming a frame-rate gain. Physical-hardware timing remains unmeasured.

Native traces measured a maximum protected sprite publication of 75 bus
cycles: 1.190 PAL or 1.154 NTSC scanlines, below the two-line guard budget.
Unchanged fixture repeats produced no sprite publications. Separate PAL/NTSC
smoke checks verified six unchanged frames with zero image writes, a score
change affecting only the top-left pair, and sprite positions matching every
bomb-shake phase, retrigger, and return to rest.

Before reports are `target/hud-before-bench-{pal,ntsc,space-pal,space-ntsc}/report.json`;
after reports are `target/hud-bench-{pal,ntsc,space-pal,space-ntsc}/report.json`.
Each records package hashes, cold and retained samples, and phase timings.
The candidate's cold samples in those reports inherited HUD state from earlier
samples and should not be used for matched-work timing comparisons; the
retained-palette medians above already used a matching warmup.

The current driver warms the sprite HUD with the identical fixture before
every measured frame, then clears bitmap attributes when cold palettes are
requested. Reports record `sprite_hud_setup: matching-fixture-warmup-v1` and
per-frame HUD commit/image-write deltas, which must both remain zero. Cold
baseline reports with the former inherited HUD state are rejected and must be
regenerated with the current driver.
