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

The default matrix has eight named states: title, lowlands, canyon, base, carrier,
missiles, full pools, and a section boundary with a deliberate segment-cache miss.
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

When the package contains `SKY_CACHE`, the driver also runs a 22-frame transition
history twice, once normally and once forcing cache misses before every frame.
Stable sky, a sky occluder, sector banners, boss HUD, and title/playing transitions
must produce identical geometry, game state, and native bytes in both runs.
The checks require actual warm-cache reuse and exercise each invalidation path.
Use `--skip-cache-checks` only for preliminary timing runs.

For a quick harness check, use `--samples 3` or `--samples 1`, select fixtures with
`--fixtures lowlands,full-pools`, and disable the independent real-time pass with
`--real-time-ticks 0`. Final comparison runs use the defaults. The JSON report
records package/emulator/driver hashes, arguments, layouts, and fixture data.

Physical hardware validation remains separate: use the same verified PRG/REU/CFG
on the C64 Ultimate with 64 MHz mode, 16 MB REU, and dual SIDs at `$D400/$D420`.
Record PAL/NTSC, firmware version, frame-rate observations, and audible continuity
under the dense scene. The VICE model does not establish the hardware's exact DMA
or interrupt timing.

## Verified results: 2026-10-17

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
