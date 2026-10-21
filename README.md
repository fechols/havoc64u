# H.A.V.O.C.

**Heavy Assault Vessel: Orbital Combat**

Your ship is an unreasonable amount of firepower with a cockpit attached!

Fly an original 3D rail shooter for the **64 MHz C64 Ultimate**. Skim green hills, squeeze through crystal caves, and blast into orbit to take on enemy fleets.

## Features

- **3D worlds** — Dive through colorful landscapes and orbital scenery.
- **Stereo SID music** — Enjoy original tunes, with optional quad-SID arrangements.
- **18 varied stages** — Journey from green hills to orbital battlefields.
- **Five bosses** — Break their defenses and strike their exposed cores.
- **Aircraft-like handling** — Bank, climb, and dive with weight and momentum.
- **Versatile weapons** — Unleash autofire, charged shots, homing missiles, and bombs.
- **Defensive barrel rolls** — Deflect enemy shots while keeping your guns firing.
- **Magnetic pickups** — Pull in nearby supplies to keep fighting.
- **Checkpoints and pause** — Retain your score after losing a life; take breaks.
- **Punchy impacts** — Feel hits and bombs through screen shake and flashes.

## See it in action

**Sound on!** Hear the enhanced four-SID score and combat effects as the action climbs into orbit. Recorded in a PAL emulator at the game's original speed (72 seconds).

https://github.com/user-attachments/assets/f2c0d2a9-250d-456e-89e2-7e5f1bc5605a

Screenshots: **v0.1.0, four-SID build**, captured in a PAL emulator.

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

**H.A.V.O.C. v0.1.0** includes the complete eighteen-stage campaign. Choose your preferred arrangement:

- **[Download the standard two-SID package](https://github.com/fechols/havoc64u/releases/download/v0.1.0/havoc-v0.1.0-2sid.zip)** — uses the two built-in UltiSIDs; no extra chips needed.
- **[Download the enhanced four-SID package](https://github.com/fechols/havoc64u/releases/download/v0.1.0/havoc-v0.1.0-4sid.zip)** — a fuller score using four built-in UltiSIDs with the configuration update below; no extra chips needed.

Both packages include the same missions and sound effects. Choose your sound profile before launching; it cannot be switched in-game.

The archived v0.1.0 four-SID download contains the older physical-SID configuration. Apply the [UltiSID configuration update](docs/hardware.md#update-an-existing-four-sid-package) to its included `havoc.cfg` before importing it. New builds include the updated routing.

You need a **C64 Ultimate running at 64 MHz**, **1 MiB of REU memory**, and a joystick connected to **port 2**. The supplied configuration sets the speed, memory, and sound routing. A stock C64 cannot run the game.

1. Unzip one package and copy its three files — `havoc.cfg`, `havoc.reu`, and `havoc.prg` — to the Ultimate's storage.
2. Import `havoc.cfg`.
3. Load `havoc.reu` into the REU at **address zero**.
4. Run `havoc.prg`, then press fire to launch.

Keep all three files from the **same package and version** together. If startup reports a mismatch or invalid REU data, reload the matching REU at zero before running the PRG again.

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
| Space bar | Pause and resume the flight |
| F | Toggle the frame-rate readout at the top right |

Watch the weapon meter: **HOMING MISSILE** means your next shot tracks a random target ahead, switching targets when needed. Charge longer for stronger straight shots.

- **Roll through shots, dodge scenery.** Keep steering and firing during a roll, but avoid solid obstacles.
- **Clear a path.** Shoot small orbital fragments; bomb large rocks and wreckage. Every five enemy kills restore one bomb, up to three.
- **Collect supplies.** Green shields restore 60 points, up to 100; yellow rectangles add one bomb, up to three.
- **Break the boss's defenses.** Destroy both side units, then the core, to advance. Each boss defeat restores full shields and three bombs.
- **Make each life count.** You start with three lives. Losing one restarts the current stage with your score intact; losing all three ends the run.
- **Catch your breath.** Scenic crossings are safe, but bombs still consume inventory. Leaving a cleared boss stage restores one life, up to three.

Wait fifteen seconds at the title for scenery and music previews. Move or press fire to return, then release and press fire to start. Release held fire before restarting after a run, too.

## Help

See the [setup and troubleshooting guide](docs/hardware.md) for help getting started, or the [release notes](https://github.com/fechols/havoc64u/releases/tag/v0.1.0) for what's new. Found a problem? [Report it](https://github.com/fechols/havoc64u/issues) with your hardware, sound profile, and what happened.

## License

The game, tools, models, music, and screenshots are released under the [MIT License](LICENSE).
