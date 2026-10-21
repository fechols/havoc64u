//! Cell-direct band rasterization.
//!
//! The VIC-II multicolor bitmap gives every 4x8 cell the global background
//! plus three colors of its own. The background register shows the frame's
//! clear colour, so a cell's unpainted pixels and any ink of that colour are
//! code 0 and need no slot. Instead of painting one byte per pixel and
//! quantizing each cell afterwards, spans write two-bit color codes straight
//! into per-cell bitmap bytes while a per-cell palette assigns the three
//! slots on first use. Only a cell that is asked for a fourth colour besides
//! the background (counting colors later hidden by overdraw) falls back to
//! the pixel form and the per-pixel quantizer, so its result is byte-identical
//! to the classic pack; every other cell is pixel-identical and skips the pack
//! entirely. This module is the host reference; the native kernels keep the
//! same state layout: a slot stores its colour XOR the clear colour (the
//! "key"), so the clear colour is key 0 like an empty slot and the kernels'
//! zero tests need no comparison with a variable.

use crate::render::{BAND_PIXELS, HUD_INK, WIDTH};

/// Cell row codes: bit 7..6 is the leftmost pixel. Slot k paints `PATTERN[k]`.
const PATTERN: [u8; 4] = [0x00, 0x55, 0xaa, 0xff];
/// Expand a four-bit pixel mask (bit 3 = leftmost pixel) to its code-byte mask.
const fn expand(mask: u8) -> u8 {
    (if mask & 8 != 0 { 0xc0 } else { 0 })
        | (if mask & 4 != 0 { 0x30 } else { 0 })
        | (if mask & 2 != 0 { 0x0c } else { 0 })
        | (if mask & 1 != 0 { 0x03 } else { 0 })
}
const EXPAND: [u8; 16] = {
    let mut table = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        table[i] = expand(i as u8);
        i += 1;
    }
    table
};
/// Bit k set when code k occurs in a code byte (which slots a row still uses).
const CODES_PRESENT: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut byte = 0;
    while byte < 256 {
        table[byte] = (1 << (byte >> 6)) | (1 << ((byte >> 4) & 3)) | (1 << ((byte >> 2) & 3)) | (1 << (byte & 3));
        byte += 1;
    }
    table
};
/// Pixel mask of columns `lo..hi` within a cell (bit 3 = column 0).
const fn column_mask(lo: usize, hi: usize) -> u8 {
    let mut mask = 0u8;
    let mut c = lo;
    while c < hi {
        mask |= 8 >> c;
        c += 1;
    }
    mask
}

/// How a geometry span treats the terrain-clear coverage of the cells it paints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paint {
    /// Ordinary ink: paints every pixel, coverage bits untouched.
    Plain,
    /// Terrain-clear geometry: paints and marks every pixel as covered.
    Clear,
    /// Exterior terrain: paints only pixels that are not covered.
    Exterior,
}

/// Statistics for the host prototype (cycle-model inputs).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct CellStats {
    pub cell_rows: u32,
    pub overflow_cells: u32,
    pub overflow_pixels: u32,
    pub plots: u32,
    pub recycled_slots: u32,
}

/// One 160x8 band in cell form. Rows are row-major so a span writes
/// consecutive bytes; `finish` transposes into the cell-major VIC layout.
pub struct CellBand {
    codes: [[u8; 40]; 8],
    /// Slot keys: the slot's colour XOR the clear colour; 0 = free.
    palette: [[u8; 40]; 3],
    cover: [[u8; 40]; 8],
    hud_slots: [u8; 40],
    overflow: [bool; 40],
    /// Pixel form of overflowed cells (the classic band layout).
    pub pixels: [u8; BAND_PIXELS],
    coverage_seen: bool,
    /// The frame's clear colour: code 0 and the VIC background.
    background: u8,
    pub stats: CellStats,
}

impl CellBand {
    pub const fn new() -> Self {
        Self {
            codes: [[0; 40]; 8],
            palette: [[0; 40]; 3],
            cover: [[0; 40]; 8],
            hud_slots: [0; 40],
            overflow: [false; 40],
            pixels: [0; BAND_PIXELS],
            coverage_seen: false,
            background: 0,
            stats: CellStats {
                cell_rows: 0,
                overflow_cells: 0,
                overflow_pixels: 0,
                plots: 0,
                recycled_slots: 0,
            },
        }
    }

    /// Start a band filled with `color` (the frame's clear color): every
    /// pixel is code 0 and every slot is free.
    pub fn clear(&mut self, color: u8) {
        self.background = color & 15;
        for row in &mut self.codes {
            row.fill(0);
        }
        for slots in &mut self.palette {
            slots.fill(0);
        }
        for row in &mut self.cover {
            row.fill(0);
        }
        self.hud_slots.fill(0);
        self.overflow.fill(false);
        self.coverage_seen = false;
    }

    /// Whether a terrain-clear span has been painted in this band.
    pub fn coverage_seen(&self) -> bool {
        self.coverage_seen
    }

    /// Paint a geometry span: `color_flags` carries the color in bits 0-3 and
    /// the TERRAIN_CLEAR_GEOMETRY / EXTERIOR_TERRAIN flags of the triangle.
    pub fn geometry_span(&mut self, row: usize, first: usize, end: usize, color_flags: u8) {
        let paint = if color_flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
            self.coverage_seen = true;
            Paint::Clear
        } else if self.coverage_seen && color_flags & crate::EXTERIOR_TERRAIN != 0 {
            Paint::Exterior
        } else {
            Paint::Plain
        };
        self.span(row, first, end, color_flags & 15, false, paint);
    }

    /// Paint `[first, end)` of `row`. HUD ink marks its slot so an overflowed
    /// cell's pixels carry the priority bit the quantizer expects.
    pub fn span(&mut self, row: usize, first: usize, end: usize, color: u8, hud: bool, paint: Paint) {
        debug_assert!(row < 8 && first <= end && end <= WIDTH);
        if first >= end {
            return;
        }
        let color = color & 15;
        let first_cell = first / 4;
        let last_cell = (end - 1) / 4;
        for cell in first_cell..=last_cell {
            let lo = first.max(cell * 4) - cell * 4;
            let hi = end.min(cell * 4 + 4) - cell * 4;
            self.cell_row(row, cell, column_mask(lo, hi), color, hud, paint);
        }
    }

    /// Paint one pixel.
    pub fn plot(&mut self, row: usize, x: usize, color: u8, hud: bool) {
        debug_assert!(row < 8 && x < WIDTH);
        self.stats.plots += 1;
        self.cell_row(row, x / 4, 8 >> (x & 3), color & 15, hud, Paint::Plain);
    }

    fn cell_row(&mut self, row: usize, cell: usize, mut mask: u8, color: u8, hud: bool, paint: Paint) {
        self.stats.cell_rows += 1;
        if self.overflow[cell] {
            self.pixel_row(row, cell, mask, color, hud, paint);
            return;
        }
        if paint == Paint::Exterior {
            mask &= !self.cover[row][cell];
            if mask == 0 {
                return;
            }
        }
        // The clear colour is code 0; every other colour's key is nonzero.
        let key = color ^ self.background;
        let slot = if key == 0 {
            0
        } else {
            match self.slot(cell, key) {
                Some(slot) => slot,
                None => {
                    self.spill(cell);
                    self.pixel_row(row, cell, mask, color, hud, paint);
                    return;
                }
            }
        };
        if hud && slot != 0 {
            self.hud_slots[cell] |= 1 << (slot - 1);
        }
        let bits = EXPAND[mask as usize];
        self.codes[row][cell] = (self.codes[row][cell] & !bits) | (PATTERN[slot] & bits);
        if paint == Paint::Clear {
            self.cover[row][cell] |= mask;
        }
    }

    /// The slot holding the colour with key `key` in `cell`, assigning a free
    /// one if needed. A slot whose color no longer appears in any row (hidden
    /// by overdraw) is recycled before the cell is given up to the per-pixel
    /// form.
    fn slot(&mut self, cell: usize, key: u8) -> Option<usize> {
        for slot in 0..3 {
            if self.palette[slot][cell] == key {
                return Some(slot + 1);
            }
        }
        for slot in 0..3 {
            if self.palette[slot][cell] == 0 {
                self.palette[slot][cell] = key;
                return Some(slot + 1);
            }
        }
        let mut live = 0u8;
        for row in 0..8 {
            live |= CODES_PRESENT[self.codes[row][cell] as usize];
        }
        for slot in 0..3 {
            if live & (2 << slot) == 0 {
                self.stats.recycled_slots += 1;
                self.palette[slot][cell] = key;
                self.hud_slots[cell] &= !(1 << slot);
                return Some(slot + 1);
            }
        }
        None
    }

    /// Expand a cell into its pixel form before a fourth color is painted.
    fn spill(&mut self, cell: usize) {
        self.stats.overflow_cells += 1;
        for row in 0..8 {
            let codes = self.codes[row][cell];
            let cover = self.cover[row][cell];
            for column in 0..4 {
                let slot = (codes >> (6 - 2 * column)) & 3;
                let mut pixel = if slot == 0 {
                    self.background
                } else {
                    self.palette[slot as usize - 1][cell] ^ self.background
                };
                if cover & (8 >> column) != 0 {
                    pixel |= crate::render::TERRAIN_CLEAR_INK;
                }
                if slot != 0 && self.hud_slots[cell] & (1 << (slot - 1)) != 0 {
                    pixel |= HUD_INK;
                }
                self.pixels[row * WIDTH + cell * 4 + column] = pixel;
            }
        }
        self.overflow[cell] = true;
    }

    /// The classic per-pixel rules for an overflowed cell.
    fn pixel_row(&mut self, row: usize, cell: usize, mask: u8, color: u8, hud: bool, paint: Paint) {
        let ink = color | if hud { HUD_INK } else { 0 };
        for column in 0..4 {
            if mask & (8 >> column) == 0 {
                continue;
            }
            self.stats.overflow_pixels += 1;
            let pixel = &mut self.pixels[row * WIDTH + cell * 4 + column];
            match paint {
                Paint::Clear => *pixel = ink | crate::render::TERRAIN_CLEAR_INK,
                Paint::Exterior => {
                    if *pixel & crate::render::TERRAIN_CLEAR_INK == 0 {
                        *pixel = ink;
                    }
                }
                Paint::Plain => {
                    *pixel = if hud {
                        ink
                    } else {
                        (*pixel & crate::render::TERRAIN_CLEAR_INK) | ink
                    }
                }
            }
        }
    }

    /// Produce the VIC bytes of the band. `old_screen`/`old_color` are the
    /// cells' preceding attributes, used only by overflowed cells' quantizer.
    /// A free slot shows the clear colour (key 0), which no code selects.
    #[cfg(not(target_arch = "mos"))]
    pub fn finish(
        &self,
        old_screen: &[u8; 40],
        old_color: &[u8; 40],
        bitmap: &mut [u8; 320],
        screen: &mut [u8; 40],
        colors: &mut [u8; 40],
    ) {
        for cell in 0..40 {
            if self.overflow[cell] {
                let (cell_bitmap, cell_screen, cell_color) = crate::render::pack_cell(
                    &self.pixels,
                    cell,
                    [old_screen[cell] >> 4, old_screen[cell] & 15, old_color[cell] & 15],
                    self.background,
                );
                bitmap[cell * 8..cell * 8 + 8].copy_from_slice(&cell_bitmap);
                screen[cell] = cell_screen;
                colors[cell] = cell_color;
            } else {
                for row in 0..8 {
                    bitmap[cell * 8 + row] = self.codes[row][cell];
                }
                screen[cell] = ((self.palette[0][cell] << 4) | self.palette[1][cell])
                    ^ ((self.background << 4) | self.background);
                colors[cell] = self.palette[2][cell] ^ self.background;
            }
        }
    }
}

impl Default for CellBand {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::render::Canvas for CellBand {
    fn span(&mut self, row: usize, first: usize, end: usize, color: u8) {
        CellBand::span(self, row, first, end, color & 15, color & HUD_INK != 0, Paint::Plain);
    }
    fn plot(&mut self, row: usize, x: usize, color: u8) {
        CellBand::plot(self, row, x, color & 15, color & HUD_INK != 0);
    }
}

/// The target's band: the kernels in `target-c64/asm/cells.S` keep the same
/// state in fixed RAM (`CODES` at $9200, palettes at $9480, HUD slots and
/// overflow flags at $7700, the pixel form at $8C00), so this is a handle.
#[cfg(target_arch = "mos")]
pub struct NativeCells;

#[cfg(target_arch = "mos")]
extern "C" {
    fn fast_cell_clear(color: u8);
    fn fast_cell_span(row: u8, first: u8, end: u8, color: u8);
    fn fast_cell_plot(row: u8, x: u8, color: u8);
    fn fast_cell_glyph(row: u8, x: u8, rows: *const u8, color: u8);
    fn fast_cell_stage(old_screen: *const u8, old_colors: *const u8);
}

#[cfg(target_arch = "mos")]
impl NativeCells {
    /// Start a band filled with `color`.
    #[inline(always)]
    pub fn clear(&mut self, color: u8) {
        // SAFETY: the kernel writes only the fixed PIXELS-phase cell state.
        unsafe { fast_cell_clear(color) };
    }
    /// Stage the band's VIC bytes in fixed RAM: bitmap at $9340, screen at
    /// $7700 and colors at $7728. The preceding attributes are read only;
    /// live color RAM needs I/O mapped in. Staging consumes the band's cover,
    /// HUD-slot and overflow state. Publish it before calling `clear`, and
    /// clear before painting or staging another band.
    #[inline(always)]
    pub fn stage(&mut self, old_screen: &[u8; 40], old_colors: &[u8; 40]) {
        // SAFETY: the statically sized inputs provide the complete preceding
        // attributes; output occupies only the fixed PIXELS-phase workspace.
        unsafe { fast_cell_stage(old_screen.as_ptr(), old_colors.as_ptr()) };
    }
}

#[cfg(target_arch = "mos")]
impl crate::render::Canvas for NativeCells {
    #[inline(always)]
    fn span(&mut self, row: usize, first: usize, end: usize, color: u8) {
        // SAFETY: callers keep row < 8 and first <= end <= 160.
        unsafe { fast_cell_span(row as u8, first as u8, end as u8, color) };
    }
    #[inline(always)]
    fn plot(&mut self, row: usize, x: usize, color: u8) {
        // SAFETY: callers keep row < 8 and x < 160.
        unsafe { fast_cell_plot(row as u8, x as u8, color) };
    }
    #[inline(always)]
    fn glyph(&mut self, row: usize, x: usize, rows: &[u8; 5], color: u8) {
        // SAFETY: the text guards prove the five rows and three columns fit.
        unsafe { fast_cell_glyph(row as u8, x as u8, rows.as_ptr(), color) };
    }
}

/// Decode packed VIC bytes of one band back to pixel colors (bits 0-3);
/// `background` is the colour code 0 shows (`$D021`).
pub fn unpack_band(
    bitmap: &[u8; 320],
    screen: &[u8; 40],
    colors: &[u8; 40],
    background: u8,
) -> [u8; BAND_PIXELS] {
    let mut pixels = [0u8; BAND_PIXELS];
    for cell in 0..40 {
        for row in 0..8 {
            let byte = bitmap[cell * 8 + row];
            for column in 0..4 {
                let code = (byte >> (6 - 2 * column)) & 3;
                pixels[row * WIDTH + cell * 4 + column] = match code {
                    0 => background & 15,
                    1 => screen[cell] >> 4,
                    2 => screen[cell] & 15,
                    _ => colors[cell] & 15,
                };
            }
        }
    }
    pixels
}
