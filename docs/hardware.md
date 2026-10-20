# H.A.V.O.C. hardware and build notes

H.A.V.O.C. (Heavy Assault Vessel: Orbital Combat) targets the C64 Ultimate at 64 MHz with its built-in 16 MiB REU. The standard package in `dist/` uses two UltiSIDs; `dist/4sid/` adds two physical ARMSIDs for an enhanced arrangement. Import the chosen package's `havoc.cfg`, load its `havoc.reu` at REU address zero, then run its `havoc.prg`. Keep all three files from the same build and sound profile together.

The configuration selects the C64U turbo registers, 64 MHz CPU speed, disabled badline timing, and quiet bus operation. The runtime programs the turbo register at startup and checks that the setting is available. An ordinary 1 MHz C64 is not a supported execution target.

## Graphics and memory

The display is native VIC-II multicolor bitmap mode: 160×200 logical pixels drawn from the 16-color C64 palette. Each 4×8 logical-pixel cell has global black plus three local colors. Flat triangles first render into an eight-row color band; the packer chooses a stable legal cell palette and generates bitmap, screen, and color bytes. There is no texture mapping.

Two VIC banks hold the visible and next bitmap. The renderer publishes the color shadow and switches the VIC bank during the border interval. Six execution phases share one RAM window, with four ordinary enemy-profile SIM images, three boss-profile SIM images, and a terminal SIM image stored in REU. The audio player and interrupt handler remain resident. BOOT runs once; frames then run SIM, optional AUDIO page service, SCENERY, GEOMETRY, and PIXELS in order. The AUDIO overlay validates the complete score bank during startup and validates and prefetches requested pages during play; it is never called by the interrupt. Later AUDIO transfers omit the startup-only validator and directory parser.

Bomb impacts use the VIC fine-scroll and border registers, adding no triangles, bitmap copies, or sky-cache invalidation. A one-byte simulation event records a spent bomb independently of inventory rewards and the audio queue. SIM drains the latest bomb/reset event after catch-up and updates a resident sequencer with interrupts masked. Its 32-tick countdown runs on the existing 50 Hz clock; the raster interrupt at line 251 applies sixteen 40 ms samples. Horizontal kicks reach six physical pixels and vertical motion reaches three pixels either side of the normal Y-scroll value, tapering to rest over 640 ms. The border spends 80 ms each at white, yellow, orange, and red, then returns to black. Frame publication preserves active Y-scroll, and register writes preserve display-enable and bitmap-mode bits while keeping the raster comparison below line 256. Respawn and scene resets cancel the effect; scenic-flight seams preserve it, and a finishing bomb can continue through the results screen. Existing host snapshots contain only bitmap/screen/color bytes, so presentation-register effects are checked with native traces and VICE captures.

The bomb build passes 171 host tests per SID profile, with one existing fixture export ignored, and both production packages pass the PAL/NTSC weapon checks. Physical joystick gestures produce every shake phase, motion between frame publications, active retriggering, and neutral expiry; empty-inventory gestures produce no impulse. The observed countdowns last 623-639 ms, within the expected 20 ms clock quantization, with register updates following at the next raster interrupt. The two-SID ordinary-frame timing sample remains 5.493 FPS PAL / 5.766 FPS NTSC before and after the change. These short emulator samples do not establish physical-machine performance. Reports are under `target/vice/bomb-{2sid,4sid}-{pal,ntsc}`; the two-SID PAL run also covers controls, scenic streaming, and results-screen restarts.

Production companion screenshots have an existing screen/color-fetch defect with `$D031` bit 7 set: its matrix-fetch prefetch counter stays at four while badline stalls are disabled. Baseline captures made before the bomb change show the same corruption. Decoded bitmap/screen/color bytes and the VIC register traces validate rendering data and presentation timing, but final visual acceptance remains a physical C64 Ultimate check. The companion limitation is documented in the neighboring `doomc64u/docs/dual-image-game.md`.

| Address | Use |
|---|---|
| `$0002–$0021` | LLVM-MOS imaginary registers |
| `$0200–$07FF` | Resident globals, including a 168-byte validated music directory, and compiler static scratch frames; four-SID builds end below `$0720` |
| `$0720–$07FF` | Four-SID next-score page, 224 bytes; unused by music in the two-SID build |
| `$0801–$2FFF` | BASIC startup, resident code, data initializers |
| `$3000–$71FF` | BOOT, SIM, GEOMETRY, PIXELS, AUDIO, or SCENERY code overlay |
| `$6000–$6BFF` | Twelve-byte persistent raster tails during PIXELS; pixel code ends before `$6000` |
| `$6C00–$6F1F` | Twenty-five ordered triangle masks, 32 bytes per band |
| `$7200–$75FF` | IRQ software stack |
| `$7600–$77FF` | Color-output staging and packer scratch |
| `$7800–$7FFF` | Resident constants |
| `$8000` / `$A000` | Screen A / bitmap A |
| `$8400–$8BFF` | Current level's model bank and enemy descriptors |
| `$8C00–$8E7F` | 64-entry projected-vertex cache, then 624-byte depth-sort scratch during GEOMETRY |
| `$8C00–$90FF` / `$9100–$94FF` | Color band / color shadow; the band workspace also holds startup checks, SIM helpers and initial AUDIO directory scratch in separate phases |
| `$9500–$98FF` | Two 512-byte mission-segment buffers |
| `$9900–$9FFF` | Resident math/palette tables and compact sort/raster kernels |
| `$BF40–$BFFF` | Tail of IRQ static-frame backup |
| `$C000` / `$E000` | Screen B / bitmap B |
| `$C400–$C7FF` | Main software stack |
| `$C800–$CA7F` | Game state |
| `$CA80–$CDFF` | Main IRQ static-frame backup |
| `$CE00–$CFAF` | Resident audio engine, including its active score page; also the next page in the two-SID build |
| `$CFFA–$CFFF` / `$D000–$DFFF` | Renderer header / triangle commands, then sixteen-byte raster heads beneath I/O |

The PRG spans `$0801-$9FFF`, including resident constants and zero-filled gaps; its load image excludes phase overlays. Existing BOOT, SIM, GEOMETRY, PIXELS, AUDIO and terrestrial SCENERY allocations remain in place. Eighteen model banks occupy `$0B0000-$0B8FFF`; victory geometry is at `$0C0000`, terminal simulation at `$0D0000`, and SIM helpers at `$170000`. Ascent occupies `$180000`; the six orbital scenery images occupy `$190000-$1E0000`, one per 64 KiB slot. The 256 KiB music allocation is `$200000-$23FFFF`.

The REU image is always padded to 16 MiB. The allocated directories, common data, eighteen encounter/model banks, twenty-four `$4200` code slots and music reservation occupy 744,832 bytes in total, leaving 16,032,384 bytes (15.29 MiB) unreserved. The contiguous tail from `$240000` to `$FFFFFF` alone provides 13.75 MiB. These figures count reserved address ranges; the serialized code and music payloads use less than their reservations.

The frozen `target/offplanet-release/` images contain 496,349 bytes of serialized payload for two SIDs and 580,001 bytes for four SIDs. This counts 256 bytes of package/model/code directories, 3,072 common bytes, thirty-six 2,048-byte encounter/model banks, the twenty-four image lengths recorded by `OVL7`, and the `SMU3` bank's recorded total length. It excludes allocation padding without treating zero-valued payload bytes as free space.

Each composed SIM image has the same common prefix and one profile tail. The audited `encounter_step(Game*, AssetBank*)` entry owns actor movement, attacks, and the optional boss update. Shared code handles controls, progression, spawning, collisions, projectiles, and pickups. Stage identity is independent of this code profile: changing stages always refreshes the encounter metadata and model bank even when the next stage shares code. Native SIM reloads every frame because later phases reuse the window. A catch-up stage/profile change returns to the resident loader before another tick or render, preserving its existing fixed deadline. Enemy updates remain suspended during scenic flights.

SIM also loads a checked helper image from REU `$170000` into `$8C00–$90FF`, a 1,280-byte execution allowance. This reuses retired startup-check space and the later geometry/pixel workspace; it does not replace the model cache or mission buffers. The resident loader restores it in two transfers before every SIM entry because later rendering phases overwrite that RAM. All general DMA transfers are capped at 1,024 bytes to bound audio IRQ latency. The IRQ never calls the helper image.

The 128-byte `OVL7` directory at REU `$000100` records twenty-four checked code images: existing phases, all eight SIM compositions, victory GEOMETRY, ten composed scenery images, and the SIM helper. The ascent and six orbital scenery records occupy bytes 88-115; helper fields remain at 84-87. The directory also authenticates SID configuration and the PRG, constants and startup code. SIM images retain the common padded size for startup validation and the eight-entry `__sim_load_sizes` table for shorter runtime transfers. Boss kinds 4 and 5 reuse SIM profiles 6 and 5; terminal profile 7 remains distinct. The packer rejects overlapping allocations and images beyond the 16 MiB REU. Build the PRG and REU together.

Package ID `$534D0600` uses eighteen 2,048-byte `SMA3` model banks at `$0B0000-$0B8FFF`. The 64-byte `MDL2` directory at `$000040` retains version 2: magic at 0-3, stage count at 4, version at 5, bank size at 6, REU base at 8, eighteen checksums at 12-47, zero padding at 48-61, and checksum at 62. BOOT validates every bank, leaving stage 0 in the existing `$8400-$8BFF` cache. A stage change checks and copies the incoming bank in two 1,024-byte transfers. Same-stage frames and respawns retain the cache; model views never survive replacement.

The 64-byte main header contains eighteen encounter checksums at 20-55, zero padding at 56-61, and its checksum at 62. Eighteen 2,048-byte encounter allocations occupy `$002000-$00AFFF`; only their checked 512-byte prefixes enter the two gameplay buffers. `SMS2` keeps spawn records at byte 16 and the 62-record maximum. Bytes 8-15 hold the unchanged eight-byte `StageInfo`: enemy profile, music cue, crossing profile, boss kind, current/next/previous themes and one reserved zero byte. Complete campaign authoring remains host-only. Orbital encounters use ordinary movement profiles 0-3 and explicit three-dimensional hazard positions; ROCK flag bit 1 (`SMALL_DEBRIS = 2`) marks a shootable fragment. Unflagged rocks remain solid obstacles.

`SMA3` retains nine mesh descriptors at bytes 16–87 and three eight-byte enemy descriptors at 88–111. Header bytes 5, 6, and 7 identify stage, active enemy count, and boss kind. Ordinary stages expose three enemy slots; boss stages expose two escorts and reserve mesh slot 3 for boss side units. Mesh slot 4 contains the crawler, reactor, carrier hull, Dreadnought or Gate Guardian ring; slot 8 remains the 50-vertex / 88-triangle medal. The player and pickup models remain shared; crystal and volcanic stages customize obstacle meshes. The 3,072-byte common bank contains the initial models and score directory. Runtime encounter fetches checksum their previously validated 512-byte prefixes.

SCENERY lays out the terrestrial prefix first, followed by shared space helpers and a wildlife, transport, meteors, ascent, or orbital environment tail. Ordinary terrestrial frames transfer only `__phase_scenery_terrain_size`, omitting both space helpers and decoration tails. A terrestrial crossing additionally loads its selected decoration at the fixed `scenic_decoration` address, leaving the intervening space-helper region unloaded. Ascent and orbital frames load the selected full composition. The orbital tail also draws its outgoing crossing, while the existing transition state and sound-effect queue handle ascent. The complete padded images remain checked at startup. GEOMETRY retains its player/projectile/effects prefix and combat or medal drawing. The relocation audit accepts only the reviewed opaque entries and rejects cross-profile references. Every borrowed asset view ends before its DMA destination is reused.

Victory reuses the existing mode with a 400-tick reveal counter. The first 100 ticks freeze and darken the ship and battlefield; medal geometry is appended after color remapping so it stays gold. Once black, scenery and ship emission stop. The alternate GEOMETRY tail replaces combat drawing. The terminal SIM tail handles title/menu changes, respawning, scenic crossings, Victory and GameOver; ordinary attract ticks use the scene's enemy profile, returning to terminal SIM for dismissal or the next preview. Mode/profile changes return to the resident loader before another catch-up tick. The medal shares the ordinary mesh-transform instantiation through a camera-neutral scene budget; it does not duplicate that code or enlarge its vertex cache. PIXELS paints all 25 bands and shows only the final captions after tick 400. The existing preparation optimization leaves room for these captions in the normal PIXELS image, so no extra pixel overlay is required.

Checked crossing metadata selects wildlife, cargo transport, meteors, ascent or orbital flight. The original three decorations retain their existing bounded geometry; ascent raises the view from the carrier coast into the starfield, and orbital crossings retain the active space environment. All scenery must fit the 80-face transition reservation. The common SCENERY prefix selects sound cues after simulation catch-up; foreground Runtime owns its small scene/time history. A frame submits only the newest still-valid cue per lane, preventing missed frames from replaying old bursts. Distant scenery has no collision state; authored ROCK actors supply the visible hazards.

Cue publication and scene-only cancellation briefly mask interrupts and update audio RAM without touching SID registers, even while I/O is banked out for scenery. The existing resident 50 Hz effects player owns all envelopes and pitch changes on voice three of SID 0 and SID 1. Scenic effects outrank basic gunfire and yield to every higher player-effect tier; music, filters, global volume, and added four-SID music lanes are untouched. Leaving a crossing removes pending scenic cues and gates off an active scenic voice on the next audio tick while retaining player effects. No IRQ code calls the scenery overlay.

The pre-expansion production baseline used 609 of 640 Game bytes, 16,241 of 16,896 composed SIM bytes, 13,302 SCENERY bytes, 15,888 combat GEOMETRY bytes, 15,798 victory GEOMETRY bytes and 12,245 of 12,288 PIXELS bytes. Its helper used 1,207 of 1,280 bytes, its tables 1,777 of 1,792, and resident headroom was 81/69 bytes for two/four SIDs. These are historical baseline figures, not orbital-build results. Current authoritative sizes are generated in each package's `memory-map.txt`; linker and post-link checks enforce every region. New label strings are packed in constant RAM with byte-offset tables in PIXELS, avoiding eighteen four-byte slice descriptors.

The optimized release maps retain the 609-byte Game state and 12,234-byte PIXELS image. Composed SIM uses 16,810 bytes, combat GEOMETRY 16,680 bytes, SCENERY 16,709 bytes, and SIM helpers 1,260 bytes. The terrestrial SCENERY transfer is 10,726 bytes, compared with its complete 13,591-byte shared prefix. Runtime AUDIO transfers are 1,117/1,127 bytes for two/four SIDs; startup additionally loads 1,961/1,982 bytes of validation code and data. Resident code headroom is 139/127 bytes, constants occupy 1,938/1,958 of 2,048 bytes, and compiler scratch ends at `$06BF`, below the four-SID page at `$0720`. These are allocation and transfer sizes, not frame-rate measurements.

The post-link audit checks all twenty-four startup `__code_sizes` entries and all eight runtime `__sim_load_sizes` entries against linked image sizes. Negative fixtures shorten and lengthen table entries. Native corruption checks repair the directory checksum after changing stored lengths, ensuring startup rejects authenticated but incorrect code sizes.

The diamond and shield pickup share one planar emitter with 15 bytes of face selectors, saving 623 GEOMETRY bytes while preserving triangle order and coordinates. Terrain side selection uses signs instead of general multiplication. Both changes reduce per-frame work without changing the geometry or native pixels; the matched timing results are in [benchmarks.md](benchmarks.md#campaign-expansion-final-2026-10-18).

Exact precomputed PAL/NTSC note-frequency tables reduce resident IRQ code without changing any note's pitch. The shared swept-depth collision helper uses 92 bytes of assembly in the SIM helper image instead of the compiler's 127-byte implementation. Its independent native simulator test passes 6,658 cases through the production C ABI, including signed limits and wrapping interval bounds; results are in `target/game-math-test/report.json`. This is a code-size and kernel-correctness result, not a campaign frame-rate measurement. No title-specific RAM is reclaimed: title rendering already uses PIXELS. The code loader preserves 1,024-byte DMA chunks and specializes address calculations for the fixed code window; general asset transfers retain their range checks.

Startup disables both ROMs before clearing BSS. The custom linker discards the SDK's `.init.250` PETSCII case-mode hook because it calls KERNAL through vector RAM now owned by game globals. The game's data-copy hook uses `.init.251`; the build audit verifies that startup calls only BSS clearing, data copying, and `main`.

A fatal startup or runtime error stops playback and displays a text diagnostic in screen A. This terminal error path restores normal VIC badline timing with `$D031=$0F`, allowing the character display to refresh normally; gameplay uses the configured disabled-badline mode.

SCENERY, GEOMETRY, and PIXELS bank out I/O to access the triangle command array. SCENERY writes up to 96 raw, camera-space records of 20 bytes each into its existing tail at `$D880` through `$DFFF`; no additional resident staging allocation is needed. Each record holds three signed 16-bit XYZ vertices, color, and flags. GEOMETRY reads a record before projecting it, checks that emitted commands cannot overwrite unread records, and then adds foreground geometry. It rotates the projected scenery prefix behind the foreground before stable sorting, preserving the host renderer's foreground-first order at equal depth. The foreground face allowance is `min(174, 233 - raw scenery count)`, leaving near-plane clipping room in the 256-command array.

GEOMETRY clears and uses a 640-byte vertex cache inside the color-band workspace at `$8C00`. Once all vertices are projected, the stable depth radix sort reuses 624 bytes of that workspace for byte indices, counts, and permutation scratch. It handles all 256 commands and retains authored order at equal depth. Mesh transforms calculate each active axis's sine and cosine once per object and skip identity scaling, preserving the original separate Q8 products and rounding. No projection-cache references survive sorting, and the IRQ never uses this workspace. PIXELS subsequently reuses the complete command array for raster heads, so raw scenery records never survive to the next frame.

PIXELS consumes each sorted triangle command in place into a sixteen-byte persistent raster head. Twelve-byte tails at `$6000` and ordered bit masks at `$6C00` identify the triangles touching each band. Edge positions advance sequentially in Q16 across all 25 bands, including bands whose painting can be skipped; edges are not recomputed from scratch for every band. Pixel packing writes bitmap and screen bytes directly into the back VIC bank, staging only the 40 color bytes before updating the color shadow. Cells with at most three nonblack colors skip omitted-color approximation while keeping the same palette ranking, retained slots, and HUD priority.

Each of the ten terrestrial visual themes uses five horizon triangles and nine cloud facets, decoded from compact signed-byte descriptors in SCENERY. All 14 faces stay fixed in camera space during ordinary gameplay; crossings interpolate the outgoing and incoming arrangements. Horizons use the neutral gameplay camera height of 40. The extended horizon bases cover the moving terrain even at camera extremes. Each theme has its own horizon and cloud arrangement and palette. Outdoor clouds and horizons occupy separate bands, so each 4-by-8 cell contains at most three nonblack colors despite the larger palette across the complete backdrop. Crystal Cave reuses the fourteen faces for walls, roof, and hanging crystals, sharing three nonblack colors even where they overlap. Ashen Wastes limits its scenery and rock obstacles to orange, charcoal, and black, leaving the third nonblack cell slot available to an isolated red projectile.

An eight-byte cache tracks which bands in each physical VIC bank contain only the current section's static backdrop. Reuse is limited to Playing mode and requires all 14 accepted backdrop commands, an authored sky clear color, no dropped commands, no foreground geometry touching the band, no HUD, and matching front/back screen bytes. With at most three nonblack colors per cell and identical static pixels, the exact screen-byte comparison also proves matching bitmap and color bytes; no hashes or additional color buffers are needed. Before rendering a different section, the SCENERY sky-selection helper compares the rendered section and transition state and clears both validity masks. Scenic flights disable reuse throughout their animated backdrop, with explicit invalidation on entry and exit. This forces both banks to repaint even if different backdrop arrangements happen to produce matching screen palettes. Both banks must be valid to prevent old geometry from reappearing two frames later. Reused bands still advance the raster edges, and completing a frame replaces that back bank's validity mask. Foreground bounds remain conservative, so canyon walls can prevent reuse even with a fixed backdrop.

Attract mode uses the same simulation, section cache, geometry, and raster pipeline as gameplay. Title inactivity and each section preview last 750 fixed 20 ms steps on both PAL and NTSC. A latched activity bit preserves joystick pulses between simulation steps; scene resets consume the dismissal input and clear encounters and gestures. Attract frames omit the ship, reticle, and cockpit and paint all 25 bands, with sky reuse disabled. The small phase-independent Q8 multiply helper lives in resident code to accommodate the expanded game-state layout. Attract scenes select the same area cues as gameplay.

Playing mode inserts a 400-tick scenic flight at each of seventeen boundaries. Each stage has 3,750 combat ticks: eighteen stages plus crossings total 74,300 ticks, or 24:46 before deaths and boss holds. Native progress uses a stage-local `u16 stage_ticks` plus the stage ID; aggregate HUD/report distance is derived with 32-bit arithmetic. Scenery uses local crossing coordinates so campaign length cannot overflow its old absolute 16-bit clock. Bosses enter at local tick 1,000 in stages 4, 8 and 12, and tick 2,500 in stages 17 and 18. Unfinished bosses hold progress at tick 3,750 while combat continues. The outgoing checkpoint remains active through a crossing; its successor begins only at exit.

Player controls, charge and double-tap history, player projectiles, and explosions remain live during scenic flights. Entry removes outgoing actors, pickups, and hostile shots before processing controls; enemy spawning, boss simulation, collisions, and damage are suspended. Bombs retain their inventory cost. Successful boss-stage crossings restore shield 100, raise bombs to at least two, and add one life up to three, once on entry; other crossings preserve resources. Decorations have no actor, collision, or targeting state. The incoming checkpoint and music begin on exit, with encounter processing on the following tick. The existing two-slot cache prefetches that incoming stage during the crossing.

The assembly IRQ wrapper preserves the processor-port setting, maps I/O back in, saves the compiler registers and static scratch frames, switches to the IRQ stack, and restores everything before returning. This also protects interrupted arithmetic helpers that LLVM-MOS gives static local storage. Compile-time guards, the linker, and the post-link layout checker enforce the reserved boundaries.

## SID configurations

The two executables are selected at build time with Cargo's `four-sid` feature. There is no runtime hardware probe or profile menu. Both configurations disable range splitting and automatic address mirroring, giving every enabled SID a separate address. The standard configuration leaves both physical sockets unmapped and muted.

| Chip | Address | Two-SID package | Four-SID package | Pan |
|---|---|---|---|---|
| UltiSID 1 | `$D400` | 0 dB; two music + one effect voice | Same | Left 2 |
| UltiSID 2 | `$D420` | 0 dB; two music + one effect voice | Same | Right 2 |
| Physical socket 1 | `$D440` | Unmapped, muted | −6 dB; three music voices | Left 2 |
| Physical socket 2 | `$D460` | Unmapped, muted | −6 dB; three music voices | Right 2 |

The extra pair is intended for ARMSIDs configured for 8580 behavior. Set that behavior on the ARMSIDs themselves: the game's configuration supplies socket addressing, gain, and pan, not ARMSID firmware settings. The −6 dB socket gain is the initial balance for the added orchestration. Addressing and mixer controls are described in the [Ultimate MultiSID guide](https://1541u-documentation.readthedocs.io/en/latest/config/multi_sid.html).

Both UltiSIDs use the same `8580 Lo` filter curve, `High` resonance setting, and `8580` combined-waveform behavior. Music owns their filters; the reserved third effects voices bypass them. All three voices on each added physical SID are available to music. The engine uses oscillator waveforms, ADSR envelopes, and noise. SID volume remains constant during playback; there are no digital samples or volume-register DAC effects.

The Ultimate Audio mapping at `$DF20–$DFFF` is disabled, and both sampler channels are muted. The firmware's `Digis Level=Medium` setting is retained but is not used for sampled playback. Mixer panning applies to HDMI and AV output, not the physical SID-TAP header. The built-in SID player's autoconfiguration can override mixer settings, so re-import the game configuration after using that player. See the [official mixer documentation](https://1541u-documentation.readthedocs.io/en/latest/config/audio_mixer.html).

## Paged melodic score

The score runs at 50 Hz from CIA1 Timer A independently of rendering, with PAL/NTSC timer periods and frequency tables. Twenty-one cues preserve the original thirteen arrangements and add six mission themes, an ascent cue and the Gate Guardian encounter. The four core parts remain identical in both profiles; baking strips only the optional six-part orchestration for two SIDs. The additional cues reuse the existing paged player and require no larger IRQ page cache.

Each score page contains one bar of sixteen sixteenth-note rows. Cues carry their own checked row duration. The score allocation is `$200000-$23FFFF` (256 KiB), with 32-bit bank-relative offsets; package `assets.txt` records the exact used byte count for its sound profile. Track names, arrangements and row timing are authored in [src/soundtrack.rs](../src/soundtrack.rs).

The resident engine holds the active page, SID register shadows, two effects states, and a small event queue. The foreground AUDIO overlay fills a separate next-page buffer and publishes it while interrupts are masked. The buffer is not modified until the player consumes it. The IRQ copies a ready page at the bar boundary and requests its successor; it performs no REU access. If the next page is late, playback waits before the boundary's release tick, preserving held music and continuing effects. `MUSIC_UNDERRUNS` counts each incident. Cue changes request page zero and insert a complete gate-low tick before the new cue starts.

Startup also caches the twenty-one validated `CueInfo` descriptors in 168 bytes of resident low-RAM `.bss`. This array lies outside the compiler scratch region saved by the IRQ, and only foreground AUDIO code reads it. Its entries become available after the complete bank scan succeeds and remain unchanged during play. Page requests use these cached offsets, lengths and tempos, avoiding repeated REU directory transfers. The IRQ engine and its active/next page capacities remain unchanged.

Historical OST-build MOS layouts used 177 bytes for the two-SID engine plus a 96-byte next page, or 355 bytes for the four-SID engine plus a 224-byte next page: **273 and 579 bytes total**. Seven further bytes hold the loader checksum, validation flag, and exported music diagnostics. The two-SID build places both audio objects inside the existing 432-byte `$CE00–$CFAF` allocation. The four-SID engine uses that allocation and places its next page at `$0720–$07FF`; compiler scratch ends at `$05CD`. The foreground loader reuses the retired band workspace for its directory. That OST revision had 710 bytes of resident headroom in the standard build and 696 in the enhanced build, with one spare byte in GEOMETRY. These are historical figures, not the six-overlay scenic build limits. The memory maps in each package record the actual target sizes; host ABI sizes can differ.

The historical OST ELF symbols account for **2,699 / 2,707 bytes of resident player code** in the two/four-SID builds: music transport, effects, register writes, pitch conversion, page requests, and the audio-tick wrapper. This excludes shared arithmetic helpers, constants, and the common IRQ wrapper. The foreground AUDIO overlay is 2,859 / 2,881 bytes and reuses the existing overlay window; it adds no separate resident code allocation. Its loader and validators are unreachable from the IRQ. Four-SID routing tables avoid a per-note divide, keeping the extra six voices inexpensive in code size.

The bank format is `SMU3`. Its 256-byte header records SID/voice profile, total length, page size, and twenty-one eight-byte cue entries; the directory can represent thirty cues without another resize. Entries contain bank-relative offset, page count, row duration and cue checksum. The common asset bank stores a matching header copy. `SPG2` pages remain unchanged: a 12-byte header, five instrument bytes per voice, and sixteen rows of four or ten events, totaling 96/224 bytes for two/four SIDs. Unused bytes are zero.

Events are `0` for rest, `1` for hold, MIDI notes `24..95` for an articulated attack, or a note with bit `$80` set for legato. Holds preserve the gate and pitch; legato changes pitch without releasing the gate. A new attack or rest releases the preceding note one 20 ms tick before its boundary. Page publication rejects a hold or legato opening when switching cues or changing that voice's instrument. For *Lost Wing*, an attack plus hold at nine ticks per row reproduces each old 18-tick note, followed by two resting rows; its full loop remains 2,304 ticks, or 46.08 seconds.

Header and page checksums treat their own two-byte fields as zero. Whole-cue checksums cover the stored pages including their checksums. Before the first cue plays, the foreground validator checks the header signature, all directory bounds, contiguous cue spans, profile fields, instrument/note ranges, tempos, page indices, next-page links, padding, and checksums. It also verifies the final bank extent before publishing the cached descriptors. Every subsequently fetched page is validated again before publication, including its checksum, cue/page identity, tempo and next-page link. Runtime uses the validated cached descriptors as its directory authority; it does not reread the REU header. The first AUDIO load includes the validator and parser, while later loads stop at `__phase_audio_runtime_size`. The `OVL7` code directory still checks the complete AUDIO image alongside the other executable images and matching PRG, constants, startup code, and SID profile.

## Build and verification

Host tools require Rust and Cargo. The target build additionally requires Linux/WSL, Python 3, LLVM-MOS SDK **v22.5.0**, and a Rust-MOS toolchain with the `mos-c64-none` target and Rust sources. This workspace was built with `rustc 1.87.0-dev` registered as the `mos` rustup toolchain; `target-c64/rust-toolchain.toml` selects that alias. `scripts/env.sh` defaults to `$HOME/.local/llvm-mos` and `$HOME/.local/rust-mos-targets`. Override `LLVM_MOS`, `MOS_CLANG`, and `RUST_TARGET_PATH` for another compatible installation.

From PowerShell, using the configured Ubuntu-24.04 WSL toolchain:

```powershell
./scripts/build-windows.ps1
```

Or from Linux/WSL:

```sh
bash scripts/build.sh
```

The complete build tests both profiles, bakes assets, links the C64 programs, audits memory and overlays, and produces matching `havoc.prg`, `havoc.reu`, and `havoc.cfg` files in `dist/` and `dist/4sid/`. The four-SID native build uses its own `target-c64/target-4sid` directory. Generated packages are ignored by Git; cloning the repository does not supply `dist/`.

For a single profile after changing assets, bake with `cargo +stable run --release -- bake dist` or `cargo +stable run --release --features four-sid -- bake dist/4sid`, then run `bash scripts/build-target.sh 2sid` or `bash scripts/build-target.sh 4sid` respectively. The bake step alone does not produce executable overlays. On Windows, `scripts/build-windows.ps1` runs the complete two-profile build through WSL; its optional output directory receives the standard package and a `4sid` subfolder.

Host inspection commands:

```sh
cargo test --all-targets
cargo test --all-targets --features four-sid
cargo run --release -- sizes
cargo run --release --features four-sid -- sizes
cargo run --release -- snapshot title 0 target/previews/title
cargo run --release -- snapshot 2 5000 target/previews/canyon
cargo run --release -- snapshot attract 35000 target/previews/attract-crystal
cargo run --release -- snapshot transition 0 4000 target/previews/lowlands-crystal
python3 tools/previews.py
```

The host binary is an asset and inspection tool, not a desktop game. Snapshots produce an aspect-correct PPM and the exact 10,000 bitmap/screen/color bytes; the preview converter needs only Python's standard library. `snapshot attract` replays elapsed milliseconds from a fresh title screen, so `35000` shows five seconds into Crystal Cave. `snapshot transition <0..16> <elapsed-ms> [output-prefix]` selects an outgoing stage and a time from `0` to `8000`, rounded down to a 20 ms tick; `8000` shows the incoming checkpoint. Transition snapshots do not replay preceding combat. For orbital examples, use stage `12..17`; `snapshot transition 11 4000` samples the ascent.

Optional native kernel checks use the LLVM-MOS 6502 simulator:

```sh
source scripts/env.sh
python3 tools/test-pack-kernel.py
python3 tools/test-raster-kernel.py
python3 tools/test-raster-prep.py
python3 tools/test-preparation-abi.py
python3 tools/test-sort-kernel.py
python3 tools/test-memcpy-kernel.py
python3 tools/test-game-math-kernel.py
python3 tools/test-glyph-kernel.py
python3 tools/benchmark-division.py
```

Emulator checks use the calibrated sibling `doomc64u/target/audit-fast/x64sc-64x` companion and its ROMs; override these with `--vice` and `--roms`. An unmodified companion can map the turbo setting to 48x, invalidating a 64x timing comparison.

```sh
cargo +stable run --release -- snapshot title 0 target/previews/title
python3 tools/vice-smoke.py --video pal --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video ntsc --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video pal --control-checks --weapon-checks --streaming-checks
python3 tools/vice-smoke.py --video pal --attract-checks
python3 tools/vice-smoke.py --video pal --music-checks
python3 tools/vice-smoke.py --video pal --package-dir dist/4sid --music-checks
python3 tools/test-corrupt-package.py
python3 tools/test-overlay-audit.py dist/havoc.elf
python3 tools/test-overlay-audit.py dist/4sid/havoc.elf
```

The smoke test records package/emulator hashes, exact native title bytes, physical joystick-port-2 input, audio activity, frame progress, and runtime guards under `target/vice`. Control and weapon checks exercise steering, roll dodges, homing missiles, hit flashes, and bomb display-register effects; streaming checks seed stage boundaries, checkpoints, and restart scenarios. Attract checks follow the complete idle tour and exercise dismissal with each input. These monitor-assisted cases are separate from a continuous campaign and physical-machine acceptance. Use distinct `--out-dir` paths when retaining multiple runs.

`tools/native-campaign.py` uses the same package and emulator options to play all eighteen stages with physical port-2 joystick input. It reads game state to aim and avoid solid obstacles, but never writes game state, health, clocks or benchmark controls. Small debris is targeted while distant and avoided if it survives into the collision corridor. The pilot checks geometry drops every rendered frame and cumulative input/music diagnostics at checkpoints; victory requires visiting every stage. `tools/test-native-campaign.py` checks these obstacle decisions independently of the emulator.

The frozen `target/offplanet-final/` packages completed full campaigns on 2026-10-20. Reports in `target/offplanet-validation/final-campaign-2sid-pal/` and `final-campaign-4sid-ntsc/` record 8,491 and 9,149 rendered frames, all eighteen stages, all five bosses and the completed victory reveal. Both finished with three lives and 88 shield; the NTSC run included a natural stage-11 death and checkpoint respawn. Package hashes remained unchanged, with no input overflows or music underruns. Those runs predate the pilot's per-frame geometry assertion and checked geometry drops at campaign checkpoints; the native showcase checked every captured frame. The accompanying seven-scene four-SID PAL video and its capture/encoding verification live under `target/offplanet-validation/final-showcase-4sid-pal/`.

These full playthroughs and captures precede the selective SCENERY transfers, cached music descriptors and shortened runtime AUDIO transfer described above. They establish campaign behavior for their recorded package hashes, not a full playthrough of the later `target/offplanet-release/` binaries. Retain this distinction when interpreting the separate final-build checks and performance results.

The optimized `target/offplanet-release/` packages passed all four final native smoke suites on 2026-10-20. Each suite checks four control scenarios, six weapon scenarios, forty-three streaming/results scenarios, seven attract scenarios, and a complete loop plus one page for each new cue (13–20). Streaming coverage includes all seventeen crossings, five bosses, eighteen checkpoints, the victory reveal, and both restart paths. Each seven-map attract tour advances naturally and checks physical-input dismissal. All four reports confirm unchanged package hashes, zero music underruns, intact stack guards, complete score-page coverage, and correct audio clocks and authored pitches:

| Profile | Final smoke and eight-cue report |
| --- | --- |
| Two SIDs, PAL | `target/offplanet-validation/release-smoke-2sid-pal/report.json` |
| Two SIDs, NTSC | `target/offplanet-validation/release-smoke-2sid-ntsc/report.json` |
| Four SIDs, PAL | `target/offplanet-validation/release-smoke-4sid-pal/report.json` |
| Four SIDs, NTSC | `target/offplanet-validation/release-smoke-4sid-ntsc/report.json` |

The final four-SID PAL showcase also uses the optimized release package. `target/offplanet-validation/release-showcase-4sid-pal/manifest.json` and `target/offplanet-validation/release-showcase-4sid-pal/encoded/report.json` both pass: seven scenes, a 72-second MP4, unchanged package hashes, and every captured frame checked for geometry drops, input overflows and music errors. This selected-scene capture is distinct from the earlier complete campaign pilots.

`target-c64/build.rs` assembles the native hardware and rendering helpers, disables additional LTO zero-page register allocation, selects the custom linker script, and emits a memory map. Six primary phase sections share an execution address; seven encounter tails and the terminal tail share the SIM prefix's end address, while combat and medal drawing share the GEOMETRY prefix's end address. Explicit linker assertions and the project's post-link checks replace the general linker overlap check. ELF symbols and relocations are preserved for emulator debugging, package extraction, and `tools/check-overlays.py`. That checker rejects cross-phase references, permits only named phase entry calls and reviewed opaque dispatches into the enemy, terminal, and rendering tails, validates ownership of compiler switch tables and the lifetime of startup checks, and checks that the IRQ's relocation graph stays resident. Runtime-computed pointers and timing still require execution tests.

The optimized loader's `tools/test-overlay-audit.py` suite rejects 55 damaged relocation and ownership fixtures, including terrestrial references to unloaded space helpers and AUDIO references to unloaded startup code. `tools/test-layout-audit.py` rejects 66 mutations: short/long entries for all eight SIM profiles and twenty-four startup code images, plus an undersized music descriptor array or placement inside IRQ-saved compiler scratch. Both use private copies and verify that their input packages remain unchanged. Native scenery passes the concrete `SceneryStage` sink through its opaque decoration entry, so no resident vtable exception is needed for a callback into an overlay.

For repeatable size profiling, run `python3 tools/profile-size.py --baseline target/campaign-baseline/dist/havoc.elf --current dist/havoc.elf --out-dir target/campaign-size`. The read-only comparison writes `README.txt` and `profile.json` with function sizes and deltas, per-section totals, and common-prefix-plus-tail headroom. Either input may instead be a linker `.map`, allowing an overflowing build to be inspected before it produces an ELF. Repeated generic names with ambiguous matches are marked for review rather than assigned a guessed baseline.

Compiler flags must be checked at both optimization stages: Rust emits bitcode for this target, and the LLVM-MOS SDK performs a final LTO link. The [isolated compiler-option comparison](benchmarks.md#campaign-compiler-options-2026-10-18) distinguishes those stages, records size and failure results, and shows package-level overrides. Cargo has no per-source-file optimization profile; linker sections select memory placement, not a different optimization level.

`python3 tools/test-game-math-kernel.py` builds the production swept-depth assembly and a frozen Rust reference under the configured Linux/WSL toolchain. It checks 6,658 native simulator cases and records sizes and an identical 2,048-call timing harness in `target/game-math-test/report.json`. That harness measured 825,860 reference cycles and 758,449 production cycles; these include harness overhead and do not measure complete game frames.

The native `memcpy.S` replaces the Rust compiler's larger memory-copy routine and its static scratch frame. It follows the LLVM-MOS C ABI and uses only caller-clobbered registers. `python3 tools/test-memcpy-kernel.py` runs 1,297 native 6502 simulator cases against an independent volatile byte-copy loop, covering empty copies, page boundaries, unaligned addresses, returned pointers, and source/destination guards. Its generated report lives under `target/memcpy-test`.

Unscaled 3-by-5 text that fits completely within one eight-row band uses a bounded path: Rust checks the whole label once, then calls the 59-byte `fast_glyph` kernel in PIXELS. The kernel writes only set glyph pixels at a 160-byte row stride and uses no static scratch. Clipped or split-band text retains the general renderer. Host tests compare this path with `draw_text_scaled` for all 256 character bytes and clipping boundaries. Under the configured Linux/WSL toolchain, `python3 tools/test-glyph-kernel.py` checks 5,120 native calls against an independent scalar oracle: all 256 patterns in each of five rows, four ink bytes, every destination page alignment, source page crossings, complete buffer/guard comparisons, and preservation of `rc8..31`, including the C ABI callee-saved registers. Results are recorded in `target/glyph-kernel-test/report.json`.

The other native kernels preserve host reference behavior: `sort.S` implements stable far-to-near radix ordering, `raster.S` advances prepared edges and paints flat spans, and `pack.S` chooses and emits VIC cell palettes. The compact sort kernel and static-sky scan use the resident table region; the raster walker and preparation kernel use PIXELS, leaving the fixed overlay boundaries intact. Their simulator checks compare bytes against independent Rust fixtures and verify memory guards. Host geometry tests compare cached transforms with the original per-vertex arithmetic; sky-cache tests exercise every previous cell palette, repeated packing, two-bank occlusion recovery, and HUD eligibility boundaries. These correctness checks do not substitute for target timing measurements.

`preparation.S` implements exact native Q16 triangle setup, including sorted-edge orientation parity, signed rounding, near-screen clipping, and in-place command replacement. Its specialized signed slope helper shares the PIXELS overlay. Host builds retain the Rust reference. Preparation temporarily uses 31 bytes at `$77A0–$77BE`, before row walking or palette packing borrows that same scratch; model and mission buffers remain intact. The linker and memory audit check this ownership.

`python3 tools/test-raster-prep.py` checks both the production Rust entry point and the direct assembly kernel against an independent integer reference. It compares all 28 prepared head/tail bytes for 1,108 triangles at four alignments, with separate storage and actual in-place command replacement: 17,728 preparations with buffer and scratch guards. This closes the gap left by the row-walker test, whose inputs are already prepared on the host. Its report lives under `target/raster-prep-test`. `python3 tools/test-preparation-abi.py` separately checks 1,024 calls with changing callee-saved register canaries, stack and I/D flags, immutable input, and output guards; its report lives under `target/preparation-abi`.

`python3 tools/test-corrupt-package.py` exercises package rejection in the calibrated VICE companion using damaged code/model directories, executable images, common and level assets, PRG/REU pairing, SID profile, and score pages. It uses private copies under `target/corrupt-package-test`, verifies the error-screen text bytes, saves screenshots and logs, and confirms the input package hashes remain unchanged. It reads failure addresses from the matching ELF and chooses a free monitor port. Pass `--package-dir dist/4sid` to exercise the enhanced package. These checks are emulator regressions, not physical C64 Ultimate acceptance.

The final optimized release passed 125 corruption cases per sound profile, including every executable image and all twenty-one music cues. `target/offplanet-validation/release-corrupt-2sid/summary.json` and `target/offplanet-validation/release-corrupt-4sid/summary.json` record all 250 successful rejections and unchanged input hashes.

`tools/vice-smoke.py --music-checks` derives the SID profile and cue count from the package, then verifies every complete cue loop plus an extra page while frames continue. It observes page order, clocks, underruns, authored pitches and lane activity; a held-fire Lowland fixture exercises both effects voices. The two-SID profile must not write to `$D440/$D460`. Add `--music-cues 13 14 15 16 17 18 19 20` to repeat just the eight new full loops, as in the final release suites above.

All twenty-one cues previously passed complete native loops in both sound profiles and both video standards on the frozen `target/offplanet-final/` packages. Those reports are `target/offplanet-validation/final-smoke-2sid-pal/report.json`, `target/offplanet-validation/final-smoke-2sid-ntsc/report.json`, `target/offplanet-validation/final-music-4sid-pal/report.json`, and `target/offplanet-validation/final-music-4sid-ntsc/report.json`. They precede the optimized directory cache and AUDIO transfers; the final release repeats the eight new loops and functional checks on the exact release hashes. The older thirteen-cue suite under `target/music-validation/native-{2sid,4sid}-{pal,ntsc}/` is historical evidence. These monitor-assisted scenes are separate from ordinary-play timing and a complete campaign.

`tools/music-previews.py` records the native player through VICE's SID audio output. Historical recordings of *Crystal Current*, *Ashes and Steel*, *Tempest Drive*, *Gorge Warden*, *Citadel Heart*, and *Carrier Overdrive* remain under `target/music-validation/previews-{2sid,4sid}/`; those manifests report zero clipped samples. The orbital score adds *Slingshot Through the Belt*, *Silver Fractures*, *Echoes of Lost Fleets*, *Rivets in Orbit*, *Shadow of the Dreadnought*, *Beyond the Blue Threshold*, *Atmosphere to Starlight*, and *The Last Sentinel*. The six mission cues and final boss cue each contain four distinct eight-bar sections; the ascent lasts eight seconds.

`target/offplanet-validation/music-2sid/recordings.json` and `target/offplanet-validation/music-4sid/recordings.json` pass all eight new recordings: 407.36 seconds per profile of 44.1 kHz stereo WAV audio, with both channels active and zero clipped samples. The WAVs and manifests are bundled in `target/offplanet-validation/music-2sid.zip` and `target/offplanet-validation/music-4sid.zip`. They retain the earlier frozen packages as their recording provenance. `target/offplanet-validation/release-music-equivalence.json` verifies that their complete encoded score banks are byte-identical to the final release: 62,944 bytes for two SIDs and 146,528 for four SIDs. Physical SID listening and mixer balance remain unverified.

The soundtrack tests check cue lengths, distinct melodic sections, loop/page articulation, both serialization profiles, and the exact *Lost Wing* transcription. A passing host test or valid linker map does not establish a frame-rate result on the machine. Hardware acceptance must check PAL and NTSC presentation timing, all twenty-one cues and page boundaries, simultaneous music and effects, REU transfers during play, stack guards, and full-mission restart/checkpoint behavior. Four-SID acceptance also needs the physical ARMSID pair and its final mixer balance.

The bootstrap structure was informed by the neighboring `doomc64u` repository. `secondreality/src/output/cfg.rs` provided a local example of the Ultimate mixer and SID-address configuration keys. H.A.V.O.C. uses its own synthesis player and does not adopt Second Reality's digital-audio modes.


## Reproduce the README showcase

Build the four-SID package first, then run the capture and encoding steps from Linux/WSL:

```sh
python3 tools/capture-showcase.py \
  --package-dir dist/4sid --video pal --out-dir target/readme-showcase
python3 tools/encode-showcase.py target/readme-showcase/manifest.json
```

Capture uses the compiled C64 program in the calibrated emulator and records its SID music and gameplay effects. The showcase presents selected scenes rather than a continuous campaign or a physical-machine recording. Keep the footage at its recorded game speed: duplicated video frames must not be presented as a higher game rendering rate. The four-SID recording represents emulated SID synthesis, not a recording of the physical ARMSIDs or their final hardware mixer balance.

The storyboard includes the ascent and all six orbital missions. Repeat `--scene <name>` to select scenes, or use `--duration 1` for short capture probes. The capture and encoder also accept the two-SID package with `--package-dir dist`; their manifests record the actual sound profile.

The encoder writes `target/readme-showcase/havoc-showcase.mp4` by default. It requires FFmpeg and FFprobe; use `--ffmpeg <path>` and `--ffprobe <path>` to select their executables. Native Linux and Windows executables are supported, with WSL path conversion for Windows tools. Preserve the capture manifest and source recordings with the generated video so its package, scene selection, timing, and audio can be checked before publishing. Generated capture artifacts stay under `target/`; only the selected README images belong in `docs/media/`.

## Terrain visibility coverage

The player hull/exhaust, ordinary-stage tracked guns and Fortress bunkers use the
`TERRAIN_CLEAR_GEOMETRY` command flag (`0x20`). Their authored geometry is proven
to lie in front of every exterior terrain face along the camera ray. The flag
survives near clipping and preparation without changing the 16-byte command or
12-byte tail. Drawing these faces sets pixel bit `0x40`; ordinary foreground
writes replace the color while retaining that coverage, and tagged exterior
terrain skips covered pixels. Canyon wall batteries, boss turrets, buildings,
rocks, effects and other actors keep normal foreground ordering.

The hull stays inside X -136..137 and above world Y -103 for all 256 roll angles;
tracked guns fit the same minimum 145-unit lane. Fortress bunkers reach X +/-158
but stay at Y >= -98, above the conservative campaign shoulder envelope
`495*(y+105) = 240*(abs(x)-145)`. Authored ground enemies spawn after their incoming
terrain blend finishes. Transitions clear actors, and attract previews use their
stage's terrain style. Tests cover all eight stages with ordinary ground enemies,
the original twelve terrain sequences and eleven terrestrial crossings, plus orbital coverage tests. They enforce these bounds,
spawn timing and terrain separation. Do not assign this flag to arbitrary
meshes or below-ground objects without establishing the same visibility proof.

Only lane, shoulder, outer-cap and runway strips carry `EXTERIOR_TERRAIN` (`0x40`
in commands/tails). Wildlife and transport strips remain ordinary geometry. The
private scenery triangle helper temporarily packs the terrain tag above its
color nibble to retain a compact native calling convention, then emits separate
color and geometry flags to the sink.

The raster walker uses one sequential-phase byte at `$77AB` to remember whether
a protected span has painted the current band. Each band resets it, including
hidden and empty bands. Preparation and packing borrow overlapping scratch only
before and after rasterization; the IRQ never accesses it. Bands without protected
coverage retain the ordinary constant-color fill. Both packers ignore pixel bit 6;
HUD priority remains bit 7. There is no additional framebuffer or triangle storage.

The raster walker now resides in PIXELS. The conservative static-sky scan and the
pure scenery blend-weight helper occupy resident table-code space, keeping all
phase ownership and memory-limit checks active. The coverage rule does not replace
the general average-depth painter sort for other overlapping geometry.
