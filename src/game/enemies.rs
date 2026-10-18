//! Level-local enemy code. Native builds load exactly one entry at the shared
//! enemy tail address; the host selects the same implementation explicitly.
use super::Actor;
use crate::{math::sin8, Vec3};

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
        a.pos.x = a.origin_x + sin8((a.age as u8).wrapping_mul(2)) / 12;
        a.pos.y = a.origin_y + sin8((a.age as u8).wrapping_add(64)) / 24;
    } else if a.kind == 2 && a.pos.z > 300 {
        a.pos.x += (player.x - a.pos.x).clamp(-2, 2);
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
            u8::from(phase == 64)
        }
        2 => {
            a.pos.y = a.origin_y + sin8((a.age as u8).wrapping_mul(2).wrapping_add(64)) / 4;
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
            a.pos.x = a.origin_x + sin8(a.age as u8) / 12;
            u8::from(a.age & 63 == 0)
        }
        2 => {
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
        a.pos.x = a.origin_x + sin8((a.age as u8).wrapping_mul(2)) / 16;
        let phase = a.age as u8 & 127;
        u8::from(phase == 64 || phase == 72)
    } else {
        a.pos.z -= 14;
        if a.pos.z > 500 {
            a.pos.x += (player.x - a.pos.x).clamp(-2, 2);
            a.pos.y += (player.y - a.pos.y).clamp(-2, 2);
        }
        0
    };
    firing(a, fire)
}
