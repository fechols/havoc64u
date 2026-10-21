//! The cell-direct band (the host reference of the native cell writer) shows
//! the same pixels as the classic pixel band quantized by `pack_band`, over
//! real frames with their sky colours as the VIC background.
use havoc::{
    assets::{AssetBank, Segment, SEGMENT_COUNT},
    cells::{unpack_band, CellBand},
    game::Game,
    hud,
    render::{pack_band, Renderer},
    space_dust::SpaceDustFrame,
};

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn frames() -> Vec<Game> {
    let mut games = vec![Game::new()];
    for section in 0..SEGMENT_COUNT {
        games.push(Game::preview_at(section, 5000));
        games.push(Game::preview_at(section, 11000));
    }
    games
}

#[test]
fn cell_band_shows_the_pixel_band_of_every_scene() {
    let mut bands = 0;
    let mut spills = 0;
    let mut backgrounds = std::collections::BTreeSet::new();
    for game in frames() {
        let models = baked_assets::model_bank(game.segment_index());
        let bank = AssetBank::new(&models).unwrap();
        let segment_data = baked_assets::segment_bank(game.segment_index());
        let segment = Segment::new(&segment_data).unwrap();
        let mut renderer = Renderer::new();
        renderer.begin(game.sky_color());
        game.render(&mut renderer, &bank, &segment);
        renderer.finish();
        let status = game.hud();
        let mut dust = SpaceDustFrame::EMPTY;
        let ambient = dust.prepare(status.section, status.mode, game.animation_ticks());
        let background = renderer.clear_color();
        backgrounds.insert(background);
        // Preceding attributes: slot retention must not change what is shown.
        let mut old_screen = [0x6cu8; 40];
        let mut old_color = [5u8; 40];
        for band in 0..25u8 {
            let mut pixels = [0u8; 1280];
            renderer.render_band_with_ambient(band, &mut pixels, ambient);
            hud::draw(band, &mut pixels, &status, 7);
            let (mut bitmap, mut screen, mut colors) = ([0u8; 320], [0u8; 40], [0u8; 40]);
            pack_band(&pixels, &old_screen, &old_color, background, &mut bitmap, &mut screen, &mut colors);
            let expected = unpack_band(&bitmap, &screen, &colors, background);

            let mut cells = CellBand::new();
            renderer.render_band_cells(band, &mut cells, ambient);
            hud::draw(band, &mut cells, &status, 7);
            let (mut cell_bitmap, mut cell_screen, mut cell_colors) = ([0u8; 320], [0u8; 40], [0u8; 40]);
            cells.finish(&old_screen, &old_color, &mut cell_bitmap, &mut cell_screen, &mut cell_colors);
            let actual = unpack_band(&cell_bitmap, &cell_screen, &cell_colors, background);
            if actual != expected {
                let first = (0..1280).find(|&i| actual[i] != expected[i]).unwrap();
                panic!(
                    "section {} band {band}: pixel {first} (row {}, x {}) shows {} in cells, {} in the pixel band (background {background})",
                    game.segment_index(),
                    first / 160,
                    first % 160,
                    actual[first],
                    expected[first]
                );
            }
            // The classic pixel path quantizes what the cells spilled: with the
            // sky as the free background, four distinct colours stay exact.
            for cell in 0..40 {
                let mut distinct = std::collections::BTreeSet::new();
                for row in 0..8 {
                    for column in 0..4 {
                        distinct.insert(pixels[row * 160 + cell * 4 + column] & 15);
                    }
                }
                distinct.remove(&background);
                if distinct.len() <= 3 {
                    for row in 0..8 {
                        for column in 0..4 {
                            let at = row * 160 + cell * 4 + column;
                            assert_eq!(actual[at], pixels[at] & 15, "band {band} cell {cell} is exact");
                        }
                    }
                }
            }
            old_screen = cell_screen;
            old_color = cell_colors;
            bands += 1;
            spills += cells.stats.overflow_cells;
        }
    }
    eprintln!("{bands} bands over {} backgrounds, {spills} spilled cells", backgrounds.len());
    assert!(bands >= 25 * 5);
    assert!(backgrounds.len() >= 3, "scenes should exercise several sky colours: {backgrounds:?}");
}
