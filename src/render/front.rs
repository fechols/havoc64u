//! Reverse painter traversal over a monotonically shrinking set of row holes.
//! Terrain-clear coverage is collected first without committing any edge state.
//! It excludes every exterior-terrain contribution at those pixels, independent
//! of its position in the painter sequence. The first remaining reverse hit wins.

use super::{SpanStats, BAND_HEIGHT, BAND_PIXELS, TERRAIN_CLEAR_INK, WIDTH};
use crate::space_dust::SpaceDustFrame;

const DONE: u8 = WIDTH as u8;

pub(super) trait FrontCanvas {
    fn clear(&mut self, color: u8);
    fn cover(&mut self, row: usize, first: usize, end: usize);
    fn paint(&mut self, row: usize, first: usize, end: usize, color: u8);
}

impl FrontCanvas for [u8; BAND_PIXELS] {
    fn clear(&mut self, color: u8) {
        self.fill(color);
    }
    fn cover(&mut self, row: usize, first: usize, end: usize) {
        for pixel in &mut self[row * WIDTH + first..row * WIDTH + end] {
            *pixel |= TERRAIN_CLEAR_INK;
        }
    }
    fn paint(&mut self, row: usize, first: usize, end: usize, color: u8) {
        for pixel in &mut self[row * WIDTH + first..row * WIDTH + end] {
            *pixel = (*pixel & TERRAIN_CLEAR_INK) | color;
        }
    }
}

impl FrontCanvas for crate::cells::CellBand {
    fn clear(&mut self, color: u8) {
        self.clear(color);
    }
    fn cover(&mut self, row: usize, first: usize, end: usize) {
        self.mark_terrain_coverage(row, first, end);
    }
    fn paint(&mut self, row: usize, first: usize, end: usize, color: u8) {
        self.geometry_span(row, first, end, color);
    }
}

pub(super) struct FrontBand {
    next: [[u8; WIDTH]; BAND_HEIGHT],
    end: [[u8; WIDTH]; BAND_HEIGHT],
    heads: [u8; BAND_HEIGHT],
    // Native coverage is the existing CellBand low nibble; the host keeps a
    // separate reference so the same walker can also target the raw pixel band.
    cover: [[u8; WIDTH / 4]; BAND_HEIGHT],
    runs: [u16; BAND_HEIGHT],
    background: u8,
    pub(super) stats: SpanStats,
}

impl FrontBand {
    pub(super) fn new(background: u8, canvas: &mut impl FrontCanvas) -> Self {
        let background = background & 15;
        canvas.clear(background);
        let mut band = Self {
            next: [[0; WIDTH]; BAND_HEIGHT],
            end: [[0; WIDTH]; BAND_HEIGHT],
            heads: [0; BAND_HEIGHT],
            cover: [[0; WIDTH / 4]; BAND_HEIGHT],
            runs: [1; BAND_HEIGHT],
            background,
            stats: SpanStats { peak_runs: 1, ..SpanStats::default() },
        };
        for row in 0..BAND_HEIGHT {
            band.next[row][0] = DONE;
            band.end[row][0] = DONE;
        }
        band
    }

    pub(super) fn cover_span(&mut self, row: usize, first: usize, end: usize, canvas: &mut impl FrontCanvas) {
        debug_assert!(row < BAND_HEIGHT && first <= end && end <= WIDTH);
        if first == end {
            return;
        }
        self.stats.prepass_spans += 1;
        self.stats.prepass_pixels += (end - first) as u32;
        for x in first..end {
            self.cover[row][x / 4] |= 8 >> (x & 3);
        }
        canvas.cover(row, first, end);
    }

    fn protected(&self, row: usize, x: usize) -> bool {
        self.cover[row][x / 4] & (8 >> (x & 3)) != 0
    }

    fn link(&mut self, row: usize, previous: u8, next: u8) {
        if previous == DONE {
            self.heads[row] = next;
        } else {
            self.next[row][previous as usize] = next;
        }
    }

    /// Remove one accepted interval from its containing hole, returning the
    /// predecessor/current pair for continuing rightward without a head search.
    fn claim(&mut self, row: usize, previous: u8, current: u8, first: usize, end: usize) -> (u8, u8) {
        let start = current as usize;
        let old_end = self.end[row][start] as usize;
        let next = self.next[row][start];
        debug_assert!(start <= first && first < end && end <= old_end);
        if first == start {
            if end == old_end {
                self.link(row, previous, next);
                self.runs[row] -= 1;
                (previous, next)
            } else {
                self.end[row][end] = old_end as u8;
                self.next[row][end] = next;
                self.link(row, previous, end as u8);
                (previous, end as u8)
            }
        } else {
            self.end[row][start] = first as u8;
            if end == old_end {
                (current, next)
            } else {
                self.end[row][end] = old_end as u8;
                self.next[row][end] = next;
                self.next[row][start] = end as u8;
                self.runs[row] += 1;
                self.stats.splits += 1;
                (current, end as u8)
            }
        }
    }

    pub(super) fn geometry_span(&mut self, row: usize, first: usize, end: usize, flags: u8, canvas: &mut impl FrontCanvas) {
        debug_assert!(row < BAND_HEIGHT && first <= end && end <= WIDTH);
        if first == end {
            return;
        }
        self.stats.input_spans += 1;
        let exterior = flags & crate::EXTERIOR_TERRAIN != 0
            && flags & crate::TERRAIN_CLEAR_GEOMETRY == 0;
        let color = flags & 15;
        let mut previous = DONE;
        let mut current = self.heads[row];
        while current != DONE && self.end[row][current as usize] as usize <= first {
            self.stats.run_visits += 1;
            self.stats.search_visits += 1;
            previous = current;
            current = self.next[row][current as usize];
        }
        let mut accepted = false;
        while current != DONE && (current as usize) < end {
            self.stats.run_visits += 1;
            let mut left = first.max(current as usize);
            let limit = end.min(self.end[row][current as usize] as usize);
            if exterior {
                while left < limit && self.protected(row, left) {
                    left += 1;
                }
            }
            if left == limit {
                previous = current;
                current = self.next[row][current as usize];
                continue;
            }
            let mut right = left + 1;
            if exterior {
                while right < limit && !self.protected(row, right) {
                    right += 1;
                }
            } else {
                right = limit;
            }
            (previous, current) = self.claim(row, previous, current, left, right);
            self.stats.claimed_pixels += (right - left) as u32;
            self.stats.peak_runs = self.stats.peak_runs.max(self.runs[row]);
            accepted = true;
            // Clear-color geometry occludes later candidates even though its
            // ink and preseeded terrain coverage already exist in the canvas.
            if color != self.background {
                canvas.paint(row, left, right, color);
                self.stats.emitted_spans += 1;
                self.stats.emitted_pixels += (right - left) as u32;
            }
        }
        if !accepted {
            self.stats.rejected_spans += 1;
        }
    }

    pub(super) fn ambient(&mut self, band: u8, ambient: Option<&SpaceDustFrame>, canvas: &mut impl FrontCanvas) {
        if let Some(ambient) = ambient {
            for &(x, y) in ambient.positions() {
                if y >> 3 == band {
                    self.geometry_span((y & 7) as usize, x as usize, x as usize + 1, crate::space_dust::COLOR, canvas);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counted {
        pixels: [u8; BAND_PIXELS],
        writes: [bool; BAND_PIXELS],
    }
    impl FrontCanvas for Counted {
        fn clear(&mut self, color: u8) {
            self.pixels.fill(color);
            self.writes.fill(false);
        }
        fn cover(&mut self, row: usize, first: usize, end: usize) {
            self.pixels.cover(row, first, end);
        }
        fn paint(&mut self, row: usize, first: usize, end: usize, color: u8) {
            for written in &mut self.writes[row * WIDTH + first..row * WIDTH + end] {
                assert!(!*written, "overlapping final writes");
                *written = true;
            }
            self.pixels.paint(row, first, end, color);
        }
    }

    fn validate(band: &FrontBand) {
        for row in 0..8 {
            let mut current = band.heads[row];
            let mut runs = 0;
            let mut last_end = None;
            while current != DONE {
                let x = current as usize;
                let end = band.end[row][x];
                assert!(current < end && end <= DONE);
                if let Some(previous) = last_end {
                    assert!(previous < current, "holes must be separated by owned pixels");
                }
                last_end = Some(end);
                current = band.next[row][x];
                runs += 1;
            }
            assert_eq!(runs, band.runs[row]);
            assert!(runs <= 80);
        }
    }

    #[test]
    fn coverage_union_and_reverse_order_match_an_independent_pixel_oracle() {
        let mut random = 0x5e91_2df3u32;
        let mut next = || {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            random
        };
        let mut spilled_cells = 0;
        for background in 0..16 {
            let mut ops = [(0usize, 0usize, 0usize, 0u8); 512];
            let mut expected = [background; BAND_PIXELS];
            for (index, op) in ops.iter_mut().enumerate() {
                let row = (next() >> 16) as usize % 8;
                let a = (next() >> 16) as usize % 161;
                let b = (next() >> 16) as usize % 161;
                let flags = (next() >> 16) as u8 & 15 | ((index & 3) as u8) << 5;
                *op = (row, a.min(b), a.max(b), flags);
                for pixel in &mut expected[row * WIDTH + a.min(b)..row * WIDTH + a.max(b)] {
                    if flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                        *pixel = flags & 15 | TERRAIN_CLEAR_INK;
                    } else if flags & crate::EXTERIOR_TERRAIN == 0 || *pixel & TERRAIN_CLEAR_INK == 0 {
                        *pixel = flags & 15 | *pixel & TERRAIN_CLEAR_INK;
                    }
                }
            }
            let mut canvas = Counted { pixels: [0; BAND_PIXELS], writes: [false; BAND_PIXELS] };
            let mut front = FrontBand::new(background, &mut canvas);
            for &(row, first, end, flags) in &ops {
                if flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                    front.cover_span(row, first, end, &mut canvas);
                }
            }
            for &(row, first, end, flags) in ops.iter().rev() {
                front.geometry_span(row, first, end, flags, &mut canvas);
                validate(&front);
            }
            assert_eq!(canvas.pixels, expected, "background {background}");
            assert!(front.stats.claimed_pixels <= BAND_PIXELS as u32);

            // Preseeded coverage must survive a later palette overflow. Drive
            // the actual cell writer independently and compare decoded colors
            // with the flat pixel oracle under the same prior palette.
            let mut cells = crate::cells::CellBand::new();
            let mut front_cells = FrontBand::new(background, &mut cells);
            for &(row, first, end, flags) in &ops {
                if flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                    front_cells.cover_span(row, first, end, &mut cells);
                }
            }
            for &(row, first, end, flags) in ops.iter().rev() {
                front_cells.geometry_span(row, first, end, flags, &mut cells);
            }
            assert_eq!(front_cells.stats, front.stats);
            spilled_cells += cells.stats.overflow_cells;
            for (old_screen, old_color) in [([0; 40], [0; 40]), ([0x6c; 40], [5; 40])] {
                let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
                super::super::pack_band(&expected, &old_screen, &old_color, background,
                    &mut bitmap, &mut screen, &mut color);
                let decoded = crate::cells::unpack_band(&bitmap, &screen, &color, background);
                cells.finish(&old_screen, &old_color, &mut bitmap, &mut screen, &mut color);
                assert_eq!(crate::cells::unpack_band(&bitmap, &screen, &color, background), decoded);
            }
        }
        assert!(spilled_cells > 0, "the corpus must exercise coverage expansion on spill");
    }

    #[test]
    fn alternating_pixel_holes_fit_and_clear_color_takes_ownership() {
        let mut pixels = [0; BAND_PIXELS];
        let mut front = FrontBand::new(0, &mut pixels);
        for row in 0..8 {
            for x in (1..160).step_by(2) {
                front.geometry_span(row, x, x + 1, 0, &mut pixels);
            }
        }
        validate(&front);
        assert_eq!(front.runs, [80; 8]);
        assert_eq!(front.stats.peak_runs, 80);
        for row in 0..8 {
            front.geometry_span(row, 0, 160, 12, &mut pixels);
        }
        validate(&front);
        assert_eq!(front.heads, [DONE; 8]);
        for (x, &pixel) in pixels.iter().enumerate() {
            assert_eq!(pixel, if x & 1 == 0 { 12 } else { 0 });
        }
        assert_eq!(front.stats.claimed_pixels, BAND_PIXELS as u32);
    }

    #[test]
    fn preseeded_coverage_expands_exactly_when_every_cell_spills() {
        let mut cells = crate::cells::CellBand::new();
        let mut front = FrontBand::new(0, &mut cells);
        let mut expected = [0; BAND_PIXELS];
        for row in 0..8 {
            front.cover_span(row, row * 3, WIDTH - row * 2, &mut cells);
            expected.cover(row, row * 3, WIDTH - row * 2);
        }
        // Eight final colors per cell force every cell to expand. Ordinary
        // foreground preserves the coverage set by fully hidden TC geometry.
        for row in 0..8 {
            front.geometry_span(row, 0, WIDTH, row as u8 + 1, &mut cells);
            expected.paint(row, 0, WIDTH, row as u8 + 1);
        }
        assert_eq!(cells.stats.overflow_cells, 40);
        assert_eq!(cells.pixels, expected);
    }
}
