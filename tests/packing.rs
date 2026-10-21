use havoc::render::pack_band;

const CLOUD_PALETTES: [[u8; 3]; 10] = [
    [6, 1, 15],
    [8, 7, 10],
    [11, 15, 12],
    [4, 10, 14],
    [0, 3, 11],
    [11, 8, 0],
    [8, 15, 12],
    [11, 15, 3],
    [6, 1, 15],
    [4, 15, 10],
];
const HORIZON_PALETTES: [[u8; 3]; 10] = [
    [6, 14, 12],
    [8, 9, 2],
    [11, 6, 12],
    [4, 6, 11],
    [0, 11, 6],
    [11, 8, 0],
    [8, 9, 12],
    [11, 12, 15],
    [6, 11, 14],
    [4, 10, 12],
];
const CAVE_PALETTE: [u8; 4] = [0, 6, 3, 11];

fn band_pattern(palette: &[u8], distribution: usize) -> [u8; 1280] {
    let mut pixels = [0; 1280];
    for row in 0..8 {
        for cell in 0..40 {
            for col in 0..4 {
                let at = row * 4 + col;
                let index = if distribution == 0 { at } else { at * at / 7 };
                pixels[row * 160 + cell * 4 + col] = palette[index % palette.len()];
            }
        }
    }
    pixels
}

fn background_pattern(theme: usize, distribution: usize) -> [u8; 1280] {
    if theme == 4 {
        // The black cave interior can put cyan crystals, blue walls, and gray
        // facets in the same cell. Exercise that complete union as well.
        return band_pattern(&CAVE_PALETTE, distribution);
    }
    let mut pixels = band_pattern(&CLOUD_PALETTES[theme], distribution);
    let horizon = band_pattern(&HORIZON_PALETTES[theme], distribution);
    // Exercise more than three nonblack colors in one band while preserving
    // the actual requirement of at most three in each 4x8 cell.
    for row in 0..8 {
        pixels[row * 160 + 80..(row + 1) * 160]
            .copy_from_slice(&horizon[row * 160 + 80..(row + 1) * 160]);
    }
    pixels
}

#[test]
fn crystal_cave_background_union_fits_three_nonblack_cell_slots() {
    let mut actual: Vec<_> = CLOUD_PALETTES[4]
        .into_iter()
        .chain(HORIZON_PALETTES[4])
        .collect();
    actual.sort_unstable();
    actual.dedup();
    let mut expected = CAVE_PALETTE;
    expected.sort_unstable();
    assert_eq!(actual, expected);
    assert_eq!(actual.iter().filter(|&&color| color != 0).count(), 3);
}

fn unpack_band(bitmap: &[u8; 320], screen: &[u8; 40], color: &[u8; 40]) -> [u8; 1280] {
    let mut pixels = [0; 1280];
    for cell in 0..40 {
        let palette = [0, screen[cell] >> 4, screen[cell] & 15, color[cell] & 15];
        for row in 0..8 {
            for column in 0..4 {
                let slot = (bitmap[cell * 8 + row] >> (6 - column * 2)) & 3;
                pixels[row * 160 + cell * 4 + column] = palette[slot as usize];
            }
        }
    }
    pixels
}

#[test]
fn ashen_scenery_preserves_isolated_red_shots_at_every_cell_position_and_old_palette() {
    // Black is the global VIC-II color: orange and charcoal leave the third
    // per-cell slot free for a red enemy or shot, even when it covers one pixel.
    assert_eq!(CLOUD_PALETTES[5], [11, 8, 0]);
    assert_eq!(HORIZON_PALETTES[5], [11, 8, 0]);
    for shot in [2, 10] {
        for distribution in 0..2 {
            for position in 0..32 {
                let mut pixels = background_pattern(5, distribution);
                for cell in 0..40 {
                    pixels[position / 4 * 160 + cell * 4 + position % 4] = shot;
                }
                // Include cold black palettes, retained scenery/shot colors,
                // duplicates, and every unrelated preceding scene palette.
                for base in (0..4096).step_by(40) {
                    let mut previous_screen = [0; 40];
                    let mut previous_color = [0; 40];
                    for cell in 0..40 {
                        let old = (base + cell).min(4095);
                        previous_screen[cell] = (old >> 4) as u8;
                        previous_color[cell] = (old & 15) as u8;
                    }
                    let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
                    pack_band(
                        &pixels,
                        &previous_screen,
                        &previous_color,
                        0,
                        &mut bitmap,
                        &mut screen,
                        &mut color,
                    );
                    assert_eq!(
                        unpack_band(&bitmap, &screen, &color),
                        pixels,
                        "shot {shot}, distribution {distribution}, position {position}, old base {base}"
                    );
                    let (mut retained_bitmap, mut retained_screen, mut retained_color) =
                        ([0; 320], [0; 40], [0; 40]);
                    pack_band(
                        &pixels,
                        &screen,
                        &color,
                        0,
                        &mut retained_bitmap,
                        &mut retained_screen,
                        &mut retained_color,
                    );
                    assert_eq!(
                        (retained_bitmap, retained_screen, retained_color),
                        (bitmap, screen, color),
                        "retained shot {shot}, distribution {distribution}, position {position}, old base {base}"
                    );
                }
            }
        }
    }
}

#[test]
fn screen_bytes_determine_all_packed_background_bytes_for_every_previous_palette() {
    // Every subset, with and without black, covers uniform/absent colors; all
    // 4096 previous palettes include duplicate slots and unrelated old colors.
    for (palette_index, [sky, top, shade]) in CLOUD_PALETTES
        .into_iter()
        .chain(HORIZON_PALETTES)
        .chain([[6, 3, 11]])
        .enumerate()
    {
        for subset in 1..16 {
            let palette: Vec<_> = [0, sky, top, shade]
                .into_iter()
                .enumerate()
                .filter_map(|(bit, color)| (subset & (1 << bit) != 0).then_some(color))
                .collect();
            for distribution in 0..2 {
                let pixels = band_pattern(&palette, distribution);
                let mut by_screen: [Option<([u8; 8], u8)>; 256] = [None; 256];
                for base in (0..4096).step_by(40) {
                    let mut previous_screen = [0; 40];
                    let mut previous_color = [0; 40];
                    for cell in 0..40 {
                        let old = (base + cell).min(4095);
                        previous_screen[cell] = (old >> 4) as u8;
                        previous_color[cell] = (old & 15) as u8;
                    }
                    let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
                    pack_band(
                        &pixels,
                        &previous_screen,
                        &previous_color,
                        0,
                        &mut bitmap,
                        &mut screen,
                        &mut color,
                    );
                    for cell in 0..40 {
                        let bytes = bitmap[cell * 8..cell * 8 + 8].try_into().unwrap();
                        let result = (bytes, color[cell]);
                        let known = &mut by_screen[screen[cell] as usize];
                        if let Some(expected) = known {
                            assert_eq!(
                                *expected,
                                result,
                                "background palette {palette_index}, subset {subset}, old palette {}",
                                base + cell
                            );
                        } else {
                            *known = Some(result);
                        }
                    }
                    let (mut repeated_bitmap, mut repeated_screen, mut repeated_color) =
                        ([0; 320], [0; 40], [0; 40]);
                    pack_band(
                        &pixels,
                        &screen,
                        &color,
                        0,
                        &mut repeated_bitmap,
                        &mut repeated_screen,
                        &mut repeated_color,
                    );
                    assert_eq!(
                        (bitmap, screen, color),
                        (repeated_bitmap, repeated_screen, repeated_color),
                        "background palette {palette_index}, subset {subset}, palette base {base}"
                    );
                }
            }
        }
    }
}
