use havoc::{
    game::{Game, Mode},
    hud,
    render::{Renderer, BAND_PIXELS},
    space_dust::{SpaceDustFrame, COLOR, SPECK_COUNT},
    GeometrySink, Vec3, TWO_SIDED,
};
use std::collections::BTreeSet;

fn prepared(ticks: u16) -> SpaceDustFrame {
    let mut frame = SpaceDustFrame::EMPTY;
    assert!(frame.prepare(12, Mode::Playing, ticks).is_some());
    frame
}

fn raster(renderer: &Renderer, ambient: Option<&SpaceDustFrame>) -> Vec<u8> {
    let mut output = Vec::new();
    for band in 0..25 {
        let mut pixels = [0; BAND_PIXELS];
        renderer.render_band_with_ambient(band, &mut pixels, ambient);
        output.extend_from_slice(&pixels);
    }
    output
}

#[test]
fn space_flight_modes_only_and_crossings_keep_the_same_phase() {
    let expected = prepared(193);
    let mut frame = SpaceDustFrame::EMPTY;
    for stage in 0..=18 {
        for mode in [
            Mode::Title,
            Mode::Playing,
            Mode::Respawning,
            Mode::Victory,
            Mode::GameOver,
            Mode::Attract,
            Mode::Transition,
        ] {
            let active = (12..18).contains(&stage)
                && matches!(
                    mode,
                    Mode::Playing | Mode::Respawning | Mode::Attract | Mode::Transition
                );
            let ambient = frame.prepare(stage, mode, 193);
            assert_eq!(ambient.is_some(), active, "{stage} {mode:?}");
            if let Some(frame) = ambient {
                assert_eq!(*frame, expected, "stage/mode changes must not reset motion");
            }
        }
    }
}

#[test]
fn clock_wrap_and_period_are_exact_without_stage_local_time() {
    for phase in 0..256 {
        assert_eq!(prepared(phase), prepared(phase + 256));
        assert_eq!(prepared(phase), prepared(phase + 0xff00));
    }
    assert_eq!(prepared(u16::MAX), prepared(255));
    assert_eq!(prepared(u16::MAX.wrapping_add(1)), prepared(256));
}

#[test]
fn rays_accelerate_outward_then_respawn_from_outside_the_view() {
    for index in 0..SPECK_COUNT {
        let mut previous_radius = 0;
        let mut saw_offscreen = false;
        for phase in 0..=255u8 {
            let ticks = phase.wrapping_sub(index as u8 * 32) as u16;
            let (x, y) = prepared(ticks).positions()[index];
            if y == 255 {
                saw_offscreen = true;
                continue;
            }
            assert!(!saw_offscreen, "ray reentered before its phase wrapped");
            let radius = (x as i16 - 80).abs().max((y as i16 - 100).abs());
            assert!(radius >= previous_radius);
            previous_radius = radius;
        }
        assert!(
            saw_offscreen,
            "every ray must leave the viewport before resetting"
        );
        let origin = prepared((index as u8 * 32).wrapping_neg() as u16);
        assert_eq!(origin.positions()[index], (80, 100));
    }
    // The second half covers substantially more distance than the first half.
    assert_eq!(prepared(64).positions()[0].0 - 80, 8);
    assert_eq!(prepared(128).positions()[0].0 - 80, 32);
    assert_eq!(prepared(192).positions()[0].0 - 80, 72);
}

#[test]
fn each_visible_speck_is_one_plain_gray_pixel_with_no_trail() {
    let mut renderer = Renderer::new();
    renderer.begin(0);
    for ticks in 0..256 {
        let frame = prepared(ticks);
        let expected: BTreeSet<_> = frame
            .positions()
            .iter()
            .filter(|&&(_, y)| y < 200)
            .map(|&(x, y)| y as usize * 160 + x as usize)
            .collect();
        let pixels = raster(&renderer, Some(&frame));
        let actual: BTreeSet<_> = pixels
            .iter()
            .enumerate()
            .filter(|(_, color)| **color != 0)
            .map(|(index, &color)| {
                assert_eq!(color, COLOR, "no HUD or terrain-coverage flag");
                index
            })
            .collect();
        assert_eq!(actual, expected);
        assert!(actual.len() <= SPECK_COUNT);
    }
    assert_eq!(renderer.triangle_count, 0);
    assert_eq!(renderer.dropped, 0);
}

#[test]
fn packed_vic_frames_preserve_single_gray_specks_without_double_buffer_trails() {
    let mut renderer = Renderer::new();
    renderer.begin(0);
    let mut bitmaps = [[0u8; 8000]; 2];
    let mut screens = [[0u8; 1000]; 2];
    let mut colors = [0u8; 1000];
    // Include the phase wrap. Each back bank still contains its old frame,
    // while packing receives the preceding front frame's palette and colors.
    for tick in 0..=256u16 {
        let frame = prepared(tick);
        let expected: BTreeSet<_> = frame
            .positions()
            .iter()
            .filter(|&&(_, y)| y < 200)
            .map(|&(x, y)| y as usize * 160 + x as usize)
            .collect();
        let back = tick as usize & 1;
        let front = back ^ 1;
        let mut actual = BTreeSet::new();
        for band in 0..25usize {
            let mut pixels = [0; BAND_PIXELS];
            renderer.render_band_with_ambient(band as u8, &mut pixels, Some(&frame));
            let cells = band * 40..(band + 1) * 40;
            let previous_screen: [u8; 40] = screens[front][cells.clone()].try_into().unwrap();
            let previous_color: [u8; 40] = colors[cells.clone()].try_into().unwrap();
            let bitmap: &mut [u8; 320] = (&mut bitmaps[back][band * 320..(band + 1) * 320])
                .try_into()
                .unwrap();
            let screen: &mut [u8; 40] = (&mut screens[back][cells.clone()]).try_into().unwrap();
            let color: &mut [u8; 40] = (&mut colors[cells]).try_into().unwrap();
            havoc::render::pack_band(
                &pixels,
                &previous_screen,
                &previous_color,
                0,
                bitmap,
                screen,
                color,
            );
            for y in 0..8 {
                for x in 0..160 {
                    let cell = x / 4;
                    let slot = (bitmap[cell * 8 + y] >> (6 - (x % 4) * 2)) & 3;
                    let decoded = match slot {
                        0 => 0,
                        1 => screen[cell] >> 4,
                        2 => screen[cell] & 15,
                        _ => color[cell] & 15,
                    };
                    if decoded != 0 {
                        assert_eq!(decoded, COLOR, "packed color changed at tick {tick}");
                        actual.insert((band * 8 + y) * 160 + x);
                    }
                }
            }
        }
        assert_eq!(
            actual, expected,
            "packed dot footprint/trail at tick {tick}"
        );
    }
}

fn rectangle(renderer: &mut Renderer, color: u8) {
    let v = |x, y| Vec3 { x, y, z: 128 };
    renderer.triangle(v(-32, -32), v(-32, 32), v(32, 32), color, TWO_SIDED);
    renderer.triangle(v(-32, -32), v(32, 32), v(32, -32), color, TWO_SIDED);
    renderer.finish();
}

#[test]
fn black_and_colored_geometry_occlude_dust_in_both_raster_paths() {
    let dust = prepared(0);
    for color in [0, 5] {
        let mut renderer = Renderer::new();
        renderer.begin(6);
        rectangle(&mut renderer, color);
        let expected = raster(&renderer, Some(&dust));
        assert_eq!(expected[100 * 160 + 80], color);
        assert!(expected.contains(&COLOR), "uncovered specks remain visible");
        let count = renderer.triangle_count;
        let mut pass = renderer.raster_pass().unwrap();
        for band in 0..25 {
            let mut pixels = [99; BAND_PIXELS];
            assert_eq!(
                pass.next_band_with_ambient(&mut pixels, true, Some(&dust)),
                Some(band)
            );
            assert_eq!(
                &pixels,
                &expected[band as usize * 1280..(band as usize + 1) * 1280]
            );
        }
        let mut sentinel = [99; BAND_PIXELS];
        assert_eq!(
            pass.next_band_with_ambient(&mut sentinel, true, Some(&dust)),
            None
        );
        assert_eq!(sentinel, [99; BAND_PIXELS]);
        drop(pass);
        assert_eq!(renderer.triangle_count, count);
        assert_eq!(renderer.dropped, 0);
    }
}

#[test]
fn hidden_bands_advance_edges_without_writing_dust_and_none_keeps_old_pixels() {
    let dust = prepared(0);
    let mut renderer = Renderer::new();
    renderer.begin(6);
    rectangle(&mut renderer, 5);
    let original = raster(&renderer, None);
    let mut pass = renderer.raster_pass().unwrap();
    for band in 0..25 {
        let mut pixels = [99; BAND_PIXELS];
        let paint = band & 1 != 0;
        assert_eq!(
            pass.next_band_with_ambient(&mut pixels, paint, None),
            Some(band)
        );
        if paint {
            assert_eq!(
                &pixels,
                &original[band as usize * 1280..(band as usize + 1) * 1280]
            );
        } else {
            assert_eq!(pixels, [99; BAND_PIXELS]);
        }
    }
    drop(pass);
    renderer.begin(6);
    let mut pass = renderer.raster_pass().unwrap();
    for band in 0..25 {
        let mut pixels = [99; BAND_PIXELS];
        assert_eq!(
            pass.next_band_with_ambient(&mut pixels, false, Some(&dust)),
            Some(band)
        );
        assert_eq!(pixels, [99; BAND_PIXELS]);
    }
}

#[test]
fn cockpit_is_painted_after_dust() {
    let status = Game::preview_at(12, 0).hud();
    let mut renderer = Renderer::new();
    renderer.begin(0);
    let mut checked = false;
    for ticks in 0..256 {
        let dust = prepared(ticks);
        if let Some(&(x, y)) = dust.positions().iter().find(|&&(_, y)| y < 8) {
            let mut pixels = [0; BAND_PIXELS];
            renderer.render_band_with_ambient(0, &mut pixels, Some(&dust));
            assert_eq!(pixels[y as usize * 160 + x as usize], COLOR);
            let mut without = [0; BAND_PIXELS];
            hud::draw(0, &mut pixels, &status, 0);
            hud::draw(0, &mut without, &status, 0);
            assert_eq!(pixels, without);
            checked = true;
            break;
        }
    }
    assert!(checked);
}
