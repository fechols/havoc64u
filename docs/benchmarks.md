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
  --out-dir target/bench/candidate-pal --video pal \
  --compare target/bench/baseline-static-pal/report.json
python3 tools/bench-native.py \
  --package-dir target/bench/candidate-instrumented \
  --out-dir target/bench/candidate-ntsc --video ntsc \
  --compare target/bench/baseline-static-ntsc/report.json
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
The geometry hash excludes only the cached-orientation bookkeeping bit; positions,
depth, color, and visible flags remain exact. Raw command hashes and JSON remain
available for inspection.

`--compare` requires a passing baseline with matching video standard, protocol,
and fixture identity. It checks the same three hashes before reporting each
case's median whole-frame speedup. Reports retain all samples and worst times;
a fast raster subphase alone is not evidence of a whole-frame target being met.
Always report PAL and NTSC separately and distinguish zero-tick rendering from
ten-tick simulation-plus-rendering results.

For a quick harness check, use `--samples 3` or `--samples 1`, select fixtures with
`--fixtures lowlands,full-pools`, and disable the independent real-time pass with
`--real-time-ticks 0`. Final comparison runs use the defaults. The JSON report
records package/emulator/driver hashes, arguments, layouts, and fixture data.

Physical hardware validation remains separate: use the same verified PRG/REU/CFG
on the C64 Ultimate with 64 MHz mode, 16 MB REU, and dual SIDs at `$D400/$D420`.
Record PAL/NTSC, firmware version, frame-rate observations, and audible continuity
under the dense scene. The VICE model does not establish the hardware's exact DMA
or interrupt timing.
