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


def campaign(mon, syms, failures, elf, dwarf, output, report):
    layouts = bench.Layouts(elf, dwarf, output)
    address = vice.resident_symbol(syms, "GAME")[1]
    size = layouts.size("Game")
    image = (elf.parent / "havoc.reu").read_bytes()
    report.update(passed=False, scope="Continuous campaign, physical port-2 joystick only; no game or clock writes",
                  checkpoints=[], visited=[], frames=0)
    visited = set()
    previous = None
    last_bits = None
    for frame in range(20000):
        mon.command("bank ram")
        state = layouts.decode("Game", mon.read(address, size))
        mon.command("bank cpu")
        stage, mode = state["stage_id"], state["mode"]
        visited.add(stage)
        checkpoint = (stage, mode, state["lives"], state["boss_defeated"])
        if checkpoint != previous:
            counters = vice.counters(mon, syms)
            if counters["frame_dropped"] or counters["input_overflows"] or counters.get("music_underruns", 0):
                raise RuntimeError(f"Campaign dropped geometry, input or music: {counters}")
            if counters["test_input"] != 255:
                raise RuntimeError("Campaign is not using physical joystick input")
            row = {key: state[key] for key in ("stage_id", "mode", "rail_ticks", "ticks", "lives", "shield", "score",
                                               "boss_started", "boss_defeated", "boss_left", "boss_right", "boss_core")}
            row["frame"] = frame
            row["counters"] = counters
            report["checkpoints"].append(row)
            report.update(frames=frame, visited=sorted(visited))
            (output / "campaign-progress.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"CAMPAIGN stage={stage + 1} mode={mode} rail={state['rail_ticks']} lives={state['lives']} shield={state['shield']}", flush=True)
            previous = checkpoint
        if mode == 4:
            raise RuntimeError(f"Pilot lost the campaign in stage {stage + 1}")
        if mode == 3 and state["menu_ticks"] >= 400:
            if visited != set(range(12)):
                raise RuntimeError(f"Campaign omitted stages: {visited}")
            report.update(passed=True, frames=frame, visited=sorted(visited), final_state=state,
                          capture=vice.capture(mon, syms, output, "campaign-victory"))
            return
        target_x = target_y = 0
        if state["boss_started"] and not state["boss_defeated"]:
            target_x = -100 if state["boss_left"] else 100 if state["boss_right"] else 0
            target_y = 30 if state["boss_left"] or state["boss_right"] else 0
        else:
            targets = [a for a in state["actors"] if a["kind"] and a["hp"] != 255 and 300 < a["pos"]["z"] < 2400]
            if targets:
                actor = min(targets, key=lambda a: a["pos"]["z"])
                descriptor = 0xb0000 + stage * 2048 + 88 + (actor["kind"] - 1) * 8
                center = int.from_bytes(image[descriptor + 1:descriptor + 2], "little", signed=True)
                target_x, target_y = actor["pos"]["x"], actor["pos"]["y"] + center
            else:
                pickup = next((p for p in state["pickups"] if p["kind"] == 1 and p["pos"]["z"] > 200), None)
                if pickup:
                    target_x, target_y = pickup["pos"]["x"], pickup["pos"]["y"]
        dx, dy = target_x - state["player"]["x"], target_y - state["player"]["y"]
        bits = 16 | (8 if dx > 5 else 4 if dx < -5 else 0) | (1 if dy > 5 else 2 if dy < -5 else 0)
        # Consecutive release/press frames form real double taps inside the
        # game's 500 ms input window, independent of PAL/NTSC refresh rate.
        if mode == 1:
            if frame % 12 in (0, 2):
                bits &= ~12
            elif frame % 12 in (1, 3):
                bits = (bits & ~8) | 4
            if state["boss_started"] and frame % 48 in (0, 2):
                bits &= ~16
        if bits != last_bits:
            mon.command(f"jpdb 1 ${(31 ^ bits):02x}")
            last_bits = bits
        mon.run_until(syms["frame_complete"], failures)
    raise RuntimeError("Campaign did not reach its victory reveal within 20,000 rendered frames")


if __name__ == "__main__":
    # Run through the existing monitor/session lifecycle, replacing only its
    # optional controls scenario with this continuous input-driven scenario.
    vice.control_checks = campaign
    if "--control-checks" not in sys.argv:
        sys.argv.append("--control-checks")
    sys.exit(vice.main())
