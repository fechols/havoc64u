use super::tests::{empty_segment, test_bank};
use super::*;
use std::vec;
use std::vec::Vec;

fn running(stage: u8) -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, stage, 0);
    game
}

fn scripted(stage: u8, spawns: &[Spawn]) -> [u8; assets::SEGMENT_SIZE] {
    let mut bytes = empty_segment(stage);
    bytes[7] = spawns.len() as u8;
    for (index, spawn) in spawns.iter().enumerate() {
        let at = 16 + index * 8;
        bytes[at..at + 2].copy_from_slice(&spawn.distance.to_le_bytes());
        bytes[at + 2] = spawn.kind;
        bytes[at + 3] = spawn.flags;
        bytes[at + 4..at + 6].copy_from_slice(&spawn.x.to_le_bytes());
        bytes[at + 6..at + 8].copy_from_slice(&spawn.y.to_le_bytes());
    }
    bytes
}

fn spawn(kind: u8, distance: u16, x: i16) -> Spawn {
    Spawn {
        kind,
        distance,
        x,
        y: 20,
        flags: 0,
    }
}

#[test]
fn delayed_waves_repeat_each_enemy_once_with_its_properties_and_rewards() {
    let mut records: Vec<_> = [assets::FIGHTER, assets::DIVER, assets::TURRET]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| Spawn {
            flags: 1,
            ..spawn(kind, 800, index as i16 * 60 - 60)
        })
        .collect();
    records.push(spawn(assets::ROCK, 800, 90));
    records.push(spawn(assets::CRYSTAL, 800, 0));
    let bytes = scripted(0, &records);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    let mut game = running(0);
    game.stage_ticks = 100;
    game.spawn_encounters(&segment, &bank);
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 4);
    let originals = [game.actors[0], game.actors[1], game.actors[2]];
    assert_eq!(game.pickups.iter().filter(|p| p.kind != 0).count(), 1);

    game.stage_ticks = 268; // 2144, just before the repeats at 2150.
    game.spawn_encounters(&segment, &bank);
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 4);
    game.stage_ticks = 269;
    game.spawn_encounters(&segment, &bank);
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 7);
    for (index, original) in originals.into_iter().enumerate() {
        let repeated = game.actors[index + 4];
        assert_eq!(
            (
                repeated.kind,
                repeated.hp,
                repeated.flags,
                repeated.approach_speed
            ),
            (
                original.kind,
                original.hp,
                original.flags,
                original.approach_speed
            )
        );
        assert_eq!(repeated.age, 0);
        assert!((1734..=1861).contains(&repeated.pos.z));
        assert!((records[index].x - 16..=records[index].x + 15).contains(&repeated.origin_x));
        assert_eq!(
            (repeated.pos.x, repeated.pos.y),
            (repeated.origin_x, repeated.origin_y)
        );
    }
    game.stage_ticks = 1000;
    game.spawn_encounters(&segment, &bank);
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 7);
    assert_eq!(game.pickups.iter().filter(|p| p.kind != 0).count(), 1);

    game.bombs = 0;
    for slot in [0, 1, 2, 4, 5, 6] {
        game.kill_actor(slot, &bank);
    }
    assert_eq!(game.kills, 6);
    assert_eq!((game.bombs, game.bomb_kills), (0, 6));
    let expected_score: u32 = [assets::FIGHTER, assets::DIVER, assets::TURRET]
        .into_iter()
        .map(|kind| u32::from(bank.enemy(kind).score) * 2)
        .sum();
    assert_eq!(game.score, expected_score);
    assert!(game.pickups.iter().any(|p| p.kind == 2));
}

#[test]
fn encounter_catchup_merges_both_streams_and_originals_win_ties() {
    for (records, distance, expected) in [
        (
            vec![
                spawn(assets::FIGHTER, 800, 10),
                spawn(assets::DIVER, 2000, 20),
                spawn(assets::TURRET, 2250, 30),
            ],
            2304,
            vec![
                assets::FIGHTER,
                assets::DIVER,
                assets::FIGHTER,
                assets::TURRET,
            ],
        ),
        (
            vec![
                spawn(assets::FIGHTER, 800, 10),
                spawn(assets::DIVER, 2150, 20),
            ],
            2200,
            vec![assets::FIGHTER, assets::DIVER, assets::FIGHTER],
        ),
    ] {
        let bytes = scripted(0, &records);
        let segment = Segment::new(&bytes).unwrap();
        let mut game = running(0);
        game.stage_ticks = distance / 8;
        game.spawn_encounters(&segment, &test_bank(0));
        let kinds: Vec<_> = game
            .actors
            .iter()
            .filter(|a| a.kind != 0)
            .map(|a| a.kind)
            .collect();
        assert_eq!(kinds, expected);
    }
}

#[test]
fn late_handoff_can_expire_originals_without_expiring_their_repeats() {
    let bytes = scripted(0, &[spawn(assets::FIGHTER, 800, 10)]);
    let segment = Segment::new(&bytes).unwrap();
    let mut on_time = running(0);
    on_time.encounter_seed = 37;
    on_time.stage_ticks = 269;
    on_time.spawn_encounters(&segment, &test_bank(0));
    let repeated = on_time.actors[1];
    let mut game = running(0);
    game.encounter_seed = 37;
    game.stage_ticks = 300; // Original is 1600 units late; repeat is only 250 late.
    game.spawn_encounters(&segment, &test_bank(0));
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 1);
    assert_eq!(
        game.actors[0],
        Actor {
            pos: Vec3 {
                z: repeated.pos.z - 248,
                ..repeated.pos
            },
            age: 31,
            ..repeated
        }
    );
    assert_eq!((game.spawn_cursor, game.repeat_spawn_cursor), (1, 1));
    game.spawn_encounters(&segment, &test_bank(0));
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 1);
}

#[test]
fn defeated_bosses_suppress_original_and_repeated_escorts() {
    let bytes = scripted(11, &[spawn(assets::FIGHTER, 800, 10)]);
    let segment = Segment::new(&bytes).unwrap();
    let mut game = running(11);
    game.stage_ticks = 269;
    game.boss_defeated = true;
    game.spawn_encounters(&segment, &test_bank(11));
    assert!(game.actors.iter().all(|a| a.kind == 0));
    assert_eq!((game.spawn_cursor, game.repeat_spawn_cursor), (1, 1));
}

#[test]
fn spawn_cursors_reset_for_scenes_lives_and_stage_transitions() {
    for reset in [Game::reset_run, Game::respawn, Game::reset_in_place] {
        let mut game = running(0);
        game.spawn_cursor = 19;
        game.repeat_spawn_cursor = 12;
        reset(&mut game);
        assert_eq!((game.spawn_cursor, game.repeat_spawn_cursor), (0, 0));
    }
    let mut game = running(0);
    game.spawn_cursor = 19;
    game.repeat_spawn_cursor = 12;
    game.reset_scene(Mode::Attract, 12, 500);
    assert_eq!((game.spawn_cursor, game.repeat_spawn_cursor), (0, 0));

    let mut game = running(0);
    game.mode = Mode::Transition;
    game.transition_ticks = TRANSITION_TICKS - 1;
    game.spawn_cursor = 19;
    game.repeat_spawn_cursor = 12;
    game.tick(&Segment::new(&empty_segment(0)).unwrap(), &test_bank(0));
    assert_eq!(game.stage_id, 1);
    assert_eq!((game.spawn_cursor, game.repeat_spawn_cursor), (0, 0));
}

#[test]
fn run_layouts_change_between_runs_but_repeat_after_a_lost_life() {
    let bytes = scripted(
        0,
        &[
            spawn(assets::FIGHTER, 800, -60),
            spawn(assets::DIVER, 800, 60),
        ],
    );
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    let mut game = running(0);
    game.reset_run();
    let seed = game.encounter_seed;
    game.stage_ticks = 100;
    game.spawn_encounters(&segment, &bank);
    game.stage_ticks = 269;
    game.spawn_encounters(&segment, &bank);
    let layout = game.actors;
    assert_ne!(
        layout[0].pos,
        Vec3 {
            z: layout[2].pos.z + 2,
            ..layout[2].pos
        },
        "the extra wave has its own depth variation"
    );
    assert_ne!(layout[0].origin_x, layout[2].origin_x, "repeat x must vary");
    assert_ne!(layout[0].origin_y, layout[2].origin_y, "repeat y must vary");
    game.respawn();
    assert_eq!(game.encounter_seed, seed);
    game.stage_ticks = 100;
    game.spawn_encounters(&segment, &bank);
    game.stage_ticks = 269;
    game.spawn_encounters(&segment, &bank);
    assert_eq!(game.actors, layout);
    game.reset_run();
    assert_ne!(game.encounter_seed, seed);
    game.stage_ticks = 100;
    game.spawn_encounters(&segment, &bank);
    game.stage_ticks = 269;
    game.spawn_encounters(&segment, &bank);
    assert_ne!(game.actors, layout);

    let seed = game.encounter_seed;
    game.mode = Mode::Transition;
    game.transition_ticks = TRANSITION_TICKS - 1;
    game.tick(&segment, &bank);
    assert_eq!((game.stage_id, game.encounter_seed), (1, seed));
}

#[test]
fn every_layout_seed_keeps_enemies_in_bounds_and_ground_units_mounted() {
    let records = [
        Spawn {
            y: -55,
            ..spawn(assets::FIGHTER, 800, -180)
        },
        Spawn {
            y: 90,
            ..spawn(assets::DIVER, 800, 180)
        },
        Spawn {
            y: -90,
            ..spawn(assets::TURRET, 800, 0)
        },
        spawn(assets::ROCK, 800, 70),
        spawn(assets::CRYSTAL, 800, -70),
    ];
    let bytes = scripted(0, &records);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    let mut layouts = Vec::new();
    for seed in 0..=u8::MAX {
        let mut game = running(0);
        game.encounter_seed = seed;
        game.stage_ticks = 100;
        game.spawn_encounters(&segment, &bank);
        assert_eq!(
            game.actors[3].pos,
            Vec3 {
                x: 70,
                y: 20,
                z: 1800
            }
        );
        assert_eq!(
            game.pickups[0].pos,
            Vec3 {
                x: -70,
                y: 20,
                z: 1800
            }
        );
        for actor in &game.actors[..3] {
            let limit = if actor.kind == assets::TURRET { 115 } else { 180 };
            assert!((-limit..=limit).contains(&actor.origin_x));
            assert!((1736..=1863).contains(&actor.pos.z));
            if actor.kind == assets::TURRET {
                assert_eq!(actor.origin_y, -90);
            } else {
                assert!((-55..=90).contains(&actor.origin_y));
            }
        }
        layouts.push([game.actors[0].pos, game.actors[1].pos, game.actors[2].pos]);
    }
    for axis in 0..3 {
        let coordinate = |p: Vec3| [p.x, p.y, p.z][axis];
        assert!(
            layouts
                .iter()
                .any(|layout| coordinate(layout[0]) != coordinate(layouts[0][0])),
            "axis {axis} never varied"
        );
    }
}

#[test]
fn extreme_valid_coordinates_clamp_without_wrapping_or_overflowing() {
    let records = [
        Spawn {
            y: i16::MIN,
            ..spawn(assets::FIGHTER, 800, i16::MIN)
        },
        Spawn {
            y: i16::MAX,
            ..spawn(assets::DIVER, 800, i16::MAX)
        },
        Spawn {
            y: i16::MAX,
            ..spawn(assets::FIGHTER, 800, i16::MIN)
        },
        Spawn {
            y: i16::MIN,
            ..spawn(assets::DIVER, 800, i16::MAX)
        },
    ];
    let bytes = scripted(0, &records);
    let segment = Segment::new(&bytes).unwrap();
    for seed in 0..=u8::MAX {
        let mut game = running(0);
        game.encounter_seed = seed;
        game.stage_ticks = 100;
        game.spawn_encounters(&segment, &test_bank(0));
        let positions: Vec<_> = game.actors[..4]
            .iter()
            .map(|a| (a.origin_x, a.origin_y))
            .collect();
        assert_eq!(
            positions,
            [(-180, -55), (180, 90), (-180, 90), (180, -55)],
            "seed {seed}"
        );
    }
}

#[test]
fn partially_full_shot_pools_keep_the_center_then_left_then_right_fan() {
    let holes = [1, 8, SHOTS - 1];
    let position = Vec3 { x: 0, y: 0, z: 900 };
    for available in 0..=3 {
        let mut game = running(0);
        game.missile_targets.fill(10);
        for (index, shot) in game.shots.iter_mut().enumerate() {
            *shot = Shot {
                pos: Vec3 {
                    x: index as i16,
                    y: 10,
                    z: 500,
                },
                dx: 1,
                dy: -1,
                damage: 4,
                kind: 1 + (index % 3) as u8,
            };
        }
        for &index in &holes[..available] {
            game.shots[index] = Shot::EMPTY;
        }
        let before = game.shots;
        game.enemy_attack(position, 2);
        for (index, &old) in before.iter().enumerate() {
            if let Some(side) = holes[..available].iter().position(|&hole| hole == index) {
                assert_eq!(
                    game.shots[index],
                    Shot {
                        pos: position,
                        dx: [0, -2, 2][side],
                        dy: 0,
                        damage: 6,
                        kind: 2,
                    }
                );
                assert_eq!(game.missile_lock(index), 0);
            } else {
                assert_eq!(game.shots[index], old, "overwrote occupied slot {index}");
                assert_eq!(game.missile_lock(index), 10);
            }
        }
        assert_eq!(game.pop_audio().unwrap().effect, Effect::EnemyFire);
        assert!(game.pop_audio().is_none());
    }
}

fn assert_six_point_projectile_hit(game: &mut Game, bank: &AssetBank<'_>) {
    let shot = *game
        .shots
        .iter()
        .find(|s| s.kind == 2)
        .expect("hostile attack");
    assert!(game
        .shots
        .iter()
        .filter(|s| s.kind == 2)
        .all(|s| s.damage == 6));
    game.shots = [Shot::EMPTY; SHOTS];
    game.shots[0] = Shot {
        pos: Vec3 {
            z: PLAYER_Z + 18,
            ..game.player
        },
        dx: 0,
        dy: 0,
        ..shot
    };
    game.update_shots(bank);
    assert_eq!((game.shield, game.shots[0].kind), (94, 0));
    assert_eq!(game.presentation_event, PresentationEvent::Hit);
    assert_eq!(game.invulnerability, 35);
}

#[test]
fn ordinary_and_boss_projectiles_all_deal_six_damage() {
    for pattern in [1, 2] {
        let mut game = running(0);
        game.enemy_attack(Vec3 { x: 0, y: 0, z: 900 }, pattern);
        assert_eq!(
            game.shots.iter().filter(|s| s.kind == 2).count(),
            if pattern == 1 { 1 } else { 3 }
        );
        assert_six_point_projectile_hit(&mut game, &test_bank(0));
    }
    for stage in [3, 7, 11, 16, 17] {
        for exposed in [false, true] {
            let mut game = running(stage);
            game.stage_ticks = game.boss_start_ticks();
            game.bosses[0].pos.z = 680;
            if exposed {
                game.bosses[0].left = 0;
                game.bosses[0].right = 0;
            }
            game.update_boss();
            assert_six_point_projectile_hit(&mut game, &test_bank(stage));
        }
    }
}

#[test]
fn enemies_and_every_scenery_collision_deal_fifteen_damage() {
    for (kind, flags) in [
        (assets::FIGHTER, 0),
        (assets::DIVER, 0),
        (assets::TURRET, 0),
        (assets::BUILDING, 0),
        (assets::ROCK, 0),
        (assets::ROCK, assets::SMALL_DEBRIS),
    ] {
        let mut game = running(0);
        let bank = test_bank(0);
        game.spawn_actor(
            Spawn {
                flags,
                ..spawn(kind, 0, 0)
            },
            0,
            &bank,
        );
        let center = actor_center_sim(&game.actors[0], &bank);
        game.actors[0].pos.y -= center.y;
        game.actors[0].origin_y = game.actors[0].pos.y;
        game.actors[0].pos.z = PLAYER_Z + 8;
        game.update_actors(&bank);
        assert_eq!(
            (game.shield, game.actors[0].kind),
            (85, 0),
            "kind {kind}, flags {flags}"
        );
        assert_eq!(game.invulnerability, 35);
    }
}

#[test]
#[cfg(feature = "host-tools")]
fn all_campaign_enemies_repeat_without_exhausting_actor_slots() {
    let mut totals = [[0usize; 2]; 256];
    let mut peak = 0;
    for stage in 0..assets::SEGMENT_COUNT {
        let bytes = enemies::authored::segment_bank(stage);
        let models = enemies::authored::model_bank(stage);
        let segment = Segment::new(&bytes).unwrap();
        let bank = AssetBank::new(&models).unwrap();
        let mut expected = vec![[0usize; 2]; SECTION_TICKS as usize];
        for index in 0..segment.spawn_count() {
            let record = segment.spawn(index);
            let enemy = matches!(
                record.kind,
                assets::FIGHTER | assets::DIVER | assets::TURRET
            );
            let class = usize::from(!enemy);
            expected[((record.distance as usize + 7) / 8).max(1)][class] += 1;
            if enemy {
                let repeat_tick = (record.distance as usize + 1350 + 7) / 8;
                assert!(
                    repeat_tick < SECTION_TICKS as usize,
                    "stage {stage} late repeat"
                );
                expected[repeat_tick][0] += 1;
            }
        }
        for seed in 0..=u8::MAX {
            let mut game = running(stage);
            game.encounter_seed = seed;
            game.mode = Mode::Attract; // Suppress ramming, leaving every actor alive until retirement.
            for tick in 1..SECTION_TICKS {
                game.stage_ticks = tick;
                let before = game.actors;
                game.spawn_encounters(&segment, &bank);
                let mut admitted = [0usize; 2];
                for (old, new) in before.iter().zip(&game.actors) {
                    if old.kind == 0 && new.kind != 0 {
                        admitted[usize::from(!matches!(
                            new.kind,
                            assets::FIGHTER | assets::DIVER | assets::TURRET
                        ))] += 1;
                    }
                }
                assert_eq!(
                    admitted, expected[tick as usize],
                    "seed {seed}, stage {stage}, tick {tick}: lost spawn"
                );
                totals[seed as usize][0] += admitted[0];
                totals[seed as usize][1] += admitted[1];
                peak = peak.max(game.actors.iter().filter(|a| a.kind != 0).count());
                game.update_actors(&bank);
            }
            assert_eq!(game.spawn_cursor, segment.spawn_count());
            assert_eq!(game.repeat_spawn_cursor, segment.spawn_count());
        }
    }
    assert!(totals.iter().all(|&count| count == [890, 102]));
    assert!(peak <= ACTORS);
}
