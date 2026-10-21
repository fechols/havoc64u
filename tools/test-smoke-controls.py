#!/usr/bin/env python3
"""Regression for smoke-control movement checks without launching VICE."""
import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location("vice_smoke", Path(__file__).with_name("vice-smoke.py"))
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)


def state(x, bits=0):
    return {"player.x": x, "controls.bits": bits}


class DodgeMotionTests(unittest.TestCase):
    def test_recorded_right_roll_trace_has_real_motion_despite_zero_gesture_net(self):
        # smoke-painter-4sid-ntsc: ticks 152, 172, 177. The previous
        # left roll coasts during the tap gap; the right roll then reverses it.
        before_gesture, before_activation, active = map(state, (-68, -70, -68))
        self.assertEqual(active["player.x"] - before_gesture["player.x"], 0)
        self.assertEqual(smoke.released_dodge_displacement(before_activation, active, 1, "right"), 2)
        # The live harness now measures the following frame against active,
        # after validating roll activation, direction and countdown.
        self.assertEqual(smoke.released_dodge_displacement(active, state(-65), 1, "right"), 3)

    def test_stationary_wrong_direction_or_held_input_still_fail(self):
        for direction, label in ((-1, "left"), (1, "right")):
            for after in (state(10), state(10 - direction), state(10 + direction, 4 if direction < 0 else 8)):
                with self.subTest(direction=direction, after=after):
                    with self.assertRaisesRegex(RuntimeError, "no physical dodge displacement after release"):
                        smoke.released_dodge_displacement(state(10), after, direction, label)

    def test_both_roll_directions_require_positive_directional_motion(self):
        self.assertEqual(smoke.released_dodge_displacement(state(10), state(7), -1, "left"), -3)
        self.assertEqual(smoke.released_dodge_displacement(state(-10), state(-7), 1, "right"), 3)


if __name__ == "__main__":
    unittest.main()
