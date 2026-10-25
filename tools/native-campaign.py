#!/usr/bin/env python3
"""Play a continuous native campaign using physical joystick-port input only.

Reuses vice-smoke's boot, package hashing, monitor and failure handling. The
pilot reads DWARF-described game state to aim, but never writes game state,
simulation time, assets, health or benchmark controls. Run with the ordinary
vice-smoke arguments, e.g. --package-dir dist --video pal --out-dir target/run.
"""
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "tools" / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


vice = module("campaign_vice", "vice-smoke.py")
bench = module("campaign_layouts", "bench-native.py")


def actor_center(actor, stage, image):
    """Use the native collision center without indexing non-enemy descriptors."""
    kind = actor["kind"]
    if kind == 7:
        center = 10 if actor.get("flags", 0) & 2 else 20
    elif kind == 6:
        center = 30
    elif 1 <= kind <= 3:
        descriptor = 0xb0000 + stage * 2048 + 88 + (kind - 1) * 8
        center = int.from_bytes(image[descriptor + 1:descriptor + 2], "little", signed=True)
    else:
        center = 0
    return actor["pos"]["x"], actor["pos"]["y"] + center


def pilot_target(state, image, finish_boss=True):
    """Aim at live targets, then choose a clear route around near obstacles."""
    stage = state["stage_id"]
    target = (0, 0)
    if state["boss_started"] and not state["boss_defeated"]:
        if "bosses" in state:
            phase = state["boss_phase"]
            divisor = (1, 4, 8)[phase]
            for boss in state["bosses"][:1 << phase]:
                if not boss["core"]:
                    continue
                x, y = boss["pos"]["x"], boss["pos"]["y"]
                if boss["left"]:
                    target = x - 130 // divisor, y + 30 // divisor
                elif boss["right"] or not finish_boss:
                    target = x + 130 // divisor, y + 30 // divisor
                else:
                    target = x, y
                break
        else:
            target = (-100 if state["boss_left"] else 100 if state["boss_right"] or not finish_boss else 0,
                      30 if state["boss_left"] or state["boss_right"] or not finish_boss else 0)
    else:
        targets = [actor for actor in state["actors"]
                   if actor["kind"] and 0 < actor["hp"] < 255
                   and (850 if actor["kind"] == 7 else 300) < actor["pos"]["z"] < 2400]
        if targets:
            target = actor_center(min(targets, key=lambda a: a["pos"]["z"]), stage, image)
        else:
            pickup = next((p for p in state["pickups"]
                           if p["kind"] == 1 and p["pos"]["z"] > 200), None)
            if pickup:
                target = pickup["pos"]["x"], pickup["pos"]["y"]
    target = max(-100, min(100, target[0])), max(-65, min(65, target[1]))
    hazards = [actor for actor in state["actors"]
               if actor["kind"] in (6, 7) and actor["hp"] > 0
               and 100 < actor["pos"]["z"] < (650 if actor.get("flags", 0) & 2 else 1000)]
    if not hazards:
        return target, False

    def risk(point):
        value = 0
        for actor in hazards:
            x, y = actor_center(actor, stage, image)
            radius = (18 if actor.get("flags", 0) & 2 else 36) + 18
            # Native collision is an axis-aligned square, not radial distance.
            clearance = max(abs(point[0] - x), abs(point[1] - y)) - radius
            if clearance <= 0:
                value += (1 + 1000 - actor["pos"]["z"]) * (1 - clearance)
        return value

    player = state["player"]["x"], state["player"]["y"]
    if risk(target) == 0 and risk(player) == 0:
        return target, False
    choices = [target, player] + [(x, y) for x in (-100, -50, 0, 50, 100)
                          for y in (-65, -30, 0, 30, 65)]
    safest = min(choices, key=lambda point: (
        risk(point),
        abs(point[0] - player[0]) + abs(point[1] - player[1])
        + (abs(point[0] - target[0]) + abs(point[1] - target[1])) / 4,
    ))
    return safest, True


def campaign(mon, syms, failures, elf, dwarf, output, report):
    layouts = bench.Layouts(elf, dwarf, output)
    address = vice.resident_symbol(syms, "GAME")[1]
    size = layouts.size("Game")
    image = (elf.parent / "havoc.reu").read_bytes()
    stage_count = vice.package_model_layout(image)["count"]
    report.update(passed=False, scope="Continuous campaign, physical port-2 joystick only; no game or clock writes",
                  diagnostic_scope="Geometry drops every frame; cumulative input/music diagnostics at checkpoints",
                  geometry_frames_checked=0, checkpoints=[], visited=[], frames=0)
    visited = set()
    previous = None
    last_bits = None
    for frame in range(30000):
        mon.command("bank ram")
        state = vice.logical_game_state(layouts.decode("Game", mon.read(address, size)))
        mon.command("bank cpu")
        if mon.integer(syms["FRAME_DROPPED"], 2):
            raise RuntimeError(f"Campaign dropped geometry in rendered frame {frame}")
        report["geometry_frames_checked"] = frame + 1
        stage, mode = state["stage_id"], state["mode"]
        visited.add(stage)
        checkpoint = (stage, mode, state["lives"], state["boss_defeated"],
                      state.get("boss_phase", 0), bool(state.get("boss_split_ticks", 0)))
        if checkpoint != previous:
            counters = vice.counters(mon, syms)
            if counters["frame_dropped"] or counters["input_overflows"] or counters.get("music_underruns", 0):
                raise RuntimeError(f"Campaign dropped geometry, input or music: {counters}")
            if counters["test_input"] != 255:
                raise RuntimeError("Campaign is not using physical joystick input")
            row = {key: state[key] for key in ("stage_id", "mode", "rail_ticks", "ticks", "lives", "shield", "score",
                                               "boss_started", "boss_defeated", "boss_left", "boss_right", "boss_core")}
            row["frame"] = frame
            if "bosses" in state:
                row.update(bosses=state["bosses"], boss_phase=state["boss_phase"],
                           boss_split_ticks=state["boss_split_ticks"])
            row["stage_ticks"] = state.get("stage_ticks", state["rail_ticks"] - stage * 3750)
            row["counters"] = counters
            report["checkpoints"].append(row)
            report.update(frames=frame, visited=sorted(visited))
            (output / "campaign-progress.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"CAMPAIGN stage={stage + 1} mode={mode} rail={state['rail_ticks']} lives={state['lives']} shield={state['shield']}", flush=True)
            previous = checkpoint
        if mode == 4:
            raise RuntimeError(f"Pilot lost the campaign in stage {stage + 1}")
        if mode == 3 and state["menu_ticks"] >= 400:
            if visited != set(range(stage_count)):
                raise RuntimeError(f"Campaign omitted stages: {visited}")
            report.update(passed=True, frames=frame, visited=sorted(visited), final_state=state,
                          capture=vice.capture(mon, syms, output, "campaign-victory"))
            return
        (target_x, target_y), avoiding = pilot_target(state, image)
        dx, dy = target_x - state["player"]["x"], target_y - state["player"]["y"]
        bits = 16 | (8 if dx > 5 else 4 if dx < -5 else 0) | (1 if dy > 5 else 2 if dy < -5 else 0)
        # Consecutive release/press frames form real double taps inside the
        # game's 500 ms input window, independent of PAL/NTSC refresh rate.
        if mode == 1:
            if not avoiding and frame % 12 in (0, 2):
                bits &= ~12
            elif not avoiding and frame % 12 in (1, 3):
                bits = (bits & ~8) | 4
            if state["boss_started"] and not state.get("boss_split_ticks", 0) and frame % 48 in (0, 2):
                bits &= ~16
        if bits != last_bits:
            mon.command(f"jpdb 1 ${(31 ^ bits):02x}")
            last_bits = bits
        mon.run_until(syms["frame_complete"], failures)
    raise RuntimeError("Campaign did not reach its victory reveal within 30,000 rendered frames")


if __name__ == "__main__":
    # Run through the existing monitor/session lifecycle, replacing only its
    # optional controls scenario with this continuous input-driven scenario.
    vice.control_checks = campaign
    if "--control-checks" not in sys.argv:
        sys.argv.append("--control-checks")
    sys.exit(vice.main())
