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

fn empty_sections() -> [[u8; SEGMENT_SIZE]; 4] {
    core::array::from_fn(|index| {
        let mut bytes = segment_bytes();
        bytes[4] = index as u8;
        bytes
    })
}

fn advance(game: &mut Game, sections: &[[u8; SEGMENT_SIZE]; 4], ticks: u16) {
    for _ in 0..ticks {
        let segment = Segment::new(&sections[game.segment_index() as usize]).unwrap();
        game.tick(&segment);
    }
}

#[test]
fn attract_waits_fifteen_seconds_then_previews_each_section_and_returns_to_title() {
    let sections = empty_sections();
    let mut game = Game::new();
    advance(&mut game, &sections, 749);
    assert_eq!(game.hud().mode, Mode::Title);
    advance(&mut game, &sections, 1);
    for section in 0..4 {
        assert_eq!(game.hud().mode, Mode::Attract);
        assert_eq!(game.hud().section, section);
        assert_eq!(game.hud().distance, section as u32 * 30_000);
        advance(&mut game, &sections, 749);
        assert_eq!(game.hud().mode, Mode::Attract);
        assert_eq!(game.hud().section, section);
        assert_eq!(game.hud().distance, section as u32 * 30_000 + 5992);
        advance(&mut game, &sections, 1);
    }
    assert_eq!(game.hud().mode, Mode::Title);
    assert_eq!(game.hud().distance, 0);
    advance(&mut game, &sections, 749);
    assert_eq!(game.hud().mode, Mode::Title);
    advance(&mut game, &sections, 1);
    assert_eq!(game.hud().mode, Mode::Attract);
    assert_eq!(game.hud().section, 0);
}

#[test]
fn title_idle_timer_resets_for_short_pulses_and_stays_reset_while_a_direction_is_held() {
    let sections = empty_sections();
    let mut game = Game::new();
    advance(&mut game, &sections, 749);
    input(&mut game, LEFT, 14_985);
    input(&mut game, 0, 14_990);
    advance(&mut game, &sections, 1);
    advance(&mut game, &sections, 749);
    assert_eq!(game.hud().mode, Mode::Title);
    input(&mut game, RIGHT, 29_990);
    advance(&mut game, &sections, 1000);
    assert_eq!(game.hud().mode, Mode::Title);
    input(&mut game, 0, 49_990);
    advance(&mut game, &sections, 749);
    assert_eq!(game.hud().mode, Mode::Title);
    advance(&mut game, &sections, 1);
    assert_eq!(game.hud().mode, Mode::Attract);
}

#[test]
fn every_attract_wake_input_is_consumed_and_a_fresh_fire_starts_a_clean_run() {
    use havoc::game::{DOWN, UP};
    let sections = empty_sections();
    for wake in [UP, DOWN, LEFT, RIGHT, FIRE] {
        let mut game = Game::new();
        advance(&mut game, &sections, 750 + 750 * 3 + 300);
        assert_eq!(game.hud().section, 3);
        assert_eq!(game.hud().boss_shield, 100);
        input(&mut game, wake, 66_000);
        advance(&mut game, &sections, 1);
        assert_eq!(game.hud().mode, Mode::Title);
        assert_eq!(game.hud().distance, 0);
        assert_eq!(game.hud().boss_shield, 0);
        advance(&mut game, &sections, 800);
        assert_eq!(game.hud().mode, Mode::Title, "held input {wake}");
        input(&mut game, 0, 82_000);
        input(&mut game, FIRE, 82_020);
        advance(&mut game, &sections, 1);
        assert_eq!(game.hud().mode, Mode::Playing);
        assert_eq!(game.hud().distance, 0);
        assert_eq!(game.hud().score, 0);
        assert_eq!(game.hud().shield, 100);
        assert_eq!(game.hud().bombs, 3);
        assert_eq!(game.hud().lives, 3);
        assert_eq!(game.hud().charge, 0);
        assert_eq!(game.hud().roll_ticks, 0);
        assert!(effects(&mut game).is_empty());
        advance(&mut game, &sections, 1);
        assert_eq!(effects(&mut game), [Effect::Fire]);
        assert_eq!(game.hud().bombs, 3);
    }
}

#[test]
fn attract_retains_pressed_pulses_between_ticks_without_starting_the_game() {
    let sections = empty_sections();
    for pressed in [LEFT, FIRE] {
        let mut game = Game::new();
        advance(&mut game, &sections, 750);
        game.sample_input(InputSample {
            bits: 0,
            pressed,
            ms: 15_005,
        });
        input(&mut game, 0, 15_010);
        advance(&mut game, &sections, 1);
        assert_eq!(game.hud().mode, Mode::Title);
        advance(&mut game, &sections, 749);
        assert_eq!(game.hud().mode, Mode::Title);
        advance(&mut game, &sections, 1);
        assert_eq!(game.hud().mode, Mode::Attract);
    }
}

#[test]
fn attract_wake_overrides_a_section_cut_and_does_not_prime_a_rapid_fire_bomb() {
    let sections = empty_sections();
    let mut game = Game::new();
    advance(&mut game, &sections, 750 + 749);
    input(&mut game, FIRE, 29_990);
    advance(&mut game, &sections, 1);
    assert_eq!(game.hud().mode, Mode::Title);
    assert_eq!(game.hud().section, 0);
    input(&mut game, 0, 30_000);
    input(&mut game, FIRE, 30_020);
    advance(&mut game, &sections, 1);
    assert_eq!(game.hud().mode, Mode::Playing);
    assert_eq!(game.hud().distance, 0);
    advance(&mut game, &sections, 1);
    assert_eq!(effects(&mut game), [Effect::Fire]);
    assert_eq!(game.hud().bombs, 3);
}

#[cfg(feature = "host-tools")]
#[test]
fn attract_runs_live_encounters_and_the_carrier_without_sfx_or_resource_changes() {
    let sections = core::array::from_fn(|i| baked_assets::segment_bank(i as u8));
    let common = baked_assets::common_bank();
    let bank = havoc::assets::AssetBank::new(&common).unwrap();
    let mut game = Game::new();
    advance(&mut game, &sections, 750);
    for section in 0..4 {
        for tick in 1..750 {
            advance(&mut game, &sections, 1);
            let status = game.hud();
            assert_eq!(status.mode, Mode::Attract);
            assert_eq!(
                (status.shield, status.bombs, status.lives, status.score),
                (100, 3, 3, 0)
            );
            assert!(effects(&mut game).is_empty(), "section {section}, tick {tick}");
            if section == 3 {
                assert_eq!(status.boss_shield, if tick < 150 { 0 } else { 100 });
            }
            if tick % 50 == 0 {
                let segment = Segment::new(&sections[section]).unwrap();
                let mut renderer = havoc::render::Renderer::new();
                renderer.begin(game.sky_color());
                game.render(&mut renderer, &bank, &segment);
                renderer.finish();
                assert_eq!(renderer.dropped, 0, "section {section}, tick {tick}");
                assert!(renderer.triangle_count > 10);
            }
        }
        advance(&mut game, &sections, 1);
    }
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
    assert!(effects(&mut g).contains(&Effect::MissileLaunch));
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
fn only_full_charge_uses_the_missile_launch_sound() {
    let bytes = segment_bytes();
    let segment = Segment::new(&bytes).unwrap();
    for (elapsed, charge) in [(1000, 0), (1004, 1), (1500, 127), (1999, 254), (2000, 255)] {
        let mut game = started(&segment);
        input(&mut game, 0, 20);
        game.tick(&segment);
        input(&mut game, 0, 20 + elapsed);
        assert_eq!(game.hud().charge, charge);
        input(&mut game, FIRE, 20 + elapsed);
        game.tick(&segment);
        let expected = if charge == 255 {
            Effect::MissileLaunch
        } else {
            Effect::Fire
        };
        assert_eq!(effects(&mut game), [expected], "charge {charge}");
        assert_eq!(game.hud().charge, 0);
    }
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
    input(&mut g, FIRE, 600);
    g.tick(&s);
    assert_eq!(g.hud().bombs, 2);
    assert!(effects(&mut g).contains(&Effect::Bomb));
    input(&mut g, 0, 620);
    input(&mut g, FIRE, 650);
    g.tick(&s);
    assert_eq!(g.hud().bombs, 2, "third tap starts a new pair");
}

#[test]
fn double_tap_boundaries_survive_timestamp_wrap_and_delayed_simulation() {
    let bytes = segment_bytes();
    let s = Segment::new(&bytes).unwrap();
    for bits in [FIRE, LEFT, RIGHT] {
        for first_at in [100_u32, u32::MAX - 250] {
            for (gap, accepted) in [(500, true), (501, false)] {
                for delayed in [false, true] {
                    let mut g = started(&s);
                    input(&mut g, 0, first_at.wrapping_sub(20));
                    input(&mut g, bits, first_at);
                    if !delayed {
                        g.tick(&s);
                    }
                    input(&mut g, 0, first_at.wrapping_add(20));
                    input(&mut g, bits, first_at.wrapping_add(gap));
                    g.tick(&s);
                    assert_eq!(
                        (g.hud().bombs, g.hud().roll_ticks),
                        (
                            if bits == FIRE && accepted { 2 } else { 3 },
                            if bits != FIRE && accepted { 25 } else { 0 },
                        ),
                        "bits={bits}, first_at={first_at}, gap={gap}, delayed={delayed}"
                    );
                }
            }
        }
    }
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
            renderer.begin(game.sky_color());
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
fn attract_renders_the_game_world_without_the_ship_exhaust_or_reticle() {
    let sections = empty_sections();
    let segment = Segment::new(&sections[0]).unwrap();
    let common = baked_assets::common_bank();
    let bank = havoc::assets::AssetBank::new(&common).unwrap();
    let playing = started(&segment);
    let mut attract = Game::new();
    advance(&mut attract, &sections, 750);
    let mut normal_scene = CapturedGeometry::default();
    let mut preview_scene = CapturedGeometry::default();
    playing.render(&mut normal_scene, &bank, &segment);
    attract.render(&mut preview_scene, &bank, &segment);
    let player_faces = bank.face_count(havoc::assets::PLAYER) as usize + 4;
    assert_eq!(preview_scene.0, normal_scene.0[player_faces..]);
    assert!(!preview_scene.0.is_empty());
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
            scene.begin(game.sky_color());
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
fn chase_camera_translates_foreground_without_moving_world_geometry_or_backdrop() {
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
    // their initial world positions. Only gameplay adds two reticle faces;
    // all 14 final backdrop faces remain fixed in camera space.
    let reticle_start = bank.face_count(PLAYER) as usize + 2;
    assert_eq!(playing_geometry.0.len(), title_geometry.0.len() + 2);
    for (index, &(vertices, color, flags)) in title_geometry.0.iter().enumerate() {
        let gameplay_index = index + usize::from(index >= reticle_start) * 2;
        let shifted = if index >= title_geometry.0.len() - 14 {
            vertices
        } else {
            vertices.map(|p| Vec3 { y: p.y - 40, ..p })
        };
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
