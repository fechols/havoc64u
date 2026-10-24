// Included by terrain_visibility_tests.rs to reuse its authored-geometry capture.
mod space_carrier {
    use super::*;
    use crate::shadow::ShadowFrame;

    fn encounter(phase: u16, player_y: i16, boss_z: i16, exposed: bool) -> Game {
        let mut game = running();
        set_rail(
            &mut game,
            16 * u32::from(SECTION_TICKS) + 2560 + u32::from(phase),
        );
        game.player.y = player_y;
        game.boss_started = true;
        game.boss_z = boss_z;
        if exposed {
            game.boss_left = 0;
            game.boss_right = 0;
        }
        game
    }

    fn deck(game: &Game, segment: &Segment<'_>) -> Capture {
        let mut scene = Capture::default();
        game.render_scenery(&mut scene, segment);
        scene.0.retain(|face| face.2 & EXTERIOR_TERRAIN != 0);
        assert_eq!(
            scene.0.len(),
            24,
            "four deck rows and their paired guide strips"
        );
        scene
    }

    fn pixels(mut renderer: Renderer) -> Vec<u8> {
        assert_eq!(renderer.dropped, 0);
        let mut result = Vec::with_capacity(160 * 200);
        let mut pass = renderer.raster_pass().unwrap();
        for band in 0..25 {
            let mut row = [0; BAND_PIXELS];
            assert_eq!(pass.next_band(&mut row, true), Some(band));
            result.extend_from_slice(&row);
        }
        result
    }

    fn scene_pixels(
        game: &Game,
        bank: &AssetBank<'_>,
        boss: &[Face],
        scenery: &[Face],
        shadows: bool,
        remove_protection: bool,
    ) -> Vec<u8> {
        let mut renderer = Renderer::new();
        put_faces(
            &mut renderer,
            boss,
            if remove_protection {
                TERRAIN_CLEAR_GEOMETRY
            } else {
                0
            },
        );
        put_faces(&mut renderer, scenery, 0);
        if shadows {
            let mut frame = ShadowFrame::new();
            game.collect_shadow_casters(&mut frame, bank);
            frame.render(
                &mut renderer,
                game.shadow_ground_y().unwrap(),
                game.shadow_color(),
            );
            assert!(
                frame.shadow_commands() > 0,
                "the real boss must cast a visible shadow"
            );
        } else {
            renderer.finish();
        }
        pixels(renderer)
    }

    fn covered_pixels(isolated: &[u8], scene: &[u8]) -> usize {
        isolated
            .iter()
            .zip(scene)
            .filter(|(before, after)| {
                **before & TERRAIN_CLEAR_INK != 0 && (**before & 15) != (**after & 15)
            })
            .count()
    }

    #[test]
    fn boss_survives_deck_and_shadows_at_every_scroll_phase_height_and_defense_state() {
        let models = enemies::authored::model_bank(16);
        let bank = AssetBank::new(&models).unwrap();
        let bytes = mission_segment(16);
        let segment = Segment::new(&bytes).unwrap();
        let mut shadowed_ground = 0;
        for player_y in [-65, 0, 65] {
            for boss_z in [1800, 900, 680] {
                for exposed in [false, true] {
                    for phase in 0..64 {
                        let game = encounter(phase, player_y, boss_z, exposed);
                        let boss = combat_capture(&game, &bank, 174);
                        assert!(boss.0.iter().all(|face| face.2 == TERRAIN_CLEAR_GEOMETRY));
                        let deck = deck(&game, &segment);
                        let isolated = scene_pixels(&game, &bank, &boss.0, &[], false, false);
                        assert!(isolated.iter().any(|pixel| pixel & TERRAIN_CLEAR_INK != 0));
                        let on_deck = scene_pixels(&game, &bank, &boss.0, &deck.0, false, false);
                        let shadowed = scene_pixels(&game, &bank, &boss.0, &[], true, false);
                        let combined = scene_pixels(&game, &bank, &boss.0, &deck.0, true, false);
                        for (name, scene) in [
                            ("deck", &on_deck),
                            ("shadows", &shadowed),
                            ("both", &combined),
                        ] {
                            assert_eq!(covered_pixels(&isolated, scene), 0,
                                "{name}: phase {phase}, player y {player_y}, boss z {boss_z}, exposed {exposed}");
                        }
                        shadowed_ground += on_deck
                            .iter()
                            .zip(&combined)
                            .filter(|(before, after)| {
                                **before & TERRAIN_CLEAR_INK == 0
                                    && (**before & 15) != (**after & 15)
                                    && **after & 15 == game.shadow_color()
                            })
                            .count();
                    }
                }
            }
        }
        assert!(
            shadowed_ground > 1000,
            "protecting the boss must retain shadows on the deck"
        );
    }

    #[test]
    fn flying_enemy_and_boss_shadows_are_black_on_both_gray_deck_shades() {
        let models = enemies::authored::model_bank(16);
        let bank = AssetBank::new(&models).unwrap();
        let bytes = mission_segment(16);
        let segment = Segment::new(&bytes).unwrap();
        for kind in [assets::FIGHTER, assets::DIVER, assets::CARRIER] {
            let mut darkened = [0usize; 2];
            for phase in 0..64 {
                let mut game = encounter(phase, -65, 680, false);
                game.boss_started = kind == assets::CARRIER;
                if kind != assets::CARRIER {
                    game.actors[0] = enemy(kind, 0, 0, 680);
                }
                let mut collected = ShadowFrame::new();
                game.collect_shadow_casters(&mut collected, &bank);
                assert_eq!(collected.caster_count(), 2, "player and selected craft");

                // Isolate the authored enemy/boss footprint so the player's
                // shadow cannot satisfy this contrast regression by itself.
                let mut shadow = ShadowFrame::new();
                shadow.push_caster(collected.casters()[1]);
                let deck = deck(&game, &segment);
                let before = scene_pixels(&game, &bank, &[], &deck.0, false, false);
                let mut renderer = Renderer::new();
                put_faces(&mut renderer, &deck.0, 0);
                shadow.render(
                    &mut renderer,
                    game.shadow_ground_y().unwrap(),
                    game.shadow_color(),
                );
                assert_eq!(shadow.rendered_casters(), 1, "kind {kind}, phase {phase}");
                let after = pixels(renderer);
                for (&before, &after) in before.iter().zip(&after) {
                    if matches!(before & 15, 11 | 12) && before != after {
                        assert_eq!(after & 15, 0, "kind {kind}, phase {phase}");
                        darkened[usize::from((before & 15) - 11)] += 1;
                    }
                }
            }
            assert!(
                darkened.iter().all(|&count| count > 10),
                "kind {kind} needs visible black shadows on both deck grays: {darkened:?}"
            );
        }
    }

    #[test]
    fn deck_and_shadow_regressions_reproduce_independently_without_boss_protection() {
        let models = enemies::authored::model_bank(16);
        let bank = AssetBank::new(&models).unwrap();
        let bytes = mission_segment(16);
        let segment = Segment::new(&bytes).unwrap();
        let mut deck_loss = 0;
        let mut shadow_loss = 0;
        // A complete scroll period avoids tying the regression to stage-origin
        // offsets, while keeping the two sources of overdraw independent.
        for phase in 0..64 {
            let game = encounter(phase, 0, 680, false);
            let boss = combat_capture(&game, &bank, 174);
            let deck = deck(&game, &segment);
            let isolated = scene_pixels(&game, &bank, &boss.0, &[], false, false);
            deck_loss += covered_pixels(
                &isolated,
                &scene_pixels(&game, &bank, &boss.0, &deck.0, false, true),
            );
            shadow_loss += covered_pixels(
                &isolated,
                &scene_pixels(&game, &bank, &boss.0, &[], true, true),
            );
        }
        assert!(
            deck_loss > 0,
            "the old average-depth deck overwrite must be exercised"
        );
        assert!(
            shadow_loss > 0,
            "the old final shadow pass must overwrite the boss"
        );
    }

    #[test]
    fn protection_ends_with_boss_group_and_preserves_ordinary_foreground_occlusion() {
        let models = enemies::authored::model_bank(16);
        let bank = AssetBank::new(&models).unwrap();
        let mut game = encounter(0, 0, 680, true);
        let boss = combat_capture(&game, &bank, 174);
        game.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 300 },
            kind: 1,
            damage: 2,
            ..Shot::EMPTY
        };
        game.explosions[0] = Explosion {
            pos: Vec3 {
                x: 50,
                y: 0,
                z: 400,
            },
            ticks: 5,
            color: 7,
        };
        game.actors[0] = enemy(assets::FIGHTER, -50, 0, 500);
        let combat = combat_capture(&game, &bank, 174);
        assert_eq!(&combat.0[..boss.0.len()], &boss.0);
        assert!(combat.0.len() > boss.0.len());
        assert!(
            combat.0[boss.0.len()..]
                .iter()
                .all(|face| { face.2 & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN) == 0 }),
            "boss protection must not leak into effects or orbital actors"
        );

        // An ordinary closer object still paints over the protected boss.
        // Use an independent screen-filling face to make ownership unambiguous.
        let occluder = [(
            [
                Vec3 {
                    x: -200,
                    y: -100,
                    z: 100,
                },
                Vec3 {
                    x: 0,
                    y: 200,
                    z: 100,
                },
                Vec3 {
                    x: 200,
                    y: -100,
                    z: 100,
                },
            ],
            5,
            0,
        )];
        let isolated = scene_pixels(&game, &bank, &boss.0, &[], false, false);
        let occluded = scene_pixels(&game, &bank, &boss.0, &occluder, false, false);
        assert!(covered_pixels(&isolated, &occluded) > 100);
    }

    #[test]
    fn deck_stays_below_boss_and_parked_hulls_stay_outside_its_width() {
        let models = enemies::authored::model_bank(16);
        let bank = AssetBank::new(&models).unwrap();
        let bytes = mission_segment(16);
        let segment = Segment::new(&bytes).unwrap();
        let mut sides = [false; 2];
        for phase in 0..128 {
            let game = encounter(phase, 0, 680, false);
            let boss = combat_capture(&game, &bank, 174);
            let boss_width = boss
                .0
                .iter()
                .flat_map(|face| face.0)
                .map(|p| p.x.abs())
                .max()
                .unwrap();
            assert!(boss
                .0
                .iter()
                .flat_map(|face| face.0)
                .all(|p| p.y + game.scenery_camera_y() > -103));
            let mut scenery = Capture::default();
            game.render_scenery(&mut scenery, &segment);
            let mut deck_faces = 0;
            let mut hull_faces = 0;
            for (points, color, flags) in scenery.0 {
                assert_eq!(flags & TERRAIN_CLEAR_GEOMETRY, 0);
                if flags & EXTERIOR_TERRAIN != 0 {
                    deck_faces += 1;
                    assert!(matches!(color, 7 | 11 | 12));
                    assert!(points
                        .iter()
                        .all(|p| matches!(p.y + game.scenery_camera_y(), -105 | -103)));
                } else if flags & STATIC_SKY == 0 {
                    hull_faces += 1;
                    assert!(
                        points.iter().all(|p| p.x.abs() > boss_width),
                        "parked hull enters boss footprint: {points:?}"
                    );
                    assert!(points.iter().all(|p| p.x.signum() == points[0].x.signum()));
                    sides[usize::from(points[0].x > 0)] = true;
                }
            }
            assert_eq!(deck_faces, 24);
            assert!(hull_faces >= 24);
        }
        assert_eq!(sides, [true, true]);
    }
}
