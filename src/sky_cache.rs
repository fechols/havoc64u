//! Reuse packed static sky in the two physical VIC banks without extra pixels.
//!
//! A valid band contains only black/blue/white/gray static pixels and no HUD.
//! These fit the three cell slots exactly. Packing zeros absent colors and
//! assigns every present color once, even with duplicate colors in the previous
//! palette. For identical pixels, equal screen bytes therefore determine the
//! remaining color-RAM slot too (including uniform cells with screen byte zero).
//! The complete palette then determines identical bitmap codes. Repacking a
//! valid band using its own palette is idempotent, so equal front/back screen
//! bytes prove that leaving the back bitmap and current color shadow alone is
//! identical to repainting and packing against the front palette.

use crate::game::{Hud, Mode};

/// Valid static-sky bands in physical VIC banks 2 and 3; exactly eight bytes.
/// Geometry eligibility must come from `Renderer::static_sky_bands` each frame.
pub struct SkyCache {
    valid: [u32; 2],
}

impl SkyCache {
    pub const fn new() -> Self {
        Self { valid: [0; 2] }
    }

    /// Conservatively exclude every band that the playing HUD can touch.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn eligible_bands(geometry_mask: u32, hud: &Hud) -> u32 {
        if hud.mode != Mode::Playing {
            return 0;
        }
        let mut mask = geometry_mask & 0x007f_fffe; // Exclude 0, 23, 24.
        if hud.boss_shield != 0 {
            mask &= !0x6;
        }
        if crate::hud::sector_banner_visible(hud) {
            mask &= !0x78;
        }
        mask
    }

    /// Intersect with the current eligibility mask before processing bands.
    /// Both bank bits are needed: a clean front does not rule out old geometry
    /// in the back bank from two frames ago.
    #[inline(always)]
    pub fn reusable_bands(&self) -> u32 {
        self.valid[0] & self.valid[1]
    }

    /// Compare before touching this band's pixels, output, or color shadow.
    /// The caller must first check its bit in `reusable_bands` and eligibility.
    /// This comparison uses no hashes.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn screens_match(front_screen: &[u8; 40], back_screen: &[u8; 40]) -> bool {
        let mut cell = 0;
        while cell < 40 {
            if front_screen[cell] != back_screen[cell] {
                return false;
            }
            cell += 1;
        }
        true
    }

    /// Call only after all 25 back-bank bands have been painted or reused.
    /// Replacing the mask clears all nonstatic outputs to prevent stale trails.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn record_frame(&mut self, back_bank: u8, eligible: u32) {
        if !(2..=3).contains(&back_bank) {
            return;
        }
        self.valid[(back_bank - 2) as usize] = eligible & 0x01ff_ffff;
    }
}

impl Default for SkyCache {
    fn default() -> Self {
        Self::new()
    }
}
