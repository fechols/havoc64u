# Native performance benchmarks

`tools/bench-native.py` measures complete frames in the calibrated C64 Ultimate
VICE build used by `tools/vice-smoke.py`. These are emulator measurements, not
claims about physical C64 Ultimate frame rates. The package is read-only during
measurement; its PRG, REU, and ELF hashes are recorded and checked afterward.

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
on the C64 Ultimate with 64 MHz mode, 16 MB REU, and dual SIDs at `$D400/$D420`.
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
