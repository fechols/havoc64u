use havoc::{
    game::{Hud, Mode},
    hud_sprites::{render_corner, DisplayState, CORNER_POSITIONS},
    render::{draw_text, HUD_INK},
};

fn hud(mode: Mode) -> Hud {
    Hud {
        mode,
        story_mode: false,
        shield: 75,
        charge: 128,
        bombs: 2,
        lives: 3,
        score: 12345,
        distance: 1000,
        section: 0,
        roll_ticks: 0,
        boss_shield: 0,
        boss_kind: 0,
        victory_ticks: 0,
    }
}

fn image(corner: u8, state: &DisplayState) -> [u8; 128] {
    let mut output = [0xff; 128];
    render_corner(corner, state, &mut output);
    assert_eq!(output[63], 0);
    assert_eq!(output[127], 0);
    for y in 0..21 {
        for x in 0..24 {
            let value = pixel(&output, x, y);
            // Grey is functional ink only: spent bombs or empty gauge space.
            let bomb = corner == 1 && (9..14).contains(&y) && x % 8 != 0 && x % 8 != 7;
            let gauge = match corner {
                1 => (16..19).contains(&y) && (7..22).contains(&x),
                2 => (10..13).contains(&y) && (1..23).contains(&x),
                3 => (12..15).contains(&y) && (1..23).contains(&x),
                _ => false,
            };
            if !bomb && !gauge {
                assert_ne!(value, 1);
            }
            if y < 2 || y == 20 {
                assert_eq!(value, 0, "decorative pixel at {corner}:{x},{y}");
            }
        }
    }
    output
}

fn pixel(image: &[u8; 128], x: usize, y: usize) -> u8 {
    let offset = (x / 12) * 64 + y * 3 + (x % 12) / 4;
    image[offset] >> (6 - (x % 4) * 2) & 3
}

fn assert_text(image: &[u8; 128], x: usize, y: usize, value: &[u8], color: u8) {
    let mut reference = [0; 1280];
    draw_text(0, &mut reference, 0, 0, value, 1);
    for row in 0..5 {
        for column in 0..value.len() * 4 - 1 {
            let lit = reference[row * 160 + column] == HUD_INK | 1;
            assert_eq!(
                pixel(image, x + column, y + row) == color,
                lit,
                "text {value:?}, pixel {column},{row}"
            );
        }
    }
}

#[test]
fn six_digit_score_crosses_sprite_boundary_and_clamps_without_losing_glyphs() {
    for (value, expected) in [(0, b"000000"), (12345, b"012345"), (u32::MAX, b"999999")] {
        let mut status = hud(Mode::Playing);
        status.score = value;
        let state = DisplayState::from_hud(&status, 0, None);
        let score = image(0, &state);
        assert_text(&score, 2, 2, b"SCORE", 3);
        assert_text(&score, 0, 9, expected, 3);
        assert_eq!(pixel(&score, 23, 0), 0);
        assert_eq!(pixel(&score, 23, 10), 0);
    }
}

#[test]
fn resource_labels_and_bomb_rectangles_fit_their_four_corner_footprints() {
    let mut status = hud(Mode::Playing);
    status.lives = u8::MAX;
    status.bombs = u8::MAX;
    let state = DisplayState::from_hud(&status, 0, None);
    let inventory = image(1, &state);
    assert_text(&inventory, 0, 2, b"LIFE99", 3);
    for bombs in [0, 1, 2, 3, u8::MAX] {
        status.bombs = bombs;
        for boss_shield in [0, 25, 100] {
            status.boss_shield = boss_shield;
            let state = DisplayState::from_hud(&status, 0, None);
            assert_eq!(state.accent(1), 7, "loaded bombs must stay yellow");
            let inventory = image(1, &state);
            for y in 9..14 {
                for x in 0..24 {
                    let expected = if x % 8 == 0 || x % 8 == 7 {
                        0
                    } else if x / 8 < bombs.min(3) as usize {
                        2
                    } else {
                        1
                    };
                    assert_eq!(pixel(&inventory, x, y), expected);
                }
            }
        }
    }
    assert_eq!(pixel(&inventory, 0, 0), 0);
    let shield = image(2, &state);
    assert_text(&shield, 0, 2, b"SHIELD", 3);
    assert_eq!(pixel(&shield, 0, 20), 0);
    assert_eq!(pixel(&shield, 11, 20), 0);
    let charge = image(3, &state);
    assert_text(&charge, 0, 4, b"CHARGE", 3);
    assert_eq!(pixel(&charge, 0, 0), 0);
    assert_eq!(pixel(&charge, 23, 20), 0);
    assert_ne!(shield, charge);
    for (x, y) in CORNER_POSITIONS {
        assert!(x as usize + 24 <= 160);
        assert!(y as usize + 21 <= 200);
    }
    assert_eq!(core::mem::size_of::<DisplayState>(), 12);
}

#[test]
fn lives_fit_two_digits_without_moving_single_digits_or_inventory_gauges() {
    let mut status = hud(Mode::Playing);
    status.boss_shield = 75;
    let baseline = DisplayState::from_hud(&status, 0, None);
    let inventory = image(1, &baseline);
    for lives in 0..=u8::MAX {
        let expected = format!("LIFE{:2}", lives.min(99));
        status.lives = lives;
        let state = DisplayState::from_hud(&status, 0, None);
        let actual = image(1, &state);
        assert_text(&actual, 0, 2, expected.as_bytes(), 3);
        for y in 0..21 {
            assert_eq!(pixel(&actual, 23, y), pixel(&inventory, 23, y));
            if !(2..7).contains(&y) {
                for x in 0..24 {
                    assert_eq!(pixel(&actual, x, y), pixel(&inventory, x, y));
                }
            }
        }
        for corner in [0, 2, 3] {
            assert_eq!(image(corner, &state), image(corner, &baseline));
        }
    }
}

#[test]
fn lives_changes_dirty_only_inventory_and_clear_the_old_tens_digit() {
    let mut status = hud(Mode::Playing);
    status.story_mode = true;
    for (old, new) in [(99, 98), (10, 9), (3, 99)] {
        status.lives = old;
        let before = DisplayState::from_hud(&status, 0, None);
        let mut published = image(1, &before);
        status.lives = new;
        let after = DisplayState::from_hud(&status, 0, None);
        assert_eq!(after.changed_corners(Some(&before)), 2);
        render_corner(1, &after, &mut published);
        assert_eq!(published, image(1, &after));
        assert_ne!(published, image(1, &before));
        if new < 10 {
            assert_text(&published, 16, 2, b" ", 3);
        }
    }
    status.lives = 99;
    let capped = DisplayState::from_hud(&status, 0, None);
    for lives in [100, u8::MAX] {
        status.lives = lives;
        let same = DisplayState::from_hud(&status, 0, None);
        assert_eq!(same, capped);
        assert_eq!(same.changed_corners(Some(&capped)), 0);
        assert_eq!(image(1, &same), image(1, &capped));
    }
}

#[test]
fn gauges_have_grey_empty_space_white_frames_and_status_colors() {
    for (shield_value, charge_value, boss_value) in [(0, 0, 1), (25, 128, 25), (100, 255, 100)] {
        let mut status = hud(Mode::Playing);
        status.shield = shield_value;
        status.charge = charge_value;
        status.boss_shield = boss_value;
        let state = DisplayState::from_hud(&status, 0, None);
        for (corner, x, y, width, fill) in [
            (2, 0, 9, 22, shield_value as usize * 22 / 100),
            (3, 0, 11, 22, charge_value as usize * 22 / 255),
            (1, 6, 15, 15, boss_value as usize * 15 / 100),
        ] {
            let picture = image(corner, &state);
            for column in 0..width + 2 {
                assert_eq!(pixel(&picture, x + column, y), 3);
                assert_eq!(pixel(&picture, x + column, y + 4), 3);
            }
            for column in 0..width {
                assert_eq!(
                    pixel(&picture, x + 1 + column, y + 2),
                    if column < fill { 2 } else { 1 }
                );
            }
        }
        assert_eq!(state.accent(2), if shield_value <= 25 { 10 } else { 13 });
        assert_eq!(state.accent(1), 7);
        assert_eq!(state.accent(3), if charge_value == 255 { 7 } else { 3 });
    }
}

#[test]
fn unchanged_visible_fields_and_subpixel_gauge_changes_do_not_dirty_images() {
    let mut status = hud(Mode::Playing);
    let before = DisplayState::from_hud(&status, 0, None);
    assert_eq!(before.changed_corners(None), 15);
    status.distance += 8;
    status.section += 1;
    status.roll_ticks = 10;
    status.victory_ticks = 20;
    status.boss_kind = 4;
    status.story_mode = true;
    status.shield = 76;
    status.charge = 129;
    let unchanged = DisplayState::from_hud(&status, 16, None);
    assert_eq!(before, unchanged);
    assert_eq!(unchanged.changed_corners(Some(&before)), 0);
    status.score += 1;
    assert_eq!(
        DisplayState::from_hud(&status, 0, None).changed_corners(Some(&before)),
        1
    );
    status.score -= 1;
    status.bombs -= 1;
    assert_eq!(
        DisplayState::from_hud(&status, 0, None).changed_corners(Some(&before)),
        2
    );
    status.bombs += 1;
    status.shield = 50;
    assert_eq!(
        DisplayState::from_hud(&status, 0, None).changed_corners(Some(&before)),
        4
    );
}

#[test]
fn palette_thresholds_and_ready_blink_invalidate_only_their_corner() {
    let mut status = hud(Mode::Playing);
    status.shield = 26;
    status.boss_shield = 26;
    let before = DisplayState::from_hud(&status, 0, None);
    status.shield = 25;
    assert_eq!(
        DisplayState::from_hud(&status, 0, None).changed_corners(Some(&before)),
        4
    );
    status.shield = 26;
    status.boss_shield = 25;
    assert_eq!(
        DisplayState::from_hud(&status, 0, None).changed_corners(Some(&before)),
        0
    );
    status.boss_shield = 26;
    status.charge = 255;
    let ready = DisplayState::from_hud(&status, 0, None);
    assert_eq!(ready.changed_corners(Some(&before)), 8);
    let blink = DisplayState::from_hud(&status, 16, None);
    assert_eq!(blink.changed_corners(Some(&ready)), 8);
    assert_eq!(
        DisplayState::from_hud(&status, 17, None).changed_corners(Some(&blink)),
        0
    );
    assert_text(&image(3, &ready), 0, 4, b"HOMING", 3);
    assert_text(&image(3, &blink), 0, 4, b"HOMING", 2);
    for y in 11..16 {
        for x in 0..24 {
            assert_eq!(
                pixel(&image(3, &ready), x, y),
                pixel(&image(3, &blink), x, y)
            );
        }
    }
}

#[test]
fn fps_preserves_tenths_and_is_the_only_sprite_content_in_menu_modes() {
    for mode in [Mode::Title, Mode::Attract, Mode::Victory] {
        let mut status = hud(mode);
        status.story_mode = true;
        status.lives = 99;
        let hidden = DisplayState::from_hud(&status, 0, None);
        assert_eq!(hidden.enabled_mask(), 0);
        assert_eq!(hidden.changed_corners(None), 0);
        for corner in 0..4 {
            assert_eq!(image(corner, &hidden), [0; 128]);
        }
        for (fps, expected) in [
            (0, b"F 0.0"),
            (62, b"F 6.2"),
            (123, b"F12.3"),
            (u16::MAX, b"F99.9"),
        ] {
            let shown = DisplayState::from_hud(&status, 0, Some(fps));
            assert_eq!(shown.enabled_mask(), 3);
            assert_eq!(shown.changed_corners(Some(&hidden)), 1);
            assert_eq!(hidden.changed_corners(Some(&shown)), 0);
            let chip = image(0, &shown);
            assert_text(&chip, 2, 15, expected, 3);
            for y in 0..21 {
                for x in 0..24 {
                    if !(15..20).contains(&y) || !(2..21).contains(&x) {
                        assert_eq!(pixel(&chip, x, y), 0);
                    }
                }
            }
            for corner in 1..4 {
                assert_eq!(image(corner, &shown), [0; 128]);
            }
        }
    }
}

#[test]
fn mode_transitions_restore_all_corners_and_show_boss_health_only_during_play() {
    let menu = DisplayState::from_hud(&hud(Mode::Title), 0, Some(100));
    for mode in [
        Mode::Playing,
        Mode::Transition,
        Mode::Respawning,
        Mode::GameOver,
    ] {
        let mut status = hud(mode);
        status.boss_shield = 100;
        let state = DisplayState::from_hud(&status, 0, Some(100));
        assert_eq!(state.enabled_mask(), 255);
        assert_eq!(state.changed_corners(Some(&menu)), 15);
        assert_eq!(menu.changed_corners(Some(&state)), 1);
        assert_eq!(
            pixel(&image(1, &state), 10, 17),
            if mode == Mode::Playing { 2 } else { 0 }
        );
    }
}
