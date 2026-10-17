use havoc::{
    game::{Hud, Mode},
    hud,
    render::{pack_band, HUD_INK},
};

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
    status.boss_shield = 20;
    let boss = frame(&status, 0);
    assert_eq!(boss[14 * 160 + 70], HUD_INK | 10);
    assert_eq!(boss[14 * 160 + 79], HUD_INK | 10);
    assert_eq!(boss[14 * 160 + 80], 11);
}

#[test]
fn respawn_and_results_show_distinct_status_and_restart_cues() {
    let respawn = frame(&state(Mode::Respawning), 0);
    assert!(respawn[84 * 160..89 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 10));
    let victory = frame(&state(Mode::Victory), 0);
    let defeat = frame(&state(Mode::GameOver), 0);
    assert!(victory[77 * 160..87 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 7));
    assert!(defeat[77 * 160..87 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 10));
    assert!(victory[124 * 160..129 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 1));
    assert!(defeat[124 * 160..129 * 160]
        .iter()
        .any(|&p| p == HUD_INK | 1));
    assert_ne!(victory, defeat);
}

#[test]
fn attract_labels_identify_each_map_without_cockpit_or_boss_panels() {
    let mut images = Vec::new();
    for section in 0..4 {
        let mut status = state(Mode::Attract);
        status.section = section;
        status.distance = section as u32 * 30_000;
        status.boss_shield = 100;
        let pixels = frame(&status, 0);
        assert!(pixels[10 * 160..15 * 160].iter().any(|&p| p == HUD_INK | 1));
        assert!(pixels[189 * 160..194 * 160]
            .iter()
            .any(|&p| p == HUD_INK | 1));
        for y in 0..200 {
            if !(10..15).contains(&y) && !(189..194).contains(&y) {
                assert!(pixels[y * 160..(y + 1) * 160].iter().all(|&p| p == 6));
            }
        }
        assert_ne!(pixels, frame(&status, 16));
        assert!(!images.contains(&pixels), "section labels must be distinct");
        images.push(pixels);
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
    ] {
        for band in 0..25 {
            let mut pixels = [6; 1280];
            hud::draw(band, &mut pixels, &state(mode), 0);
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
