use havoc::assets::{Segment, SEGMENT_SIZE};
use havoc::audio::Effect;
use havoc::game::{Game, InputSample, Mode, FIRE, LEFT, RIGHT};

#[cfg(feature = "host-tools")]
#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn segment_bytes() -> [u8; SEGMENT_SIZE] {
    let mut bytes = [0; SEGMENT_SIZE];
    bytes[..4].copy_from_slice(b"SMS1");
    bytes[6] = 5;
    bytes
}
fn input(game: &mut Game, bits: u8, ms: u32) {
    game.sample_input(InputSample {
        bits,
        pressed: 0,
        ms,
    });
}
fn started(segment: &Segment<'_>) -> Game {
    let mut game = Game::new();
    input(&mut game, FIRE, 0);
    game.tick(segment);
    assert_eq!(game.hud().mode, Mode::Playing);
    while game.pop_audio().is_some() {}
    game
}
fn effects(game: &mut Game) -> Vec<Effect> {
    let mut result = Vec::new();
    while let Some(e) = game.pop_audio() {
        result.push(e.effect);
    }
    result
}

#[test]
fn held_fire_produces_ten_immediate_cadenced_shots_per_second() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    let mut count = 0;
    for i in 1..=50 {
        input(&mut g, FIRE, i * 20);
        g.tick(&s);
        count += effects(&mut g)
            .iter()
            .filter(|&&e| e == Effect::Fire)
            .count();
    }
    assert_eq!(count, 10);
    assert_eq!(g.hud().bombs, 3, "holding fire is never a double tap");
}

#[test]
fn charge_begins_after_one_second_and_is_retained_until_press() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    input(&mut g, 0, 20);
    g.tick(&s);
    input(&mut g, 0, 1020);
    assert_eq!(g.hud().charge, 0);
    input(&mut g, 0, 1520);
    assert_eq!(g.hud().charge, 127);
    input(&mut g, 0, 2020);
    assert_eq!(g.hud().charge, 255);
    input(&mut g, 0, 4020);
    assert_eq!(g.hud().charge, 255);
    input(&mut g, FIRE, 4040);
    g.tick(&s);
    assert!(effects(&mut g).contains(&Effect::ChargedShot));
    assert_eq!(g.hud().charge, 0);
    for i in 1..=5 {
        input(&mut g, FIRE, 4040 + i * 20);
        g.tick(&s);
    }
    assert!(
        effects(&mut g).contains(&Effect::Fire),
        "autofire resumes after charged shot"
    );
}

#[test]
fn inclusive_double_tap_window_consumes_pairs_and_first_shot_is_immediate() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    input(&mut g, 0, 20);
    g.tick(&s);
    input(&mut g, FIRE, 100);
    g.tick(&s);
    assert!(effects(&mut g).contains(&Effect::Fire));
    assert_eq!(g.hud().bombs, 3);
    input(&mut g, 0, 120);
    input(&mut g, FIRE, 350);
    g.tick(&s);
    assert_eq!(g.hud().bombs, 2);
    assert!(effects(&mut g).contains(&Effect::Bomb));
    input(&mut g, 0, 370);
    input(&mut g, FIRE, 400);
    g.tick(&s);
    assert_eq!(g.hud().bombs, 2, "third tap starts a new pair");
}

#[test]
fn a_251_millisecond_gap_is_not_a_double_tap() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    input(&mut g, 0, 20);
    input(&mut g, FIRE, 100);
    g.tick(&s);
    input(&mut g, 0, 120);
    input(&mut g, FIRE, 351);
    g.tick(&s);
    assert_eq!(g.hud().bombs, 3);
}

#[test]
fn fire_edges_between_simulation_ticks_are_preserved() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    input(&mut g, 0, 17);
    input(&mut g, FIRE, 33);
    input(&mut g, 0, 50);
    g.tick(&s);
    assert!(effects(&mut g).contains(&Effect::Fire));
    input(&mut g, FIRE, 67);
    input(&mut g, 0, 83);
    g.tick(&s);
    assert_eq!(g.hud().bombs, 2);
}

#[test]
fn bomb_inventory_cannot_underflow() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    input(&mut g, 0, 20);
    for pair in 0..4 {
        let time = 100 + pair * 600;
        input(&mut g, FIRE, time);
        g.tick(&s);
        input(&mut g, 0, time + 20);
        input(&mut g, FIRE, time + 100);
        g.tick(&s);
        input(&mut g, 0, time + 120);
        effects(&mut g);
    }
    assert_eq!(g.hud().bombs, 0);
}

#[test]
fn rolls_last_half_a_second_and_cannot_restart_inside_cooldown() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut g = started(&s);
    input(&mut g, LEFT, 100);
    g.tick(&s);
    input(&mut g, 0, 120);
    input(&mut g, LEFT, 350);
    g.tick(&s);
    assert_eq!(g.hud().roll_ticks, 25);
    for i in 1..=25 {
        input(&mut g, LEFT, 350 + i * 20);
        g.tick(&s);
    }
    assert_eq!(
        g.hud().roll_ticks,
        0,
        "holding a direction cannot repeat a roll"
    );
    input(&mut g, RIGHT, 870);
    input(&mut g, 0, 880);
    input(&mut g, RIGHT, 890);
    g.tick(&s);
    assert_eq!(g.hud().roll_ticks, 0);
    for i in 1..=25 {
        input(&mut g, 0, 890 + i * 20);
        g.tick(&s);
    }
    input(&mut g, RIGHT, 1410);
    input(&mut g, 0, 1430);
    input(&mut g, RIGHT, 1450);
    g.tick(&s);
    assert_eq!(g.hud().roll_ticks, 25);
}

#[test]
fn deterministic_inputs_produce_identical_game_state() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    let mut a = started(&s);
    let mut b = started(&s);
    for tick in 1..2000u32 {
        let bits = if tick % 40 < 20 { LEFT | FIRE } else { RIGHT };
        input(&mut a, bits, tick * 20);
        input(&mut b, bits, tick * 20);
        a.tick(&s);
        b.tick(&s);
        assert_eq!(a.hud(), b.hud());
        assert_eq!(effects(&mut a), effects(&mut b));
    }
}

#[cfg(feature = "host-tools")]
#[test]
fn original_asset_previews_fit_the_triangle_budget_in_every_section() {
    let common = baked_assets::common_bank();
    let bank = havoc::assets::AssetBank::new(&common).unwrap();
    for section in 0..4 {
        let bytes = baked_assets::segment_bank(section);
        let segment = Segment::new(&bytes).unwrap();
        // Every 20 ms phase of a 512-unit terrain band, plus later encounters.
        for local in (0..512).step_by(8).chain([1400, 2500, 10000, 28000]) {
            let mut game = Game::preview_at(section, local);
            game.tick(&segment);
            let mut renderer = havoc::render::Renderer::new();
            renderer.begin(0);
            game.render(&mut renderer, &bank, &segment);
            renderer.finish();
            assert_eq!(renderer.dropped, 0, "section {section}, distance {local}");
            assert!(renderer.triangle_count > 10);
        }
    }
}

#[cfg(feature = "host-tools")]
#[derive(Default)]
struct CapturedGeometry(Vec<([havoc::Vec3; 3], u8, u8)>);

#[cfg(feature = "host-tools")]
impl havoc::GeometrySink for CapturedGeometry {
    fn triangle(&mut self, a: havoc::Vec3, b: havoc::Vec3, c: havoc::Vec3, color: u8, flags: u8) {
        self.0.push(([a, b, c], color, flags));
    }
}

#[cfg(feature = "host-tools")]
#[test]
fn scenic_terrain_keeps_the_centered_player_visible_through_every_band_phase() {
    use havoc::{assets::AssetBank, render::Renderer, GeometrySink};
    let common = baked_assets::common_bank();
    let bank = AssetBank::new(&common).unwrap();
    let player_faces = bank.face_count(havoc::assets::PLAYER) as usize + 2; // Hull and exhaust.
    for section in 0..4 {
        let bytes = baked_assets::segment_bank(section);
        let segment = Segment::new(&bytes).unwrap();
        for distance in (0..512).step_by(8) {
            let game = Game::preview_at(section, distance);
            let mut geometry = CapturedGeometry::default();
            game.render(&mut geometry, &bank, &segment);
            let mut player = Renderer::new();
            let mut scene = Renderer::new();
            player.begin(0);
            scene.begin(0);
            for (index, &(points, color, flags)) in geometry.0.iter().enumerate() {
                scene.triangle(points[0], points[1], points[2], color, flags);
                if index < player_faces {
                    player.triangle(points[0], points[1], points[2], color, flags);
                }
            }
            player.finish();
            scene.finish();
            for band in 14..19 {
                let mut expected = [0u8; 1280];
                let mut actual = [0u8; 1280];
                player.render_band(band, &mut expected);
                scene.render_band(band, &mut actual);
                for (pixel, (&ship, &full)) in expected.iter().zip(&actual).enumerate() {
                    if ship != 0 {
                        assert_eq!(
                            full,
                            ship,
                            "section {section}, distance {distance}, pixel ({}, {})",
                            pixel % 160,
                            band as usize * 8 + pixel / 160
                        );
                    }
                }
            }
        }
    }
}

#[cfg(feature = "host-tools")]
#[test]
fn chase_camera_translates_the_scene_without_moving_world_geometry_or_title() {
    use havoc::{
        assets::{AssetBank, PLAYER},
        emit_mesh, Vec3,
    };
    let common = baked_assets::common_bank();
    let bank = AssetBank::new(&common).unwrap();
    let bytes = segment_bytes();
    let segment = Segment::new(&bytes).unwrap();
    let title = Game::new();
    let playing = started(&segment);
    let mut title_geometry = CapturedGeometry::default();
    let mut playing_geometry = CapturedGeometry::default();
    title.render(&mut title_geometry, &bank, &segment);
    playing.render(&mut playing_geometry, &bank, &segment);

    // Starting via ordinary fire leaves the ship, terrain and animation at
    // their initial world positions. Only gameplay adds two reticle faces.
    let reticle_start = bank.face_count(PLAYER) as usize + 2;
    assert_eq!(playing_geometry.0.len(), title_geometry.0.len() + 2);
    for (index, &(vertices, color, flags)) in title_geometry.0.iter().enumerate() {
        let gameplay_index = index + usize::from(index >= reticle_start) * 2;
        let shifted = vertices.map(|p| Vec3 { y: p.y - 40, ..p });
        assert_eq!(playing_geometry.0[gameplay_index], (shifted, color, flags));
    }
    let mut world_ship = CapturedGeometry::default();
    emit_mesh(
        &mut world_ship,
        &bank,
        PLAYER,
        Vec3 { x: 0, y: 0, z: 180 },
        0,
        0,
        0,
        256,
    );
    assert_eq!(
        &title_geometry.0[..bank.face_count(PLAYER) as usize],
        world_ship.0.as_slice()
    );
    assert_eq!(
        playing.hud().distance,
        0,
        "rendering must not advance simulation"
    );
}

#[cfg(feature = "host-tools")]
#[test]
fn chase_camera_reveals_carrier_and_reticle_and_keeps_controlled_hull_on_screen() {
    use havoc::{
        assets::{AssetBank, CARRIER, PLAYER},
        emit_mesh,
        game::{DOWN, UP},
        projection::project,
        Vec3,
    };
    let common = baked_assets::common_bank();
    let bank = AssetBank::new(&common).unwrap();
    let carrier_bytes = baked_assets::segment_bank(3);
    let carrier_segment = Segment::new(&carrier_bytes).unwrap();
    let carrier = Game::preview_at(3, 1200);
    let mut geometry = CapturedGeometry::default();
    carrier.render(&mut geometry, &bank, &carrier_segment);
    let ship_faces = bank.face_count(PLAYER) as usize;
    let carrier_start = ship_faces + 4; // Exhaust and aiming reticle each use two faces.
    let bounds = |faces: &[([havoc::Vec3; 3], u8, u8)]| {
        faces
            .iter()
            .flat_map(|face| face.0)
            .map(|p| project(p).1)
            .fold((i16::MAX, i16::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)))
    };
    let player_y = bounds(&geometry.0[..ship_faces]);
    let target_y =
        bounds(&geometry.0[carrier_start..carrier_start + bank.face_count(CARRIER) as usize]);
    let reticle_y = bounds(&geometry.0[ship_faces + 2..carrier_start]);
    assert!(
        player_y.0 > target_y.1 + 6,
        "player {player_y:?} must not conceal carrier {target_y:?}"
    );
    assert!(
        reticle_y.0 >= 105 && reticle_y.1 <= 110,
        "reticle remains on the distant firing line: {reticle_y:?}"
    );
    assert!(player_y.0 >= 120 && player_y.1 <= 145);

    let bytes = segment_bytes();
    let segment = Segment::new(&bytes).unwrap();
    for direction in [UP, DOWN, UP | LEFT, UP | RIGHT, DOWN | LEFT, DOWN | RIGHT] {
        let mut game = started(&segment);
        for tick in 1..=60 {
            input(&mut game, direction, tick * 20);
            game.tick(&segment);
        }
        let mut geometry = CapturedGeometry::default();
        game.render(&mut geometry, &bank, &segment);
        let world_y = if direction & UP != 0 { 65 } else { -65 };
        let (world_x, bank_angle) = if direction & LEFT != 0 {
            (-100, 12)
        } else if direction & RIGHT != 0 {
            (100, (-12i8) as u8)
        } else {
            (0, 0)
        };
        let mut expected = CapturedGeometry::default();
        emit_mesh(
            &mut expected,
            &bank,
            PLAYER,
            Vec3 {
                x: world_x,
                y: world_y,
                z: 180,
            },
            0,
            0,
            bank_angle,
            256,
        );
        let eye_y = 40 + world_y / 4;
        for face in &mut expected.0 {
            face.0 = face.0.map(|p| Vec3 {
                y: p.y - eye_y,
                ..p
            });
        }
        assert_eq!(
            &geometry.0[..ship_faces],
            expected.0.as_slice(),
            "chase eye must follow the ordinary world pose by one quarter"
        );
        let y = bounds(&geometry.0[..ship_faces]);
        assert!(
            y.0 >= 0 && y.1 < 185,
            "hull touches the cockpit at vertical/banking limit: {y:?}"
        );
    }
}
