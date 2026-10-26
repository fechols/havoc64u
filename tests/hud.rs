use havoc::{
    assets::SEGMENT_COUNT,
    game::{Hud, MenuMusicStatus, Mode},
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
        story_mode: false,
        menu_music: MenuMusicStatus::Title,
        shield: 75,
        bombs: 2,
        lives: 3,
        charge: 128,
        boost: 255,
        boost_active: false,
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

fn level_strip(section: u8) -> [u8; 1280] {
    let mut pixels = [0; 1280];
    let name = LEVEL_NAMES[section as usize];
    draw_text(
        0,
        &mut pixels,
        ((160 - (name.len() * 4 - 1)) / 2) as i16,
        2,
        name,
        1,
    );
    pixels
}

fn assert_centered_caption(pixels: &[u8], y: usize, text: &[u8], color: u8, panel: (usize, usize)) {
    let mut reference = [0; 1280];
    draw_text(
        0,
        &mut reference,
        ((160 - (text.len() * 4 - 1)) / 2) as i16,
        0,
        text,
        color,
    );
    for row in 0..5 {
        for x in 0..160 {
            let expected = if (panel.0..panel.1).contains(&x) {
                reference[row * 160 + x]
            } else {
                6
            };
            assert_eq!(
                pixels[(y + row) * 160 + x],
                expected,
                "caption {text:?} at {x},{row}"
            );
        }
    }
}

#[test]
fn title_has_large_lettering_and_keeps_the_ship_visible() {
    let pixels = frame(&state(Mode::Title), 0);
    assert_centered_caption(&pixels, 134, b"PORT2: FIRE2/LSHIFT BOOST", 12, (10, 150));
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
fn title_names_the_selected_mode_without_moving_the_other_rows() {
    for (tick, color) in [(0, 1), (16, 3)] {
        let mut status = state(Mode::Title);
        let normal = frame(&status, tick);
        assert_centered_caption(&normal, 189, b"S: NORMAL 3 / FIRE: START", color, (10, 150));
        status.story_mode = true;
        let story = frame(&status, tick);
        assert_centered_caption(&story, 189, b"S: STORY 99 / FIRE: START", color, (10, 150));
        assert_eq!(&normal[..189 * 160], &story[..189 * 160]);
        assert_eq!(&normal[194 * 160..], &story[194 * 160..]);
    }
}

#[test]
fn menu_music_status_is_centered_persistent_and_leaves_other_rows_unchanged() {
    for (mode, y) in [(Mode::Title, 2), (Mode::Attract, 10)] {
        let mut status = state(mode);
        let baseline = frame(&status, 0);
        for (music, caption) in [
            (
                MenuMusicStatus::Title,
                b"NOW PLAYING TITLE MUSIC...".as_slice(),
            ),
            (
                MenuMusicStatus::Soundtrack,
                b"NOW PLAYING SOUNDTRACK...".as_slice(),
            ),
            (
                MenuMusicStatus::Victory,
                b"NOW PLAYING VICTORY MUSIC...".as_slice(),
            ),
        ] {
            status.menu_music = music;
            let pixels = frame(&status, 0);
            assert_centered_caption(&pixels, y, caption, 3, (0, 160));
            assert_eq!(&pixels[..y * 160], &baseline[..y * 160]);
            assert_eq!(&pixels[(y + 5) * 160..], &baseline[(y + 5) * 160..]);
            let blinking = frame(&status, 16);
            assert_eq!(
                &pixels[y * 160..(y + 5) * 160],
                &blinking[y * 160..(y + 5) * 160]
            );
        }
    }
    for mode in [
        Mode::Playing,
        Mode::Transition,
        Mode::Respawning,
        Mode::GameOver,
        Mode::Victory,
    ] {
        let mut status = state(mode);
        let baseline = frame(&status, 0);
        for music in [MenuMusicStatus::Soundtrack, MenuMusicStatus::Victory] {
            status.menu_music = music;
            assert_eq!(
                frame(&status, 0),
                baseline,
                "music status leaked into {mode:?}"
            );
        }
    }
}

#[test]
fn menu_music_strips_survive_cell_packing_over_every_sky_and_retained_palette() {
    use havoc::cells::{unpack_band, CellBand};

    for (mode, band) in [(Mode::Title, 0), (Mode::Attract, 1)] {
        for music in [
            MenuMusicStatus::Title,
            MenuMusicStatus::Soundtrack,
            MenuMusicStatus::Victory,
        ] {
            let mut status = state(mode);
            status.menu_music = music;
            for background in 0..16 {
                let mut pixels = [background; 1280];
                assert!(hud::draw(band, &mut pixels, &status, 0));
                let mut cells = CellBand::new();
                cells.clear(background);
                assert!(hud::draw(band, &mut cells, &status, 0));
                let expected = pixels.map(|pixel| pixel & 15);
                for (old_screen, old_color) in [(0, 0), (0x12, 3), (0x8b, 2), (0xff, 15)] {
                    let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
                    cells.finish(
                        &[old_screen; 40],
                        &[old_color; 40],
                        &mut bitmap,
                        &mut screen,
                        &mut color,
                    );
                    assert_eq!(unpack_band(&bitmap, &screen, &color, background), expected);
                }
                assert_eq!(cells.stats.overflow_cells, 0);
            }
        }
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
fn respawn_lives_are_centered_at_their_actual_width_and_saturate_at_99() {
    for (lives, caption) in [
        (0, &b"SHIPS REMAINING: 0"[..]),
        (3, &b"SHIPS REMAINING: 3"[..]),
        (9, &b"SHIPS REMAINING: 9"[..]),
        (10, &b"SHIPS REMAINING: 10"[..]),
        (98, &b"SHIPS REMAINING: 98"[..]),
        (99, &b"SHIPS REMAINING: 99"[..]),
        (u8::MAX, &b"SHIPS REMAINING: 99"[..]),
    ] {
        let mut status = state(Mode::Respawning);
        status.story_mode = true;
        status.lives = lives;
        assert_centered_caption(&frame(&status, 0), 106, caption, 3, (26, 134));
    }
}

#[test]
fn victory_reveal_hides_all_hud_and_cockpit_until_arrival() {
    let mut status = state(Mode::Victory);
    status.boss_shield = 100;
    status.distance = 0;
    for tick in [0, 1, 100, REVEAL_TICKS - 1] {
        status.victory_ticks = tick;
        let pixels = frame(&status, 0);
        assert_eq!(&pixels[..1280], &[0; 1280]);
        assert!(pixels[1280..].iter().all(|&pixel| pixel == 6));
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
    assert_eq!(&victory[..1280], &[0; 1280]);
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
        status.distance = 0;
        status.boss_shield = 100;
        let pixels = frame(&status, 0);
        assert_eq!(&pixels[..1280], &level_strip(section));
        assert!(pixels[189 * 160..194 * 160]
            .iter()
            .any(|&p| p == HUD_INK | 1));
        assert_centered_caption(&pixels, 10, b"NOW PLAYING TITLE MUSIC...", 3, (0, 160));
        for y in 16..200 {
            if !(189..194).contains(&y) {
                assert!(pixels[y * 160..(y + 1) * 160].iter().all(|&p| p == 6));
            }
        }
        assert_ne!(pixels, frame(&status, 16));
        assert!(!images.contains(&pixels), "section labels must be distinct");
        images.push(pixels);
    }
}

#[test]
fn attract_names_survive_native_packing_and_previous_cell_palettes() {
    for mode in [Mode::Attract] {
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
                    0,
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
    for (mode, story_mode, music) in [
        Mode::Title,
        Mode::Playing,
        Mode::Respawning,
        Mode::Victory,
        Mode::GameOver,
        Mode::Attract,
        Mode::Transition,
    ]
    .into_iter()
    .flat_map(|mode| [false, true].map(|story_mode| (mode, story_mode)))
    .flat_map(|(mode, story_mode)| {
        [
            MenuMusicStatus::Title,
            MenuMusicStatus::Soundtrack,
            MenuMusicStatus::Victory,
        ]
        .map(|music| (mode, story_mode, music))
    }) {
        for band in 0..25 {
            let mut pixels = [6; 1280];
            let mut status = state(mode);
            status.story_mode = story_mode;
            status.menu_music = music;
            status.lives = if story_mode { 99 } else { 3 };
            status.victory_ticks = REVEAL_TICKS;
            hud::draw(band, &mut pixels, &status, 0);
            let mut bitmap = [0; 320];
            let mut screen = [0; 40];
            let mut color = [0; 40];
            pack_band(
                &pixels,
                &[0; 40],
                &[0; 40],
                0,
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
                    "mode {mode:?}, story {story_mode}, music {music:?}, band {band}, pixel {x},{y}"
                );
            }
        }
    }
}

#[test]
fn level_names_fill_only_the_reserved_strip_and_persist_without_blinking() {
    for mode in [
        Mode::Playing,
        Mode::Transition,
        Mode::Respawning,
        Mode::GameOver,
    ] {
        for section in 0..SEGMENT_COUNT {
            let mut status = state(mode);
            status.section = section;
            status.charge = 255;
            let pixels = frame(&status, 16);
            assert_eq!(&pixels[..1280], &level_strip(section));
            assert_eq!(&pixels[..1280], &frame(&status, 0)[..1280]);
            if matches!(mode, Mode::Playing | Mode::Transition) {
                assert!(pixels[1280..].iter().all(|&pixel| pixel == 6));
            }
            assert!(pixels[184 * 160..].iter().all(|&pixel| pixel == 6));
        }
    }
}

#[test]
fn boss_bar_is_red_proportional_and_replaces_the_name_only_during_combat() {
    for health in [1, 2, 25, 50, 99, 100, u8::MAX] {
        let mut status = state(Mode::Playing);
        status.boss_shield = health;
        let pixels = frame(&status, 0);
        let filled = 152 * health.min(100) as usize / 100;
        for y in 0..8 {
            for x in 0..160 {
                let expected = if (2..6).contains(&y) && (4..156).contains(&x) {
                    if x < 4 + filled {
                        HUD_INK | 2
                    } else {
                        11
                    }
                } else {
                    0
                };
                assert_eq!(pixels[y * 160 + x], expected, "health {health}, {x},{y}");
            }
        }
        assert!(pixels[1280..].iter().all(|&pixel| pixel == 6));
        status.boss_shield = 0;
        assert_eq!(&frame(&status, 0)[..1280], &level_strip(0));
    }
    for mode in [
        Mode::Attract,
        Mode::Transition,
        Mode::Respawning,
        Mode::GameOver,
    ] {
        let mut status = state(mode);
        status.boss_shield = 100;
        assert_eq!(&frame(&status, 0)[..1280], &level_strip(0));
    }
    assert_eq!(&frame(&state(Mode::Victory), 0)[..1280], &[0; 1280]);
    assert_centered_caption(
        &frame(&state(Mode::Title), 0),
        2,
        b"NOW PLAYING TITLE MUSIC...",
        3,
        (0, 160),
    );
}

#[cfg(feature = "host-tools")]
#[test]
fn every_crossing_keeps_the_outgoing_name_until_arrival_and_keeps_the_entry_banner() {
    use havoc::game::{Game, TRANSITION_TICKS};
    for section in 0..SEGMENT_COUNT - 1 {
        for tick in [
            0,
            TRANSITION_TICKS / 2,
            TRANSITION_TICKS - 1,
            TRANSITION_TICKS,
        ] {
            let status = Game::preview_transition(section, tick).hud();
            let arrived = tick == TRANSITION_TICKS;
            assert_eq!(status.section, section + u8::from(arrived));
            assert_eq!(
                status.mode,
                if arrived {
                    Mode::Playing
                } else {
                    Mode::Transition
                }
            );
            let image = frame(&status, 0);
            assert_eq!(&image[..1280], &level_strip(section + u8::from(arrived)));
            assert!(image[184 * 160..].iter().all(|&pixel| pixel == 6));
            assert_eq!(
                image[29 * 160..51 * 160]
                    .iter()
                    .any(|&pixel| pixel & HUD_INK != 0),
                arrived
            );
        }
    }
}

#[test]
fn sector_banner_expires_and_all_eighteen_numbers_remain_distinct() {
    let mut sectors = Vec::new();
    for section in 0..SEGMENT_COUNT {
        let mut status = state(Mode::Playing);
        status.section = section;
        status.distance = 0;
        let image = frame(&status, 0);
        assert!(image[29 * 160..51 * 160]
            .iter()
            .any(|&pixel| pixel & HUD_INK != 0));
        let number = image[33 * 160..38 * 160].to_vec();
        assert!(
            !sectors.contains(&number),
            "duplicate sector number {section}"
        );
        sectors.push(number);
        status.distance = 800;
        let expired = frame(&status, 0);
        assert_eq!(&expired[..1280], &level_strip(section));
        assert!(expired[1280..].iter().all(|&pixel| pixel == 6));
    }
}

#[test]
fn strip_is_exact_in_pixel_and_cell_packing_over_every_sky_and_retained_palette() {
    use havoc::cells::{unpack_band, CellBand};

    for background in 0..16 {
        for health in [0, 1, 25, 99, 100] {
            let mut status = state(Mode::Playing);
            status.boss_shield = health;
            let mut pixels = [background; 1280];
            assert!(hud::draw(0, &mut pixels, &status, 0));
            let mut cells = CellBand::new();
            cells.clear(background);
            assert!(hud::draw(0, &mut cells, &status, 0));
            let expected = pixels.map(|pixel| pixel & 15);
            for (old_screen, old_color) in [(0, 0), (0x12, 3), (0x8b, 2), (0xff, 15)] {
                let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
                pack_band(
                    &pixels,
                    &[old_screen; 40],
                    &[old_color; 40],
                    background,
                    &mut bitmap,
                    &mut screen,
                    &mut color,
                );
                assert_eq!(unpack_band(&bitmap, &screen, &color, background), expected);
                cells.finish(
                    &[old_screen; 40],
                    &[old_color; 40],
                    &mut bitmap,
                    &mut screen,
                    &mut color,
                );
                assert_eq!(unpack_band(&bitmap, &screen, &color, background), expected);
            }
            assert_eq!(cells.stats.overflow_cells, 0);
            let paused = pixels;
            assert!(!hud::draw_paused(0, &mut pixels));
            assert_eq!(pixels, paused);
        }
    }
}
