use super::{step, Actor};
use crate::Vec3;
use std::vec::Vec;

const PLAYER: Vec3 = Vec3 { x: 0, y: 0, z: 180 };

fn actor(kind: u8, x: i16, y: i16, z: i16) -> Actor {
    Actor {
        pos: Vec3 { x, y, z },
        origin_x: x,
        origin_y: y,
        age: 0,
        kind,
        hp: 3,
        flags: 1,
        approach_speed: 8,
    }
}

fn pose(level: u8, kind: u8, age: u16) -> Vec3 {
    let mut a = actor(kind, 0, 0, 10_000);
    a.age = age;
    step(level, &mut a, &PLAYER, 0);
    a.pos
}

#[test]
fn harmonic_paths_have_distinct_turns_and_crossings() {
    // Quarter-cycle landmarks distinguish a figure-eight, a swoop, and ellipses.
    for (level, kind, age, x, y) in [
        (0, 1, 16, 45, 32),
        (0, 1, 32, 64, 0),
        (0, 1, 64, 0, 0),
        (0, 1, 96, -64, 0),
        (1, 2, 16, 32, 45),
        (1, 2, 48, -32, -45),
        (1, 2, 64, 0, -64),
        (2, 1, 0, 0, 16),
        (2, 1, 64, 32, 0),
        (2, 1, 128, 0, -16),
        (2, 1, 192, -32, 0),
        (3, 1, 0, 0, 16),
        (3, 1, 32, 32, 0),
        (3, 1, 64, 0, -16),
        (3, 1, 96, -32, 0),
    ] {
        let p = pose(level, kind, age);
        assert_eq!((p.x, p.y), (x, y), "level {level}, kind {kind}, age {age}");
    }
}

#[test]
fn harmonic_paths_stay_bounded_and_repeat_without_phase_wrap_jumps() {
    for (level, kind, width, height, period, max_dx, max_dy) in [
        (0, 1, 64, 32, 128, 4, 4),
        (1, 2, 32, 64, 128, 4, 4),
        (2, 1, 32, 16, 256, 1, 1),
        (3, 1, 32, 16, 128, 2, 1),
    ] {
        let positions: Vec<_> = (0..=512).map(|age| pose(level, kind, age)).collect();
        let extrema = positions
            .iter()
            .fold((i16::MAX, i16::MIN, i16::MAX, i16::MIN), |b, p| {
                (b.0.min(p.x), b.1.max(p.x), b.2.min(p.y), b.3.max(p.y))
            });
        assert_eq!(extrema, (-width, width, -height, height), "level {level}");
        for (age, pair) in positions.windows(2).enumerate() {
            assert!(
                (pair[1].x - pair[0].x).abs() <= max_dx,
                "x jump: level {level}, age {age}"
            );
            assert!(
                (pair[1].y - pair[0].y).abs() <= max_dy,
                "y jump: level {level}, age {age}"
            );
        }
        for age in period..positions.len() {
            assert_eq!(
                positions[age],
                positions[age - period],
                "period: level {level}, age {age}"
            );
        }
    }
}

#[test]
fn skimmers_cross_once_while_their_vertical_weave_continues() {
    for origin in [-110, 110] {
        let mut a = actor(1, origin, 13, 4000);
        let mut previous = a.pos;
        let mut low = i16::MAX;
        let mut high = i16::MIN;
        for age in 1..=256 {
            a.age = age;
            step(1, &mut a, &PLAYER, 0);
            let dx = a.pos.x - previous.x;
            assert_eq!(
                dx,
                if age > 110 {
                    0
                } else if origin < 0 {
                    2
                } else {
                    -2
                }
            );
            assert!((a.pos.y - previous.y).abs() <= 2);
            assert_eq!(a.pos.z, 4000 - age as i16 * 10);
            low = low.min(a.pos.y);
            high = high.max(a.pos.y);
            if age >= 110 {
                assert_eq!(a.pos.x, -origin);
            }
            previous = a.pos;
        }
        assert_eq!((low, high), (13 - 32, 13 + 32));
    }
}

#[test]
fn sentries_feint_inward_then_return_before_holding_and_rushing() {
    for origin in [-70, -60, 60, 70] {
        let mut a = actor(2, origin, 25, 1800);
        let mut previous_x = origin;
        for age in 1..=180 {
            a.age = age;
            step(2, &mut a, &PLAYER, 0);
            assert!((a.pos.x - previous_x).abs() <= 2, "age {age}");
            assert_eq!(a.pos.y, 25);
            if let Some((_, offset, z)) = [
                (48, 0, 1416),
                (49, 2, 1408),
                (80, 64, 1160),
                (81, 62, 1152),
                (111, 2, 912),
                (112, 0, 904),
                (113, 0, 904),
                (120, 0, 904),
                (128, 0, 904),
                (152, 0, 904),
                (153, 0, 890),
            ]
            .iter()
            .find(|&&(sample, _, _)| sample == age)
            {
                assert_eq!(
                    a.pos.x,
                    origin + if origin < 0 { *offset } else { -*offset }
                );
                assert_eq!(a.pos.z, *z);
            }
            if age <= 48 || age >= 112 {
                assert_eq!(a.pos.x, origin);
            }
            previous_x = a.pos.x;
        }
    }
}

#[test]
fn absolute_paths_use_elapsed_age_and_spawn_origin_without_accumulating_drift() {
    for (level, kind) in [(0, 1), (1, 1), (1, 2), (2, 1), (2, 2), (3, 1)] {
        let mut continuous = actor(kind, -70, 25, 10_000);
        for age in 1..=512 {
            continuous.age = age;
            step(level, &mut continuous, &PLAYER, 0);
            if [49, 80, 112, 127, 128, 255, 256, 257, 512].contains(&age) {
                let mut late = actor(kind, -70, 25, 1800);
                late.age = age;
                step(level, &mut late, &PLAYER, 0);
                assert_eq!(
                    (late.pos.x, late.pos.y),
                    (continuous.pos.x, continuous.pos.y),
                    "level {level}, kind {kind}, age {age}"
                );
                assert_eq!((late.origin_x, late.origin_y, late.flags), (-70, 25, 1));
            }
        }
    }
}

#[test]
fn pursuing_enemies_weave_at_distinct_rates_without_losing_their_speed_caps() {
    for (level, amplitude, quarter, vertical_cap) in [(0, 32, 32, 1), (3, 16, 16, 2)] {
        let mut centered = actor(2, 0, 0, 6000);
        let mut moving = actor(2, -80, 25, 6000);
        for age in 1..=256 {
            centered.age = age;
            step(level, &mut centered, &PLAYER, 0);
            if age == quarter || age == quarter * 3 {
                assert_eq!(
                    centered.pos.x,
                    if age == quarter {
                        amplitude
                    } else {
                        -amplitude
                    }
                );
            }
            if age % (quarter * 2) == 0 {
                assert_eq!(centered.pos.x, 0, "level {level}, age {age}");
            }
            assert!((-amplitude..=amplitude).contains(&centered.pos.x));
            assert_eq!(centered.pos.y, 0);

            let player = Vec3 {
                x: if age / 37 % 2 == 0 { 100 } else { -100 },
                y: if age / 29 % 2 == 0 { -65 } else { 65 },
                z: 180,
            };
            let previous = moving.pos;
            moving.age = age;
            step(level, &mut moving, &player, 0);
            assert!((moving.pos.x - previous.x).abs() <= 2);
            assert!((moving.pos.y - previous.y).abs() <= vertical_cap);
            assert_eq!(
                (moving.origin_x, moving.origin_y, moving.flags),
                (-80, 25, 1)
            );
        }
    }
}

#[test]
fn pursuit_uses_late_age_but_commits_at_the_existing_depth_boundaries() {
    for (level, speed, boundary, last_tracking_age, crest_age) in
        [(0, 12, 300, 124, 32), (3, 14, 500, 92, 16)]
    {
        // A late-age enemy follows the current phase immediately, rather than
        // restarting the weave or replacing its position with the spawn origin.
        let mut late = actor(2, -80, 0, 900);
        late.pos.x = 0;
        late.age = crest_age;
        step(level, &mut late, &PLAYER, 0);
        assert_eq!(late.pos.x, 2);

        let mut a = actor(2, 0, 0, 1800);
        for age in 1..=last_tracking_age {
            a.age = age;
            step(level, &mut a, &PLAYER, 0);
        }
        assert!(a.pos.z > boundary);
        let committed = (a.pos.x, a.pos.y);
        let fleeing = Vec3 {
            x: 100,
            y: 65,
            z: 180,
        };
        for age in last_tracking_age + 1..=last_tracking_age + 12 {
            a.age = age;
            step(level, &mut a, &fleeing, 0);
            assert!(a.pos.z <= boundary);
            assert_eq!((a.pos.x, a.pos.y), committed, "level {level}, age {age}");
            assert_eq!(a.pos.z, 1800 - age as i16 * speed);
        }

        // Check the strict comparison even when a late spawn lands exactly on it.
        let mut edge = actor(2, -80, -25, boundary + speed);
        edge.age = 16;
        step(level, &mut edge, &fleeing, 0);
        assert_eq!((edge.pos.x, edge.pos.y, edge.pos.z), (-80, -25, boundary));
        edge.pos.z = boundary + speed + 1;
        step(level, &mut edge, &fleeing, 0);
        assert_eq!(
            (edge.pos.x, edge.pos.y),
            (-78, -25 + if level == 0 { 1 } else { 2 })
        );
    }
}

#[test]
fn every_roster_retains_its_firing_schedule_and_depth_gate() {
    for level in 0..4 {
        for kind in 1..=if level == 3 { 2 } else { 3 } {
            for slot in [0, 1, 11] {
                let mut a = actor(kind, -70, 25, 1800);
                let mut actual = Vec::new();
                for age in 1..=220 {
                    a.age = age;
                    let fire = step(level, &mut a, &PLAYER, slot);
                    if fire != 0 {
                        actual.push((age, fire));
                    }
                }
                // These are the established first-220-tick schedules from a
                // normal spawn, including attacks suppressed by the depth gate.
                let expected: &[(u16, u8)] = match (level, kind, slot) {
                    (0, 2, 0) => &[(65, 1)],
                    (0, 2, 1) => &[(48, 1), (113, 1)],
                    (0, 2, 11) => &[(73, 1)],
                    (0, _, 0) => &[(65, 1), (130, 1)],
                    (0, _, 1) => &[(113, 1), (178, 1)],
                    (0, _, 11) => &[(73, 1), (138, 1)],
                    (1, 1, _) => &[(64, 1)],
                    (1, 2, _) => &[(64, 1), (128, 1)],
                    (1, 3, _) | (3, 1, _) => &[(64, 1), (72, 1)],
                    (2, 1, _) => &[(128, 1), (192, 1)],
                    (2, 2, _) => &[(120, 1), (128, 1)],
                    (2, 3, _) => &[(64, 2)],
                    (3, 2, _) => &[],
                    _ => unreachable!(),
                };
                assert_eq!(actual, expected, "level {level}, kind {kind}, slot {slot}");
            }
        }
    }
}
