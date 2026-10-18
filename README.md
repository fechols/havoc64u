# H.A.V.O.C.

**Heavy Assault Vessel: Orbital Combat**

Your ship is an unreasonable amount of firepower with a cockpit attached!

The title is a tribute to **A.L.C.O.N.**

An original polygon rail shooter for the **64 MHz C64 Ultimate**, written in Rust with a small 6502 assembly hardware layer. The visual inspiration is early flat-shaded console shooters; the models, mission, music, and effects are original. No Nintendo ROM or disassembly is required.

The first mission travels across open terrain, a canyon, and a defended base before a carrier boss. It includes enemies, scenery collisions, shields, pickups, explosions, three lives, section checkpoints, and victory/restart screens. This is a working prototype: rendering is still below the intended frame rate, and physical C64 Ultimate testing remains outstanding.

Each pair of levels is connected by an eight-second scenic flight. Hills rise into canyon walls around a walking creature and distant birds; a cargo hovercraft crosses right to left as the canyon opens onto the fortress; and eight staggered meteors streak across the darkening carrier coast. The hovercraft has faceted engine pods, hovering motion, and pulsing exhaust. The meteor shower ends before carrier combat begins. These three breaks add 24 seconds to the five-minute combat route.

Completing the mission and defeating the carrier starts a final eight-second victory reveal. The landscape, sky, and ship fade to black during the first two seconds while a fully 3D gold medal turns toward the player and grows beyond the screen edges. Its thick body, beveled rim, and raised silhouette of the player's ship are polygon geometry. The emblem shows the ship from above, nose upward, with swept wings and a raised cockpit. The medal settles into a gentle sway behind **MISSION COMPLETE**, the final score, and **FIRE: FLY AGAIN**. Victory music begins with the reveal and continues through the results. Controls are ignored during the reveal; afterward, release and press fire again to restart. The results remain on screen until then.

Each level has its own mechanical enemies, with distinct polygon models and behavior code loaded from REU. Eleven types make up the four exclusive rosters:

| Level | Enemies and behavior |
|---|---|
| Lowland Approach | **Kite patrol** flies a broad figure-eight and fires steadily; **Needle diver** weaves toward the player before committing to its dive; **Tracked gun** provides predictable ground fire. |
| Iron Canyon | **Razor skimmer** crosses the ravine with a rising and falling arc; **Gorge swooper** loops sideways through deep vertical passes; **Cliff battery** fires a two-shot burst from the wall. |
| Fortress Run | **Iron gunship** advances slowly in a shallow ellipse with heavy armor; **Lance sentry** feints inward and returns to its lane, pauses to fire, then accelerates; **Citadel bunker** fires a three-shot fan. |
| Carrier Assault | **Wing guard** flies an elliptical patrol and fires paired bursts; **Sting rammer** weaves while tracking the player, then commits to a fast unarmed attack. Both escort the existing carrier boss. |

All eight flying enemies have distinct movement patterns. Harmonic paths reuse the existing sine lookup table, while the sentry's feint uses a few age thresholds. Starting positions and age already stored on each enemy drive the patterns, with no additional tables or per-enemy state. Divers and rammers retain bounded steering and commit to their final approach before reaching the player.

The carrier's two turrets and protected core retain their existing rules. Scenic crossings clear the outgoing roster before the next level begins; attract previews and checkpoint restarts use the same level-specific enemies as gameplay.

All enemies, including the carrier, its turrets, and its exposed core, are twice their original size. Distant simplified shapes are enlarged too, so enemies remain easier to see throughout their approach. Shot and collision hitboxes scale with them; ground units retain their footing, and carrier targeting and gun positions follow the larger models.

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

Every five enemy kills immediately restore one bomb, up to the three-bomb limit. Gun, missile, and bomb kills all count. Progress carries across respawns and resets for a new run; rewards earned at the limit are not stored for later. Existing shield and bomb pickups still work normally.

Nearby shield pickups gently drift toward your ship as they approach, giving near misses a little magnetic help. Steer toward the shield to draw it in; sharp turns can still leave it behind.

Firing a bomb shakes the whole display, including the cockpit, for 640 ms. The border flashes white, yellow, orange, and red for 80 ms each before returning to black as the shake fades. These hardware effects keep moving between rendered frames and also work during scenic flights. Another bomb restarts the impact; an empty bomb inventory produces no effect.

Steering, firing, charging, rolls, and bombs remain available during scenic flights. Bombs still consume inventory. Outgoing enemies and hostile shots are cleared, damage is disabled, and wildlife is decorative: it cannot be targeted or collided with. The flight grants no shields, bombs, or score. The cockpit and reticle remain visible, and the next section's checkpoint, music, and arrival banner activate when the crossing ends.

The two reserved SID effects voices accompany each crossing: soft footfalls and bird calls, a passing transport engine with a lower departing pitch, and descending meteor sweeps. Sounds follow the visible scene across the left and right voices while the music continues. Scene cues take priority over ordinary gunfire; rolls, pickups, missiles, hits, bombs, and deaths can interrupt them. Interrupted one-shots are discarded, and later transport pulses can resume. Scene sounds finish before arrival and are canceled when the crossing ends or resets.

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
cargo run --release -- snapshot transition 0 4000 target/previews/lowlands-canyon
cargo run --release -- snapshot transition 2 8000 target/previews/carrier-arrival
cargo run --release -- snapshot victory 1000 target/previews/victory-fade
cargo run --release -- snapshot victory 8000 target/previews/victory-results
python3 tools/previews.py
```

Snapshots produce an aspect-correct PPM and the exact 10,000 native bitmap/screen/color bytes. The preview converter uses only Python's standard library.

`snapshot attract` takes elapsed milliseconds from a fresh title screen and replays the actual idle/preview simulation. For example, `35000` captures five seconds into Iron Canyon; `75000` captures the automatic return to the title.

`snapshot transition <0|1|2> <elapsed-ms> [output-prefix]` selects the outgoing section and a time from `0` through `8000`, rounded down to a 20 ms simulation tick. `4000` shows the bridge midpoint; `8000` shows the incoming checkpoint. These inspection snapshots do not replay preceding combat.

`snapshot victory <elapsed-ms> [output-prefix]` captures the final reveal from a representative completed mission, also rounded down to a 20 ms tick. `1000` shows the fading battlefield, `4000` shows the approaching medal, and `8000` shows the results. Later times capture the continuing sway and restart prompt; the preview supports up to 65,535 elapsed ticks (`1310719` ms before rounding). It does not replay preceding combat.

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
python3 tools/test-overlay-audit.py dist/havoc.elf
python3 tools/test-overlay-audit.py dist/4sid/havoc.elf
```

The smoke test checks the exact 10,000-byte native title, physical joystick port 2, SID music/effects activity, independent 50 Hz audio, frame progress, and dropped-input/triangle counters. It reads the SID profile from the package; use `--package-dir dist/4sid` for the enhanced build. Add `--control-checks` to verify up-to-climb/down-to-dive movement and left/right roll dodges through joystick port 2. Add `--weapon-checks` to seed encounters, trigger actual hit flashes, and fire fully charged homing missiles through the physical joystick input; it checks native hull colors, exhaust visibility, off-axis interception, and targetless expiry. Add `--streaming-checks` to seed section boundaries and results screens through the monitor, verify prefetched REU bytes, and exercise restart with real joystick input. These additional scenarios are separate from the ordinary timing sample; they are not a full native mission playthrough. It records emulator/package hashes, timing, native screenshots, and logs under `target/vice`. Corruption tests operate on private package copies under `target`; they leave `dist` intact.

The weapon checks also double-tap physical FIRE to verify bomb shake and border timing, a second bomb during the effect, and empty-inventory attempts. They trace actual display-register writes to check all sixteen shake phases, movement between rendered frames, bank publication during the effect, and restoration of the normal display. The companion emulator has an existing disabled-badline screen/color-fetch defect in its screenshots; exact native bitmap captures and register traces remain available for validation, while final appearance requires a physical C64 Ultimate check.

`--music-checks` plays a full loop plus a page of each of the seven cues while rendering continues. Monitor fixtures hold each scene, and Lowland uses physical held fire to exercise both effects voices. It checks page order, loop timing, active music lanes, and underruns. The two-SID run also checks that no writes reach the additional SID addresses.

`--attract-checks` observes a full 90-second title/map/title/repeat sequence, counting actual native simulation calls to verify each 750-tick interval. It checks segment-cache bytes, rendered sections, player resources, and stack/input/geometry guards, and captures every map. It also exercises dismissal with every joystick direction and fire, held-input consumption, and fresh-fire launch. Monitor writes establish the initial timer baseline and per-map input fixtures; the complete tour advances naturally and all dismissal/launch input uses physical joystick port 2.

## Implementation

The display is native **160 x 200 multicolor bitmap mode**, with the C64's 16-color palette and normal per-cell restrictions. An eight-line software color band feeds a cell-palette packer; retained color slots reduce palette changes. There is no texture mapping. The renderer performs fixed-point transforms, near-plane clipping, stable radix depth sorting, and flat triangle rasterization into two VIC bitmap banks. Persistent Q16 edges advance through the bands in a small assembly kernel. The packer writes directly to the hidden bitmap and screen, staging only color RAM.

Four coarse terrain bands form hills and uneven canyon ridges around a level flight corridor. Shared edge heights stay anchored to world distance, and two flat color tones reveal the triangular facets. Each section has its own sky, cloud arrangement, and horizon: blue-gray lowland ridges, warm canyon mesas, an overcast industrial fortress, and a purple dusk coast for the carrier. All five horizon triangles and nine cloud triangles stay fixed in screen space within each section, while terrain and gameplay objects follow the camera. The horizon bases extend below the far terrain to prevent sky gaps as the ground scrolls. Unobstructed background bands reuse the two existing bitmap buffers when both contain the same static backdrop and palette; section changes invalidate both banks, and moving geometry or HUD elements invalidate affected bands. Terrain and sky still use 38 triangles in the lowlands and carrier section, or 54 with canyon caps or base guide strips. The hills are scenic; existing obstacle collisions and ground-object positions remain unchanged.

Bridges use a separate scenery clock while combat distance pauses at the section boundary. Their terrain is sampled by world position, so the connecting landscape appears ahead and continues scrolling through both seams. Horizons and clouds interpolate between the neighboring areas, with stepped changes through the native palette. Animals, transport, and meteors are procedural geometry and use no enemy slots. Sound choreography samples the same clock once per rendered scene, discards expired cues after catch-up, and leaves envelopes to the existing 50 Hz audio interrupt. Backdrop caching is disabled throughout the crossing and invalidated at both ends; title and attract scenes retain their original terrain and timing.

Original synthesized music runs on a 50 Hz CIA interrupt independently of rendering. A foreground AUDIO overlay prefetches one-bar score pages from REU; the resident player handles notes, ties, legato, and effects without performing REU transfers. The two builds provide four or ten music voices and retain the same two reserved effects voices. Their complete score banks occupy 22,400 and 52,096 REU bytes respectively. Mixer settings use moderate left/right separation; the extra physical SID pair starts at −6 dB.

Simulation uses fixed 20 ms steps, while joystick gestures use timestamped display-rate input. Executable phases share a RAM window and are loaded from REU; audio and the interrupt handler remain resident. Detailed memory ownership and configuration references are in [docs/hardware.md](docs/hardware.md).

The six execution phases are BOOT, SIM, GEOMETRY, PIXELS, AUDIO, and SCENERY. SIM has four level images combining shared simulation with each level's enemy code, plus a terminal image for victory and defeat. Every SIM pass loads the matching image into the shared RAM window; a section or terminal-mode change during catch-up returns to the resident loader before the next tick or render. The 2,048-byte model cache changes only when the selected level changes. After SIM, each frame runs AUDIO when a score page is needed, SCENERY, GEOMETRY, then PIXELS. SCENERY stages up to 96 camera-space triangles in the existing command array; GEOMETRY projects them, adds the foreground, and preserves foreground-first ordering at equal depth. Victory substitutes a medal-drawing tail for combat geometry and repaints all 25 screen bands.

Package version 4 uses the `OVL5` code directory and four checked `SMA3` model banks, each including the medal mesh. Startup validates every code image and model bank; old PRG/REU packages are incompatible. The original SIM image remains at REU `$020000`, the other three level images occupy `$080000` through `$0A0000`, and the model banks begin at `$0B0000`. Victory geometry occupies `$0C0000` and terminal simulation `$0D0000`. The score and scenery banks remain at `$060000` and `$070000`.

During bridges, native transfers omit enemy/boss drawing code and include wildlife code in the shared overlay memory. Ordinary frames skip the wildlife code. Player and weapon rendering stay available in both cases; enemy simulation is paused throughout the crossing.

## Validation history and performance

The ship-medal revision passes **225 host tests per SID profile**, with one existing native-fixture export ignored. The emblem uses the player's actual upper hull and footprint, reducing the medal from 58 vertices / 100 triangles to 50 / 88. Both packages pass native memory and overlay audits plus PAL/NTSC checks for the complete reveal, frozen state, uninterrupted music, and held/fresh-fire behavior. Native captures show the raised silhouette with clear captions and no retained battlefield after blackout. Reports and previews are under `target/medal-silhouette/`; physical C64 Ultimate testing remains outstanding.

The terrain visibility correction prevents ground triangles from covering the
ship, its exhaust, Lowlands tracked guns and Fortress bunkers. Canyon wall batteries
and ordinary foreground occlusion keep their existing behavior. It uses a spare
coverage bit without adding triangles or framebuffers; the geometric limits are
documented in [the hardware notes](docs/hardware.md#terrain-visibility-coverage).


The gold-medal ending passes **212 host tests per SID profile**, with one existing native-fixture export ignored. Both packages pass memory and overlay audits, 13 negative relocation cases, and 29 startup-corruption cases per profile. PAL and NTSC native checks for both profiles verify the complete 400-tick reveal, frozen player state, continuing victory music, and fresh-fire restart. Two-SID PAL also passes controls, weapons, checkpoints, and the full attract tour. Each video standard passes 169 cached-versus-forced-repaint comparisons. The same 40-frame opening-game sample changes from 5.8628 to 5.8454 FPS PAL and remains 6.1360 FPS NTSC, within the five-percent regression budget. See [the medal measurements](docs/benchmarks.md#gold-medal-ending-2026-10-17). Physical C64 Ultimate testing remains outstanding.

Kaiju's native triangle preparation port preserves exact state, geometry, and pixels in 720 paired fixed-work samples. Median whole-frame case speed improves 9.1% PAL / 8.3% NTSC; the separate 400-tick ordinary-play probe improves from 5.503 to 5.872 FPS PAL and 5.644 to 6.096 FPS NTSC. Both SID packages and the cache replays pass. See [the native preparation measurements](docs/benchmarks.md#native-triangle-preparation-port-2026-10-17).

The doubled-enemy build passes **201 host tests per SID profile**, with one existing native-fixture export ignored, plus 95 library tests without host tools. Both native packages pass memory and overlay audits. Two-SID PAL checks pass weapons, section streaming, and the full attract tour; two-SID NTSC smoke and four-SID NTSC weapon checks also pass. The same 40-frame opening gameplay sample measures **5.4816 FPS PAL / 5.6573 FPS NTSC**, compared with 5.4517 / 5.6440 before enlargement (changes of +0.55% / +0.24%). These small differences show no slowdown in that sample, not a whole-mission speedup. Reports and before/after captures are under `target/enemy-scale/`. Physical C64 Ultimate testing remains outstanding.

The distinct transition animations and stereo effects pass **196 host tests per SID profile**, with one existing native-fixture export ignored. Both production packages and their instrumented counterparts pass memory, overlay-reference, IRQ-residency, and package audits. Final PAL/NTSC checks for both SID profiles verify all three 400-tick crossings, both effects channels alongside continuing music, controls, weapons, checkpoints, and result-screen restarts, with no geometry drops, input overflows, or music underruns. Reports are under `target/vice/transition-final-{2sid,4sid}-{pal,ntsc}`. The ordinary-play production samples remain within the five-percent regression budget; see [the transition measurements](docs/benchmarks.md#distinct-transitions-and-effects-2026-10-17). Physical hardware presentation and listening remain unverified.

The level-specific enemy build passes **181 host tests per SID profile**, with one existing native-fixture export ignored. Both native packages pass memory and overlay-reference audits, including eight negative relocation tests per package. Final PAL and NTSC checks for both SID profiles pass exact title output, controls, weapons, all three section handoffs, checkpoint respawns, result-screen restarts, and the complete attract tour. Every checked frame uses the expected enemy code and model bank. All **25 startup-corruption cases per profile** pass. Reports are under `target/enemies-final-vice-{2sid,4sid}-{pal,ntsc}` and `target/enemies-final-corrupt-{2sid,4sid}`.

The instrumented two-SID build passes 88 fixed-work cases (264 samples) and 157 cache/repaint comparisons per video standard. Focused bank-miss and bridge-exit checks also preserve the exact simulation tick count through SIM reloads. The ordinary-play probe measures **5.4892 FPS PAL / 5.6724 FPS NTSC**, respectively 1.17% and 0.72% above the preserved baseline. This meets the five-percent regression budget for that opening lowland sample; it does not establish a whole-mission speedup. See [the enemy benchmark measurements](docs/benchmarks.md#per-level-enemy-measurements) for workload and scope. Physical C64 Ultimate testing remains outstanding.

The measurements below describe earlier builds and are historical.

The scenic build passes **164 host tests per SID profile**, with one existing native-fixture export ignored. Coverage includes all three 400-tick crossings, controls across both seams, checkpoint and boss timing, and 4,800 saturated bridge/camera states with no dropped geometry. Direct and staged rendering also match in 36 full-frame comparisons. Both native sound profiles pass memory and overlay-reference audits.

Final two- and four-SID packages pass calibrated PAL and NTSC VICE checks for exact title output, all three natural scenic flights, section/music handoffs, result-screen restarts, and the complete attract tour. Controls and weapons pass on both video standards, with an additional four-SID NTSC run. The final cache replay matches forced repainting exactly across all 157 frames on each video standard. All 11 startup-corruption cases pass for each profile, including both partially loaded code tails and rejection of the old overlay directory. Reports are under `target/vice/scenic-release-{2sid,4sid}-{pal,ntsc}`, `target/bench/scenic-release-cache-{pal,ntsc}`, and `target/corrupt-scenic-release-{2sid,4sid}`.

The eight-second ordinary-play benchmark measures 5.4720 FPS PAL and 5.6843 FPS NTSC, respectively 4.48% and 4.08% below the preserved original build. That probe meets the five-percent budget; individual fixed fixtures can have larger costs. See [the scenic benchmark measurements](docs/benchmarks.md#scenic-flight-measurements) for the workload, comparisons, and limits. Physical C64 Ultimate testing remains outstanding.

The older reports and measurements below describe previous builds. Their frame rates, overlay sizes, and native acceptance results are historical.

The OST build passes **140 host tests per SID profile**, with one existing native-fixture export ignored. Six composition/playback tests cover the complete authored suite twice on PAL and NTSC clocks, with page service limited to five frames per second and effects continuing throughout. Additional checks preserve the complete *Lost Wing* transcription, verify cue selection, and reject damaged score data.

Both final packages pass all seven full cue loops plus continued refills in calibrated PAL and NTSC VICE, with zero score underruns, skipped pages, stack damage, or dropped input. Music stays at 50 Hz while rendering and effects continue. Both NTSC profiles also pass physical joystick controls, weapons, section streaming, result-screen restarts, and the complete attract tour. All seven startup-corruption cases pass for each profile. Reports are in `target/vice/havoc-ost-final-{2sid-pal,4sid-pal,4sid-ntsc}`, `target/vice/havoc-ost-2sid-ntsc`, and `target/corrupt-package-ost-final-{2sid,4sid}`; each records the unchanged input-package hashes.

The two/four-SID resident players occupy 2,699/2,707 code bytes, with 273/579 bytes for engine state and both score pages. Foreground audio loading shares the existing overlay window. Both production builds and the four-SID benchmark build pass memory and overlay audits. See [the memory accounting](docs/hardware.md#paged-melodic-score) for scope and remaining headroom. Physical-machine listening, final ARMSID mixer balance, and a complete native mission remain outstanding.

The fixed-background build passes **146 host tests per SID profile**, including 16,384 terrain/camera coverage cases with no sky slit below the stationary horizons. Native pack/raster checks pass, and all 133 camera/scene/HUD transition frames match forced repainting byte for byte on both PAL and NTSC. Production checks pass exact title rendering, section streaming, results/restarts, and complete attract tours. Both SID builds retain the eight-byte cache and fit all five overlays; GEOMETRY has 12 bytes free. Reports are under `target/vice/static-background-{pal,ntsc}` and `target/bench/static-background-{cache,cloud-only,full}-{pal,ntsc}`. See [the benchmark protocol and measurements](docs/benchmarks.md) for the scene-dependent benefit of horizon reuse.

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
