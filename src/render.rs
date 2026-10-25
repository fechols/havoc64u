//! Allocation-free, flat-shaded triangle renderer for the VIC-II multicolor bitmap.
//!
//! Camera coordinates are integer world units, +X right, +Y up, +Z forward.
//! A face whose normal points toward the camera is front-facing. Bit 7 in a
//! band pixel marks HUD ink that must take precedence during cell quantization.
//! Bit 6 remembers coverage by terrain-protected geometry (the player and
//! selected terrestrial enemies/obstacles and the Dreadnought). Exterior terrain
//! cannot erase it; ordinary foreground still paints normally and retains the bit.

#[cfg(not(target_arch = "mos"))]
use crate::projection::project;
use crate::space_dust::SpaceDustFrame;
use crate::{GeometrySink, Vec3};

#[cfg(all(feature = "span-front-to-back", not(target_arch = "mos")))]
mod front;
#[cfg(test)]
mod shadow_tests;
mod sort;
mod probe;
pub use probe::PlayerPixelProbe;
#[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
mod spans;
#[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
pub use spans::SpanStats;

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 200;
pub const BAND_HEIGHT: usize = 8;
pub const BAND_PIXELS: usize = WIDTH * BAND_HEIGHT;
pub const TRIANGLE_CAPACITY: usize = 256;
pub const SCENERY_CAPACITY: usize = 96;
const SCENERY_OFFSET: usize = 2176;
/// The clipping plane sits almost on the lens: a vertex is on-screen only when
/// |x| < z and |y| < 0.75 z, so geometry can be cut visibly only inside the
/// tiny z < NEAR pyramid. Passing ships leave the screen edges before they are
/// clipped, and every actor, terrain and scenery coordinate still projects
/// inside the +/-8191 clamp at this depth.
pub const NEAR: i16 = 8;
pub const HUD_INK: u8 = 0x80;
pub(crate) const TERRAIN_CLEAR_INK: u8 = 0x40;
/// Staging-only: a sky layer with doubled XYZ also needs doubled camera drift.
/// Projection drops this bit; depth sorting and rasterization see normal sky.
pub(crate) const DOUBLE_SKY_DEPTH: u8 = 0x04;

/// An append-only geometry transaction, valid before sorting or rasterization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub triangle_count: u16,
    pub dropped: u16,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Triangle {
    x: [i16; 3],
    y: [i16; 3],
    depth: i16,
    color: u8,
    reserved: u8,
}

/// Camera-space records occupy the unused command tail during SCENERY.
#[derive(Clone, Copy)]
#[repr(C)]
struct SceneryTriangle {
    points: [Vec3; 3],
    color: u8,
    flags: u8,
}
const _: () = {
    assert!(core::mem::size_of::<Triangle>() == 16);
    assert!(core::mem::size_of::<SceneryTriangle>() == 20);
    assert!(SCENERY_OFFSET + SCENERY_CAPACITY * 20 == 4096);
    // Once a record has been copied out, even two clipped output commands
    // cannot catch the following unread record in the shared allocation.
    assert!(SCENERY_CAPACITY * (32 - 20) <= SCENERY_OFFSET);
};

/// The sky parallax: a slight camera pan toward the ship moves the far
/// backdrop (STATIC_SKY faces) opposite to its movement, nearer clouds more
/// than the horizon by perspective. Authored coordinates and the bounded
/// offsets cannot overflow. The host reference; `fast_replay_scenery` drifts
/// the staged records in place on the target.
#[cfg(not(target_arch = "mos"))]
#[inline(always)]
pub(crate) fn drift_sky(points: &mut [Vec3; 3], flags: u8, parallax_x: i16, parallax_y: i16) {
    if flags & crate::game::STATIC_SKY != 0 {
        let scale = if flags & DOUBLE_SKY_DEPTH != 0 { 2 } else { 1 };
        drift_points(points, parallax_x * scale, parallax_y * scale);
    }
}

#[cfg(not(target_arch = "mos"))]
#[inline(never)]
fn drift_points(points: &mut [Vec3; 3], parallax_x: i16, parallax_y: i16) {
    for point in points {
        point.x -= parallax_x;
        point.y -= parallax_y;
    }
}

pub struct SceneryStage<'a> {
    renderer: &'a mut Renderer,
    count: u8,
    shadows: Option<&'a mut crate::shadow::ShadowFrame>,
}
impl SceneryStage<'_> {
    #[inline(always)]
    pub fn count(&self) -> u8 {
        self.count
    }
}
impl GeometrySink for SceneryStage<'_> {
    #[inline(always)]
    fn shadow_enabled(&self) -> bool {
        self.shadows.is_some()
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery_aux")]
    fn shadow_caster(&mut self, caster: crate::shadow::Caster) {
        if let Some(frame) = &mut self.shadows {
            frame.push_caster(caster);
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        if self.count as usize == SCENERY_CAPACITY {
            self.renderer.dropped = self.renderer.dropped.saturating_add(1);
            return;
        }
        let record = SceneryTriangle {
            points: [a, b, c],
            color,
            flags,
        };
        // Derive provenance from the whole command allocation. Integer-only
        // records admit every bit pattern; no reference to retyped storage is
        // created, and the stage's exclusive borrow ends before projection.
        unsafe {
            self.renderer
                .triangles
                .as_mut_ptr()
                .cast::<u8>()
                .add(SCENERY_OFFSET + self.count as usize * 20)
                .cast::<SceneryTriangle>()
                .write_unaligned(record);
        }
        self.count += 1;
    }
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
    #[inline(always)]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            triangle_count: self.triangle_count,
            dropped: self.dropped,
        }
    }

    #[inline(always)]
    pub fn rollback(&mut self, checkpoint: Checkpoint) {
        assert_eq!(self.padding, 0, "cannot roll back rasterized commands");
        assert!(checkpoint.triangle_count <= self.triangle_count);
        self.triangle_count = checkpoint.triangle_count;
        self.dropped = checkpoint.dropped;
    }

    #[inline(always)]
    pub fn remaining_commands(&self) -> u16 {
        TRIANGLE_CAPACITY as u16 - self.triangle_count
    }

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
        self.padding = 0;
        #[cfg(target_arch = "mos")]
        sort::set_segments(0, 0);
    }

    /// The frame's clear colour: the VIC background every band starts from.
    #[inline(always)]
    pub fn clear_color(&self) -> u8 {
        self.clear_color
    }

    /// The victory overlay darkens the completed battlefield before adding
    /// its medal. Commands and the clear color are each mapped exactly once.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.victory")]
    pub fn fade_scene(&mut self, ticks: u16) {
        self.clear_color = crate::victory::fade_color(self.clear_color, ticks);
        for triangle in &mut self.triangles[..self.triangle_count as usize] {
            triangle.color = crate::victory::fade_color(triangle.color, ticks);
        }
    }

    /// Borrow the command tail for unprojected scenery after begin().
    #[inline(always)]
    pub fn scenery_stage(&mut self) -> SceneryStage<'_> {
        assert_eq!(self.triangle_count, 0, "stage scenery before projection");
        assert_eq!(self.padding, 0, "begin a new frame before scenery");
        SceneryStage {
            renderer: self,
            count: 0,
            shadows: None,
        }
    }

    #[inline(always)]
    pub fn scenery_stage_with_shadows<'a>(
        &'a mut self,
        frame: &'a mut crate::shadow::ShadowFrame,
    ) -> SceneryStage<'a> {
        assert_eq!(self.triangle_count, 0, "stage scenery before projection");
        assert_eq!(self.padding, 0, "begin a new frame before scenery");
        SceneryStage {
            renderer: self,
            count: 0,
            shadows: Some(frame),
        }
    }

    /// Append one flat-ground silhouette triangle through the ordinary projector.
    /// The caller owns whole-caster rollback and keeps these commands as a suffix.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow")]
    pub fn append_ground_shadow_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8) {
        #[cfg(not(target_arch = "mos"))]
        self.triangle(a, b, c, color, crate::EXTERIOR_TERRAIN);
        #[cfg(target_arch = "mos")]
        unsafe {
            extern "C" {
                fn fast_shadow_clip_triangle(
                    points: *const [Vec3; 3],
                    color: u8,
                    flags: u8,
                    renderer: *mut Renderer,
                );
            }
            assert_eq!(self.padding, 0, "begin a new frame before geometry");
            // The kernel uses the fixed resident address. Passing the alias
            // also tells LLVM that emission changes the caller's command count.
            fast_shadow_clip_triangle(&[a, b, c], color, crate::EXTERIOR_TERRAIN, self);
        }
    }

    /// Sort the ordinary scene, leaving solid ground silhouettes as a final layer.
    /// Their identical ink makes sorting the shadow suffix unnecessary.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    pub fn finish_ground_shadows(&mut self, base_count: u16) {
        assert_eq!(self.padding, 0, "begin a new frame before geometry");
        assert!(base_count <= self.triangle_count, "shadow count");
        sort::depth_sort(&mut self.triangles[..base_count as usize]);
        #[cfg(target_arch = "mos")]
        for index in base_count..self.triangle_count {
            // Projection is complete, and sorting has initialized the base prefix.
            unsafe {
                (SORT_ORDER as *mut u8)
                    .add(index as usize)
                    .write(index as u8)
            };
        }
    }

    /// Consume staged records before foreground can reuse their command bytes.
    /// Backdrop records (STATIC_SKY) receive the sky parallax here, so staging
    /// stays free of per-frame camera state.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn project_scenery(&mut self, count: u8, parallax_x: i16, parallax_y: i16) -> u16 {
        assert!(count as usize <= SCENERY_CAPACITY);
        assert_eq!(self.triangle_count, 0, "project scenery before foreground");
        #[cfg(target_arch = "mos")]
        {
            extern "C" {
                fn fast_replay_scenery(
                    start: u8,
                    count: u8,
                    parallax_x: i16,
                    parallax_y: i16,
                ) -> u8;
            }
            let mut index = 0;
            while index < count {
                // SAFETY: the kernel consumes the staged records of this
                // resident renderer in order and stops at the first one that
                // needs near clipping, which it has already drifted.
                index = unsafe { fast_replay_scenery(index, count, parallax_x, parallax_y) };
                if index < count {
                    let record = self.scenery_record(index);
                    self.triangle(
                        record.points[0],
                        record.points[1],
                        record.points[2],
                        record.color,
                        record.flags,
                    );
                    index += 1;
                }
            }
        }
        #[cfg(not(target_arch = "mos"))]
        for index in 0..count {
            let mut record = self.scenery_record(index);
            drift_sky(&mut record.points, record.flags, parallax_x, parallax_y);
            self.triangle(
                record.points[0],
                record.points[1],
                record.points[2],
                record.color,
                record.flags,
            );
        }
        self.triangle_count
    }

    /// Copy one staged record out before triangle() may reuse its bytes.
    /// After k inputs the output is at most 32*k bytes and the next input
    /// begins at 2176 + 20*k; the compile-time bound proves non-overlap.
    #[inline(always)]
    fn scenery_record(&self, index: u8) -> SceneryTriangle {
        unsafe {
            self.triangles
                .as_ptr()
                .cast::<u8>()
                .add(SCENERY_OFFSET + index as usize * 20)
                .cast::<SceneryTriangle>()
                .read_unaligned()
        }
    }

    /// Restore foreground-first authoring order before the stable depth sort.
    /// The host rotates the commands; the target only records the two
    /// segment boundaries, which the sort kernel scans in logical order
    /// (foreground, then scenery, then anything added later).
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn scenery_last(&mut self, prefix: u16) {
        assert!(prefix <= self.triangle_count && self.triangle_count as usize <= TRIANGLE_CAPACITY);
        if prefix == 0 || prefix == self.triangle_count {
            return;
        }
        #[cfg(target_arch = "mos")]
        {
            // prefix < count <= 256: the pivot is stored modulo 256 (0 = 256).
            sort::set_segments(prefix as u8, self.triangle_count as u8);
        }
        #[cfg(not(target_arch = "mos"))]
        {
            self.reverse_commands(0, prefix);
            self.reverse_commands(prefix, self.triangle_count);
            self.reverse_commands(0, self.triangle_count);
        }
    }

    #[inline(never)]
    #[cfg(not(target_arch = "mos"))]
    fn reverse_commands(&mut self, mut first: u16, mut end: u16) {
        while first < end {
            end -= 1;
            if first == end {
                break;
            }
            // Whole-record assignment still lowers to a general memmove on
            // LLVM-MOS. Scalar volatile byte swaps keep the operation bounded,
            // prohibit that transformation and need only two temporary bytes.
            // Both distinct command indices are within the initialized array;
            // integer-only records remain valid throughout the exchange.
            unsafe {
                let commands = self.triangles.as_mut_ptr();
                let left = commands.add(first as usize).cast::<u8>();
                let right = commands.add(end as usize).cast::<u8>();
                for byte in 0..16 {
                    let a = left.add(byte).read_volatile();
                    let b = right.add(byte).read_volatile();
                    left.add(byte).write_volatile(b);
                    right.add(byte).write_volatile(a);
                }
            }
            first += 1;
        }
    }

    /// Stable far-to-near radix sort: ties retain their authored ordering.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    pub fn finish(&mut self) {
        assert_eq!(self.padding, 0, "begin a new frame before geometry");
        sort::depth_sort(&mut self.triangles[..self.triangle_count as usize]);
    }

    #[cfg(not(target_arch = "mos"))]
    #[inline(always)]
    pub fn render_band(&self, band: u8, pixels: &mut [u8; BAND_PIXELS]) {
        self.render_band_with_ambient(band, pixels, None);
    }

    /// Host reference of the classic band: one byte per pixel, to be
    /// quantized by `pack_band`.
    #[cfg(not(target_arch = "mos"))]
    #[inline(never)]
    pub fn render_band_with_ambient(
        &self,
        band: u8,
        pixels: &mut [u8; BAND_PIXELS],
        ambient: Option<&SpaceDustFrame>,
    ) {
        assert_eq!(self.padding, 0, "raster pass consumed geometry commands");
        fill_color(pixels, self.clear_color);
        if band >= 25 {
            return;
        }
        if let Some(ambient) = ambient {
            ambient.draw_band(band, pixels);
        }
        let mut coverage_seen = false;
        self.band_spans(band, |row, first, end, flags| {
            paint_reference_span(pixels, row, first, end, flags, &mut coverage_seen);
        });
    }

    /// Host reference of the cell-direct pass: the same geometry walk as
    /// `render_band_with_ambient`, painting spans into a `CellBand`.
    #[cfg(not(target_arch = "mos"))]
    pub fn render_band_cells(
        &self,
        band: u8,
        cells: &mut crate::cells::CellBand,
        ambient: Option<&SpaceDustFrame>,
    ) {
        assert_eq!(self.padding, 0, "raster pass consumed geometry commands");
        cells.clear(self.clear_color);
        if band >= 25 {
            return;
        }
        if let Some(ambient) = ambient {
            ambient.draw_band(band, cells);
        }
        self.band_spans(band, |row, first, end, flags| {
            cells.geometry_span(row, first, end, flags);
        });
    }

    #[cfg(not(target_arch = "mos"))]
    fn band_spans(&self, band: u8, mut paint: impl FnMut(usize, usize, usize, u8)) {
        for triangle in &self.triangles[..self.triangle_count as usize] {
            let (mut head, tail) = prepare_triangle(triangle);
            seek_row(&mut head, &tail, band * 8);
            raster_rows(&mut head, &tail, band * 8, &mut paint);
        }
    }

    /// Experimental span rendering, retaining the classic band's raw colors
    /// and terrain coverage. `span-front-to-back` selects reverse ownership;
    /// `span-buffer` alone selects deferred painter-order composition.
    #[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
    pub fn render_band_spans(
        &self,
        band: u8,
        pixels: &mut [u8; BAND_PIXELS],
        ambient: Option<&SpaceDustFrame>,
    ) -> SpanStats {
        #[cfg(feature = "span-front-to-back")]
        {
            self.render_front_band(band, pixels, ambient)
        }
        #[cfg(not(feature = "span-front-to-back"))]
        {
            let mut spans = self.compose_band(band, ambient);
            spans.write_pixels(pixels);
            spans.stats
        }
    }

    /// Write only visible spans into cell palettes using the selected span
    /// backend. Palette slot order may differ from the painter.
    #[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
    pub fn render_band_span_cells(
        &self,
        band: u8,
        cells: &mut crate::cells::CellBand,
        ambient: Option<&SpaceDustFrame>,
    ) -> SpanStats {
        #[cfg(feature = "span-front-to-back")]
        {
            self.render_front_band(band, cells, ambient)
        }
        #[cfg(not(feature = "span-front-to-back"))]
        {
            let mut spans = self.compose_band(band, ambient);
            spans.write_cells(cells);
            spans.stats
        }
    }

    #[cfg(all(
        feature = "span-buffer",
        not(feature = "span-front-to-back"),
        not(target_arch = "mos")
    ))]
    fn compose_band(&self, band: u8, ambient: Option<&SpaceDustFrame>) -> spans::SpanBand {
        assert_eq!(self.padding, 0, "raster pass consumed geometry commands");
        let mut spans = spans::SpanBand::new(self.clear_color);
        if band >= 25 {
            return spans;
        }
        if let Some(ambient) = ambient {
            ambient.draw_band(band, &mut spans);
        }
        self.band_spans(band, |row, first, end, flags| {
            spans.geometry_span(row, first, end, flags);
        });
        spans
    }

    #[cfg(all(feature = "span-front-to-back", not(target_arch = "mos")))]
    fn render_front_band(
        &self,
        band: u8,
        canvas: &mut impl front::FrontCanvas,
        ambient: Option<&SpaceDustFrame>,
    ) -> SpanStats {
        assert_eq!(self.padding, 0, "raster pass consumed geometry commands");
        let mut spans = front::FrontBand::new(self.clear_color, canvas);
        if band >= 25 {
            return spans.stats;
        }
        let triangles = &self.triangles[..self.triangle_count as usize];
        // Coverage is independent of visible ownership. Exterior terrain can
        // never survive any terrain-clear hit, before or after it in the order.
        for triangle in triangles {
            if triangle.reserved & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                let (mut head, tail) = prepare_triangle(triangle);
                seek_row(&mut head, &tail, band * 8);
                raster_rows(&mut head, &tail, band * 8, |row, first, end, _| {
                    spans.cover_span(row, first, end, canvas);
                });
            }
        }
        for triangle in triangles.iter().rev() {
            let (mut head, tail) = prepare_triangle(triangle);
            seek_row(&mut head, &tail, band * 8);
            raster_rows(&mut head, &tail, band * 8, |row, first, end, flags| {
                spans.geometry_span(row, first, end, flags, canvas);
            });
        }
        spans.ambient(band, ambient, canvas);
        spans.stats
    }

    /// Consume sorted geometry into a bounded sequential raster pass. A second
    /// pass requires begin(); the borrow prevents geometry changes during a pass.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn raster_pass(&mut self) -> Option<RasterPass<'_>> {
        let required = if cfg!(target_arch = "mos") { 1 } else { 0 };
        if self.padding != required {
            return None;
        }
        self.padding = 2;
        #[allow(unused_mut)]
        let mut pass = RasterPass::new(self);
        #[cfg(not(target_arch = "mos"))]
        pass.prepare(None);
        Some(pass)
    }

    /// SHADOW retires its frame before PIXELS reuses the prepared records.
    #[cfg(target_arch = "mos")]
    #[inline(never)]
    #[link_section = ".phase.shadow.final"]
    pub fn prepare_raster(&mut self) {
        assert_eq!(self.padding, 0);
        let mut pass = RasterPass::new(self);
        pass.prepare(None);
        pass.renderer.padding = 1;
    }

    /// Reuse the normal preparation pass to sample the player's silhouette.
    #[cfg(target_arch = "mos")]
    #[inline(never)]
    #[link_section = ".phase.shadow.final"]
    pub fn prepare_raster_with_probe(&mut self, probe: &mut PlayerPixelProbe) {
        assert_eq!(self.padding, 0);
        let mut pass = RasterPass::new(self);
        pass.prepare(Some(probe));
        pass.renderer.padding = 1;
    }

    /// Host captures use the same prepared-edge predicate without consuming
    /// commands; native rendering performs this during its existing preparation.
    #[cfg(not(target_arch = "mos"))]
    pub fn probe_player_pixels(&self, probe: &mut PlayerPixelProbe) {
        assert_eq!(self.padding, 0);
        for triangle in &self.triangles[..self.triangle_count as usize] {
            if triangle.reserved & crate::PLAYER_GEOMETRY != 0 {
                let (head, tail) = prepare_triangle(triangle);
                probe.triangle(&head, &tail);
            }
        }
    }

    /// Project, cull and record one triangle whose corners all lie at or
    /// behind the near plane: the host reference of `fast_emit_triangle`.
    #[cfg(not(target_arch = "mos"))]
    #[inline(always)]
    fn emit_unclipped(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        self.project_triangle(a, b, c, color, flags);
    }

    #[cfg(not(target_arch = "mos"))]
    #[inline(never)]
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
        if area <= 0 {
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
        triangle.reserved = flags
            & (crate::game::STATIC_SKY | crate::TERRAIN_CLEAR_GEOMETRY | crate::EXTERIOR_TERRAIN | crate::PLAYER_GEOMETRY);
        self.triangles[self.triangle_count as usize] = triangle;
        self.triangle_count += 1;
    }
}

impl GeometrySink for Renderer {
    #[inline(always)]
    fn checkpoint(&self) -> Option<Checkpoint> {
        Some(Renderer::checkpoint(self))
    }
    #[inline(always)]
    fn rollback(&mut self, checkpoint: Checkpoint) {
        Renderer::rollback(self, checkpoint);
    }
    #[inline(always)]
    fn scenery_last(&mut self, prefix: u16) {
        Renderer::scenery_last(self, prefix);
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        assert_eq!(self.padding, 0, "begin a new frame before geometry");
        #[cfg(target_arch = "mos")]
        {
            extern "C" {
                fn fast_clip_triangle(points: *const [Vec3; 3], color: u8, flags: u8);
            }
            let points = [a, b, c];
            // SAFETY: the kernel performs exactly this function's near-plane
            // classification, clipping and emission on the resident RENDERER
            // (the only Renderer the target constructs) through
            // fast_emit_triangle and GEOMETRY's projection cache.
            unsafe { fast_clip_triangle(&points, color, flags) };
        }
        #[cfg(not(target_arch = "mos"))]
        self.triangle_reference(a, b, c, color, flags);
    }
}

impl Renderer {
    /// The host reference of the sink: near-plane classification, Sutherland–
    /// Hodgman clipping against z = NEAR and projection of the pieces.
    #[cfg(not(target_arch = "mos"))]
    fn triangle_reference(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        if a.z >= NEAR && b.z >= NEAR && c.z >= NEAR {
            self.emit_unclipped(a, b, c, color, flags);
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
        self.emit_unclipped(polygon[0], polygon[1], polygon[2], color, flags);
        if len == 4 {
            self.emit_unclipped(polygon[0], polygon[2], polygon[3], color, flags);
        }
    }
}

#[inline(never)]
#[cfg(not(target_arch = "mos"))]
fn near_intersection(a: Vec3, b: Vec3) -> Vec3 {
    // Both owners of a shared edge must round the same intersection, even
    // when their winding traverses that edge in opposite directions.
    let (a, b) = if a.z < b.z { (a, b) } else { (b, a) };
    // Q15 interpolation keeps full-range i16 input products within signed i32.
    let t = crate::math::div_i32((NEAR as i32 - a.z as i32) << 15, b.z as i32 - a.z as i32);
    Vec3 {
        x: (a.x as i32 + (((b.x as i32 - a.x as i32) * t) >> 15)) as i16,
        y: (a.y as i32 + (((b.y as i32 - a.y as i32) * t) >> 15)) as i16,
        z: NEAR,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct RasterHead {
    long_x: i32,
    long_step: i32,
    short_x: i32,
    short_step: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct RasterTail {
    lower_x: i32,
    lower_step: i32,
    top: u8,
    middle: u8,
    bottom: u8,
    color_flags: u8,
}
#[cfg(not(target_arch = "mos"))]
impl RasterTail {
    const EMPTY: Self = Self {
        lower_x: 0,
        lower_step: 0,
        top: 0,
        middle: 0,
        bottom: 0,
        color_flags: 0,
    };
}
const _: [(); 16] = [(); core::mem::size_of::<RasterHead>()];
const _: [(); 12] = [(); core::mem::size_of::<RasterTail>()];
const _: [(); 8] = [(); core::mem::offset_of!(RasterTail, top)];
const _: [(); 11] = [(); core::mem::offset_of!(RasterTail, color_flags)];

/// Native storage is phase-owned: the depth sort's order table at $7600 maps
/// sorted positions to the commands left in emission order at $D000, heads
/// occupy $EF20-$FF1F, tails $E000-$EBFF, and ordered band masks $EC00-$EF1F.
/// With `span-buffer`, commands retire after all preparation and $D000-$D9FF
/// becomes the band compositor. Front-to-back additionally borrows eight row
/// heads at $77AC-$77B3 until packing. Host heads reuse commands, so host spans
/// have separate storage. Both paths retain persistent Q16 edge precision.
pub struct RasterPass<'a> {
    renderer: &'a mut Renderer,
    next_band: u8,
    #[cfg(not(target_arch = "mos"))]
    tails: [RasterTail; TRIANGLE_CAPACITY],
    #[cfg(not(target_arch = "mos"))]
    masks: [[u8; 32]; 25],
    #[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
    span_stats: SpanStats,
}
#[cfg(target_arch = "mos")]
const SORT_ORDER: usize = 0x7600;
#[cfg(target_arch = "mos")]
const RASTER_HEADS: usize = 0xef20;
impl<'a> RasterPass<'a> {
    #[inline(always)]
    fn new(renderer: &'a mut Renderer) -> Self {
        Self {
            renderer,
            next_band: 0,
            #[cfg(not(target_arch = "mos"))]
            tails: [RasterTail::EMPTY; TRIANGLE_CAPACITY],
            #[cfg(not(target_arch = "mos"))]
            masks: [[0; 32]; 25],
            #[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
            span_stats: SpanStats::default(),
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
    fn prepare(&mut self, mut probe: Option<&mut PlayerPixelProbe>) {
        self.masks_mut().fill([0; 32]);
        for index in 0..self.renderer.triangle_count as usize {
            #[cfg(target_arch = "mos")]
            let tail = unsafe {
                let source = *(SORT_ORDER as *const u8).add(index) as usize;
                let head = self.head_ptr(index);
                let tail = self.tails_mut().as_mut_ptr().add(index);
                let player = self.renderer.triangles[source].reserved & crate::PLAYER_GEOMETRY != 0;
                prepare_triangle_into(&self.renderer.triangles[source], &mut *head, &mut *tail);
                if player {
                    if let Some(probe) = probe.as_deref_mut() {
                        probe.triangle(&*head, &*tail);
                    }
                }
                *tail
            };
            #[cfg(not(target_arch = "mos"))]
            let (head, tail) = prepare_triangle(&self.renderer.triangles[index]);
            #[cfg(not(target_arch = "mos"))]
            {
                if self.renderer.triangles[index].reserved & crate::PLAYER_GEOMETRY != 0 {
                    if let Some(probe) = probe.as_deref_mut() {
                        probe.triangle(&head, &tail);
                    }
                }
                self.write_head(index, head);
                self.tails[index] = tail;
            }
            if tail.top < tail.bottom {
                for band in tail.top / 8..=(tail.bottom - 1) / 8 {
                    self.masks_mut()[band as usize][index / 8] |= 1 << (index & 7);
                }
            }
        }
    }
    /// Accumulated compositor work for the visible bands produced by this pass.
    #[cfg(all(feature = "span-buffer", not(target_arch = "mos")))]
    pub fn span_stats(&self) -> SpanStats {
        self.span_stats
    }

    #[inline(always)]
    #[cfg(target_arch = "mos")]
    fn tails_mut(&mut self) -> &mut [RasterTail; TRIANGLE_CAPACITY] {
        unsafe { &mut *(0xe000 as *mut [RasterTail; TRIANGLE_CAPACITY]) }
    }
    #[inline(always)]
    fn masks_mut(&mut self) -> &mut [[u8; 32]; 25] {
        #[cfg(target_arch = "mos")]
        unsafe {
            &mut *(0xec00 as *mut [[u8; 32]; 25])
        }
        #[cfg(not(target_arch = "mos"))]
        {
            &mut self.masks
        }
    }
    /// The persistent head of sorted position `index`: a fixed array on the
    /// target, the (sorted) command's own bytes on the host.
    #[inline(always)]
    fn head_ptr(&mut self, index: usize) -> *mut RasterHead {
        #[cfg(target_arch = "mos")]
        {
            (RASTER_HEADS + index * 16) as *mut RasterHead
        }
        #[cfg(not(target_arch = "mos"))]
        {
            self.renderer
                .triangles
                .as_mut_ptr()
                .wrapping_add(index)
                .cast()
        }
    }
    #[cfg(not(target_arch = "mos"))]
    #[inline(always)]
    fn write_head(&mut self, index: usize, head: RasterHead) {
        // Triangle storage can be only 2-byte aligned on the host. No typed
        // RasterHead reference is formed; native fixed storage is aligned.
        unsafe { self.head_ptr(index).write_unaligned(head) };
    }
    /// Produce the next band. Hidden scene bands leave pixels untouched while
    /// advancing every active edge; an opaque HUD panel must replace the band.
    #[inline(always)]
    pub fn next_band(&mut self, pixels: &mut [u8; BAND_PIXELS], paint_scene: bool) -> Option<u8> {
        self.next_band_with_ambient(pixels, paint_scene, None)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn next_band_with_ambient(
        &mut self,
        pixels: &mut [u8; BAND_PIXELS],
        paint_scene: bool,
        ambient: Option<&SpaceDustFrame>,
    ) -> Option<u8> {
        let band = self.next_band;
        if band >= 25 {
            return None;
        }
        self.next_band += 1;
        #[cfg(target_arch = "mos")]
        unsafe {
            // PIXELS owns this byte between preparation and packing. It is
            // reset even for hidden/empty bands; the IRQ never accesses it.
            (0x77ab as *mut u8).write_volatile(0);
        }
        #[cfg(all(not(feature = "span-buffer"), not(target_arch = "mos")))]
        let mut coverage_seen = false;
        #[cfg(target_arch = "mos")]
        {
            // The cell writer starts every band from its clear colour: a
            // band the HUD replaces still needs fresh palettes and flags.
            let mut cells = crate::cells::NativeCells;
            cells.clear(self.renderer.clear_color);
            #[cfg(feature = "span-buffer")]
            extern "C" {
                fn fast_span_clear();
                fn fast_span_plot(row: u8, x: u8, color: u8);
                #[cfg(not(feature = "span-front-to-back"))]
                fn fast_span_flush();
            }
            // Preparation has consumed every command before this point. The
            // compositor borrows only the retired command bytes at $D000.
            #[cfg(feature = "span-buffer")]
            unsafe {
                fast_span_clear()
            };
            #[cfg(not(feature = "span-front-to-back"))]
            if paint_scene {
                if let Some(ambient) = ambient {
                    #[cfg(not(feature = "span-buffer"))]
                    ambient.draw_band(band, &mut cells);
                    #[cfg(feature = "span-buffer")]
                    for &(x, y) in ambient.positions() {
                        if y >> 3 == band {
                            // prepare() guarantees x<160 for every visible y.
                            unsafe { fast_span_plot(y & 7, x, crate::space_dust::COLOR) };
                        }
                    }
                }
            }
            extern "C" {
                fn fast_raster_band(mask: *const u8, pixels: *mut u8, band_y: u8, paint: u8);
            }
            // SAFETY: the kernel derives each marked triangle's persistent
            // head ($EF20 + 16 i) and prepared tail ($E000 + 12 i) from its
            // sorted position and paints through the cell writer; `pixels`
            // is the fixed band at $8E00 that overflowed cells expand into.
            unsafe {
                fast_raster_band(
                    self.masks_mut()[band as usize].as_ptr(),
                    pixels.as_mut_ptr(),
                    band * 8,
                    paint_scene as u8,
                );
            }
            #[cfg(all(feature = "span-buffer", not(feature = "span-front-to-back")))]
            if paint_scene {
                // Final runs are disjoint; the HUD draws after this pass.
                unsafe { fast_span_flush() };
            }
            #[cfg(feature = "span-front-to-back")]
            if paint_scene {
                if let Some(ambient) = ambient {
                    for &(x, y) in ambient.positions() {
                        if y >> 3 == band {
                            // Dust is the last candidate in the reverse pass.
                            unsafe { fast_span_plot(y & 7, x, crate::space_dust::COLOR) };
                        }
                    }
                }
            }
        }
        #[cfg(all(not(feature = "span-front-to-back"), not(target_arch = "mos")))]
        {
            #[cfg(feature = "span-buffer")]
            let mut spans = spans::SpanBand::new(self.renderer.clear_color);
            if paint_scene {
                #[cfg(not(feature = "span-buffer"))]
                fill_color(pixels, self.renderer.clear_color);
                if let Some(ambient) = ambient {
                    #[cfg(not(feature = "span-buffer"))]
                    ambient.draw_band(band, pixels);
                    #[cfg(feature = "span-buffer")]
                    ambient.draw_band(band, &mut spans);
                }
            }
            for byte in 0..32usize {
                let mut mask = self.masks_mut()[band as usize][byte];
                let mut bit = 0;
                while mask != 0 {
                    if mask & 1 != 0 {
                        let index = byte * 8 + bit;
                        let mut head = unsafe { self.head_ptr(index).read_unaligned() };
                        let tail = self.tails[index];
                        let mut paint = |row, first, end, flags| {
                            if paint_scene {
                                #[cfg(not(feature = "span-buffer"))]
                                paint_reference_span(
                                    pixels,
                                    row,
                                    first,
                                    end,
                                    flags,
                                    &mut coverage_seen,
                                );
                                #[cfg(feature = "span-buffer")]
                                spans.geometry_span(row, first, end, flags);
                            }
                        };
                        raster_rows(&mut head, &tail, band * 8, &mut paint);
                        self.write_head(index, head);
                    }
                    mask >>= 1;
                    bit += 1;
                }
            }
            #[cfg(feature = "span-buffer")]
            if paint_scene {
                spans.write_pixels(pixels);
                self.span_stats += spans.stats;
            }
        }
        #[cfg(all(feature = "span-front-to-back", not(target_arch = "mos")))]
        {
            let mut spans = if paint_scene {
                Some(front::FrontBand::new(self.renderer.clear_color, pixels))
            } else {
                None
            };
            if let Some(spans) = &mut spans {
                // Read copies only: this walk must not advance the persistent
                // heads that the actual reverse pass consumes below.
                for byte in 0..32usize {
                    let mask = self.masks[band as usize][byte];
                    for bit in 0..8 {
                        let index = byte * 8 + bit;
                        if mask & (1 << bit) != 0
                            && self.tails[index].color_flags & crate::TERRAIN_CLEAR_GEOMETRY != 0
                        {
                            let mut head = unsafe { self.head_ptr(index).read_unaligned() };
                            let tail = self.tails[index];
                            raster_rows(&mut head, &tail, band * 8, |row, first, end, _| {
                                spans.cover_span(row, first, end, pixels);
                            });
                        }
                    }
                }
            }
            // Reverse sorted positions, including ties. Even a hidden band or
            // a fully owned row must consume each active edge exactly once.
            for byte in (0..32usize).rev() {
                let mask = self.masks[band as usize][byte];
                for bit in (0..8).rev() {
                    if mask & (1 << bit) != 0 {
                        let index = byte * 8 + bit;
                        let mut head = unsafe { self.head_ptr(index).read_unaligned() };
                        let tail = self.tails[index];
                        let mut paint = |row, first, end, flags| {
                            if let Some(spans) = &mut spans {
                                spans.geometry_span(row, first, end, flags, pixels);
                            }
                        };
                        raster_rows(&mut head, &tail, band * 8, &mut paint);
                        self.write_head(index, head);
                    }
                }
            }
            if let Some(mut spans) = spans {
                spans.ambient(band, ambient, pixels);
                self.span_stats += spans.stats;
            }
        }
        Some(band)
    }
}

// Canonical initialization is per edge, never per band or per owning triangle.
// Projected coordinates are within +/-8191. dx*65536 and dx*t*2 fit i32;
// the visible-center fraction t is below 32768 and its numerator below 2^28.
#[inline(never)]
#[cfg(not(target_arch = "mos"))]
fn prepare_edge(ax: i16, ay: i16, bx: i16, by: i16) -> (i32, i32) {
    if ay == by || by <= 0 || ay >= 200 {
        return (0, 0);
    }
    let dx = bx as i32 - ax as i32;
    let dy = by as i32 - ay as i32;
    let start = ay.max(0) as i32;
    let numerator = ((start - ay as i32) * 2 + 1) << 14;
    let t = crate::math::div_u32_u16(numerator as u32, dy as u16) as i32;
    (
        (ax as i32 * 65536) + dx * t * 2,
        crate::math::div_i32(dx * 65536, dy),
    )
}

#[cfg(not(target_arch = "mos"))]
fn prepare_triangle(triangle: &Triangle) -> (RasterHead, RasterTail) {
    let mut head = RasterHead {
        long_x: 0,
        long_step: 0,
        short_x: 0,
        short_step: 0,
    };
    let mut tail = RasterTail::EMPTY;
    prepare_triangle_into(triangle, &mut head, &mut tail);
    (head, tail)
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
fn prepare_triangle_into(triangle: &Triangle, head: &mut RasterHead, tail: &mut RasterTail) {
    #[cfg(target_arch = "mos")]
    {
        extern "C" {
            fn fast_prepare_triangle(
                triangle: *const Triangle,
                head: *mut RasterHead,
                tail: *mut RasterTail,
            );
        }
        // PIXELS owns $77A0-$77BE until all edges have been prepared;
        // row walking and packing reuse it afterward. The kernel copies all
        // input bytes before writing either output, preserving the reference's
        // signed rounding and sorting parity.
        unsafe {
            fast_prepare_triangle(triangle, head, tail);
        }
    }
    #[cfg(not(target_arch = "mos"))]
    {
        // Keep the independent host reference explicit: sort each endpoint
        // pair and retain parity before preparing three canonical edges.
        let mut a = Point2 {
            x: triangle.x[0],
            y: triangle.y[0],
        };
        let mut b = Point2 {
            x: triangle.x[1],
            y: triangle.y[1],
        };
        let mut c = Point2 {
            x: triangle.x[2],
            y: triangle.y[2],
        };
        let mut orientation = 16;
        orientation ^= sort_pair(&mut a, &mut b);
        orientation ^= sort_pair(&mut b, &mut c);
        orientation ^= sort_pair(&mut a, &mut b);
        let Point2 { x: ax, y: ay } = a;
        let Point2 { x: bx, y: by } = b;
        let Point2 { x: cx, y: cy } = c;
        tail.top = ay.clamp(0, 200) as u8;
        tail.middle = by.clamp(0, 200) as u8;
        tail.bottom = cy.clamp(0, 200) as u8;
        tail.color_flags = triangle.color
            | orientation
            | (triangle.reserved & (crate::TERRAIN_CLEAR_GEOMETRY | crate::EXTERIOR_TERRAIN));
        (head.long_x, head.long_step) = prepare_edge(ax, ay, cx, cy);
        (head.short_x, head.short_step) = prepare_edge(ax, ay, bx, by);
        (tail.lower_x, tail.lower_step) = prepare_edge(bx, by, cx, cy);
    }
}

#[cfg(not(target_arch = "mos"))]
struct Point2 {
    x: i16,
    y: i16,
}

#[inline(never)]
#[cfg(not(target_arch = "mos"))]
fn sort_pair(a: &mut Point2, b: &mut Point2) -> u8 {
    if a.y > b.y || (a.y == b.y && a.x > b.x) {
        let x = a.x;
        let y = a.y;
        a.x = b.x;
        a.y = b.y;
        b.x = x;
        b.y = y;
        16
    } else {
        0
    }
}

#[cfg(not(target_arch = "mos"))]
fn seek_row(head: &mut RasterHead, tail: &RasterTail, row: u8) {
    let row = row.max(tail.top).min(tail.bottom);
    head.long_x += head.long_step * (row - tail.top) as i32;
    if row >= tail.middle {
        head.short_x = tail.lower_x + tail.lower_step * (row - tail.middle) as i32;
        head.short_step = tail.lower_step;
    } else {
        head.short_x += head.short_step * (row - tail.top) as i32;
    }
}

/// Walk a triangle's rows within one band, reporting every nonempty span as
/// (band row, first, end, color_flags) in top-to-bottom order.
#[cfg(not(target_arch = "mos"))]
fn raster_rows(
    head: &mut RasterHead,
    tail: &RasterTail,
    band_y: u8,
    mut paint: impl FnMut(usize, usize, usize, u8),
) {
    for y in tail.top.max(band_y)..tail.bottom.min(band_y + 8) {
        if y == tail.middle {
            head.short_x = tail.lower_x;
            head.short_step = tail.lower_step;
        }
        let (left, right) = if tail.color_flags & 16 != 0 {
            (head.long_x, head.short_x)
        } else {
            (head.short_x, head.long_x)
        };
        let first = ((left + 32767) >> 16).clamp(0, WIDTH as i32) as usize;
        let end = ((right + 32767) >> 16).clamp(0, WIDTH as i32) as usize;
        if first < end {
            paint((y - band_y) as usize, first, end, tail.color_flags);
        }
        head.long_x += head.long_step;
        head.short_x += head.short_step;
    }
}

#[cfg(not(target_arch = "mos"))]
fn paint_reference_span(
    pixels: &mut [u8; BAND_PIXELS],
    row: usize,
    first: usize,
    end: usize,
    color_flags: u8,
    coverage_seen: &mut bool,
) {
    let offset = row * WIDTH;
    let span = &mut pixels[offset + first..offset + end];
    let color = color_flags & 15;
    if color_flags & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
        *coverage_seen = true;
        span.fill(color | TERRAIN_CLEAR_INK);
    } else if !*coverage_seen {
        span.fill(color);
    } else if color_flags & crate::EXTERIOR_TERRAIN != 0 {
        for pixel in span {
            if *pixel & TERRAIN_CLEAR_INK == 0 {
                *pixel = color;
            }
        }
    } else {
        for pixel in span {
            *pixel = (*pixel & TERRAIN_CLEAR_INK) | color;
        }
    }
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

/// Convert one 160x8 band into forty VIC-II multicolor cells. The global
/// background (`$D021`) shows the frame's clear colour; each cell supplies
/// screen high/low nibbles and one color RAM nibble for its other colours.
/// Output bitmap bytes are cell-major, matching the native VIC bitmap layout.
#[inline(never)]
#[cfg(not(target_arch = "mos"))]
pub fn pack_band(
    pixels: &[u8; BAND_PIXELS],
    previous_screen: &[u8; 40],
    previous_color: &[u8; 40],
    background: u8,
    bitmap: &mut [u8; 320],
    screen: &mut [u8; 40],
    colors: &mut [u8; 40],
) {
    for cell in 0..40 {
        let (cell_bitmap, cell_screen, cell_color) = pack_cell(
            pixels,
            cell,
            [
                previous_screen[cell] >> 4,
                previous_screen[cell] & 15,
                previous_color[cell] & 15,
            ],
            background,
        );
        bitmap[cell * 8..cell * 8 + 8].copy_from_slice(&cell_bitmap);
        screen[cell] = cell_screen;
        colors[cell] = cell_color;
    }
}

/// Quantize one cell of a pixel band: the per-pixel reference the cell-direct
/// rasterizer falls back to for cells with more than three colours besides the
/// background. `old` holds the cell's preceding-frame slot colors (screen
/// high, screen low, color RAM); `background` is the frame's clear colour,
/// which code 0 shows and which therefore needs no slot. Unused slots hold the
/// background colour. Returns the eight bitmap bytes, screen byte and color
/// nibble.
#[cfg(not(target_arch = "mos"))]
pub fn pack_cell(
    pixels: &[u8; BAND_PIXELS],
    cell: usize,
    old: [u8; 3],
    background: u8,
) -> ([u8; 8], u8, u8) {
    let background = background & 15;
    let mut bitmap = [0u8; 8];
    {
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
        // Large stretches of sky, hull, and terrain need no quantization.
        if present & present.wrapping_sub(1) == 0 {
            let color = pixels[cell * 4] & 15;
            let mut slot = 0;
            if color != background {
                slot = 1;
                for i in 0..3 {
                    if old[i] == color {
                        slot = i + 1;
                        break;
                    }
                }
            }
            bitmap.fill(slot as u8 * 0x55);
            let mut palette = [background; 4];
            palette[slot] = color;
            return (bitmap, (palette[1] << 4) | palette[2], palette[3]);
        }
        // The background marks "no colour" below: it is never selected.
        let mut selected = [background; 3];
        let mut selected_bits = 0u16;
        for chosen in &mut selected {
            let mut best = background;
            let mut best_score = 0;
            for color in 0..16 {
                if color == background as usize
                    || counts[color] == 0
                    || selected_bits & (1 << color) != 0
                {
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
            if best != background {
                selected_bits |= 1 << best;
            }
        }
        // Retain palette slots when a preceding-frame color is still selected.
        let mut palette = [background; 4];
        let mut assigned = 0u16;
        for slot in 0..3 {
            if selected_bits & (1 << old[slot]) != 0 && assigned & (1 << old[slot]) == 0 {
                palette[slot + 1] = old[slot];
                assigned |= 1 << old[slot];
            }
        }
        for chosen in selected {
            if chosen == background || assigned & (1 << chosen) != 0 {
                continue;
            }
            for slot in &mut palette[1..] {
                if *slot == background {
                    *slot = chosen;
                    assigned |= 1 << chosen;
                    break;
                }
            }
        }
        let mut map = [0u8; 16];
        for color in 0..16 {
            if color == background as usize || counts[color] == 0 {
                continue;
            }
            let mut best_distance = COLOR_DISTANCE[color][background as usize];
            for slot in 1..4 {
                let distance = COLOR_DISTANCE[color][palette[slot] as usize];
                if distance < best_distance {
                    map[color] = slot as u8;
                    best_distance = distance;
                }
            }
        }
        for (row, byte) in bitmap.iter_mut().enumerate() {
            let mut packed = 0;
            for column in 0..4 {
                packed =
                    (packed << 2) | map[(pixels[row * WIDTH + cell * 4 + column] & 15) as usize];
            }
            *byte = packed;
        }
        (bitmap, (palette[1] << 4) | palette[2], palette[3])
    }
}

/// A band-local drawing surface: eight rows of 160 pixels. Colours carry the
/// VIC colour in bits 0-3 and `HUD_INK` in bit 7 (priority ink for the
/// quantizer). The classic pixel band, the cell-direct band and the native
/// cell kernels all implement it, so HUD and text code is written once.
pub trait Canvas {
    /// Paint `[first, end)` of band row `row`.
    fn span(&mut self, row: usize, first: usize, end: usize, color: u8);
    /// Paint one pixel.
    fn plot(&mut self, row: usize, x: usize, color: u8);
    /// Paint a 3x5 glyph whose top-left pixel is (`x`, `row`); all five rows
    /// and three columns lie inside the band.
    fn glyph(&mut self, row: usize, x: usize, rows: &[u8; 5], color: u8) {
        for (dy, &bits) in rows.iter().enumerate() {
            if bits & 4 != 0 {
                self.plot(row + dy, x, color);
            }
            if bits & 2 != 0 {
                self.plot(row + dy, x + 1, color);
            }
            if bits & 1 != 0 {
                self.plot(row + dy, x + 2, color);
            }
        }
    }
}

/// The classic pixel band: one byte per pixel, quantized by `pack_band`.
#[cfg(not(target_arch = "mos"))]
impl Canvas for [u8; BAND_PIXELS] {
    #[inline(always)]
    fn span(&mut self, row: usize, first: usize, end: usize, color: u8) {
        fill_color(&mut self[row * WIDTH + first..row * WIDTH + end], color);
    }
    #[inline(always)]
    fn plot(&mut self, row: usize, x: usize, color: u8) {
        self[row * WIDTH + x] = color;
    }
    #[inline(always)]
    fn glyph(&mut self, row: usize, x: usize, rows: &[u8; 5], color: u8) {
        let mut offset = row * WIDTH + x;
        for &bits in rows {
            if bits & 4 != 0 {
                self[offset] = color;
            }
            if bits & 2 != 0 {
                self[offset + 1] = color;
            }
            if bits & 1 != 0 {
                self[offset + 2] = color;
            }
            offset += WIDTH;
        }
    }
}

/// Draw a compact 3x5 uppercase/digit font, with a one-pixel advance gap.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw_text(band: u8, canvas: &mut impl Canvas, x: i16, y: i16, text: &[u8], color: u8) {
    let row = y.wrapping_sub(band as i16 * 8);
    if band >= 25
        || row as u16 > 3
        || x < 0
        || x > WIDTH as i16 - 3
        || text.len() > (WIDTH + 1 - x as usize) / 4
    {
        draw_text_scaled(band, canvas, x, y, text, color, 1);
        return;
    }
    // Most cockpit labels fit wholly within one band. Check their bounds once
    // instead of repeating scaled-text clipping for every glyph pixel.
    let ink = (color & 15) | HUD_INK;
    let mut cursor = x as usize;
    for &character in text {
        canvas.glyph(row as usize, cursor, glyph(character), ink);
        cursor += 4;
    }
}

/// Integer scaling for title lettering; ordinary HUD text uses scale one.
/// Each lit glyph pixel becomes a `scale`-wide span on `scale` rows.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw_text_scaled(
    band: u8,
    canvas: &mut impl Canvas,
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
    let ink = (color & 15) | HUD_INK;
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
                        let first = glyph_x.clamp(0, WIDTH as i16) as usize;
                        let end = glyph_x.saturating_add(scale).clamp(0, WIDTH as i16) as usize;
                        if first < end {
                            canvas.span((screen_y - band_y) as usize, first, end, ink);
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
    canvas: &mut impl Canvas,
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
    // Sixteen-bit arithmetic: every caller keeps width × maximum below 65,536
    // (the 152-pixel boss bar uses a maximum of 100).
    debug_assert!(width as u32 * maximum as u32 <= u16::MAX as u32);
    let fill = if maximum == 0 {
        0
    } else {
        (width as u16 * value.min(maximum) / maximum) as i16
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
        let band_row = (row - band_y) as usize;
        if left < split {
            canvas.span(band_row, left, split, color & 15 | HUD_INK);
        }
        if split < right {
            canvas.span(band_row, split, right, 11);
        }
    }
}

/// Fill a pixel slice; the classic pixel band is host-only now that the
/// target paints cells directly.
#[cfg(not(target_arch = "mos"))]
pub(crate) fn fill_color(pixels: &mut [u8], color: u8) {
    pixels.fill(color);
}

// Glyph bytes are read only while PIXELS is loaded.
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
pub(crate) fn glyph(c: u8) -> &'static [u8; 5] {
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
    fn compact_text_matches_scaled_reference_for_glyphs_clipping_and_bands() {
        for band in [0, 1, 12, 23, 24, 25, 255] {
            for row in -5..=8 {
                let y = band as i16 * 8 + row;
                for x in [-4, -1, 0, 1, 23, 48, 145, 154, 157, 158, 159, 160] {
                    for character in 0..=255u8 {
                        let text = [character; 3];
                        let mut compact = [6; BAND_PIXELS];
                        let mut reference = compact;
                        draw_text(band, &mut compact, x, y, &text, character);
                        draw_text_scaled(band, &mut reference, x, y, &text, character, 1);
                        assert_eq!(
                            compact, reference,
                            "band {band}, x {x}, y {y}, glyph {character}"
                        );
                    }
                }
            }
        }
        for text in [
            b"".as_slice(),
            b"LOWLAND APPROACH",
            &[b'W'; 40],
            &[b'W'; 41],
        ] {
            for x in [0, 1, 2, 48, 157] {
                let mut compact = [11; BAND_PIXELS];
                let mut reference = compact;
                draw_text(0, &mut compact, x, 2, text, 0xff);
                draw_text_scaled(0, &mut reference, x, 2, text, 0xff, 1);
                assert_eq!(compact, reference);
            }
        }
    }
    fn clipped_scenery(sink: &mut impl GeometrySink, index: u8) {
        sink.triangle(
            Vec3 {
                x: 0,
                y: 12,
                z: NEAR / 2,
            },
            Vec3 {
                x: 12,
                y: -12,
                z: 64,
            },
            Vec3 {
                x: -12,
                y: -12,
                z: 64,
            },
            index & 15,
            0,
        );
    }
    fn tied_foreground(sink: &mut impl GeometrySink, index: u8) {
        let z = if index & 1 == 0 { 42 } else { 53 };
        sink.triangle(
            Vec3 { x: -8, y: -8, z },
            Vec3 { x: 0, y: 8, z },
            Vec3 { x: 8, y: -8, z },
            index.wrapping_add(7) & 15,
            0,
        );
    }
    #[test]
    fn staged_scenery_preserves_unread_records_and_foreground_tie_order() {
        for count in [0, 1, 2, 16, 95, 96] {
            let mut direct = Renderer::new();
            let mut staged = Renderer::new();
            direct.begin(6);
            staged.begin(6);
            for index in 0..64 {
                tied_foreground(&mut direct, index);
            }
            for index in 0..count {
                clipped_scenery(&mut direct, index);
            }
            let raw_count = {
                let mut stage = staged.scenery_stage();
                for index in 0..count {
                    clipped_scenery(&mut stage, index);
                }
                stage.count()
            };
            assert_eq!(raw_count, count);
            let prefix = staged.project_scenery(raw_count, 0, 0);
            assert_eq!(prefix, count as u16 * 2, "every staged input splits");
            for index in 0..64 {
                tied_foreground(&mut staged, index);
            }
            staged.scenery_last(prefix);
            direct.finish();
            staged.finish();
            assert_eq!(staged.triangle_count, 64 + count as u16 * 2);
            assert_eq!((direct.dropped, staged.dropped), (0, 0));
            for (a, b) in direct.triangles[..direct.triangle_count as usize]
                .iter()
                .zip(&staged.triangles[..staged.triangle_count as usize])
            {
                assert_eq!(
                    (a.x, a.y, a.depth, a.color, a.reserved),
                    (b.x, b.y, b.depth, b.color, b.reserved)
                );
            }
            for band in 0..25 {
                let mut expected = [0; BAND_PIXELS];
                let mut actual = [0; BAND_PIXELS];
                direct.render_band(band, &mut expected);
                staged.render_band(band, &mut actual);
                assert_eq!(actual, expected);
            }
        }
    }
    #[test]
    fn scenery_stage_has_a_checked_ninety_six_record_limit() {
        let mut renderer = Renderer::new();
        let count = {
            let mut stage = renderer.scenery_stage();
            for index in 0..97 {
                clipped_scenery(&mut stage, index);
            }
            stage.count()
        };
        assert_eq!(count, SCENERY_CAPACITY as u8);
        assert_eq!(renderer.dropped, 1);
        assert_eq!(renderer.project_scenery(count, 0, 0), 192);
        // Starting another frame retires both the commands and overflow state.
        renderer.begin(6);
        assert_eq!(renderer.dropped, 0);
        assert_eq!(renderer.scenery_stage().count(), 0);
    }
    #[test]
    fn replayed_scenery_drifts_only_backdrop_records_by_the_sky_parallax() {
        use crate::game::STATIC_SKY;
        let far = |x: i16, y: i16| Vec3 { x, y, z: 2350 };
        let command = |renderer: &Renderer, index: usize| {
            let t = &renderer.triangles[index];
            (t.x, t.y, t.depth, t.color, t.reserved)
        };
        let mut plain = None;
        for (dx, dy) in [(0, 0), (100, 32), (-100, -32), (37, -5)] {
            let mut direct = Renderer::new();
            let mut staged = Renderer::new();
            direct.begin(6);
            staged.begin(6);
            {
                let mut stage = staged.scenery_stage();
                stage.triangle(far(-800, 300), far(0, 900), far(800, 300), 14, STATIC_SKY);
                // The depth modifier alone must never make scenery drift.
                stage.triangle(
                    far(-800, -300),
                    far(0, 200),
                    far(800, -300),
                    5,
                    DOUBLE_SKY_DEPTH,
                );
                assert_eq!(stage.count(), 2);
            }
            direct.triangle(
                far(-800 - dx, 300 - dy),
                far(-dx, 900 - dy),
                far(800 - dx, 300 - dy),
                14,
                STATIC_SKY,
            );
            direct.triangle(far(-800, -300), far(0, 200), far(800, -300), 5, 0);
            assert_eq!(staged.project_scenery(2, dx, dy), 2);
            direct.finish();
            staged.finish();
            for index in 0..2 {
                assert_eq!(
                    command(&direct, index),
                    command(&staged, index),
                    "parallax {dx},{dy}"
                );
            }
            let reference = *plain.get_or_insert((command(&staged, 0), command(&staged, 1)));
            // Backdrop moves against the pan; ordinary scenery never moves.
            assert_eq!(command(&staged, 0) == reference.0, (dx, dy) == (0, 0));
            assert_eq!(command(&staged, 1), reference.1);
        }
    }
    #[test]
    fn doubled_sky_depth_preserves_projected_shape_and_parallax() {
        use crate::game::STATIC_SKY;
        for (dx, dy) in [(0, 0), (100, 32), (-100, -32), (37, -5)] {
            for depth in [2350, 3072, 3584, 4064] {
                let mut original = Renderer::new();
                let mut distant = Renderer::new();
                for (renderer, scale, flags) in [
                    (&mut original, 1, STATIC_SKY),
                    (&mut distant, 2, STATIC_SKY | DOUBLE_SKY_DEPTH),
                ] {
                    let points = [(-800, 300), (0, 900), (800, 300)].map(|(x, y)| Vec3 {
                        x: x * scale,
                        y: y * scale,
                        z: depth * scale,
                    });
                    renderer
                        .scenery_stage()
                        .triangle(points[0], points[1], points[2], 14, flags);
                    assert_eq!(renderer.project_scenery(1, dx, dy), 1);
                }
                let a = original.triangles[0];
                let b = distant.triangles[0];
                assert_eq!(
                    (a.x, a.y, a.color, a.reserved),
                    (b.x, b.y, b.color, b.reserved)
                );
                assert_eq!(b.depth, a.depth * 2);
                assert_eq!(
                    b.reserved, STATIC_SKY,
                    "depth modifier retires after projection"
                );
            }
        }
    }
    #[test]
    #[should_panic]
    fn scenery_rotation_rejects_a_corrupted_public_command_count() {
        let mut renderer = Renderer::new();
        renderer.triangle_count = TRIANGLE_CAPACITY as u16 + 1;
        renderer.scenery_last(1);
    }
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
    fn terrain_clear_coverage_survives_foreground_and_only_blocks_exterior_terrain() {
        for foreground_color in [0, 8] {
            let mut renderer = Renderer::new();
            let mut add = |depth, color, flags| {
                renderer.triangle(
                    Vec3 {
                        x: -40,
                        y: -40,
                        z: depth,
                    },
                    Vec3 {
                        x: 0,
                        y: 40,
                        z: depth,
                    },
                    Vec3 {
                        x: 40,
                        y: -40,
                        z: depth,
                    },
                    color,
                    flags,
                );
            };
            add(200, 1, crate::TERRAIN_CLEAR_GEOMETRY);
            add(150, foreground_color, 0);
            // Deliberately misleading terrain key, matching the clipped
            // terrain failure: the protection must outlive the hull color.
            add(100, 5, crate::EXTERIOR_TERRAIN);
            renderer.finish();
            let mut expected = [[0; BAND_PIXELS]; 25];
            for band in 0..25 {
                renderer.render_band(band, &mut expected[band as usize]);
            }
            assert_eq!(
                expected[12][4 * WIDTH + 80],
                foreground_color | TERRAIN_CLEAR_INK
            );
            assert!(expected.iter().flatten().any(|&pixel| pixel == 5));
            let mut pass = renderer.raster_pass().unwrap();
            let mut pixels = [0; BAND_PIXELS];
            for band in 0..25 {
                pass.next_band(&mut pixels, true);
                assert_eq!(pixels, expected[band as usize]);
            }
            // Normal geometry can still cover the same protected pixel.
            renderer.begin(0);
            renderer.triangle(
                Vec3 {
                    x: -40,
                    y: -40,
                    z: 200,
                },
                Vec3 {
                    x: 0,
                    y: 40,
                    z: 200,
                },
                Vec3 {
                    x: 40,
                    y: -40,
                    z: 200,
                },
                1,
                crate::TERRAIN_CLEAR_GEOMETRY,
            );
            renderer.triangle(
                Vec3 {
                    x: -40,
                    y: -40,
                    z: 100,
                },
                Vec3 {
                    x: 0,
                    y: 40,
                    z: 100,
                },
                Vec3 {
                    x: 40,
                    y: -40,
                    z: 100,
                },
                5,
                0,
            );
            renderer.finish();
            renderer.render_band(12, &mut pixels);
            assert_eq!(pixels[4 * WIDTH + 80], 5 | TERRAIN_CLEAR_INK);
        }
    }

    #[test]
    fn clipping_and_preparation_retain_visibility_flags() {
        for flags in [crate::TERRAIN_CLEAR_GEOMETRY, crate::EXTERIOR_TERRAIN] {
            let mut renderer = Renderer::new();
            renderer.triangle(
                Vec3 {
                    x: -16,
                    y: -16,
                    z: NEAR / 2,
                },
                Vec3 { x: 0, y: 16, z: 64 },
                Vec3 {
                    x: 16,
                    y: -16,
                    z: 64,
                },
                2,
                flags,
            );
            assert_eq!(renderer.triangle_count, 2);
            for triangle in &renderer.triangles[..2] {
                assert_eq!(triangle.reserved & 0x60, flags);
                assert_eq!(prepare_triangle(triangle).1.color_flags & 0x60, flags);
            }
        }
    }

    #[test]
    fn terrain_clear_coverage_bit_does_not_change_packing_or_hud_priority() {
        let mut pixels = [0u8; BAND_PIXELS];
        for (i, pixel) in pixels.iter_mut().enumerate() {
            *pixel = (i % 16) as u8 | if i % 7 == 0 { HUD_INK } else { 0 };
        }
        let mut marked = pixels;
        for (i, pixel) in marked.iter_mut().enumerate() {
            if i % 3 != 0 {
                *pixel |= TERRAIN_CLEAR_INK;
            }
        }
        let pack = |input: &[u8; BAND_PIXELS]| {
            let (mut bitmap, mut screen, mut colors) = ([0; 320], [0; 40], [0; 40]);
            pack_band(
                input,
                &[0x12; 40],
                &[3; 40],
                0,
                &mut bitmap,
                &mut screen,
                &mut colors,
            );
            (bitmap, screen, colors)
        };
        assert_eq!(pack(&pixels), pack(&marked));
    }

    include!("../tools/raster-fixtures.inc.rs");
}
