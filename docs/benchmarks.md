# Native performance benchmarks

`tools/bench-native.py` measures complete frames in the calibrated C64 Ultimate
VICE build used by `tools/vice-smoke.py`. These are emulator measurements, not
claims about physical C64 Ultimate frame rates. The package is read-only during
measurement; its PRG, REU, and ELF hashes are recorded and checked afterward.

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

## Deterministic matrix

The default matrix has eight combat/menu states: title, lowlands, canyon, base,
carrier, missiles, full pools, and a section boundary with a deliberate
segment-cache miss. Nine scenic states cover elapsed ticks 0, 200, and 399 of
each bridge. Ten-tick samples starting at tick 399 also exercise the handoff back
to combat.
Each runs with exactly zero or ten simulation ticks and either cold or retained
palette state. Cold palettes clear both screen buffers and the color shadow.
Retained palettes render the identical post-simulation state twice before the
measured frame, priming both VIC banks. Cold setup also invalidates the optional
static-sky cache; retained setup preserves its two warmed banks. Each case has nine samples: three CIA
timer phases, repeated three times. Live stereo music remains enabled throughout.

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

## Scenic-flight measurements

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
