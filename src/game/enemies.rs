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

#[cfg(target_arch = "mos")]
extern "C" {
    fn enemy_step(actor: *mut Actor, player: *const Vec3, slot: u8) -> u8;
}

#[inline(always)]
pub(super) fn step(level: u8, actor: &mut Actor, player: &Vec3, slot: u8) -> u8 {
    #[cfg(target_arch = "mos")]
    {
        let _ = level;
        // The platform installs the matching level tail before entering SIM.
        unsafe { enemy_step(actor, player, slot) }
    }
    #[cfg(not(target_arch = "mos"))]
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
macro_rules! wave {
    ($name:ident, $section:literal) => {
        #[inline(never)]
        #[cfg_attr(target_arch = "mos", link_section = $section)]
        fn $name(phase: u8, divisor: u8) -> i16 {
            sin8(phase) / i16::from(divisor)
        }
    };
}
wave!(lowland_wave, ".phase.enemy.lowlands");
wave!(canyon_wave, ".phase.enemy.canyon");
wave!(fortress_wave, ".phase.enemy.fortress");
wave!(carrier_wave, ".phase.enemy.carrier");

/// Kite patrol, Needle diver and Tracked gun.
#[no_mangle]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.enemy.lowlands.entry")]
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
        // Weave the aim point, retaining the diver's bounded steering and
        // its actual position when it commits to the final approach.
        let offset = lowland_wave((a.age as u8).wrapping_mul(2), 8);
        a.pos.x += (player.x + offset - a.pos.x).clamp(-2, 2);
        a.pos.y += (player.y - a.pos.y).clamp(-1, 1);
    }
    // Preserve the teaching level's staggered, predictable firing rhythm.
    firing(a, u8::from((a.age + 17 * slot as u16) % 65 == 0))
}

/// Razor skimmer, Gorge swooper and Cliff battery.
#[no_mangle]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.enemy.canyon.entry")]
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
#[no_mangle]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.enemy.fortress.entry")]
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
#[no_mangle]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.enemy.carrier.entry")]
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
    } else {
        a.pos.z -= 14;
        if a.pos.z > 500 {
            let offset = carrier_wave((a.age as u8).wrapping_mul(4), 16);
            a.pos.x += (player.x + offset - a.pos.x).clamp(-2, 2);
            a.pos.y += (player.y - a.pos.y).clamp(-2, 2);
        }
        0
    };
    firing(a, fire)
}
