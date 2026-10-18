//! Fixed-point medal reveal shared by the host and native victory geometry.
use crate::{
    assets::{AssetBank, MEDAL},
    emit_mesh, math, GeometrySink, Vec3,
};

pub const REVEAL_TICKS: u16 = 400;
pub const FADE_TICKS: u16 = 100;

// Five descending VIC-II luminance steps; the final step is complete black.
// Keep this beside victory code so ordinary gameplay pays no resident cost.
#[cfg_attr(target_arch = "mos", link_section = ".phase.victory.data")]
static FADE: [[u8; 16]; 5] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [0, 15, 2, 14, 9, 9, 6, 8, 9, 11, 8, 11, 11, 5, 6, 12],
    [0, 12, 0, 9, 11, 11, 0, 9, 11, 0, 9, 0, 0, 9, 0, 11],
    [0, 11, 0, 11, 0, 0, 0, 11, 0, 0, 11, 0, 0, 11, 0, 0],
    [0, 11, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.victory")]
pub fn fade_color(color: u8, ticks: u16) -> u8 {
    if ticks >= FADE_TICKS {
        0
    } else {
        FADE[(ticks / 20) as usize][(color & 15) as usize]
    }
}

/// Move a real mesh toward the camera, retaining a safe depth at full size.
/// The fixed camera and identity mesh scale preserve the geometry cache path.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.victory")]
pub fn emit(sink: &mut impl GeometrySink, bank: &AssetBank<'_>, ticks: u16, animation: u16) {
    let remaining = REVEAL_TICKS - ticks.min(REVEAL_TICKS);
    let remaining_q8 = ((remaining as u32 * 256) / REVEAL_TICKS as u32) as i16;
    let eased = math::mul_q8(remaining_q8, remaining_q8);
    let depth = 102 + math::mul_q8(922, eased);
    let idle = ticks >= REVEAL_TICKS;
    let phase = (animation >> 2) as u8;
    let yaw = 6
        + math::mul_q8(34, eased)
        + if idle {
            math::mul_q8(2, math::sin8(phase))
        } else {
            0
        };
    let pitch = 4
        + math::mul_q8(8, eased)
        + if idle {
            math::mul_q8(2, math::sin8(phase.wrapping_add(64)))
        } else {
            0
        };
    let roll = -math::mul_q8(10, eased);
    emit_mesh(
        sink,
        bank,
        MEDAL,
        Vec3 {
            x: 0,
            y: 0,
            z: depth,
        },
        yaw as u8,
        pitch as u8,
        roll as u8,
        256,
    );
}
