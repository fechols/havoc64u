# H.A.V.O.C. hardware and build notes

H.A.V.O.C. (Heavy Assault Vessel: Orbital Combat) targets the C64 Ultimate at 64 MHz with its built-in 16 MiB REU and two UltiSIDs. Import `havoc.cfg`, load `havoc.reu` at REU address zero, then run `havoc.prg`. Keep all three files from the same build together.

The configuration selects the C64U turbo registers, 64 MHz CPU speed, disabled badline timing, and quiet bus operation. The runtime programs the turbo register at startup and checks that the setting is available. An ordinary 1 MHz C64 is not a supported execution target.

## Graphics and memory

The display is native VIC-II multicolor bitmap mode: 160×200 logical pixels drawn from the 16-color C64 palette. Each 4×8 logical-pixel cell has global black plus three local colors. Flat triangles first render into an eight-row color band; the packer chooses a stable legal cell palette and generates bitmap, screen, and color bytes. There is no texture mapping.

Two VIC banks hold the visible and next bitmap. The renderer publishes the color shadow and switches the VIC bank during the border interval. The original 20 KiB resident-code budget proved too small for the Rust program; four REU code overlays now share a larger execution window while the audio interrupt remains resident.

| Address | Use |
|---|---|
| `$0002–$0021` | LLVM-MOS imaginary registers |
| `$0200–$07FF` | Resident globals and compiler static scratch frames |
| `$0801–$2FFF` | BASIC startup, resident code, data initializers |
| `$3000–$71FF` | BOOT, SIM, GEOMETRY, or PIXELS code overlay |
| `$6000–$6BFF` | Twelve-byte persistent raster tails during PIXELS; pixel code ends before `$6000` |
| `$6C00–$6F1F` | Twenty-five ordered triangle masks, 32 bytes per band |
| `$7200–$75FF` | IRQ software stack |
| `$7600–$77FF` | Color-output staging and packer scratch |
| `$7800–$7FFF` | Resident constants |
| `$8000` / `$A000` | Screen A / bitmap A |
| `$8400–$8BFF` | Common model assets |
| `$8C00–$8E7F` | 64-entry projected-vertex cache, then 624-byte depth-sort scratch during GEOMETRY |
| `$8C00–$90FF` / `$9100–$94FF` | Color band / color shadow |
| `$9500–$98FF` | Two 512-byte mission-segment buffers |
| `$9900–$9FFF` | Resident math/palette tables and compact sort/raster kernels |
| `$BF40–$BFFF` | Tail of IRQ static-frame backup |
| `$C000` / `$E000` | Screen B / bitmap B |
| `$C400–$C7FF` | Main software stack |
| `$C800–$CA7F` | Game state |
| `$CA80–$CDFF` | Main IRQ static-frame backup |
| `$CE00–$CFAF` | Resident audio engine |
| `$CFFA–$CFFF` / `$D000–$DFFF` | Renderer header / triangle commands, then sixteen-byte raster heads beneath I/O |

The PRG spans `$0801–$9FFF`, including zero-filled gaps, so both constant pools exist before the first Rust call. Its load image does not contain the phase overlays. Their ELF sections `.phase.boot`, `.phase.sim`, `.phase.geometry`, and `.phase.pixels` are packaged at REU offsets `$010000`, `$020000`, `$030000`, and `$040000`; each executes at `$3000` after transfer.

Startup disables both ROMs before clearing BSS. The custom linker discards the SDK's `.init.250` PETSCII case-mode hook because it calls KERNAL through vector RAM now owned by game globals. The game's data-copy hook uses `.init.251`; the build audit verifies that startup calls only BSS clearing, data copying, and `main`.

A fatal startup or runtime error stops playback and displays a text diagnostic in screen A. This terminal error path restores normal VIC badline timing with `$D031=$0F`, allowing the character display to refresh normally; gameplay uses the configured disabled-badline mode.

Geometry and pixel processing bank out I/O to access the triangle commands. GEOMETRY clears and uses a 640-byte vertex cache inside the color-band workspace at `$8C00`. Once all vertices are projected, the stable depth radix sort reuses 624 bytes of that workspace for byte indices, counts, and permutation scratch. It handles all 256 commands and retains authored order at equal depth. Mesh transforms calculate each active axis's sine and cosine once per object and skip identity scaling, preserving the original separate Q8 products and rounding. No projection-cache references survive sorting, and the IRQ never uses this workspace.

PIXELS consumes each sorted triangle command in place into a sixteen-byte persistent raster head. Twelve-byte tails at `$6000` and ordered bit masks at `$6C00` identify the triangles touching each band. Edge positions advance sequentially in Q16 across all 25 bands, including bands whose painting can be skipped; edges are not recomputed from scratch for every band. Pixel packing writes bitmap and screen bytes directly into the back VIC bank, staging only the 40 color bytes before updating the color shadow. Cells with at most three nonblack colors skip omitted-color approximation while keeping the same palette ranking, retained slots, and HUD priority.

The nine cloud facets stay fixed in screen space while mountains retain camera movement. An eight-byte cache tracks which bands in each physical VIC bank contain only this static sky. Reuse is limited to Playing mode and requires all nine accepted cloud commands, blue clear color, no dropped commands, no other geometry touching the band, no HUD, and matching front/back screen bytes. Because the static colors fit all three local palette slots, the exact screen-byte comparison also proves matching bitmap and color bytes; no hashes or additional color buffers are needed. Both banks must be valid to prevent an old enemy or panel from reappearing two frames later. Reused bands still advance the raster edges, and completing a frame replaces that back bank's validity mask.

The assembly IRQ wrapper preserves the processor-port setting, maps I/O back in, saves the compiler registers and static scratch frames, switches to the IRQ stack, and restores everything before returning. This also protects interrupted arithmetic helpers that LLVM-MOS gives static local storage. Compile-time guards, the linker, and the post-link layout checker enforce the reserved boundaries.

## Dual-SID configuration

`UltiSID 1` is mapped at `$D400` and panned `Left 2`; `UltiSID 2` is mapped at `$D420` and panned `Right 2`. Both mixer gains are 0 dB. This gives moderate separation, including some sound in the opposite output channel. Physical SID sockets are unmapped and muted. Range splitting is off because the game needs exactly two independently addressed SIDs; automatic address mirroring is enabled. This is the two-SID starting configuration documented by the [Ultimate MultiSID guide](https://1541u-documentation.readthedocs.io/en/latest/config/multi_sid.html).

Both chips use the same `8580 Lo` filter curve, `High` resonance setting, and `8580` combined-waveform behavior. Each supplies two music voices and one reserved effects voice. Music owns the filters; the effects voices bypass them. The engine uses oscillator waveforms, ADSR envelopes, and noise. SID volume remains constant during playback; there are no digital samples or volume-register DAC effects.

The Ultimate Audio mapping at `$DF20–$DFFF` is disabled, and both sampler channels are muted. The firmware's `Digis Level=Medium` setting is retained but is not used for sampled playback. Mixer panning applies to HDMI and AV output, not the physical SID-TAP header. The built-in SID player's autoconfiguration can override mixer settings, so re-import the game configuration after using that player. See the [official mixer documentation](https://1541u-documentation.readthedocs.io/en/latest/config/audio_mixer.html).

The score runs at 50 Hz from CIA1 Timer A, independently of frame presentation. PAL and NTSC use different timer periods and SID frequency tables. The compact score bank is copied from REU into the resident audio engine before interrupts start. Mission Lost adds seven continuation phrases in a 224-byte resident constant table: eight distinct phrases play for 46.08 seconds before repeating, compared with the original 4.48-second loop. The audio engine's mutable state remains unchanged, and the audio interrupt performs no REU transfers.

## Build and verification

The target build uses Linux/WSL, the `mos` Rust toolchain, the `mos-c64-none` target, and LLVM-MOS SDK v22.5.0. `scripts/env.sh` exposes the installed compiler and target descriptions; `scripts/build.sh` runs host checks, bakes assets, cross-compiles, and packages the outputs. `MOS_CLANG`, `LLVM_MOS`, and `RUST_TARGET_PATH` can point to another compatible installation.

`target-c64/build.rs` assembles the native hardware and rendering helpers, disables additional LTO zero-page register allocation, selects the custom linker script, and emits a memory map. Four phase sections deliberately share an execution address, so the general linker overlap check is replaced by explicit linker assertions and the project's post-link checks. ELF symbols and relocations are preserved for emulator debugging, package extraction, and `tools/check-overlays.py`. That checker rejects cross-phase references, allows only the four named entry calls from `main`, validates ownership of compiler switch tables, and checks that the IRQ's relocation graph stays resident. Runtime-computed pointers and timing still require execution tests.

The native `memcpy.S` replaces the Rust compiler's larger memory-copy routine and its static scratch frame. It follows the LLVM-MOS C ABI and uses only caller-clobbered registers. `python3 tools/test-memcpy-kernel.py` runs 1,297 native 6502 simulator cases against an independent volatile byte-copy loop, covering empty copies, page boundaries, unaligned addresses, returned pointers, and source/destination guards. Its generated report lives under `target/memcpy-test`.

The other native kernels preserve host reference behavior: `sort.S` implements stable far-to-near radix ordering, `raster.S` advances prepared edges and paints flat spans, and `pack.S` chooses and emits VIC cell palettes. The compact sort and raster kernels share unused space in the resident table region, leaving the fixed overlay boundaries intact. Their simulator checks compare bytes against independent Rust fixtures and verify memory guards. Host geometry tests compare cached transforms with the original per-vertex arithmetic; sky-cache tests exercise every previous cell palette, repeated packing, two-bank occlusion recovery, and HUD eligibility boundaries. These correctness checks do not substitute for target timing measurements.

`python3 tools/test-raster-prep.py` separately compiles the production Rust edge-preparation helpers and division assembly for the MOS CPU. It compares all 28 prepared head/tail bytes against an independent integer reference for 1,108 triangles at four alignments, both with separate storage and with the command replaced in place: 8,864 preparations with guard checks. This closes the gap left by the row-walker test, whose inputs are already prepared on the host. Its report lives under `target/raster-prep-test`.

`python3 tools/test-corrupt-package.py` checks four startup rejection paths in the calibrated VICE companion: a damaged code directory, corrupt BOOT code, corrupt common assets, and mismatched PRG/REU checksums. It uses private copies under `target/corrupt-package-test`, verifies the error-screen text bytes, saves screenshots and logs, and confirms the input package hashes remain unchanged. It reads failure addresses from the matching ELF and chooses a free monitor port. This is emulator regression evidence, not physical C64 Ultimate acceptance.

Useful host checks are `cargo test --all-targets`, `cargo run -- sizes`, and `cargo run -- snapshot title`. A passing host test or valid linker map does not establish a frame-rate result on the machine. Hardware acceptance must check PAL and NTSC presentation timing, simultaneous stereo music and effects, REU transfers during play, stack guards, and full-mission restart/checkpoint behavior.

The bootstrap structure was informed by the neighboring `doomc64u` repository. `secondreality/src/output/cfg.rs` provided a local example of the Ultimate mixer and SID-address configuration keys. H.A.V.O.C. uses its own synthesis player and does not adopt Second Reality's digital-audio modes.
