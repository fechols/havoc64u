#![cfg(feature = "host-tools")]

use havoc::assets::{AssetBank, Segment, SEGMENT_COUNT};
use havoc::game::Game;
use havoc::render::{Renderer, BAND_PIXELS};
use havoc::shadow::{Caster, ShadowFrame};
use havoc::{GeometrySink, Vec3};

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn pixels(renderer: &Renderer) -> Vec<u8> {
    let mut result = Vec::with_capacity(160 * 200);
    for band in 0..25 {
        let mut row = [0; BAND_PIXELS];
        renderer.render_band(band, &mut row);
        #[cfg(feature = "span-buffer")]
        {
            let mut spans = [0; BAND_PIXELS];
            renderer.render_band_spans(band, &mut spans, None);
            assert_eq!(spans, row, "span/reference pixels, band {band}");
        }
        result.extend_from_slice(&row);
    }
    result
}

// The unchanged native command ABI is padding-free; preserve order as well as
// every byte of the ordinary prefix, without relying on a shadow marker.
fn commands(renderer: &Renderer) -> Vec<[u8; 16]> {
    assert_eq!(core::mem::size_of::<Renderer>(), 6 + 256 * 16);
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (renderer as *const Renderer).cast::<u8>().add(6),
            renderer.triangle_count as usize * 16,
        )
    };
    bytes
        .chunks_exact(16)
        .map(|command| command.try_into().unwrap())
        .collect()
}

fn compare_scene(game: &Game) -> (u16, u8) {
    let stage = game.segment_index();
    let models = baked_assets::model_bank(stage);
    let data = baked_assets::segment_bank(stage);
    let bank = AssetBank::new(&models).unwrap();
    let segment = Segment::new(&data).unwrap();
    let mut base = Renderer::new();
    base.begin(game.sky_color());
    game.render(&mut base, &bank, &segment);
    base.finish();
    let mut shaded = Renderer::new();
    shaded.begin(game.sky_color());
    let mut frame = ShadowFrame::new();
    game.render_with_shadows(&mut shaded, &mut frame, &bank, &segment);
    assert_eq!(frame.base_commands(), base.triangle_count, "stage {stage}");
    assert_eq!(
        commands(&base),
        commands(&shaded)[..frame.base_commands() as usize],
        "stage {stage}"
    );
    assert_eq!(base.dropped, shaded.dropped, "stage {stage}");
    assert_eq!(
        shaded.triangle_count - base.triangle_count,
        frame.shadow_commands()
    );
    assert!(frame.caster_count() <= 64);
    for command in &commands(&shaded)[frame.base_commands() as usize..] {
        assert_eq!(command[14], 11, "shadow ink in stage {stage}");
        assert_eq!(command[15], 0x40, "shadow flags in stage {stage}");
    }
    if game.shadow_ground_y().is_none() {
        assert_eq!(
            (frame.shadow_commands(), frame.caster_count()),
            (0, 0),
            "stage {stage}"
        );
        assert_eq!(pixels(&base), pixels(&shaded), "disabled stage {stage}");
    }
    (frame.shadow_commands(), frame.caster_count())
}

#[test]
fn flat_shadows_preserve_ordinary_commands_in_every_stage_and_transition() {
    let mut visible = 0;
    for stage in 0..SEGMENT_COUNT {
        let models = baked_assets::model_bank(stage);
        let data = baked_assets::segment_bank(stage);
        let bank = AssetBank::new(&models).unwrap();
        let segment = Segment::new(&data).unwrap();
        for distance in [0, 1400, 8000, 16000, 29000] {
            let mut game = Game::preview_at(stage, distance);
            game.tick(&segment, &bank);
            let (commands, casters) = compare_scene(&game);
            if stage < 12 || stage == 16 {
                assert!(casters > 0, "stage {stage}");
            }
            visible += usize::from(commands > 0);
        }
        if stage + 1 < SEGMENT_COUNT {
            for tick in [0, 99, 199, 200, 299, 399, 400] {
                compare_scene(&Game::preview_transition(stage, tick));
            }
        }
    }
    assert!(visible >= 40, "only {visible} shadowed scenes");
    for tick in [0, 30, 60, 119, 120, 180, 360] {
        compare_scene(&Game::preview_victory(tick));
    }
    compare_scene(&Game::new());
}

fn background(renderer: &mut Renderer, y: i16, slope: i16, color: u8) {
    let p = |x, z| Vec3 {
        x,
        y: y + x / 4 * slope,
        z,
    };
    let [a, b, c, d] = [p(-200, 160), p(200, 160), p(200, 800), p(-200, 800)];
    renderer.triangle(a, d, c, color, 0);
    renderer.triangle(a, c, b, color, 0);
}

#[test]
fn changing_or_removing_scenery_cannot_change_projected_shadows() {
    let source = Caster {
        center: Vec3 {
            x: 0,
            y: 100,
            z: 440,
        },
        half_x: 100,
        half_z: 120,
        shape: 1,
    };
    let mut expected = None;
    for scene in 0..5 {
        let mut renderer = Renderer::new();
        renderer.begin(6);
        if scene != 0 {
            background(&mut renderer, -105, scene - 2, 5);
        }
        if scene >= 3 {
            background(&mut renderer, -50, 0, 12);
        }
        let mut frame = ShadowFrame::new();
        frame.push_caster(source);
        frame.render(&mut renderer, -105, 11);
        let suffix = commands(&renderer)[frame.base_commands() as usize..].to_vec();
        assert_eq!(frame.omitted_casters(), 0);
        assert!(!suffix.is_empty());
        if let Some(ref expected) = expected {
            assert_eq!(&suffix, expected);
        } else {
            expected = Some(suffix);
        }
        pixels(&renderer);
    }
}

#[test]
fn raised_objects_do_not_clip_or_receive_a_different_shadow() {
    let mut renderer = Renderer::new();
    renderer.begin(6);
    background(&mut renderer, -50, 0, 12);
    let mut frame = ShadowFrame::new();
    frame.push_caster(Caster {
        center: Vec3 {
            x: 0,
            y: 100,
            z: 440,
        },
        half_x: 100,
        half_z: 120,
        shape: 4,
    });
    frame.render(&mut renderer, -105, 11);
    let actual = pixels(&renderer);
    let mut reference = Renderer::new();
    reference.begin(6);
    background(&mut reference, -50, 0, 12);
    reference.finish();
    let before = pixels(&reference);
    let changed: Vec<_> = before.iter().zip(&actual).filter(|(a, b)| a != b).collect();
    assert!(changed.len() > 30);
    assert!(changed.iter().all(|(_, after)| **after == 11));
}
