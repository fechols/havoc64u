# H.A.V.O.C.

**Heavy Assault Vessel: Orbital Combat**

Your ship is an unreasonable amount of firepower with a cockpit attached!

Fly an original 3D rail shooter for the **64 MHz C64 Ultimate**. Skim green hills, squeeze through crystal caves, and blast into orbit to take on enemy fleets.

## Features

- **3D worlds** — Dive through colorful landscapes and orbital scenery.
- **Quad-SID effects** — Deep energy-cannon fire, heavy hull impacts, and resonant filter sweeps. Each effect uses one SID or a stereo pair; four chips let sounds overlap. Music plays entirely through the DACs.
- **Rock soundtrack** — Rock out to Hammond Rocker through the title and attract tour, hear a different song in every stage, and earn the final gold medal for Galactic Dash. Mission Failed closes an unsuccessful run.
- **18 varied stages** — Journey from green hills to orbital battlefields.
- **Five bosses** — Break their defenses and strike their exposed cores.
- **Aircraft-like handling** — Bank, climb, and dive with weight and momentum.
- **Versatile weapons** — Unleash autofire, charged shots, homing missiles, and bombs.
- **Defensive barrel rolls** — Deflect enemy shots while keeping your guns firing.
- **Magnetic pickups** — Pull in nearby supplies to keep fighting.
- **Checkpoints and pause** — Retain your score after losing a life; take breaks.
- **Punchy impacts** — Feel hits and bombs through screen shake and flashes.

The development build includes twenty-one original digital recordings composed for H.A.V.O.C. by William Echols. All eighteen stages have their own songs; Shattered Moon plays Lunar Awe. Mission Failed plays after losing all lives, and Galactic Dash celebrates victory. Stage music, Mission Failed, and Galactic Dash use 16 kHz, 3-bit mono compression with a 10 ms delay between outputs. Hammond Rocker retains true stereo, and SID combat effects remain active. Every song keeps its full length, with one encoding per recording. See the [current implementation status](docs/digital-music-handoff.md#current-development-status-dlt4). The v0.1.2 download below retains its released soundtrack and playback behavior.

The development revision runs the four-SID effects engine in resident assembly at 50 Hz. Each chip acts as one instrument with three hardware voices shaping its attack, body, and tail. Each effect uses at most one SID per channel; a stereo bomb leaves one left and one right chip available for ship explosions, and repeating it restarts its existing pair. Shots and impacts follow their screen position with a narrow center band and gentle headphone separation. Pitch and pulse-width movement, staged waveforms, sync, ring modulation and filter sweeps remain. Track changes fade the outgoing digital music for about one second before the next song starts, without a crossfade. Current validation and the sustained-combat CPU measurements are tracked in the [audio implementation notes](docs/hardware.md#quad-sid-effects).

## See it in action

**Sound on!** This earlier trailer features the original four-SID score and combat effects as the action climbs into orbit. Recorded in a PAL emulator at the game's original speed (72 seconds).

https://github.com/user-attachments/assets/f2c0d2a9-250d-456e-89e2-7e5f1bc5605a

Screenshots: **original v0.1.0 four-SID build**, captured in a PAL emulator.

| Lowland Approach — barrel roll | Crystal Cave — diving |
| --- | --- |
| ![Ship mid-barrel roll over the green hills of Lowland Approach](docs/media/lowlands.png) | ![Ship pitching nose-down while descending through Crystal Cave](docs/media/crystal-cave.png) |
| **Iron Canyon — banking left** | **Foundry Belt — banking right** |
| ![Ship banking left between the ridges of Iron Canyon](docs/media/iron-canyon.png) | ![Ship banking right over the industrial scenery of Foundry Belt](docs/media/foundry.png) |
| **Storm Front — climbing** | **Carrier Assault — banking right** |
| ![Ship pitching nose-up while climbing into Storm Front](docs/media/storm-front.png) | ![Ship banking right beside the enemy carrier during Carrier Assault](docs/media/carrier.png) |
| **Asteroid Belt — banking right** | **Shattered Moon — banking left** |
| ![Ship banking right among rocks in Asteroid Belt](docs/media/asteroid-belt.png) | ![Ship banking left and climbing beneath the Shattered Moon](docs/media/shattered-moon.png) |
| **Ship Graveyard — banking left** | **Orbital Shipyard — banking right** |
| ![Ship banking left and descending among abandoned hulls in Ship Graveyard](docs/media/ship-graveyard.png) | ![Ship banking right and descending in front of Orbital Shipyard's gantries](docs/media/orbital-shipyard.png) |
| **Space Carrier — climbing turn** | **Jump Gate — banking left** |
| ![Ship banking right and climbing above the Dreadnought's broad deck](docs/media/space-carrier.png) | ![Ship banking left beside the Gate Guardian's open aperture](docs/media/jump-gate.png) |

## Download and launch

**[Download H.A.V.O.C. v0.1.2](https://github.com/fechols/havoc64u/releases/download/v0.1.2/havoc-v0.1.2.zip)** — the complete eighteen-stage campaign, full four-SID score, and new stereo rock music for the title and victory screens. It uses the C64 Ultimate's built-in audio; no extra chips or hardware installation needed.

You need a **C64 Ultimate running at 64 MHz**, **16 MiB of REU memory**, and a joystick connected to **port 2**. The supplied configuration enables Ultimate Audio and sets the speed, memory, and SID routing. A stock C64 cannot run the game.

1. Unzip the package and copy its three runtime files — `havoc.cfg`, `havoc.reu`, and `havoc.prg` — to the Ultimate's storage.
2. Import `havoc.cfg`.
3. Load `havoc.reu` into the REU at **address zero**.
4. Run `havoc.prg`, then press fire to launch.

Keep all three files from the **same package and version** together. If startup reports a mismatch or invalid REU data, reload the matching REU at zero before running the PRG again.

Upgrading from v0.1.1 or an earlier download? Replace all three runtime files and re-import `havoc.cfg` to enable the 16 MiB REU and stereo music. Earlier configurations do not include the new audio setup.

**Turn your joystick's hardware autofire off.** The game supplies its own autofire; controller-generated fire pulses can accidentally spend your bombs.

## Fly, fire, survive

Your ship flies forward automatically. You steer around enemies, incoming shots, and scenery.

| Input | Action |
| --- | --- |
| Left / right | Move sideways |
| Up / down | Climb / dive |
| Hold fire | Fire continuously |
| Release fire, then press again | Fire a charged shot; charging starts after one second |
| Release fire for two seconds, then press | Launch a homing missile |
| Double-tap fire within half a second | Spend one bomb, as well as firing normally |
| Double-tap left or right within half a second | Barrel roll and dodge in that direction |
| Fire at the title or after a run | Start a new game |
| S at the title (development build) | Toggle Normal / Story mode, then press fire to launch |
| Space bar | Pause and resume the flight |
| F | Toggle the frame-rate readout in the top-left HUD corner |

Watch the weapon meter: **HOMING MISSILE** means your next shot tracks a random target ahead, switching targets when needed. Charge longer for stronger straight shots.

- **Roll through shots, dodge scenery.** Keep steering and firing during a roll, but avoid solid obstacles.
- **Clear a path.** Shoot small orbital fragments; bomb large rocks and wreckage. Every 15 enemy kills restore one bomb, up to three.
- **Collect supplies.** Green shields restore 60 points, up to 100; purple rectangles add one bomb, up to three.
- **Break the boss's defenses.** Destroy both side units, then the core, to advance. Each boss defeat restores full shields and three bombs.
- **Make each life count.** Normal mode starts with three lives. The development build also offers Story mode with 99 lives: press **S** at the title to select it, then press fire. The choice remains selected until you toggle it again or restart the program. Losing a life restarts the current stage with your score intact; losing all lives ends the run. A fresh fire press retries with the selected starting lives.
- **Catch your breath.** Scenic crossings are safe, but bombs still consume inventory. Leaving a cleared boss stage restores one life, up to the selected mode's starting lives.

Wait fifteen seconds at the title for a scenery tour. The digital track continues across every attract map and the return to the title. Move or press fire to return, then release and press fire to start. Release held fire before restarting after a run, too. Completing the campaign starts Galactic Dash with the gold-medal reveal; a fresh fire press returns to the title with that track still playing.

Secret music preview: press **CTRL + C= (Commodore) + DEL** at the title or during attract mode to switch between the two digital songs, even before winning. Hold the modifiers and tap DEL again to switch back. Each switch starts the chosen song from the beginning; the selection stays until you switch again, complete the campaign, or reload.

## Help

See the [setup and troubleshooting guide](docs/hardware.md) for help getting started, or the [release notes](https://github.com/fechols/havoc64u/releases/tag/v0.1.2) for what's new. Found a problem? [Report it](https://github.com/fechols/havoc64u/issues) with your hardware, video standard, and what happened.

## License

The game, its original tools, models, SID score, and screenshots are released under the [MIT License](LICENSE).

All twenty-one original digital recordings were composed for H.A.V.O.C. by **William Echols**: Hammond Rocker, Galactic Dash, Mission Failed, Lowland Approach, Crystal Cave, Iron Canyon, Canyon Gate, Ashen Wastes, Foundry Belt, Fortress Run, Citadel Core, Storm Front, Meteor Reach, Carrier Coast, Carrier Assault, Asteroid Belt, Lunar Awe, Ship Graveyard, Orbital Shipyard, Space Carrier, and Jump Gate. Their source recordings and encoded versions are covered by the [music MIT License](assets/music/LICENSE), included in player archives as `MUSIC-LICENSE.txt`.

The digital encoder also includes public-domain code under its accompanying [Unlicense](tools/encode-digital-music/UNLICENSE).
