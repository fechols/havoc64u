use havoc::{
    assets::SEGMENT_COUNT,
    game::{Hud, Mode},
    hud,
    render::{draw_text, pack_band, HUD_INK},
    victory::REVEAL_TICKS,
};

const LEVEL_NAMES: [&[u8]; SEGMENT_COUNT as usize] = [
    b"LOWLAND APPROACH",
    b"CRYSTAL CAVE",
    b"IRON CANYON",
    b"CANYON GATE",
    b"ASHEN WASTES",
    b"FOUNDRY BELT",
    b"FORTRESS RUN",
    b"CITADEL CORE",
    b"STORM FRONT",
    b"METEOR REACH",
    b"CARRIER COAST",
    b"CARRIER ASSAULT",
    b"ASTEROID BELT",
    b"SHATTERED MOON",
    b"SHIP GRAVEYARD",
    b"ORBITAL SHIPYARD",
    b"SPACE CARRIER",
    b"JUMP GATE",
];

fn state(mode: Mode) -> Hud {
    Hud {
        mode,
        shield: 75,
        bombs: 2,
        lives: 3,
        charge: 128,
        score: 12345,
        distance: 1000,
        section: 0,
        roll_ticks: 0,
        boss_shield: 0,
        boss_kind: 0,
        victory_ticks: 0,
    }
}

fn frame(hud_state: &Hud, frame: u16) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(32000);
    for band in 0..25 {
        let mut row = [6; 1280];
        hud::draw(band, &mut row, hud_state, frame);
        pixels.extend_from_slice(&row);
    }
    pixels
}

#[test]
fn cockpit_leaves_playfield_untouched_and_displays_resource_levels() {
    let pixels = frame(&state(Mode::Playing), 0);
    assert!(pixels[8 * 160..185 * 160].iter().all(|&p| p == 6));
    assert_eq!(pixels[194 * 160 + 3], HUD_INK | 13);
    assert_eq!(pixels[194 * 160 + 47], HUD_INK | 13);
    assert_eq!(pixels[194 * 160 + 48], 11);
    assert_eq!(pixels[194 * 160 + 129], HUD_INK | 7);
    assert_eq!(pixels[194 * 160 + 136], HUD_INK | 7);
    assert_eq!(pixels[194 * 160 + 143], 11);
}

#[test]
fn all_level_names_fit_between_score_and_lives_in_every_cockpit_mode() {
    for mode in [
        Mode::Playing,
        Mode::Transition,
        Mode::Respawning,
        Mode::GameOver,
    ] {
        for (section, name) in LEVEL_NAMES.iter().enumerate() {
            for (score, lives, digits, life) in [
                (0, 0, b"000000", b'0'),
                (999_999, 3, b"999999", b'3'),
                (u32::MAX, u8::MAX, b"999999", b'9'),
            ] {
                let mut status = state(mode);
                status.section = section as u8;
                status.score = score;
                status.lives = lives;
                let mut pixels = [6; 1280];
                hud::draw(0, &mut pixels, &status, 0);

                // Compare each complete field independently: no name may
                // overwrite a score digit or the unchanged lives display.
                let name_x = (160 - (name.len() * 4 - 1)) / 2;
                assert!(name_x >= 48 && name_x + name.len() * 4 - 1 <= 111);
                let mut expected = [0; 1280];
                draw_text(0, &mut expected, 1, 2, b"SCORE", 12);
                draw_text(0, &mut expected, 23, 2, digits, 1);
                draw_text(0, &mut expected, name_x as i16, 2, name, 1);
                draw_text(0, &mut expected, 115, 2, b"LIVES", 12);
                draw_text(0, &mut expected, 143, 2, &[life], 1);
                assert_eq!(
                    pixels, expected,
                    "{mode:?}, section {section}, score {score}, lives {lives}"
                );
            }
        }
    }
}

#[cfg(feature = "host-tools")]
#[test]
fn crossing_keeps_the_outgoing_name_until_arrival_for_all_seventeen_boundaries() {
    use havoc::game::{Game, TRANSITION_TICKS};

    for section in 0..SEGMENT_COUNT - 1 {
        for tick in [
            0,
            TRANSITION_TICKS / 2,
            TRANSITION_TICKS - 1,
            TRANSITION_TICKS,
        ] {
            let status = Game::preview_transition(section, tick).hud();
            let expected_section = section + u8::from(tick == TRANSITION_TICKS);
            assert_eq!(status.section, expected_section);
            assert_eq!(
                status.mode,
                if tick == TRANSITION_TICKS {
                    Mode::Playing
                } else {
                    Mode::Transition
                }
            );
            let image = frame(&status, 0);
            let mut expected = [0; 1280];
            let name = LEVEL_NAMES[expected_section as usize];
            draw_text(
                0,
                &mut expected,
                ((160 - (name.len() * 4 - 1)) / 2) as i16,
                2,
                name,
                1,
            );
            for row in 0..8 {
                assert_eq!(
                    &image[row * 160 + 48..row * 160 + 111],
                    &expected[row * 160 + 48..row * 160 + 111],
                    "section {section}, crossing tick {tick}"
                );
            }
        }
    }
}

#[test]
fn scenic_flight_retains_cockpit_without_obscuring_the_landscape() {
    let playing = frame(&state(Mode::Playing), 0);
    let mut status = state(Mode::Transition);
    status.distance = 30_000;
    let transit = frame(&status, 0);
    assert_eq!(&transit[..8 * 160], &playing[..8 * 160]);
    assert_eq!(&transit[184 * 160..], &playing[184 * 160..]);
    assert!(transit[8 * 160..184 * 160].iter().all(|&pixel| pixel == 6));
    status.mode = Mode::Playing;
    status.section = 1;
    let arrived = frame(&status, 0);
    assert!(arrived[29 * 160..51 * 160]
        .iter()
        .any(|&pixel| pixel & HUD_INK != 0));
}

#[test]
fn title_has_large_lettering_and_keeps_the_ship_visible() {
    let pixels = frame(&state(Mode::Title), 0);
    assert!(
        pixels[31 * 160..46 * 160]
            .iter()
            .filter(|&&p| p == HUD_INK | 1)
            .count()
            > 400
    );
    assert!(pixels[70 * 160..130 * 160].iter().all(|&p| p == 6));
    assert!(pixels[189 * 160..194 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 1));
    assert_ne!(pixels, frame(&state(Mode::Title), 16));
}

#[test]
fn sector_banner_expires_and_boss_meter_uses_normalized_health() {
    let mut status = state(Mode::Playing);
    status.distance = 0;
    let first = frame(&status, 0);
    assert!(first[29 * 160..51 * 160].iter().any(|&p| p & HUD_INK != 0));
    status.distance = 800;
    let expired = frame(&status, 0);
    assert!(expired[29 * 160..51 * 160].iter().all(|&p| p == 6));
    assert_eq!(&first[..1280], &expired[..1280]);
    assert!(expired[..1280]
        .chunks_exact(160)
        .any(|row| row[48..111].contains(&(HUD_INK | 1))));
    status.boss_shield = 20;
    let boss = frame(&status, 0);
    assert_eq!(boss[14 * 160 + 70], HUD_INK | 10);
    assert_eq!(boss[14 * 160 + 79], HUD_INK | 10);
    assert_eq!(boss[14 * 160 + 80], 11);
}

#[test]
fn all_eighteen_sector_numbers_and_five_boss_labels_remain_distinct() {
    let mut sectors = Vec::new();
    for stage in 0..SEGMENT_COUNT {
        let mut status = state(Mode::Playing);
        status.section = stage;
        status.distance = stage as u32 * havoc::assets::SEGMENT_DISTANCE;
        let image = frame(&status, 0);
        let number = image[33 * 160..38 * 160].to_vec();
        assert!(
            !sectors.contains(&number),
            "duplicate sector number {stage}"
        );
        sectors.push(number);
    }
    let mut labels = Vec::new();
    for kind in 1..=5 {
        let mut status = state(Mode::Playing);
        status.boss_kind = kind;
        status.boss_shield = 100;
        let image = frame(&status, 0);
        let label: Vec<_> = (13..18)
            .flat_map(|y| image[y * 160 + 14..y * 160 + 66].iter().copied())
            .collect();
        assert!(!labels.contains(&label), "duplicate boss label {kind}");
        assert_eq!(image[14 * 160 + 70], HUD_INK | 8);
        labels.push(label);
    }
}

#[test]
fn respawn_and_defeat_show_status_and_restart_cues() {
    let respawn = frame(&state(Mode::Respawning), 0);
    assert!(respawn[84 * 160..89 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 10));
    let defeat = frame(&state(Mode::GameOver), 0);
    assert!(defeat[77 * 160..87 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 10));
    assert!(defeat[124 * 160..129 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 1));
}

#[test]
fn victory_reveal_hides_all_hud_and_cockpit_until_arrival() {
    let mut status = state(Mode::Victory);
    status.boss_shield = 100;
    status.distance = 0;
    for tick in [0, 1, 100, REVEAL_TICKS - 1] {
        status.victory_ticks = tick;
        assert!(frame(&status, 0).iter().all(|&pixel| pixel == 6));
    }
}

#[test]
fn victory_results_leave_the_star_clear_and_show_score_and_restart() {
    let mut status = state(Mode::Victory);
    status.victory_ticks = REVEAL_TICKS;
    let victory = frame(&status, 0);
    assert!(victory[13 * 160..23 * 160]
        .iter()
        .any(|&pixel| pixel == HUD_INK | 7));
    assert!(victory[180 * 160..185 * 160]
        .iter()
        .any(|&pixel| pixel == HUD_INK | 1));
    assert!(victory[190 * 160..195 * 160]
        .iter()
        .any(|&pixel| pixel == HUD_INK | 1));
    assert!(victory[27 * 160..177 * 160].iter().all(|&pixel| pixel == 6));
    assert!(victory[..8 * 160].iter().all(|&pixel| pixel == 6));
    for y in 185..200 {
        assert!(victory[y * 160..y * 160 + 36]
            .iter()
            .all(|&pixel| pixel == 6));
        assert!(victory[y * 160 + 124..(y + 1) * 160]
            .iter()
            .all(|&pixel| pixel == 6));
    }
    assert_ne!(victory, frame(&status, 16));
    status.score += 1;
    let different_score = frame(&status, 0);
    assert_ne!(
        &victory[180 * 160..185 * 160],
        &different_score[180 * 160..185 * 160]
    );
}

#[test]
fn attract_labels_identify_each_map_without_cockpit_or_boss_panels() {
    let mut images = Vec::new();
    for section in 0..SEGMENT_COUNT {
        let mut status = state(Mode::Attract);
        status.section = section;
        status.distance = section as u32 * 30_000;
        status.boss_shield = 100;
        let pixels = frame(&status, 0);
        let mut expected = [6; 1280];
        let name = LEVEL_NAMES[section as usize];
        draw_text(
            0,
            &mut expected,
            ((160 - (name.len() * 4 - 1)) / 2) as i16,
            2,
            name,
            1,
        );
        assert_eq!(&pixels[..1280], &expected);
        assert!(pixels[189 * 160..194 * 160]
            .iter()
            .any(|&p| p == HUD_INK | 1));
        for y in 0..200 {
            if !(2..7).contains(&y) && !(189..194).contains(&y) {
                assert!(pixels[y * 160..(y + 1) * 160].iter().all(|&p| p == 6));
            }
        }
        assert_ne!(pixels, frame(&status, 16));
        assert!(!images.contains(&pixels), "section labels must be distinct");
        images.push(pixels);
    }
}

#[test]
fn every_level_top_strip_survives_native_packing_and_previous_cell_palettes() {
    for mode in [Mode::Playing, Mode::Attract] {
        for section in 0..SEGMENT_COUNT {
            let mut status = state(mode);
            status.section = section;
            status.score = u32::MAX;
            status.lives = u8::MAX;
            let mut pixels = [6; 1280];
            hud::draw(0, &mut pixels, &status, 0);
            for (old_screen, old_color) in [(0, 0), (0x12, 3), (0x8b, 2), (0xff, 15)] {
                let mut bitmap = [0; 320];
                let mut screen = [0; 40];
                let mut color = [0; 40];
                pack_band(
                    &pixels,
                    &[old_screen; 40],
                    &[old_color; 40],
                    &mut bitmap,
                    &mut screen,
                    &mut color,
                );
                for (i, &pixel) in pixels.iter().enumerate() {
                    let x = i % 160;
                    let y = i / 160;
                    let cell = x / 4;
                    let palette = [0, screen[cell] >> 4, screen[cell] & 15, color[cell]];
                    let packed = palette[((bitmap[cell * 8 + y] >> (6 - x % 4 * 2)) & 3) as usize];
                    assert_eq!(packed, pixel & 15, "{mode:?}, section {section}, pixel {x},{y}, old palette {old_screen:02x}/{old_color:02x}");
                }
            }
        }
    }
}

#[test]
fn every_mode_survives_native_bitmap_packing_with_readable_ink() {
    for mode in [
        Mode::Title,
        Mode::Playing,
        Mode::Respawning,
        Mode::Victory,
        Mode::GameOver,
        Mode::Attract,
        Mode::Transition,
    ] {
        for band in 0..25 {
            let mut pixels = [6; 1280];
            let mut status = state(mode);
            status.victory_ticks = REVEAL_TICKS;
            hud::draw(band, &mut pixels, &status, 0);
            let mut bitmap = [0; 320];
            let mut screen = [0; 40];
            let mut color = [0; 40];
            pack_band(
                &pixels,
                &[0; 40],
                &[0; 40],
                &mut bitmap,
                &mut screen,
                &mut color,
            );
            for (i, &pixel) in pixels.iter().enumerate() {
                if pixel & HUD_INK == 0 {
                    continue;
                }
                let x = i % 160;
                let y = i / 160;
                let cell = x / 4;
                let palette = [0, screen[cell] >> 4, screen[cell] & 15, color[cell]];
                let packed = palette[((bitmap[cell * 8 + y] >> (6 - x % 4 * 2)) & 3) as usize];
                assert_eq!(
                    packed,
                    pixel & 15,
                    "mode {mode:?}, band {band}, pixel {x},{y}"
                );
            }
        }
    }
}
