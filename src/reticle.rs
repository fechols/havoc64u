//! Immutable high-resolution aiming dot, borrowed by sprite zero below the HUD.

pub const DISTANCE: i16 = 512;
pub const COLOR: u8 = 1;

/// One native 320x200 pixel at sprite-local (12,10), with transparent padding.
#[cfg_attr(target_arch = "mos", link_section = ".phase.boot.data")]
pub static IMAGE: [u8; 64] = {
    let mut image = [0; 64];
    image[31] = 0x08;
    image
};

/// Unscrolled sprite origin; the lit bit lands at logical bitmap cell `center`.
#[inline(always)]
pub fn vic_position(center: (i16, i16)) -> (u16, u8) {
    ((24 + center.0 * 2 - 12) as u16, (50 + center.1 - 10) as u8)
}

/// Recover the logical cell from a latched, unscrolled sprite origin.
#[inline(always)]
pub fn screen_position(origin: (u16, u8)) -> (i16, i16) {
    ((origin.0 as i16 - 12) / 2, origin.1 as i16 - 40)
}
