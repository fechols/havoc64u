use havoc::assets::SEGMENT_DISTANCE;
use havoc::game::{Hud, Mode};
use havoc::render::pack_band;
use havoc::sky_cache::SkyCache;

fn hud() -> Hud {
    Hud {
        mode: Mode::Playing,
        shield: 100,
        bombs: 3,
        lives: 3,
        charge: 0,
        score: 0,
        distance: 800,
        section: 0,
        roll_ticks: 0,
        boss_shield: 0,
    }
}

#[test]
fn only_current_two_bank_validity_and_exact_screen_bytes_allow_reuse() {
    assert_eq!(core::mem::size_of::<SkyCache>(), 8);
    let mut cache = SkyCache::new();
    let screen = [0x6f; 40];
    assert_eq!(cache.reusable_bands(), 0);
    cache.record_frame(2, 1 << 5);
    assert_eq!(cache.reusable_bands(), 0);
    cache.record_frame(3, 1 << 5);
    assert_eq!(cache.reusable_bands(), 1 << 5);
    assert!(SkyCache::screens_match(&screen, &screen));
    for cell in 0..40 {
        let mut different = screen;
        different[cell] ^= 1;
        assert!(!SkyCache::screens_match(&screen, &different));
    }
    for invalid_bank in [0, 1, 4, 255] {
        cache.record_frame(invalid_bank, 0);
    }
    assert_eq!(cache.reusable_bands(), 1 << 5);
    cache.record_frame(2, 1 << 4);
    assert_eq!(cache.reusable_bands(), 0);
    cache.record_frame(2, u32::MAX);
    cache.record_frame(3, u32::MAX);
    assert_eq!(cache.reusable_bands(), 0x01ff_ffff);
}

#[test]
fn hud_eligibility_covers_modes_boss_and_exact_sector_banner_boundaries() {
    let mut hud = hud();
    let clean = 0x007f_fffe;
    assert_eq!(SkyCache::eligible_bands(u32::MAX, &hud), clean);
    assert_eq!(SkyCache::eligible_bands(1 << 5, &hud), 1 << 5);
    hud.boss_shield = 1;
    assert_eq!(SkyCache::eligible_bands(u32::MAX, &hud), clean & !6);
    for section in 0..=3 {
        hud.section = section;
        for distance in [0, 799, 800, 801] {
            hud.distance = section as u32 * SEGMENT_DISTANCE + distance;
            assert_eq!(
                SkyCache::eligible_bands(u32::MAX, &hud),
                clean & !6 & if distance < 800 { !0x78 } else { u32::MAX }
            );
        }
    }
    hud.distance = 0; // Saturation must match the HUD even with stale section.
    assert_eq!(SkyCache::eligible_bands(u32::MAX, &hud), clean & !6 & !0x78);
    for mode in [Mode::Title, Mode::Respawning, Mode::Victory, Mode::GameOver] {
        hud.mode = mode;
        assert_eq!(SkyCache::eligible_bands(u32::MAX, &hud), 0);
    }
}

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

#[test]
fn screen_bytes_determine_all_packed_sky_bytes_for_every_previous_palette() {
    // Every subset, with and without black, covers uniform/absent colors; all
    // 4096 previous palettes include duplicate slots and unrelated old colors.
    for subset in 1..16 {
        let palette: Vec<_> = [0, 6, 1, 15]
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
                            "subset {subset}, old palette {}",
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
                    &mut repeated_bitmap,
                    &mut repeated_screen,
                    &mut repeated_color,
                );
                assert_eq!(
                    (bitmap, screen, color),
                    (repeated_bitmap, repeated_screen, repeated_color)
                );
            }
        }
    }
}

#[test]
fn alternating_banks_match_uncached_output_through_occlusion_and_hud_changes() {
    let mut cache = SkyCache::new();
    let mut bitmaps = [[0; 320]; 2];
    let mut screens = [[0; 40]; 2];
    let mut color_shadow = [0; 40];
    let mut front = 0;
    let mut hits = 0;
    let mut misses = 0;
    for frame in 0..32 {
        let back = 1 - front;
        let mut pixels = band_pattern(&[6, 1, 15], 0);
        let eligible = !matches!(frame, 4 | 5 | 10 | 16 | 23);
        if !eligible {
            // Enemy, opaque HUD and mode changes all invalidate a bank.
            pixels[17] = if frame & 1 == 0 { 2 } else { 0x80 | 7 };
        }
        let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
        pack_band(
            &pixels,
            &screens[front],
            &color_shadow,
            &mut bitmap,
            &mut screen,
            &mut color,
        );
        if eligible
            && cache.reusable_bands() & (1 << 5) != 0
            && SkyCache::screens_match(&screens[front], &screens[back])
        {
            hits += 1;
        } else {
            misses += 1;
            bitmaps[back] = bitmap;
            screens[back] = screen;
            color_shadow = color;
        }
        cache.record_frame((back + 2) as u8, if eligible { 1 << 5 } else { 0 });
        assert_eq!(bitmaps[back], bitmap, "bitmap frame {frame}");
        assert_eq!(screens[back], screen, "screen frame {frame}");
        assert_eq!(color_shadow, color, "color shadow frame {frame}");
        front = back;
    }
    assert!(hits >= 8, "cache must exercise repeated reuse");
    assert!(misses >= 12, "both banks must recover after invalidation");
}
