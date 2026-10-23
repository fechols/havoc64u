// Included inside game::tests so fixtures can exercise the actual private scene
// state. These checks deliberately use the authored banks, not the synthetic
// three-vertex meshes used by the lightweight no-default-features tests.
#[cfg(feature = "host-tools")]
mod terrain_visibility {
    use super::*;
    use crate::render::{Renderer, BAND_PIXELS, TERRAIN_CLEAR_INK};
    use crate::{EXTERIOR_TERRAIN, TERRAIN_CLEAR_GEOMETRY};
    use std::vec::Vec;

    type Face = ([Vec3; 3], u8, u8);

    #[derive(Default)]
    struct Capture(Vec<Face>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }

    const POSITIONS: [(i16, i16); 5] = [(-100, -65), (100, -65), (-100, 65), (100, 65), (0, 0)];

    fn ship_faces(bank: &AssetBank<'_>) -> u16 {
        bank.face_count(assets::PLAYER) as u16 + 4
    }

    fn put_faces(renderer: &mut Renderer, faces: &[Face], remove_flags: u8) {
        for &(points, color, flags) in faces {
            renderer.triangle(
                points[0],
                points[1],
                points[2],
                color,
                flags & !remove_flags,
            );
        }
    }

    fn captures(game: &Game, bank: &AssetBank<'_>, segment: &Segment<'_>) -> (Capture, Capture) {
        let mut ship = Capture::default();
        game.render_foreground(&mut ship, bank, ship_faces(bank));
        assert_eq!(ship.0.len(), ship_faces(bank) as usize);
        assert!(ship
            .0
            .iter()
            .all(|face| face.2 & TERRAIN_CLEAR_GEOMETRY != 0));
        let mut scenery = Capture::default();
        game.render_scenery(&mut scenery, segment);
        scenery.0.retain(|face| face.2 & EXTERIOR_TERRAIN != 0);
        assert!(scenery.0.len() >= 24);
        (ship, scenery)
    }

    fn compare_ship(
        game: &Game,
        bank: &AssetBank<'_>,
        segment: &Segment<'_>,
        remove_protection: bool,
    ) -> (usize, usize) {
        let (ship, terrain) = captures(game, bank, segment);
        let result = compare_geometry(&ship.0, &terrain.0, remove_protection);
        assert!(result.0 > 0, "actual ship must reach the viewport");
        result
    }

    fn compare_geometry(
        protected: &[Face],
        terrain: &[Face],
        remove_protection: bool,
    ) -> (usize, usize) {
        let mut isolated = Renderer::new();
        put_faces(&mut isolated, protected, 0);
        isolated.finish();
        let mut scene = Renderer::new();
        put_faces(
            &mut scene,
            protected,
            if remove_protection {
                TERRAIN_CLEAR_GEOMETRY
            } else {
                0
            },
        );
        put_faces(&mut scene, terrain, 0);
        scene.finish();
        assert_eq!((isolated.dropped, scene.dropped), (0, 0));
        let mut isolated_pass = isolated.raster_pass().unwrap();
        let mut scene_pass = scene.raster_pass().unwrap();
        let mut ship_pixels = 0;
        let mut covered_pixels = 0;
        for band in 0..25 {
            let mut expected = [0; BAND_PIXELS];
            let mut actual = [0; BAND_PIXELS];
            assert_eq!(isolated_pass.next_band(&mut expected, true), Some(band));
            assert_eq!(scene_pass.next_band(&mut actual, true), Some(band));
            for (&hull, &composite) in expected.iter().zip(&actual) {
                // Coverage identifies the isolated silhouette, including the
                // black rock facets that are indistinguishable from clear ink.
                if hull & TERRAIN_CLEAR_INK != 0 {
                    ship_pixels += 1;
                    covered_pixels += usize::from((hull & 15) != (composite & 15));
                }
            }
        }
        (ship_pixels, covered_pixels)
    }

    #[test]
    fn authored_hull_and_exhaust_stay_inside_the_terrain_free_prism_for_every_roll() {
        let mut minimum_y = i16::MAX;
        let mut maximum_x = i16::MIN;
        let mut minimum_x = i16::MAX;
        for level in 0..12 {
            let models = enemies::authored::model_bank(level);
            let bank = AssetBank::new(&models).unwrap();
            assert_eq!(bank.vertex_count(assets::PLAYER), 8);
            assert_eq!(bank.face_count(assets::PLAYER), 12);
            for (x, y) in POSITIONS {
                for roll in 0..=255u8 {
                    let mut game = running();
                    set_rail(&mut game, level as u16 * SECTION_TICKS);
                    game.player.x = x;
                    game.player.y = y;
                    // The same byte consumed by the mesh rotation covers all
                    // orientations, including between the 25 native roll poses.
                    game.flight.bank = (roll as i8 as i16) << 8;
                    game.ticks = roll as u16;
                    let mut capture = Capture::default();
                    game.render_foreground(&mut capture, &bank, ship_faces(&bank));
                    assert_eq!(capture.0.len(), ship_faces(&bank) as usize);
                    for (points, color, flags) in capture.0 {
                        assert_eq!(
                            flags & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN),
                            TERRAIN_CLEAR_GEOMETRY
                        );
                        assert_ne!(color & 15, 0);
                        for point in points {
                            let world_y = point.y + game.scenery_camera_y();
                            assert!(point.x > -145 && point.x < 145);
                            assert!(
                                world_y > -103,
                                "level {level}, roll {roll}, point {point:?}"
                            );
                            assert!(point.z > 0);
                            minimum_x = minimum_x.min(point.x);
                            maximum_x = maximum_x.max(point.x);
                            minimum_y = minimum_y.min(world_y);
                        }
                    }
                }
            }
        }
        // Recorded with the composed Q8 matrix rotation; the earlier pair
        // sequence rounded the mirrored wing tips one unit the other way.
        assert_eq!((minimum_x, maximum_x, minimum_y), (-137, 136, -102));
    }

    fn check_terrain_tags(game: &Game, segment: &Segment<'_>) -> usize {
        // The world terrain sampler is independent of the scenic overlay.
        // A call without wildlife gives the exact terrain count before its
        // fourteen backdrop faces, even during a blended world-row crossing.
        let mut plain = Capture::default();
        crate::scenery::emit(
            &mut plain,
            segment,
            game.scenery_ticks(),
            game.segment_index(),
            None,
            game.scenery_camera_y(),
            false,
        );
        let count = plain.0.len() - 14;
        assert!((24..=40).contains(&count));
        let mut actual = Capture::default();
        game.render_scenery(&mut actual, segment);
        assert_eq!(&actual.0[..count], &plain.0[..count]);
        for (points, _, flags) in &actual.0[..count] {
            assert_eq!(
                flags & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN),
                EXTERIOR_TERRAIN
            );
            let below = points.iter().all(|p| p.y + game.scenery_camera_y() <= -103);
            let left = points.iter().all(|p| p.x <= -145);
            let right = points.iter().all(|p| p.x >= 145);
            assert!(
                below || left || right,
                "tagged face enters the protected prism: {points:?}"
            );
        }
        // This includes transport engine/creature-leg strip calls. They are
        // scenery, but are not part of the terrain exclusion proof.
        assert!(actual.0[count..]
            .iter()
            .all(|face| face.2 & (EXTERIOR_TERRAIN | TERRAIN_CLEAR_GEOMETRY) == 0));
        actual.0.len() - count - 14
    }

    #[test]
    fn terrain_tags_cover_only_faces_outside_the_prism_across_world_rows_and_crossings() {
        for level in 0..12 {
            let bytes = mission_segment(level);
            let segment = Segment::new(&bytes).unwrap();
            for phase in 0..1024 {
                let mut game = running();
                set_rail(&mut game, level as u16 * SECTION_TICKS + phase);
                game.player.y = if phase & 1 == 0 { -65 } else { 65 };
                assert_eq!(check_terrain_tags(&game, &segment), 0);
            }
        }
        for outgoing in 0..11 {
            let bytes = mission_segment(outgoing);
            let segment = Segment::new(&bytes).unwrap();
            let mut wildlife_faces = 0;
            for elapsed in 0..TRANSITION_TICKS {
                let mut game = running();
                game.mode = Mode::Transition;
                set_rail(&mut game, (outgoing as u16 + 1) * SECTION_TICKS);
                game.transition_ticks = elapsed;
                game.player.y = if elapsed & 1 == 0 { -65 } else { 65 };
                wildlife_faces += check_terrain_tags(&game, &segment);
            }
            assert!(
                wildlife_faces > 0,
                "crossing {outgoing} must exercise untagged wildlife"
            );
        }
    }

    #[test]
    fn foreground_protection_includes_only_ship_and_terrestrial_ground_actors() {
        for level in 0..assets::SEGMENT_COUNT {
            let models = enemies::authored::model_bank(level);
            let bank = AssetBank::new(&models).unwrap();
            let mut game = running();
            set_rail(&mut game, level as u32 * SECTION_TICKS as u32);
            game.shots[0] = Shot {
                pos: Vec3 {
                    x: 0,
                    y: -180,
                    z: 400,
                },
                kind: 2,
                damage: 12,
                ..Shot::EMPTY
            };
            game.explosions[0] = Explosion {
                pos: Vec3 {
                    x: 60,
                    y: -90,
                    z: 300,
                },
                ticks: 1,
                color: 7,
            };
            game.pickups[0] = Pickup {
                pos: Vec3 {
                    x: -60,
                    y: 0,
                    z: 450,
                },
                kind: 1,
            };
            let mut capture = Capture::default();
            game.render_foreground(&mut capture, &bank, 174);
            let count = ship_faces(&bank) as usize;
            assert!(capture.0.len() > count);
            assert!(capture.0[..count]
                .iter()
                .all(|face| face.2 & TERRAIN_CLEAR_GEOMETRY != 0));
            assert!(capture.0[count..]
                .iter()
                .all(|face| face.2 & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN) == 0));
            game.shots = [Shot::EMPTY; SHOTS];
            game.explosions.fill(Explosion::EMPTY);
            game.pickups = [Pickup::EMPTY; 4];

            // Exercise each kind as a full mesh, a distance proxy and a forced
            // proxy. Boss-stage slot three is a side unit, never a ground gun.
            for (kind, protected) in [
                (assets::BUILDING, level < 12),
                (assets::ROCK, level < 12),
                (assets::TURRET, level < 12 && !matches!(level, 3 | 7 | 11)),
                (assets::FIGHTER, false),
                (assets::DIVER, false),
            ] {
                for (z, limit) in [(500, 174), (1201, 174), (500, 2)] {
                    game.actors[0] = enemy(kind, -70, -110, z);
                    let actor = combat_capture(&game, &bank, limit);
                    assert!(!actor.0.is_empty());
                    let expected = if protected { TERRAIN_CLEAR_GEOMETRY } else { 0 };
                    assert!(
                        actor
                            .0
                            .iter()
                            .all(|face| face.2 & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN)
                                == expected),
                        "stage {level}, kind {kind}, z {z}, budget {limit}"
                    );
                    // Small orbital debris shares ROCK's ID and must remain
                    // ordinary foreground even when its proxy is selected.
                    if level >= 12 && kind == assets::ROCK {
                        game.actors[0].flags = assets::SMALL_DEBRIS;
                        let debris = combat_capture(&game, &bank, limit);
                        assert!(debris
                            .0
                            .iter()
                            .all(|face| face.2 & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN) == 0));
                    }
                }
            }

            // Boss guns use the same model ID but retain normal ordering.
            game.actors = [Actor::EMPTY; ACTORS];
            game.boss_started = true;
            game.boss_defeated = false;
            game.boss_left = 1;
            game.boss_right = 1;
            let boss = combat_capture(&game, &bank, 174);
            assert!(boss.0.len() > bank.face_count(assets::CARRIER) as usize);
            assert!(boss
                .0
                .iter()
                .all(|face| face.2 & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN) == 0));
        }
    }

    #[test]
    fn lower_right_ship_regression_at_rail_tick_34_preserves_every_hull_pixel() {
        let models = enemies::authored::model_bank(0);
        let bank = AssetBank::new(&models).unwrap();
        let bytes = mission_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        let mut game = running();
        set_rail(&mut game, 34);
        game.player.x = 100;
        game.player.y = -65;
        game.flight.bank = -12 * 256;
        let (pixels, covered) = compare_ship(&game, &bank, &segment, false);
        assert_eq!(covered, 0, "terrain covered {covered}/{pixels} ship pixels");
        assert!(
            compare_ship(&game, &bank, &segment, true).1 > 0,
            "fixture must reproduce the old average-depth overlap"
        );
    }

    fn set_pose(game: &mut Game, pose: u8) {
        if pose < 3 {
            game.flight.bank = [-12 * 256, 0, 12 * 256][pose as usize];
        } else {
            game.roll_ticks = [24, 18, 12, 6][(pose - 3) as usize];
            // Quarter-turn samples cover all four orientation quadrants. The
            // geometric proof above separately exercises every signed angle.
            game.roll_direction = 1;
        }
    }

    fn combat_capture(game: &Game, bank: &AssetBank<'_>, limit: u16) -> Capture {
        let mut capture = Capture::default();
        game.render_combat(
            &mut SceneBudget {
                sink: &mut capture,
                count: 0,
                limit,
                camera_y: game.scenery_camera_y(),
                color_override: 0,
                extra_flags: 0,
            },
            bank,
        );
        capture
    }

    fn terrain_capture(game: &Game, segment: &Segment<'_>) -> Capture {
        let mut terrain = Capture::default();
        game.render_scenery(&mut terrain, segment);
        terrain.0.retain(|face| face.2 & EXTERIOR_TERRAIN != 0);
        assert!(terrain.0.len() >= 24);
        terrain
    }

    // The Fortress's widened clear region is convex. Its floor and two side
    // planes bound every camera-to-target ray, even for the bunker corners
    // extending thirteen units beyond the ordinary central corridor.
    fn fortress_clear(point: Vec3) -> bool {
        let y = i32::from(point.y);
        let x = i32::from(point.x);
        y > -103 && 495 * (y + 105) > 240 * (x - 145) && 495 * (y + 105) > 240 * (-x - 145)
    }

    fn assert_fortress_terrain_outside(game: &Game, segment: &Segment<'_>) {
        for (points, _, _) in terrain_capture(game, segment).0 {
            let points = points.map(|p| Vec3 {
                y: p.y + game.scenery_camera_y(),
                ..p
            });
            let below = points.iter().all(|p| p.y <= -103);
            let right = points
                .iter()
                .all(|p| 495 * (i32::from(p.y) + 105) <= 240 * (i32::from(p.x) - 145));
            let left = points
                .iter()
                .all(|p| 495 * (i32::from(p.y) + 105) <= 240 * (-i32::from(p.x) - 145));
            assert!(
                below || left || right,
                "terrain intersects the bunker clear region at scene {}: {points:?}",
                game.scenery_ticks()
            );
        }
    }

    #[test]
    fn authored_ground_models_proxies_and_motion_stay_above_their_terrain_envelopes() {
        for level in [0, 1, 4, 5, 6, 8, 9, 10] {
            let bank = test_bank(level);
            let bytes = mission_segment(level);
            let segment = Segment::new(&bytes).unwrap();
            assert_eq!(bank.enemy(assets::TURRET).render_mode, 0);
            let mut spawns = 0;
            let mut largest_x = 0;
            let mut lowest_y = i16::MAX;
            for index in 0..segment.spawn_count() {
                let spawn = segment.spawn(index);
                if spawn.kind != assets::TURRET {
                    continue;
                }
                spawns += 1;
                assert_eq!(spawn.y, -90);
                assert_eq!(spawn.x.abs(), if level == 0 { 95 } else { 90 });
                let mut game = running();
                set_rail(
                    &mut game,
                    level as u16 * SECTION_TICKS + (spawn.distance + 7) / 8,
                );
                game.spawn_actor(spawn, 0, &bank);
                assert_eq!(game.actors[0].kind, assets::TURRET);
                // Real enemy motion changes only depth, including throughout
                // its visible lifetime; no animated roll invalidates the bounds.
                for _ in 0..215 {
                    enemies::step(
                        game.stage_info.enemy_profile,
                        &mut game.actors[0],
                        &game.player,
                        0,
                    );
                    game.actors[0].age += 1;
                    assert_eq!(
                        (game.actors[0].pos.x, game.actors[0].pos.y),
                        (spawn.x, spawn.y)
                    );
                }
                for (z, limit, count) in [(400, 174, bank.face_count(assets::TURRET) as usize), (1201, 174, 2), (400, 2, 2)] {
                    game.actors[0].pos.z = z;
                    let target = combat_capture(&game, &bank, limit);
                    assert_eq!(target.0.len(), count);
                    for (points, color, flags) in target.0 {
                        assert_eq!(
                            flags & (TERRAIN_CLEAR_GEOMETRY | EXTERIOR_TERRAIN),
                            TERRAIN_CLEAR_GEOMETRY
                        );
                        assert_ne!(color & 15, 0);
                        for mut point in points {
                            point.y += game.scenery_camera_y();
                            lowest_y = lowest_y.min(point.y);
                            largest_x = largest_x.max(point.x.abs());
                            if game.stage_info.enemy_profile == 0 {
                                assert!(point.x.abs() < 145 && point.y > -103);
                            } else {
                                assert!(fortress_clear(point));
                            }
                        }
                    }
                }
            }
            assert!(
                spawns >= 5,
                "stage {level} must exercise authored ground encounters"
            );
            assert_eq!(lowest_y, -98);
            assert!(
                largest_x
                    <= if assets::STAGE_INFOS[level as usize].enemy_profile == 0 {
                        145
                    } else {
                        158
                    }
            );
        }
        for y in [0, 24, 40, 56] {
            assert!(fortress_clear(Vec3 { x: 0, y, z: 0 }));
        }
    }

    #[test]
    fn actual_bunkers_spawn_after_incoming_blend_and_attract_uses_current_terrain() {
        for stage in [5, 6] {
            let bank = test_bank(stage);
            let bytes = mission_segment(stage);
            let segment = Segment::new(&bytes).unwrap();
            let first = (0..segment.spawn_count())
                .map(|i| segment.spawn(i))
                .find(|spawn| spawn.kind == assets::TURRET)
                .unwrap();
            let first_tick = (first.distance + 7) / 8;
            assert!(
                first_tick >= 64,
                "ground targets must follow the incoming terrain blend"
            );
            for mode in [Mode::Playing, Mode::Attract] {
                let mut game = running();
                game.mode = mode;
                set_rail(&mut game, stage as u16 * SECTION_TICKS);
                game.shield = 255;
                for elapsed in 1..=first_tick {
                    game.tick(&segment, &bank);
                    assert_eq!(game.mode, mode);
                    let present = game.actors.iter().any(|actor| actor.kind == assets::TURRET);
                    assert_eq!(present, elapsed == first_tick);
                }
                assert_eq!(game.scenery_ticks(), first_tick);
                assert_fortress_terrain_outside(&game, &segment);
            }
            for rail in
                (stage as u16 * SECTION_TICKS + first_tick)..((stage as u16 + 1) * SECTION_TICKS)
            {
                let mut game = running();
                set_rail(&mut game, rail);
                assert_fortress_terrain_outside(&game, &segment);
            }
            for phase in 0..1024 {
                let mut game = running();
                game.mode = Mode::Attract;
                set_rail(&mut game, stage as u16 * SECTION_TICKS + phase);
                assert_fortress_terrain_outside(&game, &segment);
            }
        }
    }

    #[test]
    fn tracked_gun_at_rail_tick_34_reproduces_and_fixes_false_floor_occlusion() {
        let bank = test_bank(0);
        let bytes = mission_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        let mut game = running();
        set_rail(&mut game, 34);
        game.actors[0] = enemy(assets::TURRET, 95, -90, 280);
        let target = combat_capture(&game, &bank, 174);
        let terrain = terrain_capture(&game, &segment);
        let (visible, covered) = compare_geometry(&target.0, &terrain.0, false);
        assert!(visible > 0);
        assert_eq!(covered, 0);
        let (_, before) = compare_geometry(&target.0, &terrain.0, true);
        assert!(
            before > 0,
            "fixture must reproduce average-depth ground overlap"
        );
    }

    #[test]
    fn first_lowland_rock_keeps_its_full_silhouette_through_the_actual_approach() {
        let bank = test_bank(0);
        let bytes = mission_segment(0);
        let segment = Segment::new(&bytes).unwrap();
        let first = (0..segment.spawn_count())
            .map(|i| segment.spawn(i))
            .find(|spawn| spawn.kind == assets::ROCK)
            .unwrap();
        assert_eq!((first.distance, first.x, first.y), (2200, -70, -110));
        let first_tick = (first.distance + 7) / 8;
        let mut game = running();
        set_rail(&mut game, first_tick);
        game.spawn_actor(first, 0, &bank);
        assert_eq!(game.actors[0].hp, 255);
        let mut visible_frames = 0;
        let mut previously_obscured = 0;
        for elapsed in 0..=234 {
            set_rail(&mut game, first_tick + elapsed);
            if game.actors[0].kind == 0 {
                break;
            }
            assert_eq!((game.actors[0].pos.x, game.actors[0].pos.y), (-70, -110));
            let target = combat_capture(&game, &bank, 174);
            let terrain = terrain_capture(&game, &segment);
            let (visible, covered) = compare_geometry(&target.0, &terrain.0, false);
            visible_frames += usize::from(visible > 0);
            assert_eq!(
                covered,
                0,
                "first rock at tick {}, z {}: {covered}/{visible}",
                first_tick + elapsed,
                game.actors[0].pos.z
            );
            previously_obscured += compare_geometry(&target.0, &terrain.0, true).1;
            game.update_actors(&bank);
        }
        assert!(visible_frames > 100);
        assert!(
            previously_obscured > 0,
            "the authored rock approach must reproduce the old terrain overdraw"
        );
        assert_eq!(
            game.actors[0].kind, 0,
            "the approach must reach actor retirement"
        );
    }

    #[test]
    fn black_obstacle_facets_count_as_protected_silhouette() {
        let bank = test_bank(4);
        let bytes = mission_segment(4);
        let segment = Segment::new(&bytes).unwrap();
        let mut game = running();
        set_rail(&mut game, 4 * SECTION_TICKS + 34);
        game.actors[0] = enemy(assets::ROCK, -70, -110, 400);
        let mut target = combat_capture(&game, &bank, 174);
        assert!(
            target.0.iter().any(|face| face.1 == 0),
            "Ashen Wastes rocks include authored black facets"
        );
        let terrain = terrain_capture(&game, &segment);
        let (visible, covered) = compare_geometry(&target.0, &terrain.0, false);
        assert!(visible > 0);
        assert_eq!(covered, 0);

        // An all-black copy has the exact same silhouette. A nonzero-color
        // visibility oracle would silently count none of these pixels.
        for face in &mut target.0 {
            face.1 = 0;
        }
        assert_eq!(compare_geometry(&target.0, &terrain.0, false), (visible, 0));
        // A deliberately late, screen-filling exterior face must reject black
        // protected pixels just as it rejects the authored colored facets.
        let exterior = [(
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
            EXTERIOR_TERRAIN,
        )];
        assert_eq!(compare_geometry(&target.0, &exterior, false), (visible, 0));
        assert_eq!(
            compare_geometry(&target.0, &exterior, true),
            (visible, visible)
        );
    }

    #[test]
    fn terrain_preserves_ground_targets_at_all_scroll_phases_depths_and_chase_heights() {
        let mut cases_with_visible_pixels = 0;
        for level in [0, 1, 2, 4, 5, 6, 8, 9, 10] {
            let bank = test_bank(level);
            let bytes = mission_segment(level);
            let segment = Segment::new(&bytes).unwrap();
            // Derive each stage's actual ground actors from its encounter page:
            // this includes Canyon batteries, crystal spires and buildings.
            let mut ground = Vec::new();
            for index in 0..segment.spawn_count() {
                let spawn = segment.spawn(index);
                if matches!(spawn.kind, assets::TURRET | assets::ROCK | assets::BUILDING)
                    && !ground.iter().any(|s: &Spawn| s.kind == spawn.kind)
                {
                    ground.push(spawn);
                }
            }
            assert_eq!(ground.len(), 2);
            for spawn in ground {
                // Include near clipping, full meshes at their far cutoff, ordinary
                // far proxies, and the same proxy forced by a tight face budget.
                for (z, limit) in [
                    (8, 174),
                    (24, 174),
                    (80, 174),
                    (120, 174),
                    (180, 174),
                    (280, 174),
                    (400, 174),
                    (800, 174),
                    (1200, 174),
                    (1201, 174),
                    (1800, 174),
                    (400, 2),
                ] {
                    for side in [-1, 1] {
                        for player_y in [-65, 0, 65] {
                            for phase in 0..64 {
                                for mode in [Mode::Playing, Mode::Attract] {
                                    let mut game = running();
                                    game.mode = mode;
                                    set_rail(&mut game, level as u16 * SECTION_TICKS + 320 + phase);
                                    game.player.y = player_y;
                                    let x = side * spawn.x.abs();
                                    game.spawn_actor(Spawn { x, ..spawn }, 0, &bank);
                                    game.actors[0].pos.z = z;
                                    let target = combat_capture(&game, &bank, limit);
                                    let terrain = terrain_capture(&game, &segment);
                                    assert!(target
                                        .0
                                        .iter()
                                        .all(|face| face.2 & TERRAIN_CLEAR_GEOMETRY != 0));
                                    let (visible, covered) =
                                        compare_geometry(&target.0, &terrain.0, false);
                                    cases_with_visible_pixels += usize::from(visible > 0);
                                    assert_eq!(covered, 0, "section {level}, kind {}, z {z}, budget {limit}, x {x}, player_y {player_y}, phase {phase}, mode {mode:?}: {covered}/{visible}", spawn.kind);
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(cases_with_visible_pixels > 20_000);
    }

    fn assert_visible(game: &Game, bank: &AssetBank<'_>, segment: &Segment<'_>, pose: u8) {
        let (pixels, covered) = compare_ship(game, bank, segment, false);
        assert_eq!(covered, 0, "rail {}, bridge {}, position {:?}, pose {pose}: terrain covered {covered}/{pixels} hull pixels", game.stage_ticks, game.transition_ticks, game.player);
    }

    #[test]
    fn terrain_never_covers_the_ship_at_corners_scroll_phases_and_banked_or_rolled_poses() {
        for level in 0..12 {
            let models = enemies::authored::model_bank(level);
            let bank = AssetBank::new(&models).unwrap();
            let bytes = mission_segment(level);
            let segment = Segment::new(&bytes).unwrap();
            for (x, y) in POSITIONS {
                for phase in 0..64 {
                    for pose in 0..7 {
                        let mut game = running();
                        set_rail(&mut game, level as u16 * SECTION_TICKS + phase);
                        game.ticks = phase;
                        game.player.x = x;
                        game.player.y = y;
                        set_pose(&mut game, pose);
                        assert_visible(&game, &bank, &segment, pose);
                    }
                }
            }
        }
        for outgoing in 0..11 {
            let models = enemies::authored::model_bank(outgoing);
            let bank = AssetBank::new(&models).unwrap();
            let bytes = mission_segment(outgoing);
            let segment = Segment::new(&bytes).unwrap();
            for elapsed in [0, 63, 64, 127, 199, 255, 319, 399] {
                for (x, y) in POSITIONS {
                    for pose in 0..7 {
                        let mut game = running();
                        game.mode = Mode::Transition;
                        set_rail(&mut game, (outgoing as u16 + 1) * SECTION_TICKS);
                        game.transition_ticks = elapsed;
                        game.ticks = elapsed;
                        game.player.x = x;
                        game.player.y = y;
                        set_pose(&mut game, pose);
                        assert_visible(&game, &bank, &segment, pose);
                    }
                }
            }
        }
    }
}
