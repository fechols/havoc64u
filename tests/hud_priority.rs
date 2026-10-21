use havoc::{
    cells::{CellBand, Paint},
    render::{BAND_PIXELS, HUD_INK, WIDTH},
};

// Pixel-band ABI: bit 6 records terrain-clear coverage.
const TERRAIN_CLEAR_INK: u8 = 0x40;

fn paint(cells: &mut CellBand, pixels: &mut [u8; BAND_PIXELS], row: usize, first: usize, end: usize, color: u8, hud: bool, mode: Paint) {
    cells.span(row, first, end, color, hud, mode);
    let ink = (color & 15) | if hud { HUD_INK } else { 0 };
    for pixel in &mut pixels[row * WIDTH + first..row * WIDTH + end] {
        match mode {
            Paint::Clear => *pixel = ink | TERRAIN_CLEAR_INK,
            Paint::Exterior if *pixel & TERRAIN_CLEAR_INK != 0 => {},
            Paint::Exterior => *pixel = ink,
            Paint::Plain if hud => *pixel = ink,
            Paint::Plain => *pixel = (*pixel & TERRAIN_CLEAR_INK) | ink,
        }
    }
}

fn force_spill(cells: &mut CellBand, pixels: &mut [u8; BAND_PIXELS], background: u8) {
    for row in 4..8 {
        paint(cells, pixels, row, 0, 12, background ^ (row as u8 - 3), false, Paint::Plain);
    }
}

#[test]
fn hud_and_terrain_metadata_are_per_pixel_before_and_after_spill() {
    for background in 0..16 {
        for early_spill in [false, true] {
            for mode in [Paint::Plain, Paint::Clear, Paint::Exterior] {
                for hud in [false, true] {
                    let mut cells = CellBand::new();
                    cells.clear(background);
                    let mut pixels = [background; BAND_PIXELS];
                    if early_spill {
                        force_spill(&mut cells, &mut pixels, background);
                    }
                    // Equal colors have different metadata in the same cell.
                    paint(&mut cells, &mut pixels, 0, 0, 12, background ^ 2, false, Paint::Clear);
                    paint(&mut cells, &mut pixels, 1, 0, 12, background ^ 2, false, Paint::Clear);
                    paint(&mut cells, &mut pixels, 0, 1, 11, background ^ 2, true, Paint::Plain);
                    // Include both protected and unprotected destinations, and
                    // background HUD ink (which has no palette slot).
                    paint(&mut cells, &mut pixels, 0, 2, 10, background, hud, mode);
                    paint(&mut cells, &mut pixels, 1, 1, 11, background, hud, mode);
                    paint(&mut cells, &mut pixels, 2, 0, 12, background ^ 3, true, Paint::Plain);
                    paint(&mut cells, &mut pixels, 2, 3, 9, background ^ 3, false, Paint::Plain);
                    paint(&mut cells, &mut pixels, 3, 0, 12, background, true, Paint::Clear);
                    paint(&mut cells, &mut pixels, 3, 1, 11, background ^ 4, true, Paint::Exterior);
                    force_spill(&mut cells, &mut pixels, background);
                    assert_eq!(cells.stats.overflow_cells, 3);
                    for row in 0..8 {
                        let range = row * WIDTH..row * WIDTH + 12;
                        assert_eq!(&cells.pixels[range.clone()], &pixels[range],
                            "background {background}, early {early_spill}, mode {mode:?}, HUD {hud}, row {row}");
                    }
                    cells.clear(background);
                    let mut cleared = [background; BAND_PIXELS];
                    force_spill(&mut cells, &mut cleared, background);
                    assert_eq!(&cells.pixels[..12], &cleared[..12], "clear removes both metadata nibbles");
                }
            }
        }
    }
}

#[cfg(feature = "span-buffer")]
#[test]
fn overwritten_hud_gauge_matches_between_painter_and_spans() {
    use havoc::{cells::unpack_band, render::{draw_bar, pack_band, Renderer}, GeometrySink, Vec3, TWO_SIDED};

    fn corner(x: i16, y: i16, z: i16) -> Vec3 {
        let wx = (-3000..=3000).find(|&wx| havoc::projection::project(Vec3 { x: wx, y: 0, z }).0 == x).unwrap();
        let wy = (-3000..=3000).find(|&wy| havoc::projection::project(Vec3 { x: 0, y: wy, z }).1 == y).unwrap();
        Vec3 { x: wx, y: wy, z }
    }
    fn rect(renderer: &mut Renderer, x0: i16, y0: i16, x1: i16, y1: i16, z: i16, color: u8) {
        let [a, b, c, d] = [corner(x0, y0, z), corner(x1, y0, z), corner(x1, y1, z), corner(x0, y1, z)];
        renderer.triangle(a, b, c, color, TWO_SIDED);
        renderer.triangle(a, c, d, color, TWO_SIDED);
    }

    let mut renderer = Renderer::new();
    renderer.begin(0);
    rect(&mut renderer, 80, 96, 84, 104, 1000, 5); // hidden color makes only the painter spill early
    rect(&mut renderer, 80, 96, 82, 97, 900, 14);
    rect(&mut renderer, 82, 96, 84, 97, 800, 1);
    rect(&mut renderer, 80, 97, 84, 100, 800, 1);
    rect(&mut renderer, 80, 100, 84, 104, 700, 2);
    renderer.finish();
    let mut painter = CellBand::new();
    let mut spans = CellBand::new();
    let mut pixels = [0; BAND_PIXELS];
    renderer.render_band_cells(12, &mut painter, None);
    renderer.render_band_span_cells(12, &mut spans, None);
    renderer.render_band(12, &mut pixels);
    assert_eq!((painter.stats.overflow_cells, spans.stats.overflow_cells), (1, 0));
    for cells in [&mut painter, &mut spans] {
        draw_bar(12, cells, 80, 96, 1, 1, 1, 1, 14);
        draw_bar(12, cells, 80, 96, 1, 1, 0, 1, 14);
    }
    draw_bar(12, &mut pixels, 80, 96, 1, 1, 1, 1, 14);
    draw_bar(12, &mut pixels, 80, 96, 1, 1, 0, 1, 14);
    assert_eq!(pixels[81], 14, "the adjacent blue pixel never carried HUD priority");
    for (screen, color) in [(0, 0), (0x6c, 5), (0xe2, 1), (0x21, 11)] {
        let old_screen = [screen; 40];
        let old_color = [color; 40];
        let (mut bitmap, mut screen, mut colors) = ([0; 320], [0; 40], [0; 40]);
        pack_band(&pixels, &old_screen, &old_color, 0, &mut bitmap, &mut screen, &mut colors);
        let expected = unpack_band(&bitmap, &screen, &colors, 0);
        for cells in [&painter, &spans] {
            cells.finish(&old_screen, &old_color, &mut bitmap, &mut screen, &mut colors);
            assert_eq!(unpack_band(&bitmap, &screen, &colors, 0), expected);
        }
    }
}
