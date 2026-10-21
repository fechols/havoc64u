#!/usr/bin/env python3
"""Check the showcase controller without running or changing the native game."""
import copy
import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location("showcase", Path(__file__).with_name("capture-showcase.py"))
showcase = importlib.util.module_from_spec(spec)
spec.loader.exec_module(showcase)


def state():
    return dict(stage_id=0, ticks=100, mode=1, player=dict(x=0, y=0, z=180),
                flight=dict(frac=[0, 0], vel=[0, 0]), actors=[], shots=[], pickups=[],
                boss_started=0, boss_defeated=0, roll_ticks=0, roll_direction=0)


class ShowcasePilot(unittest.TestCase):
    def setUp(self):
        self.package = bytearray(0xb0800)
        self.package[0xb0000 + 88 + 3] = 20  # Fighter's authored ram radius.

    def test_brakes_for_inertia_and_holds_fire_without_periodic_roll_taps(self):
        game = state()
        self.assertEqual(showcase.showcase_input(game, self.package), 16)
        game["flight"]["vel"] = [768, 0]
        self.assertEqual(showcase.showcase_input(game, self.package), 16 | 4)
        for tick in (101, 102, 112, 200):
            game["ticks"] = tick
            self.assertEqual(showcase.showcase_input(game, self.package), 16 | 4)

    def test_avoids_an_ordinary_fighter_ram_without_modifying_state(self):
        game = state()
        game["actors"] = [dict(kind=1, hp=4, age=100, flags=0, approach_speed=8,
                              origin_x=0, origin_y=0, pos=dict(x=0, y=0, z=500))]
        original = copy.deepcopy(game)
        bits = showcase.showcase_input(game, self.package)
        self.assertEqual(bits & 16, 16)
        self.assertNotEqual(bits & 15, 0, "the aiming target is also a collision threat")
        self.assertEqual(game, original)

    def test_evades_incoming_fire_but_ignores_own_shots_and_retired_actors(self):
        game = state()
        game["shots"] = [dict(kind=2, dx=0, dy=0, pos=dict(x=0, y=0, z=540))]
        self.assertNotEqual(showcase.showcase_input(game, self.package) & 15, 0)
        game["shots"][0]["kind"] = 1
        game["actors"] = [dict(kind=1, hp=0, pos=dict(x=0, y=0, z=300))]
        self.assertEqual(showcase.showcase_input(game, self.package), 16)


if __name__ == "__main__":
    unittest.main()
