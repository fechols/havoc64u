use super::*;
use crate::shadow::{Caster, ShadowFrame};

#[allow(dead_code)]
#[path = "../host.rs"]
mod baked;

fn populated(stage: u8) -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, stage, 0);
    game.player.y = 32;
    game.boss_started = true;
    game.boss_z = 800;
    for (i, actor) in game.actors.iter_mut().enumerate() {
        *actor = Actor {
            kind: if i & 1 == 0 {
                assets::FIGHTER
            } else {
                assets::DIVER
            },
            hp: 3,
            pos: Vec3 {
                x: i as i16 * 12 - 60,
                y: 20,
                z: 350 + i as i16 * 30,
            },
            ..Actor::EMPTY
        };
    }
    for (i, shot) in game.shots.iter_mut().enumerate() {
        *shot = Shot {
            kind: 1 + (i % 3) as u8,
            damage: 2,
            pos: Vec3 {
                x: i as i16 - 12,
                y: 10,
                z: 400 + i as i16 * 10,
            },
            ..Shot::EMPTY
        };
    }
    for (i, pickup) in game.pickups.iter_mut().enumerate() {
        *pickup = Pickup {
            kind: 1 + (i & 1) as u8,
            pos: Vec3 {
                x: i as i16 * 30,
                y: 0,
                z: 600,
            },
            ..Pickup::EMPTY
        };
    }
    for explosion in &mut game.explosions {
        explosion.ticks = 15;
    }
    game
}

#[test]
fn every_gameplay_pool_casts_in_detail_priority_without_exhaust_or_explosion_casters() {
    let game = populated(0);
    let bytes = baked::model_bank(0);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut frame = ShadowFrame::new();
    for x in 1..=2 {
        frame.push_caster(Caster {
            center: Vec3 { x, y: 0, z: 900 },
            half_x: 20,
            half_z: 40,
            shape: 2,
        });
    }
    game.collect_shadow_casters(&mut frame, &bank);
    assert_eq!(frame.caster_count(), 44);
    let casters = frame.casters();
    assert_eq!(casters[0].shape & 15, 1);
    assert_eq!(casters[1].shape & 15, 2);
    assert_eq!((casters[14].center.x, casters[15].center.x), (1, 2));
    assert_eq!(casters[16].shape & 15, 3);
    assert_eq!(casters[17].shape & 15, 4);
    assert!(casters[20..].iter().all(|caster| caster.shape & 15 == 3));
    assert_eq!(
        casters[20..23]
            .iter()
            .map(|caster| (caster.half_x, caster.half_z))
            .collect::<std::vec::Vec<_>>(),
        [(5, 5), (4, 4), (5, 5)],
        "only hostile projectiles use the smaller shadow footprint"
    );
    assert_eq!(
        casters[20].center.y,
        10 - scene_camera_y(game.mode, game.player.y, game.ticks)
    );
}

#[test]
fn grounded_actors_do_not_cast_but_orbital_debris_uses_its_center_and_scale() {
    for stage in [0, 16] {
        let mut game = populated(stage);
        game.boss_started = false;
        game.shots.fill(Shot::EMPTY);
        game.pickups.fill(Pickup::EMPTY);
        game.actors.fill(Actor::EMPTY);
        game.actors[0] = Actor {
            kind: assets::ROCK,
            pos: Vec3 { x: 0, y: 5, z: 400 },
            ..Actor::EMPTY
        };
        game.actors[1] = Actor {
            flags: assets::SMALL_DEBRIS,
            ..game.actors[0]
        };
        game.actors[2] = Actor {
            kind: assets::BUILDING,
            ..game.actors[0]
        };
        game.actors[3] = Actor {
            kind: assets::TURRET,
            ..game.actors[0]
        };
        let bytes = baked::model_bank(stage);
        let bank = AssetBank::new(&bytes).unwrap();
        let mut frame = ShadowFrame::new();
        game.collect_shadow_casters(&mut frame, &bank);
        assert_eq!(frame.caster_count(), if stage == 0 { 1 } else { 3 });
        if stage == 16 {
            let rock = frame.casters()[1];
            let debris = frame.casters()[2];
            assert_eq!(debris.half_x, rock.half_x / 2);
            assert_eq!(debris.half_z, rock.half_z / 2);
            assert_eq!(rock.center.y - debris.center.y, 10);
        }
    }
}

#[test]
fn silhouettes_ignore_pose_flash_and_proxy_distance() {
    let mut game = populated(0);
    let bytes = baked::model_bank(0);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut before = ShadowFrame::new();
    game.collect_shadow_casters(&mut before, &bank);
    game.flight.bank = 80 << 8;
    game.flight.pitch = -20 << 8;
    game.roll_ticks = 7;
    game.hit_flash_ticks = 30;
    game.actors[0].age = 71;
    game.actors[0].pos.z += 1500;
    let mut after = ShadowFrame::new();
    game.collect_shadow_casters(&mut after, &bank);
    assert_eq!(before.casters()[0], after.casters()[0]);
    let mut expected = before.casters()[2];
    expected.center.z += 1500;
    assert_eq!(expected, after.casters()[2]);
}

#[test]
fn transition_and_victory_caster_selection_matches_visible_gameplay() {
    let mut game = populated(0);
    let bytes = baked::model_bank(0);
    let bank = AssetBank::new(&bytes).unwrap();
    for (mode, fade, count) in [
        (Mode::Playing, 0, 42),
        (Mode::Attract, 0, 41),
        (Mode::Transition, 0, 29),
        (Mode::Victory, 0, 1),
        (Mode::Victory, crate::victory::FADE_TICKS, 0),
    ] {
        game.mode = mode;
        game.menu_ticks = fade;
        let mut frame = ShadowFrame::new();
        game.collect_shadow_casters(&mut frame, &bank);
        assert_eq!(frame.caster_count(), count, "mode={mode:?}, fade={fade}");
    }
}

#[test]
fn stage_shadow_switches_include_the_space_carrier_and_disable_orbital_crossings() {
    for stage in 0..assets::SEGMENT_COUNT {
        let mut game = populated(stage);
        let enabled = stage < 12 || stage == 16;
        assert_eq!(game.shadow_color(), if stage == 16 { 0 } else { 11 });
        for mode in [Mode::Playing, Mode::Title, Mode::Attract, Mode::Victory] {
            game.mode = mode;
            game.menu_ticks = 0;
            for player_y in [-64, 0, 64] {
                game.player.y = player_y;
                game.ticks = 123;
                assert_eq!(
                    game.shadow_ground_y(),
                    enabled.then_some(-105 - scene_camera_y(mode, player_y, game.ticks)),
                    "stage {stage}, mode {mode:?}"
                );
            }
        }
        game.mode = Mode::Transition;
        for elapsed in [0, 199, 200, 399] {
            game.transition_ticks = elapsed;
            assert_eq!(game.shadow_ground_y().is_some(), stage < 11);
        }
    }
}

#[test]
fn scenic_crossings_keep_foreground_shadows_without_background_casters() {
    use crate::render::Renderer;

    for stage in 0..assets::SEGMENT_COUNT - 1 {
        let model_bytes = baked::model_bank(stage);
        let bank = AssetBank::new(&model_bytes).unwrap();
        let segment_bytes = baked::segment_bank(stage);
        let segment = Segment::new(&segment_bytes).unwrap();
        for elapsed in [0, 99, 150, 199, 200, 250, 299, 399] {
            let game = Game::preview_transition(stage, elapsed);
            let mut foreground = ShadowFrame::new();
            game.collect_shadow_casters(&mut foreground, &bank);
            assert_eq!(
                foreground.caster_count(),
                u8::from(game.shadow_ground_y().is_some()),
                "player shadow: stage {stage}, tick {elapsed}"
            );
            let mut renderer = Renderer::new();
            renderer.begin(game.sky_color());
            let mut complete = ShadowFrame::new();
            game.render_with_shadows(&mut renderer, &mut complete, &bank, &segment);
            assert_eq!(
                complete.casters(),
                foreground.casters(),
                "background shadow: stage {stage}, tick {elapsed}"
            );
        }
    }
}

#[test]
fn disabled_levels_skip_all_gameplay_casters() {
    for stage in [12, 13, 14, 15, 17] {
        let game = populated(stage);
        let bytes = baked::model_bank(stage);
        let bank = AssetBank::new(&bytes).unwrap();
        let mut frame = ShadowFrame::new();
        game.collect_shadow_casters(&mut frame, &bank);
        assert_eq!(frame.caster_count(), 0, "stage {stage}");
    }
}

#[test]
fn disabled_shadows_match_ordinary_scene_pixels_and_reset_prior_frame_state() {
    use crate::render::{Renderer, BAND_PIXELS};
    let mut frame = ShadowFrame::new();
    for (stage, mode) in [
        (12, Mode::Playing),
        (15, Mode::Playing),
        (17, Mode::Playing),
        (11, Mode::Transition),
        (16, Mode::Transition),
    ] {
        let mut game = populated(stage);
        game.mode = mode;
        game.transition_ticks = 150;
        let model_bytes = baked::model_bank(stage);
        let bank = AssetBank::new(&model_bytes).unwrap();
        let segment_bytes = baked::segment_bank(stage);
        let segment = Segment::new(&segment_bytes).unwrap();
        let mut ordinary = Renderer::new();
        ordinary.begin(game.sky_color());
        game.render(&mut ordinary, &bank, &segment);
        ordinary.finish();
        frame.push_caster(Caster {
            center: Vec3 { x: 0, y: 0, z: 400 },
            half_x: 40,
            half_z: 40,
            shape: 4,
        });
        let mut actual = Renderer::new();
        actual.begin(game.sky_color());
        game.render_with_shadows(&mut actual, &mut frame, &bank, &segment);
        assert_eq!(frame.caster_count(), 0);
        assert_eq!(frame.shadow_commands(), 0);
        assert_eq!(frame.base_commands(), ordinary.triangle_count);
        assert_eq!(actual.checkpoint(), ordinary.checkpoint());
        for band in 0..25 {
            let mut expected_pixels = [0; BAND_PIXELS];
            let mut actual_pixels = [0; BAND_PIXELS];
            ordinary.render_band(band, &mut expected_pixels);
            actual.render_band(band, &mut actual_pixels);
            assert_eq!(actual_pixels, expected_pixels, "stage {stage}, band {band}");
        }
    }
}

#[test]
fn space_carrier_retains_visible_player_shadows_and_parked_hull_casters() {
    use crate::render::{Renderer, BAND_PIXELS};
    let mut game = populated(16);
    game.boss_started = false;
    // The lower chase camera keeps the player's ground footprint comfortably
    // on screen now that parked hulls sit outside the flight deck.
    game.player.y = -65;
    game.actors.fill(Actor::EMPTY);
    game.shots.fill(Shot::EMPTY);
    game.pickups.fill(Pickup::EMPTY);
    game.explosions.fill(Explosion {
        pos: Vec3 { x: 0, y: 0, z: 0 },
        ticks: 0,
        color: 0,
    });
    let model_bytes = baked::model_bank(16);
    let bank = AssetBank::new(&model_bytes).unwrap();
    let segment_bytes = baked::segment_bank(16);
    let segment = Segment::new(&segment_bytes).unwrap();
    let mut darkened = 0;
    // Sample both deck shades instead of depending on one row's initial ink.
    for phase in [0, 16, 32, 48] {
        game.stage_ticks = phase;
        let mut ordinary = Renderer::new();
        game.render(&mut ordinary, &bank, &segment);
        ordinary.finish();
        let mut shaded = Renderer::new();
        let mut frame = ShadowFrame::new();
        game.render_with_shadows(&mut shaded, &mut frame, &bank, &segment);
        assert!(frame.caster_count() > 1, "player and parked hulls");
        assert!(frame.shadow_commands() > 0);
        assert!(frame.rendered_casters() > 0);
        for band in 0..25 {
            let mut before = [0; BAND_PIXELS];
            let mut after = [0; BAND_PIXELS];
            ordinary.render_band(band, &mut before);
            shaded.render_band(band, &mut after);
            darkened += before
                .iter()
                .zip(after)
                .filter(|(a, b)| matches!(**a, 11 | 12) && *b == 0)
                .count();
        }
    }
    assert!(darkened > 10, "visible deck pixels darkened: {darkened}");
}
