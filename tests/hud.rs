use havoc::{
    game::{Hud, Mode},
    hud,
    render::{pack_band, HUD_INK},
    victory::REVEAL_TICKS,
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
    status.boss_shield = 20;
    let boss = frame(&status, 0);
    assert_eq!(boss[14 * 160 + 70], HUD_INK | 10);
    assert_eq!(boss[14 * 160 + 79], HUD_INK | 10);
    assert_eq!(boss[14 * 160 + 80], 11);
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
