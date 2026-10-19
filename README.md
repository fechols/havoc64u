# H.A.V.O.C.

**Heavy Assault Vessel: Orbital Combat**

Your ship is an unreasonable amount of firepower with a cockpit attached!

Fly an original polygon rail shooter for the **64 MHz C64 Ultimate**. Skim green hills, squeeze through crystal caves, dodge canyon batteries, and take the fight to an enemy carrier. Everything is drawn in the C64's sixteen colors, backed by an original stereo SID soundtrack.

## See it in action

**Sound on!** Unmute the player to hear the four-SID arrangement and the game's own weapon, engine, and impact effects. Footage comes from the compiled C64 game running in an emulator.

https://github.com/user-attachments/assets/7493efc7-68c3-4a6f-b649-75e3ac2fc472

Twelve stages take you through three acts, with three bosses standing between you and the end of the mission. A clean run takes roughly seventeen minutes; retries and stubborn bosses add time. Between battles, brief scenic flights give you room to breathe while the landscape and music carry you onward.

| Lowland Approach | Crystal Cave |
| --- | --- |
| ![Ship flying over green hills in Lowland Approach](docs/media/lowlands.png) | ![Flight through the enclosed Crystal Cave](docs/media/crystal-cave.png) |
| **Iron Canyon** | **Foundry Belt** |
| ![Combat between the ridges of Iron Canyon](docs/media/iron-canyon.png) | ![Industrial scenery in Foundry Belt](docs/media/foundry.png) |
| **Storm Front** | **Carrier Assault** |
| ![Flight beneath the dark sky of Storm Front](docs/media/storm-front.png) | ![Approaching the enemy carrier in Carrier Assault](docs/media/carrier.png) |

## Download and launch

This is a **development preview**. Choose the package that matches your sound hardware:

- **[Download the standard two-SID package](https://github.com/fechols/havoc64u/releases/download/v0.1.0-preview.1/havoc-preview-2sid.zip)** — uses the Ultimate's two built-in UltiSIDs. No physical SID chips are required.
- **[Download the enhanced four-SID package](https://github.com/fechols/havoc64u/releases/download/v0.1.0-preview.1/havoc-preview-4sid.zip)** — adds two physical ARMSIDs for a fuller musical arrangement. Set both ARMSIDs to **8580 behavior** separately.

Both versions include the complete campaign, music, and sound effects. There is no in-game sound-profile switch.

You need a **C64 Ultimate running at 64 MHz**, its **16 MiB REU**, and a joystick connected to **port 2**. The supplied configuration sets the speed, REU, and SID routing. This release requires the Ultimate's capabilities; a stock C64 cannot run it.

1. Unzip one package and copy its three files — `havoc.cfg`, `havoc.reu`, and `havoc.prg` — to the Ultimate's storage.
2. Import `havoc.cfg`.
3. Load `havoc.reu` into the REU at **address zero**.
4. Run `havoc.prg`, then press fire to launch.

Keep all three files from the **same package and version** together. If startup reports a mismatch or invalid REU data, reload the matching REU at zero before running the PRG again. Once loaded, the game needs no further disk reads.

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

Watch the weapon meter: when it reads **HOMING MISSILE**, your next shot will seek an enemy ahead. Partial charges produce stronger straight shots.

- **Roll through enemy fire.** Rolls deflect hostile shots, and you can keep steering and firing. Scenery can still hurt you.
- **Use your bombs.** Every five enemy kills restore one, up to a maximum of three. Shield and bomb pickups help keep you flying.
- **Aim for the boss's side units first.** Destroy both to expose its core. You must defeat the boss before its stage can end.
- **Make each life count.** You start with three. Losing one returns you to the current stage's checkpoint with your score intact; losing all three ends the run.
- **Catch your breath between stages.** Scenic crossings are safe, but bombs still consume inventory. Clearing either of the first two acts also replenishes supplies.

Leave the title idle for fifteen seconds to watch the automatic scenery previews and hear the music. Move or press fire to return to the title, then release and press fire again to start. After a run, release any held fire button before restarting, too.

## Current status

The campaign has completed emulator playthroughs, but **physical C64 Ultimate testing is still outstanding**. Rendering is currently slow, around six frames per second in measured emulator opening scenes; performance varies by scene. Music and sound effects continue independently of the picture.

For sound setup, troubleshooting, or building from source, see the [hardware and build guide](docs/hardware.md). Developers can also read the [performance measurements](docs/benchmarks.md). Found a problem? [Open an issue](https://github.com/fechols/havoc64u/issues) with your hardware, sound profile, and what happened.
