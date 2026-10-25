//! The player's assisted flight model: havoc2's `fly_player` + `Aircraft::step`
//! behaviour in Q8 fixed point at the 50 Hz simulation tick.
//!
//! The stick sets a target velocity; the velocity follows it with a first-order
//! lag, a soft corridor slows the ship exponentially into the arena edges, the
//! hull banks with its lateral acceleration (coordinated and transient, like a
//! real turn) and pitches with the commanded climb. The native body is
//! `fast_flight_step` (target-c64/asm/flight.S); `step_reference` is the
//! host's bit-exact reference and the kernel harness's oracle.
use crate::Vec3;

/// Target speed for a held stick, Q8 units per tick (3.0; havoc2 160 u/s).
pub const V_MAX_X: i16 = 768;
/// Vertical target speed, Q8 units per tick (2.75; havoc2 135 u/s).
pub const V_MAX_Y: i16 = 704;
/// Afterburner steering targets: three halves of the ordinary speeds.
pub const V_BOOST_X: i16 = 1152;
pub const V_BOOST_Y: i16 = 1056;
/// Sideways push while the barrel roll spins (2.5; havoc2 110 u/s).
pub const V_PUSH: i16 = 640;
/// Arena half extents, Q8 (the former hard clamps ±100 and ±65).
pub const EDGE_X: i16 = 100 * 256;
pub const EDGE_Y: i16 = 65 * 256;
/// Bank saturates at ±20/256 of a turn (28°; havoc2 ±0.48 rad).
pub const BANK_MAX: i16 = 20 * 256;

/// Fractional position, velocity and attitude, all Q8; declared directly after
/// `Game::player`, which keeps the integer position every other system reads.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Flight {
    /// Low bytes of the Q8 x and y positions; `player` holds the high bytes.
    pub frac: [u8; 2],
    /// Q8 units per tick.
    pub vel: [i16; 2],
    /// Roll angle, Q8 of 256ths of a turn; the high byte feeds the mesh.
    pub bank: i16,
    /// Pitch angle, same scale; negative is nose up.
    pub pitch: i16,
}

impl Flight {
    pub const ZERO: Self = Self {
        frac: [0; 2],
        vel: [0; 2],
        bank: 0,
        pitch: 0,
    };
}

/// One tick: `bits` is the live stick plus BOOST while the afterburner burst is
/// active; `push` is the barrel roll's direction while it spins (else 0).
/// Updates `player.x/y` and `flight`.
#[inline(always)]
pub fn step(player: &mut Vec3, flight: &mut Flight, bits: u8, push: i8) {
    #[cfg(target_arch = "mos")]
    unsafe {
        extern "C" {
            fn fast_flight_step(bits: u8, push: i8, player: *mut Vec3, flight: *mut Flight);
        }
        // The kernel implements this C ABI with caller-saved registers only.
        fast_flight_step(bits, push, player, flight);
    }
    #[cfg(not(target_arch = "mos"))]
    step_reference(player, flight, bits, push);
}

/// The error's arithmetic shift, computed on its magnitude so both directions
/// respond alike, and at least one unit while any error remains, so every
/// lagged state settles exactly on its target instead of drifting below it.
#[cfg(not(target_arch = "mos"))]
fn delta(error: i16, shift: u32) -> i16 {
    let magnitude = error.unsigned_abs() >> shift;
    let magnitude = if magnitude == 0 && error != 0 {
        1
    } else {
        magnitude
    };
    if error < 0 {
        (magnitude as i16).wrapping_neg()
    } else {
        magnitude as i16
    }
}

/// havoc2's `(edge − p) × 6/s` velocity clamp: the ship decelerates
/// exponentially into the wall and reaches it exactly without crossing it.
/// Only the near side is tested, so the difference always fits i16.
#[cfg(not(target_arch = "mos"))]
fn wall(velocity: i16, position: i16, edge: i16) -> i16 {
    if position >= 0 {
        velocity.min((edge.wrapping_sub(position).wrapping_add(7)) >> 3)
    } else {
        velocity.max(-((edge.wrapping_add(position).wrapping_add(7)) >> 3))
    }
}

/// One axis: the lagged velocity, the wall and the Q8 position; returns the
/// velocity change.
#[cfg(not(target_arch = "mos"))]
fn axis(position: &mut i16, frac: &mut u8, velocity: &mut i16, target: i16, edge: i16) -> i16 {
    let before = *velocity;
    // 3/32 of the error per tick: a 213 ms time constant (havoc2's 263 ms).
    // `d / 2` truncates toward zero, keeping the two directions mirrored.
    let d = delta(target.wrapping_sub(before), 4);
    let lagged = before.wrapping_add(d).wrapping_add(d / 2);
    let q = ((*position as u16) << 8 | *frac as u16) as i16;
    let v = wall(lagged, q, edge);
    let q = q.wrapping_add(v);
    *position = q >> 8;
    *frac = q as u8;
    *velocity = v;
    v.wrapping_sub(before)
}

/// Bit-exact host reference of `fast_flight_step`.
#[cfg(not(target_arch = "mos"))]
pub fn step_reference(player: &mut Vec3, flight: &mut Flight, bits: u8, push: i8) {
    use super::{BOOST, DOWN, LEFT, RIGHT, UP};
    let dx = (bits & RIGHT != 0) as i16 - (bits & LEFT != 0) as i16;
    let dy = (bits & UP != 0) as i16 - (bits & DOWN != 0) as i16;
    let boosting = bits & BOOST != 0;
    let target_x = dx * if boosting { V_BOOST_X } else { V_MAX_X } + push as i16 * V_PUSH;
    let target_y = dy * if boosting { V_BOOST_Y } else { V_MAX_Y };
    let dvx = axis(
        &mut player.x,
        &mut flight.frac[0],
        &mut flight.vel[0],
        target_x,
        EDGE_X,
    );
    axis(
        &mut player.y,
        &mut flight.frac[1],
        &mut flight.vel[1],
        target_y,
        EDGE_Y,
    );
    // Coordinated bank: −16 × the lateral acceleration (units per tick²),
    // saturating at BANK_MAX; the single unit of a settling velocity
    // commands none. The attitude follows with a 160 ms lag.
    let bank_target = if (-1..=1).contains(&dvx) {
        0
    } else {
        dvx.wrapping_neg().clamp(-20, 20) << 8
    };
    flight.bank = flight
        .bank
        .wrapping_add(delta(bank_target.wrapping_sub(flight.bank), 3));
    // Extra thrust changes speed without increasing the hull's climb angle.
    let pitch_target = -((dy * V_MAX_Y) << 2);
    flight.pitch = flight
        .pitch
        .wrapping_add(delta(pitch_target.wrapping_sub(flight.pitch), 3));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{BOOST, DOWN, LEFT, RIGHT, UP};

    fn fly(player: &mut Vec3, flight: &mut Flight, inputs: impl IntoIterator<Item = (u8, i8)>) {
        for (bits, push) in inputs {
            step_reference(player, flight, bits, push);
            assert!((-100..=100).contains(&player.x), "{player:?} {flight:?}");
            assert!((-65..=65).contains(&player.y), "{player:?} {flight:?}");
            assert!((-20 * 256..=20 * 256).contains(&flight.bank));
            assert!((-11 * 256..=11 * 256).contains(&flight.pitch));
        }
    }

    #[test]
    fn random_flying_never_leaves_the_arena_and_settles_exactly_when_released() {
        let mut state = 0x2545_F491u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 8
        };
        let mut player = Vec3 { x: 0, y: 0, z: 180 };
        let mut flight = Flight::ZERO;
        for _ in 0..40 {
            // Held gestures of random length, including rolls into the walls.
            let bits = (next() & (15 | BOOST as u32)) as u8;
            let push = [0i8, 1, -1, 0][(next() % 4) as usize];
            let length = 1 + next() % 90;
            fly(&mut player, &mut flight, (0..length).map(|_| (bits, push)));
        }
        fly(&mut player, &mut flight, (0..200).map(|_| (0, 0)));
        assert_eq!((flight.vel, flight.bank, flight.pitch), ([0, 0], 0, 0));
    }

    #[test]
    fn mirrored_sticks_fly_mirrored_courses_within_a_unit() {
        let mut a = (Vec3 { x: 0, y: 0, z: 180 }, Flight::ZERO);
        let mut b = a;
        for tick in 0..300 {
            let (bits, mirror) = match tick {
                0..=59 => (RIGHT | UP, LEFT | DOWN),
                60..=99 => (0, 0),
                100..=199 => (LEFT, RIGHT),
                _ => (UP, DOWN),
            };
            let push = if (10..35).contains(&tick) { 1 } else { 0 };
            fly(&mut a.0, &mut a.1, [(bits, push)]);
            fly(&mut b.0, &mut b.1, [(mirror, -push)]);
            assert!(
                (a.0.x + b.0.x).abs() <= 1,
                "tick {tick}: {:?} {:?}",
                a.0,
                b.0
            );
            assert!(
                (a.0.y + b.0.y).abs() <= 1,
                "tick {tick}: {:?} {:?}",
                a.0,
                b.0
            );
            assert!((a.1.bank + b.1.bank).abs() <= 3 * 256);
            assert!((a.1.pitch + b.1.pitch).abs() <= 3 * 256);
        }
    }

    #[test]
    fn a_held_stick_banks_into_the_turn_then_levels_and_banks_back_when_released() {
        let mut player = Vec3 { x: 0, y: 0, z: 180 };
        let mut flight = Flight::ZERO;
        let mut peak = 0;
        for _ in 0..30 {
            step_reference(&mut player, &mut flight, RIGHT, 0);
            peak = peak.min(flight.bank);
        }
        assert!(peak <= -15 * 256, "peak bank {peak}");
        assert!(
            flight.bank > peak && flight.bank < 0,
            "levels out while sliding"
        );
        assert!(flight.vel[0] > 700);
        for _ in 0..12 {
            step_reference(&mut player, &mut flight, 0, 0);
        }
        assert!(flight.bank > 8 * 256, "banks the other way while braking");
        assert_eq!(flight.pitch, 0);
    }

    #[test]
    fn boost_accelerates_both_axes_and_releasing_it_smoothly_returns_to_cruise() {
        let mut normal = (Vec3 { x: 0, y: 0, z: 180 }, Flight::ZERO);
        let mut boosted = normal;
        for _ in 0..10 {
            step_reference(&mut normal.0, &mut normal.1, RIGHT | UP, 0);
            step_reference(&mut boosted.0, &mut boosted.1, RIGHT | UP | BOOST, 0);
        }
        for axis in 0..2 {
            // Fixed-point rounding is tiny compared with the 50% speed gain.
            assert!((boosted.1.vel[axis] * 2 - normal.1.vel[axis] * 3).abs() <= 16);
        }
        assert!(boosted.0.x > normal.0.x && boosted.0.y > normal.0.y);
        assert_eq!(boosted.1.pitch, normal.1.pitch);

        let mut player = Vec3 {
            x: -90,
            y: 0,
            z: 180,
        };
        let mut flight = Flight {
            vel: [V_BOOST_X, 0],
            ..Flight::ZERO
        };
        step_reference(&mut player, &mut flight, RIGHT, 0);
        assert!(flight.vel[0] > V_MAX_X && flight.vel[0] < V_BOOST_X);
        fly(&mut player, &mut flight, (0..200).map(|_| (0, 0)));
        assert_eq!(flight.vel, [0, 0]);
    }

    #[test]
    fn boost_preserves_pitch_limits_and_roll_push_without_a_stick_command() {
        for bits in [UP, DOWN] {
            let mut normal = (Vec3 { x: 0, y: 0, z: 180 }, Flight::ZERO);
            let mut boosted = normal;
            for _ in 0..200 {
                fly(&mut normal.0, &mut normal.1, [(bits, 0)]);
                fly(&mut boosted.0, &mut boosted.1, [(bits | BOOST, 0)]);
                assert_eq!(boosted.1.pitch, normal.1.pitch);
            }
            assert_eq!(boosted.1.pitch.abs(), 11 * 256);
        }
        for push in [-1, 0, 1] {
            let mut normal = (Vec3 { x: 0, y: 0, z: 180 }, Flight::ZERO);
            let mut boosted = normal;
            fly(&mut normal.0, &mut normal.1, (0..50).map(|_| (0, push)));
            fly(&mut boosted.0, &mut boosted.1, (0..50).map(|_| (BOOST, push)));
            assert_eq!(normal, boosted);
        }
    }
}
