//! Bounded painter-order span composition for the host/native A/B experiment.
//!
//! Each row is a linked list indexed by the run's first pixel. Its successor
//! is also its exclusive end; 160 terminates the list and never needs storage.
//! Only reachable nodes matter, so splitting and merging never allocate.

use super::{Canvas, BAND_HEIGHT, BAND_PIXELS, TERRAIN_CLEAR_INK, WIDTH};

/// Work performed by the host span compositor, before the existing cell packer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpanStats {
    pub input_spans: u32,
    pub run_visits: u32,
    pub emitted_spans: u32,
    pub emitted_pixels: u32,
    /// Largest number of live runs in one row after a completed input span.
    pub peak_runs: u16,
}

pub(super) struct SpanBand {
    next: [[u8; WIDTH]; BAND_HEIGHT],
    ink: [[u8; WIDTH]; BAND_HEIGHT],
    runs: [u16; BAND_HEIGHT],
    background: u8,
    pub(super) stats: SpanStats,
}

impl SpanBand {
    pub(super) fn new(background: u8) -> Self {
        let mut band = Self {
            next: [[0; WIDTH]; BAND_HEIGHT],
            ink: [[0; WIDTH]; BAND_HEIGHT],
            runs: [1; BAND_HEIGHT],
            background: background & 15,
            stats: SpanStats {
                peak_runs: 1,
                ..SpanStats::default()
            },
        };
        for row in 0..BAND_HEIGHT {
            band.next[row][0] = WIDTH as u8;
            band.ink[row][0] = band.background;
        }
        band
    }

    /// Compose geometry using exactly the classic far-to-near coverage rules.
    pub(super) fn geometry_span(&mut self, row: usize, first: usize, end: usize, flags: u8) {
        debug_assert!(row < BAND_HEIGHT && first <= end && end <= WIDTH);
        if first == end {
            return;
        }
        self.stats.input_spans += 1;
        let mut current = 0;
        let mut previous = None;
        while self.next[row][current] as usize <= first {
            self.stats.run_visits += 1;
            previous = Some(current);
            current = self.next[row][current] as usize;
        }
        if current < first {
            self.split(row, current, first);
            previous = Some(current);
            current = first;
        }
        while current < end {
            self.stats.run_visits += 1;
            if self.next[row][current] as usize > end {
                self.split(row, current, end);
            }
            let next = self.next[row][current] as usize;
            let old = self.ink[row][current];
            self.ink[row][current] = if flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                flags & 15 | TERRAIN_CLEAR_INK
            } else if flags & crate::EXTERIOR_TERRAIN != 0 && old & TERRAIN_CLEAR_INK != 0 {
                old
            } else {
                flags & 15 | old & TERRAIN_CLEAR_INK
            };
            if let Some(before) = previous {
                if self.ink[row][before] == self.ink[row][current] {
                    self.next[row][before] = next as u8;
                    self.runs[row] -= 1;
                } else {
                    previous = Some(current);
                }
            } else {
                previous = Some(current);
            }
            current = next;
        }
        if current < WIDTH {
            let before = previous.unwrap();
            if self.ink[row][before] == self.ink[row][current] {
                self.next[row][before] = self.next[row][current];
                self.runs[row] -= 1;
            }
        }
        self.stats.peak_runs = self.stats.peak_runs.max(self.runs[row]);
    }

    fn split(&mut self, row: usize, node: usize, at: usize) {
        debug_assert!(node < at && at < self.next[row][node] as usize);
        self.next[row][at] = self.next[row][node];
        self.ink[row][at] = self.ink[row][node];
        self.next[row][node] = at as u8;
        self.runs[row] += 1;
    }

    /// Emit each final non-background pixel once. Protected background ink is
    /// retained because coverage is part of the raw reference's output.
    fn emit(&mut self, mut visit: impl FnMut(usize, usize, usize, u8)) {
        self.stats.emitted_spans = 0;
        self.stats.emitted_pixels = 0;
        for row in 0..BAND_HEIGHT {
            let mut first = 0;
            while first < WIDTH {
                let end = self.next[row][first] as usize;
                let ink = self.ink[row][first];
                if ink != self.background {
                    visit(row, first, end, ink);
                    self.stats.emitted_spans += 1;
                    self.stats.emitted_pixels += (end - first) as u32;
                }
                first = end;
            }
        }
    }

    pub(super) fn write_pixels(&mut self, pixels: &mut [u8; BAND_PIXELS]) {
        pixels.fill(self.background);
        self.emit(|row, first, end, ink| pixels[row * WIDTH + first..row * WIDTH + end].fill(ink));
    }

    pub(super) fn write_cells(&mut self, cells: &mut crate::cells::CellBand) {
        cells.clear(self.background);
        self.emit(|row, first, end, ink| {
            let flags = ink & 15
                | if ink & TERRAIN_CLEAR_INK != 0 {
                    crate::TERRAIN_CLEAR_GEOMETRY
                } else {
                    0
                };
            cells.geometry_span(row, first, end, flags);
        });
    }
}

// Ambient dust uses ordinary background ink. HUD remains a later cell pass.
impl Canvas for SpanBand {
    fn span(&mut self, row: usize, first: usize, end: usize, color: u8) {
        self.geometry_span(row, first, end, color & 15);
    }

    fn plot(&mut self, row: usize, x: usize, color: u8) {
        self.span(row, x, x + 1, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(band: &SpanBand) {
        for row in 0..BAND_HEIGHT {
            let mut node = 0;
            let mut runs = 0;
            let mut previous = None;
            while node < WIDTH {
                runs += 1;
                let next = band.next[row][node] as usize;
                assert!(next > node && next <= WIDTH);
                assert_eq!(band.ink[row][node] & !0x4f, 0);
                assert_ne!(previous, Some(band.ink[row][node]), "unmerged runs");
                previous = Some(band.ink[row][node]);
                node = next;
            }
            assert_eq!(runs, band.runs[row]);
            assert!(runs <= WIDTH as u16);
        }
    }

    fn reference(pixels: &mut [u8; BAND_PIXELS], row: usize, first: usize, end: usize, flags: u8) {
        for pixel in &mut pixels[row * WIDTH + first..row * WIDTH + end] {
            if flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                *pixel = flags & 15 | TERRAIN_CLEAR_INK;
            } else if flags & crate::EXTERIOR_TERRAIN == 0 || *pixel & TERRAIN_CLEAR_INK == 0 {
                *pixel = flags & 15 | *pixel & TERRAIN_CLEAR_INK;
            }
        }
    }

    #[test]
    fn randomized_dense_overlap_matches_each_pixel_for_every_flag_combination() {
        let mut random = 0x71a9_e21bu32;
        let mut next = || {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            random
        };
        for background in 0..16 {
            let mut band = SpanBand::new(background);
            let mut expected = [background; BAND_PIXELS];
            for step in 0..2048 {
                let row = (next() >> 16) as usize % BAND_HEIGHT;
                let a = (next() >> 16) as usize % (WIDTH + 1);
                let b = (next() >> 16) as usize % (WIDTH + 1);
                let flags = (next() >> 16) as u8 & 15 | ((step & 3) as u8) << 5;
                let (first, end) = (a.min(b), a.max(b));
                band.geometry_span(row, first, end, flags);
                reference(&mut expected, row, first, end, flags);
                validate(&band);
                let mut actual = [0; BAND_PIXELS];
                band.write_pixels(&mut actual);
                assert_eq!(actual, expected, "background {background}, step {step}");
                assert!(band.stats.emitted_pixels <= BAND_PIXELS as u32);
            }
        }
    }

    #[test]
    fn all_160_runs_fit_and_full_span_merges_them_without_duplicate_emission() {
        let mut band = SpanBand::new(6);
        for row in 0..BAND_HEIGHT {
            for x in 0..WIDTH {
                band.geometry_span(row, x, x + 1, (x & 1) as u8);
            }
        }
        validate(&band);
        assert_eq!(band.runs, [WIDTH as u16; BAND_HEIGHT]);
        assert_eq!(band.stats.peak_runs, 160);
        let mut seen = [false; BAND_PIXELS];
        band.emit(|row, first, end, _| {
            for pixel in &mut seen[row * WIDTH + first..row * WIDTH + end] {
                assert!(!*pixel, "a final pixel was emitted twice");
                *pixel = true;
            }
        });
        assert!(seen.iter().all(|&pixel| pixel));
        assert_eq!(band.stats.emitted_spans, 1280);
        assert_eq!(band.stats.emitted_pixels, 1280);
        for row in 0..BAND_HEIGHT {
            band.geometry_span(row, 0, WIDTH, 9);
        }
        validate(&band);
        assert_eq!(band.runs, [1; BAND_HEIGHT]);
        band.emit(|_, _, _, _| {});
        assert_eq!(band.stats.emitted_spans, 8);
    }

    #[test]
    fn boundaries_clear_ink_and_protected_runs_keep_exact_coverage() {
        let mut band = SpanBand::new(0);
        let mut expected = [0; BAND_PIXELS];
        for (first, end, flags) in [
            (0, 160, 3), (0, 1, 2), (159, 160, 4), (1, 159, 5),
            (17, 143, 0x20), (20, 140, 8), (0, 160, 0x45),
            (80, 81, 0x67), (80, 80, 0), (160, 160, 0),
        ] {
            band.geometry_span(7, first, end, flags);
            reference(&mut expected, 7, first, end, flags);
            validate(&band);
        }
        let mut actual = [0; BAND_PIXELS];
        band.write_pixels(&mut actual);
        assert_eq!(actual, expected);
        assert_eq!(actual[7 * WIDTH + 17], TERRAIN_CLEAR_INK);
        assert_eq!(actual[7 * WIDTH + 20], 8 | TERRAIN_CLEAR_INK);
        assert_eq!(actual[7 * WIDTH + 80], 7 | TERRAIN_CLEAR_INK);
    }

    #[test]
    fn full_triangle_capacity_dust_and_hidden_bands_match_the_classic_reference() {
        use super::super::{Renderer, Triangle, POSITIVE_AREA, TRIANGLE_CAPACITY};
        use crate::{game::Mode, space_dust::SpaceDustFrame};
        let mut random = 0x329a_fae1u32;
        let mut next = || {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            random
        };
        for count in [0, 1, 17, TRIANGLE_CAPACITY] {
            let mut renderer = Renderer::new();
            renderer.begin(6);
            for index in 0..count {
                let mut triangle = Triangle::EMPTY;
                for corner in 0..3 {
                    triangle.x[corner] = (next() % 400) as i16 - 120;
                    triangle.y[corner] = (next() % 400) as i16 - 100;
                }
                let area = (triangle.x[1] as i32 - triangle.x[0] as i32)
                    * (triangle.y[2] as i32 - triangle.y[0] as i32)
                    - (triangle.y[1] as i32 - triangle.y[0] as i32)
                        * (triangle.x[2] as i32 - triangle.x[0] as i32);
                triangle.reserved = ((index & 3) as u8) << 5
                    | if area > 0 { POSITIVE_AREA } else { 0 };
                triangle.color = (next() & 15) as u8;
                triangle.depth = (next() & 3) as i16;
                renderer.triangles[index] = triangle;
            }
            renderer.triangle_count = count as u16;
            renderer.finish();
            let mut dust = SpaceDustFrame::EMPTY;
            let ambient = dust.prepare(12, Mode::Playing, 49);
            let mut expected = [[0; BAND_PIXELS]; 25];
            for band in 0..25 {
                renderer.render_band_with_ambient(band, &mut expected[band as usize], ambient);
                let mut actual = [0; BAND_PIXELS];
                let stats = renderer.render_band_spans(band, &mut actual, ambient);
                assert_eq!(actual, expected[band as usize], "count {count}, band {band}");
                assert!(stats.emitted_pixels <= BAND_PIXELS as u32);
                assert!(stats.peak_runs <= WIDTH as u16);
            }
            for band in [25, 255] {
                let mut actual = [0; BAND_PIXELS];
                assert_eq!(renderer.render_band_spans(band, &mut actual, ambient).input_spans, 0);
                assert_eq!(actual, [6; BAND_PIXELS]);
            }
            let mut pass = renderer.raster_pass().unwrap();
            for band in 0..25 {
                let paint = band % 3 != 1;
                let mut actual = [0xa5; BAND_PIXELS];
                assert_eq!(pass.next_band_with_ambient(&mut actual, paint, ambient), Some(band));
                assert_eq!(actual, if paint { expected[band as usize] } else { [0xa5; BAND_PIXELS] });
            }
            assert!(pass.span_stats().emitted_pixels <= 25 * BAND_PIXELS as u32);
        }
    }

    #[test]
    fn opaque_black_geometry_hides_dust_before_final_span_emission() {
        let mut band = SpanBand::new(0);
        band.plot(0, 80, 12);
        band.geometry_span(0, 79, 82, 0);
        let mut pixels = [1; BAND_PIXELS];
        band.write_pixels(&mut pixels);
        assert_eq!(pixels, [0; BAND_PIXELS]);
        assert_eq!(band.stats.emitted_pixels, 0);
    }
}
