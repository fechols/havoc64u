//! Harmless, one-pixel forward-motion cues behind space-scene geometry.

use crate::game::Mode;

pub const SPECK_COUNT: usize = 8;
pub const COLOR: u8 = 11;

/// Prepared once per frame. Offscreen positions have y=255, outside all bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SpaceDustFrame {
    positions: [(u8, u8); SPECK_COUNT],
}

impl SpaceDustFrame {
    pub const EMPTY: Self = Self {
        positions: [(0, 255); SPECK_COUNT],
    };

    /// The shared 50 Hz clock continues across crossings and wraps naturally.
    /// These particles are screen-space background ink, never game actors.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels_aux")]
    pub fn prepare(&mut self, stage: u8, mode: Mode, ticks: u16) -> Option<&Self> {
        if !(12..18).contains(&stage)
            || !matches!(
                mode,
                Mode::Playing | Mode::Transition | Mode::Attract | Mode::Respawning
            )
        {
            return None;
        }
        let mut shifted = ticks;
        let mut salt = 0xa7u8;
        for index in 0..SPECK_COUNT {
            let phase = shifted as u8;
            // Derive a new heading only when this speck respawns. Keeping the
            // phase carry in the full clock also preserves other live specks
            // across u16 wrap. No state survives the native overlay reload.
            let mut ray = ((shifted >> 8) as u8).wrapping_add(salt);
            ray ^= ray << 3;
            ray ^= ray >> 5;
            ray ^= ray << 1;
            // A quadratic radius accelerates outward without growing the dot.
            // Maximum product 255² fits u16; all rays exit before wrapping.
            let radius = ((phase as u16 * phase as u16) >> 9) as u8;
            // Quarter, half and three-quarter slopes in eight octants give
            // 24 outward headings without tables or another multiplication.
            let minor = if ray & 8 == 0 { radius >> 2 } else { radius >> 1 };
            let minor = if ray & 16 == 0 { minor } else { radius - minor };
            let (dx, dy) = if ray & 1 == 0 {
                (radius, minor)
            } else {
                (minor, radius)
            };
            let x = if ray & 2 == 0 {
                80u8.wrapping_add(dx)
            } else {
                80u8.wrapping_sub(dx)
            };
            let y = if ray & 4 == 0 {
                100u8.wrapping_add(dy)
            } else {
                100u8.wrapping_sub(dy)
            };
            self.positions[index] = (x, if x < 160 && y < 200 { y } else { 255 });
            shifted = shifted.wrapping_add(32);
            salt = salt.wrapping_add(29);
        }
        Some(self)
    }

    #[inline(always)]
    pub fn positions(&self) -> &[(u8, u8); SPECK_COUNT] {
        &self.positions
    }

    /// Called after clearing, before triangles, so even black geometry occludes.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels_aux")]
    pub(crate) fn draw_band(&self, band: u8, canvas: &mut impl crate::render::Canvas) {
        for &(x, y) in &self.positions {
            if y >> 3 == band {
                // Only prepare() can create positions. A matching valid band
                // proves y<200 and x<160; one plot is exactly one logical pixel.
                canvas.plot((y & 7) as usize, x as usize, COLOR);
            }
        }
    }
}

const _: [(); 16] = [(); core::mem::size_of::<SpaceDustFrame>()];
