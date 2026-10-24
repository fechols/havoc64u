//! Level-local enemy code. Native builds load exactly one entry at the shared
//! enemy tail address; the host selects the same implementation explicitly.
use super::Actor;
use crate::{math::sin8, Vec3};

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "host-tools"))]
#[allow(dead_code)]
#[path = "../host.rs"]
pub(super) mod authored;

#[inline(always)]
#[cfg(test)]
pub(super) fn step(level: u8, actor: &mut Actor, player: &Vec3, slot: u8) -> u8 {
    unsafe {
        match level {
            0 => enemy_lowlands(actor, player, slot),
            1 => enemy_canyon(actor, player, slot),
            2 => enemy_fortress(actor, player, slot),
            _ => enemy_carrier(actor, player, slot),
        }
    }
}

#[inline(always)]
fn firing(a: &Actor, requested: u8) -> u8 {
    if a.pos.z > 320 && a.pos.z < 1400 {
        requested
    } else {
        0
    }
}

// One copy per level stays in its owning overlay. Sharing the signed scaling
// uses the existing resident division routine instead of expanding constant
// division at every axis. Evaluate each wave before loading its origin to avoid
// keeping extra coordinates alive across the call on the target.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim")]
fn movement_wave(phase: u8, divisor: u8) -> i16 {
    sin8(phase) / i16::from(divisor)
}

// One resident copy serves the lowland, carrier and flagship overlays. Fixed
// relative bounds avoid a dynamic clamp's bounds-order check on the target.
#[inline(never)]
fn pursue_lane_x(actor: &mut Actor, target: i16) {
    let target = (target - actor.origin_x).clamp(-32, 32) + actor.origin_x;
    actor.pos.x += (target - actor.pos.x).clamp(-2, 2);
}

macro_rules! wave {
    ($name:ident, $section:literal) => {
        #[inline(always)]
        fn $name(phase: u8, divisor: u8) -> i16 {
            movement_wave(phase, divisor)
        }
    };
}
wave!(lowland_wave, ".phase.enemy.lowlands");
wave!(canyon_wave, ".phase.enemy.canyon");
wave!(fortress_wave, ".phase.enemy.fortress");
wave!(carrier_wave, ".phase.enemy.carrier");

/// Kite patrol, Needle diver and Tracked gun.
#[inline(always)]
pub(super) unsafe extern "C" fn enemy_lowlands(
    actor: *mut Actor,
    player: *const Vec3,
    slot: u8,
) -> u8 {
    let a = &mut *actor;
    let player = &*player;
    a.pos.z -= if a.kind == 2 { 12 } else { 8 };
    if a.kind == 1 {
        a.pos.x = lowland_wave((a.age as u8).wrapping_mul(2), 4) + a.origin_x;
        a.pos.y = lowland_wave((a.age as u8).wrapping_mul(4), 8) + a.origin_y;
    } else if a.kind == 2 && a.pos.z > 300 {
        // Weave the aim point inside the authored lane, retaining bounded
        // steering and its actual position when it commits to the approach.
        let offset = lowland_wave((a.age as u8).wrapping_mul(2), 8);
        pursue_lane_x(a, player.x + offset);
        a.pos.y += (player.y - a.pos.y).clamp(-1, 1);
    }
    // Preserve the teaching level's staggered, predictable firing rhythm.
    firing(a, u8::from((a.age + 17 * slot as u16) % 65 == 0))
}

/// Razor skimmer, Gorge swooper and Cliff battery.
#[inline(always)]
pub(super) unsafe extern "C" fn enemy_canyon(
    actor: *mut Actor,
    _player: *const Vec3,
    _slot: u8,
) -> u8 {
    let a = &mut *actor;
    a.pos.z -= if a.kind == 1 { 10 } else { 8 };
    let phase = a.age as u8 & 127;
    let fire = match a.kind {
        1 => {
            let travel = (a.age.min(110) * 2) as i16;
            a.pos.x = a.origin_x + if a.origin_x < 0 { travel } else { -travel };
            a.pos.y = canyon_wave((a.age as u8).wrapping_mul(2), 8) + a.origin_y;
            u8::from(phase == 64)
        }
        2 => {
            a.pos.x = canyon_wave((a.age as u8).wrapping_mul(4), 8) + a.origin_x;
            a.pos.y = canyon_wave((a.age as u8).wrapping_mul(2).wrapping_add(64), 4) + a.origin_y;
            u8::from(a.age & 63 == 0)
        }
        _ => u8::from(phase == 64 || phase == 72),
    };
    firing(a, fire)
}

/// Iron gunship, Lance sentry and Citadel bunker.
#[inline(always)]
pub(super) unsafe extern "C" fn enemy_fortress(
    actor: *mut Actor,
    _player: *const Vec3,
    _slot: u8,
) -> u8 {
    let a = &mut *actor;
    let fire = match a.kind {
        1 => {
            a.pos.z -= 6;
            a.pos.x = fortress_wave(a.age as u8, 8) + a.origin_x;
            a.pos.y = fortress_wave((a.age as u8).wrapping_add(64), 16) + a.origin_y;
            u8::from(a.age & 63 == 0)
        }
        2 => {
            // Return to the authored lane before the stationary firing hold.
            if a.age >= 48 && a.age <= 112 {
                let age = a.age as u8;
                let travel = if age <= 80 { age - 48 } else { 112 - age } as i16 * 2;
                a.pos.x = a.origin_x + if a.origin_x < 0 { travel } else { -travel };
            }
            a.pos.z -= if a.age <= 112 {
                8
            } else if a.age <= 152 {
                0
            } else {
                14
            };
            u8::from(a.age == 120 || a.age == 128)
        }
        _ => {
            a.pos.z -= 8;
            if a.age & 127 == 64 {
                2
            } else {
                0
            }
        }
    };
    firing(a, fire)
}

/// Wing guard and Sting rammer. The carrier boss remains shared simulation.
#[inline(always)]
pub(super) unsafe extern "C" fn enemy_carrier(
    actor: *mut Actor,
    player: *const Vec3,
    _slot: u8,
) -> u8 {
    let a = &mut *actor;
    let player = &*player;
    let fire = if a.kind == 1 {
        a.pos.z -= 8;
        a.pos.x = carrier_wave((a.age as u8).wrapping_mul(2), 8) + a.origin_x;
        a.pos.y = carrier_wave((a.age as u8).wrapping_mul(2).wrapping_add(64), 16) + a.origin_y;
        let phase = a.age as u8 & 127;
        u8::from(phase == 64 || phase == 72)
    } else if a.kind == 3 {
        a.pos.z -= 8;
        let phase = a.age as u8 & 127;
        u8::from(phase == 64 || phase == 72)
    } else {
        a.pos.z -= 14;
        if a.pos.z > 500 {
            let offset = carrier_wave((a.age as u8).wrapping_mul(4), 16);
            pursue_lane_x(a, player.x + offset);
            a.pos.y += (player.y - a.pos.y).clamp(-2, 2);
        }
        0
    };
    firing(a, fire)
}

/// One modulo supplies both the firing gate and alternating gun side. Keeping
/// the divisor opaque avoids expanding constant division in every boss.
/// Rates are the nonzero, bounded 25/40/45/50-tick attack intervals.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.sim_aux")]
pub(super) fn boss_pulse(ticks: u16, rate: u8) -> u8 {
    let phase = ticks % (u16::from(rate) * 2);
    if phase == 0 {
        1
    } else if phase == u16::from(rate) {
        2
    } else {
        0
    }
}

// Each native entry owns its actor loop, movement and optional act boss.
// Outlining the two specialized callbacks bounds their register live ranges;
// each still belongs to its own tail and cannot borrow another loaded image.
macro_rules! encounter {
    ($entry:ident, $section:literal, $body:literal, $enemy:ident, $boss:expr) => {
        #[no_mangle]
        #[inline(never)]
        #[cfg_attr(target_arch = "mos", link_section = $section)]
        pub unsafe extern "C" fn $entry(game: *mut (), bank: *const ()) {
            let game = &mut *game.cast::<super::Game>();
            let bank = &*bank.cast::<crate::assets::AssetBank<'_>>();
            #[inline(never)]
            #[cfg_attr(target_arch = "mos", link_section = $body)]
            unsafe extern "C" fn motion(actor: *mut Actor, player: *const Vec3, slot: u8) -> u8 {
                $enemy(actor, player, slot)
            }
            #[inline(never)]
            #[cfg_attr(target_arch = "mos", link_section = $body)]
            fn boss(game: &mut super::Game) {
                game.update_boss_for($boss);
            }
            game.update_actors_with(bank, motion);
            if $boss != 0 {
                boss(game);
            }
        }
    };
}
encounter!(
    encounter_lowlands,
    ".phase.enemy.lowlands.entry",
    ".phase.enemy.lowlands",
    enemy_lowlands,
    0
);
encounter!(
    encounter_canyon,
    ".phase.enemy.canyon.entry",
    ".phase.enemy.canyon",
    enemy_canyon,
    0
);
encounter!(
    encounter_fortress,
    ".phase.enemy.fortress.entry",
    ".phase.enemy.fortress",
    enemy_fortress,
    0
);
encounter!(
    encounter_carrier,
    ".phase.enemy.carrier.entry",
    ".phase.enemy.carrier",
    enemy_carrier,
    0
);
encounter!(
    encounter_warden,
    ".phase.enemy.warden.entry",
    ".phase.enemy.warden",
    enemy_canyon,
    1
);
encounter!(
    encounter_citadel,
    ".phase.enemy.citadel.entry",
    ".phase.enemy.citadel",
    enemy_fortress,
    2
);
encounter!(
    encounter_flagship,
    ".phase.enemy.flagship.entry",
    ".phase.enemy.flagship",
    enemy_carrier,
    3
);

#[cfg(test)]
mod pulse_tests {
    use super::boss_pulse;

    #[test]
    fn boss_pulse_preserves_firing_and_side_for_every_wrapping_clock_value() {
        for rate in [25u8, 40, 45, 50] {
            for ticks in 0..=u16::MAX {
                let expected = if ticks % u16::from(rate) != 0 {
                    0
                } else if (ticks / u16::from(rate)) & 1 == 0 {
                    1
                } else {
                    2
                };
                assert_eq!(boss_pulse(ticks, rate), expected);
            }
        }
    }
}
