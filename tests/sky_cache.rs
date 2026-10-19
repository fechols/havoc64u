use havoc::assets::SEGMENT_DISTANCE;
use havoc::game::{Hud, Mode};
use havoc::render::pack_band;
use havoc::sky_cache::SkyCache;

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
        boss_kind: 0,
        victory_ticks: 0,
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
    for section in 0..12 {
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
    for mode in [
        Mode::Title,
        Mode::Respawning,
        Mode::Victory,
        Mode::GameOver,
        Mode::Attract,
    ] {
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
        let mut pixels = background_pattern(0, 0);
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct PackedBanks {
    bitmaps: [[u8; 320]; 2],
    screens: [[u8; 40]; 2],
    color_shadow: [u8; 40],
    front: usize,
}

impl PackedBanks {
    fn new() -> Self {
        Self {
            bitmaps: [[0; 320]; 2],
            screens: [[0; 40]; 2],
            color_shadow: [0; 40],
            front: 0,
        }
    }

    fn draw(&mut self, pixels: &[u8; 1280], reuse: bool) {
        let back = 1 - self.front;
        if !reuse {
            let (mut bitmap, mut screen, mut color) = ([0; 320], [0; 40], [0; 40]);
            pack_band(
                pixels,
                &self.screens[self.front],
                &self.color_shadow,
                &mut bitmap,
                &mut screen,
                &mut color,
            );
            self.bitmaps[back] = bitmap;
            self.screens[back] = screen;
            self.color_shadow = color;
        }
        self.front = back;
    }

    fn can_reuse(&self, cache: &SkyCache) -> bool {
        cache.reusable_bands() & (1 << 5) != 0
            && SkyCache::screens_match(&self.screens[self.front], &self.screens[1 - self.front])
    }
}

#[test]
fn scene_resets_match_forced_repainting_through_all_themes_and_rapid_switches() {
    let mut cache = SkyCache::new();
    let mut cached = PackedBanks::new();
    let mut forced = PackedBanks::new();
    let mut previous_scene = None;
    let mut scene_frames = 0;
    let mut hits = [0; 10];
    // Warm both banks in each scene, return to lowlands, then cut every frame.
    let scenes: Vec<_> = (0..10)
        .flat_map(|theme| [theme; 4])
        .chain([0, 0, 0, 0, 4, 6, 8, 9, 7, 2, 0, 0, 0, 0])
        .collect();
    for (frame, scene) in scenes.into_iter().enumerate() {
        if previous_scene != Some(scene) {
            cache = SkyCache::new();
            scene_frames = 0;
        }
        let pixels = background_pattern(scene, scene & 1);
        let reuse = cached.can_reuse(&cache);
        assert_eq!(
            reuse,
            scene_frames >= 2,
            "both physical banks must repaint after scene {scene} starts at frame {frame}"
        );
        cached.draw(&pixels, reuse);
        forced.draw(&pixels, false);
        cache.record_frame((cached.front + 2) as u8, 1 << 5);
        assert_eq!(cached, forced, "scene {scene}, frame {frame}");
        hits[scene] += usize::from(reuse);
        scene_frames += 1;
        previous_scene = Some(scene);
    }
    assert!(hits.into_iter().all(|count| count >= 2));
}

#[test]
fn scene_reset_is_required_even_when_new_backgrounds_keep_the_same_palette() {
    let old_pixels = background_pattern(0, 0);
    let new_pixels = background_pattern(0, 1);
    let mut cache = SkyCache::new();
    let mut cached = PackedBanks::new();
    for _ in 0..2 {
        cached.draw(&old_pixels, false);
        cache.record_frame((cached.front + 2) as u8, 1 << 5);
    }
    assert!(cached.can_reuse(&cache));
    let mut forced = cached.clone();
    forced.draw(&new_pixels, false);
    assert_eq!(cached.screens, forced.screens);
    assert_eq!(cached.color_shadow, forced.color_shadow);
    assert_ne!(cached.bitmaps[forced.front], forced.bitmaps[forced.front]);
    // Palette equality accepts the old banks, so only a scene reset prevents
    // the previous arrangement from surviving indefinitely in both bitmaps.
    cache = SkyCache::new();
    assert!(!cached.can_reuse(&cache));
    cached.draw(&new_pixels, false);
    cache.record_frame((cached.front + 2) as u8, 1 << 5);
    assert_eq!(cached, forced);
    assert!(!cached.can_reuse(&cache));
    for _ in 0..3 {
        let reuse = cached.can_reuse(&cache);
        cached.draw(&new_pixels, reuse);
        forced.draw(&new_pixels, false);
        cache.record_frame((cached.front + 2) as u8, 1 << 5);
        assert_eq!(cached, forced);
    }
    assert!(cached.can_reuse(&cache));
}
