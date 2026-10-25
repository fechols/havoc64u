//! Exact, allocation-free samples of the player's prepared raster coverage.
use super::{RasterHead, RasterTail};

/// At most three points: the next aim and the active/pending prior sprite.
/// Duplicate positions share an evaluation. Results retain the caller's slots.
#[repr(C)]
pub struct PlayerPixelProbe {
    points: [(u8, u8); 3],
    aliases: [u8; 3],
    count: u8,
    covered: u8,
}

impl PlayerPixelProbe {
    pub const EMPTY: Self = Self {
        points: [(0, 0); 3],
        aliases: [0; 3],
        count: 0,
        covered: 0,
    };

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    pub fn new(points: [Option<(i16, i16)>; 3]) -> Self {
        let mut probe = Self::EMPTY;
        for (slot, point) in points.iter().enumerate() {
            if let Some((x, y)) = *point {
                if !(0..160).contains(&x) || !(0..200).contains(&y) {
                    continue;
                }
                let point = (x as u8, y as u8);
                let mut index = 0;
                while index < probe.count && probe.points[index as usize] != point {
                    index += 1;
                }
                probe.points[index as usize] = point;
                probe.aliases[index as usize] |= 1 << slot;
                if index == probe.count {
                    probe.count += 1;
                }
            }
        }
        probe
    }

    #[inline(always)]
    pub fn covered(&self, slot: u8) -> bool {
        self.covered & (1 << slot) != 0
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    pub(super) fn triangle(&mut self, head: &RasterHead, tail: &RasterTail) {
        for i in 0..self.count as usize {
            if self.covered & self.aliases[i] != 0 {
                continue;
            }
            let (x, y) = self.points[i];
            if y < tail.top || y >= tail.bottom {
                continue;
            }
            let long = advance(head.long_x, head.long_step, y - tail.top);
            let short = if y < tail.middle {
                advance(head.short_x, head.short_step, y - tail.top)
            } else {
                advance(tail.lower_x, tail.lower_step, y - tail.middle)
            };
            let (left, right) = if tail.color_flags & 16 != 0 {
                (long, short)
            } else {
                (short, long)
            };
            // The target preparation pre-biases positions; host heads are raw.
            let bias = if cfg!(target_arch = "mos") { 0 } else { 32767 };
            let first = left.wrapping_add(bias) >> 16;
            let end = right.wrapping_add(bias) >> 16;
            // x is in bounds, so comparing unclamped endpoints is equivalent.
            if first <= x as i32 && (x as i32) < end {
                self.covered |= self.aliases[i];
            }
        }
    }
}

/// Exact signed Q16 step times a byte row offset. Bounded eight-bit work;
/// avoids the target's general 32x32 multiply and does not alter raster heads.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
fn advance(mut start: i32, mut step: i32, mut rows: u8) -> i32 {
    while rows != 0 {
        if rows & 1 != 0 {
            start = start.wrapping_add(step);
        }
        rows >>= 1;
        step = step.wrapping_shl(1);
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{prepare_triangle, raster_rows, seek_row, Renderer, Triangle, BAND_PIXELS};
    use crate::{GeometrySink, Vec3};

    #[test]
    fn probes_match_actual_spans_at_every_edge_and_middle_row() {
        let fixtures = [
            ([20, 100, 35], [30, 60, 90]),
            ([20, 100, 35], [30, 30, 90]),
            ([20, 100, 35], [30, 90, 90]),
            ([-15, 180, 50], [-12, 11, 210]),
            ([0, 160, 159], [0, 1, 200]),
            ([79, 80, 81], [100, 199, 100]),
            ([20, 21, 23], [100, 99, 98]),
        ];
        for (mut x, mut y) in fixtures {
            let area = (x[1] - x[0]) as i32 * (y[2] - y[0]) as i32
                - (y[1] - y[0]) as i32 * (x[2] - x[0]) as i32;
            if area == 0 {
                continue;
            }
            if area < 0 {
                x.swap(1, 2);
                y.swap(1, 2);
            }
            let triangle = Triangle {
                x,
                y,
                depth: 180,
                color: 0,
                reserved: crate::PLAYER_GEOMETRY,
            };
            let (head, tail) = prepare_triangle(&triangle);
            for band in 0..25 {
                let (mut walked, _) = prepare_triangle(&triangle);
                seek_row(&mut walked, &tail, band * 8);
                let mut pixels = [false; BAND_PIXELS];
                raster_rows(&mut walked, &tail, band * 8, |row, first, end, _| {
                    pixels[row * 160 + first..row * 160 + end].fill(true);
                });
                for row in 0..8 {
                    for column in (0..160).step_by(3) {
                        let points = core::array::from_fn(|i| {
                            Some(((column + i) as i16, (band as usize * 8 + row) as i16))
                        });
                        let mut probe = PlayerPixelProbe::new(points);
                        probe.triangle(&head, &tail);
                        for i in 0..3 {
                            let expected = column + i < 160 && pixels[row * 160 + column + i];
                            assert_eq!(
                                probe.covered(i as u8), expected,
                                "{x:?} {y:?} at {:?}", points[i]
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn duplicate_points_alias_results_and_outside_points_do_not_hit() {
        let triangle = Triangle {
            x: [0, 160, 0],
            y: [0, 0, 200],
            depth: 180,
            color: 0,
            reserved: crate::PLAYER_GEOMETRY,
        };
        let (head, tail) = prepare_triangle(&triangle);
        let mut probe = PlayerPixelProbe::new([Some((1, 1)), Some((1, 1)), Some((-1, 1))]);
        assert_eq!(probe.count, 1);
        probe.triangle(&head, &tail);
        assert!(probe.covered(0) && probe.covered(1) && !probe.covered(2));
        let mut probe = PlayerPixelProbe::new([Some((160, 0)), Some((0, 200)), None]);
        probe.triangle(&head, &tail);
        assert_eq!(probe.covered, 0);
    }

    #[test]
    fn player_identity_survives_sorting_but_rolled_back_and_unrelated_faces_do_not_occlude() {
        let mut renderer = Renderer::new();
        let points = [
            Vec3 { x: -50, y: 50, z: 180 },
            Vec3 { x: 50, y: 50, z: 180 },
            Vec3 { x: 0, y: -50, z: 180 },
        ];
        let aim = Some((80, 100));
        // Backface winding for camera coordinates is clockwise after projection.
        renderer.triangle(
            points[0], points[1], points[2], 1, crate::TERRAIN_CLEAR_GEOMETRY,
        );
        let checkpoint = renderer.checkpoint();
        renderer.triangle(points[0], points[1], points[2], 0, crate::PLAYER_GEOMETRY);
        let mut probe = PlayerPixelProbe::new([aim, aim, aim]);
        renderer.probe_player_pixels(&mut probe);
        assert_eq!(probe.covered, 7, "black player facets are opaque");
        renderer.rollback(checkpoint);
        renderer.finish();
        let mut probe = PlayerPixelProbe::new([aim, None, None]);
        renderer.probe_player_pixels(&mut probe);
        assert!(!probe.covered(0), "terrain-protected enemies cannot hide the dot");
        renderer.begin(0);
        renderer.triangle(points[0], points[1], points[2], 0, crate::PLAYER_GEOMETRY);
        renderer.finish();
        let mut probe = PlayerPixelProbe::new([aim, None, None]);
        renderer.probe_player_pixels(&mut probe);
        assert!(probe.covered(0));
    }
}
