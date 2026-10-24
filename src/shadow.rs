//! Fixed silhouettes on one known horizontal plane. Scenery never participates
//! in shadow generation; the ordinary rasterizer paints a final single-ink suffix.
use crate::{render::Renderer, Vec3};

pub const CASTER_CAPACITY: usize = 64;
/// Default C64 dark gray; Space Carrier selects black for its gray deck.
pub const SHADOW_COLOR: u8 = 11;
const ZERO: Vec3 = Vec3 { x: 0, y: 0, z: 0 };

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Caster {
    pub center: Vec3,
    pub half_x: i16,
    pub half_z: i16,
    /// Low nibble: craft, hull, diamond, rectangle. Bits 4..5: quarter-turn.
    pub shape: u8,
}

/// Scratch retires before raster preparation. Native storage starts at $E800.
#[repr(C)]
pub struct ShadowFrame {
    casters: [Caster; CASTER_CAPACITY],
    caster_count: u8,
    rendered: u8,
    omitted: u8,
    base_count: u16,
    command_count: u16,
}
const _: () = assert!(core::mem::size_of::<ShadowFrame>() <= 0x400);

impl Default for ShadowFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl ShadowFrame {
    pub const fn new() -> Self {
        Self {
            casters: [Caster {
                center: ZERO,
                half_x: 0,
                half_z: 0,
                shape: 0,
            }; CASTER_CAPACITY],
            caster_count: 0,
            rendered: 0,
            omitted: 0,
            base_count: 0,
            command_count: 0,
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery_aux")]
    pub fn reset(&mut self) {
        self.caster_count = 0;
        self.rendered = 0;
        self.omitted = 0;
        self.base_count = 0;
        self.command_count = 0;
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery_aux")]
    pub fn push_caster(&mut self, caster: Caster) {
        if valid_caster(caster) && (self.caster_count as usize) < CASTER_CAPACITY {
            self.casters[self.caster_count as usize] = caster;
            self.caster_count += 1;
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow")]
    pub fn insert_caster(&mut self, index: u8, caster: Caster) -> bool {
        if !valid_caster(caster) {
            return false;
        }
        if self.caster_count as usize == CASTER_CAPACITY {
            self.omitted = self.omitted.saturating_add(1);
            return false;
        }
        let index = index.min(self.caster_count) as usize;
        for n in (index..self.caster_count as usize).rev() {
            self.casters[n + 1] = self.casters[n];
        }
        self.casters[index] = caster;
        self.caster_count += 1;
        true
    }

    #[inline(always)]
    pub fn caster_count(&self) -> u8 {
        self.caster_count
    }
    #[inline(always)]
    pub fn base_commands(&self) -> u16 {
        self.base_count
    }
    #[inline(always)]
    pub fn shadow_commands(&self) -> u16 {
        self.command_count
    }
    #[inline(always)]
    pub fn rendered_casters(&self) -> u8 {
        self.rendered
    }
    #[inline(always)]
    pub fn omitted_casters(&self) -> u8 {
        self.omitted
    }
    #[cfg(test)]
    pub(crate) fn casters(&self) -> &[Caster] {
        &self.casters[..self.caster_count as usize]
    }

    #[cfg(not(target_arch = "mos"))]
    pub fn render(&mut self, renderer: &mut Renderer, ground_y: i16, color: u8) {
        self.render_unsorted(renderer, ground_y, color);
        self.finish(renderer);
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow")]
    pub fn render_unsorted(&mut self, renderer: &mut Renderer, ground_y: i16, color: u8) {
        self.base_count = renderer.triangle_count;
        for index in 0..self.caster_count as usize {
            let caster = self.casters[index];
            if caster.center.y <= ground_y {
                continue;
            }
            let checkpoint = renderer.checkpoint();
            if renderer.remaining_commands() == 0 {
                self.omitted = self.omitted.saturating_add(1);
                continue;
            }
            let (points, len) = footprint(caster, ground_y);
            for corner in 2..len {
                renderer.append_ground_shadow_triangle(
                    points[0],
                    points[corner - 1],
                    points[corner],
                    color,
                );
                if renderer.dropped != checkpoint.dropped {
                    break;
                }
            }
            if renderer.dropped != checkpoint.dropped {
                renderer.rollback(checkpoint);
                self.omitted = self.omitted.saturating_add(1);
            } else if renderer.triangle_count != checkpoint.triangle_count {
                self.rendered += 1;
            }
        }
        self.command_count = renderer.triangle_count - self.base_count;
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    pub fn finish(&self, renderer: &mut Renderer) {
        renderer.finish_ground_shadows(self.base_count);
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    pub fn finish_without_shadows(&mut self, renderer: &mut Renderer) {
        self.base_count = renderer.triangle_count;
        self.command_count = 0;
        renderer.finish();
    }
}

/// No reference obtained here may survive raster preparation.
#[cfg(target_arch = "mos")]
#[inline(always)]
pub unsafe fn native_frame() -> &'static mut ShadowFrame {
    &mut *(0xE800 as *mut ShadowFrame)
}

#[inline(always)]
fn valid_caster(caster: Caster) -> bool {
    caster.half_x > 0
        && caster.half_x <= 4096
        && caster.half_z > 0
        && caster.half_z <= 4096
        && caster.shape & 15 >= 1
        && caster.shape & 15 <= 4
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.shadow")]
fn footprint(caster: Caster, ground_y: i16) -> ([Vec3; 6], usize) {
    let x = caster.half_x;
    let z = caster.half_z;
    // Exact old template fractions, expressed as bounded shifts and additions.
    // All half extents are positive and <=4096; the largest numerator is 36864.
    let (outline, len) = match caster.shape & 15 {
        1 => {
            let inset = (((x as u16) << 3) + x as u16) >> 5;
            let tail = (((z as u16) << 3) - z as u16) >> 3;
            (
                [
                    (0, z),
                    (x, -(z >> 2)),
                    (inset as i16, -(tail as i16)),
                    (0, -z),
                    (-(inset as i16), -(tail as i16)),
                    (-x, -(z >> 2)),
                ],
                6,
            )
        }
        2 => {
            let bow = ((x << 2) + x) >> 3;
            (
                [
                    (-bow, z),
                    (bow, z),
                    (x, z >> 1),
                    (x, -z),
                    (-x, -z),
                    (-x, z >> 1),
                ],
                6,
            )
        }
        3 => ([(0, z), (x, 0), (0, -z), (-x, 0), (0, 0), (0, 0)], 4),
        _ => ([(-x, z), (x, z), (x, -z), (-x, -z), (0, 0), (0, 0)], 4),
    };
    let mut points = [ZERO; 6];
    for index in 0..len {
        let (x, z) = outline[index];
        let (x, z) = match (caster.shape >> 4) & 3 {
            1 => (z, -x),
            2 => (-x, -z),
            3 => (-z, x),
            _ => (x, z),
        };
        points[index] = Vec3 {
            x: caster.center.x.saturating_add(x),
            y: ground_y,
            z: caster.center.z.saturating_add(z),
        };
    }
    (points, len)
}

#[cfg(test)]
mod tests;
