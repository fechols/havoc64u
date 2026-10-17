# H.A.V.O.C. hardware and build notes

H.A.V.O.C. (Heavy Assault Vessel: Orbital Combat) targets the C64 Ultimate at 64 MHz with its built-in 16 MiB REU. The standard package in `dist/` uses two UltiSIDs; `dist/4sid/` adds two physical ARMSIDs for an enhanced arrangement. Import the chosen package's `havoc.cfg`, load its `havoc.reu` at REU address zero, then run its `havoc.prg`. Keep all three files from the same build and sound profile together.

The configuration selects the C64U turbo registers, 64 MHz CPU speed, disabled badline timing, and quiet bus operation. The runtime programs the turbo register at startup and checks that the setting is available. An ordinary 1 MHz C64 is not a supported execution target.

## Graphics and memory

The display is native VIC-II multicolor bitmap mode: 160×200 logical pixels drawn from the 16-color C64 palette. Each 4×8 logical-pixel cell has global black plus three local colors. Flat triangles first render into an eight-row color band; the packer chooses a stable legal cell palette and generates bitmap, screen, and color bytes. There is no texture mapping.

Two VIC banks hold the visible and next bitmap. The renderer publishes the color shadow and switches the VIC bank during the border interval. Five REU code overlays share an execution window while the audio player and interrupt handler remain resident. The AUDIO overlay only validates and prefetches score pages; it is never called by the interrupt.

| Address | Use |
|---|---|
| `$0002–$0021` | LLVM-MOS imaginary registers |
| `$0200–$07FF` | Resident globals and compiler static scratch frames; four-SID builds end below `$0720` |
| `$0720–$07FF` | Four-SID next-score page, 224 bytes; unused by music in the two-SID build |
| `$0801–$2FFF` | BASIC startup, resident code, data initializers |
| `$3000–$71FF` | BOOT, SIM, GEOMETRY, PIXELS, or foreground AUDIO code overlay |
| `$6000–$6BFF` | Twelve-byte persistent raster tails during PIXELS; pixel code ends before `$6000` |
| `$6C00–$6F1F` | Twenty-five ordered triangle masks, 32 bytes per band |
| `$7200–$75FF` | IRQ software stack |
| `$7600–$77FF` | Color-output staging and packer scratch |
| `$7800–$7FFF` | Resident constants |
| `$8000` / `$A000` | Screen A / bitmap A |
| `$8400–$8BFF` | Common model assets |
| `$8C00–$8E7F` | 64-entry projected-vertex cache, then 624-byte depth-sort scratch during GEOMETRY |
| `$8C00–$90FF` / `$9100–$94FF` | Color band / color shadow; the band workspace also holds retired startup checks, then the AUDIO directory scratch |
| `$9500–$98FF` | Two 512-byte mission-segment buffers |
| `$9900–$9FFF` | Resident math/palette tables and compact sort/raster kernels |
| `$BF40–$BFFF` | Tail of IRQ static-frame backup |
| `$C000` / `$E000` | Screen B / bitmap B |
| `$C400–$C7FF` | Main software stack |
| `$C800–$CA7F` | Game state |
| `$CA80–$CDFF` | Main IRQ static-frame backup |
| `$CE00–$CFAF` | Resident audio engine, including its active score page; also the next page in the two-SID build |
| `$CFFA–$CFFF` / `$D000–$DFFF` | Renderer header / triangle commands, then sixteen-byte raster heads beneath I/O |

The PRG spans `$0801–$9FFF`, including zero-filled gaps, so both constant pools exist before the first Rust call. It also initializes `.startup_checks` at `$8C00`; those checks finish before BOOT reuses that space for assets. Its load image does not contain the phase overlays. The BOOT, SIM, GEOMETRY, PIXELS, and AUDIO ELF sections are packaged at REU offsets `$010000`, `$020000`, `$030000`, `$040000`, and `$050000`; each executes at `$3000` after transfer. The score bank begins at REU `$060000`.

Startup disables both ROMs before clearing BSS. The custom linker discards the SDK's `.init.250` PETSCII case-mode hook because it calls KERNAL through vector RAM now owned by game globals. The game's data-copy hook uses `.init.251`; the build audit verifies that startup calls only BSS clearing, data copying, and `main`.

A fatal startup or runtime error stops playback and displays a text diagnostic in screen A. This terminal error path restores normal VIC badline timing with `$D031=$0F`, allowing the character display to refresh normally; gameplay uses the configured disabled-badline mode.

Geometry and pixel processing bank out I/O to access the triangle commands. GEOMETRY clears and uses a 640-byte vertex cache inside the color-band workspace at `$8C00`. Once all vertices are projected, the stable depth radix sort reuses 624 bytes of that workspace for byte indices, counts, and permutation scratch. It handles all 256 commands and retains authored order at equal depth. Mesh transforms calculate each active axis's sine and cosine once per object and skip identity scaling, preserving the original separate Q8 products and rounding. No projection-cache references survive sorting, and the IRQ never uses this workspace.

PIXELS consumes each sorted triangle command in place into a sixteen-byte persistent raster head. Twelve-byte tails at `$6000` and ordered bit masks at `$6C00` identify the triangles touching each band. Edge positions advance sequentially in Q16 across all 25 bands, including bands whose painting can be skipped; edges are not recomputed from scratch for every band. Pixel packing writes bitmap and screen bytes directly into the back VIC bank, staging only the 40 color bytes before updating the color shadow. Cells with at most three nonblack colors skip omitted-color approximation while keeping the same palette ranking, retained slots, and HUD priority.

Each section uses five horizon triangles and nine cloud facets, decoded from compact signed-byte descriptors by a resident helper. Horizons retain camera movement; clouds stay fixed in screen space within their section. The sky/cloud palettes are blue/white/light gray, orange/yellow/light red, dark gray/light gray/gray, and purple/light red/light blue. Each contains exactly three nonblack colors, fitting the three local palette slots.

An eight-byte cache tracks which bands in each physical VIC bank contain only the current section's static sky. Reuse is limited to Playing mode and requires all nine accepted cloud commands, an authored sky clear color, no dropped commands, no other geometry touching the band, no HUD, and matching front/back screen bytes. For identical static pixels, the exact screen-byte comparison also proves matching bitmap and color bytes; no hashes or additional color buffers are needed. Before rendering a different section, the resident sky-selection helper compares the existing rendered-section identifier and clears both validity masks. This forces both banks to repaint even if different cloud arrangements happen to produce matching screen palettes. Both banks must be valid to prevent old geometry from reappearing two frames later. Reused bands still advance the raster edges, and completing a frame replaces that back bank's validity mask.

Attract mode uses the same simulation, section cache, geometry, and raster pipeline as gameplay. Title inactivity and each section preview last 750 fixed 20 ms steps on both PAL and NTSC. A latched activity bit preserves joystick pulses between simulation steps; scene resets consume the dismissal input and clear encounters and gestures. Attract frames omit the ship, reticle, and cockpit and paint all 25 bands, with sky reuse disabled. The small phase-independent Q8 multiply helper lives in resident code to accommodate the expanded game-state layout. Attract scenes select the same area cues as gameplay.

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

The score runs at 50 Hz from CIA1 Timer A, independently of frame presentation. PAL and NTSC use different timer periods and SID frequency tables. The seven cues share a common flight motif but have separately authored melodies and harmonic sections. The four core parts are identical in both packages; only the optional six-part orchestration is stripped when baking the two-SID bank. *Lost Wing* preserves its original four instruments and all 64 sounding note tuples, including the spaces between them.

Each score page contains one bar of sixteen sixteenth-note rows. Title and Lowland use six ticks per row; Canyon, Fortress, and Carrier use five; Victory uses seven; Defeat uses nine. At the fastest tempo a page lasts 1.6 seconds. The 232 pages and their directory occupy 22,400 bytes for two SIDs or 52,096 bytes for four SIDs in the REU bank. Track names and loop durations are listed in the [soundtrack table](../README.md#original-soundtrack).

The resident engine holds the active page, SID register shadows, two effects states, and a small event queue. The foreground AUDIO overlay fills a separate next-page buffer and publishes it while interrupts are masked. The buffer is not modified until the player consumes it. The IRQ copies a ready page at the bar boundary and requests its successor; it performs no REU access. If the next page is late, playback waits before the boundary's release tick, preserving held music and continuing effects. `MUSIC_UNDERRUNS` counts each incident. Cue changes request page zero and insert a complete gate-low tick before the new cue starts.

Measured MOS layouts use 177 bytes for the two-SID engine plus a 96-byte next page, or 355 bytes for the four-SID engine plus a 224-byte next page: **273 and 579 bytes total**. Seven further bytes hold the loader checksum, validation flag, and exported music diagnostics. The two-SID build places both audio objects inside the existing 432-byte `$CE00–$CFAF` allocation. The four-SID engine uses that allocation and places its next page at `$0720–$07FF`; compiler scratch ends at `$05CD`. The foreground loader reuses the retired band workspace for its directory. Resident code has 710 bytes of headroom in the standard build and 696 bytes in the enhanced build; the geometry overlay remains tight at one spare byte. The memory maps in each package record the actual target sizes; host ABI sizes can differ.

Final ELF symbols account for **2,699 / 2,707 bytes of resident player code** in the two/four-SID builds: music transport, effects, register writes, pitch conversion, page requests, and the audio-tick wrapper. This excludes shared arithmetic helpers, constants, and the common IRQ wrapper. The foreground AUDIO overlay is 2,859 / 2,881 bytes and reuses the existing overlay window; it adds no separate resident code allocation. Its loader and validators are unreachable from the IRQ. Four-SID routing tables avoid a per-note divide, keeping the extra six voices inexpensive in code size.

The bank format is `SMU2`. Its 128-byte header records the SID/voice profile, total bank length, page size, and seven entries containing a bank-relative cue offset, page count, row duration, and cue checksum. A copy of the header is kept in the common asset bank so the runtime can verify the REU directory. Each `SPG2` page contains a 12-byte header, five instrument bytes per music voice, and sixteen rows of four or ten events. Page sizes are 96 and 224 bytes; unused bytes are zero.

Events are `0` for rest, `1` for hold, MIDI notes `24..95` for an articulated attack, or a note with bit `$80` set for legato. Holds preserve the gate and pitch; legato changes pitch without releasing the gate. A new attack or rest releases the preceding note one 20 ms tick before its boundary. Page publication rejects a hold or legato opening when switching cues or changing that voice's instrument. For *Lost Wing*, an attack plus hold at nine ticks per row reproduces each old 18-tick note, followed by two resting rows; its full loop remains 2,304 ticks, or 46.08 seconds.

Header and page checksums treat their own two-byte fields as zero. Whole-cue checksums cover the stored pages including their checksums. Before the first cue plays, the foreground validator checks all directory bounds, contiguous cue spans, profile fields, instrument/note ranges, tempos, page indices, next-page links, padding, and checksums. Every subsequently fetched page is validated again before publication. The `OVL2` code directory separately records five overlay sizes/checksums, the SID profile, and PRG/constant/startup checksums to reject mismatched packages.

## Build and verification

The target build uses Linux/WSL, the `mos` Rust toolchain, the `mos-c64-none` target, and LLVM-MOS SDK v22.5.0. `scripts/env.sh` exposes the installed compiler and target descriptions. `scripts/build.sh` tests and packages both profiles, producing matching `havoc.prg`, `havoc.reu`, and `havoc.cfg` files in `dist/` and `dist/4sid/`. The four-SID native build uses its own `target-c64/target-4sid` directory. `MOS_CLANG`, `LLVM_MOS`, and `RUST_TARGET_PATH` can point to another compatible installation.

For a single profile after changing assets, bake with `cargo +stable run --release -- bake dist` or `cargo +stable run --release --features four-sid -- bake dist/4sid`, then run `bash scripts/build-target.sh 2sid` or `bash scripts/build-target.sh 4sid` respectively. The bake step alone does not produce executable overlays. On Windows, `scripts/build-windows.ps1` runs the complete two-profile build through WSL; its optional output directory receives the standard package and a `4sid` subfolder.

`target-c64/build.rs` assembles the native hardware and rendering helpers, disables additional LTO zero-page register allocation, selects the custom linker script, and emits a memory map. Five phase sections deliberately share an execution address, so the general linker overlap check is replaced by explicit linker assertions and the project's post-link checks. ELF symbols and relocations are preserved for emulator debugging, package extraction, and `tools/check-overlays.py`. That checker rejects cross-phase references, allows only the five named entry calls from `main`, validates ownership of compiler switch tables and the lifetime of startup checks, and checks that the IRQ's relocation graph stays resident. Runtime-computed pointers and timing still require execution tests.

The native `memcpy.S` replaces the Rust compiler's larger memory-copy routine and its static scratch frame. It follows the LLVM-MOS C ABI and uses only caller-clobbered registers. `python3 tools/test-memcpy-kernel.py` runs 1,297 native 6502 simulator cases against an independent volatile byte-copy loop, covering empty copies, page boundaries, unaligned addresses, returned pointers, and source/destination guards. Its generated report lives under `target/memcpy-test`.

The other native kernels preserve host reference behavior: `sort.S` implements stable far-to-near radix ordering, `raster.S` advances prepared edges and paints flat spans, and `pack.S` chooses and emits VIC cell palettes. The compact sort and raster kernels share unused space in the resident table region, leaving the fixed overlay boundaries intact. Their simulator checks compare bytes against independent Rust fixtures and verify memory guards. Host geometry tests compare cached transforms with the original per-vertex arithmetic; sky-cache tests exercise every previous cell palette, repeated packing, two-bank occlusion recovery, and HUD eligibility boundaries. These correctness checks do not substitute for target timing measurements.

`python3 tools/test-raster-prep.py` separately compiles the production Rust edge-preparation helpers and division assembly for the MOS CPU. It compares all 28 prepared head/tail bytes against an independent integer reference for 1,108 triangles at four alignments, both with separate storage and with the command replaced in place: 8,864 preparations with guard checks. This closes the gap left by the row-walker test, whose inputs are already prepared on the host. Its report lives under `target/raster-prep-test`.

`python3 tools/test-corrupt-package.py` checks seven package rejection paths in the calibrated VICE companion: a damaged code directory, corrupt BOOT code, corrupt AUDIO code, corrupt common assets, mismatched PRG/REU checksums, a mismatched SID profile, and a corrupt score page. It uses private copies under `target/corrupt-package-test`, verifies the error-screen text bytes, saves screenshots and logs, and confirms the input package hashes remain unchanged. It reads failure addresses from the matching ELF and chooses a free monitor port. Pass `--package-dir dist/4sid` to exercise the enhanced package. These checks are emulator regressions, not physical C64 Ultimate acceptance.

`tools/vice-smoke.py --music-checks` derives the SID profile from the package and verifies all seven complete cue loops plus an extra page while frames continue. It observes page order, clocks, underruns, and authored lane activity; a held-fire Lowland fixture also exercises both effects voices. The two-SID profile must not write to `$D440/$D460`. These monitor-assisted scenes are separate from ordinary-play timing and from a complete mission playthrough.

Useful host checks are `cargo test --all-targets`, `cargo test --all-targets --features four-sid`, `cargo run -- sizes`, and `cargo run -- snapshot title`. The soundtrack tests check cue lengths, distinct melodic sections, loop/page articulation, both serialization profiles, and the exact *Lost Wing* transcription. A passing host test or valid linker map does not establish a frame-rate result on the machine. Hardware acceptance must check PAL and NTSC presentation timing, all seven cues and page boundaries, simultaneous music and effects, REU transfers during play, stack guards, and full-mission restart/checkpoint behavior. Four-SID acceptance also needs the physical ARMSID pair and its final mixer balance.

The bootstrap structure was informed by the neighboring `doomc64u` repository. `secondreality/src/output/cfg.rs` provided a local example of the Ultimate mixer and SID-address configuration keys. H.A.V.O.C. uses its own synthesis player and does not adopt Second Reality's digital-audio modes.
