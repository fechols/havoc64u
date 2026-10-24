#!/usr/bin/env python3
"""Native Story lifecycle checks using the production VICE smoke driver.

The linked keyboard test covers S itself. Here monitor writes select the mode
and arrange hostile projectiles; physical joystick FIRE and production SIM own
launch, damage, respawn, result dismissal, and retry. All offsets come from ELF.
"""
import importlib.util
from pathlib import Path
import re
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location("vice_smoke", Path(__file__).with_name("vice-smoke.py"))
VICE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VICE)


def checks(mon, syms, failures, elf, dwarf_tool, output, result):
    layout = VICE.dwarf_game_layout(elf, dwarf_tool, syms, output, controls=True)
    fields, modes = layout["fields"], layout["modes"]
    sizes = {"u8": 1, "i8": 1, "bool": 1, "u16": 2, "i16": 2, "u32": 4}
    for item in fields.values():
        if item["type"] in sizes:
            item["size"] = sizes[item["type"]]
    observed = ("mode", "story_mode", "lives", "shield", "score", "bombs", "menu_ticks",
                "respawn_ticks", "invulnerability", "player.x", "player.y", "player.z")
    for name in observed:
        if "size" not in fields[name]:
            raise RuntimeError(f"Unsupported Story fixture field {name}")
    structs = {}
    for name in ("Shot", "Actor", "Vec3"):
        description = subprocess.check_output([str(dwarf_tool), f"--name={name}",
            "--show-children", str(elf)], text=True)
        size = re.search(r"DW_AT_byte_size\s+\((0x[0-9a-fA-F]+|\d+)\)", description)
        if not size:
            raise RuntimeError(f"Missing {name} size")
        structs[name] = dict(size=int(size[1], 0), fields=VICE.dwarf_members(description, name))
    shot = structs["Shot"]
    for axis in ("x", "y", "z"):
        part = structs["Vec3"]["fields"][axis]
        shot["fields"][f"pos.{axis}"] = dict(part, offset=shot["fields"]["pos"]["offset"] + part["offset"])
    result.update(passed=False, monitor_assisted=True, excluded_from_timing=True,
        scope="Mode selection and projectile fixtures use ELF-derived monitor writes; physical FIRE and production SIM execute launches, damage, respawn and retry. S scanning is covered separately by test-keyboard-kernel.py.",
        layout=layout, patches=[], frames=[], captures={})

    def write(label, address, raw):
        mon.command("bank ram")
        before = mon.read(address, len(raw))
        for offset in range(0, len(raw), 16):
            mon.command(f"> {address+offset:04x} " + " ".join(f"{v:02x}" for v in raw[offset:offset+16]))
        if mon.read(address, len(raw)) != raw:
            raise RuntimeError(f"Story fixture write failed: {label}")
        mon.command("bank cpu")
        result["patches"].append(dict(label=label, address=address, before=before.hex(), after=raw.hex()))

    def seed(**values):
        for name, value in values.items():
            item = fields[name]
            write(name, layout["address"] + item["offset"],
                value.to_bytes(item["size"], "little", signed=item["type"].startswith("i")))

    def observe(label):
        mon.run_until(syms["frame_complete"], failures)
        frame = VICE.counters(mon, syms)
        mon.command("bank ram")
        frame["game"] = VICE.read_game_state(mon, layout, observed)
        frame["guards"] = {name: mon.integer(syms[name], 1)
            for name in ("__stack_bottom", "__irq_stack_bottom")}
        mon.command("bank cpu")
        frame["label"] = label
        result["frames"].append(frame)
        if (any(frame.get(name, 0) for name in ("input_overflows", "frame_dropped", "music_underruns"))
                or any(v != 0xa5 for v in frame["guards"].values()) or frame["test_input"] != 255):
            raise RuntimeError(f"Story fixture lost native safety invariants: {label}")
        return frame["game"]

    def until(label, mode, lives, selected):
        for _ in range(40):
            game = observe(label)
            if game["mode"] == modes[mode]:
                if game["lives"] != lives or game["story_mode"] != selected:
                    raise RuntimeError(f"Wrong Story state for {label}: {game}")
                return game
        raise RuntimeError(f"Story fixture did not reach {mode}: {label}")

    def capture(name):
        result["captures"][name] = VICE.capture(mon, syms, output, name)

    def fire():
        mon.command("jpdb 1 $0f")

    def release():
        mon.command("jpdb 1 $1f")
        observe("released-fire")

    def hit():
        # Clear unrelated opponents, then insert one real hostile shot exactly
        # one forward step from the neutral player; never write remaining lives.
        for field, struct_name in (("shots", "Shot"), ("actors", "Actor")):
            count = int(re.search(r"\[(\d+)\]$", fields[field]["type"])[1])
            write(field, layout["address"] + fields[field]["offset"], bytes(count * structs[struct_name]["size"]))
        seed(shield=1, invulnerability=0)
        address = layout["address"] + fields["shots"]["offset"]
        for name, value in {"kind": 2, "damage": 10, "dx": 0, "dy": 0,
                            "pos.x": 0, "pos.y": 0, "pos.z": 198}.items():
            item = shot["fields"][name]
            write(f"shot.{name}", address + item["offset"],
                value.to_bytes(sizes[item["type"]], "little", signed=item["type"].startswith("i")))

    # The common smoke has already started a normal run with physical FIRE.
    release()
    game = observe("normal-launch")
    if game["lives"] != 3 or game["story_mode"]:
        raise RuntimeError("Fresh boot did not start Normal with three lives")
    capture("normal-three-lives")
    # Selection is the only Story-specific state patched by this test.
    seed(mode=modes["Title"], menu_ticks=0, story_mode=1)
    until("story-title", "Title", 3, 1)
    capture("story-title")
    fire()
    until("story-launch", "Playing", 99, 1)
    capture("story-99-lives")
    release()
    hit()
    until("story-death", "Respawning", 98, 1)
    capture("story-98-respawning")
    until("story-respawn", "Playing", 98, 1)
    capture("story-98-lives")
    # Skip the other97 deaths, then let the same real collision reach results.
    seed(lives=1)
    hit()
    until("story-final-death", "GameOver", 0, 1)
    capture("story-game-over")
    release()
    fire()
    until("story-retry", "Playing", 99, 1)
    capture("story-retry-99")
    release()
    # Returning to Title preserves the selected mode; changing it back must
    # restore the original three-life start on the next fresh FIRE edge.
    seed(mode=modes["Title"], menu_ticks=0)
    until("story-return-title", "Title", 99, 1)
    seed(story_mode=0)
    until("normal-title", "Title", 99, 0)
    capture("normal-title")
    fire()
    until("normal-relaunch", "Playing", 3, 0)
    capture("normal-relaunch-three")
    release()
    result["passed"] = True
    print("STORY: Normal3; Story99 → death98 → respawn98 → GameOver → retry99; Normal3 restored", flush=True)


if __name__ == "__main__":
    VICE.hud_checks = checks
    if "--hud-checks" not in sys.argv:
        sys.argv.append("--hud-checks")
    VICE.main()
