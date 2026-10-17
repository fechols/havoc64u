//! Allocation-free, flat-shaded triangle renderer for the VIC-II multicolor bitmap.
//!
//! Camera coordinates are integer world units, +X right, +Y up, +Z forward.
//! A face whose normal points toward the camera is front-facing. Bit 7 in a
//! band pixel marks HUD ink that must take precedence during cell quantization.

use crate::projection::project;
use crate::{GeometrySink, Vec3, TWO_SIDED};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 200;
pub const BAND_HEIGHT: usize = 8;
pub const BAND_PIXELS: usize = WIDTH * BAND_HEIGHT;
pub const TRIANGLE_CAPACITY: usize = 256;
pub const NEAR: i16 = 32;
pub const HUD_INK: u8 = 0x80;

#[derive(Clone, Copy)]
#[repr(C)]
struct Triangle {
    x: [i16; 3],
    y: [i16; 3],
    depth: i16,
    color: u8,
    reserved: u8,
}

impl Triangle {
    const EMPTY: Self = Self {
        x: [0; 3],
        y: [0; 3],
        depth: 0,
        color: 0,
        reserved: 0,
    };
}

/// Commands occupy exactly 4 KiB; metadata needs a further six bytes.
#[repr(C)]
pub struct Renderer {
    pub triangle_count: u16,
    pub dropped: u16,
    clear_color: u8,
    padding: u8,
    triangles: [Triangle; TRIANGLE_CAPACITY],
}

const _: [(); 6] = [(); core::mem::offset_of!(Renderer, triangles)];
const _: [(); 4102] = [(); core::mem::size_of::<Renderer>()];

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub const fn new() -> Self {
        Self {
            triangle_count: 0,
            dropped: 0,
            clear_color: 0,
            padding: 0,
            triangles: [Triangle::EMPTY; TRIANGLE_CAPACITY],
        }
    }

    pub fn begin(&mut self, clear_color: u8) {
        self.triangle_count = 0;
        self.dropped = 0;
        self.clear_color = clear_color & 15;
    }

    /// Stable far-to-near insertion sort: ties retain their authored ordering.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn finish(&mut self) {
        for i in 1..self.triangle_count as usize {
            let command = self.triangles[i];
            let mut j = i;
            while j > 0 && self.triangles[j - 1].depth < command.depth {
                self.triangles[j] = self.triangles[j - 1];
                j -= 1;
            }
            self.triangles[j] = command;
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn render_band(&self, band: u8, pixels: &mut [u8; BAND_PIXELS]) {
        fill_color(pixels, self.clear_color);
        if band >= 25 {
            return;
        }
        // The target publishes bands in order. Its pixels overlay leaves this
        // 3 KiB region free after the previous geometry overlay is replaced.
        #[cfg(target_arch = "mos")]
        if band == 0 {
            self.prepare_slopes();
        }
        let band_y = band as i16 * 8;
        for (index, triangle) in self.triangles[..self.triangle_count as usize]
            .iter()
            .enumerate()
        {
            #[cfg(target_arch = "mos")]
            let slopes = Some(unsafe { &*((0x6000 + index * 12) as *const [i32; 3]) });
            #[cfg(not(target_arch = "mos"))]
            let slopes = {
                let _ = index;
                None
            };
            raster_triangle(triangle, band_y, pixels, slopes);
        }
    }

    #[cfg(target_arch = "mos")]
    #[inline(never)]
    #[link_section = ".phase.pixels"]
    fn prepare_slopes(&self) {
        for (index, triangle) in self.triangles[..self.triangle_count as usize]
            .iter()
            .enumerate()
        {
            // SAFETY: the linker keeps pixel code below $6000, the private
            // scratch ends at $6C00, and at most 256 entries are generated.
            unsafe {
                core::ptr::write(
                    (0x6000 + index * 12) as *mut [i32; 3],
                    triangle_slopes(triangle),
                );
            }
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn project_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        let mut triangle = Triangle::EMPTY;
        let (ax, ay) = project(a);
        let (bx, by) = project(b);
        let (cx, cy) = project(c);
        triangle.x = [ax, bx, cx];
        triangle.y = [ay, by, cy];
        let area = (triangle.x[1] as i32 - triangle.x[0] as i32)
            * (triangle.y[2] as i32 - triangle.y[0] as i32)
            - (triangle.y[1] as i32 - triangle.y[0] as i32)
                * (triangle.x[2] as i32 - triangle.x[0] as i32);
        if area == 0 || (area < 0 && flags & TWO_SIDED == 0) {
            return;
        }
        if triangle.x.iter().all(|&x| x <= 0)
            || triangle.x.iter().all(|&x| x >= WIDTH as i16)
            || triangle.y.iter().all(|&y| y <= 0)
            || triangle.y.iter().all(|&y| y >= HEIGHT as i16)
        {
            return;
        }
        if self.triangle_count as usize == TRIANGLE_CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        // Clipping leaves positive i16 depths, so their average fits in i16.
        triangle.depth = crate::math::div_u32_u16(a.z as u32 + b.z as u32 + c.z as u32, 3) as i16;
        triangle.color = color & 15;
        self.triangles[self.triangle_count as usize] = triangle;
        self.triangle_count += 1;
    }
}

impl GeometrySink for Renderer {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        if a.z >= NEAR && b.z >= NEAR && c.z >= NEAR {
            self.project_triangle(a, b, c, color, flags);
            return;
        }
        if a.z < NEAR && b.z < NEAR && c.z < NEAR {
            return;
        }
        // A triangle clipped against a single plane produces at most four points.
        let input = [a, b, c];
        let mut polygon = [Vec3 { x: 0, y: 0, z: 0 }; 4];
        let mut len = 0;
        let mut previous = c;
        for current in input {
            if (previous.z >= NEAR) != (current.z >= NEAR) {
                polygon[len] = near_intersection(previous, current);
                len += 1;
            }
            if current.z >= NEAR {
                polygon[len] = current;
                len += 1;
            }
            previous = current;
        }
        if len < 3 {
            return;
        }
        self.project_triangle(polygon[0], polygon[1], polygon[2], color, flags);
        if len == 4 {
            self.project_triangle(polygon[0], polygon[2], polygon[3], color, flags);
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn near_intersection(a: Vec3, b: Vec3) -> Vec3 {
    // Q15 interpolation keeps full-range i16 input products within signed i32.
    let t = crate::math::div_i32((NEAR as i32 - a.z as i32) << 15, b.z as i32 - a.z as i32);
    Vec3 {
        x: (a.x as i32 + (((b.x as i32 - a.x as i32) * t) >> 15)) as i16,
        y: (a.y as i32 + (((b.y as i32 - a.y as i32) * t) >> 15)) as i16,
        z: NEAR,
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Edge {
    y0: i16,
    y1: i16,
    x: i32,
    step: i32,
}

impl Edge {
    const EMPTY: Self = Self {
        y0: 0,
        y1: 0,
        x: 0,
        step: 0,
    };
}

#[repr(C)]
struct RasterRows {
    edges: [Edge; 3],
    first_y: u8,
    end_y: u8,
    band_y: u8,
    color: u8,
}

const _: [(); 12] = [(); core::mem::size_of::<Edge>()];
const _: [(); 40] = [(); core::mem::size_of::<RasterRows>()];
const _: [(); 36] = [(); core::mem::offset_of!(RasterRows, first_y)];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn raster_triangle(
    triangle: &Triangle,
    band_y: i16,
    pixels: &mut [u8; BAND_PIXELS],
    slopes: Option<&[i32; 3]>,
) {
    let y0 = triangle.y[0]
        .min(triangle.y[1])
        .min(triangle.y[2])
        .max(band_y);
    let y1 = triangle.y[0]
        .max(triangle.y[1])
        .max(triangle.y[2])
        .min(band_y + 8);
    if y0 >= y1 {
        return;
    }
    let mut rows = RasterRows {
        edges: [Edge::EMPTY; 3],
        first_y: y0 as u8,
        end_y: y1 as u8,
        band_y: band_y as u8,
        color: triangle.color,
    };
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (ax, ay, bx, by) = if triangle.y[i] < triangle.y[j] {
            (triangle.x[i], triangle.y[i], triangle.x[j], triangle.y[j])
        } else {
            (triangle.x[j], triangle.y[j], triangle.x[i], triangle.y[i])
        };
        if ay == by || by <= y0 || ay >= y1 {
            continue;
        }
        let start = y0.max(ay);
        let dx = bx as i32 - ax as i32;
        let dy = by as i32 - ay as i32;
        // Evaluate at the center of the first pixel row. Normalize all edges
        // upward so adjacent triangles make identical shared-edge decisions.
        let numerator = ((start as i32 - ay as i32) * 2 + 1) << 14;
        let (t, step) = if let Some(slopes) = slopes {
            (
                crate::math::div_u32_u16(numerator as u32, dy as u16) as i32,
                slopes[i],
            )
        } else {
            (
                crate::math::div_i32(numerator, dy),
                crate::math::div_i32(dx << 8, dy),
            )
        };
        rows.edges[i] = Edge {
            y0: ay,
            y1: by,
            x: ((ax as i32) << 8) + ((dx * t) >> 7),
            step,
        };
    }
    #[cfg(target_arch = "mos")]
    {
        extern "C" {
            fn fast_raster_rows(rows: *mut RasterRows, pixels: *mut u8);
        }
        // SAFETY: the fixed C layout is asserted above; visible row bounds and
        // clipped spans constrain the kernel to the supplied 1,280-byte band.
        unsafe {
            fast_raster_rows(&mut rows, pixels.as_mut_ptr());
        }
    }
    #[cfg(not(target_arch = "mos"))]
    raster_rows_reference(&mut rows, pixels);
}

#[cfg(not(target_arch = "mos"))]
fn raster_rows_reference(rows: &mut RasterRows, pixels: &mut [u8; BAND_PIXELS]) {
    for y in rows.first_y as i16..rows.end_y as i16 {
        let mut left = i32::MAX;
        let mut right = i32::MIN;
        for edge in &mut rows.edges {
            if y >= edge.y0 && y < edge.y1 {
                left = left.min(edge.x);
                right = right.max(edge.x);
                edge.x += edge.step;
            }
        }
        if left >= right {
            continue;
        }
        // Half-open spans implement a shared-edge fill convention.
        let first = ((left + 127) >> 8).clamp(0, WIDTH as i32) as usize;
        let end = ((right + 127) >> 8).clamp(0, WIDTH as i32) as usize;
        if first < end {
            let row = (y - rows.band_y as i16) as usize * WIDTH;
            fill_color(&mut pixels[row + first..row + end], rows.color);
        }
    }
}

#[cfg_attr(not(target_arch = "mos"), allow(dead_code))]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn triangle_slopes(triangle: &Triangle) -> [i32; 3] {
    let mut slopes = [0; 3];
    for i in 0..3 {
        let j = (i + 1) % 3;
        let dx = triangle.x[j] as i32 - triangle.x[i] as i32;
        let dy = triangle.y[j] as i32 - triangle.y[i] as i32;
        if dy != 0 {
            slopes[i] = crate::math::div_i32(dx << 8, dy);
        }
    }
    slopes
}

/// Approximate VIC-II palette for host previews and quantization distances.
pub const PALETTE_RGB: [[u8; 3]; 16] = [
    [0, 0, 0],
    [255, 255, 255],
    [136, 0, 0],
    [170, 255, 238],
    [204, 68, 204],
    [0, 204, 85],
    [0, 0, 170],
    [238, 238, 119],
    [221, 136, 85],
    [102, 68, 0],
    [255, 119, 119],
    [51, 51, 51],
    [119, 119, 119],
    [170, 255, 102],
    [0, 136, 255],
    [187, 187, 187],
];

const fn color_distances() -> [[u16; 16]; 16] {
    let mut result = [[0; 16]; 16];
    let mut a = 0;
    while a < 16 {
        let mut b = 0;
        while b < 16 {
            let r = PALETTE_RGB[a][0] as i32 - PALETTE_RGB[b][0] as i32;
            let g = PALETTE_RGB[a][1] as i32 - PALETTE_RGB[b][1] as i32;
            let blue = PALETTE_RGB[a][2] as i32 - PALETTE_RGB[b][2] as i32;
            result[a][b] = ((r * r + 2 * g * g + blue * blue) / 4) as u16;
            b += 1;
        }
        a += 1;
    }
    result
}

#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", used)]
static COLOR_DISTANCE: [[u16; 16]; 16] = color_distances();

/// Convert one 160x8 band into forty VIC-II multicolor cells. Global color is
/// black. Each cell supplies screen high/low nibbles and one color RAM nibble.
/// Output bitmap bytes are cell-major, matching the native VIC bitmap layout.
#[inline(never)]
#[cfg(not(target_arch = "mos"))]
pub fn pack_band(
    pixels: &[u8; BAND_PIXELS],
    previous_screen: &[u8; 40],
    previous_color: &[u8; 40],
    bitmap: &mut [u8; 320],
    screen: &mut [u8; 40],
    colors: &mut [u8; 40],
) {
    for cell in 0..40 {
        let mut counts = [0u8; 16];
        let mut hud = 0u16;
        let mut present = 0u16;
        for row in 0..8 {
            for column in 0..4 {
                let pixel = pixels[row * WIDTH + cell * 4 + column];
                counts[(pixel & 15) as usize] += 1;
                present |= 1 << (pixel & 15);
                if pixel & HUD_INK != 0 {
                    hud |= 1 << (pixel & 15);
                }
            }
        }
        let old = [
            previous_screen[cell] >> 4,
            previous_screen[cell] & 15,
            previous_color[cell] & 15,
        ];
        // Large stretches of sky, hull, and terrain need no quantization.
        if present & present.wrapping_sub(1) == 0 {
            let color = pixels[cell * 4] & 15;
            let mut slot = 0;
            if color != 0 {
                slot = 1;
                for i in 0..3 {
                    if old[i] == color {
                        slot = i + 1;
                        break;
                    }
                }
            }
            fill_color(&mut bitmap[cell * 8..cell * 8 + 8], slot as u8 * 0x55);
            screen[cell] = match slot {
                1 => color << 4,
                2 => color,
                _ => 0,
            };
            colors[cell] = if slot == 3 { color } else { 0 };
            continue;
        }
        let mut selected = [0u8; 3];
        let mut selected_bits = 0u16;
        for chosen in &mut selected {
            let mut best = 0;
            let mut best_score = 0;
            for color in 1..16 {
                if counts[color] == 0 || selected_bits & (1 << color) != 0 {
                    continue;
                }
                let score = counts[color] as u16 * 4
                    + if old.contains(&(color as u8)) { 1 } else { 0 }
                    + if hud & (1 << color) != 0 { 256 } else { 0 };
                if score > best_score {
                    best = color as u8;
                    best_score = score;
                }
            }
            *chosen = best;
            selected_bits |= 1 << best;
        }
        // Retain palette slots when a preceding-frame color is still selected.
        let mut palette = [0u8; 4];
        let mut assigned = 0u16;
        for slot in 0..3 {
            if old[slot] != 0
                && selected_bits & (1 << old[slot]) != 0
                && assigned & (1 << old[slot]) == 0
            {
                palette[slot + 1] = old[slot];
                assigned |= 1 << old[slot];
            }
        }
        for chosen in selected {
            if chosen == 0 || assigned & (1 << chosen) != 0 {
                continue;
            }
            for slot in &mut palette[1..] {
                if *slot == 0 {
                    *slot = chosen;
                    assigned |= 1 << chosen;
                    break;
                }
            }
        }
        let mut map = [0u8; 16];
        for color in 1..16 {
            if counts[color] == 0 {
                continue;
            }
            let mut best_distance = COLOR_DISTANCE[color][0];
            for slot in 1..4 {
                let distance = COLOR_DISTANCE[color][palette[slot] as usize];
                if distance < best_distance {
                    map[color] = slot as u8;
                    best_distance = distance;
                }
            }
        }
        for row in 0..8 {
            let mut packed = 0;
            for column in 0..4 {
                packed =
                    (packed << 2) | map[(pixels[row * WIDTH + cell * 4 + column] & 15) as usize];
            }
            bitmap[cell * 8 + row] = packed;
        }
        screen[cell] = (palette[1] << 4) | palette[2];
        colors[cell] = palette[3];
    }
}

/// Assembly implements the same cell palette policy; host tests retain the
/// Rust reference so emulator output can be compared byte for byte.
#[cfg(target_arch = "mos")]
#[inline(never)]
#[link_section = ".phase.pixels"]
pub fn pack_band(
    pixels: &[u8; BAND_PIXELS],
    previous_screen: &[u8; 40],
    previous_color: &[u8; 40],
    bitmap: &mut [u8; 320],
    screen: &mut [u8; 40],
    colors: &mut [u8; 40],
) {
    extern "C" {
        fn fast_pack_band(
            pixels: *const u8,
            previous_screen: *const u8,
            previous_color: *const u8,
            bitmap: *mut u8,
            screen: *mut u8,
            colors: *mut u8,
        );
    }
    // SAFETY: the statically sized references provide all six complete buffers;
    // the kernel accesses only their documented bounds and private $77A0 scratch.
    unsafe {
        fast_pack_band(
            pixels.as_ptr(),
            previous_screen.as_ptr(),
            previous_color.as_ptr(),
            bitmap.as_mut_ptr(),
            screen.as_mut_ptr(),
            colors.as_mut_ptr(),
        );
    }
}

/// Draw a compact 3x5 uppercase/digit font, with a one-pixel advance gap.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw_text(band: u8, pixels: &mut [u8; BAND_PIXELS], x: i16, y: i16, text: &[u8], color: u8) {
    draw_text_scaled(band, pixels, x, y, text, color, 1);
}

/// Integer scaling for title lettering; ordinary HUD text uses scale one.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw_text_scaled(
    band: u8,
    pixels: &mut [u8; BAND_PIXELS],
    x: i16,
    y: i16,
    text: &[u8],
    color: u8,
    scale: u8,
) {
    let scale = scale.clamp(1, 4) as i16;
    let band_y = band as i16 * 8;
    if band >= 25 || y >= band_y + 8 || y.saturating_add(5 * scale) <= band_y {
        return;
    }
    let mut cursor = x;
    for &character in text {
        let glyph = glyph(character);
        let mut glyph_y = y;
        for &bits in glyph {
            for dy in 0..scale {
                let screen_y = glyph_y + dy;
                if screen_y < band_y || screen_y >= band_y + 8 {
                    continue;
                }
                let mut glyph_x = cursor;
                for mask in [4, 2, 1] {
                    if bits & mask != 0 {
                        for dx in 0..scale {
                            let screen_x = glyph_x.saturating_add(dx);
                            if screen_x >= 0 && screen_x < WIDTH as i16 {
                                pixels[(screen_y - band_y) as usize * WIDTH + screen_x as usize] =
                                    (color & 15) | HUD_INK;
                            }
                        }
                    }
                    glyph_x = glyph_x.saturating_add(scale);
                }
            }
            glyph_y += scale;
        }
        cursor = cursor.saturating_add(4 * scale);
        if cursor >= WIDTH as i16 {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw_bar(
    band: u8,
    pixels: &mut [u8; BAND_PIXELS],
    x: i16,
    y: i16,
    width: u8,
    height: u8,
    value: u16,
    maximum: u16,
    color: u8,
) {
    let band_y = band as i16 * 8;
    if band >= 25 || y >= band_y + 8 || y.saturating_add(height as i16) <= band_y {
        return;
    }
    let fill = if maximum == 0 {
        0
    } else {
        crate::math::div_u32(width as u32 * value.min(maximum) as u32, maximum as u32) as i16
    };
    let left = x.clamp(0, WIDTH as i16) as usize;
    let right = x.saturating_add(width as i16).clamp(0, WIDTH as i16) as usize;
    if left >= right {
        return;
    }
    let split = x.saturating_add(fill).clamp(left as i16, right as i16) as usize;
    for row in y..y.saturating_add(height as i16) {
        if row < band_y || row >= band_y + 8 {
            continue;
        }
        let offset = (row - band_y) as usize * WIDTH;
        fill_color(
            &mut pixels[offset + left..offset + split],
            color & 15 | HUD_INK,
        );
        fill_color(&mut pixels[offset + split..offset + right], 11);
    }
}

/// Target assembly preserves the raw pixel byte, including the HUD-priority bit.
/// Chunking also permits this helper to clear a complete 1,280-byte band safely.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub(crate) fn fill_color(pixels: &mut [u8], color: u8) {
    #[cfg(target_arch = "mos")]
    {
        extern "C" {
            fn fast_fill_span(destination: *mut u8, count: u8, color: u8);
        }
        for chunk in pixels.chunks_mut(255) {
            // SAFETY: every chunk is writable, non-overlapping, and at most 255
            // bytes; the assembly kernel implements this exact C ABI.
            unsafe {
                fast_fill_span(chunk.as_mut_ptr(), chunk.len() as u8, color);
            }
        }
    }
    #[cfg(not(target_arch = "mos"))]
    pixels.fill(color);
}

static FONT: [[u8; 5]; 43] = [
    [2, 5, 7, 5, 5],
    [6, 5, 6, 5, 6],
    [3, 4, 4, 4, 3],
    [6, 5, 5, 5, 6],
    [7, 4, 6, 4, 7],
    [7, 4, 6, 4, 4],
    [3, 4, 5, 5, 3],
    [5, 5, 7, 5, 5],
    [7, 2, 2, 2, 7],
    [1, 1, 1, 5, 2],
    [5, 5, 6, 5, 5],
    [4, 4, 4, 4, 7],
    [5, 7, 7, 5, 5],
    [5, 7, 7, 7, 5],
    [2, 5, 5, 5, 2],
    [6, 5, 6, 4, 4],
    [2, 5, 5, 3, 1],
    [6, 5, 6, 5, 5],
    [3, 4, 2, 1, 6],
    [7, 2, 2, 2, 2],
    [5, 5, 5, 5, 7],
    [5, 5, 5, 5, 2],
    [5, 5, 7, 7, 5],
    [5, 5, 2, 5, 5],
    [5, 5, 2, 2, 2],
    [7, 1, 2, 4, 7],
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [6, 1, 2, 4, 7],
    [6, 1, 2, 1, 6],
    [5, 5, 7, 1, 1],
    [7, 4, 6, 1, 6],
    [3, 4, 6, 5, 2],
    [7, 1, 2, 2, 2],
    [2, 5, 2, 5, 2],
    [2, 5, 3, 1, 6],
    [0, 2, 0, 2, 0],
    [0, 0, 7, 0, 0],
    [1, 1, 2, 4, 4],
    [2, 2, 2, 0, 2],
    [0, 0, 0, 0, 2],
    [0, 2, 7, 2, 0],
    [0; 5],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn glyph(c: u8) -> &'static [u8; 5] {
    let index = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a',
        b'0'..=b'9' => c - b'0' + 26,
        b':' => 36,
        b'-' => 37,
        b'/' => 38,
        b'!' => 39,
        b'.' => 40,
        b'+' => 41,
        _ => 42,
    };
    &FONT[index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_fit_four_kibibytes() {
        assert_eq!(core::mem::size_of::<Triangle>() * TRIANGLE_CAPACITY, 4096);
    }
    #[test]
    fn command_array_follows_six_byte_header() {
        assert_eq!(core::mem::offset_of!(Renderer, triangles), 6);
        assert_eq!(core::mem::size_of::<Renderer>(), 4102);
    }
    #[test]
    fn cached_slopes_and_short_divider_preserve_every_band_pixel() {
        let mut state = 0x0915_ba74u32;
        for _ in 0..400 {
            let mut triangle = Triangle::EMPTY;
            for i in 0..3 {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                triangle.x[i] = (state % 1600) as i16 - 700;
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                triangle.y[i] = (state % 1600) as i16 - 700;
            }
            triangle.color = 7;
            let slopes = triangle_slopes(&triangle);
            for band in 0..25 {
                let mut reference = [0; BAND_PIXELS];
                let mut cached = [0; BAND_PIXELS];
                raster_triangle(&triangle, band * 8, &mut reference, None);
                raster_triangle(&triangle, band * 8, &mut cached, Some(&slopes));
                assert_eq!(
                    cached, reference,
                    "triangle {:?}/{:?}, band {band}",
                    triangle.x, triangle.y
                );
            }
        }
    }
}
