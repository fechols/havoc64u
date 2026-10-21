#![cfg(all(feature = "span-buffer", feature = "host-tools"))]

use havoc::{
    assets::{AssetBank, Segment, SEGMENT_COUNT},
    cells::{unpack_band, CellBand},
    game::Game,
    hud,
    render::{Renderer, SpanStats, BAND_PIXELS},
    space_dust::SpaceDustFrame,
};

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn packed(cells: &CellBand, old_screen: &[u8; 40], old_color: &[u8; 40], background: u8)
    -> ([u8; BAND_PIXELS], [u8; 40], [u8; 40])
{
    let (mut bitmap, mut screen, mut colors) = ([0; 320], [0; 40], [0; 40]);
    cells.finish(old_screen, old_color, &mut bitmap, &mut screen, &mut colors);
    (unpack_band(&bitmap, &screen, &colors, background), screen, colors)
}

#[test]
fn every_scene_matches_raw_pixels_and_decoded_cells_with_identical_previous_palettes() {
    let mut totals = SpanStats::default();
    let mut bands = 0;
    let mut painter_spills = 0;
    let mut span_spills = 0;
    let mut painter_cell_rows = 0;
    let mut span_cell_rows = 0;
    for section in 0..SEGMENT_COUNT {
        for distance in [5000, 11000] {
            let game = Game::preview_at(section, distance);
            let models = baked_assets::model_bank(game.segment_index());
            let bank = AssetBank::new(&models).unwrap();
            let data = baked_assets::segment_bank(game.segment_index());
            let segment = Segment::new(&data).unwrap();
            let mut renderer = Renderer::new();
            renderer.begin(game.sky_color());
            game.render(&mut renderer, &bank, &segment);
            renderer.finish();
            let status = game.hud();
            let mut dust = SpaceDustFrame::EMPTY;
            let ambient = dust.prepare(status.section, status.mode, game.animation_ticks());
            for band in 0..25 {
                let mut expected = [0; BAND_PIXELS];
                renderer.render_band_with_ambient(band, &mut expected, ambient);
                let mut actual = [0; BAND_PIXELS];
                let stats = renderer.render_band_spans(band, &mut actual, ambient);
                assert_eq!(actual, expected, "section {section}, distance {distance}, band {band}");
                assert!(stats.emitted_pixels <= BAND_PIXELS as u32);
                totals.input_spans += stats.input_spans;
                totals.run_visits += stats.run_visits;
                totals.emitted_spans += stats.emitted_spans;
                totals.emitted_pixels += stats.emitted_pixels;
                totals.peak_runs = totals.peak_runs.max(stats.peak_runs);

                let mut painter = CellBand::new();
                let mut spans = CellBand::new();
                renderer.render_band_cells(band, &mut painter, ambient);
                renderer.render_band_span_cells(band, &mut spans, ambient);
                hud::draw(band, &mut painter, &status, 7);
                hud::draw(band, &mut spans, &status, 7);
                for (old_screen, old_color) in [([0; 40], [0; 40]), ([0x6c; 40], [5; 40])] {
                    let a = packed(&painter, &old_screen, &old_color, renderer.clear_color()).0;
                    let b = packed(&spans, &old_screen, &old_color, renderer.clear_color()).0;
                    assert_eq!(a, b, "decoded section {section}, distance {distance}, band {band}");
                }
                painter_spills += painter.stats.overflow_cells;
                span_spills += spans.stats.overflow_cells;
                painter_cell_rows += painter.stats.cell_rows;
                span_cell_rows += spans.stats.cell_rows;
                bands += 1;
            }
        }
    }
    eprintln!("span-buffer: {bands} scene bands; {totals:?}; spills painter={painter_spills}, spans={span_spills}; cell-row writes painter={painter_cell_rows}, spans={span_cell_rows}");
    assert!(bands >= 25 * 5);
}

#[test]
fn consecutive_scene_frames_report_independent_palette_history_differences() {
    // Compare both histories, not just a fixed palette seeded before every
    // frame. Historical unused slots can legitimately expose quantizer ties;
    // cross-packing each image with both histories proves any independent-
    // history difference comes only from palette selection, not raster/HUD ink.
    let mut changed_pixels = 0usize;
    let mut changed_bands = 0usize;
    let mut frames = 0;
    let mut first_difference = None;
    for section in 0..SEGMENT_COUNT {
        let mut game = Game::preview_at(section, 5000);
        let models = baked_assets::model_bank(game.segment_index());
        let bank = AssetBank::new(&models).unwrap();
        let data = baked_assets::segment_bank(game.segment_index());
        let segment = Segment::new(&data).unwrap();
        let mut painter_screen = [[0x6c; 40]; 25];
        let mut painter_color = [[5; 40]; 25];
        let mut span_screen = painter_screen;
        let mut span_color = painter_color;
        for frame in 0..8 {
            let mut renderer = Renderer::new();
            renderer.begin(game.sky_color());
            game.render(&mut renderer, &bank, &segment);
            renderer.finish();
            let status = game.hud();
            let mut dust = SpaceDustFrame::EMPTY;
            let ambient = dust.prepare(status.section, status.mode, game.animation_ticks());
            for band in 0..25 {
                let mut painter_pixels = [0; BAND_PIXELS];
                let mut span_pixels = [0; BAND_PIXELS];
                renderer.render_band_with_ambient(band, &mut painter_pixels, ambient);
                renderer.render_band_spans(band, &mut span_pixels, ambient);
                assert_eq!(span_pixels, painter_pixels,
                    "raw history frame section {section}, frame {frame}, band {band}");
                let mut a = CellBand::new();
                let mut b = CellBand::new();
                renderer.render_band_cells(band, &mut a, ambient);
                renderer.render_band_span_cells(band, &mut b, ambient);
                hud::draw(band, &mut a, &status, 7);
                hud::draw(band, &mut b, &status, 7);
                let index = band as usize;
                let (pa, sa, ca) = packed(&a, &painter_screen[index], &painter_color[index], renderer.clear_color());
                let (pb, sb, cb) = packed(&b, &span_screen[index], &span_color[index], renderer.clear_color());
                let span_with_painter_history = packed(&b, &painter_screen[index], &painter_color[index], renderer.clear_color()).0;
                let painter_with_span_history = packed(&a, &span_screen[index], &span_color[index], renderer.clear_color()).0;
                assert_eq!(pa, span_with_painter_history,
                    "painter history section {section}, frame {frame}, band {band}");
                assert_eq!(pb, painter_with_span_history,
                    "span history section {section}, frame {frame}, band {band}");
                let differences = pa.iter().zip(&pb).filter(|(a, b)| a != b).count();
                if differences != 0 {
                    changed_pixels += differences;
                    changed_bands += 1;
                    first_difference.get_or_insert((section, frame, band, differences));
                }
                painter_screen[index] = sa;
                painter_color[index] = ca;
                span_screen[index] = sb;
                span_color[index] = cb;
            }
            game.tick(&segment, &bank);
            frames += 1;
        }
    }
    eprintln!("span-buffer history diagnostic: {frames} frames, {changed_pixels} changed decoded pixels in {changed_bands} bands; first={first_difference:?}");
    assert_eq!(frames, SEGMENT_COUNT as usize * 8);
}

#[test]
fn unused_palette_slots_can_change_a_later_equal_frequency_quantizer_tie() {
    let mut painter = CellBand::new();
    let mut spans = CellBand::new();
    painter.clear(0);
    spans.clear(0);
    for row in 0..8 {
        painter.geometry_span(row, 0, 4, 8);
    }
    for row in 0..8 {
        painter.geometry_span(row, 0, 4, 9);
        spans.geometry_span(row, 0, 4, 9);
    }
    let (a, painter_screen, painter_color) = packed(&painter, &[0; 40], &[0; 40], 0);
    let (b, span_screen, span_color) = packed(&spans, &[0; 40], &[0; 40], 0);
    assert_eq!(a, b, "the first frame is visually identical");
    painter.clear(0);
    spans.clear(0);
    for row in 0..8 {
        let color = [1, 2, 3, 8][row / 2];
        painter.geometry_span(row, 0, 4, color);
        spans.geometry_span(row, 0, 4, color);
    }
    assert_eq!(painter.pixels, spans.pixels, "the later unquantized image is identical");
    let a = packed(&painter, &painter_screen, &painter_color, 0).0;
    let b = packed(&spans, &span_screen, &span_color, 0).0;
    assert_ne!(a, b, "unused previous palette colors affect equal-frequency selection");
}
