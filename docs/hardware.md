# H.A.V.O.C. hardware and build notes

H.A.V.O.C. (Heavy Assault Vessel: Orbital Combat) targets the C64 Ultimate at 64 MHz with its built-in REU configured to 16 MB (16 MiB). The single package in `dist/` uses Ultimate Audio for digital title/attract/victory music and four built-in UltiSIDs for gameplay; it requires no physical SID chips. Import its `havoc.cfg`, load its `havoc.reu` at REU address zero, then run its `havoc.prg`. Keep all three files from the same build together. Historical 1 MiB and 8 MiB packages retain their original memory settings when reproducing older results.

The configuration selects the C64U turbo registers, 64 MHz CPU speed, disabled badline timing, and quiet bus operation. The runtime programs the turbo register at startup and checks that the setting is available. An ordinary 1 MHz C64 is not a supported execution target.

## Graphics and memory

Flying objects cast fixed-size, recognizable silhouettes directly downward onto
one known horizontal plane at world Y = -105. This is both the terrestrial floor
and the Space Carrier's flight deck. Named per-level switches in `src/game.rs`
enable or disable shadows. `Game::shadow_ground_y()`
keeps the enable rules: Lowland Approach through Carrier Assault and
Space Carrier are on; the other five orbital levels are off. Orbital crossings
and the ascent are off while scenery moves away from the fixed plane. The named
settings compile into three enable bytes without changing the checked stage
format. Every enabled level and terrestrial crossing uses C64 dark gray 11,
defined once as `SHADOW_COLOR` in `src/shadow.rs`. No palette lookup or color
change is needed at transition entry, during the crossing, or on exit.

`SMA6` retains nine 12-byte descriptors in each model bank's final 128 bytes:
template family, quarter-turn heading, local XZ offset and half extents derived
from the final quantized mesh. Craft, hull, diamond and rectangular templates
cover models, projectiles, pickups and decorative flying objects. Their outline
coordinates use shifts and additions; each vertex receives the same
`-105 - camera_y` height before ordinary perspective projection. Scenery geometry,
slopes, deck edges, raised objects and occlusion do not affect the silhouette.
Caster centers at or below the plane are skipped. Shadows use a single solid
dark gray ink without sampling or recoloring the surface beneath them.
They can blend into matching ground patches; overlap with scenery objects or
another shadow is intentional.

Ordinary geometry finishes first. Shadows use the actual unused entries of the
256-command array, with no separate shadow-command cap. Each caster is an
append-only transaction: insufficient command capacity rolls back its entire
silhouette while preserving every ordinary command. Ordinary commands keep
their stable depth order; shadow commands form a final suffix painted by the
ordinary rasterizer. They carry neither sky nor terrain-clear flags. No receiver
records, parent links, plane normals, surface clipping or special shadow raster
path remain. The renderer stays 4,102 bytes, commands stay 16 bytes, Game stays
637 bytes, and model banks stay 2,048 bytes. The 64-caster frame uses 711 bytes
on the native target (776 on the host), down from 5,768 native bytes. It remains
bounded to 1,024 bytes of caller-owned scratch, reused before raster preparation.

The native driver transfers one 10,936-byte SHADOW image after GEOMETRY. Its entry
generates enabled shadows, then sorts and prepares the complete command list.
Disabled levels collect no caster descriptors and load only the 2,874-byte
ordinary sorting and preparation prefix; an audited guarded entry prevents
calls into the unloaded generation tail. The separate drawing overlay and wide-arithmetic helper are
gone. Preparation overwrites retired shadow scratch with raster heads, tails
and masks before PIXELS. All three native raster backends pass layout and
overlay audits; the production build retains 64 resident bytes, 14 table bytes,
and 5 constant bytes of headroom without raising any memory ceiling. Timings
and stress limits are in `docs/benchmarks.md`.

The display is native VIC-II multicolor bitmap mode: 160×200 logical pixels drawn from the 16-color C64 palette. Each 4×8 logical-pixel cell has the global background colour plus three local colors; the background register shows the frame's clear colour (the sky on terrestrial stages), so unpainted pixels and ink of that colour cost no cell slot. Flat triangles paint two-bit colour codes straight into the cells; only a cell asked for a fourth other colour is quantized per pixel. There is no texture mapping.

The game draws into a single VIC bank. All twenty-five eight-row bands contain scenery, including the pixels behind the cockpit. Each band is rasterized, given any bitmap captions, and fully packed in scratch RAM before a bounded copy publishes its bitmap, screen and color bytes. Publication proceeds immediately unless the raster is near that band's fetch window; it waits locally when necessary, with no whole-frame v-sync wait. A frame still rolls down the picture as it is produced. Seven execution phases share one RAM window, with four ordinary enemy-profile SIM images, three boss-profile SIM images, and a terminal SIM image stored in REU. The SID tick player and interrupt handler remain resident; the DAC decoder runs in AUDIO. BOOT runs once; frames then run SIM, optional AUDIO page service, SCENERY, GEOMETRY, SHADOW generation/sorting/preparation, and PIXELS in order. Disabled scenes load only the ordinary sorting/preparation prefix of SHADOW. The AUDIO overlay validates the complete score bank during startup and validates and prefetches requested pages during play; it is never called by the interrupt. Later AUDIO transfers omit the startup-only validator and directory parser.

Eight unexpanded multicolor sprites form four floating cockpit readouts, with two adjacent images per corner. Each 24×21 logical-pixel area contains only text and functional bars: lettering and gauge outlines are white, filled gauge portions and warnings use color, and empty gauge interiors are gray. Every pixel outside the text and bars is fully transparent; there are no backing panels or decorative accents. The upper-left readout holds the six-digit score; upper-right holds lives, three bomb rectangles, and a compact yellow boss meter when needed. Bomb rectangles are yellow when available and gray when spent. Lower-left shows shield strength; lower-right shows charge and homing readiness. The level name appears in the section splash rather than a permanent strip. The images occupy exactly 512 bytes at `$8C00–$8DFF`, with sprite pointers `$30–$37` at `$83F8–$83FF`. Shared sprite colors are gray (`$D025=12`) and white (`$D026=1`); the inventory pair keeps its individual color yellow even while a boss is active. Title, attract, and victory hide the cockpit, while their bitmap panels remain available.

A small displayed-value cache identifies dirty corners. PIXELS stages each changed pair in the retired workspace at `$8E00–$8E7F` with its functional color at `$8E80`, then writes only image bytes and color registers that actually differ. Unchanged corners keep their existing images without redraws. The publisher waits outside raster lines 44–83 for top corners and 212–253 for bottom corners, then masks interrupts and samples again before the bounded copy. The line-251 IRQ derives all sprite positions from fixed bases plus the bitmap's fine-scroll sample, so bomb and hit shake move the instruments with the scene without accumulating drift. `tools/test-hud-kernel.py` executes the assembled routines across 482 PAL/NTSC publication cases and 129 scroll samples, including IRQ races, raster rollover, dirty-byte checks, and counter wrap; its conservative model measures a maximum masked interval of 1.192 PAL lines, below the two-line budget.

Pressing F toggles a frame-rate readout in tenths within the upper-left sprite pair, and the space bar pauses the flight: simulation time stands still behind a double-height PAUSED caption while frames keep rendering, stale joystick samples are discarded and the simulation deadline follows the clock so resuming does not replay the missed ticks. PIXELS counts completed frames over windows of at least fifty 50 Hz audio ticks and scans keyboard rows 7 (Space, CTRL, Commodore), 2 (F), and 0 (DEL) once per frame with the sampling interrupt masked, returning CIA 1 port A to input immediately so joystick port 2 remains undisturbed. A column counts as a key only when it also reads high with no row selected, so a joystick in port 1 or an emulator driving port B cannot fake a press. The readout is off at startup and can appear by itself even while the cockpit is hidden; pausing is ignored outside Playing and Transition.

Secret music preview: press **CTRL + C= (Commodore) + DEL** on the title screen or during attract mode to toggle between **Hammond Rocker** and **Galactic Dash**, even before winning. Each press starts the selected song from the beginning; holding the chord does not repeat. Keep CTRL and Commodore held and tap DEL again to switch back. The scene, camera, and attract timer continue unchanged. Selection persists across scene changes and new runs until reload, which restores Hammond Rocker; completing the campaign selects Galactic Dash automatically. The chord is ignored in gameplay and result screens. It shares the existing key-edge latch and menu selection byte, and the AUDIO overlay reuses its saved sampler calibration when switching songs.

Player damage reuses the bomb's resident impulse sequencer with a lighter profile: a `Hit` presentation event (outranked by a pending bomb, replaced by a reset) runs 240 ms of two-pixel horizontal kicks with at most one pixel of vertical motion and an 80 ms red border, packed into the same single byte as the bomb countdown with bit 7 selecting the profile. A hit never shortens a bomb shake in progress.

Damaged enemies and whole bosses turn white for 200 ms, then recover in their normal colors for at least 200 ms before another damaging hit can start a flash. Damage during either phase still applies but never extends or queues feedback. GEOMETRY prepares these phases once per frame using the 20 ms simulation clock: a new hit waits for its first white frame, and catch-up can advance only one phase, so it cannot skip the normal-color interval. Slow rendering may extend either phase; pausing stops its clock. One byte per actor plus one for the boss and a shared frame tick keep the game state at 637 of its 640 bytes without changing the 16-byte actor assembly layout. Meshes and distant proxies share the color override.

Bomb impacts use the VIC fine-scroll and border registers, adding no triangles or bitmap copies. A one-byte simulation event records a spent bomb independently of inventory rewards and the audio queue. SIM drains the latest bomb/reset event after catch-up and updates a resident sequencer with interrupts masked. Its 32-tick countdown runs on the existing 50 Hz clock; the raster interrupt at line 251 applies sixteen 40 ms samples. Horizontal kicks reach six physical pixels and vertical motion reaches three pixels either side of the normal Y-scroll value, tapering to rest over 640 ms. The border spends 80 ms each at white, yellow, orange, and red, then returns to black. Frame completion preserves active Y-scroll, and register writes preserve display-enable and bitmap-mode bits while keeping the raster comparison below line 256. Respawn and scene resets cancel the effect; scenic-flight seams preserve it, and a finishing bomb can continue through the results screen. Bitmap-only frame dumps omit presentation registers, so shake effects are checked with native traces and VICE captures; sprite-aware captures also record the independent cockpit images and VIC state.

The bomb build passes 171 host tests per SID profile, with one existing fixture export ignored, and both production packages pass the PAL/NTSC weapon checks. Physical joystick gestures produce every shake phase, motion between frame publications, active retriggering, and neutral expiry; empty-inventory gestures produce no impulse. The observed countdowns last 623-639 ms, within the expected 20 ms clock quantization, with register updates following at the next raster interrupt. The two-SID ordinary-frame timing sample remains 5.493 FPS PAL / 5.766 FPS NTSC before and after the change. These short emulator samples do not establish physical-machine performance. Reports are under `target/vice/bomb-{2sid,4sid}-{pal,ntsc}`; the two-SID PAL run also covers controls, scenic streaming, and results-screen restarts.

Production companion screenshots have an existing screen/color-fetch defect with `$D031` bit 7 set: its matrix-fetch prefetch counter stays at four while badline stalls are disabled. Baseline captures made before the bomb change show the same corruption. Decoded bitmap/screen/color bytes and the VIC register traces validate rendering data and presentation timing, but final visual acceptance remains a physical C64 Ultimate check. The companion limitation is documented in the neighboring `doomc64u/docs/dual-image-game.md`.

| Address | Use |
|---|---|
| `$0002–$0021` | LLVM-MOS imaginary registers |
| `$00D0–$00E0` | Row walker zero page during PIXELS: the current triangle band's four Q16 edge values and its row count (`target-c64/asm/raster.S`); the compiler's zero-page region `$0022–$00EF` is otherwise empty (`tools/check-layout.py` asserts it) |
| `$00F0–$00FF` | Cell writer zero page: the span being painted and its hot temporaries; `$FE` the span colour's key (colour XOR the clear colour), `$FF` the band's clear colour, read by the quantizer as its background (`target-c64/asm/cells.S`, `pack.S`) |
| `$0200–$071F` | Resident globals, including a 168-byte validated music directory, and compiler static scratch frames; all end below `$0720` |
| `$0720–$07FF` | Next-score page, 224 bytes |
| `$0801–$2DFF` | BASIC startup, resident code, data initializers |
| `$2E00–$2FFF` | First 512-byte mission-segment buffer; the linker prevents resident code from growing into it |
| `$3000–$71FF` | BOOT, SIM, GEOMETRY, SHADOW, PIXELS, AUDIO, or SCENERY code overlay; render prefixes end below `$6F20` |
| `$6F20–$71FF` | Shared projection helper during GEOMETRY/SHADOW; reloaded PIXELS helper code and ambient-particle frame afterward |
| `$7200–$74FF` | Music kernel slot: the playing cue's per-song modulation code, copied from the REU at each cue change |
| `$7500–$75FF` | IRQ software stack (measured high-water mark: 3 bytes) |
| `$7600–$77FF` | Sorted command order written by the depth sort (`$7600–$76FF`, one command index per sorted position; BOOT's directory scratch before any frame), screen staging (`$7700`), cell overflow flags (`$7728`), cover-clean marker (`$7750`), then preparation, row-walker and quantizer scratch from `$77A0` |
| `$7800–$7FFF` | Resident constants |
| `$8000` / `$A000` | The single visible screen / bitmap (VIC bank 2), updated in place band by band |
| `$8400–$8BFF` | Current level's model bank and enemy descriptors |
| `$8C00–$8DFF` | Eight persistent 64-byte multicolor HUD sprite images |
| `$8E00–$91FF` | 64-entry projected-vertex cache, then depth-sort scratch after all projection |
| `$8E00–$92FF` | Pixel form of overflowed cells during PIXELS; the workspace also holds startup checks, SIM/SCENERY helpers, initial AUDIO directory scratch, and one staged HUD corner in separate lifetimes |
| `$9300–$93FF` | GEOMETRY kernel scratch (matrix, rotation, projection, emit, replay, mesh, near clip at `$93D0`); PIXELS reuses `$9300–$9328` for the cell writer's rare paths and HUD-seen flag; SIM reuses `$9340–$937F` for the missile guidance kernel |
| `$9400–$96F7` | Cell writer band state during PIXELS: 8×40 two-bit code bytes, 8×40 coverage nibbles, three 40-byte palette slots holding keys (colour XOR the clear colour; 0 = free, `$FF` ×3 marks a cell that lives in the pixel band) |
| `$9700–$98FF` | Second 512-byte mission-segment buffer |
| `$9900–$9FFF` | Resident math/palette tables and compact kernels: the depth sort, the Q8 multiply and the bounded division |
| `$BF40–$BFFF` | Tail of IRQ static-frame backup |
| `$C000–$C3FF` | Quarter-square table for the exact Q8 multiply, built by BOOT (former screen B) |
| `$C400–$C7FF` | Main software stack |
| `$C800–$CA7F` | Game state |
| `$CA80–$CDFF` | Main IRQ static-frame backup |
| `$CE00–$CFAF` | Resident audio allocation; a 400-byte union holds the SID engine or DAC decoder state at `$CE00–$CF8F` |
| `$CFFA–$CFFF` / `$D000–$DFFF` | Renderer header / triangle commands beneath I/O, left in emission order for the whole frame |
| `$D000–$D9FF` | Optional span storage in retired triangle commands after raster preparation |
| `$E000–$E7FF` | Additional SCENERY authoring helpers, retired before raster preparation |
| `$E800–$FF1F` | Reserved shadow workspace before raster preparation; the 64-caster frame uses at most the first 1,024 bytes |
| `$E000–$EBFF` | Twelve-byte persistent raster tails during PIXELS, after shadow work retires |
| `$EC00–$EF1F` | Twenty-five ordered triangle masks, 32 bytes per band |
| `$EF20–$FF1F` | Sixteen-byte persistent raster heads, one per sorted position |
| `$FF20–$FFF9` | Free RAM; the CPU vectors follow at `$FFFA` |

The PRG spans `$0801-$9FFF`, including resident constants and zero-filled gaps; its load image excludes phase overlays. BOOT's initial 3,072-byte common-bank validation temporarily writes `$8400-$8FFF`, so sprite images are initialized only after it finishes; later model loads end at `$8BFF`. The CPU can use the shared workspace across `$9000` normally; the VIC character-ROM window is avoided by keeping every sprite image below that address. The IRQ backup tail remains reserved. The nine code bases at `$010000`, `$020000`, `$030000`, `$040000`, `$050000`, `$070000`, `$080000`, `$090000` and `$0A0000` remain in place. Common data and encounter allocations are unchanged, and eighteen model banks occupy `$0B0000-$0B8FFF`. The remaining code images fill unused gaps below the models:

| Code images, in order | REU bases, respectively |
| --- | --- |
| Victory GEOMETRY, terminal SIM | `$015000`, `$01A000` |
| Warden, citadel, flagship SIM | `$025000`, `$02A000`, `$035000` |
| Transport, meteors, SIM helpers | `$03A000`, `$055000`, `$05A000` |
| Ascent, asteroid, moon SCENERY | `$060000`, `$065000`, `$06A000` |
| Graveyard, shipyard SCENERY | `$075000`, `$07A000` |
| Space carrier, gate SCENERY | `$085000`, `$08A000` |
| SHADOW, SCENERY helpers | `$095000`, `$0AA000` |

The music reservation occupies `$0C0000-$0FFFFF` (256 KiB). Every code reservation stays within one 64 KiB bank, and PIXELS retains its exclusive bank. Existing logical overlay IDs retain their values: ID 31 is SHADOW, ID 32 is unused, and ID 33 holds SCENERY helpers. A linker-authored, 34-byte table in resident `.tables` maps IDs to REU bases in 4 KiB units. The loader resolves each base once before its bounded DMA loop. The helper image contains SCENERY code at offsets zero and `$500`, loaded into `$8E00` and `$E000` for that phase only. OVL7 checksums all 26 complete code images, including padding and helper portions.

The build produces an exactly 16,777,216-byte REU image. Existing code, models, encounters and SID scores retain their addresses in the first MiB. Galactic Dash occupies the reservation `$100000-$4FFFFF` and Hammond Rocker occupies `$500000-$FDFFFF`; the decoder writes a shared 131,072-byte interleaved stereo PCM ring at `$FE0000-$FFFFFF`. That ring is mutable during playback and starts zero-filled in the packaged image. Game state remains in C64 RAM. The separate hardware timing probe writes solely to unreserved padding at `$0BFC00-$0BFFFF`. The packer reports reserved and unreserved totals from the linked layout.

The historical 1 MiB layout's same-source comparison against a 16 MiB build passes eighteen four-SID PAL fixtures with identical game state, geometry and native pixels. Measured frame-time changes range from -0.209% to +0.177% in calibrated VICE, with one sample per fixture; these are not physical-machine measurements. `target/reu-1m/performance-summary.json` and its source manifest record the comparison. Benchmark-only builds keep the unchanged segment-checksum helper in resident RAM to accommodate their SIM instrumentation; production retains its existing SIM placement.

The earlier dual-profile 1 MiB packages passed 311 host tests, 119 layout rejection fixtures, 64 overlay rejection fixtures and 127 package-corruption cases per sound profile. Each profile also passed 43 streaming/respawn/restart scenarios and all 21 complete music loops under both PAL and NTSC with a 1,024 KiB emulated REU and no music underruns. `target/reu-1m/validation-summary.json` records those historical package hashes, reports and verified copies to the hardware drive.

On 2026-10-21 the user reported that the 1 MiB build looks and sounds great on physical hardware. The sound profile, video standard and campaign coverage were not specified.

The frozen historical 16 MiB `target/offplanet-release/` images contain 496,349 bytes of serialized payload for two SIDs and 580,001 bytes for four SIDs. This counts 256 bytes of package/model/code directories, 3,072 common bytes, thirty-six 2,048-byte encounter/model banks, the twenty-four image lengths recorded by `OVL7`, and the `SMU3` bank's recorded total length. It excludes allocation padding without treating zero-valued payload bytes as free space.

Each composed SIM image has the same common prefix and one profile tail. The audited `encounter_step(Game*, AssetBank*)` entry owns actor movement, attacks, and the optional boss update. Shared code handles controls, progression, spawning, collisions, projectiles, and pickups. Stage identity is independent of this code profile: changing stages always refreshes the encounter metadata and model bank even when the next stage shares code. Native SIM reloads every frame because later phases reuse the window. A catch-up stage/profile change returns to the resident loader before another tick or render, preserving its existing fixed deadline. Enemy updates remain suspended during scenic flights.

SIM also loads a checked helper image from REU `$05A000` into `$8E00–$92FF`, a 1,280-byte execution allowance. This reuses retired startup-check space and the later geometry/pixel workspace; it does not replace the model cache, sprite images, or mission buffers. The resident loader restores it in two transfers before every SIM entry because later rendering phases overwrite that RAM. All general DMA transfers are capped at 1,024 bytes to bound audio IRQ latency. The IRQ never calls the helper image.

The 128-byte `OVL7` directory at REU `$000100` records twenty-six checked code images: existing phases, all eight SIM compositions, victory GEOMETRY, ten composed scenery images, the SIM helper, SHADOW and the SCENERY helper. The ascent and six orbital scenery records occupy bytes 88-115; SIM helper fields remain at 84-87, SHADOW uses 116-119, and SCENERY helpers use 58-61. The directory also authenticates SID configuration and the PRG, constants and startup code. SIM images retain the common padded size for startup validation and the eight-entry `__sim_load_sizes` table for shorter runtime transfers. Boss kinds 4 and 5 reuse SIM profiles 6 and 5; terminal profile 7 remains distinct. The packer rejects overlapping allocations and images beyond the 16 MiB REU. Build the PRG and REU together.

Package ID `$534D0C00` identifies outward-wound `SMA6` models with baked shadow descriptors in the existing 16 MiB stereo digital-music layout. It retains eighteen 2,048-byte model banks at `$0B0000-$0B8FFF`: each mesh descriptor keeps its vertex/face offsets and counts, coordinate shift (byte 6, 0–3), and a reserved zero byte (byte 7). Vertices remain three `i8` bytes in units of `1 << shift`. Every regular enemy, the player, the pickups and the medal fit shift 0 exactly; ordinary slot-4 hulls use shift 1 (their doubled coordinates are even, so they are exact too) and only the Dreadnought and Gate Guardian need shift 2. Faces keep three indices and a colour. Every triangle is backface culled; solid models have outward surfaces and thin sheets have explicitly authored reverse faces where both sides must be visible. Older model magic and nonzero reserved descriptor bytes are rejected. The 64-byte `MDL2` directory at `$000040` retains version 2: magic at 0-3, stage count at 4, version at 5, bank size at 6, REU base at 8, eighteen checksums at 12-47, zero padding at 48-61, and checksum at 62. BOOT validates every bank, leaving stage 0 in the existing `$8400-$8BFF` cache. A stage change checks and copies the incoming bank in two 1,024-byte transfers. Same-stage frames and respawns retain the cache; model views never survive replacement.

The 64-byte main header contains eighteen encounter checksums at 20-55, zero padding at 56-61, and its checksum at 62. Eighteen 2,048-byte encounter allocations occupy `$002000-$00AFFF`; only their checked 512-byte prefixes enter the two gameplay buffers. `SMS2` keeps spawn records at byte 16 and the 62-record maximum. Bytes 8-15 hold the unchanged eight-byte `StageInfo`: enemy profile, music cue, crossing profile, boss kind, current/next/previous themes and one reserved zero byte. Complete campaign authoring remains host-only. Orbital encounters use ordinary movement profiles 0-3 and explicit three-dimensional hazard positions; ROCK flag bit 1 (`SMALL_DEBRIS = 2`) marks a shootable fragment. Unflagged rocks resist guns and missiles. A bomb destroys either size of active orbital ROCK, including wreckage, for 25 points without advancing enemy-kill rewards.

`SMA6` retains nine mesh descriptors at bytes 16–87 and three eight-byte enemy descriptors at 88–111. Header bytes 5, 6, and 7 identify stage, active enemy count, and boss kind. Ordinary stages expose three enemy slots; boss stages expose two escorts and reserve mesh slot 3 for boss side units. Mesh slot 4 contains the crawler, reactor, carrier hull, Dreadnought or Gate Guardian ring; slot 8 contains the 70-vertex / 116-triangle medal, including its bronze cockpit and twin engine relief. The tightest mesh payload ends at byte 1,565, below the shadow-descriptor boundary at byte 1,920. The player and pickup models remain shared; crystal and volcanic stages customize obstacle meshes. The 3,072-byte common bank contains the initial models and score directory. Runtime encounter fetches checksum their previously validated 512-byte prefixes.

Combat meshes are baked at their final size so the C64 keeps identity transforms. Regular enemies (slots 1–3 in ordinary stages, slots 1–2 in boss stages) use `assets::enemy_scaled`, three halves of the authored coordinates with truncation toward zero; their hit, ram and center descriptors scale the same way (shot radius 28, ram radius 27). The boss hull in slot 4 and the boss side units in slot 3 of boss stages keep `assets::BOSS_SCALE` (2×), as do the boss layout constants. Ground guns keep their −8 base through the pivot. Actors and pickups are no longer removed in front of the camera: `game::ACTOR_RETIRE_Z` keeps every actor alive and submitted until the deepest actor mesh is entirely behind the near plane, where the renderer rejects every face; `tests/assets.rs` checks that invariant against every stage's model bank. Flat pickups and hostile shots retire at the near plane itself. That plane (`render::NEAR`) now sits 8 units from the eye instead of 32: a vertex is on-screen only when |x| < z and |y| < 0.75 z, so geometry can be cut visibly only inside that small pyramid, and passing ships leave the screen edges before they reach it. Every actor, terrain (|x| ≤ 640) and scenery coordinate still projects inside the ±8191 clamp at that depth, and `projection::project_uncached` bounds |y| to 30,000 at unshifted depths, where larger values already saturate, so the i32 vertical product cannot overflow.

SCENERY lays out the terrestrial prefix first, followed by shared space helpers and a wildlife, transport, meteors, ascent, or orbital environment tail. Ordinary terrestrial frames transfer only `__phase_scenery_terrain_size`, omitting both space helpers and decoration tails. A terrestrial crossing additionally loads its selected decoration at the fixed `scenic_decoration` address, leaving the intervening space-helper region unloaded. Ascent and orbital frames load the selected full composition. The orbital tail also draws its outgoing crossing, while the existing transition state and sound-effect queue handle ascent. The complete padded images remain checked at startup. GEOMETRY retains its player/projectile/effects prefix and combat or medal drawing. The relocation audit accepts only the reviewed opaque entries and rejects cross-profile references. Every borrowed asset view ends before its DMA destination is reused.

Victory reuses the existing mode with a 400-tick reveal counter. The first 100 ticks freeze and darken the ship and battlefield; medal geometry is appended after color remapping so it stays gold. Once black, scenery and ship emission stop. The alternate GEOMETRY tail replaces combat drawing. The terminal SIM tail handles title/menu changes, respawning, scenic crossings, Victory and GameOver; ordinary attract ticks use the scene's enemy profile, returning to terminal SIM for dismissal or the next preview. Mode/profile changes return to the resident loader before another catch-up tick. The medal shares the ordinary mesh-transform instantiation through a camera-neutral scene budget; it does not duplicate that code or enlarge its vertex cache. PIXELS paints all 25 bands and shows only the final captions after tick 400. The complete fading scene uses at most 160 triangles, including the detailed medal.

PIXELS uses a split image: its prefix may occupy `$3000-$6F1F`, while a small helper tail occupies `$6F20-$71FF`. The existing page-4 `OVL7` record checksums the prefix, zero-filled gap, and tail as one image. Runtime loads only the prefix, then maps I/O in to DMA the tail from `$043F20`, restoring raster access before drawing. The tail contains centered text, label lookup and sprite-meter helpers, and a regenerated 16-byte particle frame. Neither transfer overwrites the prepared edge or mask workspace; no additional resident state is allocated.

Levels 13–18 draw eight staggered gray pixels as a forward-motion cue. Their radial positions use the continuous 50 Hz animation clock, so crossings and PAL/NTSC frame rates do not reset their phase. They are painted after clearing and before triangles, allowing black hull surfaces and the HUD to occlude them. They add no actors, collisions, or geometry commands. Space Carrier keeps three alternating hull rows and retires each only after its rear geometry has passed the near plane.

Checked crossing metadata selects wildlife, cargo transport, meteors, ascent or orbital flight. The original three decorations retain their existing bounded geometry; ascent raises the view from the carrier coast into the starfield, and orbital crossings retain the active space environment. All scenery must fit the 80-face transition reservation. The common SCENERY prefix selects sound cues after simulation catch-up; foreground Runtime owns its small scene/time history. A frame submits only the newest still-valid cue per lane, preventing missed frames from replaying old bursts. Distant scenery has no collision state; authored ROCK actors supply the visible hazards.

Cue publication and scene-only cancellation briefly mask interrupts and update audio RAM without touching SID registers, even while I/O is banked out for scenery. The existing resident 50 Hz effects player owns all envelopes and pitch changes on voice three of SID 0 and SID 1. Scenic effects outrank basic gunfire and yield to every higher player-effect tier; music, filters, global volume, and added four-SID music lanes are untouched. Leaving a crossing removes pending scenic cues and gates off an active scenic voice on the next audio tick while retaining player effects. No IRQ code calls the scenery overlay.

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

Each of the ten terrestrial visual themes uses five horizon triangles and nine cloud facets, decoded from compact signed-byte descriptors in SCENERY. The backdrop is authored in camera space; crossings interpolate the outgoing and incoming arrangements. Horizons use the neutral gameplay camera height of 40. The extended horizon bases cover the moving terrain even at camera extremes. Each theme has its own horizon and cloud arrangement and palette. Outdoor clouds and horizons occupy separate bands, so each 4-by-8 cell contains at most three colours besides the sky (the background) despite the larger palette across the complete backdrop. Crystal Cave reuses the fourteen faces for walls, roof, and hanging crystals, sharing three such colors even where they overlap. Ashen Wastes limits its scenery and rock obstacles to orange, charcoal, and black, leaving a cell slot available to an isolated red projectile.

Backdrop faces and the orbital starfield carry the `STATIC_SKY` flag, also while a crossing blends them. Since every band is repainted every frame, the flag no longer selects cached bands; GEOMETRY applies a slight sky parallax to flagged records as it replays staged scenery (`Renderer::project_scenery`): the far sky moves opposite to the ship by its full sideways offset and half its height (`Game::sky_parallax`), which perspective reduces to a few pixels at the horizon and slightly more for the nearer clouds. The host reference (`Game::render_scenery`) applies the identical shift before the sink, so staged and direct host renders stay pixel-identical. Planets, terrain and foreground geometry do not receive sideways sky parallax. Title and attract scenes instead use automatic sine-wave offsets: sky drift of ±64 world units horizontally over 10.24 seconds and ±8 vertically over 20.48 seconds, with a matching ±4-unit vertical camera float around their base heights of 0 and 40. The existing 50 Hz animation clock drives the motion independently of the ship; scene cuts reset it to neutral, but title activity does not. Text stays fixed. No sky band cache or validity mask remains.

Attract mode uses the same simulation, section cache, geometry, and raster pipeline as gameplay. Title inactivity and each section preview last 750 fixed 20 ms steps on both PAL and NTSC. A latched activity bit preserves joystick pulses between simulation steps; scene resets consume the dismissal input and clear encounters and gestures. Attract frames omit the ship and cockpit and paint all 25 bands. The small phase-independent Q8 multiply helper lives in resident code to accommodate the expanded game-state layout. Attract scenes retain the continuous digital title track across area changes.

Playing mode inserts a 400-tick scenic flight at each of seventeen boundaries. Each stage has 3,750 combat ticks: eighteen stages plus crossings total 74,300 ticks, or 24:46 before deaths and boss holds. Native progress uses a stage-local `u16 stage_ticks` plus the stage ID; aggregate HUD/report distance is derived with 32-bit arithmetic. Scenery uses local crossing coordinates so campaign length cannot overflow its old absolute 16-bit clock. Bosses enter at local tick 1,000 in stages 4, 8 and 12, and tick 2,500 in stages 17 and 18. Unfinished bosses hold progress at tick 3,750 while combat continues. The outgoing checkpoint remains active through a crossing; its successor begins only at exit.

Player controls, charge and double-tap history, player projectiles, and explosions remain live during scenic flights. Entry removes outgoing actors, pickups, and hostile shots before processing controls; enemy spawning, boss simulation, collisions, and damage are suspended. Bombs retain their inventory cost. Defeating any boss immediately restores shield 100 and bombs 3, once; its subsequent crossing preserves those resources and adds one life up to three. Other crossings preserve resources. Decorations have no actor, collision, or targeting state. The incoming checkpoint and music begin on exit, with encounter processing on the following tick. The existing two-slot cache prefetches that incoming stage during the crossing.

All bosses start with 40 HP per side unit and 120 core HP; the HUD displays the rounded-up percentage of their 200-point total. Shield drops are eligible every eighth enemy kill and restore 60 points capped at 100; bomb-carrier priority and full pickup-pool behavior are unchanged. All active pickup types share the original shield attraction range and speed. Rocks and bosses do not advance ordinary enemy-kill pickup or bomb rewards.

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
| UltiSID 1A | `$D400` | Two music + one effect voice; 0 dB | Left 1 |
| UltiSID 2A | `$D420` | Two music + one effect voice; 0 dB | Right 1 |
| UltiSID 1B | `$D440` | Three music voices; shared 0 dB gain | Left 1 |
| UltiSID 2B | `$D460` | Three music voices; shared 0 dB gain | Right 1 |

The package uses gentle stereo separation (`Left 1` / `Right 1`) for headphone listening, preserving left/right distinction while bringing the mix closer to center. Re-import the updated `havoc.cfg` to apply the mixer settings.

Each split pair shares its parent UltiSID's mixer gain and pan. Both parent gains remain at 0 dB; the former physical sockets' provisional −6 dB attenuation is dropped without changing the score or player. Addressing is described in the [Ultimate MultiSID guide](https://1541u-documentation.readthedocs.io/en/latest/config/multi_sid.html).

Both UltiSID groups use the same `8580 Lo` filter curve, `High` resonance setting, and `8580` combined-waveform behavior. Each SID instance has an independent filter owned by music; the reserved third effects voices on `$D400/$D420` bypass their filters. All three voices on each added SID instance are available to music. The engine uses oscillator waveforms, ADSR envelopes, noise, and, on the extra SIDs, ring modulation of triangle voices by their chip's noise oscillator. Per-song kernels sweep the filter cutoff, pulse widths and pitches between rows. SID volume remains constant during SID playback; digital title music uses the separate Ultimate Audio sampler.

The Ultimate Audio mapping at `$DF20–$DFFF` is enabled, and both sampler mixer channels are set to 0 dB. The player mutes and stops its sampler voices when switching to SID music. The firmware's `Digis Level=Medium` setting is retained but is not used for sampled playback. Mixer panning applies to HDMI and AV output, not the physical SID-TAP header. The built-in SID player's autoconfiguration can override mixer settings, so re-import the game configuration after using that player. See the [official mixer documentation](https://1541u-documentation.readthedocs.io/en/latest/config/audio_mixer.html).

### Updating an older package

Earlier downloads offered two-SID and four-SID variants, and the original four-SID configuration used physical sockets. Replace all three runtime files with the refreshed single package and re-import its configuration. An old two-SID REU image is incompatible with the current executable and is rejected at startup. Keep historical binaries with their original matching REU images and memory settings when reproducing archived measurements.

## Digital title, attract and victory music

The selected digital song plays continuously while the title or any attract scene is visible, starting with Hammond Rocker on reload. Scene cuts, returning to the title, and dismissing the tour preserve its position. CTRL + Commodore + DEL toggles between Hammond Rocker and Galactic Dash, restarting the chosen song while preserving the scene. A fresh fire press starts gameplay and selects the stage's SID score. The final gold-medal reveal selects Galactic Dash; a fresh fire press returns to the title while preserving the track's position. The menu selection survives subsequent runs until another toggle, campaign completion, or reload. Game-over music remains a SID cue.

The committed `assets/music/galactic-dash.dlt` and `assets/music/hammond-rocker.dlt` preserve their original left/right channels at 24 kHz, using four-bit differential codes per channel. Galactic Dash has 3,685,440 stereo frames, lasts 153.56 seconds and occupies 3,739,088 bytes; Hammond Rocker has 5,038,080 frames, lasts 209.92 seconds and occupies 5,093,048 bytes. The standalone encoder in `tools/encode-digital-music` resamples each channel with the same filter and uses one normalization gain for both channels, preserving their relative balance and musical dynamics. It targets -1 dBFS before quantization and backs off that common gain if the decoded true peak would exceed -1 dBTP. Adjacent JSON files record the gain, per-channel peaks and quality, and source/decoded hashes. Ordinary builds consume these assets directly and do not need the original MP3 files.

The original MP3s are preserved unchanged in `assets/music/source/Galactic Dash.mp3` and `assets/music/source/Hammond Rocker.mp3`. Their SHA-256 hashes match the `source.sha256` fields in the adjacent track provenance records. See the [encoder instructions](../tools/encode-digital-music/README.md) to regenerate the digital assets from these repository sources.

The prepared stereo tracks use common gains of +4.233 dB for Galactic Dash and +2.887 dB for Hammond Rocker. The decoded true-peak upper bounds are -1.295 and -1.096 dBTP respectively; measured reference-to-decoded SNR is 25.95 and 26.12 dB. The resampler retains floating-point output until normalization to avoid clipping before gain selection.

DLT3 uses a 32-byte header, 521 pairs of local codebooks and one compressed byte per stereo frame: the low nibble is left and the high nibble is right. Each segment stores sixteen three-byte signed-magnitude deltas for left followed by sixteen for right. Header byte 21 is the channel count (2); frame counts and frame rate retain the existing field positions. A cumulative CRC-8 table follows the complete stream, with one entry per 1,024-byte compressed chunk. Both predictors reset together at the exact song length. No artificial channel delay is applied.

The AUDIO overlay decodes once per stereo frame into a 128 KiB signed 16-bit interleaved REU ring. Two Ultimate Audio voices read it autonomously, starting at `$FE0000` and `$FE0002`, with four-byte strides, the same calibrated divider, unity volume and hard left/right pans. Their gate writes are adjacent but sequential. The configuration explicitly routes Sampler L to Left 3 and Sampler R to Right 3 at 0 dB. The existing 50 Hz CIA1 interrupt continues to own input timing and SID playback. The ring holds about 1.365 seconds at 24 kHz; PCM publication transfers 96,000 bytes per second. Shared staging is reused by rendering only after AUDIO returns. No decoder work is scheduled during normal SID gameplay.

SID and DAC state share the existing 400-byte resident audio allocation. A 128-byte compressed cache and paired codebooks share one transport and CRC state. Refills wait for at least a 128-frame deficit and decode 128-frame/512-byte batches. Each visit produces at most one ring of frames; if that work limit is reached with a safe lead, remaining catch-up is deferred to the next frame. Reaching the 2,048-frame guard stops and mutes both sampler voices. Every ring transfer uses a full 24-bit REU address, including the upper bank and the final frame below the exclusive `$1000000` endpoint.

After building and packing, run `python3 tools/test-digital-kernel.py` under the WSL toolchain. It executes the production ELF's AUDIO service and resident audio tick with bounded writable memory and an independent stereo DLT3 reference. Reports record ELF/REU hashes, frame counts, stack use and per-call costs. The model uses a CPU running at 64 times the PAL or NTSC bus clock and one bus cycle per REU byte. Its sampler uses the source's 6.25 MHz clock plus two synthetic divider ticks to exercise calibration correction; muted calibration polls advance directly to completion. This is a decoder and integration model, not a physical audio measurement.

The stereo playback build preserved under `target/stereo-validation/audio-validated/` passes this model on both PAL and NTSC (`target/stereo-validation/pal.json` and `ntsc.json`). The subsequent title-screen credit change passes all ten HUD tests, native layout and overlay audits, package validation, and a rendered title-preview check. It checks 297,088 and 290,560 produced stereo frames respectively, including both ring banks, seven wraps, the Hammond/Galactic selection, SID handoff and unchanged DAC state across 50 resident IRQ ticks. A 1.05-second service gap recovers with a minimum lead of 5,347 PAL / 5,345 NTSC frames; a deliberate 1.40-second gap trips the underrun guard and stops both voices. The main stack high-water mark is 593 of 1,024 bytes. AUDIO occupies 16,032 of 16,896 bytes, with a 13,413-byte runtime prefix; resident code has 166 bytes of headroom.

The 16 MiB package and sampler configuration must be kept together. Re-import `havoc.cfg` when upgrading so the expanded REU and explicit stereo mixer routing are applied. Historical mono DAC and SID-only performance reports do not validate stereo playback. Physical Ultimate listening remains separate from host and native-model checks.

## Paged melodic score

The score runs at 50 Hz from CIA1 Timer A independently of rendering, with PAL/NTSC timer periods and frequency tables. Twenty-one cues preserve the original thirteen arrangements and add six mission themes, an ascent cue and the Gate Guardian encounter. Every build includes the four core parts and six additional orchestration parts. The consolidation preserves the complete former four-SID score byte for byte.

Each score page contains one bar of sixteen sixteenth-note rows. Cues carry their own checked row duration. The score allocation is `$0C0000-$0FFFFF` (256 KiB), with 32-bit bank-relative offsets; package `assets.txt` records the exact used byte count. The resident engine occupies 400 bytes within its 432-byte allocation; the 224-byte next page occupies `$0720-$07FF`, with compiler globals kept below `$0720`. Track names, arrangements and row timing are authored in [src/soundtrack.rs](../src/soundtrack.rs).

The resident engine holds the active page, SID register shadows, two effects states, and a small event queue. The foreground AUDIO overlay fills a separate next-page buffer and publishes it while interrupts are masked. The buffer is not modified until the player consumes it. The IRQ copies a ready page at the bar boundary and requests its successor; it performs no REU access. If the next page is late, playback waits before the boundary's release tick, preserving held music and continuing effects. `MUSIC_UNDERRUNS` counts each incident. Cue changes request page zero and insert a complete gate-low tick before the new cue starts.

Startup also caches the twenty-one validated `CueInfo` descriptors in 168 bytes of resident low-RAM `.bss`. This array lies outside the compiler scratch region saved by the IRQ, and only foreground AUDIO code reads it. Its entries become available after the complete bank scan succeeds and remain unchanged during play. Page requests use these cached offsets, lengths and tempos, avoiding repeated REU directory transfers. The IRQ engine and its active/next page capacities remain unchanged.

Historical OST-build MOS layouts used 177 bytes for the two-SID engine plus a 96-byte next page, or 355 bytes for the four-SID engine plus a 224-byte next page: **273 and 579 bytes total**. Seven further bytes hold the loader checksum, validation flag, and exported music diagnostics. The two-SID build places both audio objects inside the existing 432-byte `$CE00–$CFAF` allocation. The four-SID engine uses that allocation and places its next page at `$0720–$07FF`; compiler scratch ends at `$05CD`. The foreground loader reuses the retired band workspace for its directory. That OST revision had 710 bytes of resident headroom in the standard build and 696 in the enhanced build, with one spare byte in GEOMETRY. These are historical figures, not the six-overlay scenic build limits. The memory maps in each package record the actual target sizes; host ABI sizes can differ.

The historical OST ELF symbols account for **2,699 / 2,707 bytes of resident player code** in the two/four-SID builds: music transport, effects, register writes, pitch conversion, page requests, and the audio-tick wrapper. This excludes shared arithmetic helpers, constants, and the common IRQ wrapper. The foreground AUDIO overlay is 2,859 / 2,881 bytes and reuses the existing overlay window; it adds no separate resident code allocation. Its loader and validators are unreachable from the IRQ. Four-SID routing tables avoid a per-note divide, keeping the extra six voices inexpensive in code size.

The bank format is `SMU3`. Its 256-byte header records SID/voice profile, total length, page size, and twenty-one eight-byte cue entries; the directory can represent thirty cues without another resize. Entries contain bank-relative offset, page count, row duration and cue checksum. The common asset bank stores a matching header copy. `SPG2` pages keep their layout: a 12-byte header, five instrument bytes per voice, and sixteen rows of ten events, totaling 224 bytes. Historical two-SID pages used four events per row and 96 bytes; the current player rejects that profile. Header byte 9 is the bar's filter cutoff target (SID register 22); zero holds the filter, so the historical pages remain byte-identical. Each instrument's control byte carries one waveform plus optional sync (`$02`) and ring-modulation (`$04`) bits; ring modulation is only accepted on triangle voices, and the player writes the byte unchanged with the gate bit, so the colours cost no resident code. Unused bytes are zero.

Events are `0` for rest, `1` for hold, MIDI notes `24..95` for an articulated attack, or a note with bit `$80` set for legato. Holds preserve the gate and pitch; legato changes pitch without releasing the gate. A new attack or rest releases the preceding note one 20 ms tick before its boundary. Page publication rejects a hold or legato opening when switching cues or changing that voice's instrument. For *Lost Wing*, an attack plus hold at nine ticks per row reproduces each old 18-tick note, followed by two resting rows; its full loop remains 2,304 ticks, or 46.08 seconds.

The produced score keeps the four core parts and all six orchestration parts. The electronic and orbital cues add Jeroen Tel-style production entirely in the page data: lead and bass pulse widths step across every eight-bar phrase; voice six echoes the lead a dotted eighth (or an eighth) later from the opposite UltiSID in the four-SID ensemble; the orbital drum kit uses real noise decays, slurred falling snares, a tuned sub-octave kick body, phrase-end rolls and section crashes; basses walk chromatically into each chord change; strings swell through the second and final eights and drop out for a two-bar breakdown before each reprise; *Slingshot Through the Belt*, *Rivets in Orbit* and *Carrier Overdrive* lift their final sections a whole tone with newly written melodies; and *Silver Fractures*, *Beyond the Blue Threshold*, *Ashes and Steel* and *Rivets in Orbit* ring-modulate bells and anvils on the extra SIDs. *Lost Wing* is untouched. `tests/soundtrack.rs` pins every cue's published four-SID checksum.

Header and page checksums treat their own two-byte fields as zero. Whole-cue checksums cover the stored pages including their checksums. Before the first cue plays, the foreground validator checks the header signature, all directory bounds, contiguous cue spans, profile fields, instrument/note ranges, tempos, page indices, next-page links, padding, and checksums. It also verifies the final bank extent before publishing the cached descriptors. Every subsequently fetched page is validated again before publication, including its checksum, cue/page identity, tempo and next-page link. Runtime uses the validated cached descriptors as its directory authority; it does not reread the REU header. The first AUDIO load includes the validator and parser, while later loads stop at `__phase_audio_runtime_size`. The `OVL7` code directory still checks the complete AUDIO image alongside the other executable images and matching PRG, constants, startup code, and SID profile.

## Per-song music kernels

Each cue except *Lost Wing* ships its own modulation kernel: a small piece of 6502 code, written in Rust in [src/audio/kernels.rs](../src/audio/kernels.rs), that the IRQ calls once per tick after the transport has written the row. Kernels are composed from shared building blocks — filter glide towards the bar's authored target, the bass-triggered "wah" envelope, triangle pulse-width shimmer, delayed vibrato, portamento into slurred notes, a chord-safe double-time arpeggio that flutters between consecutive authored pitches, a tick-rate drum program that turns kicks into sliding pulse bodies and lets snares fall, and a modulator sweep for the ring-modulated bells — with the constants baked per song. They touch SID registers only through the engine's deduplicating writes, read the row the transport already wrote, keep all state in a sixteen-byte `scratch` that clears at every loop start, and never touch the reserved effects voices, routing or volume. The single package applies them to the complete ten-part arrangement.

On the target every kernel is linked at the same resident slot (`$7200–$74FF`, 768 bytes) and stored as a separate REU image behind an `SMK1` directory at `$0B9000`: a 16-byte header with its own checksum, one `size, checksum` entry per cue, and a whole-directory checksum. The host `pack-overlays.py` writes the images from the linked ELF together with the `OVL7` code images, so the existing PRG/REU pairing check covers them. Startup verifies the complete directory and every image before `MUSIC_VALIDATED`; at each cue change the foreground AUDIO overlay marks the slot invalid under an IRQ mask, fetches the header and the cue's entry into the slot, copies and re-checksums the image (every transfer stays far below the 1 KiB DMA bound), and only then re-enables the slot for that cue. The IRQ calls the slot only while the loaded kernel belongs to the playing cue, so a changed cue's final bar is never coloured by its successor's kernel. Historically, the two/four-SID images measured 275–748 bytes; the costliest kernel measures about 3,400 cycles per tick on the 6502 model, under 55 µs at 64 MHz. The slot reuses half of the former 1 KiB IRQ software stack, whose high-water mark measured three bytes across five native cue loops; `tools/check-layout.py` and `tools/check-overlays.py` verify the slot placement, every image size, and that kernels reference only resident code and data. The `music-kernels` Cargo feature (on by default in `target-c64`) can be disabled to ship the plain transport.

`tools/test-music-kernel.py` replays the linked kernel images on a Python 6502 model against fixtures from `cargo run --release --features music-kernels -- kernel-fixtures`: the four-SID fixture set covers 1,181 engine states, each requiring identical engine bytes and SID writes to the host run, no stores outside the engine, SID, zero-page, stack and static-frame regions, and a balanced hardware stack. `tools/test-corrupt-package.py` includes a corrupted kernel directory, entry and image, which boot rejects with `INVALID MUSIC KERNEL`.

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

The complete build tests the host with and without music kernels, bakes assets, links one C64 program, audits memory and overlays, and produces matching `havoc.prg`, `havoc.reu`, and `havoc.cfg` files in `dist/`. It also creates `havoc-v0.1.2.zip` with the three runtime files, launch instructions, and license. The native build uses `target-c64/target`. Generated packages are ignored by Git; cloning the repository does not supply `dist/`.

After changing assets, bake with `cargo +stable run --release -- bake dist`, then run `bash scripts/build-target.sh`. The bake step alone does not produce executable overlays. Use the full build script to run checks and produce the release ZIP. On Windows, `scripts/build-windows.ps1` runs the same build through WSL; its optional output directory receives the single package and ZIP. SID-profile arguments and the `four-sid` Cargo feature have been removed.

Host inspection commands:

```sh
cargo test --all-targets
cargo test --all-targets --features music-kernels
cargo run --release -- sizes
cargo run --release -- snapshot title 0 target/previews/title
cargo run --release -- snapshot 2 5000 target/previews/canyon
cargo run --release -- snapshot attract 35000 target/previews/attract-crystal
cargo run --release -- snapshot transition 0 4000 target/previews/lowlands-crystal
python3 tools/previews.py
```

The host binary is an asset and inspection tool, not a desktop game. Snapshots produce an aspect-correct `.ppm` with the floating sprite HUD, a bitmap-only `.bitmap.ppm`, and a `.sprites.json` sidecar containing VIC registers, pointers, and all eight 64-byte images. The `.bin` stays bitmap-only: 10,000 bitmap/screen/color bytes followed by the VIC background byte. The preview converter needs only Python's standard library.

Native smoke captures retain their bitmap-only `-native.bin` (10,000 bytes), `-native.png`, and `-native.ppm` files for renderer comparisons. The separate `-sprites.json` records actual VIC state; `-composited.png` uses the 320×200 hardware-pixel grid, and `-preview.png`/`-preview.ppm` scale that composite to 640×480. These composites apply bitmap fine scroll and actual sprite coordinates, preserving half-multicolour-pixel shake offsets. Native benchmarks compare bitmap and visible-sprite hashes separately. `--hud-checks` verifies persistent image RAM and lazy corner updates; `--weapon-checks` also verifies sprite/bitmap shake alignment. Showcase recordings retain their neutral-scroll 160×200 frame contract with the HUD composited and a sprite sidecar for each frame.

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
cargo +stable run --release --features music-kernels -- kernel-fixtures
python3 tools/test-music-kernel.py --elf dist/havoc.elf
python3 tools/test-keyboard-kernel.py --elf dist/havoc.elf
python3 tools/test-digital-kernel.py --standard both
```

The keyboard check executes the linked PIXELS scanner against modeled CIA keyboard/joystick inputs. The digital check executes the linked AUDIO overlay against modeled REU and stereo sampler hardware on PAL/NTSC, including both directions of the menu song toggle and saved-calibration reuse. These checks do not require full VICE startup or physical hardware; the current calibrated VICE companion lacks Ultimate Audio sampler support.

`tools/branch-audit.py <source.S>` runs from `target-c64/build.rs` on every assembled source: it re-assembles the file with line information, reads the label each conditional branch names and compares it with the encoded target, because the assembler silently wraps a branch that cannot reach its label in the same section (a wrapped target before the section start is printed with a sign, which the audit also recognizes). Two more assembler habits to know: `<` and `>` applied to a negative constant yield the bytes of its magnitude, so negative immediates are written as explicit bytes; `.L` labels survive only with `-Wa,-L`.

Emulator checks use the calibrated sibling `doomc64u/target/audit-fast/x64sc-64x` companion and its ROMs; override these with `--vice` and `--roms`. An unmodified companion can map the turbo setting to 48x, invalidating a 64x timing comparison.

The emulator tools default to `--audio-backend sid` because this companion does not implement Ultimate Audio. For digital packages they set a monitor-only RAM flag after C runtime initialization, before game startup, selecting the SID title, attract and victory cues in place of the digital menu tracks. Gameplay keeps its normal SID score. Reports record this choice under `emulator_audio`; these runs validate gameplay, rendering and SID playback, not DAC decoding, ring refills, calibration or digital music timing. Use `tools/test-digital-kernel.py` for the modeled DAC checks described above.

Pass `--audio-backend native` with a sampler-capable emulator to retain the production audio path. The option is shared by `tools/vice-smoke.py`, `tools/capture-showcase.py`, `tools/music-previews.py` and `tools/bench-native.py`. For SID mode, digital packages built before the monitor hook must be rebuilt with their matching PRG, REU and ELF; older SID-only packages need no hook. The flag is set separately for each emulator launch and never changes the files on disk. Ordinary hardware startup leaves it clear and requires the configured Ultimate Audio sampler; it does not silently substitute SID menu music.

Selecting `native` changes startup routing; it does not turn SID recordings or SID activity assertions into DAC validation. Capture still checks active SID gates, and music previews still record the SID score. Digital title captures therefore require sampler-aware capture checks in addition to a sampler-capable emulator. Digital-package benchmarks require SID mode because their fixtures restore SID engine snapshots, not DAC state. This mode warms one title frame before saving audio state, while retaining the game and runtime snapshots from before the first simulation step; each fixture reloads its opening score page and modulation kernel.

```sh
python3 tools/test-emulator-audio.py
python3 tools/test-campaign-tools.py
cargo +stable run --release -- snapshot title 0 target/previews/title
python3 tools/vice-smoke.py --video pal --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video ntsc --title-reference target/previews/title.bin
python3 tools/vice-smoke.py --video pal --control-checks --weapon-checks --streaming-checks
python3 tools/vice-smoke.py --video pal --attract-checks
python3 tools/vice-smoke.py --video pal --music-checks
python3 tools/vice-smoke.py --video ntsc --music-checks
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

`target-c64/build.rs` assembles the native hardware and rendering helpers, disables additional LTO zero-page register allocation, selects the custom linker script, and emits a memory map. Six primary phase sections share an execution address; seven encounter tails and the terminal tail share the SIM prefix's end address, while combat and medal drawing share the GEOMETRY prefix's end address. Explicit linker assertions and the project's post-link checks replace the general linker overlap check. ELF symbols and relocations are preserved for emulator debugging, package extraction, and `tools/check-overlays.py`. That checker rejects cross-phase references, permits only named phase entry calls and reviewed opaque dispatches into the enemy, terminal, and rendering tails, validates ownership of compiler switch tables and the lifetime of startup checks, and checks that the IRQ's relocation graph stays resident. Runtime-computed pointers and timing still require execution tests.

The current `tools/test-overlay-audit.py` suite rejects 76 damaged relocation and ownership fixtures, including terrestrial references to unloaded space helpers, AUDIO references to unloaded startup code, and invalid sprite or mission-buffer reservations. `tools/test-layout-audit.py` rejects 142 mutations covering transfer lengths, REU addresses, music-directory ownership, the relocated shared workspace, sprite storage, and resident code growing into the first mission buffer. Both use private copies and verify that their input packages remain unchanged. Native scenery passes the concrete `SceneryStage` sink through its opaque decoration entry, so no resident vtable exception is needed for a callback into an overlay.

For repeatable size profiling, run `python3 tools/profile-size.py --baseline target/campaign-baseline/dist/havoc.elf --current dist/havoc.elf --out-dir target/campaign-size`. The read-only comparison writes `README.txt` and `profile.json` with function sizes and deltas, per-section totals, and common-prefix-plus-tail headroom. Either input may instead be a linker `.map`, allowing an overflowing build to be inspected before it produces an ELF. Repeated generic names with ambiguous matches are marked for review rather than assigned a guessed baseline.

Compiler flags must be checked at both optimization stages: Rust emits bitcode for this target, and the LLVM-MOS SDK performs a final LTO link. The [isolated compiler-option comparison](benchmarks.md#campaign-compiler-options-2026-10-18) distinguishes those stages, records size and failure results, and shows package-level overrides. Cargo has no per-source-file optimization profile; linker sections select memory placement, not a different optimization level.

`python3 tools/test-game-math-kernel.py` builds the production swept-depth assembly and a frozen Rust reference under the configured Linux/WSL toolchain. It checks 6,658 native simulator cases and records sizes and an identical 2,048-call timing harness in `target/game-math-test/report.json`. That harness measured 825,860 reference cycles and 758,449 production cycles; these include harness overhead and do not measure complete game frames.

The native `memcpy.S` replaces the Rust compiler's larger memory-copy routine and its static scratch frame. It follows the LLVM-MOS C ABI and uses only caller-clobbered registers. `python3 tools/test-memcpy-kernel.py` runs 1,297 native 6502 simulator cases against an independent volatile byte-copy loop, covering empty copies, page boundaries, unaligned addresses, returned pointers, and source/destination guards. Its generated report lives under `target/memcpy-test`.

Unscaled 3-by-5 text that fits completely within one eight-row band uses a bounded path: Rust checks the whole label once, then calls `fast_cell_glyph` with each glyph's five font rows, which paints at most two cell masks per row. Clipped or split-band text retains the general renderer, which paints each lit glyph pixel as a `scale`-wide span. Host tests compare this path with `draw_text_scaled` for all 256 character bytes and clipping boundaries; `python3 tools/test-cells-kernel.py` checks the glyph, span and plot kernels against the C transcription of the reference.

The other native kernels preserve host reference behavior: `sort.S` implements stable far-to-near radix ordering (writing the order table rather than moving commands), `raster.S` advances prepared edges and hands spans to the cell writer, and `pack.S` quantizes the overflowed cells. The compact sort kernel uses the resident table region; the raster walker uses PIXELS and the preparation kernel uses SHADOW finalization, leaving the fixed overlay boundaries intact. Their simulator checks compare bytes against independent references and verify memory guards (`tools/test-sort-kernel.py` runs the production Rust entry through 416 sorts, with and without scenery segments; `tools/test-clip-kernel.py` 1,584 triangles through the clipping kernel). Host geometry tests compare cached transforms with the original per-vertex arithmetic. These correctness checks do not substitute for target timing measurements.

`missile.S` (`fast_steer_missile`, in every SIM image) is the native body of `Game::steer_missile`: it keeps a valid target lock or randomly acquires an eligible actor, live boss turret, or exposed core ahead, then applies the per-axis guidance (`missile_velocity`) with the release build's wrapping 16-bit product, truncating division and ±12 clamp. Its 21-byte argument record contains actor and descriptor addresses, shot position, boss state, dx/dy, target ID and random state. The 1,002-byte kernel uses `$9140–$917D`, GEOMETRY scratch that is idle during SIM. Game stores 24 four-bit target locks in twelve bytes plus one random-state byte: native state is 631 bytes (including the ten-byte flight record), within the 640-byte limit. Actor-slot reuse invalidates locks before replacing the actor. The right-shift `$B8` LFSR starts at `$A5`; one-based high-nibble rejection gives equal candidate ranks over a complete accepted cycle. Retained locks and zero/one-candidate searches consume no random draws. Full scene resets restore the seed; checkpoint respawns clear locks and preserve random state. `python3 tools/test-missile-kernel.py` compares the kernel with a C transcription of the host over 12,852 cases, checking target ID, RNG state, aim point, closing speed and dx/dy, including all byte seeds, candidate counts, retained locks, boss states and wrapping-coordinate extremes. The composed SIM images retain 40 bytes of room after the lock-management changes.

`shots.S` (`fast_shot_scan`, in every SIM image) is the native body of `Game::shot_target`, the scan `Game::update_shots` runs for every moved player shot and missile: in slot order it selects each live actor's radius (18 for small debris, 35 for rocks and buildings, else the descriptor's `shot_radius`) and centre offset, then applies `swept_z` over the move and `close_xy` with the release build's wrapping differences, and returns the first hit slot or `$FF`. Its arguments arrive in registers (old depth in A/X, then the position, actor and descriptor addresses and the descriptor count in `rc2–rc8`), it keeps its state in caller-saved `rc2–rc19` and tests the lateral distance first, so a slot more than 255 units to the side costs about 70 cycles and no radius lookup. The compiled scan was about 400 cycles per live slot per shot; the kernel is 441 bytes and 0.7–2.6 k cycles per shot for twelve empty to twelve nearby slots (the missiles fixture's SIM slice fell 21.7 → 16.8 ms at ten ticks; the largest SIM profile keeps 528 bytes of room). `python3 tools/test-shots-kernel.py` compares it with a C transcription of the host loop over 56,564 scenes: every edge of every kind class's hit volume on both sides in both sweep directions, slot-order ties, the wrapping corners and random scenes with random descriptors.

`flight.S` (`fast_flight_step`, resident) is the native body of `Game::move_player`, the player's assisted flight model (`src/game/flight.rs`, a Q8 port of havoc2's aircraft behaviour at the 50 Hz tick). `Game` keeps the integer position in `player` and a ten-byte `Flight` record directly after it: the two position fractions, the Q8 velocities, the bank and the pitch (Q8 of 256ths of a turn; their high bytes are the mesh angles). Each tick the stick sets a target velocity (±3.0 units per tick sideways, ±2.75 vertically, plus a 2.5 push in the roll's direction while the hull spins); the velocity moves 3/32 of the way to it (a 213 ms time constant) by the shifted magnitude of the error and at least one unit while any error remains, so every state settles exactly; a soft wall then bounds it by `(edge + 7 − q) >> 3` on the near side of the ±100 × ±65 lane, so the ship decelerates exponentially into the former hard limits, reaches them exactly and never crosses them; the bank target is −16 × the lateral acceleration (none for a single settling unit) clamped to ±20/256 of a turn, the pitch target −4 × the vertical target, and both angles follow with an eighth of their error per tick. Negative mesh angles lower the right wing, so a right turn banks negative and the 25-tick barrel roll's linear spin runs negative for a right roll too (`Game::player_appearance` adds the spin to the bank, so the end of the full turn is continuous with the banked hull). The arguments arrive in registers (stick bits in A, push in X, the player and flight addresses in `rc2–rc5`), the state lives in caller-saved `rc6–rc18`, and the kernel is 638 bytes and about 1.9 k cycles per tick (0.03 ms); it replaced a 493-byte compiled routine, and the resident Q8 multiply and bounded division kernels moved to the `$9900` table page to make room. `python3 tools/test-flight-kernel.py` compares it byte for byte with a C transcription of the host reference over 147,850 steps: every stick pattern with every push held and released from rest and the lane's corners, and random states (stale fractions, positions outside the lane, extreme velocities and angles) as single steps and continuing flights.

`preparation.S` implements exact native Q16 triangle setup, including sorted-edge orientation parity, signed rounding and near-screen clipping; the positions it stores carry the walker's +32767 bias. Its specialized signed slope helper shares the SHADOW finalization prefix. Host builds retain the Rust reference. Preparation temporarily uses 31 bytes at `$77A0–$77BE`, before row walking or palette packing borrows that same scratch; model and mission buffers remain intact. The linker and memory audit check this ownership.

`python3 tools/test-raster-prep.py` checks both the production Rust entry point and the direct assembly kernel against an independent integer reference. It compares all 28 prepared head/tail bytes for 852 triangles at four alignments, with separate storage and in-place input: 13,632 preparations with buffer and scratch guards. This closes the gap left by the row-walker test, whose inputs are already prepared on the host. Its report lives under `target/raster-prep-test`. `python3 tools/test-preparation-abi.py` separately checks 1,024 calls with changing callee-saved register canaries, stack and I/D flags, immutable input, and output guards; its report lives under `target/preparation-abi`.

`python3 tools/test-corrupt-package.py` exercises package rejection in the calibrated VICE companion using damaged code/model directories, executable images, common and level assets, PRG/REU pairing, SID profile, and score pages. It uses private copies under `target/corrupt-package-test`, verifies the error-screen text bytes, saves screenshots and logs, and confirms the input package hashes remain unchanged. It reads failure addresses from the matching ELF and chooses a free monitor port. The default package directory is `dist`. These checks are emulator regressions, not physical C64 Ultimate acceptance.

The final optimized release passed 125 corruption cases per sound profile, including every executable image and all twenty-one music cues. `target/offplanet-validation/release-corrupt-2sid/summary.json` and `target/offplanet-validation/release-corrupt-4sid/summary.json` record all 250 successful rejections and unchanged input hashes.

`tools/vice-smoke.py --music-checks` derives the SID profile and cue count from the package, then verifies every complete cue loop plus an extra page while frames continue. It observes page order, clocks, underruns, authored pitches and lane activity; a held-fire Lowland fixture exercises both effects voices. The current package exercises all four SID banks and ten music lanes; the historical two-SID artifact checks still reject writes to `$D440/$D460`. Add `--music-cues 13 14 15 16 17 18 19 20` to repeat just the eight new full loops, as in the archived expansion-release suites.

All twenty-one cues previously passed complete native loops in both sound profiles and both video standards on the frozen `target/offplanet-final/` packages. Those reports are `target/offplanet-validation/final-smoke-2sid-pal/report.json`, `target/offplanet-validation/final-smoke-2sid-ntsc/report.json`, `target/offplanet-validation/final-music-4sid-pal/report.json`, and `target/offplanet-validation/final-music-4sid-ntsc/report.json`. They precede the optimized directory cache and AUDIO transfers; the original expansion release repeated the eight new loops and functional checks on its exact release hashes. The older thirteen-cue suite under `target/music-validation/native-{2sid,4sid}-{pal,ntsc}/` is historical evidence. These monitor-assisted scenes are separate from ordinary-play timing and a complete campaign.

`tools/music-previews.py` records the native player through VICE's SID audio output. Historical recordings of *Crystal Current*, *Ashes and Steel*, *Tempest Drive*, *Gorge Warden*, *Citadel Heart*, and *Carrier Overdrive* remain under `target/music-validation/previews-{2sid,4sid}/`; those manifests report zero clipped samples. The orbital score adds *Slingshot Through the Belt*, *Silver Fractures*, *Echoes of Lost Fleets*, *Rivets in Orbit*, *Shadow of the Dreadnought*, *Beyond the Blue Threshold*, *Atmosphere to Starlight*, and *The Last Sentinel*. The six mission cues and final boss cue each contain four distinct eight-bar sections; the ascent lasts eight seconds.

`target/offplanet-validation/music-2sid/recordings.json` and `target/offplanet-validation/music-4sid/recordings.json` pass all eight new recordings: 407.36 seconds per profile of 44.1 kHz stereo WAV audio, with both channels active and zero clipped samples. The WAVs and manifests are bundled in `target/offplanet-validation/music-2sid.zip` and `target/offplanet-validation/music-4sid.zip`. They retain the earlier frozen packages as their recording provenance. `target/offplanet-validation/release-music-equivalence.json` verifies that their complete encoded score banks are byte-identical to that historical release: 62,944 bytes for two SIDs and 146,528 for four SIDs. Those emulator recordings did not validate the former physical-SID mixer balance. Four-UltiSID playback and mixer balance on the Ultimate remain unverified.

The soundtrack tests check cue lengths, distinct melodic sections, loop/page articulation, four-SID serialization, rejection of legacy two-SID data, and the exact *Lost Wing* transcription. A passing host test or valid linker map does not establish a frame-rate result on the machine. Hardware acceptance must check PAL and NTSC presentation timing, all twenty-one cues and page boundaries, simultaneous music and effects, REU transfers during play, stack guards, and full-mission restart/checkpoint behavior. Four-SID acceptance must also confirm that importing the configuration exposes all four built-in SID instances, the physical sockets remain unmapped and muted, and the enhanced arrangement and effects sound balanced at the shared 0 dB gains. Physical chips are optional and unused.

The bootstrap structure was informed by the neighboring `doomc64u` repository. `secondreality/src/output/cfg.rs` provided a local example of the Ultimate mixer and SID-address configuration keys. Digital conversion and the Ultimate Audio backend are adapted from `secondreality-opt`; SID gameplay retains H.A.V.O.C.'s synthesis player.


## Reproduce the README showcase

Build the four-SID package first, then run the capture and encoding steps from Linux/WSL:

```sh
python3 tools/capture-showcase.py \
  --package-dir dist --video pal --out-dir target/readme-showcase \
  --scene title --scene lowlands --scene crystal-cave \
  --scene foundry-belt --scene storm-front --scene orbital-ascent \
  --scene asteroid-belt --scene title-outro
python3 tools/encode-showcase.py target/readme-showcase/manifest.json
```

Capture uses the compiled C64 program in the calibrated emulator and records its SID music and gameplay effects. Its default `--audio-backend sid` substitutes SID menu cues for the digital tracks, and both the top-level and per-scene manifests record that choice. The showcase presents selected scenes rather than a continuous campaign or a physical-machine recording. Keep the footage at its recorded game speed: duplicated video frames must not be presented as a higher game rendering rate. The four-SID recording represents VICE SID synthesis; it does not establish the Ultimate's UltiSID sound, DAC playback or final hardware mixer balance.

The README trailer is 72 seconds: title (4), Lowland Approach (12), Crystal Cave (10), Foundry Belt (10), Storm Front (10), orbital ascent (8), Asteroid Belt (10), and title outro (8). It previews the first orbital mission without revealing bosses or the ending. Omitting `--scene` records the full storyboard, including all six orbital missions. Repeat `--scene <name>` to select scenes, or use `--duration 1` for short capture probes. Artifact readers can still inspect historical two-SID packages when an explicit package path is supplied; current builds always contain four SIDs.

The encoder writes `target/readme-showcase/havoc-showcase.mp4` by default. It requires FFmpeg and FFprobe; use `--ffmpeg <path>` and `--ffprobe <path>` to select their executables. Native Linux and Windows executables are supported, with WSL path conversion for Windows tools. Preserve the capture manifest and source recordings with the generated video so its package, scene selection, timing, and audio can be checked before publishing. Generated capture artifacts stay under `target/`; only the selected README images belong in `docs/media/`.

The 2026-10-21 README refresh is preserved under `target/readme-refresh-20261021/`. Its frozen `package/` and `build-provenance.json` identify the four-SID build with music kernels enabled; `final/manifest.json` selects the eight verified takes and `final/encoded/report.json` verifies the 72-second MP4. The capture pilot uses physical joystick input with inertia-aware avoidance, and the Asteroid Belt preview starts at the beginning of the stage on a low scenic flight path. All retained takes preserve full shields and pass the existing raster-effect, geometry, input, and music checks.

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
