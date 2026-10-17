# H.A.V.O.C.

**Heavy Assault Vessel: Orbital Combat**

Your ship is an unreasonable amount of firepower with a cockpit attached!

The title is a tribute to **A.L.C.O.N.**

An original polygon rail shooter for the **64 MHz C64 Ultimate**, written in Rust with a small 6502 assembly hardware layer. The visual inspiration is early flat-shaded console shooters; the models, mission, music, and effects are original. No Nintendo ROM or disassembly is required.

The first mission travels across open terrain, a canyon, and a defended base before a carrier boss. It includes enemies, scenery collisions, shields, pickups, explosions, three lives, section checkpoints, and victory/restart screens. This is a working prototype: rendering is still below the intended frame rate, and physical C64 Ultimate testing remains outstanding.

## Run on C64 Ultimate

The build produces two packages: `dist/` for two UltiSIDs, and `dist/4sid/` for two UltiSIDs plus two physical ARMSIDs. Copy these three matching files from the chosen folder to the Ultimate's storage:

- `havoc.cfg` — 64 MHz, 16 MB REU, and the selected SID configuration.
- `havoc.reu` — assets and executable overlays, exactly 16 MiB.
- `havoc.prg` — startup and resident runtime.

Import the configuration, load the REU image at address zero, and run the PRG. Use **joystick port 2**. The initial PRG/REU load is required; gameplay performs no disk reads. Mission data and code are transferred from the already loaded REU. Startup checks detect missing, corrupt, or mismatched files.

The four-SID package adds six music voices through the physical sockets at `$D440/$D460`; the core score and effects stay on the UltiSIDs at `$D400/$D420`. Configure the ARMSIDs for 8580 behavior separately. Keep each package's PRG, REU, and configuration together; there is no in-game sound-profile switch. See [SID configuration](docs/hardware.md#sid-configurations) for the mixer settings.

## Controls

| Input | Action |
|---|---|
| Left / right | Move the ship sideways |
| Up / down | Climb / dive |
| Fire on title/results screen | Start/restart |
| Any direction or fire during attract mode | Return to title; press fire again to start |
| Hold fire | Autofire, 10 shots per second |
| Release fire | After one second, begin charging; full charge at two seconds |
| Press fire with partial charge | Fire a stronger straight shot, then resume autofire |
| Press fire at full charge | Launch an automatic homing missile, then resume autofire |
| Two fire presses within 500 ms | Fire normally and spend one bomb |
| Two left or right presses within 500 ms | Barrel roll and dodge sideways in that direction |

Leave the controller's hardware autofire disabled. The game supplies its own autofire; controller-generated fire pulses can activate the double-tap bomb gesture.

After 15 seconds without joystick activity on the title screen, attract mode previews **Lowland Approach**, **Iron Canyon**, **Fortress Run**, and **Carrier Assault** for 15 seconds each. These are live opening scenes at normal game speed, with enemies, scenery, and music, but no player ship or cockpit HUD. Enemies still fire, but all sound effects are silenced during attract mode so the soundtrack can be heard clearly. The section name and **MOVE OR FIRE FOR MENU** remain visible. After all four previews, the title returns for another 15 idle seconds before the cycle repeats. Moving the joystick or pressing fire dismisses a preview; release and press fire again to launch a fresh game. Holding a direction on the title prevents the idle preview from starting. Results screens do not enter attract mode.

A roll lasts half a second and adds a sideways burst that eases out over the first 300 ms. Steering and firing remain available; countersteering brakes the dodge, and the ship stays at its new position when the roll ends. A roll deflects hostile shots and has a one-second minimum interval between starts. Scenery can still damage the ship. Bomb capacity is three. A death restarts the current section and retains the score; losing all lives ends the run.

After a hit, the ship stays visible while its hull alternates between white and its normal colors for 700 ms. Exhaust keeps its normal color. Startup and respawn protection do not cause flashing.

A fully charged missile deals four damage and automatically steers toward the nearest enemy ahead, including exposed carrier weak points. It can acquire another target during flight. Scenery blocks missiles, and the carrier core stays protected until both turrets are destroyed. Missiles have limited turning speed, so very close targets far to one side can still escape; without a target, the missile continues forward. Partial charges retain their existing one-to-three damage.

The weapon meter reads **LASER CHARGE** until fully charged, then switches to **HOMING MISSILE** with a pulsing label and yellow bar. Firing consumes the charge and returns the label to **LASER CHARGE**.

## Original soundtrack

Seven melodic cues share the flight theme's rising E–G–B motif, with contrasting middle sections, longer held melodies, and a major-key homecoming. Each area has its own arrangement and begins with its hook so the 15-second attract scenes introduce the music immediately.

| Scene | Track | Loop |
|---|---|---:|
| Title | Wings Against the Dawn | 61.44 s |
| Lowland Approach | The Wide Horizon | 61.44 s |
| Iron Canyon | Stone and Thunder | 64.00 s |
| Fortress Run | Citadel in Flames | 64.00 s |
| Carrier Assault | Against the Iron Sky | 76.80 s |
| Victory | Homeward, at Last | 53.76 s |
| Defeat | Lost Wing | 46.08 s |

The two-SID version supplies bass, lead, inner harmony, and arpeggios, plus two reserved effects voices. The four-SID version preserves those four musical parts and adds two string lines, a tenor counterline, a flute/bell answer, and two restrained rhythmic accents. *Lost Wing* retains every original core note, instrument, and breath; its enhanced arrangement adds gentle colour without percussion. The score uses SID synthesis throughout, with no samples.

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

`scripts/env.sh` defaults to `$HOME/.local/llvm-mos` and `$HOME/.local/rust-mos-targets`. Set `LLVM_MOS`, `MOS_CLANG`, and `RUST_TARGET_PATH` for other installations. The build tests and packages both profiles, writing the two-SID package to `dist/` and the four-SID package to `dist/4sid/`. It bakes the assets, links each C64 program, checks memory allocations, and packages the code overlays into REU. Do not use `bake` alone to produce a runnable REU image: the target build supplies the executable overlays.

Useful host commands:

```sh
cargo test --all-targets
cargo test --all-targets --features four-sid
cargo run --release -- sizes
cargo run --release --features four-sid -- sizes
cargo run --release -- snapshot title 0 target/previews/title
cargo run --release -- snapshot 1 5000 target/previews/canyon
cargo run --release -- snapshot attract 35000 target/previews/attract-canyon
python3 tools/previews.py
```

Snapshots produce an aspect-correct PPM and the exact 10,000 native bitmap/screen/color bytes. The preview converter uses only Python's standard library.

`snapshot attract` takes elapsed milliseconds from a fresh title screen and replays the actual idle/preview simulation. For example, `35000` captures five seconds into Iron Canyon; `75000` captures the automatic return to the title.

Optional native checks run the assembly helpers in the LLVM-MOS 6502 simulator:

```sh
source scripts/env.sh
python3 tools/test-pack-kernel.py
python3 tools/test-raster-kernel.py
python3 tools/test-sort-kernel.py
python3 tools/test-memcpy-kernel.py
python3 tools/benchmark-division.py
```

They compare the actual CPU output against independent reference results and check buffer guards. The packer covers 168 synthetic and game-image bands; the rasterizer covers 25,600 band calls, including hidden bands and unaligned state; the stable sorter covers 108 sorts up to all 256 commands; memory copying covers 1,297 cases.

The optional emulator checks use the calibrated `doomc64u/target/audit-fast/x64sc-64x` companion and ROMs from the sibling checkout. Override paths with `--vice` and `--roms` when needed. An unmodified companion may map the turbo setting to 48x, which invalidates a 64x timing comparison.

```sh
cargo +stable run --release -- snapshot title 0 target/previews/title
python3 tools/vice-smoke.py --video pal --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video ntsc --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video pal --attract-checks --out-dir target/vice/attract-pal
python3 tools/vice-smoke.py --video ntsc --attract-checks --out-dir target/vice/attract-ntsc
python3 tools/vice-smoke.py --video pal --music-checks --out-dir target/vice/music-pal
python3 tools/vice-smoke.py --video pal --package-dir dist/4sid --music-checks --out-dir target/vice/music-4sid-pal
python3 tools/test-corrupt-package.py
```

The smoke test checks the exact 10,000-byte native title, physical joystick port 2, SID music/effects activity, independent 50 Hz audio, frame progress, and dropped-input/triangle counters. It reads the SID profile from the package; use `--package-dir dist/4sid` for the enhanced build. Add `--control-checks` to verify up-to-climb/down-to-dive movement and left/right roll dodges through joystick port 2. Add `--weapon-checks` to seed encounters, trigger actual hit flashes, and fire fully charged homing missiles through the physical joystick input; it checks native hull colors, exhaust visibility, off-axis interception, and targetless expiry. Add `--streaming-checks` to seed section boundaries and results screens through the monitor, verify prefetched REU bytes, and exercise restart with real joystick input. These additional scenarios are separate from the ordinary timing sample; they are not a full native mission playthrough. It records emulator/package hashes, timing, native screenshots, and logs under `target/vice`. Corruption tests operate on private package copies under `target`; they leave `dist` intact.

`--music-checks` plays a full loop plus a page of each of the seven cues while rendering continues. Monitor fixtures hold each scene, and Lowland uses physical held fire to exercise both effects voices. It checks page order, loop timing, active music lanes, and underruns. The two-SID run also checks that no writes reach the additional SID addresses.

`--attract-checks` observes a full 90-second title/map/title/repeat sequence, counting actual native simulation calls to verify each 750-tick interval. It checks segment-cache bytes, rendered sections, player resources, and stack/input/geometry guards, and captures every map. It also exercises dismissal with every joystick direction and fire, held-input consumption, and fresh-fire launch. Monitor writes establish the initial timer baseline and per-map input fixtures; the complete tour advances naturally and all dismissal/launch input uses physical joystick port 2.

## Implementation

The display is native **160 x 200 multicolor bitmap mode**, with the C64's 16-color palette and normal per-cell restrictions. An eight-line software color band feeds a cell-palette packer; retained color slots reduce palette changes. There is no texture mapping. The renderer performs fixed-point transforms, near-plane clipping, stable radix depth sorting, and flat triangle rasterization into two VIC bitmap banks. Persistent Q16 edges advance through the bands in a small assembly kernel. The packer writes directly to the hidden bitmap and screen, staging only color RAM.

Four coarse terrain bands form hills and uneven canyon ridges around a level flight corridor. Shared edge heights stay anchored to world distance, and two flat color tones reveal the triangular facets. Each section has its own sky, cloud arrangement, and horizon: blue-gray lowland ridges, warm canyon mesas, an overcast industrial fortress, and a purple dusk coast for the carrier. The horizon bases extend below the far terrain to prevent sky gaps as the ground scrolls. Three distant clouds add nine triangles and stay fixed in screen space within each section. Unobstructed sky bands reuse the two existing bitmap buffers when both contain the same static sky and palette; section changes invalidate both banks, and moving geometry or HUD elements invalidate affected bands. Terrain and sky still use 38 triangles in the lowlands and carrier section, or 54 with canyon caps or base guide strips. The hills are scenic; existing obstacle collisions and ground-object positions remain unchanged.

Original synthesized music runs on a 50 Hz CIA interrupt independently of rendering. A foreground AUDIO overlay prefetches one-bar score pages from REU; the resident player handles notes, ties, legato, and effects without performing REU transfers. The two builds provide four or ten music voices and retain the same two reserved effects voices. Their complete score banks occupy 22,400 and 52,096 REU bytes respectively. Mixer settings use moderate left/right separation; the extra physical SID pair starts at −6 dB.

Simulation uses fixed 20 ms steps, while joystick gestures use timestamped display-rate input. Executable phases share a RAM window and are loaded from REU; audio and the interrupt handler remain resident. Detailed memory ownership and configuration references are in [docs/hardware.md](docs/hardware.md).

## Current validation and performance

The OST build passes **140 host tests per SID profile**, with one existing native-fixture export ignored. Six composition/playback tests cover the complete authored suite twice on PAL and NTSC clocks, with page service limited to five frames per second and effects continuing throughout. Additional checks preserve the complete *Lost Wing* transcription, verify cue selection, and reject damaged score data.

Both final packages pass all seven full cue loops plus continued refills in calibrated PAL and NTSC VICE, with zero score underruns, skipped pages, stack damage, or dropped input. Music stays at 50 Hz while rendering and effects continue. Both NTSC profiles also pass physical joystick controls, weapons, section streaming, result-screen restarts, and the complete attract tour. All seven startup-corruption cases pass for each profile. Reports are in `target/vice/havoc-ost-final-{2sid-pal,4sid-pal,4sid-ntsc}`, `target/vice/havoc-ost-2sid-ntsc`, and `target/corrupt-package-ost-final-{2sid,4sid}`; each records the unchanged input-package hashes.

The two/four-SID resident players occupy 2,699/2,707 code bytes, with 273/579 bytes for engine state and both score pages. Foreground audio loading shares the existing overlay window. Both production builds and the four-SID benchmark build pass memory and overlay audits. See [the memory accounting](docs/hardware.md#paged-melodic-score) for scope and remaining headroom. Physical-machine listening, final ARMSID mixer balance, and a complete native mission remain outstanding.

The earlier scene-background build passed **126 host tests**, including 16,384 terrain/camera coverage cases with no sky slit below the horizons. Native pack/raster checks passed, and PAL/NTSC emulator checks passed the exact title image, all section transitions, results/restarts, and the full attract tour. On both video standards, 53 cache-transition frames matched forced repainting byte for byte; canyon walls conservatively prevented sky reuse, while unobstructed bands in the other themes reused normally. Captures and reports are under `target/background-previews`, `target/vice/backgrounds-{pal,ntsc}`, and `target/bench/backgrounds-cache-{pal,ntsc}`. That build retained four overlays and passed layout/overlay audits, with 278 resident bytes and one geometry-overlay byte remaining.

The attract-mode build passes **122 host tests**, with one existing native-fixture export ignored. PAL and NTSC emulator checks pass the complete 90-second title/map/title/repeat sequence, exact 750-tick intervals, all joystick dismissal inputs, held-input consumption, and fresh-fire launch. Both standards also pass the existing title-reference, control, and section-streaming checks. Reports and captures are under `target/vice/attract-pal`, `target/vice/attract-ntsc`, and their `-regression` counterparts. The build retains four code overlays and passes the memory-layout and cross-overlay audits; the game state occupies 584 of its 640 reserved bytes.

The optimized renderer passes **111 host tests**, including shared-edge coverage, independent raster arithmetic, static clouds, palette reuse, gameplay, and a complete five-minute host mission. Native PAL and NTSC checks pass the exact title, stereo music/effects, stack guards, physical vertical/roll controls, visible white damage flashes, homing interception, targetless missile expiry, section streaming, and seeded victory/game-over restarts. Weapon and streaming checks use explicit encounter/section fixtures with physical joystick input; a complete mission has not yet been played through on the target.

| Calibrated 64x VICE, ordinary held-fire gameplay | Same-content baseline | Optimized renderer | Speedup |
|---|---:|---:|---:|
| PAL | 3.845 FPS | 5.854 FPS | 1.52x |
| NTSC | 3.998 FPS | 6.011 FPS | 1.50x |

These ordinary-play samples span 400 audio ticks and measure completed frames after the first second, including simulation, REU transfers, live stereo audio, and publication. The frozen baseline receives the same static-cloud and pending weapon-SFX changes as the candidate. A separate fixed-work matrix compares identical title, lowlands, canyon, base, carrier, missile, full-pool, and section-boundary states with zero or ten simulation ticks and cold or retained palettes. See [docs/benchmarks.md](docs/benchmarks.md) for the protocol and results. These results **do not meet the 25 FPS PAL / 30 FPS NTSC goals**, and they are not physical C64 Ultimate measurements.

Resident assembly kernels occupy previously unused table space, but added preparation/cache code means this stage does **not** reduce total REU traffic: the benchmark transfers 45,682 bytes in 46 commands per ordinary frame, versus 44,281 bytes in 45 commands before optimization. The production build transfers 45,537 overlay bytes. The conditional compression experiment was not enabled: SIM (16,520 bytes) and GEOMETRY (16,882 bytes) exceed its 16,128-byte admission budget. Pixel processing and code transfers remain the next bottlenecks.

Native kernel reports are under `target/raster-test`, `target/raster-prep-test`, `target/sort-test`, and `target/pack-test`. Matching production hashes, screenshots, and control/weapon/streaming checks are in `target/vice/havoc-optimized-{pal,ntsc}/report.json`; all four startup-corruption checks pass in `target/corrupt-package-havoc-optimized/summary.json`. Historical music and HUD evidence remains under `target/vice/havoc-defeat-{pal,ntsc}` and `target/vice/weapon-labels-{pal,ntsc}`. Physical-machine display, audio, and full-mission testing remain outstanding.

Bootstrap ideas came from the neighboring `doomc64u` project, and Ultimate configuration examples from `secondreality`. The `ultrastarfox` disassembly is not copied into this project.
