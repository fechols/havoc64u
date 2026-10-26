# H.A.V.O.C. hardware and build notes

H.A.V.O.C. (Heavy Assault Vessel: Orbital Combat) targets the C64 Ultimate at 64 MHz with its built-in REU configured to 16 MB (16 MiB). The v0.1.5 package uses Ultimate Audio for digital music and built-in UltiSIDs for combat effects; the SID soundtrack has been removed. All twelve oscillators and four filters are dedicated to effects. All eighteen stages have their own recordings, and Mission Failed plays after losing all lives. No physical SID chips are required. Import `havoc.cfg`, load `havoc.reu` at REU address zero, then run `havoc.prg`. Keep all three files from the same build together. Historical 1 MiB, 8 MiB and v0.1.2 packages retain their original memory and soundtrack behavior when reproducing older results.

The configuration selects the C64U turbo registers, 64 MHz CPU speed, disabled badline timing, and quiet bus operation. The runtime programs the turbo register at startup and checks that the setting is available. An ordinary 1 MHz C64 is not a supported execution target.

## Startup splash

The PRG carries `assets/splash/havoc_splash.bin` in its existing `$3000–$71FF`
load-image gap: 8,000 bitmap bytes, 1,000 screen bytes, 1,000 color nibbles, and
one background byte. A temporary assembly initializer follows the payload.
After masking interrupts and mapping RAM/I/O, `.init.000` selects turbo and
copies the image to `$A000`, `$8000`, and `$D800`, then enables VIC bank 2 in
multicolor bitmap mode. This happens before BSS clearing, data initialization,
REU access, or Rust `main`. The PRG remains 38,913 bytes; its first overlay load
retires both the source image and its initializer. The splash cannot appear
before the external loader transfers control to the PRG.

BOOT preserves the visible image through code/package validation, asset setup,
and audio calibration. The first AUDIO call fills the initial music ring using
bounded decoder visits before returning; `AUDIO_READY` keeps the input queue
and simulation clock gated until playback starts. Later music calls retain
their existing work allowance. The effects-only emulator/benchmark backend
does not wait for the unsupported sampler. The first rendered frame overwrites
the splash band by band, and its sprite enables wait until frame completion.
Both images share background index 6, so changing to the title does not recolor
the unpainted remainder. Fatal startup errors still show the text diagnostic.

The offline converter, `tools/encode-splash.py`, preserves the source's full
composition and filters in linear light to 160×200 logical pixels. It tests all
455 local color triples per 4×8 cell using edge-weighted Oklab error. The default
`--method spatial` compares serpentine Floyd–Steinberg seeds at strengths 1,
0.75, 0.5, and 0, then refines the best decoded pixel pattern and local colors.
Its objective combines unfiltered edge-weighted Oklab error (25%) with Oklab
error after two scales of linear-light Gaussian mixing (50% and 25%). The
filters account for the wide logical pixels, truncate at image boundaries,
and renormalize there. Four pixel sweeps, one local-color replacement sweep,
and two further pixel sweeps consider legal recolors and adjacent swaps;
each accepted move strictly reduces the objective. Pixel stages stop early
when a complete sweep makes no improvement. All cells retain background 6
and at most three other hardware colors.

`--method legacy` reproduces the original exhaustive palettes and full-strength,
edge-attenuated diffusion byte for byte. Spatial refinement retains that payload
if it cannot improve the objective. This addresses the difference between
individual-pixel matching and perceived dither mixtures; a free-palette SOM is
not used. Both methods use the pinned palette from the
[Ultimate firmware default colors](https://github.com/GideonZ/1541ultimate/blob/master/software/u64/u64_config.cc).
This splash-specific palette does not change the gameplay quantizer or the
machine's saved palette settings. The committed
[640×480 preview](../assets/splash/havoc_splash.preview.png) is decoded from the
packed bytes, not the intermediate image.

Regeneration requires Python, Pillow, and NumPy:

```sh
python3 tools/encode-splash.py
python3 tools/encode-splash.py --check
python3 tools/test-splash-converter.py
```

For a reproducible decoded comparison, including title, ship, sky, and explosion
crops and separate raw/fine/coarse error measurements:

```sh
python3 tools/encode-splash.py --method legacy --output target/splash-quality/legacy
python3 tools/compare-splash.py --baseline target/splash-quality/legacy/havoc_splash.bin --candidate assets/splash/havoc_splash.bin
```

Comparison images and JSON measurements go under `target/splash-quality/comparison`.
The residual-texture measurement describes only source-flat neighbor pairs;
it is not a whole-image quality score. Spatial fidelity can improve while some
areas retain or gain visible dither texture. Inspect the decoded comparison
alongside the objective rather than treating its score as a display measurement.
For the committed artwork, spatial refinement reduces the combined error from
0.0107542693 to 0.0094767414 (11.88%); raw, fine, and coarse errors all decrease.
Conversion took about 29 seconds versus one second for legacy on the local
Python 3.14 / NumPy 2.5.1 / Pillow 12.3.0 environment. Review of the full frame
and crops preserves lettering, ship contours, and bright flame cores; the sky
remains visibly dithered. These measurements add no work to normal game builds
or C64 startup.

The JSON sidecar records the source/output hashes, palette, conversion settings,
dependency versions, and normalized hashes of both converter modules. Normal
target builds consume the committed binary and
run `python3 tools/encode-splash.py --verify-assets` to reject stale assets using
only the Python standard library; they do not need image-processing dependencies.
Linked startup, layout, and
overlay tests check exact display copies, the transient initializer's lifetime,
and rejection of references after its memory is reclaimed.

## Graphics and memory

Flying objects cast fixed-size, recognizable silhouettes directly downward onto
one known horizontal plane at world Y = -105. This is both the terrestrial floor
and the Space Carrier's flight deck. Named per-level switches in `src/game.rs`
enable or disable shadows. `Game::shadow_ground_y()`
keeps the enable rules: Lowland Approach through Carrier Assault and
Space Carrier are on; the other five orbital levels are off. Orbital crossings
and the ascent are off while scenery moves away from the fixed plane. The named
settings compile into three enable bytes without changing the checked stage
format. `Game::shadow_color()` selects black 0 for Space Carrier so silhouettes
remain visible against both gray deck shades. Other enabled levels and
terrestrial crossings retain C64 dark gray 11, defined as `SHADOW_COLOR` in
`src/shadow.rs`. Host previews and the native renderer use the same selection.
Birds, crossing transports, and meteors are background animations and cast no
ground shadows. The player and its projectiles retain their shadows during
terrestrial crossings.
Space Carrier's parked side hulls also cast no shadows: they sit outside the
flight deck with no surface beneath them. Player, enemy, boss, projectile, and
pickup shadows remain enabled.

`SMA6` retains nine 12-byte descriptors in each model bank's final 128 bytes:
template family, quarter-turn heading, local XZ offset and half extents derived
from the final quantized mesh. Craft, hull, diamond and rectangular templates
cover models, projectiles, pickups and orbital scenery. Their outline
coordinates use shifts and additions; each vertex receives the same
`-105 - camera_y` height before ordinary perspective projection. Scenery geometry,
slopes, deck edges, raised objects and occlusion do not affect the silhouette.
Caster centers at or below the plane are skipped. Shadows use one solid ink
per level without sampling or recoloring the surface beneath them.
They can blend into matching ground patches; overlap with scenery objects or
another shadow is intentional.

Ordinary geometry finishes first. Shadows use the actual unused entries of the
256-command array, with no separate shadow-command cap. Each caster is an
append-only transaction: insufficient command capacity rolls back its entire
silhouette while preserving every ordinary command. Ordinary commands keep
their stable depth order; shadow commands form a final suffix painted by the
ordinary rasterizer. They carry the exterior-terrain flag, so terrain-protected
geometry remains visible through the shadow pass. The Space Carrier protects
its entire Dreadnought assembly (hull, live turrets and exposed core); its deck
and guide strips also use exterior-terrain flags to prevent average-depth
overdraw. Parked ships retain ordinary ordering and sit outside the boss's
engine span. The blue boss armor separates it from the gray deck, with 53 world
units between the quantized hull bottom and floor. No receiver
records, parent links, plane normals, surface clipping or special shadow raster
path remain. The renderer stays 4,102 bytes, commands stay 16 bytes, Game uses
up to 768 bytes with the four-boss encounter state, and model banks stay 2,048 bytes. The 64-caster frame uses 711 bytes
on the native target (776 on the host), down from 5,768 native bytes. It remains
bounded to 1,024 bytes of caller-owned scratch, reused before raster preparation.

The native driver transfers one 14,170-byte SHADOW image after GEOMETRY. Its entry
generates enabled shadows, then sorts and prepares the complete command list.
Disabled levels collect no caster descriptors and load only the 6,018-byte
sorting, preparation and reticle-probe prefix; an audited guarded entry prevents
calls into the unloaded generation tail. The separate drawing overlay and wide-arithmetic helper are
gone. Preparation overwrites retired shadow scratch with raster heads, tails
and masks before PIXELS. All three native raster backends pass layout and
overlay audits. Current merged-build headroom is recorded with its validation below; earlier Story-only figures do not describe the combined audio build. Timings
and stress limits are in `docs/benchmarks.md`.

The aiming reticle is an unexpanded white high-resolution sprite: one native 320×200 pixel in a transparent 24×21 sprite. It marks the straight firing line at camera depth 512, using the same player position, elevated chase camera, and projection as the scene. It appears during Playing and Transition and hides during level-name splashes, pause, title, attract, respawn, game-over, and the entire victory medal reveal and results. The splash and reticle share the same distance-800 visibility boundary. Coordinates come from the completed rendered snapshot, then follow the bitmap's fine-scroll shake.

Only the player's hull and attached exhaust carry the preparation-only command
tag `0x80`, including black facets. The tag survives emission, near clipping and
sorting, then retires before rasterization. During ordinary edge preparation,
an 11-byte `PlayerPixelProbe` tests up to three distinct positions against those
faces: the new aim plus the active and pending prior sprite positions. It uses
the rasterizer's Q16 edges, row bounds and exclusive right edge, with a bounded
32-by-8-bit shift/add for each edge lookup. There is no silhouette bitmap,
second preparation pass or framebuffer readback on the target.

If a prior position intersects the new silhouette, or the new reticle is hidden,
PIXELS atomically cancels pending and active aiming before publishing any bitmap
band. Cancellation disables the borrowed sprite if the raster has switched it
to the reticle; during the top HUD phase it leaves sprite 0 enabled for its HUD
appearance. A clear new aim is published only after all bands complete. This
also prevents a stale dot from appearing over a moving ship during band updates.
See the [probe measurements](benchmarks.md#single-pixel-reticle-clipping-2026-10-24).

Its 64-byte image is initialized once at `$BFC0–$BFFF` (pointer `$FF`) and stays resident, with no per-frame REU transfer or pixel copy. This explicitly reserves the last 64 bytes of the unused bitmap tail, without requiring IRQ static-frame backups. Raster multiplexing reuses top HUD sprite 0: line 92 selects the reticle image and high-resolution mode, and line 251 restores its HUD image and multicolor mode. All eight HUD images remain intact. The middle interrupt takes a resident assembly fast path, while frame/input bookkeeping remains at line 251 and audio retains its independent timer. A late switch hides the reticle for that refresh and increments `RETICLE_MISSES`; supported PAL/NTSC validation requires zero misses. This adds no bitmap paint or scene triangles, although sprite fetches and interrupt register updates still have a cost.

Sprite-aware captures use `vic-sprites-v2` logical appearances to retain both the top HUD image and the reticle drawn by the same hardware sprite in one frame. The readers still accept v1 captures; bitmap-only dumps keep their existing format. Raw instantaneous registers remain available for diagnostics, while visual comparisons use the logical appearances and latched scroll.

The display is native VIC-II multicolor bitmap mode: 160×200 logical pixels drawn from the 16-color C64 palette. Each 4×8 logical-pixel cell has the global background colour plus three local colors; the background register shows the frame's clear colour (the sky on terrestrial stages), so unpainted pixels and ink of that colour cost no cell slot. Flat triangles paint two-bit colour codes straight into the cells; only a cell asked for a fourth other colour is quantized per pixel. There is no texture mapping.

The game draws into a single VIC bank. The top eight-row band is reserved for the GUI; the other twenty-four bands contain scenery, including the pixels behind the cockpit. Each band is rasterized, given any bitmap captions, and fully packed in scratch RAM before a bounded copy publishes its bitmap, screen and color bytes. Publication proceeds immediately unless the raster is near that band's fetch window; it waits locally when necessary, with no whole-frame v-sync wait. A frame still rolls down the picture as it is produced. Seven execution phases share one RAM window, with four ordinary enemy-profile SIM images, three boss-profile SIM images, and a terminal SIM image stored in REU. The SID effects player and interrupt handler remain resident; the DAC decoder runs in AUDIO. BOOT runs once; frames then run SIM, AUDIO music service when needed (every frame for digital playback), SCENERY, GEOMETRY, SHADOW generation/sorting/preparation, and PIXELS in order. Disabled scenes load only the ordinary sorting/preparation prefix of SHADOW. BOOT validates the resident SFX patch bank and digital catalog. During play, AUDIO refills the digital rings; it is never called by the interrupt. Startup validators and directory parsers remain in BOOT.

Eight unexpanded multicolor sprites form four floating cockpit readouts, with two adjacent images per corner. Each 24×21 logical-pixel area contains only text and functional bars: lettering and gauge outlines are white, filled gauge portions and warnings use color, and empty gauge interiors are gray. Every pixel outside the text and bars is fully transparent; there are no backing panels or decorative accents. The upper-left readout holds the six-digit score; upper-right holds lives, three bomb rectangles, and a compact purple boss meter when needed. Bomb rectangles have white outlines, purple centers when available, and gray centers when spent. Lower-left shows shield strength; lower-right shows charge and homing readiness. The level name appears in the section splash rather than a permanent strip. The images occupy exactly 512 bytes at `$8C00–$8DFF`, with sprite pointers `$30–$37` at `$83F8–$83FF`. Shared sprite colors are gray (`$D025=12`) and white (`$D026=1`); the inventory pair keeps its individual color purple even while a boss is active. Title, attract, and victory hide the cockpit, while their bitmap panels remain available.

A small displayed-value cache identifies dirty corners. PIXELS stages each changed pair in the retired workspace at `$8E00–$8E7F` with its functional color at `$8E80`, then writes only image bytes and color registers that actually differ. Unchanged corners keep their existing images without redraws. The publisher waits outside raster lines 52–91 for top corners and 212–253 for bottom corners, then masks interrupts and samples again before the bounded copy. The line-251 IRQ subtracts the bitmap's displacement from neutral fine scroll `(0,3)` from each sprite's fixed base, so bomb and hit shake move the instruments by the same distance in the opposite direction on both axes, without accumulating drift. `tools/test-hud-kernel.py` executes the assembled routines across 482 PAL/NTSC publication cases and 129 scroll samples, including IRQ races, raster rollover, dirty-byte checks, and counter wrap; its conservative model measures a maximum masked interval of 1.192 PAL lines, below the two-line budget.

Pressing F toggles a frame-rate readout in tenths within the upper-left sprite pair, and the space bar pauses the flight: simulation time stands still behind a double-height PAUSED caption while frames keep rendering, stale joystick samples are discarded and the simulation deadline follows the clock so resuming does not replay the missed ticks. PIXELS counts completed frames over windows of at least fifty 50 Hz audio ticks and scans keyboard rows 7 (Space, CTRL, Commodore), 5 (comma, period), 2 (F), 1 (S), and 0 (DEL) once per frame with the sampling interrupt masked. It then restores CIA 1 port A's low six pins to inputs and selects port 2's paddle lines with PA6/PA7. A column counts as a key only when it also reads high with no row selected, so a joystick in port 1 or an emulator driving port B cannot fake a press. The readout is off at startup and can appear by itself even while the cockpit is hidden; pausing is ignored outside Playing and Transition.

**Afterburner:** tap an independent second fire button on port 2, or Left Shift, for two seconds at twice the forward speed and 1.5 times the steering target speed, followed by four seconds of cooldown. Release does not cancel a burst; holding or pressing during cooldown cannot queue another. The B gauge below SHIELD drains while active and refills while cooling. Blue/white exhaust and the existing roll whoosh mark ignition. Weapons and enemy AI retain their normal 50 Hz cadence; collisions still hurt. Boost persists through scenic crossings, freezes during pause, and resets on death. Boss separation and stage-exit gates remain unchanged.

The joystick must expose **C64GS Fire 2 through POTX**, not duplicate the primary Fire signal or select Fire 3. Configure an ArcadeR's internal mode for C64 and its secondary selector for Fire 2. Left Shift provides the same action on a single-button stick. The raster IRQ reads POTX at `$D419` and scans Shift independently of rendering. CIA1 Timer B measures at least 1,024 Phi2 cycles after every keyboard scan before a new paddle value is trusted; this works at 64 MHz without a CPU delay loop. Unsettled reads retain the previous button state. Shift uses its raw matrix column so it works with joystick Down; arbitrary unrelated key combinations retain the C64's normal matrix ghosting limitation.

Boost adds bit `$20` to `InputSample`; digital joystick reads remain masked to `$1F`. Native smoke fixtures inject boost through `TEST_INPUT`, since VICE's `jpdb` interface only represents the five digital switches. `tools/test-input-kernel.py` separately checks the linked POT/Shift sampler and settling behavior. Control flags pack the burst timer and continuous extra-travel clock; the four-boss encounter uses a 768-byte Game allocation.

Press **S** on the title to toggle **Normal (3 lives)** or **Story (99 lives)**, then press fire to start. S only changes the selection and refreshes the title inactivity timer; SIM starts the run on the usual fresh fire edge. The selection persists through attract mode, stage changes, deaths, retries, and return to the title, until S changes it again or the program restarts. A cleared boss stage restores one life up to the selected starting count. The native Game stays within its 768-byte allocation. Lives use two digits in the sprite HUD and respawn caption, with a blank tens digit below ten. S shares bit 3 of the existing raw-key latch; holding it, changing scenes, or clearing joystick interference cannot create another press.

The Story keyboard check passes 235 fixtures and 695 linked scanner calls, including all mode guards, shifted S, held keys, both joystick ports, and simultaneous shortcuts. `tools/test-story-mode.py` checks PAL and NTSC launches, real hostile-shot deaths, checkpoint respawns and retries: Normal 3, Story 99, death 98, respawn 98, retry 99, then Normal 3 again. It seeds the selection and encounter through ELF-derived monitor fields; physical joystick FIRE and production SIM perform the transitions. The separate keyboard harness verifies S against a digital CIA model. VICE uses the explicit effects-only backend because it lacks the Ultimate Audio sampler. Original reports are under `target/story-mode/{pal,ntsc}`; the merged-build validation below records fresh checks.

Secret soundtrack player: press **CTRL + C= (Commodore) + DEL** on the title screen or during attract mode to toggle **the entire soundtrack on repeat**, even before winning. The playlist plays Hammond Rocker, all eighteen stage songs in campaign order, Mission Failed, then Galactic Dash, and automatically returns to Hammond Rocker. Each full song finishes before the next begins. In soundtrack mode, **comma (,)** selects the previous song and **period (.)** the next; both directions wrap and new songs start from the beginning. The latest selection wins; returning to the still-playing song cancels a pending switch without restarting it. Held keys do not repeat. The scene, camera, and attract timer continue unchanged.

The selected mode persists across scene changes and new runs. Gameplay and result screens retain their normal music, and ignore these shortcuts. Turning soundtrack mode off restores Hammond Rocker; reloading starts with title music. Completing the campaign still selects Galactic Dash and keeps it playing on return to the title. The toggle remains usable afterward. The screen shows a steady cyan **NOW PLAYING SOUNDTRACK...**, **NOW PLAYING TITLE MUSIC...**, or, for the title-only victory reward, **NOW PLAYING VICTORY MUSIC...**. The title uses its top strip; attract mode places the status directly below its section name, clear of the optional FPS readout. The shortcuts share the existing key-edge latch, and the AUDIO overlay reuses its saved sampler calibration when switching songs.

Player damage reuses the bomb's resident impulse sequencer with a lighter profile: a `Hit` presentation event (outranked by a pending bomb, replaced by a reset) runs 240 ms of two-pixel horizontal kicks with at most one pixel of vertical motion and an 80 ms red border, packed into the same single byte as the bomb countdown with bit 7 selecting the profile. A hit never shortens a bomb shake in progress.

Damaged enemies and whole bosses turn white for 200 ms, then recover in their normal colors for at least 200 ms before another damaging hit can start a flash. Damage during either phase still applies but never extends or queues feedback. GEOMETRY prepares these phases once per frame using the 20 ms simulation clock: a new hit waits for its first white frame, and catch-up can advance only one phase, so it cannot skip the normal-color interval. Slow rendering may extend either phase; pausing stops its clock. Feedback uses one byte per actor, one for the boss, and a shared frame tick without changing the 16-byte actor assembly layout. Meshes and distant proxies share the color override.

Bomb impacts use the VIC fine-scroll and border registers, adding no triangles or bitmap copies. A one-byte simulation event records a spent bomb independently of inventory rewards and the audio queue. SIM drains the latest bomb/reset event after catch-up and updates a resident sequencer with interrupts masked. Its 32-tick countdown runs on the existing 50 Hz clock; the raster interrupt at line 251 applies sixteen 40 ms samples. Horizontal kicks reach six physical pixels and vertical motion reaches three pixels either side of the normal Y-scroll value, tapering to rest over 640 ms. The border spends 80 ms each at white, yellow, orange, and red, then returns to black. Frame completion preserves active Y-scroll, and register writes preserve display-enable and bitmap-mode bits while keeping the raster comparison below line 256. Respawn and scene resets cancel the effect; scenic-flight seams preserve it, and a finishing bomb can continue through the results screen. Bitmap-only frame dumps omit presentation registers, so shake effects are checked with native traces and VICE captures; sprite-aware captures also record the independent cockpit images and VIC state.

The bomb build passes 171 host tests per SID profile, with one existing fixture export ignored, and both production packages pass the PAL/NTSC weapon checks. Physical joystick gestures produce every shake phase, motion between frame publications, active retriggering, and neutral expiry; empty-inventory gestures produce no impulse. The observed countdowns last 623-639 ms, within the expected 20 ms clock quantization, with register updates following at the next raster interrupt. The two-SID ordinary-frame timing sample remains 5.493 FPS PAL / 5.766 FPS NTSC before and after the change. These short emulator samples do not establish physical-machine performance. Reports are under `target/vice/bomb-{2sid,4sid}-{pal,ntsc}`; the two-SID PAL run also covers controls, scenic streaming, and results-screen restarts.

Production companion screenshots have an existing screen/color-fetch defect with `$D031` bit 7 set: its matrix-fetch prefetch counter stays at four while badline stalls are disabled. Baseline captures made before the bomb change show the same corruption. Decoded bitmap/screen/color bytes and the VIC register traces validate rendering data and presentation timing, but final visual acceptance remains a physical C64 Ultimate check. The companion limitation is documented in the neighboring `doomc64u/docs/dual-image-game.md`.

| Address | Use |
|---|---|
| `$0002–$0021` | LLVM-MOS imaginary registers |
| `$00D0–$00E0` | Row walker zero page during PIXELS: the current triangle band's four Q16 edge values and its row count (`target-c64/asm/raster.S`); the compiler's zero-page region `$0022–$00EF` is otherwise empty (`tools/check-layout.py` asserts it) |
| `$00F0–$00FF` | Cell writer zero page: the span being painted and its hot temporaries; `$FE` the span colour's key (colour XOR the clear colour), `$FF` the band's clear colour, read by the quantizer as its background (`target-c64/asm/cells.S`, `pack.S`) |
| `$0200–$05FF` | Resident globals and compiler static scratch frames; all end below `$0600` |
| `$0600–$07FF` | First 512-byte mission-segment buffer (the retired default screen page) |
| `$0801–$2FFF` | BASIC startup, resident code (including the assembly effects engine `target-c64/asm/sfx.S`), data initializers; the linker prevents growth into the overlay window |
| `$3000–$71FF` | BOOT, SIM, GEOMETRY, SHADOW, PIXELS, AUDIO, or SCENERY code overlay; render prefixes end below `$6F20` |
| `$6F20–$71FF` | Shared projection helper during GEOMETRY/SHADOW; reloaded PIXELS helper code and ambient-particle frame afterward |
| `$7200–$74FF` | Permanent read-only SFX pitches, patches and motion programs, 764 bytes before bank padding (768-byte checksummed bank) |
| `$7500–$75FF` | 256-byte IRQ software stack; the matching native report records its high-water mark |
| `$7600–$77FF` | Sorted command order written by the depth sort (`$7600–$76FF`, one command index per sorted position; BOOT's directory scratch before any frame), screen staging (`$7700`), cell overflow flags (`$7728`), cover-clean marker (`$7750`), then preparation, row-walker and quantizer scratch from `$77A0` |
| `$7800–$7FFF` | Resident constants and bounded SFX queue helper |
| `$8000` / `$A000` | The single visible screen / bitmap (VIC bank 2), updated in place band by band |
| `$8400–$8BFF` | Current level's model bank and enemy descriptors |
| `$8C00–$8DFF` | Eight persistent 64-byte multicolor HUD sprite images |
| `$8E00–$91FF` | 64-entry projected-vertex cache, then depth-sort scratch after all projection |
| `$8E00–$92FF` | Pixel form of overflowed cells during PIXELS; the workspace also holds startup checks, SIM/SCENERY helpers, BOOT music directory scratch, and one staged HUD corner in separate lifetimes |
| `$9300–$93FF` | GEOMETRY kernel scratch (matrix, rotation, projection, emit, replay, mesh, near clip at `$93D0`); PIXELS reuses `$9300–$9328` for the cell writer's rare paths and HUD-seen flag; SIM reuses `$9340–$937F` for the missile guidance kernel |
| `$9400–$96F7` | Cell writer band state during PIXELS: 8×40 two-bit code bytes, 8×40 coverage nibbles, three 40-byte palette slots holding keys (colour XOR the clear colour; 0 = free, `$FF` ×3 marks a cell that lives in the pixel band) |
| `$9700–$98FF` | Second 512-byte mission-segment buffer |
| `$9900–$9FFF` | Resident math/palette tables and compact kernels: the depth sort, the Q8 multiply and the bounded division |
| `$BF40–$BFFF` | Free (192 bytes above the bitmap; formerly the IRQ static-frame backup tail) |
| `$C000–$C3FF` | Quarter-square table for the exact Q8 multiply, built by BOOT (former screen B) |
| `$C400–$C7FF` | Main software stack |
| `$C800–$CAFF` | Game state (768-byte allocation) |
| `$CB00–$CCFF` | Free (512 bytes; formerly part of the IRQ static-frame backup) |
| `$CD00–$CDFF` | Quad-SID effects state, 200 bytes within the 256-byte reservation |
| `$CE00–$CFAF` | Permanent foreground DAC state and handoff fade (432-byte reservation; current size is reported by the matching ELF) |
| `$CFFA–$CFFF` / `$D000–$DFFF` | Renderer header / triangle commands beneath I/O, left in emission order for the whole frame |
| `$D000–$D9FF` | Optional span storage in retired triangle commands after raster preparation |
| `$E000–$E7FF` | Additional SCENERY authoring helpers, retired before raster preparation |
| `$E000–$F7FF` | Additional SIM helpers, reloaded before SIM and retired before SCENERY resets shadow state |
| `$E800–$FF1F` | Reserved shadow workspace before raster preparation; the 64-caster frame uses at most the first 1,024 bytes |
| `$E000–$EBFF` | Twelve-byte persistent raster tails during PIXELS, after shadow work retires |
| `$EC00–$EF1F` | Twenty-five ordered triangle masks, 32 bytes per band |
| `$EF20–$FF1F` | Sixteen-byte persistent raster heads, one per sorted position |
| `$FF20–$FFF9` | Free RAM; the CPU vectors follow at `$FFFA` |

The PRG spans `$0801-$9FFF`, including resident constants and the temporary splash; its load image excludes phase overlays. BOOT's initial 3,072-byte common-bank validation temporarily writes `$8400-$8FFF`, so sprite images are initialized only after it finishes; later model loads end at `$8BFF`. The CPU can use the shared workspace across `$9000` normally; the VIC character-ROM window is avoided by keeping every sprite image below that address. Interrupt-reachable static frames are forbidden by the linked audit. Package v17 preserves common data and encounter allocations, moves eighteen model banks to `$00B000-$013FFF`, and packs the 26 code images in directory order from `$014000`:

| Code images, in order | REU bases, respectively |
| --- | --- |
| BOOT, lowlands SIM, GEOMETRY | `$014000`, `$017000`, `$01B800` |
| PIXELS, AUDIO, wildlife SCENERY | `$020000`, `$024800`, `$028000` |
| Canyon, fortress, carrier SIM | `$02C800`, `$031000`, `$035800` |
| Victory GEOMETRY, terminal SIM | `$03A000`, `$03D800` |
| Warden, citadel, flagship SIM | `$042000`, `$046800`, `$04B000` |
| Transport, meteors, SIM helpers | `$04F800`, `$054000`, `$058800` |
| Ascent, asteroid, moon SCENERY | `$05A800`, `$05F000`, `$063800` |
| Graveyard, shipyard SCENERY | `$068000`, `$06C800` |
| Space carrier, gate SCENERY | `$071000`, `$075800` |
| SHADOW, SCENERY helpers | `$07A000`, `$07D800` |

Code reservations end at `$07E800`, leaving 6 KiB before music at `$080000`. Reservations are `$4800` bytes except BOOT (`$3000`), AUDIO/victory/SHADOW (`$3800`), SIM helpers (`$2000`), and SCENERY helpers (`$1000`); every actual execution image remains bounded by `$4200`. Existing logical overlay IDs retain their values: ID 31 is SHADOW, ID 32 is unused, and ID 33 holds SCENERY helpers. A linker-authored, 34-byte table in resident `.tables` maps IDs to REU bases in 2 KiB units. The loader carries source addresses across 64 KiB banks while keeping every DMA at most 1,024 bytes. PIXELS shares banks with other images; its transfer audit checks linked ranges and phase markers. OVL7 checksums all 26 complete images, including padding and helper portions. The startup-only byte/data copy routines share `.splash_init` and retire before any overlay load, freeing 60 bytes of resident code.

The SIM helper image at logical ID 23 / REU `$058800` has two parts: its prefix loads into `$8E00-$92FF`, and the extra code at REU `$058D00` loads into `$E000-$F7FF`. The combined checksum covers the prefix, padding through offset `$500`, and the extra code; runtime uses each part's linked length. Both parts reload before every SIM profile. This space contains only retired raster and shadow data during SIM; SCENERY resets its shadow frame after SIM finishes. The four-boss logic therefore fits without increasing the main `$3000-$71FF` overlay window or reducing renderer capacity. The corruption harness flips a byte at the extra code's REU start and requires startup to report `REU CODE CHECKSUM FAILED`.

The build produces an exactly 16,777,216-byte REU image. The DMC1 catalog and aligned DLT5 assets share `$080000-$FDFFFF`, a 16,121,856-byte music budget. The decoder writes two 64 KiB playback slots at `$FE0000-$FFFFFF`; these start zero-filled in the package. Game state remains in C64 RAM. The hardware timing probe writes to its explicit reservation at `$000C00-$000FFF`. The linked layout leaves 9,856 unreserved bytes. Reclaiming 512 KiB increases codebook storage from 1,225,632 to 1,749,920 bytes across all 21 tracks, shortening average adaptation intervals from about 29.9 to 20.9 ms. The packed soundtrack leaves four bytes unused in its own region.

The historical 1 MiB layout's same-source comparison against a 16 MiB build passes eighteen four-SID PAL fixtures with identical game state, geometry and native pixels. Measured frame-time changes range from -0.209% to +0.177% in calibrated VICE, with one sample per fixture; these are not physical-machine measurements. `target/reu-1m/performance-summary.json` and its source manifest record the comparison. Benchmark-only builds keep the unchanged segment-checksum helper in resident RAM to accommodate their SIM instrumentation; production retains its existing SIM placement.

The earlier dual-profile 1 MiB packages passed 311 host tests, 119 layout rejection fixtures, 64 overlay rejection fixtures and 127 package-corruption cases per sound profile. Each profile also passed 43 streaming/respawn/restart scenarios and all 21 complete music loops under both PAL and NTSC with a 1,024 KiB emulated REU and no music underruns. `target/reu-1m/validation-summary.json` records those historical package hashes, reports and verified copies to the hardware drive.

On 2026-10-21 the user reported that the 1 MiB build looks and sounds great on physical hardware. The sound profile, video standard and campaign coverage were not specified.

The frozen historical 16 MiB `target/offplanet-release/` images contain 496,349 bytes of serialized payload for two SIDs and 580,001 bytes for four SIDs. This counts 256 bytes of package/model/code directories, 3,072 common bytes, thirty-six 2,048-byte encounter/model banks, the twenty-four image lengths recorded by `OVL7`, and the `SMU3` bank's recorded total length. It excludes allocation padding without treating zero-valued payload bytes as free space.

Each composed SIM image has the same common prefix and one profile tail. The audited `encounter_step(Game*, AssetBank*)` entry owns actor movement, attacks, and the optional boss update. Shared code handles controls, progression, spawning, collisions, projectiles, and pickups. Stage identity is independent of this code profile: changing stages always refreshes the encounter metadata and model bank even when the next stage shares code. Native SIM reloads every frame because later phases reuse the window. A catch-up stage/profile change returns to the resident loader before another tick or render, preserving its existing fixed deadline. Enemy updates remain suspended during scenic flights.

SIM also loads a checked helper image from REU `$058800` into `$8E00–$92FF`, a 1,280-byte execution allowance. This reuses retired startup-check space and the later geometry/pixel workspace; it does not replace the model cache, sprite images, or mission buffers. The resident loader restores it in two transfers before every SIM entry because later rendering phases overwrite that RAM. All general DMA transfers are capped at 1,024 bytes to bound audio IRQ latency. The IRQ never calls the helper image.

The 128-byte `OVL7` directory at REU `$000100` records twenty-six checked code images: existing phases, all eight SIM compositions, victory GEOMETRY, ten composed scenery images, the SIM helper, SHADOW and the SCENERY helper. The ascent and six orbital scenery records occupy bytes 88-115; SIM helper fields remain at 84-87, SHADOW uses 116-119, and SCENERY helpers use 58-61. The directory also authenticates SID configuration and the PRG, constants and startup code. SIM images retain the common padded size for startup validation and the eight-entry `__sim_load_sizes` table for shorter runtime transfers. Boss kinds 4 and 5 reuse SIM profiles 6 and 5; terminal profile 7 remains distinct. The packer rejects overlapping allocations and images beyond the 16 MiB REU. Build the PRG and REU together.

Current package ID `$534D1100` retains outward-wound `SMA6` models with baked shadow descriptors in the 16 MiB DAC-music and quad-SID-effects layout. It retains eighteen 2,048-byte model banks at `$00B000-$013FFF`: each mesh descriptor keeps its vertex/face offsets and counts, coordinate shift (byte 6, 0–3), and a reserved zero byte (byte 7). Vertices remain three `i8` bytes in units of `1 << shift`. Every regular enemy, the player, the pickups and the medal fit shift 0 exactly; ordinary slot-4 hulls use shift 1 (their doubled coordinates are even, so they are exact too) and only the Dreadnought and Gate Guardian need shift 2. Faces keep three indices and a colour. Every triangle is backface culled; solid models have outward surfaces and thin sheets have explicitly authored reverse faces where both sides must be visible. Older model magic and nonzero reserved descriptor bytes are rejected. The 64-byte `MDL2` directory at `$000040` retains version 2: magic at 0-3, stage count at 4, version at 5, bank size at 6, REU base at 8, eighteen checksums at 12-47, zero padding at 48-61, and checksum at 62. BOOT validates every bank, leaving stage 0 in the existing `$8400-$8BFF` cache. A stage change checks and copies the incoming bank in two 1,024-byte transfers. Same-stage frames and respawns retain the cache; model views never survive replacement.

The 64-byte main header contains eighteen encounter checksums at 20-55, zero padding at 56-61, and its checksum at 62. Eighteen 2,048-byte encounter allocations occupy `$002000-$00AFFF`; only their checked 512-byte prefixes enter the two gameplay buffers. `SMS2` keeps spawn records at byte 16 and the 62-record maximum. Bytes 8-15 hold the unchanged eight-byte `StageInfo`: enemy profile, music cue, crossing profile, boss kind, current/next/previous themes and one reserved zero byte. Complete campaign authoring remains host-only. Orbital encounters use ordinary movement profiles 0-3 and explicit three-dimensional hazard positions; ROCK flag bit 1 (`SMALL_DEBRIS = 2`) marks a shootable fragment. Unflagged rocks resist guns and missiles. A bomb destroys either size of active orbital ROCK, including wreckage, for 25 points without advancing enemy-kill rewards.

`SMA6` retains nine mesh descriptors at bytes 16–87 and three eight-byte enemy descriptors at 88–111. Header bytes 5, 6, and 7 identify stage, active enemy count, and boss kind. Ordinary stages expose three enemy slots; boss stages expose two escorts and reserve mesh slot 3 for boss side units. Mesh slot 4 contains the crawler, reactor, carrier hull, Dreadnought or Gate Guardian ring; slot 8 contains the 70-vertex / 116-triangle medal, including its bronze cockpit and twin engine relief. The tightest mesh payload ends at byte 1,565, below the shadow-descriptor boundary at byte 1,920. The player and pickup models remain shared; crystal and volcanic stages customize obstacle meshes. The 3,072-byte common bank contains the initial models and score directory. Runtime encounter fetches checksum their previously validated 512-byte prefixes.

Combat meshes are baked at their final size so the C64 keeps identity transforms. Regular enemies (slots 1–3 in ordinary stages, slots 1–2 in boss stages) use `assets::enemy_scaled`, three halves of the authored coordinates with truncation toward zero; their hit, ram and center descriptors scale the same way (shot radius 28, ram radius 27). The boss hull in slot 4 and the boss side units in slot 3 of boss stages keep `assets::BOSS_SCALE` (2×), as do the boss layout constants. Ground guns keep their −8 base through the pivot. Actors and pickups are no longer removed in front of the camera: `game::ACTOR_RETIRE_Z` keeps every actor alive and submitted until the deepest actor mesh is entirely behind the near plane, where the renderer rejects every face; `tests/assets.rs` checks that invariant against every stage's model bank. Flat pickups and hostile shots retire at the near plane itself. That plane (`render::NEAR`) now sits 8 units from the eye instead of 32: a vertex is on-screen only when |x| < z and |y| < 0.75 z, so geometry can be cut visibly only inside that small pyramid, and passing ships leave the screen edges before they reach it. Every actor, terrain (|x| ≤ 640) and scenery coordinate still projects inside the ±8191 clamp at that depth, and `projection::project_uncached` bounds |y| to 30,000 at unshifted depths, where larger values already saturate, so the i32 vertical product cannot overflow.

SCENERY lays out the terrestrial prefix first, followed by shared space helpers and a wildlife, transport, meteors, ascent, or orbital environment tail. Ordinary terrestrial frames transfer only `__phase_scenery_terrain_size`, omitting both space helpers and decoration tails. A terrestrial crossing additionally loads its selected decoration at the fixed `scenic_decoration` address, leaving the intervening space-helper region unloaded. Ascent and orbital frames load the selected full composition. The orbital tail also draws its outgoing crossing, while the existing transition state and sound-effect queue handle ascent. The complete padded images remain checked at startup. GEOMETRY retains its player/projectile/effects prefix and combat or medal drawing. The relocation audit accepts only the reviewed opaque entries and rejects cross-profile references. Every borrowed asset view ends before its DMA destination is reused.

Victory reuses the existing mode with a 400-tick reveal counter. The first 100 ticks freeze and darken the ship and battlefield; medal geometry is appended after color remapping so it stays gold. Once black, scenery and ship emission stop. The alternate GEOMETRY tail replaces combat drawing. The terminal SIM tail handles title/menu changes, respawning, scenic crossings, Victory and GameOver; ordinary attract ticks use the scene's enemy profile, returning to terminal SIM for dismissal or the next preview. Mode/profile changes return to the resident loader before another catch-up tick. The medal shares the ordinary mesh-transform instantiation through a camera-neutral scene budget; it does not duplicate that code or enlarge its vertex cache. PIXELS paints all 25 bands and shows only the final captions after tick 400. The complete fading scene uses at most 160 triangles, including the detailed medal.

PIXELS uses a split image: its prefix may occupy `$3000-$6F1F`, while a small helper tail occupies `$6F20-$71FF`. The existing page-4 `OVL7` record checksums the prefix, zero-filled gap, and tail as one image. Runtime loads only the prefix, then maps I/O in to DMA the tail from `$043F20`, restoring raster access before drawing. The tail contains centered text, label lookup and sprite-meter helpers, and a regenerated 16-byte particle frame. Neither transfer overwrites the prepared edge or mask workspace; no additional resident state is allocated.

Levels 13–18 draw eight staggered gray pixels as a forward-motion cue. Their radial positions use a continuous travel clock: one increment per 50 Hz simulation tick, plus one while boosting. Crossings and PAL/NTSC frame rates do not reset their phase. They are painted after clearing and before triangles, allowing black hull surfaces and the HUD to occlude them. They add no actors, collisions, or geometry commands. Space Carrier keeps three alternating hull rows and retires each only after its rear geometry has passed the near plane.

Checked crossing metadata selects wildlife, cargo transport, meteors, ascent or orbital flight. The original three decorations retain their existing bounded geometry; ascent raises the view from the carrier coast into the starfield, and orbital crossings retain the active space environment. All scenery must fit the 80-face transition reservation. The common SCENERY prefix selects sound cues after simulation catch-up; foreground Runtime owns its small scene/time history. A frame submits only the newest still-valid cue per lane, preventing missed frames from replaying old bursts. Distant scenery has no collision state; authored ROCK actors supply the visible hazards.

Cue publication and scene-only cancellation briefly mask interrupts and update audio RAM without touching SID registers, even while I/O is banked out for scenery. The resident 50 Hz effects player owns each allocated chip's three voices and filter. Scenic effects outrank basic gunfire and yield to every higher player-effect tier; digital music and mixer settings are untouched. Leaving a crossing removes pending scenic cues and ends active scenic groups on the next audio tick while retaining player effects. No IRQ code calls the scenery overlay.

The pre-expansion production baseline used 609 of 640 Game bytes, 16,241 of 16,896 composed SIM bytes, 13,302 SCENERY bytes, 15,888 combat GEOMETRY bytes, 15,798 victory GEOMETRY bytes and 12,245 of 12,288 PIXELS bytes. Its helper used 1,207 of 1,280 bytes, its tables 1,777 of 1,792, and resident headroom was 81/69 bytes for two/four SIDs. These are historical baseline figures, not orbital-build results. Current authoritative sizes are generated in each package's `memory-map.txt`; linker and post-link checks enforce every region. New label strings are packed in constant RAM with byte-offset tables in PIXELS, avoiding eighteen four-byte slice descriptors.

The frozen original orbital release maps retain the 609-byte Game state and 12,234-byte PIXELS image. Composed SIM uses 16,810 bytes, combat GEOMETRY 16,680 bytes, SCENERY 16,709 bytes, and SIM helpers 1,260 bytes. The terrestrial SCENERY transfer is 10,726 bytes, compared with its complete 13,591-byte shared prefix. Runtime AUDIO transfers are 1,117/1,127 bytes for two/four SIDs; startup additionally loads 1,961/1,982 bytes of validation code and data. Resident code headroom is 139/127 bytes, constants occupy 1,938/1,958 of 2,048 bytes, and compiler scratch ends at `$06BF`, below the four-SID page at `$0720`. These are historical allocation and transfer sizes, not frame-rate measurements; rebuilt package maps include the later balance and particle changes.

The balance and space-presentation builds retain the 609-byte Game state and 524-byte compiler scratch. Their PIXELS prefix uses 12,180 bytes and its helper tail 643 bytes, leaving 108 and 93 bytes respectively. The checksummed composition is 16,803 bytes, including the gap omitted by runtime DMA. Production SIM uses 16,793 bytes, SCENERY 16,802 bytes and combat GEOMETRY 16,680 bytes; resident headroom is 164/152 bytes for two/four SIDs. The four-SID instrumented build fits with 25 resident bytes and 33 SIM bytes spare. Shared collision and control-reset helpers reside in SIM, while the phase-independent audio-event dequeue resides in resident code; these placement changes preserve their function bodies.

The post-link audit checks all twenty-four startup `__code_sizes` entries and all eight runtime `__sim_load_sizes` entries against linked image sizes. Negative fixtures shorten and lengthen table entries. Native corruption checks repair the directory checksum after changing stored lengths, ensuring startup rejects authenticated but incorrect code sizes.

The balance/presentation validation is recorded in `target/balance-validation/`. Both host feature suites pass, including the eighteen-stage input-driven campaign. Each of the four native SID/video combinations passes 67 control, weapon, and streaming scenarios. Both profiles pass 71 layout and 64 overlay rejection fixtures plus startup corruption checks covering the split PIXELS gap and helpers. Fresh four-SID PAL/NTSC runs each pass 64 timing fixtures with exact DMA-range checks; 659 PAL cache-history frames match forced repainting. Native moving-scene captures contain 91 frames with no geometry drops, input overflows, or music underruns. These are emulator results; the earlier physical-hardware playthrough remains a separate result.

The diamond and shield pickup share one planar emitter with 15 bytes of face selectors, saving 623 GEOMETRY bytes while preserving triangle order and coordinates. Terrain side selection uses signs instead of general multiplication. Both changes reduce per-frame work without changing the geometry or native pixels; the matched timing results are in [benchmarks.md](benchmarks.md#campaign-expansion-final-2026-10-18).

Exact precomputed PAL/NTSC note-frequency tables reduce resident IRQ code without changing any note's pitch. The shared swept-depth collision helper uses 92 bytes of assembly in the SIM helper image instead of the compiler's 127-byte implementation. Its independent native simulator test passes 6,658 cases through the production C ABI, including signed limits and wrapping interval bounds; results are in `target/game-math-test/report.json`. This is a code-size and kernel-correctness result, not a campaign frame-rate measurement. No title-specific RAM is reclaimed: title rendering already uses PIXELS. The code loader preserves 1,024-byte DMA chunks and specializes address calculations for the fixed code window; general asset transfers retain their range checks.

The gameplay arithmetic optimization removed the linked 32-bit multiply and divide helpers; digital playback still uses 32-bit positions and clock arithmetic. The millisecond clock (`CLOCK_MS`, `InputSample::ms`, the `Controls` release and tap instants, `Runtime::sim_ms`) is 16 bits and wraps every 65.5 s: every window it serves is under two seconds, `Controls::sample` holds the release instant within the two-second charge window and disarms a tap older than the 500 ms double-tap window, and a paused frame still samples the held buttons once (the clock keeps running while the simulation stands still), so no remembered instant can look recent again after a wrap; `tools/bench-native.py` writes the two-byte clock and `tools/vice-smoke.py` reads two-byte sample stamps. `Hud::distance` is the section-local distance (`stage_ticks × 8`, at most 30,000), the only form the HUD uses; the monitor-visible `GAME_DISTANCE` keeps the campaign total as `(section × 1,875) << 4 + local`. The HUD bars, the victory reveal's easing (`remaining × 64 / 100`), the wildlife slope (the bounded division's Q8 fraction times a Q8 product) and the music-kernel image offsets (`cue × slot` fits 16 bits) use 16-bit arithmetic. The 380-byte `__udivsi3`, 219-byte `__divsi3` and 103-byte `__mulsi3` compiler helpers are no longer linked; with the smaller clock code (`computed_charge` 202 → 105 bytes, `Game::hud` 533 → 309, the IRQ body −44) resident code shrank from 10,018 to 8,844 bytes, the SIM prefix by 277 bytes, the PIXELS prefix by 147 and `Game` to 621 bytes. The score stays 32-bit: a full run's boss bonuses alone exceed 65,535, and its adds and digit conversion link no helpers.

Startup disables both ROMs before clearing BSS. The custom linker discards the SDK's `.init.250` PETSCII case-mode hook because it calls KERNAL through vector RAM now owned by game globals. The game's data-copy hook uses `.init.251`; the build audit verifies that startup calls only BSS clearing, data copying, and `main`.

A fatal startup or runtime error stops playback and displays a text diagnostic in screen A. This terminal error path restores normal VIC badline timing with `$D031=$0F`, allowing the character display to refresh normally; gameplay uses the configured disabled-badline mode.

SCENERY, GEOMETRY, and PIXELS bank out I/O to access the triangle command array. SCENERY writes up to 96 raw, camera-space records of 20 bytes each into its existing tail at `$D880` through `$DFFF`; no additional resident staging allocation is needed. Each record holds three signed 16-bit XYZ vertices, color, and flags. GEOMETRY reads a record before projecting it, checks that emitted commands cannot overwrite unread records, and then adds foreground geometry. It rotates the projected scenery prefix behind the foreground before stable sorting, preserving the host renderer's foreground-first order at equal depth. The foreground allowance is `min(174, 256 - projected scenery count)`. Both scenery and foreground counts include only emitted commands after near-plane clipping, backface culling and screen rejection; one source triangle can emit two commands.

Foreground objects are append-only transactions. A complete mesh is attempted before its emitted count is checked. If it exceeds the logical allowance or fills the physical command array, rollback restores both the command count and dropped counter. Actors then attempt a complete two-triangle proxy, reserving space only for later active actors. Added reverse faces consume storage and face-processing time but no command or raster capacity when culled. The 16-byte command format and 256-command allocation are unchanged; metadata retains only sky and terrain-coverage flags, with no sidedness or orientation bit. Raster preparation starts from the guaranteed positive winding and adjusts it for vertex-sort parity.

GEOMETRY clears and uses a 640-byte vertex cache inside the color-band workspace at `$8E00`. Every triangle reaches the resident renderer through `fast_clip_triangle` (`target-c64/asm/emit.S`, the native body of the renderer's `GeometrySink::triangle`): corners all at or behind the near plane go to `fast_emit_triangle`, corners all in front are dropped, and a mixed triangle is clipped against z = 8 exactly like the host's `near_intersection` (Q15 interpolation by the bounded division, the 17-bit coordinate difference times t through quarter-square partials, the arithmetic shift) into a three- or four-corner polygon. `fast_emit_triangle` looks each corner up in the cache by a byte mix of its six coordinate bytes (the cache is a memo keyed by the whole point, so the index need not match the host's arithmetic hash), projects misses in place, culls by the signed area, rejects triangles off one screen edge, computes the depth third without a division (`85 h + (h + l) / 3` with a 256-entry thirds table) and writes the 16-byte command. Once all vertices are projected, the stable depth sort (`fast_depth_sort`, `target-c64/asm/sort.S`) reuses the 1 KiB at `$8E00–$91FF` for the two key bytes of every command, an intermediate index list and 256 bucket counts: two least-significant-digit counting passes over the byte key `!(depth ^ $8000)`, scattered from the last command down so equal depths keep their order. It handles all 256 commands and does not move them: the commands stay in emission order and the sorted order — one command index per position — goes to `$7600–$76FF`, which PIXELS reads while preparing the raster heads. Ties follow the logical order `scenery_last` records (the foreground staged after the scenery first, then the scenery prefix, then anything added later, as the host's physical rotation yields); `tools/bench-native.py` reads the commands through the order so the geometry hash is unchanged. Mesh transforms calculate each active axis's sine and cosine once per object and skip identity scaling, preserving the original separate Q8 products and rounding. No projection-cache references survive sorting, and the IRQ never uses this workspace. Raw scenery records never survive to the next frame because projection reuses their command bytes.

The resident `mul_q8` is an assembly kernel (`fast_mul_q8`): it multiplies the operand magnitudes through unsigned 8x8 partial products from the quarter-square table BOOT builds at `$C000` (`Q[n] = floor(n*n/4)`, 512 low bytes then 512 high bytes) and rounds a negative product toward minus infinity, exactly like the reference `((a as i32 * b as i32) >> 8) as i16`. GEOMETRY composes each mesh instance's yaw, pitch and roll into a 3×3 Q8 matrix once (`fast_mesh_matrix`, the body of `RotationMatrix::compose`, entries clamped to ±256) and rotates every stored `i8` vertex with `fast_rotate_vertex`: nine quarter-square products with zero entries skipped, a 24-bit sum and one arithmetic shift by `8 − mesh shift (+1 for half-size debris)`. Projection is `fast_project` (`target-c64/asm/project.S`), the body of `projection::project_reference`: z normalised to 128..255 with a signed exponent, one 16×16 product per axis against the resident `PROJECT_X`/`PROJECT_Y` tables (`80·32768/d`, `133·32768/d`), a byte-aligned shift and the ±8191 clamps; `project_cached` keeps the 64-entry hash cache around it with shifted adds instead of 16-bit multiplies. `tools/test-multiply-kernel.py` and `tools/test-projection-kernel.py` check every kernel against its reference in `mos-sim`, and `target-c64/build.rs` runs `tools/branch-audit.py` on every assembled object so a silently wrapped conditional branch fails the build.

SHADOW finalization prepares a sixteen-byte persistent raster head at `$EF20` for each sorted position, reading the command through the sorted order (the command array is retired after all heads are prepared). `fast_prepare_triangle` keeps the host's exact Q16 arithmetic but takes a fast path for the usual edge (dy ≤ 255 and |dx| ≤ 255): a 24-bit restoring division by the 8-bit dy, the row-centre fraction `16384 / dy` from a table when the start row is on screen, and two quarter-square products; native positions carry +32767 so a row's column is simply the high word. Twelve-byte tails at `$E000` and ordered bit masks at `$EC00` identify the triangles touching each band. Edge positions advance sequentially in Q16 across all 25 bands, including bands whose painting can be skipped; edges are not recomputed from scratch for every band. The band driver is assembly (`fast_raster_band`): it scans the band's ordered mask and derives each marked triangle's head and tail addresses from its sorted position; the row walker copies the head into zero page (`$D0–$DF`), splits the band's rows into the segment before the middle row and the segment from it (so no row tests for the middle), clamps both columns from the high words, hands every nonempty span to the cell writer and writes the positions back when it leaves the band, except from the triangle's last band. Spans do not paint pixels: the cell writer (`target-c64/asm/cells.S`, the native body of `havoc::cells::CellBand`) writes two-bit colour codes straight into a row-major 8×40 table of cell bytes while a three-slot palette per cell hands out the hardware colours on first use (the span loops take a free slot inline). Code 0 is the VIC background, which PIXELS sets to the frame's clear colour at the start of every frame, so the clear colour and every unpainted pixel need no slot; the slots store keys (colour XOR the clear colour), which makes the clear colour key 0 like a free slot and keeps every fast path's zero test free of a comparison. A slot whose colour no longer appears in any of the cell's eight rows is recycled; a cell asked for a genuine fourth other colour is expanded into the pixel band at `$8E00` and painted per pixel from then on (its palette is poisoned with `$FF` so the span fast paths route every later write there; whole cell rows of plain or terrain ink take unrolled pixel paths). The parallel metadata table keeps terrain coverage in its low nibble and per-pixel HUD priority in its high nibble. Exterior terrain skips covered pixels; every effective write replaces its HUD bit, and spill expansion reconstructs both metadata bits. `fast_cell_stage` transposes the 320 code bytes into the consumed coverage table and writes the 80 attribute bytes into screen staging and the consumed overflow table (XORing the keys back; a free slot shows the clear colour). Each overflow flag is consumed before its byte is replaced. Only overflowed cells pass through the classic per-pixel quantizer (`fast_pack_cell`); their staged attributes are first seeded with the visible preceding values so its palette-retention behavior stays identical. Staging invalidates the coverage-clean marker, ensuring the next band clears the reused bytes. Before the background carried the clear colour, cells crossed by terrain while still showing sky spilled into the pixel form 40–60 times per frame (about 5 ms); with it they rarely do. Decoded pixels are checked against the host reference with the background (`tools/bench-native.py --pixel-identity` records `$D021` as byte 10,000 of a capture). Bitmap panels, captions, and ambient dust draw through the same `Canvas` trait (`span`, `plot`, `glyph`) as the host reference. The persistent cockpit is composited separately by the VIC sprites.

`fast_pack_cell` (`target-c64/asm/pack.S`) is the only remaining per-pixel quantizer: it ranks a spilled cell's colours (count, retained slot, HUD priority), keeps retained colours in their preceding slots, maps omitted colours to the nearest chosen colour or the background, and writes the eight bitmap bytes and two attribute bytes in place. `tools/test-pack-kernel.py` checks it byte for byte against `render::pack_band` over 256 bands (random, exact-colour, row-uniform and rendered-scene bands with black and coloured backgrounds). The band packer and its partial-cell sampling table at `$7700` are gone; `$7700–$7727` now supplies screen staging, and `$7728–$774F` holds cell overflow flags until staging.

Each of the ten terrestrial visual themes uses five horizon triangles and nine cloud facets, decoded from compact signed-byte descriptors in SCENERY. The backdrop is authored in camera space; crossings interpolate the outgoing and incoming arrangements. Horizons use the neutral gameplay camera height of 40. The extended horizon bases cover the moving terrain even at camera extremes. Each theme has its own horizon and cloud arrangement and palette. Outdoor clouds and horizons occupy separate bands, so each 4-by-8 cell contains at most three colours besides the sky (the background) despite the larger palette across the complete backdrop. Crystal Cave reuses the fourteen faces for walls, roof, and hanging crystals, sharing three such colors even where they overlap. Ashen Wastes limits its scenery and rock obstacles to orange, charcoal, and black, leaving a cell slot available to an isolated yellow projectile.

Backdrop faces and the orbital starfield carry the `STATIC_SKY` flag, also while a crossing blends them. Since every band is repainted every frame, the flag no longer selects cached bands; GEOMETRY applies a slight sky parallax to flagged records as it replays staged scenery (`Renderer::project_scenery`): the far sky moves opposite to the ship by its full sideways offset and half its height (`Game::sky_parallax`), which perspective reduces to a few pixels at the horizon and slightly more for the nearer clouds. The host reference (`Game::render_scenery`) applies the identical shift before the sink, so staged and direct host renders stay pixel-identical. Planets, terrain and foreground geometry do not receive sideways sky parallax. Title and attract scenes instead use automatic sine-wave offsets: sky drift of ±64 world units horizontally over 10.24 seconds and ±8 vertically over 20.48 seconds, with a matching ±4-unit vertical camera float around their base heights of 0 and 40. The existing 50 Hz animation clock drives the motion independently of the ship; scene cuts reset it to neutral, but title activity does not. Text stays fixed. No sky band cache or validity mask remains.

Attract mode uses the same simulation, section cache, geometry, and raster pipeline as gameplay. Title inactivity and each section preview last 750 fixed 20 ms steps on both PAL and NTSC. A latched activity bit preserves joystick pulses between simulation steps; scene resets consume the dismissal input and clear encounters and gestures. Attract frames omit the ship and cockpit and paint all 25 bands. The small phase-independent Q8 multiply helper lives in resident code to accommodate the expanded game-state layout. Attract scenes retain the continuous digital title track across area changes.

Playing mode inserts a 400-tick scenic flight at each of seventeen boundaries. At cruise speed, each stage takes 3,750 combat ticks: eighteen stages plus crossings total 74,300 ticks, or 24:46 before deaths and boss holds. Afterburner advances the route and crossings twice as fast during a burst, shortening that travel time while combat clocks remain at 50 Hz. Native progress uses a stage-local `u16 stage_ticks` plus the stage ID; aggregate HUD/report distance is derived with 32-bit arithmetic. Scenery uses local crossing coordinates so campaign length cannot overflow its old absolute 16-bit clock. Bosses enter at local tick 1,000 in stages 4, 8 and 12, and tick 2,500 in stages 17 and 18. Unfinished bosses hold progress at tick 3,750 while combat continues. The outgoing checkpoint remains active through a crossing; its successor begins only at exit.

Player controls, charge and double-tap history, player projectiles, and explosions remain live during scenic flights. Entry removes outgoing actors, pickups, and hostile shots before processing controls; enemy spawning, boss simulation, collisions, and damage are suspended. Bombs retain their inventory cost. Clearing any boss encounter immediately restores shield 100 and bombs 3, once; its subsequent crossing preserves those resources and adds one life up to the selected mode's starting lives. Other crossings preserve resources. Decorations have no actor, collision, or targeting state. The incoming checkpoint and music begin on exit, with encounter processing on the following tick. The existing two-slot cache prefetches that incoming stage during the crossing.

All bosses start with 40 HP per side unit and 120 core HP. The final Gate Guardian then splits into two bosses with 20/20/60 HP each; defeating both spawns four bosses with 10/10/30 HP each. Every child protects its own core until both of its side units are destroyed. Each phase therefore has 200 total HP, and the HUD shows the rounded-up percentage of that phase's remaining health. A split clears projectiles and ordinary enemies, suppresses later encounter spawns, and gives 50 simulation ticks for the children to separate without attacks or damage. Player movement continues, and shields and three bombs refill once at each transition. Only defeating all four final children awards the 5,000-point boss reward and enables victory. Checkpoint respawns restart the original encounter. Shield drops are eligible every eighth enemy kill and restore 60 points capped at 100; bomb-carrier priority and full pickup-pool behavior are unchanged. All active pickup types share the original shield attraction range and speed. Rocks and bosses do not advance ordinary enemy-kill pickup or bomb rewards.

Four compact boss instances store position, three health bytes and independent hit feedback. The children use a simplified octagonal ring in the final stage's unused mesh slot 5, with explicit reverse faces and existing diamond weakpoints. Medium children are centered at `(±74, 0, 500)`; small children at `(±55, ±40, 500)`. Their hulls use quarter/eighth original dimensions, and weakpoint bounds scale with them so all defenses remain reachable with regular shots. Their phase-shifted attacks retain the Guardian's existing patterns. The projected foreground budget remains 174 triangles.

The rebuilt production package passes all 34 native weapon scenarios on both PAL and NTSC, including both Guardian split intervals, exact child health, synchronous medium-boss completion, one-time refills and the final reward. Reports are `target/split-boss/pal/report.json` and `target/split-boss/ntsc/report.json`. The four protected/exposed two-/four-boss benchmark scenes each pass at zero and ten simulation ticks on both standards (16 cases total), with 24 seeded projectiles, no geometry drops or input overflows, and intact stack guards; captures and reports are under `target/split-boss/bench-pal/` and `target/split-boss/bench-ntsc/`. The private SIM-extra corruption fixture also passes (`target/split-boss/corruption/summary.json`) and leaves the input package hashes unchanged. These are emulator checks; they do not measure physical Ultimate performance or DAC playback.

The assembly IRQ wrapper preserves the processor-port setting, maps I/O back in, saves the compiler registers and static scratch frames, switches to the IRQ stack, and restores everything before returning. This also protects interrupted arithmetic helpers that LLVM-MOS gives static local storage. Compile-time guards, the linker, and the post-link layout checker enforce the reserved boundaries.

### Guarded band publication

`publish_staged_band` (`target-c64/asm/publish.S`) copies one completed band:
320 bitmap bytes from `$9540`, 40 screen bytes from `$7700`, and 40 color
bytes from `$7728`. There is no additional framebuffer. These are consumed
coverage/priority and overflow tables plus screen staging, and `fast_cell_stage` marks coverage
dirty so the next band clears it before drawing.

For band `b`, the publisher excludes raster lines `[40 + 8b, 64 + 8b)`.
This covers the union of all Y-scroll fetch windows, eight leading lines
for the bounded copy and a trailing margin. It publishes immediately outside
that interval and polls until it ends otherwise: the nominal maximum wait
is 24 lines (about 1.5 ms), plus polling and interrupt overhead, rather than
a refresh period. `$D011` high / `$D012` low / `$D011` high
sampling rejects ninth-bit rollover; a second sample with IRQs masked closes
the race between checking and copying. IRQs stay enabled while waiting and
quantizing. The final recheck and copy alone mask IRQs, restoring the caller's
flags afterward. Both the conservative instruction/I/O model and native
benchmark traces check a seven-scanline bound. The model deliberately charges
an extra bus cycle for every VIC read and color store; physical turbo timing
remains to be measured.

Publication uses eight-byte bitmap loops and four-cell attribute loops.
The profiling detail markers distinguish staging/quantization (3), copying
and scheduling overhead (4), and raster polling (9). The benchmark also traces
the successful masked guard through the last visible store, accounting for
failed guards separately. Its checks reject missing copies or an overlong
protected section. The tests execute assembled publisher bytes across PAL/NTSC
boundaries, IRQ arrivals and raster rollover, and compare staged output with
the existing renderer reference.

The picture still updates progressively, and `$D021` still changes at the
start of PIXELS. Global background transitions can recolor old code-zero
pixels before their bands are replaced; this change addresses local
bitmap/attribute mismatches, not atomic whole-frame or background changes.

## UltiSID configuration

Every build uses four SIDs; there is no sound-profile feature, hardware probe, or profile menu. The supplied configuration disables automatic address mirroring and leaves both physical sockets unmapped and muted. `UltiSID Range Split=1/2 (A6)` gives each UltiSID two independent SID instances at the game's addresses: UltiSID 1A/1B at `$D400/$D440` and UltiSID 2A/2B at `$D420/$D460`.

| Chip | Address | Voices and gain | Pan |
|---|---|---|---|
| UltiSID 1A | `$D400` | Three effect voices; 0 dB | Left 1 |
| UltiSID 2A | `$D420` | Three effect voices; 0 dB | Right 1 |
| UltiSID 1B | `$D440` | Three effect voices; shared 0 dB gain | Left 1 |
| UltiSID 2B | `$D460` | Three effect voices; shared 0 dB gain | Right 1 |

The package uses gentle stereo separation (`Left 1` / `Right 1`) for headphone listening, preserving left/right distinction while bringing the mix closer to center. Re-import the updated `havoc.cfg` to apply the mixer settings.

Each split pair shares its parent UltiSID's mixer gain and pan. Both parent gains remain at 0 dB. The two left chips form one allocation pool and the two right chips another; selecting a backup chip does not widen the physical stereo separation. Addressing is described in the [Ultimate MultiSID guide](https://1541u-documentation.readthedocs.io/en/latest/config/multi_sid.html).

Both UltiSID groups use the same `8580 Lo` filter curve, `High` resonance setting, and `8580` combined-waveform behavior. Each SID instance supplies three oscillators and one independent filter to a single effect. Per-voice routing permits a dry attack with a filtered body; programs animate pitch, pulse width, waveform, sync, ring modulation, cutoff and resonance. SID master volume stays fixed. All music uses the separate Ultimate Audio sampler.

The Ultimate Audio mapping at `$DF20–$DFFF` is enabled, and both sampler mixer channels are set to +6 dB to raise digital music relative to the SID effects. The UltiSID gains remain at 0 dB. Re-import the current `havoc.cfg` to apply these sampler levels. Track handoffs use the sampler voices' runtime volume controls for the outgoing fade; they do not change the configured mixer gains or SID volume. The firmware's `Digis Level=Medium` setting is retained but is not used for sampled playback. Mixer panning applies to HDMI and AV output, not the physical SID-TAP header. The built-in SID player's autoconfiguration can override mixer settings, so re-import the game configuration after using that player. See the [official mixer documentation](https://1541u-documentation.readthedocs.io/en/latest/config/audio_mixer.html).

### Updating an older package

Earlier downloads offered two-SID and four-SID variants, and the original four-SID configuration used physical sockets. Replace all three runtime files with the refreshed single package and re-import its configuration. An old two-SID REU image is incompatible with the current executable and is rejected at startup. Keep historical binaries with their original matching REU images and memory settings when reproducing archived measurements.

## Digital music

The DLT5 build preserves **Hammond Rocker** as true stereo at 20 kHz, with three compressed bits per channel and rounded signed 8-bit sampler output. **Galactic Dash**, **Mission Failed**, and all eighteen stage recordings use the 16 kHz, three-bit mono profile. Their right output reads the same PCM 160 samples (10 ms) later through separate sampler start and repeat addresses; there is no second mono PCM stream. Hammond Rocker receives no artificial delay. Galactic Dash has one canonical mono encoding, with its original full length preserved.

The [catalog](../assets/music/catalog.json) includes twenty-one original digital recordings composed for H.A.V.O.C. by William Echols. Each of the eighteen stages has its own song, including Citadel Core, Orbital Shipyard, Space Carrier, and Jump Gate; Shattered Moon plays Lunar Awe. Mission Failed supplies Defeat, Galactic Dash supplies Victory and the default fallback including ascent, and Hammond Rocker remains the initial title song. The twenty-one stored assets occupy 16,121,852 bytes, including a 400-byte directory and alignment, leaving four bytes of soundtrack space free. The 99,334 local codebooks use 1,749,920 bytes, with average adaptation intervals of 20.915-20.920 ms. The unique recordings total 34:37.8835, with no duplicate encoding or shortened song. Title/attract continuity, the secret menu-song toggle, and the final Galactic Dash selection are retained. A requested track is prepared in the inactive buffer while the current track continues. The outgoing track fades for about one second through hardware sampler volume before the new track starts from its first sample; there is no crossfade or per-sample gain processing. The explicit emulator-only SFX mode disables digital playback for tools without an Ultimate sampler. All supplied digital source recordings and encoded versions use the [music MIT License](../assets/music/LICENSE), distributed as `MUSIC-LICENSE.txt` in player archives; the game's existing MIT notice and the encoder's Unlicense remain separate.

The catalog and track assets occupy `$080000-$FDFFFF`. A checked `DMC1` version 2 directory describes aligned, nonoverlapping DLT5 assets and carries separate semantic-cue and eighteen-stage maps; the generated Rust descriptors and package checks use that same catalog. Explicit stage mapping permits distinct recordings for stages that share a SID cue. Each DLT5 header describes an exact frame count, one or two channels, and eight-entry local codebooks. Three-bit codes form one continuous least-significant-bit-first stream. Signed-16 predictors persist across codebook changes and reset at the exact song loop. Publication rounds to signed 8-bit; cumulative CRC-8 checkpoints cover every 1,024 compressed bytes. See the [encoder instructions](../tools/encode-digital-music/README.md) for byte layout, gain selection, provenance and reproducible listening previews.

The final 128 KiB of REU is split into two 64 KiB slots at `$FE0000` and `$FF0000`. A stereo ring holds 32,768 interleaved frames throughout its slot (1.6384 seconds); a mono ring holds 32,768 bytes in the slot's upper half (2.048 seconds). Mono preparation explicitly clears the preceding 160-byte prefix, including after the slot held stereo. Two sampler voices use hard left/right pan, unity volume outside the outgoing fade, and the cached divider for the selected profile. Their gate writes are adjacent but sequential.

BOOT validates the catalog, track headers, codebooks and CRC tables before enabling playback. A short muted signed-16 transfer first clears retained sampler low bytes; subsequent playback uses signed 8-bit. Both 20 kHz stereo and 16 kHz mono timing are calibrated once and cached across track changes. Runtime code and both decoder contexts remain separate from BOOT validation code. Rendering may overwrite the shared `$8E00-$91FF` staging workspace after AUDIO returns: predictor, bit reservoir, compressed cache, CRC and transition state are persistent.

Each AUDIO visit services the active ring first, with a total decode allowance of 250 ms of PCM: at most 4,000 mono or 5,000 stereo frames. Active and pending streams share that duration; when their rates differ, the unused allowance is converted to the pending stream's rate. Initial loading repeats bounded decoder visits inside the first AUDIO call, priming the complete ring before starting either sampler and opening the simulation/input gate. The old song continues at full volume until the new ring is ready, then fades through 32 equal hardware-volume steps from 32 to zero over about one second, measured from the existing CIA timer in foreground service. Handoff waits for both silence and a complete pending ring. Repeated pending requests preserve preparation and fade progress. Replacing the pending song restarts its preparation without restarting an active fade; if that preparation takes longer, the old stream remains muted until ready. Requesting the active song cancels the change and restores full volume without restarting playback. The active clock advances during preparation. The 2,048-frame underrun guard remains larger than the mono delay. An unsafe lead or integrity fault mutes and stops both sampler voices. The per-byte stream CRC-8 is a debug hook (`DIGITAL_STREAM_CRC`, off in production): startup authenticates headers, codebooks and checkpoint tables once, and the gameplay decoder carries no continuous checksum cost.

The 50 Hz CIA1 IRQ continues to sample input and run SID effects during DAC playback and song preparation. Effects have their own persistent engine and queue, separate from the persistent DAC state. The IRQ does no REU access or digital decoding. Music reset, stop and backend replacement leave the effects voices and global SID volume intact.

[Host backend tests](../tests/digital_backend.rs) independently decode catalog assets through a sampler model, including complete loops, both slots, mono delay, cancellation/replacement, scratch reuse, corrupted data and underruns. [Effects tests](../tests/quad_audio.rs) check allocation, synthesis and bomb compatibility. The [linked native-player harness](../tools/test-digital-kernel.py) checks assembled code, ABI, layout, timing and effects continuity through DAC handoffs. Host-model success does not establish native throughput, physical sampler behavior, perceived quality or final mixer balance; those require their own current-build reports and Ultimate listening checks.

**Earlier complete twenty-one-track build validation:** 452 host tests pass (two ignored), along with ten packaging tests, four SFX-profile tests, 23 SFX benchmark-tool tests, 24 encoder tests, and five catalog tests. Production memory-layout, overlay, IRQ-frame, and REU packing checks pass. The matching PAL/NTSC native audio reports each pass 160 assembly cases, all twenty-one tracks and 24 handoffs, checking 2,469,487 PAL / 2,460,962 NTSC decoded frames. All eighteen stages, five boss continuities, defeat/restart, mono victory, protected ascent, handoffs and fault handling pass. Initial priming takes seven stereo or nine mono service visits, and total decoding stays within 250 ms of PCM per service call. See the [current soundtrack validation](digital-music-handoff.md#current-development-status-dlt5) for report paths and binary identities. These native execution models do not measure physical Ultimate throughput, mixer balance or listening quality; use the [hardware comparison procedure](audio-check.txt) for that acceptance.

Before the six stage recordings were added, the three-asset DLT4 build passed 160 assembly oracle/ABI cases and complete PAL/NTSC native playback checks. Its modeled steady stereo service cost about 159 ms per audio second on PAL and 153 ms on NTSC; mono cost about 71/68 ms. Stereo servicing was approximately three times faster than the prior player, including modeled DMA cost. These are historical measurements of that build, not validation of the current twenty-one-asset catalog and stage mapping. See the [handoff measurements](digital-music-handoff.md#current-development-status-dlt5) for the model, comparison basis and report paths. Current-build timing and physical Ultimate listening require their own acceptance checks.

The [DLT3 handoff snapshot](digital-music-handoff.md#havoc-digital-music-compression-and-real-time-playback-handoff) preserves the former 24 kHz, four-bit, signed-16 stereo design and measurements. The v0.1.2 release validation below describes that historical package, not DLT4. Keep matching PRG, REU and configuration files together, and re-import the configuration when upgrading.

## Quad-SID effects

The SID soundtrack, score pages, cue directories, and executable music kernels have been removed. Digital music owns the DACs permanently; `MusicState` retains semantic IDs only for digital track selection. The former SID score/kernel REU area is unreserved.

The assembly effects revision (package v16) keeps the 50 Hz resident engine and gives each SID three layered oscillators with its own 11-bit cutoff envelope. Every effect uses at most one SID per channel: one chip for a directional effect or one left/right pair for a centered or cinematic effect. The four-chip pool allows two overlapping stereo effects or up to four directional effects. Thirteen bounded motion programs provide independent per-voice pitch slides (`freq ± freq >> n` per tick, from a halving per tick to about one percent, rises saturating), pulse-width motion, a staged waveform change, a gate retrigger tick that re-attacks every envelope, and an accent: an arpeggio step every fourth tick, an octave toggle on alternate four-tick groups, or a vibrato. A timbre transition can also change resonance. Each patch authors its own pulse width per voice, an eight-bit cutoff trio (initial, attack target, body target) with the filter rate as the shift applied to the remaining distance each tick, and a volume punch: the chip's master volume holds 15 for the authored ticks, then falls one step per tick back to the mix level of 8. Directional effects use their primary program; stereo effects use authored left/right programs. Program choice, detune, cutoff offsets and pulse phase follow the logical channel, so backup chips reproduce the same sound. Noise remains on a separate oscillator, and filter routing is authored per voice.

The 21 stable effect IDs cover gunfire, missile launch/impact/readiness, hit and destruction variants, pickups, rolls, bombs, footsteps, birds, transports, meteors, boss arrival and low shields. Every instrument layers a dry transient, a tonal body and a filtered or modulated tail. The player's cannon is a D5 pulse dropping a quarter per tick that hard-syncs an E4 carrier, over a two-frame noise click; enemy fire zips upward and re-attacks into a ratchet. Enemy and boss hits are heavy hull impacts: a dry pulse thud dropping a quarter per tick (C3, or G2 for a boss) over a low-passed noise crunch and a triangle sub, the filter closing fast, with two or three punch ticks; missile impacts are small explosions. The player's own hit is a dry blast with a dissonant B4/F4 stab through a high-pass. Enemy explosions open with a brief D6-rate noise crack; a dry falling C2 pulse and steady G1 triangle sub enter 80 ms later, together on both stereo channels, as the fully opened low-pass closes toward a rumble. Their bass envelopes run behind silent waveforms until age five; the crack is not retriggered. They retain four punch ticks and a 30-tick duration. The barrel roll is a Doppler pass: a G3 pulse with pulse-width motion and a D3 sawtooth sink together while a band-passed noise whoosh swells and closes behind them, fitting the 25-tick roll without a pitch rise or punch. Pickups are chord plucks that re-attack and climb an arpeggio; missile readiness is a low two-tone lock-on, a G4 triangle pluck fading as a low-passed C5 square swells in a fourth above it, with no ring modulation or punch; the low-shield alarm toggles octaves and re-attacks. The bomb, boss explosion and player death hold three punch ticks, detonate twice, and sink their pitches while a fully opened low-pass closes slowly to a rumble; the boss explosion drags a hard-synced carrier down in a power-down sweep and the death wails with vibrato. Scenic cues (footsteps, birds, transports, meteors, boss arrival) are quieter swells and chirps without a punch. Ordinary laser contact with an indestructible actor emits `EnemyHit` as well as its visual spark. Readiness sounds once when charge reaches full. Low shield retains its warning after the softened damage onset, sounds on crossing 25, rearms after healing above 25, and is suppressed on lethal damage. A collision emits its principal impact or destruction cue once.

The eight-event queue coalesces repeated effects in the same pan lane and preserves stronger cues when full. Priorities ascend from gunfire (0), scenery (1), destruction/pickup/roll/readiness (2), missile/arrival cues (3), enemy/boss hits (4), player-hit/low-shield cues (5), bomb (6), boss explosion (7), to player death (8). Allocation prefers idle chips, then the lowest-priority oldest group. Directional events use a chip on the requested side; centered events acquire an atomic stereo pair. A full gate-low tick separates retriggers.

Bombs, boss explosions and player death always acquire one stereo pair, regardless of pan. A bomb therefore leaves one left chip and one right chip available for ship explosions or other feedback, including its activation tick. Repeating an active bomb restarts its existing pair instead of consuming the spare pair. Stereo groups remain indivisible when stolen. The bomb retains the original core pair's bass, noise, envelopes and filter trajectories, with logical-channel trace checks; removing its doubled pair deliberately changes the combined sound and replaces the former four-chip trace requirement. Same-lane duplicate coalescing and finite capacity still limit crowded scenes. Scene changes cancel scenic effects without changing DAC state; fatal errors mute all four SIDs and both sampler channels.

Spatial cues choose their side at each trigger from source X and depth. The screen is 160 logical pixels wide; the center band is ±4 pixels around X=80, or 5% of its width. The foreground uses the overflow-safe equivalent `20*abs(x) <= z` without invoking renderer projection. Player shots/actions, enemy fire, impacts and destruction use their source or contact position; existing tails stay assigned. Wildlife, transport and meteor cues sample the visible source at the actual trigger time, including delayed scenic frames. Ascent and orbital swells have no matching moving transport and retain their authored ambient pans. Nonpositional announcements remain centered. This routing leaves the configuration's gentle `Left 1` / `Right 1` separation and all mixer gains unchanged.

The effects state uses 200 bytes within its 256-byte reservation at `$CD00`, separate from the DAC context at `$CE00`. The permanent bank at `$7200-$74FF` contains 24 exact PAL/NTSC pitch slots, 21 25-byte patches and thirteen eleven-byte motion records: `96 + 525 + 143 = 764` bytes before bank padding. Startup authenticates the complete 768-byte bank. All IRQ-reachable code and data stay resident. No SFX tick performs REU access or borrows foreground DAC state. The 50 Hz engine itself (tick, allocation, retirement and every SID register write) is the assembly kernel `fast_effects_tick` in `target-c64/asm/sfx.S`; the Rust `EffectsEngine` remains the host reference and exports the patch, program, pitch and priority tables and the state layout the kernel addresses, with compile-time offset assertions beside each declaration. `tools/test-sfx-kernel.py` holds the linked kernel to the reference's complete state image, SID register image and ordered write stream for every exported fixture. The queue helper stays compiled Rust in the constants bank, because an external callee would make every simulation caller spill around the call. Event layout remains two bytes; spatial routing adds no persistent game state.

Resident memory after this change: the effects state left `$0720` for the page below the DAC state, the first mission buffer moved from `$2E00` to the retired default screen page at `$0600`, and the compiled engine's 3.4 KB of resident code became about 2.1 KB of assembly, so resident code may fill `$0801-$2FFF` with about 1.5 KB of headroom instead of eleven bytes. The modeled worst-case tick halved (about 0.51 ms PAL / 0.49 ms NTSC at the 64x bus model, excluding IRQ entry/exit).

The interrupt wrapper used to copy every compiler static frame (`.noinit`, 536 bytes) to a backup before `irq_body` and back afterwards, because LLVM-MOS may give a function called from both the foreground and the interrupt a static local frame. With the effects engine in assembly, nothing reachable from `irq_body` owns such a frame, so the wrapper now only saves the imaginary registers and switches to the interrupt's software stack; the two copies, about 1.5 % of the CPU at 50 Hz even while idle, are gone, and `$BF40-$BFFF` plus `$CB00-$CCFF` are free; the four-boss Game allocation extends through `$CAFF`. `tools/check-irq-frames.py` (run by `scripts/build-target.sh`) follows every call from `irq_body` and `nmi_entry` through the linked ELF and fails the build if a compiled function in that closure addresses the static-frame bytes. `tools/test-sfx-kernel.py` executes the linked interrupt wrapper with only its stack and state writable, so a stray write would fault in the CPU model.

**Historical merged mainline validation (2026-10-24), before the seven-track expansion:** the combined Story/graphics/audio build passes 446 default host tests (one fixture export ignored) and 260 no-default library tests (one ignored). The production ELF SHA-256 begins `89059f5f61e7649f`; it retains 1,319 resident bytes, a 297-byte DAC context and 200-byte effects state. Native effects match all 6,932 complete state/register/write fixtures per standard, including three continuous 1,500-tick workloads. Maximum modeled tick cost is 0.47561 ms PAL / 0.45607 ms NTSC; linked audio IRQ entry, exit and dispatch add 1,011 CPU cycles. Layout and overlay rejection suites pass 154 and 84 cases, the IRQ-frame audit finds no unsafe static-frame use, and all five damaged-SFX-package cases are rejected. Reports are under `target/merge-validation`; `production-manifest.json` records artifact and source hashes. This report and the following merged-build measurements describe the ten-asset package, not the final twenty-one-asset catalog.

The merged native DAC report (`target/merge-validation/digital-report.json`) passes 160 assembly cases and exact PCM checks for 1,263,983 PAL / 1,258,427 NTSC frames across all ten tracks. Eleven fades per standard last 1.001-1.067 seconds. Startup, active and pending decoding share the 250 ms limit; initial fill takes seven stereo or nine mono service visits with sampler gates muted until ready. Paired gain/gate writes, cancellation/replacement, clock wrap/rebasing, effects coexistence, ten corruption cases per standard and fatal shutdown pass. Story lifecycle, keyboard (235 fixtures / 695 scans), and HUD (482 publications / 129 scroll samples) checks also pass on the merged code.

Fresh PAL/NTSC smoke checks pass controls, pause, weapons, HUD, every campaign boundary/checkpoint, result restarts and the natural attract tour. Thirty sustained 30-second windows (five workloads, three CIA phases, both standards) pass: 6,231 complete frames, 45,000 completed 50 Hz ticks, 9,534 requested events and 6,204 admitted groups. Request/admission traces, stable repeated gameplay/rendering, complete window accounting and IRQ deadlines pass; maximum IRQ delay is 1.242 ms, below 2 ms. `target/merge-validation/tooling-summary.json` records the report/package hashes and the harmless post-run shell-wrapper diagnostic; both complete benchmark reports were independently revalidated. These effects-only emulator runs do not establish physical DAC-plus-rendering FPS or a frame-time comparison against the earlier graphics build.

**Pre-merge assembly-effects validation:** the host suite passes (one fixture-export test ignored). Native checks match 6,932 complete host state/register/write fixtures per video standard for the assembly engine's authored sounds, including three uninterrupted 1,500-tick workloads with 1,499 consecutive state links each; the fixtures exercise every kernel path (punch ramps, retriggers, the three accents, saturating rises and per-voice pulse widths). The modeled worst-case tick is about 0.48 ms PAL / 0.46 ms NTSC, and IRQ entry, exit and dispatch cost about 1,000 cycles without the former static-frame backup. The state remains 200 bytes; the bank uses 764 of its 768 bytes. Memory audits pass 154 layout and 84 overlay rejection fixtures, `tools/check-irq-frames.py` reports no interrupt-reachable static frame, and the five damaged-SFX-package cases and ten release-archive tests also pass.

**Historical v16 validation:** 421 host tests passed, and 6,932 native SFX cases per standard preserved the pitch-only reference's complete state and ordered SID writes; bomb, boss-explosion and player-death traces remained identical to their v15 references. The v16 bank held 32 pitch slots (24 used), 21 27-byte patches and eight nine-byte programs (767 bytes), with the compiled Rust engine resident. The measured v16 optimizations removed duplicate directional allocation candidates, ended allocation search at its first unbeatable free choice, and reused a byte-sized SID/register offset for hardware writes. `target/bright-sfx-comparison.json` compares the delivered v15 engine, the brighter pitch-only build and the optimized build, with complete per-PC and inclusive/exclusive function attribution in the referenced profiles. Native execution is modeled at 64 times the bus clock with an additional 63 CPU cycles per SID store; it is not a physical-machine CPU or FPS measurement.

| Sustained workload | v15 mean tick | v16 mean tick | v16 p95 / maximum | v16 worker duty at 50 Hz |
|---|---:|---:|---:|---:|
| PAL directional combat | 0.406 ms | 0.347 ms | 0.494 / 0.781 ms | 1.734% |
| PAL centered queue bursts | 0.395 ms | 0.357 ms | 0.834 / 1.020 ms | 1.784% |
| NTSC directional combat | 0.391 ms | 0.334 ms | 0.477 / 0.754 ms | 1.669% |
| NTSC centered queue bursts | 0.380 ms | 0.344 ms | 0.805 / 0.983 ms | 1.718% |

Relative to the pitch-only reference, optimization reduces directional-combat mean worker time by 14.95-14.96% and centered-burst time by 10.14-10.18%. The linked audio-only IRQ save/restore and CIA dispatch add a separate 19,283 modeled CPU cycles per tick. Including that cost gives PAL duties of 3.263% / 3.313% for these two workloads and NTSC duties of 3.143% / 3.191%. Raster/input work and non-SID I/O waits are outside that IRQ model. The fixture-wide mean mixes quiet and active states and must not be reported as sustained-combat utilization. Four independent profiler tests verify distribution, call/tail-call accounting, SID stalls and rejection of broken fixture continuity/indexes.

The historical v16 production DAC model passed all nine tracks and ten handoffs per standard, checking 1,183,703 PAL and 1,178,223 NTSC decoded frames. Both standards retain the shared maximum of 250 ms decoded audio per service; fade durations are 1.007-1.073 seconds at the tested cadences. Cancellation, replacement, clock wrap/rebasing, ready-before-fade, paired gains, SID coexistence, ten corrupt-asset cases per standard and fatal muting pass (`target/bright-digital/report.json`). PAL gameplay checks cover controls, weapons, streaming/results and pause; NTSC covers controls, weapons and pause (`target/bright-vice-{pal,ntsc}/report.json`).

All 72 PAL and 48 focused NTSC auditions pass with no clipping, including offline music mixes at the documented uncalibrated listening gains. Minimum attack levels exceed their quiet floors by 34.43 dB PAL and 33.52 dB NTSC. The center cannon's attack power centroid rises from 159 to 318 Hz on PAL and 157 to 330 Hz on NTSC; impact attack/body levels remain comparable while their noise pitches and softer envelopes stay fixed. These are VICE recordings, and oscillator-phase differences also affect recordings of the unchanged cinematic controls; spectra alone are not exact synthesis traces (`target/bright-audition-comparison.json`).

The historical v16 sustained PAL/NTSC comparison passed all ten workload/standard combinations and all thirty matched CIA-phase pairs (`target/bright-performance.json`). Sixty 30-second windows cover 12,054 complete frames, 90,000 completed 50 Hz ticks, 19,068 requested events and 11,928 admitted effect groups. Audio state and the clock remain continuous while rendering work repeats identically; request/admission traces, gameplay, geometry, pixels, sprites and full-window time accounting all match. Every phase's median and p95 increase stays below 1%. The largest pooled median increase is 0.000735% in quiet PAL; the largest pooled p95 increase is zero, and the largest individual-phase p95 increase is 0.000734%. Dense-combat medians improve 0.6953% PAL and 0.1345% NTSC. The largest maximum-frame increase is 0.05481 ms in PAL boss effects, whose median and p95 improve; maximum observed IRQ delay is 1.410 ms, below the 2 ms deadline. Twenty-three benchmark-tool tests check protocol enforcement and monitor transfers, alongside the four profiler tests. Phase costs, DMA and publication waits are retained in the source reports.

The companion cannot run Ultimate DAC playback alongside rendering, so these effects-only comparisons do not establish physical combat FPS. Use the [Ultimate comparison procedure](audio-check.txt) with the retained v15 package and the new package to check actual music, effects and combat together using the F readout.

**Historical v15 validation:** 419 host tests pass (one fixture-export test ignored), including shared-duration decode limits, fade cancellation/replacement, spatial boundaries and two-SID allocation. Native effects checks match 2,432 host tick fixtures per video standard, including a bomb with simultaneous left/right explosions and a repeated bomb. The original bomb's logical core-pair traces remain exact. The native model peaks at 0.924 ms PAL / 0.900 ms NTSC per effects tick, excluding IRQ entry/exit, with at most 101 / 99 SID writes and no software-stack use (`target/stereo-kernel/report.json`).

All ten historical matched, nine-sample PAL/NTSC effects benchmarks pass the 1% incremental frame-time limit. The largest increase is **0.4357%**, in NTSC firing; gameplay, geometry, pixels, sprites and injected event/tick schedules match (`target/stereo-performance.json`). Each sample covers one rendered frame, so later scheduled events are not reached: this establishes short-frame behavior, not sustained-combat performance. Both frozen v14 and v15 benchmark builds use the same reviewed audio-publication instrumentation; five negative fixtures enforce its narrow overlay exception. These measurements exclude DAC service and physical Ultimate timing.

The final production DAC model checks all nine tracks and ten handoffs per standard, comparing 1,184,599 PAL and 1,178,607 NTSC decoded frames. Fades measure 1.006-1.073 seconds across the tested foreground cadences. Priming retains full gain; each handoff occurs in the service call that reaches zero, after the replacement ring is complete. The weighted active-plus-pending decode quota never exceeds 250 ms of PCM per gameplay frame. Cancellation, replacement, timer wrap, clock rebasing, SID coexistence, corrupt assets and fatal muting pass (`target/stereo-digital/report.json`). Final production layout and overlay checks also pass, including 154 and 84 rejection fixtures; `target/stereo-final-artifact-audit.json` records their matching hashes and the audition-bank comparison.

The final production PAL package also passes twelve ordinary gameplay frames plus control, weapon, streaming/results and pause scenarios (`target/stereo-vice-final-pal/report.json`). Earlier v15 PAL/NTSC gameplay checks and five corrupt-SFX-package cases are retained under `target/stereo-vice-{pal,ntsc}` and `target/stereo-corrupt`; these precede the final routing/layout reductions. Release-archive validation passes all ten fixtures.

The unchanged v15 synthesis bank passed 72 PAL and 33 focused NTSC auditions with zero clipped samples. The barrel-roll attack gained 8.48 dB in the PAL center audition, and the retuned cannon and impacts shift energy toward bass and away from sharp high-frequency content. Auditions use the frozen v15 package in `target/stereo-audition-package`; later routing/layout reductions do not change its patches or synthesis programs. Reports are `target/stereo-audition-comparison.json` and `target/stereo-auditions{,-ntsc}`. Physical Ultimate listening remains the check for filter character and the final DAC/SID balance.

**Historical v14 validation:** 401 host tests passed (one fixture-export test ignored), alongside 2,322 exact host/native tick comparisons per PAL/NTSC standard, all native DAC tracks/handoffs/faults, 154 layout and 84 overlay rejection fixtures, five SFX-bank corruption cases, and native collision-to-SID-gate traces for enemy, boss and player hits. PAL covered 4 control, 34 weapon and 43 streaming/results scenarios; NTSC covered controls, weapons and pause. That build preserved the full four-chip bomb stream; those results do not certify v15's revised allocation or DAC fade.

The historical v14 matched effects-only VICE benchmark passed all ten PAL/NTSC quiet, firing, dense-combat, boss and scenic cases with nine samples each. Its largest median frame-time increase was **0.0090%**, below the 1% incremental limit; gameplay, geometry, pixels, sprites and injected event/tick schedules matched. Reports are `target/synth-performance.json` and `target/synth-{baseline,current}/bench-{pal,ntsc}/report.json`. This measured incremental effects cost in the calibrated companion, not physical Ultimate FPS or DAC decoding. That benchmark alone used a 214-byte helper bank at `$FF20-$FFF5`, loaded from `$7500` before IRQ-stack initialization; its production PRG/REU hashes were unchanged by the instrumentation.

The v14 native tick model's maximum was 1.503 ms PAL / 1.452 ms NTSC, excluding IRQ entry/exit; fixture means were 0.147 / 0.142 ms. Those means included idle/end-of-effect states and are not continuous-combat CPU percentages. Historical synthesis reports are `target/synth-kernel/report.json` and `target/synth-digital/report.json`. All 70 PAL and 25 focused NTSC auditions passed corrected post-lead attack checks with zero clipped samples. PAL enemy-hit attack energy increased 5.72–8.98 dB and player-hit energy 4.77–6.61 dB versus the frozen baseline; these are emulator measurements, not physical mixer balance or evidence for the new thud patches.

The previous v13 build passed 999 exact native ticks per standard, with a modeled maximum of 1.066 ms PAL / 1.034 ms NTSC excluding IRQ entry/exit, at most 147 SID writes, 154 layout and 84 overlay rejection cases, and five SFX bank corruption cases. Those frozen results describe the old engine and do not certify v14 or v15. Its 63 recordings had no clipped samples, but the former whole-file silence check could be satisfied by a recorder-start transient; it did not establish hit audibility.

Use `tools/sfx-previews.py` for all 63 left/center/right auditions plus repeated 10 Hz lasers, crowded combat and hits during bomb/boss-explosion playback. Recording runs for 400 ms before triggering the effect, retains 100 ms of pre-cue silence, and checks attack/body energy separately from the recorder transient. The attack must exceed the quiet AC-energy floor by 12 dB and reach at least eight PCM16 RMS counts; clipping is rejected. Audition and digital tests read effect-field offsets from target DWARF instead of duplicating the state layout.

The companion lacks the Ultimate sampler, so emulator tools default to explicit monitor-only `--audio-backend sfx`; this never changes package files. `--audio-backend native` retains production routing for a sampler-capable emulator. Optional `--music-dlt assets/music/lowland-approach.dlt` creates offline listening mixes from the actual decoded eight-bit music, preserving mono's 160-sample right delay. The default excerpt starts at 30 seconds and uses explicit music/SFX gains of -12/0 dB; these adjustable listening levels are not calibrated Ultimate mixer settings. Current physical filter character and DAC/SID balance still require Ultimate listening.

## Single-build consolidation validation (2026-10-21)

The consolidated package uses four UltiSID instances and the default painter renderer.
The optional span renderers remain disabled unless explicitly selected. Host tests
pass in all four configurations: 331 default, 343 with music kernels, 353 with
deferred spans and music kernels, and 356 with front-to-back spans and music kernels.
Each configuration retains one ignored fixture-export test.

The default native build passes its memory and overlay audits, 129 negative layout
fixtures, 69 negative overlay fixtures, 1,181 exact kernel replay states across
20 kernels, and 131 corrupt-package rejection cases. The optional native span and
front-to-back builds also pass their audits and respectively 132/75 and 136/79
negative layout/overlay fixtures. No new renderer performance claims follow from
these checks.

Production and benchmark packaging select the ELF belonging to Cargo's selected
PRG by file identity. Isolated default-to-experimental-to-default build sequences
verify that a newer cached experimental ELF cannot displace the painter ELF;
the restored painter package remains byte-identical. Missing or ambiguous matches
fail before copying native files. Evidence is in
`target/single-ultisid/artifact-selection.json`.

The release ZIP has exactly five entries: the matching configuration, PRG and REU,
launch instructions and license. Each entry matches its source bytes. The REU is
1,048,576 bytes, the resident engine is 400 bytes, the separate music page is
224 bytes, and resident headroom is 1,230 bytes. All 146,528 score bytes match the
previous four-SID build. Logs and generated reports are under
`target/single-ultisid/`; historical measurements elsewhere retain their original
binary provenance.

Both PAL and NTSC runs of the exact release files pass startup/title comparison,
4 control, 22 weapon, 43 streaming/checkpoint/restart and 7 attract scenarios,
plus the pause suite and all 21 complete music loops with every page visited and
no underruns. Each report confirms unchanged PRG, REU and ELF hashes matching
the package. `target/single-ultisid/validation-summary.json` ties together these
reports, host tests, audits and package hashes. These monitor-assisted checks are
not continuous campaign playthroughs or physical-machine measurements; four-UltiSID
listening and shared mixer balance still need physical-hardware confirmation.

## v0.1.5 release validation (2026-10-25)

The release includes the DLT5 soundtrack quality improvements, expanded music
codebooks in the compact v17 REU layout, all-song preview, refined startup splash,
full initial music-buffer readiness, and parked Space Carrier shadow correction.
The complete build ran through `scripts/build.sh` in Ubuntu-24.04 WSL.

- **514 host tests passed**, with 2 fixture-export tests ignored. All 10 packaging, 4 SFX-profile, 23 SFX benchmark-tool, 10 catalog, 34 encoder, 15 quality-pipeline, 24 regeneration, and 20 splash-converter tests passed.
- **Native startup and memory:** poisoned-RAM splash startup and 738 REU-loader cases passed, alongside production layout, overlay, IRQ-frame, and package audits. All 201 layout and 103 overlay rejection fixtures passed. The final build retains 137 bytes of resident headroom.
- **PAL/NTSC digital audio:** 160 assembly decoder cases, all 21 songs, 24 transitions and 11 corruption cases per standard passed against the release binaries. The models checked 2,733,167 PAL and 2,723,515 NTSC frames. Separate startup checks confirmed a complete 32,768-frame initial music buffer before playback and bounded subsequent refills.
- **Keyboard and emulator gameplay:** 243 native keyboard fixtures and 786 scans passed. PAL/NTSC runs passed splash preservation and first-frame replacement, controls, afterburner, pause, weapons, multi-phase bosses, HUD/reticle clipping, stage transitions, checkpoints, victory/restarts, attract mode, and Normal/Story lifecycle. All four emulator runs finished with zero reticle interrupt misses.
- **Release ZIP:** all six entries passed integrity checks and match the validated runtime files, instructions, and licenses byte for byte. Report identities and final file hashes match the frozen release package.

Fresh logs, package identities, native reports, and emulator captures are retained
under `target/release-v0.1.5-publish/`. Its `validation-summary.json` verifies report
identities and the unchanged final files against the frozen package manifest.
The ZIP is 15,855,935 bytes; the REU is 16 MiB.

Native audio results are modeled execution; VICE uses effects-only audio. These checks do not establish physical C64 Ultimate playback timing or listening quality.

| Release artifact | SHA-256 |
| --- | --- |
| `havoc.cfg` | `c97d40c3f588049037631716fa4b4c8648214cfc0dc037b7e468aa234ca9c037` |
| `havoc.prg` | `ac98ef2c3c51327d480283baf42d01c39175f25b6be38f582de133a30ac19a91` |
| `havoc.reu` | `62935b73e8e5f89c2ff00e60e47050c9f0ddeaf6904a0b25821bcbf0c17b79b9` |
| `havoc.elf` | `97e565bdb53730e6d37b7342a3f8e16ef49babb5e5be8490be7720eb3b776c26` |
| `havoc-v0.1.5.zip` | `ed04a79f5b3df24ed37d2bb22be94c1b08e0923cca1ffca4248267080cfe8d26` |

## v0.1.4 release validation (2026-10-24)

The release build includes the afterburner, multi-phase Gate Guardian, expanded
orbital scenery, and single-pixel reticle with exact ship and exhaust clipping.
The full build ran through `scripts/build.sh` in the configured Ubuntu-24.04 WSL
environment and retained 84 bytes of resident headroom.

- **513 host tests passed**, with 2 fixture-export tests ignored. All 10 packaging, 4 SFX-profile, 23 SFX benchmark-tool, 5 catalog, and 24 encoder tests passed, alongside production memory, overlay, IRQ-frame, and REU audits.
- **PAL/NTSC digital audio:** 160 assembly decoder cases, all 21 songs and 24 handoffs per standard, plus corruption and fatal-muting checks passed against the release binaries.
- **Native effects:** 6,932 exact state/register/write comparisons passed per standard.
- **Native gameplay and display:** 244,330 flight steps, 19,023 missile scenes, 1,659 input calls, 235 keyboard fixtures, and 3,964 reticle cases plus 10 emulator scene samples passed. HUD publication, scrolling, sprite multiplexing and IRQ checks passed, with 162 layout and 89 overlay rejection fixtures.
- **PAL/NTSC emulator gameplay:** controls, afterburner, pause, weapons, multi-phase bosses, HUD/reticle clipping, stage transitions, checkpoints, victory/restarts, attract mode, and Normal/Story lifecycle passed. All four emulator runs finished with zero reticle interrupt misses.
- **Release ZIP:** all six entries pass integrity checks and match the validated runtime files, instructions, and licenses byte for byte.

Fresh logs, package identities and reports are retained under
`target/release-v0.1.4-publish/`. Its `validation-summary.json` verifies the native
and emulator reports against the frozen package hashes, confirms unchanged
runtime files after testing, and checks every ZIP entry against its source.
The ZIP is 14,211,468 bytes; the REU is 16 MiB.

Native audio results are instruction-model checks; VICE uses the explicit
effects-only backend. These results do not establish physical Ultimate playback
timing, controller wiring, or listening quality.

| Release artifact | SHA-256 |
| --- | --- |
| `havoc.cfg` | `c97d40c3f588049037631716fa4b4c8648214cfc0dc037b7e468aa234ca9c037` |
| `havoc.prg` | `cab167fabd141d69f11e03250d8d3ed9d1a3b099d74d82111c1f8d1c0e148221` |
| `havoc.reu` | `d0a43a1fc4ebba6afa7367745170dcf8831664d8e4cfce9be7a1b5c62016a227` |
| `havoc.elf` | `753ad04b81d3b6b233a755f43a2b32cd6ebfc72a1ea93879e14c50d751a2d5a4` |
| `havoc-v0.1.4.zip` | `3cb372226cee0d0204d8989b76e77a71e0dd54344cdb85446b3e1940d452145f` |

## v0.1.3 release validation (2026-10-24)

The release was rebuilt with `bash scripts/build.sh` in the configured Ubuntu-24.04
WSL toolchain. Its fresh host suite passes 453 tests with two fixture-export tests
ignored. Ten release-packaging tests, four SFX-profile tests, 23 SFX benchmark-tool
tests, five catalog tests, and 24 encoder tests pass. Native memory-layout, overlay,
IRQ-frame, and REU packing audits also pass.

The linked digital-player checks pass 160 assembly decoder cases and exercise all
21 recordings and 24 handoffs on each video standard: 2,469,487 decoded PAL frames
and 2,460,962 NTSC frames. Both pass 11 corruption cases, effects continuity,
SFX-only emulator fallback, and fatal muting. Initial priming takes seven stereo
or nine mono service visits; decoding stays within 250 ms of PCM per service call.
Fresh effects fixtures pass 6,932 exact host/native comparisons per standard, and
the linked keyboard scanner passes 235 fixtures and 695 calls.

PAL and NTSC emulator runs each pass four control, 34 weapon, 43
streaming/checkpoint/result, and seven attract scenarios, plus pause and HUD
checks. Separate Story-mode runs on both standards verify Normal 3, Story 99,
death and respawn 98, retry 99, and return to Normal 3. VICE uses the effects-only
backend; digital playback is checked by the native audio model. Neither establishes
physical Ultimate timing or listening quality.

Reports and the complete hash manifest are retained under
`target/release-v0.1.3-publish/`. `validation-summary.json` ties all report identities
to the frozen pre-test manifest and final on-disk files. All six ZIP entries match
the final runtime files, launch instructions, and game/music licenses byte for byte.
These results supersede the earlier complete-soundtrack report for release purposes.

| Artifact | SHA-256 |
| --- | --- |
| `havoc.prg` | `bf371470d66d3e9870a0fa9e5d32f45e3e0c4e51ece116001e5e534dcc4fcafe` |
| `havoc.reu` | `9e6a3ab7a077cecdea8b5b83bfc997a42ab8e4fef0c89cf4f84b52603efbf854` |
| `havoc.elf` | `3b7844443f64c66c24cebaafdb4b429ad40957f5906f8bde138540f6858bd66c` |
| `havoc.cfg` | `59201d76f4abc21ae97a7a813b1e48847605c9523658fa90b98f30ee72cc78f7` |
| `havoc-v0.1.3.zip` | `97964b645fc3ad82b6e8c63afc6ec95078f304ada301b2ad8eecbf91ed0e3a7b` |

## v0.1.2 release validation (2026-10-23)

The stereo-music release passes 371 default host tests and 383 with music
kernels, with one existing fixture-export test ignored in each configuration.
All nine release-packaging tests and seventeen encoder tests pass. The native
build passes its layout and overlay audits with zero cross-phase references;
146 negative layout fixtures, 76 negative overlay fixtures, and 131 corrupted
package cases are rejected as expected. The production PRG is 38,913 bytes,
with 64 bytes of resident headroom. AUDIO occupies 16,040 bytes, including a
13,421-byte runtime prefix.

The linked keyboard scanner passes 160 fixtures and 549 scans, including song
switching and joystick interference. All 1,181 SID kernel replay states pass
across twenty kernels. The maintainer also reports checking the new music on a
physical C64 Ultimate.

The final release ELF and REU pass the linked digital-player model on both PAL
and NTSC: both stereo tracks, menu continuity and switching, calibration reuse,
ring wrapping, scratch reuse, SID handoff, recovery from a safe service gap,
and intentional underrun muting. Missing sampler hardware is rejected in the
production path; the explicit emulator-only SID substitutions also pass.

Both PAL and NTSC emulator suites pass the exact title comparison, four control
scenarios, pause/resume, 34 weapon scenarios, HUD updates, 43 streaming and
victory/restart scenarios, seven attract scenarios, and all 21 complete SID music
loops. Every score page is observed with no underruns. These runs explicitly
substitute SID menu cues because the calibrated VICE companion lacks Ultimate
Audio; digital playback is covered by the separate model above. All reports
identify the final release artifacts, which remain unchanged throughout testing.

Build logs, test reports, source provenance and package hashes are retained in
`target/release-v0.1.2-publish/`. The release ZIP contains exactly the matching
configuration, PRG, 16,777,216-byte REU, launch instructions and MIT license.

## Build and verification

Host tools require Rust and Cargo. The target build and release packager additionally require Linux/WSL, Python 3.11 or later, LLVM-MOS SDK **v22.5.0**, and a Rust-MOS toolchain with the `mos-c64-none` target and Rust sources. This workspace was built with `rustc 1.87.0-dev` registered as the `mos` rustup toolchain; `target-c64/rust-toolchain.toml` selects that alias. `scripts/env.sh` defaults to `$HOME/.local/llvm-mos` and `$HOME/.local/rust-mos-targets`. Override `LLVM_MOS`, `MOS_CLANG`, and `RUST_TARGET_PATH` for another compatible installation.

From PowerShell, using the configured Ubuntu-24.04 WSL toolchain:

```powershell
./scripts/build-windows.ps1
```

Or from Linux/WSL:

```sh
bash scripts/build.sh
```

The complete build runs host and package tests, bakes assets, links one C64 program, audits memory and overlays, and produces matching `havoc.prg`, `havoc.reu`, and `havoc.cfg` files in `dist/`. It also creates `havoc-v0.1.5.zip` with the three runtime files, launch instructions, and game/music licenses. The native build uses `target-c64/target`. Generated packages are ignored by Git; cloning the repository does not supply `dist/`.

After changing assets, bake with `cargo +stable run --release -- bake dist`, then run `bash scripts/build-target.sh`. The bake step alone does not produce executable overlays. Use the full build script to run checks and produce the release ZIP. On Windows, `scripts/build-windows.ps1` runs the same build through WSL; its optional output directory receives the single package and ZIP. SID-profile arguments and the `four-sid` Cargo feature have been removed.

Host inspection commands:

```sh
cargo test --all-targets
cargo run --release -- sizes
cargo run --release -- snapshot title 0 target/previews/title
cargo run --release -- snapshot 2 5000 target/previews/canyon
cargo run --release -- snapshot attract 35000 target/previews/attract-crystal
cargo run --release -- snapshot transition 0 4000 target/previews/lowlands-crystal
python3 tools/previews.py
```

The host binary is an asset and inspection tool, not a desktop game. Snapshots produce an aspect-correct `.ppm` with the floating sprite HUD, a bitmap-only `.bitmap.ppm`, and a `.sprites.json` sidecar containing VIC registers, pointers, and all eight 64-byte images. The `.bin` stays bitmap-only: 10,000 bitmap/screen/color bytes followed by the VIC background byte. The preview converter needs only Python's standard library.

Native smoke captures retain their bitmap-only `-native.bin` (10,000 bytes), `-native.png`, and `-native.ppm` files for renderer comparisons. The separate `-sprites.json` records actual VIC state; `-composited.png` uses the 320×200 hardware-pixel grid, and `-preview.png`/`-preview.ppm` scale that composite to 640×480. These composites apply bitmap fine scroll and actual sprite coordinates, preserving half-multicolour-pixel shake offsets. Native benchmarks compare bitmap and visible-sprite hashes separately. `--hud-checks` verifies persistent image RAM and lazy corner updates; `--weapon-checks` also verifies equal, opposite sprite/bitmap shake motion. Showcase recordings retain their neutral-scroll 160×200 frame contract with the HUD composited and a sprite sidecar for each frame.

`snapshot attract` replays elapsed milliseconds from a fresh title screen, so `35000` shows five seconds into Crystal Cave. `snapshot transition <0..16> <elapsed-ms> [output-prefix]` selects an outgoing stage and a time from `0` to `8000`, rounded down to a 20 ms tick; `8000` shows the incoming checkpoint. Transition snapshots do not replay preceding combat. For orbital examples, use stage `12..17`; `snapshot transition 11 4000` samples the ascent.

Optional native kernel checks use the LLVM-MOS 6502 simulator:

```sh
source scripts/env.sh
python3 tools/test-pack-kernel.py
python3 tools/test-cells-kernel.py
python3 tools/test-publish-kernel.py
python3 tools/test-raster-prep.py
python3 tools/test-preparation-abi.py
python3 tools/test-sort-kernel.py
python3 tools/test-memcpy-kernel.py
python3 tools/test-game-math-kernel.py
python3 tools/test-multiply-kernel.py
python3 tools/test-projection-kernel.py
python3 tools/test-emit-kernel.py
python3 tools/test-clip-kernel.py
python3 tools/test-mesh-kernel.py
python3 tools/test-missile-kernel.py
python3 tools/test-shots-kernel.py
python3 tools/test-flight-kernel.py
python3 tools/benchmark-division.py
python3 tools/test-sfx-kernel.py
python3 tools/test-keyboard-kernel.py --elf dist/havoc.elf
python3 tools/test-story-mode.py --video pal --frames 2 --out-dir target/story-mode/pal
python3 tools/test-story-mode.py --video ntsc --frames 2 --out-dir target/story-mode/ntsc
python3 tools/test-digital-kernel.py --standard both
```

The keyboard check executes the linked PIXELS scanner against modeled CIA keyboard/joystick inputs. The digital check executes the linked AUDIO overlay against modeled REU and stereo sampler hardware on PAL/NTSC, including both directions of the menu song toggle and saved-calibration reuse. These checks do not require full VICE startup or physical hardware; the current calibrated VICE companion lacks Ultimate Audio sampler support.

`tools/branch-audit.py <source.S>` runs from `target-c64/build.rs` on every assembled source: it re-assembles the file with line information, reads the label each conditional branch names and compares it with the encoded target, because the assembler silently wraps a branch that cannot reach its label in the same section (a wrapped target before the section start is printed with a sign, which the audit also recognizes). Two more assembler habits to know: `<` and `>` applied to a negative constant yield the bytes of its magnitude, so negative immediates are written as explicit bytes; `.L` labels survive only with `-Wa,-L`.

Emulator checks use the calibrated sibling `doomc64u/target/audit-fast/x64sc-64x` companion and its ROMs; override these with `--vice` and `--roms`. An unmodified companion can map the turbo setting to 48x, invalidating a 64x timing comparison.

The current companion tests use explicit `--audio-backend sfx`; DAC playback is covered separately by `tools/test-digital-kernel.py`. Effects snapshots cannot restore DAC state, so native renderer benchmarks also require this effects-only mode.

```sh
python3 tools/test-emulator-audio.py
python3 tools/test-campaign-tools.py
cargo +stable run --release -- snapshot title 0 target/previews/title
python3 tools/vice-smoke.py --video pal --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video ntsc --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video pal --control-checks --weapon-checks --streaming-checks
python3 tools/vice-smoke.py --video pal --attract-checks
python3 tools/test-sfx-kernel.py
python3 tools/sfx-previews.py --video ntsc
python3 tools/test-corrupt-package.py
python3 tools/test-overlay-audit.py dist/havoc.elf
```

The smoke test records package/emulator hashes, exact native title bytes, physical joystick-port-2 input, audio activity, frame progress, and runtime guards under `target/vice`. Control and weapon checks exercise steering, roll dodges, homing missiles, hit flashes, and bomb display-register effects; streaming checks seed stage boundaries, checkpoints, and restart scenarios. Attract checks follow the complete idle tour and exercise dismissal with each input. These monitor-assisted cases are separate from a continuous campaign and physical-machine acceptance. Use distinct `--out-dir` paths when retaining multiple runs.

`scripts/build-probe.sh` builds `dist/hwprobe/hwprobe.prg` from `tools/hwprobe/`, a standalone timing probe for the physical machine. With the game's `havoc.cfg` imported, it reports CPU speed before and after the game's turbo request, REU DMA time per KiB in both directions, the cost of VIC and color-RAM accesses, and RAM copies with I/O mapped in or banked out, all in microseconds on the text screen. [benchmarks.md](benchmarks.md#hardware-timing-probe-2026-10-20) lists the calibrated emulator's figures for comparison.

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

`target-c64/build.rs` assembles the native hardware and rendering helpers, disables additional LTO zero-page register allocation, selects the custom linker script, and emits a memory map. Six primary phase sections share an execution address; seven encounter tails and the terminal tail share the SIM prefix's end address, while combat and medal drawing share the GEOMETRY prefix's end address. Explicit linker assertions and the project's post-link checks replace the general linker overlap check. ELF symbols and relocations are preserved for emulator debugging, package extraction, and `tools/check-overlays.py`. That checker rejects cross-phase references, permits only named phase entry calls and reviewed opaque dispatches into the enemy, terminal, and rendering tails, validates ownership of compiler switch tables and the lifetime of startup checks, and checks that the IRQ's relocation graph stays resident. `tools/check-irq-frames.py` then follows the interrupt's call closure through the disassembly and rejects any compiled function in it that addresses a compiler static frame, the invariant that lets the interrupt wrapper skip a frame backup. Runtime-computed pointers and timing still require execution tests.

The current `tools/test-overlay-audit.py` suite rejects 84 damaged relocation and ownership fixtures, including terrestrial references to unloaded space helpers, AUDIO references to unloaded startup code, and invalid sprite or mission-buffer reservations. `tools/test-layout-audit.py` rejects 154 mutations covering transfer lengths, REU addresses, audio ownership, the relocated shared workspace, sprite storage, and resident code growing into the first mission buffer. Both use private copies and verify that their input packages remain unchanged. Native scenery passes the concrete `SceneryStage` sink through its opaque decoration entry, so no resident vtable exception is needed for a callback into an overlay.

For repeatable size profiling, run `python3 tools/profile-size.py --baseline target/campaign-baseline/dist/havoc.elf --current dist/havoc.elf --out-dir target/campaign-size`. The read-only comparison writes `README.txt` and `profile.json` with function sizes and deltas, per-section totals, and common-prefix-plus-tail headroom. Either input may instead be a linker `.map`, allowing an overflowing build to be inspected before it produces an ELF. Repeated generic names with ambiguous matches are marked for review rather than assigned a guessed baseline.

Compiler flags must be checked at both optimization stages: Rust emits bitcode for this target, and the LLVM-MOS SDK performs a final LTO link. The [isolated compiler-option comparison](benchmarks.md#campaign-compiler-options-2026-10-18) distinguishes those stages, records size and failure results, and shows package-level overrides. Cargo has no per-source-file optimization profile; linker sections select memory placement, not a different optimization level.

`python3 tools/test-game-math-kernel.py` builds the production swept-depth assembly and a frozen Rust reference under the configured Linux/WSL toolchain. It checks 6,658 native simulator cases and records sizes and an identical 2,048-call timing harness in `target/game-math-test/report.json`. That harness measured 825,860 reference cycles and 758,449 production cycles; these include harness overhead and do not measure complete game frames.

The native `memcpy.S` replaces the Rust compiler's larger memory-copy routine and its static scratch frame. It follows the LLVM-MOS C ABI and uses only caller-clobbered registers. `python3 tools/test-memcpy-kernel.py` runs 1,297 native 6502 simulator cases against an independent volatile byte-copy loop, covering empty copies, page boundaries, unaligned addresses, returned pointers, and source/destination guards. Its generated report lives under `target/memcpy-test`.

Unscaled 3-by-5 text that fits completely within one eight-row band uses a bounded path: Rust checks the whole label once, then calls `fast_cell_glyph` with each glyph's five font rows, which paints at most two cell masks per row. Clipped or split-band text retains the general renderer, which paints each lit glyph pixel as a `scale`-wide span. Host tests compare this path with `draw_text_scaled` for all 256 character bytes and clipping boundaries; `python3 tools/test-cells-kernel.py` checks the glyph, span and plot kernels against the C transcription of the reference.

The other native kernels preserve host reference behavior: `sort.S` implements stable far-to-near radix ordering (writing the order table rather than moving commands), `raster.S` advances prepared edges and hands spans to the cell writer, and `pack.S` quantizes the overflowed cells. The compact sort kernel uses the resident table region; the raster walker uses PIXELS and the preparation kernel uses SHADOW finalization, leaving the fixed overlay boundaries intact. Their simulator checks compare bytes against independent references and verify memory guards (`tools/test-sort-kernel.py` runs the production Rust entry through 416 sorts, with and without scenery segments; `tools/test-clip-kernel.py` 1,584 triangles through the clipping kernel). Host geometry tests compare cached transforms with the original per-vertex arithmetic. These correctness checks do not substitute for target timing measurements.

`missile.S` (`fast_steer_missile`, in every SIM image) is the native body of `Game::steer_missile`. It retains an eligible target or acquires a live actor, boss side unit, or exposed core ahead, then applies the existing wrapping 16-bit guidance and ±12 clamp. Its 19-byte argument record contains actor and descriptor pointers, shot position, a pointer to four 10-byte boss instances, active boss count, collision scale shift, dx/dy, target ID and random state. Game stores 24 one-byte locks: IDs 1–12 select actors and 13–24 select the four bosses' left/right/core parts. Actor reuse and phase changes invalidate stale locks. The `$B8` LFSR keeps high-nibble rejection sampling for up to 15 candidates and uses five bits above that; retained locks and zero/one-candidate searches consume no randomness. Full resets restore the seed; checkpoint respawns clear locks while preserving random state. The differential harness compares native and host targeting, RNG, aim points, closing speeds and guidance across all boss parts, scale shifts, dead and protected targets, and wrapping-coordinate extremes. The linked memory map is authoritative for current code and state sizes.

`shots.S` (`fast_shot_scan`, resident) is the native body of `Game::shot_target`, the scan `Game::update_shots` runs for every moved player shot and missile: in slot order it selects each live actor's radius (18 for small debris, 35 for rocks and buildings, else the descriptor's `shot_radius`) and centre offset, then applies `swept_z` over the move and `close_xy` with the release build's wrapping differences, and returns the first hit slot or `$FF`. Its arguments arrive in registers (old depth in A/X, then the position, actor and descriptor addresses and the descriptor count in `rc2–rc8`), it keeps its state in caller-saved `rc2–rc19` and tests the lateral distance first, so a slot more than 255 units to the side costs about 70 cycles and no radius lookup. The compiled scan was about 400 cycles per live slot per shot; the kernel is 441 bytes and 0.7–2.6 k cycles per shot for twelve empty to twelve nearby slots (the missiles fixture's SIM slice fell 21.7 → 16.8 ms at ten ticks; the largest SIM profile keeps 528 bytes of room). `python3 tools/test-shots-kernel.py` compares it with a C transcription of the host loop over 56,564 scenes: every edge of every kind class's hit volume on both sides in both sweep directions, slot-order ties, the wrapping corners and random scenes with random descriptors.

`flight.S` (`fast_flight_step`, resident) is the native body of `Game::move_player`, the player's assisted flight model (`src/game/flight.rs`, a Q8 port of havoc2's aircraft behaviour at the 50 Hz tick). `Game` keeps the integer position in `player` and a ten-byte `Flight` record directly after it: the two position fractions, the Q8 velocities, the bank and the pitch (Q8 of 256ths of a turn; their high bytes are the mesh angles). Each tick the stick sets a target velocity (±3.0 units per tick sideways, ±2.75 vertically, plus a 2.5 push in the roll's direction while the hull spins); the velocity moves 3/32 of the way to it (a 213 ms time constant) by the shifted magnitude of the error and at least one unit while any error remains, so every state settles exactly; a soft wall then bounds it by `(edge + 7 − q) >> 3` on the near side of the ±100 × ±65 lane, so the ship decelerates exponentially into the former hard limits, reaches them exactly and never crosses them; the bank target is −16 × the lateral acceleration (none for a single settling unit) clamped to ±20/256 of a turn, the pitch target −4 × the vertical target, and both angles follow with an eighth of their error per tick. Negative mesh angles lower the right wing, so a right turn banks negative and the 25-tick barrel roll's linear spin runs negative for a right roll too (`Game::player_appearance` adds the spin to the bank, so the end of the full turn is continuous with the banked hull). The arguments arrive in registers (stick bits in A, push in X, the player and flight addresses in `rc2–rc5`), the state lives in caller-saved `rc6–rc18`, and the kernel is 638 bytes and about 1.9 k cycles per tick (0.03 ms); it replaced a 493-byte compiled routine, and the resident Q8 multiply and bounded division kernels moved to the `$9900` table page to make room. `python3 tools/test-flight-kernel.py` compares it byte for byte with a C transcription of the host reference over 147,850 steps: every stick pattern with every push held and released from rest and the lane's corners, and random states (stale fractions, positions outside the lane, extreme velocities and angles) as single steps and continuing flights.

`preparation.S` implements exact native Q16 triangle setup, including sorted-edge orientation parity, signed rounding and near-screen clipping; the positions it stores carry the walker's +32767 bias. Its specialized signed slope helper shares the SHADOW finalization prefix. Host builds retain the Rust reference. Preparation temporarily uses 31 bytes at `$77A0–$77BE`, before row walking or palette packing borrows that same scratch; model and mission buffers remain intact. The linker and memory audit check this ownership.

`python3 tools/test-raster-prep.py` checks both the production Rust entry point and the direct assembly kernel against an independent integer reference. It compares all 28 prepared head/tail bytes for 852 triangles at four alignments, with separate storage and in-place input: 13,632 preparations with buffer and scratch guards. This closes the gap left by the row-walker test, whose inputs are already prepared on the host. Its report lives under `target/raster-prep-test`. `python3 tools/test-preparation-abi.py` separately checks 1,024 calls with changing callee-saved register canaries, stack and I/D flags, immutable input, and output guards; its report lives under `target/preparation-abi`.

`python3 tools/test-corrupt-package.py` exercises package rejection in the calibrated VICE companion using damaged code/model directories, executable images, common and level assets, PRG/REU pairing, SID profile, and the resident SFX bank. It uses private copies under `target/corrupt-package-test`, verifies the error-screen text bytes, saves screenshots and logs, and confirms the input package hashes remain unchanged. It reads failure addresses from the matching ELF and chooses a free monitor port. The default package directory is `dist`. These checks are emulator regressions, not physical C64 Ultimate acceptance.

The final optimized release passed 125 corruption cases per sound profile, including every executable image and all twenty-one music cues. `target/offplanet-validation/release-corrupt-2sid/summary.json` and `target/offplanet-validation/release-corrupt-4sid/summary.json` record all 250 successful rejections and unchanged input hashes.

The following SID soundtrack results describe retired packages and tooling. Current packages use DAC music and reserve all four SID chips for effects; current validation is recorded in [Quad-SID effects](#quad-sid-effects).

The historical `tools/vice-smoke.py --music-checks` derived the SID profile and cue count from the package, then verified every complete cue loop plus an extra page while frames continued. It observed page order, clocks, underruns, authored pitches and lane activity; a held-fire Lowland fixture exercised both effects voices. That four-SID package used ten music lanes; two-SID artifact checks rejected writes to `$D440/$D460`. The archived expansion-release suites selected the eight added full loops with `--music-cues 13 14 15 16 17 18 19 20`.

All twenty-one cues previously passed complete native loops in both sound profiles and both video standards on the frozen `target/offplanet-final/` packages. Those reports are `target/offplanet-validation/final-smoke-2sid-pal/report.json`, `target/offplanet-validation/final-smoke-2sid-ntsc/report.json`, `target/offplanet-validation/final-music-4sid-pal/report.json`, and `target/offplanet-validation/final-music-4sid-ntsc/report.json`. They precede the optimized directory cache and AUDIO transfers; the original expansion release repeated the eight new loops and functional checks on its exact release hashes. The older thirteen-cue suite under `target/music-validation/native-{2sid,4sid}-{pal,ntsc}/` is historical evidence. These monitor-assisted scenes are separate from ordinary-play timing and a complete campaign.

`tools/sfx-previews.py` records the native player through VICE's SID audio output. Historical recordings of *Crystal Current*, *Ashes and Steel*, *Tempest Drive*, *Gorge Warden*, *Citadel Heart*, and *Carrier Overdrive* remain under `target/music-validation/previews-{2sid,4sid}/`; those manifests report zero clipped samples. The orbital score adds *Slingshot Through the Belt*, *Silver Fractures*, *Echoes of Lost Fleets*, *Rivets in Orbit*, *Shadow of the Dreadnought*, *Beyond the Blue Threshold*, *Atmosphere to Starlight*, and *The Last Sentinel*. The six mission cues and final boss cue each contain four distinct eight-bar sections; the ascent lasts eight seconds.

`target/offplanet-validation/music-2sid/recordings.json` and `target/offplanet-validation/music-4sid/recordings.json` pass all eight new recordings: 407.36 seconds per profile of 44.1 kHz stereo WAV audio, with both channels active and zero clipped samples. The WAVs and manifests are bundled in `target/offplanet-validation/music-2sid.zip` and `target/offplanet-validation/music-4sid.zip`. They retain the earlier frozen packages as their recording provenance. `target/offplanet-validation/release-music-equivalence.json` verifies that their complete encoded score banks are byte-identical to that historical release: 62,944 bytes for two SIDs and 146,528 for four SIDs. Those emulator recordings did not validate the former physical-SID mixer balance. Four-UltiSID playback and mixer balance on the Ultimate remain unverified.

The historical soundtrack tests checked cue lengths, distinct melodic sections, loop/page articulation, four-SID serialization, rejection of legacy two-SID data, and the exact *Lost Wing* transcription. Their hardware acceptance covered all twenty-one cues and page boundaries, simultaneous SID music and effects, and the arrangement's mixer balance. Those results do not establish current DAC/SFX behavior. Current physical acceptance uses the [Ultimate audio comparison procedure](audio-check.txt), alongside PAL/NTSC presentation, REU transfers, stack guards, and restart/checkpoint checks. Importing the configuration must expose all four built-in SID instances while the optional physical sockets remain unmapped and muted.

The bootstrap structure was informed by the neighboring `doomc64u` repository. `secondreality/src/output/cfg.rs` provided a local example of the Ultimate mixer and SID-address configuration keys. Digital conversion and the Ultimate Audio backend are adapted from `secondreality-opt`; SID gameplay retains H.A.V.O.C.'s synthesis player.


## Reproduce the README showcase

### Current physical hardware capture (2026-10-24)

The README gallery and teaser now use `HAVOC complete playthrough 10-24-2026.mp4`, supplied as a physical C64 Ultimate recording. Its SHA-256 is `dea37ce30aeb3b0542b0d567609e6d2639dfa5ed81d9f51bddcbdab131e9a3aa`; the source is 640×480 at 60 capture frames per second, with 48 kHz stereo audio. The recorded game build is not independently identified, so the media is labeled by hardware and recording date rather than a release version.

All thirteen PNGs and the video use the fixed FFmpeg crop `crop=534:300:54:90`, producing 534×300 images with square pixels. There is no scaling, stretching, palette substitution, scanline filtering, interpolation, or gameplay retiming. The 60 fps output preserves the recording's cadence and does not claim a 60 fps game renderer. The gallery keeps its twelve scene names and order, plus the title preview.

The editing recipe and evidence are retained under `target/readme-hardware-20261024/`: `assemble.py` extracts lossless intermediate clips and encodes the teaser; `manifest.json` records source hashes and exact frame intervals; `gallery/gallery-selection.json` records still timestamps and output hashes; and `review/` retains the visual contact sheets. Run `assemble.py` with Python 3 on Windows, using the original capture at the path recorded in that script and FFmpeg/FFprobe from `C:/ffmpeg-8.1/bin/`. Generated media and review artifacts remain under `target/`; only selected gallery images belong in `docs/media/`.

The 91.2-second edit retains the existing storyboard: title (4), Lowland Approach (12), Crystal Cave (12), Iron Canyon (12), Foundry Belt (12), Storm Front (12), complete orbital ascent (10), Asteroid Belt (4), Shattered Moon (4), Space Carrier (4), and title outro (5.2). The title bookends are taken from the recording's opening, and the teaser ends before any boss defeat or campaign ending.

The full `assets/music/source/Ashen Wastes.m4a` plays once from beginning to end at its original speed and gain. Captured game audio is muted, and no emulator effects are mixed in. `final/report.json` verifies 5,472 H.264 frames, 48 kHz stereo AAC, 91.2 seconds, 6,505,585 bytes, and a decoded audio true peak of −1.6 dBTP. The high-quality CRF 16 master already fits the 9.8 MB upload limit, so the published copy uses the same bytes. The [hardware teaser MP4](https://github.com/fechols/havoc64u/releases/download/v0.1.4/havoc-teaser-hardware-20261024.mp4) has SHA-256 `4273adcd27e91d0df10cd1127035c032d6fb3fa93d8db4bae9e2880e663a08b2`. Validation includes sampled visual review, full-file decode, and audio measurements; real-time audiovisual playback remains unverified.

### Historical emulator capture workflow

Build the four-SID package first, then run the capture and encoding steps from Linux/WSL:

```sh
python3 tools/capture-showcase.py \
  --package-dir dist --video pal --out-dir target/readme-showcase \
  --audio-backend sfx --preset readme-teaser
python3 tools/encode-showcase.py target/readme-showcase/manifest.json \
  --music 'assets/music/source/Ashen Wastes.m4a' --sfx-gain-db -12
```

Capture uses the compiled C64 program in the calibrated emulator. The current default `--audio-backend sfx` records SID gameplay effects with DAC service disabled; it supplies no replacement soundtrack. Both the top-level and per-scene manifests record that choice. The showcase presents selected scenes rather than a continuous campaign or a physical-machine recording. Keep the footage at its recorded game speed: duplicated video frames must not be presented as a higher game rendering rate. The four-SID recording represents VICE SID synthesis; it does not establish the Ultimate's UltiSID sound, DAC playback or final hardware mixer balance.

The `readme-teaser` preset is 91.2 seconds: title (4), Lowland Approach (12), Crystal Cave (12), Iron Canyon (12), Foundry Belt (12), Storm Front (12), orbital ascent (10), Asteroid Belt (4), Shattered Moon (4), Space Carrier (4), and title outro (5.2). The ascent crosses naturally from the cleared coastal carrier stage into the first orbital mission. The three short orbital clips include a boss glimpse, without a defeat or ending. The preset uses a stateful scenic pilot with inertia-aware steering, persistent safe destinations, and protection against accidental double-tap rolls; game handling is unchanged.

The original Ashen Wastes recording plays continuously from its beginning through its natural ending. The encoder overlays it on the synchronized native SID effects, initially 12 dB below their normalized level, and applies only the global attenuation needed for a decoded AAC true peak below −1 dBTP. This is an edited soundtrack, not a capture of Ultimate DAC playback. Legitimately silent SFX-only title and ascent takes are allowed; combat takes still require effect activity and audible audio. Source hashes, gains, timing, and peak measurements are recorded in the encoding report.

Omitting `--preset` retains the original full storyboard and pilot. Repeat `--scene <name>` to select scenes (in preset order when a preset is supplied), or use `--duration 1` for short capture probes. The README gallery also needs `carrier-combat`, `ship-graveyard`, `orbital-shipyard`, and `jump-gate`; capture these without the teaser preset, using `--duration 3` for clean still candidates. Artifact readers can still inspect historical two-SID packages when an explicit package path is supplied; current builds always contain four SIDs.

The encoder writes `target/readme-showcase/havoc-showcase.mp4` by default, bounded below 9.8 MB for upload. With `--music`, it also retains a high-quality `havoc-showcase-master.mp4`. It requires FFmpeg and FFprobe; use `--ffmpeg <path>` and `--ffprobe <path>` to select their executables. Native Linux and Windows executables are supported, with WSL path conversion for Windows tools. Omitting `--music` retains captured audio only. Preserve the capture manifest and source recordings with the generated video so its package, scene selection, timing, and audio can be checked before publishing. Generated capture artifacts stay under `target/`; only the selected README images belong in `docs/media/`.

The 2026-10-21 README refresh is preserved under `target/readme-refresh-20261021/`. Its frozen `package/` and `build-provenance.json` identify the four-SID build with music kernels enabled; `final/manifest.json` selects the eight verified takes and `final/encoded/report.json` verifies the 72-second MP4. The capture pilot uses physical joystick input with inertia-aware avoidance, and the Asteroid Belt preview starts at the beginning of the stage on a low scenic flight path. All retained takes preserve full shields and pass the existing raster-effect, geometry, input, and music checks.

The earlier 2026-10-24 emulator refresh is preserved under `target/readme-refresh-20261024/`. Its frozen v0.1.3 `package/` and `build-provenance.json` identify the upgraded renderer; `takes.json` and `assemble.py` select eleven verified takes. `final/review.json` verifies 709 retained native frames, unchanged package hashes, full shields and three lives, zero rolls, and the complete orbital ascent. `gallery-selection.json` identifies all twelve replacement screenshots and the title preview. Visual frame sequences and trajectory checks are retained under `review/`; they are distinct from a real-time playback review. `final/encoded/report.json` verifies the 91.2-second, 4,444,861-byte MP4, full Ashen Wastes source usage, and a decoded audio true peak of −4.9 dBTP. The historical [emulator teaser MP4](https://github.com/fechols/havoc64u/releases/download/v0.1.3/havoc-teaser-v0.1.3.mp4) has SHA-256 `5e7de40765e1e528d2ec1612dafb5ca161f21b6c89d9203785e925834925dc4f`.

## Terrain visibility coverage

The player hull/exhaust, terrestrial rocks and buildings, and ordinary-stage
mounted enemies (including Canyon wall batteries) use the
`TERRAIN_CLEAR_GEOMETRY` command flag (`0x20`). This is an intentional visibility
priority over exterior terrain, including the shallow embedded bases of rocks
and buildings. Their positions and collision bounds remain unchanged. The flag
survives near clipping and preparation without changing the 16-byte command or
12-byte tail. Drawing these faces sets pixel bit `0x40`; ordinary foreground
writes replace the color while retaining that coverage, and tagged exterior
terrain skips covered pixels. Boss turrets, orbital hazards, airborne enemies,
effects and decorative scenery receive no protection. All foreground objects
retain their normal ordering relative to one another.

The hull stays inside X -136..137 and above world Y -103 for all 256 roll angles;
tracked guns fit the same minimum 145-unit lane. Fortress bunkers reach X +/-158
but stay at Y >= -98, above the conservative campaign shoulder envelope
`495*(y+105) = 240*(abs(x)-145)`. Authored ground enemies spawn after their incoming
terrain blend finishes. Transitions clear actors, and attract previews use their
stage's terrain style. These geometric bounds still apply to the ship, tracked
guns and bunkers; rocks and buildings intentionally use the broader visibility
policy even where their bases extend below the floor. Tests cover terrestrial
ground actors and their proxies, the original twelve terrain sequences and
eleven terrestrial crossings, plus orbital exclusions. Extend this policy only
to actor classes intended to remain visible through terrain.

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

## Experimental span renderers

The default-off `span-buffer` feature resolves each eight-row scene band before
painting cells. It retains the stable painter order and existing Q16 edge setup
and stepping. `src/render/spans.rs` is the host reference; `asm/spans.S` supplies
the native compositor. This removes overlapping scene paint submissions, while
band initialization, palette spill expansion, and the later HUD remain separate.

The deferred backend skips a contained write when its complete color/coverage
value is unchanged. Uniform overwrites splice out the covered runs in one pass:
terrain-clear writes always qualify; ordinary/exterior writes qualify before
any protected coverage has appeared in the band. Later writes retain the
per-run coverage checks. The compositor owns the per-band coverage flag, even
when a protected write takes an early return. Row pointers are prepared once
per triangle band and advanced with the Q16 walker, including clipped rows.

Each row is a list of maximal color/coverage runs indexed by its starting pixel.
The successor byte is also the exclusive end, with 160 as the sentinel. Eight
rows of successor and ink bytes occupy `$D000-$D9FF`, borrowing the triangle
commands only after **all** native preparation completes. The six-byte renderer
header and persistent edge heads/tails remain intact. Host heads reuse commands,
so the host compositor has separate scratch. No framebuffer or REU transfer is
added. The list has at most 160 runs per row and needs no allocation or overflow
fallback.

Dust enters first; ordinary ink preserves coverage, protected geometry sets it,
and exterior terrain skips protected runs. Insertion splits boundary runs and
coalesces equal neighbors. Flushing converts coverage bit 6 into the cell writer's
protected paint mode and emits every final non-background scene pixel once;
protected background runs are retained. HUD rendering and quantization follow.
The resident insertion core calls no overlay, and only PIXELS may call it. Its
`$E1-$E9` scratch preserves the walker's live registers; it is unreachable from
the IRQ. Linker, ownership, and negative-fixture audits enforce these boundaries.

Removing hidden colors can change unused palette slots, which may change a later
overfull cell's equal-frequency color selection. Tests require identical raw
scene pixels and coverage, and compare both renderers under the same preceding
palettes to distinguish that history effect from visibility or HUD errors.

The alternative `span-front-to-back` feature implies `span-buffer` and selects
the front-to-back backend instead of linking both. `src/render/front.rs` and
`asm/front_spans.S` walk the exact reverse of the stable painter order. Each row
starts as one uncovered interval. Geometry claims visible fragments and sends
them directly to the ordinary cell writer; clear-colored fragments also claim
ownership. Dust fills the remaining holes after geometry, and there is no flush.
The two arrays at `$D000-$D9FF` hold hole successors and ends, while eight row
heads borrow retired preparation scratch at `$77AC-$77B3`. The maximum is 80
disjoint holes per row. No buffer, transfer, or persistent allocation is added.

Before front-to-back painting, a noncommitting pass over terrain-clear faces
unions their coverage into the cells. It copies Q16 heads into the existing
walker scratch and returns before any persistent head store; tails are read-only.
This makes exterior rejection independent of reverse traversal. The main pass
advances every active head once, including fully obscured geometry and hidden
bands. Seeded coverage survives cell spills. Terrain-clear takes precedence when
both terrain flags are present, matching the painter. The coverage prepass and
hole bookkeeping are real costs, so fewer writes alone do not establish a win.

All backends share per-pixel HUD priority. The cell coverage byte now holds
terrain bits in its low nibble and HUD bits in its high nibble. Every effective
write replaces its pixel's HUD bit, including background writes, and spills
reconstruct the old metadata before applying the new write. `$9328` latches
whether any HUD write has occurred, preserving the scene fast paths beforehand.
The former `$7700-$7727` slot flags are used only for final screen staging.

Build isolated benchmark packages with
`bash scripts/build-benchmark.sh target/span-buffer/candidate-4sid spans`
(use `front` for front-to-back). Omit the final
argument, or use `painter`, for the default renderer. Native production builds opt in with
`cargo build --release --features span-buffer` from `target-c64` in the configured
toolchain; use `span-front-to-back` to select the other backend. Both features
remain off in normal builds. `HAVOC_SPAN_PREPARED_POINTERS=0` builds the deferred
pointer-hoisting ablation with no runtime branch. See
[the measured optimizations](benchmarks.md#span-renderer-optimizations-2026-10-21).
