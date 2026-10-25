#!/usr/bin/env python3
"""Exercise the physical-input pilot's obstacle choices without running VICE."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("native_campaign", Path(__file__).with_name("native-campaign.py"))
pilot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pilot)


def actor(kind=7, flags=0, hp=255, x=0, y=-20, z=500):
    return dict(kind=kind, flags=flags, hp=hp, pos=dict(x=x, y=y, z=z))


def state(*actors):
    return dict(stage_id=12, actors=list(actors), pickups=[], player=dict(x=0, y=0),
                boss_started=False, boss_defeated=False, boss_left=20, boss_right=20)


def boss(x, y, left=10, right=10, core=30):
    return dict(pos=dict(x=x, y=y, z=500), left=left, right=right, core=core, flash=0)


class PilotTests(unittest.TestCase):
    def test_obstacle_centers_do_not_read_enemy_descriptor_memory(self):
        self.assertEqual(pilot.actor_center(actor(), 12, b""), (0, 0))
        self.assertEqual(pilot.actor_center(actor(flags=2, hp=2, y=-10), 12, b""), (0, 0))
        self.assertEqual(pilot.actor_center(actor(kind=6, y=-30), 12, b""), (0, 0))

    def test_solid_asteroid_has_a_safe_evasion_target(self):
        point, avoiding = pilot.pilot_target(state(actor()), b"")
        self.assertTrue(avoiding)
        self.assertGreater(max(abs(point[0]), abs(point[1])), 54)

    def test_small_fragment_is_aimed_at_while_distant_then_avoided_if_alive(self):
        distant = actor(flags=2, hp=2, x=60, y=35, z=1600)
        self.assertEqual(pilot.pilot_target(state(distant), b""), ((60, 45), False))
        close = actor(flags=2, hp=2, y=-10, z=400)
        point, avoiding = pilot.pilot_target(state(close), b"")
        self.assertTrue(avoiding)
        self.assertGreater(max(abs(point[0]), abs(point[1])), 36)

    def test_destroyed_fragment_does_not_divert_the_pilot(self):
        self.assertEqual(pilot.pilot_target(state(actor(flags=2, hp=0)), b""), ((0, 0), False))

    def test_boss_core_is_targeted_only_after_both_side_units(self):
        current = state()
        current.update(boss_started=True, boss_left=0, boss_right=0)
        self.assertEqual(pilot.pilot_target(current, b""), ((0, 0), False))
        self.assertEqual(pilot.pilot_target(current, b"", finish_boss=False), ((100, 30), False))

    def test_split_boss_aim_uses_scaled_defenses_and_skips_dead_siblings(self):
        current = state()
        current.update(boss_started=True, boss_phase=1, boss_split_ticks=0,
                       bosses=[boss(-74, 0, core=0), boss(74, 0, 20, 20, 60)])
        self.assertEqual(pilot.pilot_target(current, b""), ((42, 7), False))
        current["bosses"][1]["left"] = 0
        self.assertEqual(pilot.pilot_target(current, b""), ((100, 7), False))
        current["bosses"][1]["right"] = 0
        self.assertEqual(pilot.pilot_target(current, b""), ((74, 0), False))

    def test_final_wave_pilot_reaches_each_remaining_boss(self):
        current = state()
        current.update(boss_started=True, boss_phase=2, boss_split_ticks=0,
                       bosses=[boss(x, y) for y in (-40, 40) for x in (-55, 55)])
        for index, target in enumerate(((-71, -37), (39, -37), (-71, 43), (39, 43))):
            self.assertEqual(pilot.pilot_target(current, b""), (target, False))
            current["bosses"][index]["core"] = 0


if __name__ == "__main__":
    unittest.main()
