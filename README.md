# SUEME64

An original polygon rail shooter for the **64 MHz C64 Ultimate**, written in Rust with a small 6502 assembly hardware layer. The visual inspiration is early flat-shaded console shooters; the models, mission, music, and effects are original. No Nintendo ROM or disassembly is required.

The first mission travels across open terrain, a canyon, and a defended base before a carrier boss. It includes enemies, scenery collisions, shields, pickups, explosions, three lives, section checkpoints, and victory/restart screens. This is a working prototype: rendering is still below the intended frame rate, and physical C64 Ultimate testing remains outstanding.

## Run on C64 Ultimate

Build the project, then copy these three matching files from `dist/` to the Ultimate's storage:

- `sueme64.cfg` — 64 MHz, 16 MB REU, two stereo UltiSIDs.
- `sueme64.reu` — assets and executable overlays, exactly 16 MiB.
- `sueme64.prg` — startup and resident runtime.

Import the configuration, load the REU image at address zero, and run the PRG. Use **joystick port 2**. The initial PRG/REU load is required; gameplay performs no disk reads. Mission data and code are transferred from the already loaded REU. Startup checks detect missing, corrupt, or mismatched files.

## Controls

| Input | Action |
|---|---|
| Left / right | Move the ship sideways |
| Up / down | Climb / dive |
| Fire on title/results screen | Start/restart |
| Hold fire | Autofire, 10 shots per second |
| Release fire | After one second, begin charging; full charge at two seconds |
| Press fire with partial charge | Fire a stronger straight shot, then resume autofire |
| Press fire at full charge | Launch an automatic homing missile, then resume autofire |
| Two fire presses within 250 ms | Fire normally and spend one bomb |
| Two left or right presses within 250 ms | Barrel roll and dodge sideways in that direction |

A roll lasts half a second and adds a sideways burst that eases out over the first 300 ms. Steering and firing remain available; countersteering brakes the dodge, and the ship stays at its new position when the roll ends. A roll deflects hostile shots and has a one-second minimum interval between starts. Scenery can still damage the ship. Bomb capacity is three. A death restarts the current section and retains the score; losing all lives ends the run.

After a hit, the ship stays visible while its hull alternates between white and its normal colors for 700 ms. Exhaust keeps its normal color. Startup and respawn protection do not cause flashing.

A fully charged missile deals four damage and automatically steers toward the nearest enemy ahead, including exposed carrier weak points. It can acquire another target during flight. Scenery blocks missiles, and the carrier core stays protected until both turrets are destroyed. Missiles have limited turning speed, so very close targets far to one side can still escape; without a target, the missile continues forward. Partial charges retain their existing one-to-three damage.

The weapon meter reads **LASER CHARGE** until fully charged, then switches to **HOMING MISSILE** with a pulsing label and yellow bar. Firing consumes the charge and returns the label to **LASER CHARGE**.

## Build

Host tools require Rust and Cargo. The hardware build additionally uses Linux or WSL, Python 3, LLVM-MOS SDK **v22.5.0**, and a Rust-MOS toolchain with the `mos-c64-none` target and Rust sources. This workspace was built with `rustc 1.87.0-dev` registered as the `mos` rustup toolchain. That toolchain alias is selected by `target-c64/rust-toolchain.toml`.

From PowerShell with the existing Ubuntu-24.04 toolchain:

```powershell
./scripts/build-windows.ps1
```

Or from Linux/WSL:

```sh
bash scripts/build.sh
```

`scripts/env.sh` defaults to `$HOME/.local/llvm-mos` and `$HOME/.local/rust-mos-targets`. Set `LLVM_MOS`, `MOS_CLANG`, and `RUST_TARGET_PATH` for other installations. The build runs host tests, bakes the assets, links the C64 program, checks all memory allocations, and packages the code overlays into REU. Do not use `bake` alone to produce a runnable REU image: the target build supplies the executable overlays.

Useful host commands:

```sh
cargo test --all-targets
cargo run --release -- sizes
cargo run --release -- snapshot title 0 target/previews/title
cargo run --release -- snapshot 1 5000 target/previews/canyon
python3 tools/previews.py
```

Snapshots produce an aspect-correct PPM and the exact 10,000 native bitmap/screen/color bytes. The preview converter uses only Python's standard library.

Optional native checks run the assembly helpers in the LLVM-MOS 6502 simulator:

```sh
source scripts/env.sh
python3 tools/test-pack-kernel.py
python3 tools/test-raster-kernel.py
python3 tools/test-memcpy-kernel.py
python3 tools/benchmark-division.py
```

They compare the actual CPU output against independent reference results and check buffer guards. The packer covers 128 synthetic and game-image bands; the rasterizer covers 6,400 bands and 8,192,000 pixel bytes; memory copying covers 1,297 cases.

The optional emulator checks use the calibrated `doomc64u/target/audit-fast/x64sc-64x` companion and ROMs from the sibling checkout. Override paths with `--vice` and `--roms` when needed. An unmodified companion may map the turbo setting to 48x, which invalidates a 64x timing comparison.

```sh
cargo +stable run --release -- snapshot title 0 target/previews/title
python3 tools/vice-smoke.py --video pal --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video ntsc --title-reference target/previews/title.bin
python3 tools/test-corrupt-package.py
```

The smoke test checks the exact 10,000-byte native title, physical joystick port 2, all six SID voices, independent 50 Hz audio, frame progress, and dropped-input/triangle counters. Add `--control-checks` to verify up-to-climb/down-to-dive movement and left/right roll dodges through joystick port 2. Add `--weapon-checks` to seed encounters, trigger actual hit flashes, and fire fully charged homing missiles through the physical joystick input; it checks native hull colors, exhaust visibility, off-axis interception, and targetless expiry. Add `--streaming-checks` to seed section boundaries and results screens through the monitor, verify prefetched REU bytes, and exercise restart with real joystick input. These additional scenarios are separate from the ordinary timing sample; they are not a full native mission playthrough. It records emulator/package hashes, timing, native screenshots, and logs under `target/vice`. Corruption tests operate on private package copies under `target`; they leave `dist` intact.

## Implementation

The display is native **160 x 200 multicolor bitmap mode**, with the C64's 16-color palette and normal per-cell restrictions. An eight-line software color band feeds a cell-palette packer; retained color slots reduce palette changes. There is no texture mapping. The renderer performs fixed-point transforms, near-plane clipping, painter sorting, and flat triangle rasterization into two VIC bitmap banks.

Four coarse terrain bands form hills and uneven canyon ridges around a level flight corridor. Shared edge heights stay anchored to world distance, and two flat color tones reveal the triangular facets. Terrain uses 29 triangles in the lowlands and carrier section, or 45 with canyon caps or base guide strips, including distant mountains. The hills are scenic; existing obstacle collisions and ground-object positions remain unchanged.

Two UltiSIDs at **$D400/$D420** provide four music voices and two reserved effects voices. Original synthesized music runs on a 50 Hz CIA interrupt independently of rendering. Effects use SID oscillators, ADSR, and noise; there is no digital audio. The shipped mixer uses moderate left/right separation and matched 8580 settings.

Simulation uses fixed 20 ms steps, while joystick gestures use timestamped display-rate input. Executable phases share a RAM window and are loaded from REU; audio and the interrupt handler remain resident. Detailed memory ownership and configuration references are in [docs/hardware.md](docs/hardware.md).

## Current validation and performance

The 2026-10-17 build passes **85 host tests**, including continuous ship geometry through every flash phase, missile targeting and bounded steering, carrier shielding, projectile-pool saturation, terrain continuity, joystick controls, steerable roll dodges, and a complete five-minute mission driven by ordinary controls. Native PAL and NTSC smoke and weapon checks pass for the exact title image, stereo music/effects, stack guards, visible white/normal hit flashes, homing interception, and targetless missile expiry. Native HUD captures verify **LASER CHARGE** when empty, partially charged, and after firing, plus **HOMING MISSILE** in both ready-color phases, with clear spacing beside BOMBS. The earlier weapon build also passed physical vertical/roll controls, section-cache integrity, rendered-section consistency, and seeded victory/game-over restarts. Weapon and streaming checks use explicit encounter/section fixtures; charging and firing use real elapsed time and physical joystick input. A complete mission has not yet been played through on the target.

| Calibrated 64x VICE | Terrain baseline | Current stateful weapon HUD |
|---|---:|---:|
| PAL | 4.87 FPS | 4.62 FPS |
| NTSC | 5.11 FPS | 4.76 FPS |

These measurements cover seven opening gameplay frames, exclude the title frame, and include REU transfers, interrupts, and presentation. The current opening sample is about 5% slower on PAL and 7% on NTSC than the terrain baseline; audio remains approximately 50 Hz. This sample does not measure a sustained homing-missile encounter. These results **do not meet the 25 FPS PAL / 30 FPS NTSC goals**, and they are not measurements of physical C64 Ultimate hardware or later, busier scenes. Rasterization and cell-palette packing remain expensive; the executable overlays also transfer 44,297 bytes per rendered frame. Rendering performance is the main unfinished goal.

Matching package hashes, raw timings, and HUD captures are in `target/vice/weapon-labels-pal/report.json` and `target/vice/weapon-labels-ntsc/report.json`. Earlier control/streaming checks remain in `target/vice/weapons-pal` and `target/vice/weapons-ntsc`; the terrain baseline remains in `target/vice/terrain-pal` and `target/vice/terrain-ntsc`. That terrain build's four startup-corruption checks are recorded in `target/corrupt-package-terrain/summary.json`. Physical-machine display, audio, and full-mission testing remain outstanding.

Bootstrap ideas came from the neighboring `doomc64u` project, and Ultimate configuration examples from `secondreality`. The `ultrastarfox` disassembly is not copied into this project.
