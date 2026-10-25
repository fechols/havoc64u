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


class NeutralJoystickTests(unittest.TestCase):
    def module(self, version=b"\x00\x01", size=26):
        return b"JPIOSIM" + bytes(9) + version + size.to_bytes(4, "little") + b"\x1f\x80\x00\x00"

    def test_only_analog_bytes_change_and_roundtrip_is_idempotent(self):
        original = b"snapshot prefix" + self.module() + b"other modules"
        neutral, modules = smoke.neutral_joystick_snapshot(original)
        self.assertEqual(neutral, original[:39] + b"\xff\xff" + original[41:])
        self.assertEqual(modules, [dict(module_offset=15, before="0000", after="ffff")])
        repeated, repeated_modules = smoke.neutral_joystick_snapshot(neutral)
        self.assertEqual(repeated, neutral)
        self.assertEqual(repeated_modules[0]["before"], "ffff")

    def test_unknown_version_wrong_size_and_truncation_are_rejected(self):
        for snapshot in (self.module(version=b"\x00\x02"), self.module(size=25), self.module()[:-1]):
            with self.subTest(snapshot=snapshot):
                with self.assertRaisesRegex(RuntimeError, "Unsupported JPIOSIM"):
                    smoke.neutral_joystick_snapshot(snapshot)

    def test_missing_and_multiple_devices_are_rejected(self):
        for snapshot in (b"no iosim device", self.module() + self.module()):
            with self.subTest(snapshot=snapshot):
                with self.assertRaisesRegex(RuntimeError, "Expected one port2 JPIOSIM"):
                    smoke.neutral_joystick_snapshot(snapshot)


if __name__ == "__main__":
    unittest.main()
