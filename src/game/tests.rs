use super::*;

#[test]
fn orbital_debris_is_bomb_destructible_but_cannot_farm_enemy_rewards() {
    for stage in 12..18 {
        let mut game = Game::preview_at_for_test(stage);
        let bank = test_bank(stage);
        game.spawn_actor(
            Spawn {
                distance: 0,
                kind: assets::ROCK,
                x: 0,
                y: -10,
                flags: 2,
            },
            0,
            &bank,
        );
        game.spawn_actor(
            Spawn {
                distance: 0,
                kind: assets::ROCK,
                x: 70,
                y: -20,
                flags: 0,
            },
            0,
            &bank,
        );
        assert_eq!((game.actors[0].hp, game.actors[1].hp), (2, 255));
        assert_eq!(actor_center_sim(&game.actors[0], &bank).y, 0);
        assert_eq!(
            game.missile_target(Vec3 { x: 0, y: 0, z: 180 }, &mut 0, &bank)
                .unwrap()
                .0
                .x,
            0
        );
        game.kills = 7;
        game.bomb_kills = 4;
        game.actors[0].pos.z = 900;
        game.actors[1].pos.z = 900;
        game.bomb(&bank);
        assert_eq!(game.actors[0].kind, 0);
        assert_eq!(game.actors[1].kind, 0);
        assert_eq!(
            (game.score, game.kills, game.bomb_kills, game.bombs),
            (50, 7, 4, 2)
        );
        assert!(game.pickups.iter().all(|pickup| pickup.kind == 0));
    }
}

#[test]
fn debris_shot_bounds_follow_its_half_size_mesh() {
    let bank = test_bank(12);
    for (x, hit) in [(18, true), (19, false)] {
        let mut game = Game::preview_at_for_test(12);
        game.spawn_actor(
            Spawn {
                distance: 0,
                kind: assets::ROCK,
                x: 0,
                y: -10,
                flags: 2,
            },
            0,
            &bank,
        );
        game.actors[0].pos.z = 520;
        game.shots[0] = Shot {
            pos: Vec3 { x, y: 0, z: 480 },
            damage: 2,
            kind: 1,
            ..Shot::EMPTY
        };
        game.update_shots(&bank);
        assert_eq!(game.actors[0].kind == 0, hit);
        assert_eq!(game.score, if hit { 25 } else { 0 });
    }
}

#[test]
fn large_orbital_rocks_still_absorb_lasers_and_missiles_without_damage() {
    for stage in 12..18 {
        for shot_kind in [1, 3] {
            let mut game = Game::preview_at_for_test(stage);
            let bank = test_bank(stage);
            game.actors[0] = Actor {
                kind: assets::ROCK,
                hp: 255,
                pos: Vec3 {
                    x: 0,
                    y: -20,
                    z: 520,
                },
                ..Actor::EMPTY
            };
            game.shots[0] = Shot {
                pos: Vec3 { x: 0, y: 0, z: 500 },
                damage: 4,
                kind: shot_kind,
                ..Shot::EMPTY
            };
            game.update_shots(&bank);
            assert_eq!(
                (game.actors[0].kind, game.actors[0].hp),
                (assets::ROCK, 255)
            );
            assert_eq!((game.shots[0].kind, game.score, game.kills), (0, 0, 0));
        }
    }
}

#[test]
fn bombs_preserve_terrestrial_obstacles_and_orbital_buildings() {
    for (stage, kind) in [
        (0, assets::ROCK),
        (0, assets::BUILDING),
        (12, assets::BUILDING),
    ] {
        let mut game = Game::preview_at_for_test(stage);
        game.actors[0] = Actor {
            kind,
            hp: 255,
            ..Actor::EMPTY
        };
        game.bomb(&test_bank(stage));
        assert_eq!((game.actors[0].kind, game.actors[0].hp), (kind, 255));
        assert_eq!((game.bombs, game.score, game.kills), (2, 0, 0));
    }
}

#[test]
fn ascent_pitch_levels_out_without_changing_the_controls() {
    let mut game = running();
    game.mode = Mode::Transition;
    set_rail(&mut game, 12 * SECTION_TICKS as u32);
    for (tick, expected) in [(0, 0), (100, 236), (300, 236), (350, 246), (399, 0)] {
        game.transition_ticks = tick;
        assert_eq!(game.player_pitch(), expected);
    }
    game.controls.bits = RIGHT | UP;
    game.transition_ticks = 200;
    for _ in 0..10 {
        game.move_player();
    }
    assert!(game.player.x > 0 && game.player.y > 0);
    // The climb's nose-up pitch adds to the authored ascent.
    assert!(game.flight.pitch < 0);
    assert_eq!(
        game.player_pitch(),
        236u8.wrapping_add((game.flight.pitch >> 8) as u8)
    );
}

impl Game {
    fn preview_at_for_test(stage: u8) -> Self {
        let mut game = running();
        set_rail(&mut game, stage as u32 * SECTION_TICKS as u32);
        game
    }
}

#[test]
fn planar_effects_preserve_ordered_vertices_colors_and_flags() {
    #[derive(Default)]
    struct Capture(std::vec::Vec<([Vec3; 3], u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }
    for x in [-32700, -300, 0, 299, 32700] {
        for y in [-32700, -120, 0, 140, 32700] {
            for z in [79, 80, 599, 600, 601, 32767] {
                let p = Vec3 { x, y, z };
                for r in [-58, -1, 0, 1, 3, 5, 13, 22, 44, 58] {
                    for color in [0, 2, 7, 15] {
                        let left = Vec3 { x: x - r, y, z };
                        let top = Vec3 { x, y: y + r, z };
                        let right = Vec3 { x: x + r, y, z };
                        let bottom = Vec3 { x, y: y - r, z };
                        let mut actual = Capture::default();
                        diamond(&mut actual, p, r, color);
                        assert_eq!(
                            actual.0,
                            [
                                ([left, top, right], color, TWO_SIDED),
                                ([left, right, bottom], color, TWO_SIDED),
                            ]
                        );
                    }
                }
                let mut expected = std::vec::Vec::new();
                for (r, color) in [(13, if z > 600 { 5 } else { 15 }), (10, 5)] {
                    let left = Vec3 { x: x - r, y, z };
                    let right = Vec3 { x: x + r, y, z };
                    let upper_left = Vec3 { y: y + r, ..left };
                    let upper_right = Vec3 { y: y + r, ..right };
                    let bottom = Vec3 { x, y: y - r, z };
                    expected.extend_from_slice(&[
                        ([upper_left, upper_right, right], color, TWO_SIDED),
                        ([upper_left, right, left], color, TWO_SIDED),
                        ([left, right, bottom], color, TWO_SIDED),
                    ]);
                }
                let mut actual = Capture::default();
                shield_pickup(&mut actual, p);
                assert_eq!(actual.0, expected);
                // The bomb pickup is the HUD icon's 4:3 rectangle with the
                // shield's rim rule, wound like the diamond and split along
                // the top-left/bottom-right diagonal.
                let mut expected = std::vec::Vec::new();
                for ((rx, ry), color) in [((16, 12), if z > 600 { 7 } else { 15 }), ((13, 9), 7)] {
                    let upper_left = Vec3 {
                        x: x - rx,
                        y: y + ry,
                        z,
                    };
                    let upper_right = Vec3 {
                        x: x + rx,
                        y: y + ry,
                        z,
                    };
                    let lower_right = Vec3 {
                        x: x + rx,
                        y: y - ry,
                        z,
                    };
                    let lower_left = Vec3 {
                        x: x - rx,
                        y: y - ry,
                        z,
                    };
                    expected.extend_from_slice(&[
                        ([upper_left, upper_right, lower_right], color, TWO_SIDED),
                        ([upper_left, lower_right, lower_left], color, TWO_SIDED),
                    ]);
                }
                let mut actual = Capture::default();
                bomb_pickup(&mut actual, p);
                assert_eq!(actual.0, expected);
            }
        }
    }
}

// Valid, immutable descriptor fixtures also keep no-default-feature tests
// independent of the std-only authoring tools.
const fn test_bank_bytes(level: u8) -> [u8; assets::COMMON_SIZE] {
    let mut data = [0; assets::COMMON_SIZE];
    data[0] = b'S';
    data[1] = b'M';
    data[2] = b'A';
    data[3] = b'4';
    data[4] = assets::MODEL_COUNT;
    data[5] = level;
    let info = assets::STAGE_INFOS[level as usize];
    let profile = info.enemy_profile;
    data[6] = if info.boss_kind != 0 { 2 } else { 3 };
    data[7] = info.boss_kind;
    let health = [[3, 2, 6], [2, 3, 4], [4, 2, 6], [3, 2, 6]];
    let speeds = [[8, 12, 8], [10, 8, 8], [6, 8, 8], [8, 14, 8]];
    let mut kind = 0;
    while kind < 3 {
        let at = assets::ENEMY_SPECS_OFFSET + kind * 8;
        data[at] = health[profile as usize][kind];
        // Slot three is a boss side unit in boss stages and keeps that scale.
        let boss_part = kind == 2 && info.boss_kind != 0;
        data[at + 1] = if kind == 2 && info.boss_kind == 0 && profile != 1 {
            (assets::enemy_scaled(18) - 8) as u8
        } else {
            0
        };
        data[at + 2] = if boss_part {
            (19 * assets::BOSS_SCALE) as u8
        } else {
            assets::enemy_scaled(19) as u8
        };
        data[at + 3] = if boss_part {
            (18 * assets::BOSS_SCALE) as u8
        } else {
            assets::enemy_scaled(18) as u8
        };
        data[at + 4] = if kind == 2 { 250 } else { 100 };
        data[at + 6] = speeds[profile as usize][kind];
        data[at + 7] = if kind == 1 && profile != 2 { 1 } else { 0 };
        kind += 1;
    }
    let mut mesh = 0;
    while mesh < assets::MODEL_COUNT as usize {
        let at = 16 + mesh * 8;
        data[at] = assets::MODEL_DATA_OFFSET as u8;
        data[at + 2] = 3;
        data[at + 3] = 1;
        data[at + 4] = (assets::MODEL_DATA_OFFSET + 9) as u8;
        mesh += 1;
    }
    data[assets::MODEL_DATA_OFFSET + 10] = 1;
    data[assets::MODEL_DATA_OFFSET + 11] = 2;
    data[assets::MODEL_DATA_OFFSET + 12] = 15;
    data
}
pub(super) fn test_bank(level: u8) -> AssetBank<'static> {
    #[cfg(feature = "host-tools")]
    {
        static BANKS: std::sync::OnceLock<
            [[u8; assets::MODEL_SIZE]; assets::SEGMENT_COUNT as usize],
        > = std::sync::OnceLock::new();
        let banks = BANKS.get_or_init(|| {
            core::array::from_fn(|level| enemies::authored::model_bank(level as u8))
        });
        AssetBank::new(&banks[level as usize]).unwrap()
    }
    #[cfg(not(feature = "host-tools"))]
    {
        static BANKS: [[u8; assets::COMMON_SIZE]; assets::SEGMENT_COUNT as usize] = [
            test_bank_bytes(0),
            test_bank_bytes(1),
            test_bank_bytes(2),
            test_bank_bytes(3),
            test_bank_bytes(4),
            test_bank_bytes(5),
            test_bank_bytes(6),
            test_bank_bytes(7),
            test_bank_bytes(8),
            test_bank_bytes(9),
            test_bank_bytes(10),
            test_bank_bytes(11),
            test_bank_bytes(12),
            test_bank_bytes(13),
            test_bank_bytes(14),
            test_bank_bytes(15),
            test_bank_bytes(16),
            test_bank_bytes(17),
        ];
        AssetBank::new(&BANKS[level as usize]).unwrap()
    }
}
use crate::scenery::{
    backdrop_face, terrain_height, BACKDROP_CAMERA_Y, CLOUD_CENTERS, CLOUD_COLORS, CLOUD_OUTLINES,
    HORIZONS, HORIZON_COLORS, SKY_COLORS,
};
#[test]
fn music_follows_stage_boss_crossing_and_result_identity() {
    let mut game = Game::new();
    assert_eq!(game.music_state(), MusicState::Title);
    for index in 0..assets::SEGMENT_COUNT {
        game.mode = Mode::Playing;
        set_rail(&mut game, index as u32 * SECTION_TICKS as u32);
        let cue = MusicState::from_cue(game.stage_info.music_cue).unwrap();
        for mode in [Mode::Playing, Mode::Attract, Mode::Respawning] {
            game.mode = mode;
            game.boss_started = false;
            assert_eq!(game.music_state(), cue);
            if game.stage_info.boss_kind != 0 {
                game.boss_started = true;
                let boss = MusicState::from_cue(match game.stage_info.boss_kind {
                    4 => 12,
                    5 => 20,
                    kind => 9 + kind,
                })
                .unwrap();
                assert_eq!(game.music_state(), boss);
                game.mode = Mode::Transition;
                assert_eq!(
                    game.music_state(),
                    if index == 11 {
                        MusicState::from_cue(19).unwrap()
                    } else {
                        boss
                    }
                );
            }
        }
    }
    game.mode = Mode::Victory;
    assert_eq!(game.music_state(), MusicState::Victory);
    game.mode = Mode::GameOver;
    assert_eq!(game.music_state(), MusicState::Defeat);
    game.mode = Mode::Title;
    assert_eq!(game.music_state(), MusicState::Title);
}

pub(super) fn empty_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
    let mut bytes = [0; assets::SEGMENT_SIZE];
    bytes[..4].copy_from_slice(b"SMS2");
    bytes[4] = index;
    bytes[6] = 5;
    let info = assets::preview_stage_info(index);
    bytes[5] = info.enemy_profile;
    bytes[8..16].copy_from_slice(&[
        info.enemy_profile,
        info.music_cue,
        info.bridge_profile,
        info.boss_kind,
        info.theme,
        info.next_theme,
        info.previous_theme,
        info.reserved,
    ]);
    bytes
}
#[cfg(feature = "host-tools")]
fn mission_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
    enemies::authored::segment_bank(index)
}
#[cfg(not(feature = "host-tools"))]
fn mission_segment(index: u8) -> [u8; assets::SEGMENT_SIZE] {
    let mut bytes = empty_segment(index);
    bytes[5] = assets::preview_stage_info(index).enemy_profile;
    let mut records = [Spawn::default(); 64];
    let mut count = 0;
    if assets::preview_stage_info(index).boss_kind == 0 {
        for wave in 0..10u16 {
            for lane in 0..3i16 {
                records[count] = Spawn {
                    distance: 1200 + wave * 2700 + lane as u16 * 100,
                    kind: if wave % 3 == 2 {
                        assets::DIVER
                    } else {
                        assets::FIGHTER
                    },
                    x: (lane - 1) * 80,
                    y: if wave % 2 == 0 { 35 } else { -25 },
                    flags: if wave % 4 == 0 && lane == 1 { 1 } else { 0 },
                };
                count += 1;
            }
            if wave % 2 == 1 {
                records[count] = Spawn {
                    distance: 2400 + wave * 2700,
                    kind: assets::TURRET,
                    x: if wave % 4 == 1 { -95 } else { 95 },
                    y: -90,
                    flags: 0,
                };
                count += 1;
            }
        }
        for i in 0..6u16 {
            records[count] = Spawn {
                distance: 2200 + i * 4300,
                kind: if index == 2 {
                    assets::BUILDING
                } else {
                    assets::ROCK
                },
                x: if i % 2 == 0 { -70 } else { 70 },
                y: -110,
                flags: 0,
            };
            count += 1;
        }
    }
    records[..count].sort_unstable_by_key(|s| s.distance);
    bytes[7] = count as u8;
    for (i, r) in records[..count].iter().enumerate() {
        let p = 16 + i * 8;
        bytes[p..p + 2].copy_from_slice(&r.distance.to_le_bytes());
        bytes[p + 2] = r.kind;
        bytes[p + 3] = r.flags;
        bytes[p + 4..p + 6].copy_from_slice(&r.x.to_le_bytes());
        bytes[p + 6..p + 8].copy_from_slice(&r.y.to_le_bytes());
    }
    bytes
}
fn running() -> Game {
    let mut game = Game::new();
    game.reset_run();
    game.invulnerability = 0;
    game
}
/// Position hand-built fixtures using the explicit stage identity introduced
/// by the campaign. Transition fixtures remain attached to the outgoing stage.
fn set_rail(game: &mut Game, ticks: impl TryInto<u32>) {
    let ticks = ticks.try_into().ok().unwrap();
    let position = if game.mode == Mode::Transition {
        ticks.saturating_sub(1)
    } else {
        ticks
    };
    game.stage_id = (position / SECTION_TICKS as u32).min((assets::SEGMENT_COUNT - 1) as u32) as u8;
    game.stage_ticks = (ticks - game.stage_id as u32 * SECTION_TICKS as u32) as u16;
    game.stage_info = assets::preview_stage_info(game.stage_id);
}
/// Samples at a millisecond instant; wider values wrap like the 16-bit clock.
fn sample(game: &mut Game, bits: u8, ms: u32) {
    game.sample_input(InputSample {
        bits,
        pressed: 0,
        ms: ms as u16,
    });
}
fn queue_roll(game: &mut Game, direction: u8) {
    sample(game, direction, 100);
    sample(game, 0, 120);
    sample(game, direction, 200);
    sample(game, 0, 220);
}
#[test]
fn scenic_bridges_add_exactly_eight_seconds_and_keep_the_outgoing_checkpoint() {
    for outgoing in 0..assets::SEGMENT_COUNT - 1 {
        let mut game = running();
        let boundary = (outgoing as u32 + 1) * SECTION_TICKS as u32;
        set_rail(&mut game, boundary - 1);
        game.spawn_segment = outgoing;
        game.spawn_cursor = 17;
        game.score = 1234;
        game.shield = 61;
        game.boss_started = game.stage_info.boss_kind != 0;
        game.boss_defeated = game.boss_started;
        game.boss_left = 0;
        game.boss_right = 0;
        game.boss_core = 0;
        let expected_shield = 61;
        let before_scene = game.scenery_ticks();
        let cue = game.music_state();
        let bytes = empty_segment(outgoing);
        let segment = Segment::new(&bytes).unwrap();
        game.tick(&segment, &test_bank(game.segment_index()));
        for elapsed in 0..TRANSITION_TICKS {
            assert_eq!(game.transition_progress(), Some((outgoing, elapsed)));
            assert_eq!(game.hud().distance, SECTION_TICKS * 8);
            assert_eq!(game.segment_index(), outgoing);
            assert_eq!(
                game.music_state(),
                if outgoing == 11 {
                    MusicState::from_cue(19).unwrap()
                } else {
                    cue
                }
            );
            assert_eq!(game.scenery_ticks(), before_scene + elapsed + 1);
            game.damage_player(100);
            assert_eq!(
                (game.shield, game.score, game.lives, game.bombs),
                (expected_shield, 1234, 3, 3)
            );
            game.tick(&segment, &test_bank(game.segment_index()));
        }
        assert_eq!(game.mode, Mode::Playing);
        assert_eq!(game.segment_index(), outgoing + 1);
        assert_eq!(game.stage_ticks, 0);
        assert_eq!(game.scenery_ticks(), 0);
        assert_eq!((game.spawn_segment, game.spawn_cursor), (outgoing + 1, 0));
        assert_eq!(
            (game.boss_left, game.boss_right, game.boss_core),
            (40, 40, 120)
        );
        game.tick(&segment, &test_bank(game.segment_index())); // A stale outgoing view cannot spawn encounters.
        assert_eq!((game.stage_ticks, game.spawn_cursor), (1, 0));
        let mut incoming = empty_segment(outgoing + 1);
        incoming[7] = 1;
        incoming[18] = assets::FIGHTER; // An encounter at local distance zero.
        let incoming = Segment::new(&incoming).unwrap();
        game.tick(&incoming, &test_bank(game.segment_index()));
        assert_eq!(game.spawn_cursor, 1);
        assert_eq!(
            game.actors.iter().filter(|actor| actor.kind != 0).count(),
            1
        );
        assert_eq!(
            game.actors[0].pos.z,
            1784 - test_bank(outgoing + 1).enemy(1).speed as i16
        );
        game.tick(&incoming, &test_bank(game.segment_index()));
        assert_eq!(
            game.actors.iter().filter(|actor| actor.kind != 0).count(),
            1
        );
    }
}
#[test]
fn crossing_exit_exposes_incoming_host_metadata_before_the_next_tick() {
    for outgoing in 0..assets::SEGMENT_COUNT - 1 {
        let mut game = running();
        set_rail(&mut game, (outgoing as u32 + 1) * SECTION_TICKS as u32 - 1);
        game.stage_ticks += 1;
        game.mode = Mode::Transition;
        game.transition_ticks = TRANSITION_TICKS - 1;
        game.boss_started = game.stage_info.boss_kind != 0;
        game.boss_defeated = game.boss_started;
        let bytes = empty_segment(outgoing);
        let segment = Segment::new(&bytes).unwrap();
        game.tick(&segment, &test_bank(outgoing));

        let incoming = assets::preview_stage_info(outgoing + 1);
        assert_eq!(game.mode, Mode::Playing);
        assert_eq!(game.segment_index(), outgoing + 1);
        assert_eq!(game.stage_info(), incoming);
        assert_eq!(game.simulation_profile(), incoming.simulation_profile());
        assert_eq!(
            game.music_state(),
            MusicState::from_cue(incoming.music_cue).unwrap()
        );
        assert_eq!(
            game.sky_color(),
            crate::scenery::sky_color(incoming.theme, None)
        );
        assert_eq!(game.hud().boss_kind, incoming.boss_kind);
    }
}
#[test]
fn bridge_entry_removes_hostiles_before_live_bombs_and_keeps_player_projectiles() {
    let mut game = running();
    set_rail(&mut game, SECTION_TICKS - 1);
    game.actors[0] = enemy(assets::FIGHTER, 0, 0, 180);
    game.shots[0] = Shot {
        pos: game.player,
        kind: 2,
        damage: 100,
        ..Shot::EMPTY
    };
    game.shots[1] = Shot {
        pos: Vec3 {
            x: 60,
            y: 10,
            z: 400,
        },
        kind: 3,
        damage: 4,
        ..Shot::EMPTY
    };
    game.pickups[0] = Pickup {
        pos: game.player,
        kind: 1,
    };
    game.controls.bomb_pending = true;
    let bytes = empty_segment(0);
    game.tick(
        &Segment::new(&bytes).unwrap(),
        &test_bank(game.segment_index()),
    );
    assert!(game.actors.iter().all(|actor| actor.kind == 0));
    assert!(game.shots.iter().all(|shot| shot.kind != 2));
    assert!(game.pickups.iter().all(|pickup| pickup.kind == 0));
    assert_eq!(game.shots[1].pos.z, 432);
    assert_eq!(game.shots[1].kind, 3);
    assert_eq!(
        game.missile_target(game.player, &mut 0, &test_bank(game.segment_index())),
        None
    );
    assert_eq!((game.shield, game.score, game.bombs), (100, 0, 2));
    assert!(game.explosions.iter().any(|explosion| explosion.ticks != 0));
    assert_eq!(game.take_presentation_event(), PresentationEvent::Bomb);
}
#[test]
fn all_controls_and_double_taps_behave_identically_across_both_bridge_seams() {
    for entering in [true, false] {
        let mut game = running();
        if entering {
            set_rail(&mut game, SECTION_TICKS - 2);
        } else {
            game.mode = Mode::Transition;
            set_rail(&mut game, SECTION_TICKS);
            game.transition_ticks = TRANSITION_TICKS - 2;
        }
        let mut reference = running();
        set_rail(&mut reference, 1000);
        let banks = [empty_segment(0), empty_segment(1)];
        for (tick, bits) in [
            LEFT | FIRE,
            0,
            LEFT | FIRE,
            UP | FIRE,
            RIGHT | FIRE,
            0,
            RIGHT,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            // The initial charged shot and each two-tap gesture crosses a seam.
            let ms = 2200 + tick as u32 * 20;
            sample(&mut game, bits, ms);
            sample(&mut reference, bits, ms);
            game.tick(
                &Segment::new(&banks[game.segment_index() as usize]).unwrap(),
                &test_bank(game.segment_index()),
            );
            reference.tick(
                &Segment::new(&banks[0]).unwrap(),
                &test_bank(reference.segment_index()),
            );
            assert_eq!(game.player, reference.player);
            assert_eq!(game.flight, reference.flight);
            assert_eq!(game.roll_ticks, reference.roll_ticks);
            assert_eq!(game.bombs, reference.bombs);
            assert_eq!(
                game.take_presentation_event(),
                reference.take_presentation_event()
            );
            assert_eq!(game.controls.charge(), reference.controls.charge());
            for (actual, expected) in game.shots.iter().zip(&reference.shots) {
                assert_eq!(
                    (actual.kind, actual.pos, actual.damage),
                    (expected.kind, expected.pos, expected.damage)
                );
            }
            while let Some(expected) = reference.pop_audio() {
                assert_eq!(game.pop_audio(), Some(expected));
            }
            assert!(game.pop_audio().is_none());
        }
        assert_eq!(game.bombs, 2);
        assert!(game.roll_ticks > 0);
    }
}
#[test]
fn all_eighteen_checkpoints_restore_stage_encounters_and_bosses_after_repeated_deaths() {
    for stage in 0..assets::SEGMENT_COUNT {
        let mut game = running();
        set_rail(&mut game, stage as u32 * SECTION_TICKS as u32 + 2200);
        game.score = 12345;
        for remaining in [2, 1] {
            game.boss_started = true;
            game.boss_left = 0;
            game.boss_right = 3;
            game.boss_core = 7;
            game.spawn_cursor = 20;
            game.actors[0] = enemy(assets::FIGHTER, 0, 0, 800);
            game.shots[0].kind = 2;
            game.invulnerability = 0;
            game.damage_player(100);
            assert_eq!(game.mode, Mode::Respawning);
            assert_eq!(game.lives, remaining);
            game.respawn();
            assert_eq!(game.stage_id, stage);
            assert_eq!(game.stage_info, assets::preview_stage_info(stage));
            assert_eq!(game.stage_ticks, 0);
            assert_eq!(game.scenery_ticks(), 0);
            assert_eq!((game.spawn_segment, game.spawn_cursor), (stage, 0));
            assert_eq!(
                (game.boss_left, game.boss_right, game.boss_core),
                (40, 40, 120)
            );
            assert!(!game.boss_started && !game.boss_defeated);
            assert!(game.actors.iter().all(|actor| actor.kind == 0));
            assert!(game.shots.iter().all(|shot| shot.kind == 0));
            assert_eq!(game.score, 12345);
        }
    }
}
#[test]
fn all_boss_gates_start_on_time_and_allow_early_or_late_kills() {
    for stage in [3u8, 7, 11, 16, 17] {
        for early in [true, false] {
            let bytes = empty_segment(stage);
            let segment = Segment::new(&bytes).unwrap();
            let bank = test_bank(stage);
            let mut game = running();
            set_rail(
                &mut game,
                stage as u32 * SECTION_TICKS as u32
                    + if stage >= 16 {
                        SPACE_BOSS_START_TICKS as u32
                    } else {
                        BOSS_START_TICKS as u32
                    }
                    - 2,
            );
            game.tick(&segment, &bank);
            assert!(!game.boss_started);
            game.tick(&segment, &bank);
            assert!(game.boss_started);
            assert_eq!(
                game.simulation_profile(),
                game.stage_info.simulation_profile()
            );
            assert_eq!(game.hud().boss_kind, game.stage_info.boss_kind);
            assert_eq!(game.hud().boss_shield, 100);
            let boundary = (stage as u32 + 1) * SECTION_TICKS as u32;
            if early {
                game.boss_left = 0;
                game.boss_right = 0;
                game.boss_core = 0;
                game.check_boss_death();
                game.tick(&segment, &bank);
                assert_eq!(
                    game.mode,
                    Mode::Playing,
                    "early kill must retain the stage duration"
                );
                set_rail(&mut game, boundary - 1);
            } else {
                set_rail(&mut game, boundary - 1);
                let animation = game.ticks;
                let before_z = game.boss_z;
                for _ in 0..80 {
                    game.tick(&segment, &bank);
                }
                assert_eq!(game.mode, Mode::Playing);
                assert_eq!(game.stage_ticks, SECTION_TICKS);
                assert_eq!(game.ticks, animation + 80);
                assert!(
                    game.boss_z < before_z,
                    "combat keeps advancing while the route is held"
                );
                game.boss_left = 0;
                game.boss_right = 0;
                game.boss_core = 0;
                game.check_boss_death();
            }
            game.tick(&segment, &bank);
            assert_eq!(game.stage_ticks, SECTION_TICKS);
            assert_eq!(
                game.mode,
                if stage == assets::SEGMENT_COUNT - 1 {
                    Mode::Victory
                } else {
                    Mode::Transition
                }
            );
            assert_eq!(game.score, 5000);
        }
    }
}
#[test]
fn act_clear_awards_a_life_without_repeating_the_boss_refill() {
    for stage in [3u8, 7, 11, 16] {
        for (bombs, lives, expected_lives) in [(0, 1, 2), (1, 2, 3), (3, 3, 3)] {
            let bytes = empty_segment(stage);
            let segment = Segment::new(&bytes).unwrap();
            let bank = test_bank(stage);
            let mut game = running();
            set_rail(&mut game, (stage as u32 + 1) * SECTION_TICKS as u32 - 1);
            game.boss_started = true;
            game.boss_defeated = true;
            game.shield = 17;
            game.bombs = bombs;
            game.lives = lives;
            game.score = 4567;
            game.tick(&segment, &bank);
            assert_eq!(game.mode, Mode::Transition);
            assert_eq!(
                (game.shield, game.bombs, game.lives),
                (17, bombs, expected_lives)
            );
            game.shield = 72;
            game.bombs = 0;
            game.lives = 1;
            for _ in 0..TRANSITION_TICKS {
                game.tick(&segment, &bank);
            }
            assert_eq!(
                (game.shield, game.bombs, game.lives, game.score),
                (72, 0, 1, 4567)
            );
            assert_eq!(game.stage_id, stage + 1);
            let incoming = empty_segment(stage + 1);
            let incoming = Segment::new(&incoming).unwrap();
            game.bind_stage(&incoming);
            assert_eq!(game.stage_info, assets::preview_stage_info(stage + 1));
            assert_eq!(
                game.simulation_profile(),
                game.stage_info.simulation_profile()
            );
            assert_eq!(
                game.music_state(),
                MusicState::from_cue(game.stage_info.music_cue).unwrap()
            );
        }
    }
}
#[test]
fn every_boss_refills_immediately_once_by_projectile_or_finishing_bomb() {
    for stage in [3, 7, 11, 16, 17] {
        for use_bomb in [false, true] {
            let mut game = Game::preview_at_for_test(stage);
            let bank = test_bank(stage);
            game.boss_started = true;
            game.boss_left = 0;
            game.boss_right = 0;
            game.boss_core = if use_bomb { 8 } else { 4 };
            game.boss_z = 680;
            game.shield = 17;
            game.bombs = 1;
            game.lives = 2;
            game.shots[1] = Shot {
                kind: 2,
                ..Shot::EMPTY
            };
            if use_bomb {
                game.bomb(&bank);
            } else {
                game.shots[0] = Shot {
                    pos: Vec3 { x: 0, y: 0, z: 450 },
                    kind: 1,
                    damage: 4,
                    ..Shot::EMPTY
                };
                game.update_shots(&bank);
            }
            assert!(game.boss_defeated, "stage {stage}, bomb {use_bomb}");
            assert_eq!(
                (game.shield, game.bombs, game.lives, game.score),
                (100, 3, 2, 5000)
            );
            assert!(game.shots.iter().all(|s| s.kind != 2));
            game.shield = 73;
            game.bombs = 0;
            game.check_boss_death();
            assert_eq!((game.shield, game.bombs, game.score), (73, 0, 5000));
        }
    }
}

#[test]
fn doubled_boss_health_keeps_a_normalized_hud_until_the_last_hit() {
    let mut game = running();
    assert_eq!(
        (game.boss_left, game.boss_right, game.boss_core),
        (40, 40, 120)
    );
    assert_eq!(game.hud().boss_shield, 0);
    game.boss_started = true;
    assert_eq!(game.hud().boss_shield, 100);
    game.boss_left = 0;
    game.boss_right = 0;
    for (health, percent) in [(120, 60), (119, 60), (2, 1), (1, 1), (0, 0)] {
        game.boss_core = health;
        assert_eq!(game.hud().boss_shield, percent);
    }
}

#[test]
fn each_boss_has_a_distinct_side_and_exposed_core_attack() {
    for (stage, rate, side_shots, core_rate, core_shots) in
        [(3, 50, 1, 50, 2), (7, 45, 3, 45, 1), (11, 40, 2, 25, 1)]
    {
        let mut game = running();
        set_rail(&mut game, stage * SECTION_TICKS + BOSS_START_TICKS);
        game.boss_z = 680;
        game.ticks = rate - 1;
        game.update_boss();
        assert!(game.shots.iter().all(|shot| shot.kind == 0));
        game.ticks = rate;
        game.update_boss();
        assert_eq!(
            game.shots.iter().filter(|shot| shot.kind == 2).count(),
            side_shots
        );
        if stage != 11 {
            assert!(game.shots[..side_shots]
                .iter()
                .all(|shot| shot.pos.x == BOSS_TURRET_X));
        }
        if stage == 7 {
            let mut dx: std::vec::Vec<_> = game.shots[..3].iter().map(|shot| shot.dx).collect();
            dx.sort();
            assert_eq!([dx[1] - dx[0], dx[2] - dx[1]], [2, 2]);
            assert!(
                dx[1] < 0,
                "the fan aims inward from the right reactor pylon"
            );
        }
        game.shots.fill(Shot::EMPTY);
        game.boss_left = 0;
        game.boss_right = 0;
        game.ticks = core_rate;
        game.update_boss();
        assert_eq!(
            game.shots.iter().filter(|shot| shot.kind == 2).count(),
            core_shots
        );
        assert!(game.shots[..core_shots]
            .iter()
            .all(|shot| shot.pos.z == 480));
    }
}
#[test]
fn checkpoints_and_carrier_start_use_combat_distance_after_bridges() {
    let banks = [empty_segment(10), empty_segment(11)];
    let mut before = running();
    set_rail(&mut before, SECTION_TICKS * 11 - 1);
    before.damage_player(100);
    before.respawn();
    assert_eq!(before.stage_ticks, 0);
    assert_eq!(before.scenery_ticks(), 0);
    let mut game = running();
    set_rail(&mut game, SECTION_TICKS * 11 - 1);
    game.tick(
        &Segment::new(&banks[0]).unwrap(),
        &test_bank(game.segment_index()),
    );
    for _ in 0..TRANSITION_TICKS {
        game.tick(
            &Segment::new(&banks[0]).unwrap(),
            &test_bank(game.segment_index()),
        );
        assert!(!game.boss_started);
    }
    for _ in 0..BOSS_START_TICKS - 1 {
        game.tick(
            &Segment::new(&banks[1]).unwrap(),
            &test_bank(game.segment_index()),
        );
        assert!(!game.boss_started);
    }
    game.tick(
        &Segment::new(&banks[1]).unwrap(),
        &test_bank(game.segment_index()),
    );
    assert!(game.boss_started);
    assert_eq!(game.stage_ticks, BOSS_START_TICKS);
    game.damage_player(100);
    game.respawn();
    assert_eq!(game.stage_ticks, 0);
    assert_eq!(game.transition_progress(), None);
    assert_eq!(game.scenery_ticks(), 0);
    set_rail(&mut game, MISSION_TICKS - 1);
    game.boss_defeated = true;
    game.tick(
        &Segment::new(&banks[1]).unwrap(),
        &test_bank(game.segment_index()),
    );
    assert_eq!(game.mode, Mode::Victory);
    assert_eq!(game.scenery_ticks(), SECTION_TICKS);
}
#[derive(Debug, PartialEq, Eq)]
struct TerrainCapture {
    faces: [([Vec3; 3], u8); 54],
    count: usize,
}
impl GeometrySink for TerrainCapture {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        assert_eq!(
            flags,
            TWO_SIDED
                | if flags & STATIC_SKY != 0 {
                    STATIC_SKY
                } else {
                    crate::EXTERIOR_TERRAIN
                }
        );
        assert!(
            self.count < self.faces.len(),
            "terrain exceeded its face reservation"
        );
        self.faces[self.count] = ([a, b, c], color);
        self.count += 1;
    }
}
fn theme_stage(theme: u8) -> u8 {
    [0, 2, 6, 10, 1, 4, 5, 7, 8, 9][theme as usize]
}
fn terrain_scene(style: u8, ticks: u16) -> TerrainCapture {
    let mut bytes = empty_segment(theme_stage(style));
    bytes[6] = [5, 9, 11, 6, 3, 11, 9, 12, 11, 4][style as usize];
    let segment = Segment::new(&bytes).unwrap();
    let mut game = Game::new();
    set_rail(&mut game, ticks);
    let mut capture = TerrainCapture {
        faces: [([Vec3::default(); 3], 0); 54],
        count: 0,
    };
    game.terrain(&mut capture, &segment);
    capture
}
#[test]
fn terrain_profile_is_bounded_repeatable_and_asymmetric() {
    for style in 0..10 {
        let (low, high) = match style {
            0 => (-105, 23),
            1 => (96, 160),
            2 | 3 => (-105, -41),
            4 => (-70, 122),
            5 => (-80, 112),
            6 => (-60, 36),
            7 => (-40, 88),
            8 => (-90, 70),
            _ => (-100, 92),
        };
        let mut seen_low = false;
        let mut seen_high = false;
        let mut different_sides = false;
        for row in 0..16 {
            let left = terrain_height(row, false, style);
            let right = terrain_height(row, true, style);
            assert!((low..=high).contains(&left));
            assert!((low..=high).contains(&right));
            assert_eq!(terrain_height(row + 16, false, style), left);
            assert_eq!(terrain_height(row + 7, false, style), right);
            seen_low |= left == low;
            seen_high |= left == high;
            different_sides |= left != right;
        }
        assert!(seen_low && seen_high && different_sides);
    }
}
#[test]
fn terrain_keeps_the_flight_lane_flat_within_its_face_budget() {
    for style in 0..10 {
        for ticks in [0, 1, 63, 64, 1023, 3750, 14999] {
            let scene = terrain_scene(style, ticks);
            assert_eq!(scene, terrain_scene(style, ticks));
            assert_eq!(
                scene.count,
                if matches!(style, 1 | 2 | 6 | 7) {
                    54
                } else {
                    38
                }
            );
            let surface = &scene.faces[..scene.count - 14];
            let mut raised = false;
            for (points, color) in surface {
                assert!(*color < 16);
                for point in points {
                    if point.x.abs() <= 145 {
                        let guide = matches!(style, 2 | 6 | 7) && matches!(point.x.abs(), 95 | 100);
                        assert_eq!(point.y, if guide { -103 } else { -105 });
                    } else {
                        raised |= point.y > -105;
                    }
                }
            }
            assert!(raised, "each view must contain actual raised geometry");
            assert!(
                surface.chunks_exact(2).any(|pair| pair[0].1 != pair[1].1),
                "raised facets need distinct flat colors"
            );
        }
    }
}
#[test]
fn adjacent_terrain_bands_share_exact_edges() {
    for style in 0..10 {
        for ticks in [0, 19, 63, 64, 511, 3750, 14999] {
            let scene = terrain_scene(style, ticks);
            let faces_per_band = (scene.count - 14) / 4;
            for band in 0..3 {
                let left = &scene.faces[band * faces_per_band..(band + 1) * faces_per_band];
                let right = &scene.faces[(band + 1) * faces_per_band..(band + 2) * faces_per_band];
                let shared_z = 40 + (band as i16 + 1) * 512 - ((ticks & 63) * 8) as i16;
                for (from, to) in [(left, right), (right, left)] {
                    let mut shared = 0;
                    for point in from.iter().flat_map(|face| face.0.iter()) {
                        if point.z == shared_z {
                            assert!(to
                                .iter()
                                .flat_map(|face| face.0.iter())
                                .any(|other| other == point));
                            shared += 1;
                        }
                    }
                    assert!(shared >= 4);
                }
            }
        }
    }
}
#[test]
fn terrain_scrolls_continuously_across_row_replacement() {
    for style in 0..10 {
        // Includes the old 20-tick reset and two complete 64-tick rows.
        for tick in 0..130 {
            let before = terrain_scene(style, tick);
            let after = terrain_scene(style, tick + 1);
            let per_band = (before.count - 14) / 4;
            let retired = if tick & 63 == 63 { per_band } else { 0 };
            for i in 0..before.count - 14 - retired {
                let (old_points, old_color) = before.faces[i + retired];
                let (new_points, new_color) = after.faces[i];
                assert_eq!(
                    old_color, new_color,
                    "facet color must remain world anchored"
                );
                for corner in 0..3 {
                    let mut translated = old_points[corner];
                    translated.z -= SPEED;
                    assert_eq!(translated, new_points[corner], "style {style}, tick {tick}");
                }
            }
        }
    }
}
#[test]
fn clouds_remain_in_the_sky_behind_scenery_with_shared_facet_edges() {
    for theme in 0..10 {
        for face in 5..14 {
            let (points, color) = backdrop_face(theme, face);
            assert!(CLOUD_COLORS[theme as usize].contains(&color));
            for point in points {
                assert!(
                    point.z > 2400,
                    "clouds must be behind gameplay and mountains"
                );
                let (_, screen_y) = crate::projection::project(point);
                assert!(
                        (24..90).contains(&screen_y),
                        "cloud must stay above the horizon and below attract labels: theme {theme}, point {point:?}, screen {screen_y}"
                    );
            }
        }
        for cloud in 0..3 {
            let first = backdrop_face(theme, 5 + cloud * 3).0;
            let second = backdrop_face(theme, 6 + cloud * 3).0;
            let third = backdrop_face(theme, 7 + cloud * 3).0;
            assert_eq!((first[0], first[2]), (second[0], second[1]));
            assert_eq!((second[0], second[2]), (third[0], third[1]));
        }
    }
}
#[test]
fn backdrop_screen_geometry_follows_only_the_sky_parallax_across_camera_modes() {
    struct SkyCapture {
        faces: [([Vec3; 3], u8); 14],
        count: usize,
    }
    impl GeometrySink for SkyCapture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            if flags & STATIC_SKY != 0 {
                self.faces[self.count] = ([a, b, c], color);
                self.count += 1;
            }
        }
    }
    let bytes = model_bank_for_tests();
    let bank = AssetBank::new(&bytes).unwrap();
    for mode in [
        Mode::Title,
        Mode::Attract,
        Mode::Playing,
        Mode::Respawning,
        Mode::Victory,
        Mode::GameOver,
    ] {
        for player_y in [-65, -1, 0, 1, 65] {
            for ticks in (0..12).flat_map(|stage| {
                [
                    stage * SECTION_TICKS,
                    stage * SECTION_TICKS + 31,
                    (stage + 1) * SECTION_TICKS - 1,
                ]
            }) {
                let mut game = Game::new();
                game.mode = mode;
                game.player.y = player_y;
                game.player.x = if player_y < 0 { -100 } else { 100 };
                set_rail(&mut game, ticks);
                let theme = game.stage_info.theme;
                // The chase camera never moves the backdrop; only the slight
                // parallax drift does, and the title and map tour have none.
                let [dx, dy] = game.sky_parallax();
                if matches!(mode, Mode::Title | Mode::Attract) {
                    assert_eq!([dx, dy], [0, 0]);
                } else {
                    assert_eq!([dx, dy], [game.player.x, player_y >> 1]);
                }
                let expected: [_; 14] = core::array::from_fn(|face| {
                    let (points, color) = backdrop_face(theme, face as u8);
                    (
                        points.map(|point| Vec3 {
                            x: point.x - dx,
                            y: point.y - dy,
                            z: point.z,
                        }),
                        color,
                    )
                });
                let segment_bytes = empty_segment(game.segment_index());
                let segment = Segment::new(&segment_bytes).unwrap();
                let mut capture = SkyCapture {
                    faces: [([Vec3::default(); 3], 0); 14],
                    count: 0,
                };
                game.render(&mut capture, &bank, &segment);
                assert_eq!(capture.count, 14);
                assert_eq!(capture.faces, expected);
            }
        }
    }
}
#[test]
fn clouds_are_visible_and_nearer_geometry_occludes_them() {
    for theme in 0..10 {
        let mut renderer = crate::render::Renderer::new();
        let (cloud, color) = backdrop_face(theme, 8);
        let near = cloud.map(|p| Vec3 {
            x: p.x / 2,
            y: p.y / 2,
            z: p.z / 2,
        });
        // Submit the nearer triangle first to exercise the depth sort.
        renderer.triangle(near[0], near[1], near[2], 6, TWO_SIDED);
        renderer.triangle(cloud[0], cloud[1], cloud[2], color, TWO_SIDED);
        assert_eq!(renderer.triangle_count, 2);
        renderer.finish();
        let mut cloud_only = crate::render::Renderer::new();
        cloud_only.triangle(cloud[0], cloud[1], cloud[2], color, TWO_SIDED);
        let mut near_only = crate::render::Renderer::new();
        near_only.triangle(near[0], near[1], near[2], 6, TWO_SIDED);
        let mut overlap = 0;
        for band in 0..25 {
            let mut actual = [0; crate::render::BAND_PIXELS];
            let mut distant = [0; crate::render::BAND_PIXELS];
            let mut foreground = [0; crate::render::BAND_PIXELS];
            renderer.render_band(band, &mut actual);
            cloud_only.render_band(band, &mut distant);
            near_only.render_band(band, &mut foreground);
            for pixel in 0..actual.len() {
                let expected = if foreground[pixel] == 6 {
                    6
                } else {
                    distant[pixel]
                };
                assert_eq!(actual[pixel], expected);
                if foreground[pixel] == 6 && distant[pixel] == color {
                    overlap += 1;
                }
            }
        }
        assert!(overlap > 0, "nearer geometry must visibly cover the cloud");
    }
}
#[test]
fn section_themes_are_distinct_compact_and_keep_all_backdrop_facets_cacheable() {
    let bytes = model_bank_for_tests();
    let bank = AssetBank::new(&bytes).unwrap();
    for theme in 0..10 {
        let mut game = running();
        set_rail(&mut game, theme_stage(theme) as u16 * SECTION_TICKS + 500);
        assert_eq!(game.sky_color(), SKY_COLORS[theme as usize]);
        let colors = [
            game.sky_color(),
            CLOUD_COLORS[theme as usize][0],
            CLOUD_COLORS[theme as usize][1],
        ];
        // Black is an intentional shadow facet in the orange ash landscape;
        // it occupies the VIC background slot rather than a nonblack palette.
        assert!(colors.iter().all(|color| *color < 16));
        assert_ne!(colors[0], colors[1]);
        assert_ne!(colors[0], colors[2]);
        assert_ne!(colors[1], colors[2]);
        for other in 0..theme {
            let horizon: [_; 5] = core::array::from_fn(|face| backdrop_face(theme, face as u8).0);
            let clouds: [_; 9] =
                core::array::from_fn(|face| backdrop_face(theme, 5 + face as u8).0);
            assert_ne!(
                horizon,
                core::array::from_fn(|face| backdrop_face(other, face as u8).0)
            );
            assert_ne!(
                clouds,
                core::array::from_fn(|face| backdrop_face(other, 5 + face as u8).0)
            );
        }
        let mut renderer = crate::render::Renderer::new();
        renderer.begin(game.sky_color());
        for face in 0..14 {
            let (points, color) = backdrop_face(theme, face);
            renderer.triangle(
                points[0],
                points[1],
                points[2],
                color,
                TWO_SIDED | STATIC_SKY,
            );
        }
        assert_eq!(
            renderer.triangle_count, 14,
            "theme {theme} lost a projected backdrop facet"
        );
        renderer.finish();
        for band in 0..25 {
            let mut pixels = [0; crate::render::BAND_PIXELS];
            renderer.render_band(band, &mut pixels);
            for cell in 0..40 {
                let mut colors = 0u16;
                for row in 0..8 {
                    for column in 0..4 {
                        colors |= 1 << pixels[row * 160 + cell * 4 + column];
                    }
                }
                assert!(
                    (colors & !1).count_ones() <= 3,
                    "backdrop palette overflow: theme {theme}, band {band}, cell {cell}"
                );
            }
        }
        let segment_bytes = empty_segment(theme_stage(theme));
        let segment = Segment::new(&segment_bytes).unwrap();
        renderer.begin(game.sky_color());
        game.render(&mut renderer, &bank, &segment);
        assert_eq!(renderer.dropped, 0);
    }
    assert!(
        core::mem::size_of_val(&SKY_COLORS)
            + core::mem::size_of_val(&HORIZON_COLORS)
            + core::mem::size_of_val(&CLOUD_COLORS)
            + core::mem::size_of_val(&HORIZONS)
            + core::mem::size_of_val(&CLOUD_CENTERS)
            + core::mem::size_of_val(&CLOUD_OUTLINES)
            <= 600
    );
}
#[test]
fn backdrop_drifts_opposite_to_the_ship_while_terrain_and_title_stay_fixed() {
    #[derive(Default)]
    struct Capture(std::vec::Vec<([Vec3; 3], u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }
    for stage in [0u8, 2, 6, 12] {
        let bytes = mission_segment(stage);
        let segment = Segment::new(&bytes).unwrap();
        // Sideways movement alone: the camera is unchanged, so terrain and
        // scenery must be identical while every backdrop face shifts left.
        let mut centered = Game::preview_at_for_test(stage);
        let mut moved = Game::preview_at_for_test(stage);
        centered.player.y = -64;
        moved.player.x = 100;
        moved.player.y = -64;
        assert_eq!(centered.sky_parallax(), [0, -32]);
        assert_eq!(moved.sky_parallax(), [100, -32]);
        let mut before = Capture::default();
        centered.render_scenery(&mut before, &segment);
        let mut after = Capture::default();
        moved.render_scenery(&mut after, &segment);
        assert_eq!(before.0.len(), after.0.len());
        let mut drifted = 0;
        for (old, new) in before.0.iter().zip(&after.0) {
            assert_eq!((old.1, old.2), (new.1, new.2));
            let backdrop = old.2 & STATIC_SKY != 0;
            for (a, b) in old.0.iter().zip(&new.0) {
                if backdrop {
                    assert_eq!((b.x, b.y, b.z), (a.x - 100, a.y, a.z), "stage {stage}");
                } else {
                    assert_eq!(a, b, "stage {stage}: terrain or scenery drifted");
                }
            }
            drifted += usize::from(backdrop);
        }
        assert!(drifted >= 10, "stage {stage} has no drifting backdrop");
        // Climbing lowers the backdrop (the camera also rises, moving terrain).
        let level = Game::preview_at_for_test(stage);
        let mut climbing = Game::preview_at_for_test(stage);
        climbing.player.y = 64;
        let mut before = Capture::default();
        level.render_scenery(&mut before, &segment);
        let mut after = Capture::default();
        climbing.render_scenery(&mut after, &segment);
        for (old, new) in before.0.iter().zip(&after.0) {
            if old.2 & STATIC_SKY != 0 {
                for (a, b) in old.0.iter().zip(&new.0) {
                    assert_eq!((b.x, b.y, b.z), (a.x, a.y - 32, a.z), "stage {stage}");
                }
            }
        }
    }
    let mut title = Game::new();
    title.player.x = 100;
    assert_eq!(title.sky_parallax(), [0, 0]);
    title.reset_scene(Mode::Attract, 0, 0);
    title.player.x = 100;
    assert_eq!(title.sky_parallax(), [0, 0]);
}
#[test]
fn lowered_horizons_overlap_terrain_at_every_scroll_phase_and_camera_extreme() {
    fn occupied_rows(renderer: &crate::render::Renderer, last: bool) -> [Option<usize>; 160] {
        let mut rows = [None; 160];
        for band in 0..25 {
            let mut pixels = [0; crate::render::BAND_PIXELS];
            renderer.render_band(band, &mut pixels);
            for (offset, color) in pixels.iter().enumerate() {
                let column = offset % 160;
                if *color != 0 && (last || rows[column].is_none()) {
                    rows[column] = Some(band as usize * 8 + offset / 160);
                }
            }
        }
        rows
    }
    let mut worst = (0i16, 0, 0, 0, 0, 0, 0);
    for theme in 0..10 {
        // The cave deliberately leaves a dark interior between its roof and
        // floor. Its enclosing silhouette is checked separately below.
        if theme == 4 {
            continue;
        }
        // Title framing at camera 0 without drift, then the playing camera at
        // the ship's vertical extremes with the matching backdrop drift
        // (sky_parallax): a climbing camera lowers the backdrop, a diving one
        // raises it toward the terrain.
        for (camera_y, parallax_y) in [
            (0, 0),
            (chase_camera_y(Mode::Playing, -65), -65 >> 1),
            (40, 0),
            (chase_camera_y(Mode::Playing, 65), 65 >> 1),
        ] {
            let mut horizon = crate::render::Renderer::new();
            for face in 0..5 {
                let (points, color) = backdrop_face(theme, face);
                assert_ne!(color, SKY_COLORS[theme as usize]);
                assert!(points
                    .iter()
                    .any(|point| point.y == -544 - BACKDROP_CAMERA_Y));
                assert!(points
                    .iter()
                    .all(|point| point.z == 2350 && point.y >= -544 - BACKDROP_CAMERA_Y));
                let points = points.map(|point| Vec3 {
                    y: point.y - parallax_y,
                    ..point
                });
                horizon.triangle(points[0], points[1], points[2], 1, TWO_SIDED);
            }
            let horizon_bottom = occupied_rows(&horizon, true);
            for phase in 0..1024 {
                // All 64 scroll positions through all 16 repeating rows.
                let terrain = terrain_scene(theme, phase);
                let mut ground = crate::render::Renderer::new();
                for (points, _) in &terrain.faces[..terrain.count - 14] {
                    let points = points.map(|point| camera_point(point, camera_y));
                    ground.triangle(points[0], points[1], points[2], 1, TWO_SIDED);
                }
                let ground_top = occupied_rows(&ground, false);
                for column in 0..160 {
                    let bottom = horizon_bottom[column].expect("horizon must span the viewport");
                    let top = ground_top[column].expect("terrain must span the viewport");
                    let gap = top as i16 - bottom as i16 - 1;
                    if gap > worst.0 {
                        worst = (gap, theme, phase, camera_y, column, bottom, top);
                    }
                }
            }
        }
    }
    assert!(
        worst.0 <= 0,
        "sky gap (pixels, theme, phase, camera, x, horizon, terrain): {worst:?}"
    );
}
#[test]
fn crystal_cave_has_an_unbroken_roof_side_walls_and_visible_hanging_crystals() {
    let mut roof = crate::render::Renderer::new();
    for face in 0..5 {
        let (points, color) = backdrop_face(4, face);
        assert!([6, 11].contains(&color));
        roof.triangle(points[0], points[1], points[2], 1, TWO_SIDED);
    }
    roof.finish();
    let mut silhouette = [0; 160 * 200];
    for band in 0..25 {
        let mut pixels = [0; crate::render::BAND_PIXELS];
        roof.render_band(band, &mut pixels);
        silhouette[band as usize * pixels.len()..(band as usize + 1) * pixels.len()]
            .copy_from_slice(&pixels);
    }
    // Above the opening, the ceiling spans every column. Both walls surround
    // the distant flight opening; the interior retains a broad clear center.
    for y in 8..32 {
        assert!(silhouette[y * 160..(y + 1) * 160].iter().all(|&p| p == 1));
    }
    for y in 32..90 {
        assert_eq!(silhouette[y * 160 + 2], 1);
        assert_eq!(silhouette[y * 160 + 157], 1);
    }
    assert!(silhouette[70 * 160 + 45..70 * 160 + 115]
        .iter()
        .all(|&p| p == 0));
    for cluster in 0..3 {
        let first = backdrop_face(4, 5 + cluster * 3).0;
        let last = backdrop_face(4, 7 + cluster * 3).0;
        for root in [first[0], last[2]] {
            let (x, y) = crate::projection::project(root);
            assert_eq!(silhouette[y as usize * 160 + x as usize], 1);
        }
        let (tip_x, tip_y) = crate::projection::project(first[2]);
        assert_eq!(silhouette[tip_y as usize * 160 + tip_x as usize], 0);
        assert!(tip_y >= 60, "crystals must visibly hang into the opening");
    }
}
#[test]
fn resident_state_fits_budget() {
    assert_eq!(core::mem::size_of::<PresentationEvent>(), 1);
    // Host C alignment adds eight bytes relative to the byte-aligned MOS
    // layout. The native compile-time assertion and ELF audit enforce 640.
    let alignment_padding = if cfg!(target_arch = "mos") { 0 } else { 8 };
    assert!(
        core::mem::size_of::<Game>() <= 640 + alignment_padding,
        "{}",
        core::mem::size_of::<Game>()
    );
}
#[test]
fn attract_silences_all_effects_and_gameplay_restores_them() {
    let mut g = Game::new();
    g.reset_scene(Mode::Attract, 0, 0);
    let effects = [
        Effect::Fire,
        Effect::EnemyFire,
        Effect::MissileLaunch,
        Effect::Hit,
        Effect::Pickup,
        Effect::Roll,
        Effect::Bomb,
        Effect::Death,
    ];
    for effect in effects {
        g.sound(effect, 64);
        assert!(g.pop_audio().is_none(), "{effect:?} during attract");
    }
    g.reset_run();
    for effect in effects {
        g.sound(effect, 64);
        assert_eq!(g.pop_audio(), Some(AudioEvent { effect, pan: 64 }));
    }
}
#[test]
fn attract_enemy_and_carrier_shots_spawn_and_move_silently() {
    for carrier in [false, true] {
        let mut g = Game::new();
        if carrier {
            g.reset_scene(Mode::Attract, 11, 1000);
            g.boss_z = 680;
            g.ticks = 39;
        } else {
            g.reset_scene(Mode::Attract, 0, 0);
            g.actors[0] = enemy(assets::FIGHTER, 50, 0, 1000);
            g.actors[0].age = 64;
        }
        let bytes = empty_segment(g.segment_index());
        let segment = Segment::new(&bytes).unwrap();
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(
            g.shots.iter().filter(|shot| shot.kind == 2).count(),
            if carrier { 2 } else { 1 }
        );
        assert!(g.pop_audio().is_none());
        let z = g.shots[0].pos.z;
        g.tick(&segment, &test_bank(g.segment_index()));
        assert_eq!(g.shots[0].kind, 2);
        assert_eq!(g.shots[0].pos.z, z - 18);
        assert!(g.pop_audio().is_none());
    }
}
#[test]
fn attract_has_no_player_collisions_or_pickup_collection() {
    let mut g = Game::new();
    g.reset_scene(Mode::Attract, 0, 0);
    g.shield = 30;
    g.bombs = 0;
    g.actors[0] = enemy(assets::FIGHTER, 0, 0, PLAYER_Z + SPEED);
    g.shots[0] = Shot {
        pos: Vec3 {
            x: 0,
            y: 0,
            z: PLAYER_Z + 18,
        },
        damage: 12,
        kind: 2,
        ..Shot::EMPTY
    };
    g.pickups[0] = Pickup {
        pos: Vec3 {
            x: 0,
            y: 0,
            z: PLAYER_Z + SPEED,
        },
        kind: 1,
    };
    g.pickups[1] = Pickup {
        kind: 2,
        ..g.pickups[0]
    };
    g.update_actors(&test_bank(g.segment_index()));
    g.update_shots(&test_bank(g.segment_index()));
    g.update_pickups();
    assert_ne!(g.actors[0].kind, 0, "enemies fly through the absent ship");
    assert_ne!(g.shots[0].kind, 0, "hostile shots continue past the camera");
    assert!(g.pickups[..2].iter().all(|p| p.kind != 0));
    assert_eq!((g.shield, g.bombs, g.lives, g.score), (30, 0, 3, 0));
    assert!(g.explosions.iter().all(|e| e.ticks == 0));
    assert!(g.pop_audio().is_none());
    // Everything flies past the camera and retires behind the near plane.
    for _ in 0..40 {
        g.update_actors(&test_bank(g.segment_index()));
        g.update_shots(&test_bank(g.segment_index()));
        g.update_pickups();
    }
    assert!(g.actors.iter().all(|a| a.kind == 0));
    assert!(g.shots.iter().all(|s| s.kind == 0));
    assert!(g.pickups.iter().all(|p| p.kind == 0));
}
#[test]
fn attract_section_cuts_clear_encounters_boss_audio_and_gestures() {
    let mut g = Game::new();
    g.reset_scene(Mode::Attract, 6, 0);
    g.menu_ticks = ATTRACT_TICKS - 1;
    g.actors[0] = enemy(assets::FIGHTER, 50, 0, 500);
    g.shots[0] = Shot {
        kind: 2,
        ..Shot::EMPTY
    };
    g.pickups[0] = Pickup {
        kind: 1,
        ..Pickup::EMPTY
    };
    g.explosions[0] = Explosion {
        ticks: 10,
        ..Explosion::EMPTY
    };
    g.boss_started = true;
    g.boss_left = 0;
    g.controls.now = 0xfff0;
    g.controls.fire_armed = true;
    g.controls.shot_pending = true;
    g.controls.roll_pending = -1;
    g.controls.bomb_pending = true;
    // Seed stale audio directly: attract mode suppresses new sound events.
    g.audio[0] = AudioEvent {
        effect: Effect::EnemyFire,
        pan: 0,
    };
    g.audio_len = 1;
    let bytes = empty_segment(6);
    g.tick(
        &Segment::new(&bytes).unwrap(),
        &test_bank(g.segment_index()),
    );
    assert_eq!(g.mode, Mode::Attract);
    assert_eq!(g.stage_ticks, 0);
    assert_eq!(g.spawn_segment, 10);
    assert_eq!((g.menu_ticks, g.spawn_cursor), (0, 0));
    assert!(g.actors.iter().all(|a| a.kind == 0));
    assert!(g.shots.iter().all(|s| s.kind == 0));
    assert!(g.pickups.iter().all(|p| p.kind == 0));
    assert!(g.explosions.iter().all(|e| e.ticks == 0));
    assert!(!g.boss_started);
    assert_eq!((g.boss_left, g.boss_right, g.boss_core), (40, 40, 120));
    assert_eq!(g.controls.now, 0xfff0);
    assert_eq!(g.controls.released_at, g.controls.now);
    assert!(!g.controls.fire_armed);
    assert!(!g.controls.shot_pending);
    assert!(!g.controls.bomb_pending);
    assert_eq!(g.controls.roll_pending, 0);
    assert!(g.pop_audio().is_none());
}
#[test]
fn result_screens_do_not_enter_attract_mode() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    for mode in [Mode::Victory, Mode::GameOver] {
        let mut g = Game::new();
        g.mode = mode;
        for _ in 0..ATTRACT_TICKS * 2 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.mode, mode);
        assert_eq!(g.stage_ticks, 0);
    }
}
#[test]
fn compact_reset_matches_every_constructor_field_without_comparing_padding() {
    let mut game = running();
    game.mode = Mode::GameOver;
    game.presentation_event = PresentationEvent::Bomb;
    game.controls = Controls {
        bits: FIRE | LEFT,
        now: 14_000,
        released_at: 12_000,
        fire_at: 13_960,
        left_at: 13_980,
        right_at: 13_990,
        fire_armed: true,
        left_armed: true,
        right_armed: true,
        cached_charge: 255,
        shot_pending: true,
        shot_charge: 255,
        bomb_pending: true,
        roll_pending: -1,
        start_pending: true,
    };
    game.player = Vec3 {
        x: 90,
        y: -65,
        z: 180,
    };
    set_rail(&mut game, MISSION_TICKS);
    game.transition_ticks = 99;
    game.score = 28_000;
    game.ticks = 36_000;
    game.menu_ticks = 400;
    game.menu_activity = true;
    game.actors.fill(enemy(1, 40, 20, 700));
    game.shots.fill(Shot {
        pos: game.player,
        dx: 2,
        dy: 3,
        damage: 4,
        kind: 3,
    });
    game.missile_targets.fill(0xff);
    game.missile_rng = 0x37;
    game.pickups.fill(Pickup {
        pos: game.player,
        kind: 2,
    });
    game.explosions.fill(Explosion {
        pos: game.player,
        ticks: 10,
        color: 7,
    });
    game.audio.fill(AudioEvent {
        effect: Effect::Bomb,
        pan: -127,
    });
    game.audio_head = 7;
    game.audio_len = 8;
    game.spawn_cursor = 12;
    game.spawn_segment = 3;
    game.shield = 17;
    game.bombs = 1;
    game.bomb_kills = 4;
    game.lives = 1;
    game.shot_cooldown = 4;
    game.invulnerability = 29;
    game.hit_flash_ticks = 18;
    game.roll_ticks = 14;
    game.roll_cooldown = 33;
    game.roll_direction = -1;
    game.respawn_ticks = 12;
    game.flight.bank = -20 * 256;
    game.flight.vel = [300, -40];
    game.boss_z = 680;
    game.boss_left = 0;
    game.boss_right = 0;
    game.boss_core = 0;
    game.boss_started = true;
    game.boss_defeated = true;
    game.kills = 279;
    game.reset_in_place();
    // Derived equality visits semantic fields, including every array member;
    // raw padding bytes are deliberately excluded from this comparison.
    assert_eq!(game, Game::new());
}
#[test]
fn victory_freezes_the_run_and_consumes_inputs_through_all_four_hundred_ticks() {
    let bytes = empty_segment(assets::SEGMENT_COUNT - 1);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(assets::SEGMENT_COUNT - 1);
    let mut game = running();
    set_rail(&mut game, MISSION_TICKS - 1);
    game.boss_started = true;
    game.boss_defeated = true;
    game.boss_left = 0;
    game.boss_right = 0;
    game.boss_core = 0;
    game.player.x = 24;
    game.player.y = -32;
    game.score = 12_345;
    game.shield = 76;
    game.bombs = 2;
    game.lives = 2;
    game.menu_ticks = 749;
    game.tick(&segment, &bank);
    assert_eq!(game.mode, Mode::Victory);
    assert_eq!(game.victory_ticks(), 0);
    assert_eq!(
        (game.simulation_profile(), game.segment_index()),
        (7, assets::SEGMENT_COUNT - 1)
    );
    let frozen = (
        game.player,
        game.stage_ticks,
        game.flight,
        game.roll_ticks,
        game.boss_z,
        game.scenery_ticks(),
        game.scenery_camera_y(),
    );
    let resources = (game.score, game.shield, game.bombs, game.lives);
    while game.pop_audio().is_some() {}
    for elapsed in 1..=crate::victory::REVEAL_TICKS {
        // Alternate fire edges throughout the reveal, with a final press
        // on its very last tick. Direction and bomb gestures also expire.
        sample(
            &mut game,
            UP | RIGHT | if elapsed & 1 == 0 { FIRE } else { 0 },
            elapsed as u32 * 20,
        );
        game.tick(&segment, &bank);
        assert_eq!(game.mode, Mode::Victory);
        assert_eq!(game.hud().victory_ticks, elapsed);
        assert_eq!(game.music_state(), MusicState::Victory);
        assert_eq!((game.score, game.shield, game.bombs, game.lives), resources);
        assert_eq!(
            (
                game.player,
                game.stage_ticks,
                game.flight,
                game.roll_ticks,
                game.boss_z,
                game.scenery_ticks(),
                game.scenery_camera_y(),
            ),
            frozen
        );
        assert!(!game.controls.start_pending);
        assert!(!game.controls.shot_pending);
        assert!(!game.controls.bomb_pending);
        assert_eq!(game.controls.roll_pending, 0);
        assert!(game.pop_audio().is_none());
    }
    for elapsed in 401..=450 {
        sample(&mut game, FIRE, elapsed * 20);
        game.tick(&segment, &bank);
        assert_eq!(game.mode, Mode::Victory);
        assert_eq!(game.victory_ticks(), crate::victory::REVEAL_TICKS);
    }
    sample(&mut game, 0, 9020);
    game.tick(&segment, &bank);
    assert_eq!(game.mode, Mode::Victory);
    sample(&mut game, FIRE, 9040);
    game.tick(&segment, &bank);
    assert_eq!(game.mode, Mode::Playing);
    assert_eq!(game.simulation_profile(), 0);
    assert_eq!(game.victory_ticks(), 0);
    assert_eq!(
        (game.score, game.shield, game.bombs, game.lives),
        (0, 100, 3, 3)
    );
}
#[test]
fn victory_world_keeps_only_the_frozen_ship_and_scenery_until_blackout() {
    #[derive(Default)]
    struct Capture(std::vec::Vec<([Vec3; 3], u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }
    let bytes = empty_segment(assets::SEGMENT_COUNT - 1);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(assets::SEGMENT_COUNT - 1);
    let mut game = running();
    game.mode = Mode::Victory;
    set_rail(&mut game, MISSION_TICKS);
    game.player.x = -65;
    game.player.y = 42;
    game.flight.bank = -20 * 256;
    game.boss_started = true;
    game.actors[0] = enemy(1, 0, 0, 700);
    game.shots[0] = Shot {
        kind: 2,
        ..Shot::EMPTY
    };
    game.pickups[0] = Pickup {
        kind: 1,
        ..Pickup::EMPTY
    };
    game.explosions[0] = Explosion {
        ticks: 10,
        color: 7,
        ..Explosion::EMPTY
    };
    let mut foreground = Capture::default();
    game.render_foreground(&mut foreground, &bank, 174);
    assert_eq!(
        foreground.0.len(),
        bank.face_count(assets::PLAYER) as usize + 2
    );
    let mut scenery = Capture::default();
    game.render_scenery(&mut scenery, &segment);
    assert!(!scenery.0.is_empty());
    for elapsed in [1, 20, 50, 99] {
        game.menu_ticks = elapsed;
        game.ticks = elapsed;
        let mut actual = Capture::default();
        game.render_foreground(&mut actual, &bank, 174);
        assert_eq!(actual.0, foreground.0);
        let mut actual = Capture::default();
        game.render_scenery(&mut actual, &segment);
        assert_eq!(actual.0, scenery.0);
    }
    for elapsed in [100, 101, 399, 400] {
        game.menu_ticks = elapsed;
        let mut actual = Capture::default();
        game.render_foreground(&mut actual, &bank, 174);
        game.render_scenery(&mut actual, &segment);
        assert!(actual.0.is_empty());
    }
}
#[cfg(feature = "host-tools")]
#[test]
fn victory_host_and_native_staging_match_through_fade_reveal_and_idle() {
    use crate::render::{Renderer, BAND_PIXELS};
    let bank = test_bank(assets::SEGMENT_COUNT - 1);
    let bytes = mission_segment(assets::SEGMENT_COUNT - 1);
    let segment = Segment::new(&bytes).unwrap();
    for elapsed in [
        0, 1, 20, 40, 60, 80, 99, 100, 101, 200, 399, 400, 401, 800, 65535,
    ] {
        let game = Game::preview_victory(elapsed);
        let mut direct = Renderer::new();
        direct.begin(game.sky_color());
        game.render(&mut direct, &bank, &segment);
        direct.finish();
        let mut staged = Renderer::new();
        staged.begin(crate::scenery::sky_color(15, None));
        let raw_count = {
            let mut stage = staged.scenery_stage();
            game.stage_scenery(&mut stage, &segment);
            stage.count()
        };
        let [dx, dy] = game.sky_parallax();
        let prefix = staged.project_scenery(raw_count, dx, dy);
        game.render_foreground(&mut staged, &bank, 174.min(233 - raw_count as u16));
        staged.scenery_last(prefix);
        staged.fade_scene(game.victory_ticks());
        game.render_medal(&mut staged, &bank);
        staged.finish();
        assert_eq!((direct.dropped, staged.dropped), (0, 0), "tick {elapsed}");
        assert_eq!(
            direct.triangle_count, staged.triangle_count,
            "tick {elapsed}"
        );
        for band in 0..25 {
            let mut expected = [0; BAND_PIXELS];
            let mut actual = [0; BAND_PIXELS];
            direct.render_band(band, &mut expected);
            staged.render_band(band, &mut actual);
            assert_eq!(actual, expected, "tick {elapsed}, band {band}");
        }
    }
}
#[test]
fn swept_projectile_hits_between_endpoints() {
    let mut g = running();
    g.actors[0] = Actor {
        pos: Vec3 { x: 0, y: 0, z: 230 },
        kind: assets::FIGHTER,
        hp: 1,
        ..Actor::EMPTY
    };
    g.shots[0] = Shot {
        pos: Vec3 { x: 0, y: 0, z: 205 },
        kind: 1,
        damage: 1,
        ..Shot::EMPTY
    };
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.actors[0].kind, 0);
    assert_eq!(g.score, 100);
}
#[test]
fn joystick_vertical_axis_follows_direction_and_opposing_inputs_cancel() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let mut reached = [0i16; 3];
    for (slot, bits) in [UP, DOWN, UP | DOWN].into_iter().enumerate() {
        let mut g = running();
        sample(&mut g, bits, 20);
        for _ in 0..20 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        reached[slot] = g.player.y;
        assert_eq!(g.player.x, 0);
    }
    assert!(reached[0] > 20, "a held stick reaches cruising climb: {reached:?}");
    assert!((reached[0] + reached[1]).abs() <= 1, "mirrored axes: {reached:?}");
    assert_eq!(reached[2], 0);
}
#[test]
fn released_roll_pushes_the_ship_sideways_and_does_not_snap_back() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    for (direction, sign) in [(LEFT, -1), (RIGHT, 1)] {
        let mut g = running();
        queue_roll(&mut g, direction);
        let mut previous = 0;
        for tick in 1..=25 {
            g.tick(&segment, &test_bank(g.segment_index()));
            assert_eq!(g.roll_ticks, 26 - tick);
            // The push accelerates the ship for the whole spin.
            assert!(g.player.x * sign >= previous, "roll tick {tick}");
            previous = g.player.x * sign;
            assert!(g.flight.vel[0] * sign > 0);
        }
        assert!((30..=50).contains(&previous), "spin displacement {previous}");
        let mut settled = None;
        for _ in 0..100 {
            g.tick(&segment, &test_bank(g.segment_index()));
            assert!(g.player.x * sign >= previous, "no snap back");
            previous = g.player.x * sign;
            if g.flight.vel[0] == 0 && settled.is_none() {
                settled = Some(previous);
            }
        }
        // The coast after the spin roughly doubles the displacement, then the
        // ship rests exactly, level, without drifting.
        assert_eq!(settled, Some(previous));
        assert!((50..=70).contains(&previous), "settled displacement {previous}");
        assert_eq!((g.flight.vel, g.flight.bank, g.flight.pitch), ([0, 0], 0, 0));
        assert_eq!(g.roll_ticks, 0);
        assert_eq!(g.roll_cooldown, 0);
    }
}
#[test]
fn dodge_keeps_horizontal_vertical_and_fire_controls_live() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let mut g = running();
    queue_roll(&mut g, RIGHT);
    sample(&mut g, RIGHT | UP | FIRE, 240);
    for _ in 0..10 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    assert!(g.player.x > 10, "stick and push add up: {}", g.player.x);
    assert!(g.player.y > 5);
    let shots: std::vec::Vec<_> = g.shots.iter().filter(|shot| shot.kind == 1).collect();
    assert!(shots.len() >= 2);
    assert!(shots.iter().all(|shot| shot.pos.x >= 0 && shot.pos.x <= g.player.x));
    assert!(shots.iter().any(|shot| shot.pos.x > 0), "fire uses the displaced position");
    assert!(g.flight.bank < 0, "banked into the turn");
    assert!(g.flight.pitch < 0, "nose up while climbing");
    let (x, vx) = (g.player.x, g.flight.vel[0]);

    sample(&mut g, LEFT | DOWN | FIRE, 460);
    for _ in 0..10 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    assert_eq!(g.roll_ticks, 6);
    assert!(g.player.x > x, "momentum carries the ship on");
    assert!(g.flight.vel[0] < vx, "opposite steering brakes the push");
    assert!(g.flight.vel[1] < 0, "the vertical axis reverses");
    assert!(g.flight.bank > 0, "banks against the braking");
    assert!(g.flight.pitch > 0);
}
#[test]
fn a_right_roll_spins_the_same_way_as_a_right_bank() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    for (direction, sign) in [(LEFT, -1i16), (RIGHT, 1)] {
        let mut g = running();
        queue_roll(&mut g, direction);
        for _ in 0..5 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        // Negative mesh angles lower the right wing: a right roll's spin and
        // its bank into the push both read negative, a left roll's positive.
        let angle = g.player_appearance() as u8 as i8 as i16;
        assert!(angle * sign < 0, "roll angle {angle} for direction {sign}");
        assert!((g.flight.bank >> 8) * sign < 0);
    }
}
#[test]
fn dodge_and_steering_share_the_arena_limits() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    for (direction, sign) in [(LEFT, -1), (RIGHT, 1)] {
        let mut g = running();
        g.player.x = sign * 99;
        g.player.y = 64;
        queue_roll(&mut g, direction);
        sample(&mut g, direction | UP, 240);
        for _ in 0..60 {
            g.tick(&segment, &test_bank(g.segment_index()));
            assert!(g.player.x * sign <= 100 && g.player.y <= 65);
        }
        // The soft wall settles the ship exactly on the former hard limits.
        assert_eq!((g.player.x, g.player.y), (sign * 100, 65));
        assert_eq!(g.flight.frac, [0, 0]);
        assert_eq!(g.flight.vel, [0, 0]);
    }
}
#[test]
fn flight_state_outside_the_arena_returns_inside_without_a_jump() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let mut g = running();
    g.player.x = 100;
    g.player.y = -66;
    g.flight.frac = [200, 90];
    for _ in 0..80 {
        g.tick(&segment, &test_bank(g.segment_index()));
        assert!(g.player.x <= 100 && g.player.y >= -66);
        assert!(g.flight.vel[0] <= 0 && g.flight.vel[1] >= 0);
    }
    // The wall pushes the ship back in and its momentum coasts a unit further.
    assert!((98..=100).contains(&g.player.x), "{}", g.player.x);
    assert!((-65..=-63).contains(&g.player.y), "{}", g.player.y);
    assert_eq!(g.flight.vel, [0, 0]);
}
#[test]
fn sidestep_avoids_old_lane_scenery_but_can_hit_the_new_lane() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    // Where the push has carried the ship when the rock arrives.
    let mut probe = running();
    queue_roll(&mut probe, RIGHT);
    for _ in 0..30 {
        probe.tick(&segment, &test_bank(probe.segment_index()));
    }
    let lane = probe.player.x;
    assert!(lane > 35, "the sidestep clears a rock's radius: {lane}");
    for (rolling, obstacle_x, expected_shield) in [(false, 0, 70), (true, 0, 100), (true, lane, 70)] {
        let mut g = running();
        g.actors[0] = Actor {
            pos: Vec3 {
                x: obstacle_x,
                y: -20,
                z: PLAYER_Z + SPEED * 30 + 31,
            },
            kind: assets::ROCK,
            hp: 255,
            ..Actor::EMPTY
        };
        if rolling {
            queue_roll(&mut g, RIGHT);
        }
        for _ in 0..30 {
            g.tick(&segment, &test_bank(g.segment_index()));
        }
        assert_eq!(g.shield, expected_shield);
    }
}
#[test]
fn roll_deflects_projectiles_but_not_scenery() {
    let mut g = running();
    g.roll_ticks = 10;
    g.shots[0] = Shot {
        pos: Vec3 { x: 0, y: 0, z: 190 },
        kind: 2,
        damage: 12,
        ..Shot::EMPTY
    };
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.shield, 100);
    g.actors[0] = Actor {
        pos: Vec3 { x: 0, y: 0, z: 190 },
        kind: assets::ROCK,
        hp: 255,
        ..Actor::EMPTY
    };
    g.update_actors(&test_bank(g.segment_index()));
    assert_eq!(g.shield, 70);
}
#[test]
fn respawn_uses_section_checkpoint_and_retains_score() {
    let mut g = running();
    set_rail(&mut g, 8000);
    g.score = 1234;
    g.damage_player(100);
    assert_eq!(g.mode, Mode::Respawning);
    assert_eq!(g.lives, 2);
    let bytes = empty_segment(2);
    let segment = Segment::new(&bytes).unwrap();
    for _ in 0..75 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    assert_eq!(g.mode, Mode::Playing);
    assert_eq!((g.hud().section, g.hud().distance), (2, 0));
    assert_eq!(g.shield, 100);
    assert_eq!(g.score, 1234);
}
#[test]
fn carrier_core_is_protected_until_both_turrets_die() {
    let mut g = running();
    g.boss_started = true;
    g.boss_z = 680;
    g.shots[0] = Shot {
        pos: Vec3 { x: 0, y: 0, z: 450 },
        kind: 1,
        damage: 4,
        ..Shot::EMPTY
    };
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.boss_core, 120);
    g.boss_left = 0;
    g.boss_right = 0;
    g.shots[0] = Shot {
        pos: Vec3 { x: 0, y: 0, z: 450 },
        kind: 1,
        damage: 4,
        ..Shot::EMPTY
    };
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.boss_core, 116);
}
#[test]
fn shield_magnet_recovers_near_misses_and_rewards_once() {
    for (x, y) in [(56, -56), (-56, 56)] {
        for (initial_shield, expected_shield) in [(10, 70), (40, 100), (90, 100), (100, 100)] {
            let mut g = running();
            g.shield = initial_shield;
            g.pickups[0] = Pickup {
                pos: Vec3 {
                    x,
                    y,
                    z: PLAYER_Z + 240 + SPEED,
                },
                kind: 1,
            };
            for _ in 0..10 {
                let before = g.pickups[0].pos;
                g.update_pickups();
                let after = g.pickups[0].pos;
                assert!(after.x.abs() < before.x.abs());
                assert!(after.y.abs() < before.y.abs());
                assert_eq!(after.z, before.z - SPEED);
                assert_eq!(g.pickups[0].kind, 1);
                assert_eq!((g.shield, g.score), (initial_shield, 0));
                assert!(g.pop_audio().is_none());
            }
            for _ in 0..30 {
                g.update_pickups();
            }
            assert_eq!(g.pickups[0].kind, 0);
            assert_eq!((g.shield, g.score), (expected_shield, 50));
            assert_eq!(g.pop_audio().unwrap().effect, Effect::Pickup);
            assert!(g.pop_audio().is_none());
            g.update_pickups();
            assert_eq!((g.shield, g.score), (expected_shield, 50));
            assert!(g.pop_audio().is_none());
        }
    }
}

#[test]
fn pickup_magnet_respects_proximity_and_mode() {
    // Depths describe the pickup after its normal forward movement.
    for (kind, mode, x, y, depth, attracted) in [
        (1, Mode::Playing, 64, -64, 240, true),
        (1, Mode::Playing, -64, 64, 0, true),
        (1, Mode::Playing, 65, 0, 120, false),
        (1, Mode::Playing, 0, -65, 120, false),
        (1, Mode::Playing, 56, -56, 241, false),
        (1, Mode::Playing, 56, -56, -1, false),
        (2, Mode::Playing, 56, -56, 120, true),
        (2, Mode::Playing, -64, 64, 240, true),
        (2, Mode::Playing, 65, 0, 120, false),
        (2, Mode::Playing, 0, -65, 120, false),
        (2, Mode::Playing, 56, -56, 241, false),
        (2, Mode::Playing, 56, -56, -1, false),
        (1, Mode::Attract, 56, -56, 120, false),
        (2, Mode::Attract, 56, -56, 120, false),
    ] {
        let mut g = running();
        g.mode = mode;
        g.player.x = 20;
        g.player.y = -10;
        let before = Vec3 {
            x: g.player.x + x,
            y: g.player.y + y,
            z: PLAYER_Z + depth + SPEED,
        };
        g.pickups[0] = Pickup { pos: before, kind };
        g.update_pickups();
        let after = g.pickups[0].pos;
        assert_eq!(after.z, PLAYER_Z + depth);
        if attracted {
            assert!((after.x - g.player.x).abs() < x.abs());
            assert!((after.y - g.player.y).abs() < y.abs());
        } else {
            assert_eq!((after.x, after.y), (before.x, before.y));
        }
        assert_eq!(g.pickups[0].kind, kind);
        assert_eq!(g.score, 0);
    }
}

#[test]
fn bomb_pickups_follow_the_same_pull_and_award_once_at_the_inventory_cap() {
    for bombs in [0, 2, 3] {
        let mut game = running();
        game.bombs = bombs;
        game.shield = 17;
        game.pickups[0] = Pickup {
            pos: Vec3 {
                x: -56,
                y: 56,
                z: PLAYER_Z + 240 + SPEED,
            },
            kind: 2,
        };
        for _ in 0..40 {
            game.update_pickups();
        }
        assert_eq!(game.pickups[0].kind, 0);
        assert_eq!(
            (game.bombs, game.shield, game.score),
            ((bombs + 1).min(3), 17, 50)
        );
        assert_eq!(game.pop_audio().unwrap().effect, Effect::Pickup);
        game.update_pickups();
        assert!(game.pop_audio().is_none());
        assert_eq!(game.score, 50);
    }
}

#[test]
fn pickup_magnet_does_not_overshoot_or_expand_collection_hitbox() {
    for kind in [1, 2] {
        let mut g = running();
        g.shield = 40;
        g.pickups[0] = Pickup {
            pos: Vec3 {
                x: 1,
                y: -1,
                z: PLAYER_Z + 80 + SPEED,
            },
            kind,
        };
        for _ in 0..2 {
            g.update_pickups();
            assert_eq!((g.pickups[0].pos.x, g.pickups[0].pos.y), (0, 0));
            assert_eq!(g.pickups[0].kind, kind);
        }
        for (x, y) in [(31, 0), (-31, 0), (0, 31), (0, -31)] {
            g.pickups[0] = Pickup {
                pos: Vec3 {
                    x,
                    y,
                    z: PLAYER_Z + 8 + SPEED,
                },
                kind,
            };
            g.update_pickups();
            assert_eq!(g.pickups[0].kind, kind, "29 units is outside the hitbox");
            g.update_pickups();
            assert_eq!(g.pickups[0].kind, 0, "27 units is inside the hitbox");
        }
        g.pickups[0] = Pickup {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: PLAYER_Z + 25 + SPEED,
            },
            kind,
        };
        g.update_pickups();
        assert_eq!(g.pickups[0].kind, kind, "depth 25 is outside the hitbox");
        g.update_pickups();
        assert_eq!(g.pickups[0].kind, 0, "depth 17 is inside the hitbox");
    }
}

#[test]
fn pickups_cap_and_bombs_clear_hostile_shots() {
    let mut g = running();
    g.pickups[0] = Pickup {
        pos: Vec3 {
            x: 0,
            y: 0,
            z: PLAYER_Z + 8,
        },
        kind: 2,
    };
    g.update_pickups();
    assert_eq!(g.bombs, 3);
    g.shots[0] = Shot {
        kind: 2,
        ..Shot::EMPTY
    };
    g.bomb(&test_bank(g.segment_index()));
    assert_eq!(g.bombs, 2);
    assert_eq!(g.shots[0].kind, 0);
}
#[test]
fn bomb_presentation_survives_rewarded_inventory_full_audio_and_catch_up() {
    let mut g = running();
    for actor in &mut g.actors[..5] {
        *actor = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
    }
    for _ in 0..AUDIO_CAPACITY {
        g.sound(Effect::Fire, 0);
    }
    g.controls.bomb_pending = true;
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    g.tick(&segment, &test_bank(g.segment_index()));
    assert_eq!(g.bombs, 3, "the reward hides the inventory decrement");
    assert_eq!(g.audio_len as usize, AUDIO_CAPACITY);
    while g.pop_audio().is_some() {}
    for _ in 0..50 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
}
#[test]
fn killing_the_gate_guardian_keeps_bomb_presentation_on_the_victory_screen() {
    let mut g = running();
    set_rail(&mut g, MISSION_TICKS - 1);
    g.boss_started = true;
    g.boss_left = 0;
    g.boss_right = 0;
    g.boss_core = 8;
    g.controls.bomb_pending = true;
    let bytes = empty_segment(assets::SEGMENT_COUNT - 1);
    let segment = Segment::new(&bytes).unwrap();
    g.tick(&segment, &test_bank(g.segment_index()));
    assert_eq!(g.mode, Mode::Victory);
    assert!(g.boss_defeated);
    assert_eq!((g.shield, g.bombs), (100, 3));
    for _ in 0..50 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
}
#[test]
fn presentation_resets_and_bombs_keep_only_the_latest_request() {
    let mut g = Game::new();
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    g.reset_run();
    assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    g.bomb(&test_bank(g.segment_index()));
    g.reset_scene(Mode::Title, 0, 0);
    assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
    g.reset_run();
    g.bomb(&test_bank(g.segment_index()));
    assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
    g.bomb(&test_bank(g.segment_index()));
    g.bomb(&test_bank(g.segment_index()));
    assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    g.reset_run();
    g.bomb(&test_bank(g.segment_index()));
    g.respawn();
    assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
}
#[test]
fn player_damage_requests_a_hit_impulse_that_bombs_outrank_and_resets_replace() {
    let mut g = running();
    // The run reset itself requests the usual Reset first.
    assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
    g.invulnerability = 0;
    g.damage_player(10);
    assert_eq!(g.shield, 90);
    assert_eq!(g.take_presentation_event(), PresentationEvent::Hit);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    // Immune hits request nothing.
    g.damage_player(10);
    assert_eq!(g.shield, 90);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
    // A bomb in the same catch-up window keeps its bigger shake.
    g.bomb(&test_bank(g.segment_index()));
    g.invulnerability = 0;
    g.damage_player(10);
    assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
    // A hit followed by a bomb reports the bomb; a reset replaces either.
    g.invulnerability = 0;
    g.damage_player(10);
    g.bomb(&test_bank(g.segment_index()));
    assert_eq!(g.take_presentation_event(), PresentationEvent::Bomb);
    g.invulnerability = 0;
    g.damage_player(10);
    g.respawn();
    assert_eq!(g.take_presentation_event(), PresentationEvent::Reset);
    // Outside play nothing is requested.
    g.reset_scene(Mode::Title, 0, 0);
    let _ = g.take_presentation_event();
    g.invulnerability = 0;
    g.damage_player(10);
    assert_eq!(g.take_presentation_event(), PresentationEvent::None);
}
#[test]
fn every_fifth_projectile_kill_adds_a_bomb_up_to_three_without_banking_overflow() {
    let mut g = running();
    g.bombs = 0;
    for kill in 1..=25 {
        if kill == 21 {
            // The reward at 20 was consumed at the cap, not deferred.
            g.bombs = 2;
        }
        g.actors[0] = Actor {
            pos: Vec3 { x: 0, y: 0, z: 210 },
            kind: assets::FIGHTER,
            hp: 1,
            flags: u8::from(kill == 5),
            ..Actor::EMPTY
        };
        g.shots[0] = Shot {
            pos: Vec3 { x: 0, y: 0, z: 180 },
            damage: 1,
            kind: if kill % 2 == 0 { 3 } else { 1 },
            ..Shot::EMPTY
        };
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.actors[0].kind, 0);
        assert_eq!(g.shots[0].kind, 0);
        assert_eq!(g.kills, kill);
        let expected = if (21..25).contains(&kill) {
            2
        } else {
            (kill / 5).min(3) as u8
        };
        assert_eq!(g.hud().bombs, expected, "kill {kill}");
        if kill == 8 {
            assert_eq!(g.pickups.iter().filter(|p| p.kind == 1).count(), 1);
        }
        if kill == 5 {
            assert_eq!(g.pickups.iter().filter(|p| p.kind == 2).count(), 1);
        }
        // A spent projectile cannot award the same kill twice.
        g.update_shots(&test_bank(g.segment_index()));
        assert_eq!(g.kills, kill);
        assert_eq!(g.bombs, expected);
    }
}
#[test]
fn shields_drop_every_eighth_enemy_kill_with_existing_priority_and_pool_rules() {
    let mut game = running();
    let bank = test_bank(0);
    for kill in 1..=24 {
        game.pickups.fill(Pickup::EMPTY);
        game.actors[0] = enemy(assets::FIGHTER, 0, 0, 900);
        game.kill_actor(0, &bank);
        assert_eq!(
            game.pickups.iter().filter(|p| p.kind == 1).count(),
            usize::from(kill % 8 == 0)
        );
    }
    // A marked bomb drop takes precedence on the same eighth-kill milestone.
    game.kills = 31;
    game.pickups.fill(Pickup::EMPTY);
    game.actors[0] = enemy(assets::FIGHTER, 0, 0, 900);
    game.actors[0].flags = 1;
    game.kill_actor(0, &bank);
    assert_eq!(game.pickups[0].kind, 2);
    assert!(game.pickups.iter().all(|p| p.kind != 1));
    game.kills = u16::MAX;
    game.pickups.fill(Pickup::EMPTY);
    game.actors[0] = enemy(assets::FIGHTER, 0, 0, 900);
    game.kill_actor(0, &bank);
    assert_eq!((game.kills, game.pickups[0].kind), (0, 1));
    // Neither a full pool nor the total-kill wrap defers a milestone.
    game.kills = u16::MAX;
    game.pickups.fill(Pickup {
        kind: 2,
        ..Pickup::EMPTY
    });
    game.actors[0] = enemy(assets::FIGHTER, 0, 0, 900);
    game.kill_actor(0, &bank);
    assert_eq!(game.kills, 0);
    game.pickups.fill(Pickup::EMPTY);
    for kill in 1..=8 {
        game.actors[0] = enemy(assets::FIGHTER, 0, 0, 900);
        game.kill_actor(0, &bank);
        assert_eq!(
            game.pickups.iter().filter(|p| p.kind == 1).count(),
            usize::from(kill == 8)
        );
    }
}

#[test]
fn bomb_multi_kills_award_each_crossed_five_kill_milestone() {
    let mut g = running();
    g.bombs = 1;
    for actor in &mut g.actors[..10] {
        *actor = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
    }
    g.actors[10] = Actor {
        kind: assets::ROCK,
        hp: 255,
        ..Actor::EMPTY
    };
    g.actors[11] = Actor {
        kind: assets::TURRET,
        hp: 9,
        ..Actor::EMPTY
    };
    g.bomb(&test_bank(g.segment_index()));
    assert_eq!(g.kills, 10);
    assert_eq!(g.bombs, 2, "spend one bomb and immediately earn two");
    assert_eq!(g.actors[10].hp, 255);
    assert_eq!(g.actors[11].hp, 1, "damage alone is not a kill");
}
#[test]
fn bomb_reward_progress_survives_respawn_and_kill_total_wrap_but_resets_with_run() {
    let mut g = running();
    for _ in 0..4 {
        g.actors[0] = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
        g.kill_actor(0, &test_bank(g.segment_index()));
    }
    g.bombs = 0;
    g.damage_player(100);
    assert_eq!(g.mode, Mode::Respawning);
    g.respawn();
    assert_eq!(g.bombs, 1);
    g.actors[0] = Actor {
        kind: assets::FIGHTER,
        hp: 1,
        ..Actor::EMPTY
    };
    g.kill_actor(0, &test_bank(g.segment_index()));
    assert_eq!(
        g.bombs, 2,
        "the fifth kill still completes the pre-death progress"
    );

    g.kills = u16::MAX - 1;
    g.bombs = 0;
    for kill in 1..=5 {
        g.actors[0] = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
        g.kill_actor(0, &test_bank(g.segment_index()));
        assert_eq!(g.bombs, u8::from(kill == 5));
    }
    assert_eq!(g.kills, 3);

    for _ in 0..4 {
        g.actors[0] = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
        g.kill_actor(0, &test_bank(g.segment_index()));
    }
    g.reset_run();
    assert_eq!((g.kills, g.bombs, g.bomb_kills), (0, 3, 0));
    g.bombs = 0;
    for kill in 1..=5 {
        g.actors[0] = Actor {
            kind: assets::FIGHTER,
            hp: 1,
            ..Actor::EMPTY
        };
        g.kill_actor(0, &test_bank(g.segment_index()));
        assert_eq!(g.bombs, u8::from(kill == 5));
    }
}
#[test]
fn paused_gestures_do_not_trigger_actions_after_neutral_resume() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    for direction in [LEFT, RIGHT] {
        let mut g = running();
        let player = g.player;
        // Also discard queued actions captured immediately before the pause.
        sample(&mut g, FIRE | direction, 60);
        sample(&mut g, 0, 80);
        sample(&mut g, FIRE | direction, 100);
        assert!(g.controls.bomb_pending);
        assert_ne!(g.controls.roll_pending, 0);
        for (bits, ms) in [
            (0, 120),
            (FIRE | direction, 200),
            (0, 220),
            (FIRE | direction, 300),
            (0, 320),
            (0, 3000),
        ] {
            g.sample_paused_input(bits, ms);
        }
        sample(&mut g, 0, 3020);
        g.tick(&segment, &bank);
        assert!(g.shots.iter().all(|shot| shot.kind == 0));
        assert_eq!((g.bombs, g.roll_ticks, g.roll_cooldown), (3, 0, 0));
        assert_eq!(g.player, player);
        assert_eq!(g.audio_len, 0);
    }
    // Fire while paused must not become a delayed title-screen start.
    let mut title = Game::new();
    title.sample_paused_input(FIRE, 100);
    title.sample_paused_input(0, 120);
    sample(&mut title, 0, 3000);
    title.tick(&segment, &bank);
    assert_eq!(title.mode, Mode::Title);
    assert!(!title.menu_activity);
}

#[test]
fn taps_cannot_bridge_pause_and_later_double_taps_work() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    for bits in [FIRE, LEFT, RIGHT] {
        for first_tap_paused in [false, true] {
            let mut g = running();
            if first_tap_paused {
                g.sample_paused_input(bits, 100);
            } else {
                sample(&mut g, bits, 100);
                g.tick(&segment, &bank);
            }
            g.sample_paused_input(0, 120);
            sample(&mut g, bits, 200);
            g.tick(&segment, &bank);
            assert_eq!(g.bombs, 3, "a fire tap cannot pair across pause");
            assert_eq!(g.roll_ticks, 0, "a direction tap cannot pair across pause");
            sample(&mut g, 0, 220);
            sample(&mut g, bits, 300);
            g.tick(&segment, &bank);
            if bits == FIRE {
                assert_eq!(g.bombs, 2, "two resumed fire taps still drop a bomb");
            } else {
                assert_eq!(g.roll_ticks, 25);
                assert_eq!(g.roll_direction, if bits == LEFT { -1 } else { 1 });
            }
        }
    }
}

#[test]
fn held_steering_and_autofire_resume_without_replaying_a_paused_edge() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    let mut g = running();
    g.sample_paused_input(0, 2000);
    assert_eq!(g.hud().charge, 255);
    g.sample_paused_input(FIRE | RIGHT | UP, 2020);
    g.sample_paused_input(FIRE | RIGHT | UP, 3000);
    sample(&mut g, FIRE | RIGHT | UP, 3020);
    g.tick(&segment, &bank);
    assert!(g.flight.vel[0] > 0 && g.flight.vel[1] > 0);
    assert_eq!(g.shots.iter().filter(|shot| shot.kind != 0).count(), 1);
    assert_eq!((g.shots[0].kind, g.shots[0].damage), (1, 1));
    assert_eq!((g.bombs, g.roll_ticks, g.hud().charge), (3, 0, 0));
}

#[test]
fn paused_charge_tracks_release_through_clock_wrap_and_fires_after_resume() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let bank = test_bank(0);
    let mut g = running();
    let release = u16::MAX as u32 - 1490;
    g.sample_paused_input(FIRE, (release - 20) as u16);
    g.sample_paused_input(0, release as u16);
    for elapsed in (20..=70_000u32).step_by(20) {
        g.sample_paused_input(0, (release + elapsed) as u16);
        let expected = if elapsed <= 1000 {
            0
        } else if elapsed >= 2000 {
            255
        } else {
            ((elapsed - 1000) * 51 / 200) as u8
        };
        assert_eq!(g.hud().charge, expected, "paused for {elapsed} ms");
    }
    sample(&mut g, FIRE, release + 70_020);
    g.tick(&segment, &bank);
    assert_eq!(g.shots.iter().filter(|shot| shot.kind != 0).count(), 1);
    assert_eq!((g.shots[0].kind, g.shots[0].damage), (3, 4));
    assert_eq!(g.bombs, 3);
}

#[test]
fn charge_uses_elapsed_milliseconds_and_wraps_safely() {
    let mut g = running();
    sample(&mut g, FIRE, u16::MAX as u32 - 1500);
    sample(&mut g, 0, u16::MAX as u32 - 1490);
    sample(&mut g, 0, 509);
    assert_eq!(g.hud().charge, 255);
    sample(&mut g, FIRE, 510);
    assert_eq!(g.controls.shot_charge, 255);
}
#[test]
fn remembered_instants_survive_the_clock_wrapping_many_times() {
    // Twenty minutes of idle sampling (over eighteen clock wraps): the charge
    // stays saturated and a single old tap never pairs with a new one.
    let mut g = running();
    sample(&mut g, FIRE, 100);
    sample(&mut g, 0, 120);
    sample(&mut g, RIGHT, 300);
    sample(&mut g, 0, 320);
    let mut ms = 320u32;
    while ms < 20 * 60 * 1000 {
        ms += 100;
        sample(&mut g, 0, ms);
        let elapsed = ms - 120;
        let expected = if elapsed <= 1000 {
            0
        } else if elapsed >= 2000 {
            255
        } else {
            ((elapsed - 1000) * 51 / 200) as u8
        };
        assert_eq!(g.hud().charge, expected, "at {ms}");
    }
    assert!(!g.controls.right_armed && !g.controls.fire_armed);
    sample(&mut g, RIGHT, ms + 20);
    assert_eq!(g.controls.roll_pending, 0, "a tap twenty minutes later is a first tap");
    sample(&mut g, 0, ms + 40);
    sample(&mut g, RIGHT, ms + 60);
    assert_eq!(g.controls.roll_pending, 1, "the second tap within the window rolls");
    sample(&mut g, FIRE, ms + 80);
    assert!(!g.controls.bomb_pending, "an ancient fire tap is not half of a double tap");
    assert_eq!(g.controls.shot_charge, 255);
}
#[test]
fn three_deaths_reach_game_over_and_fresh_press_restarts() {
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let mut g = running();
    for life in (0..3).rev() {
        g.invulnerability = 0;
        g.damage_player(100);
        assert_eq!(g.lives, life);
        if life > 0 {
            for _ in 0..75 {
                g.tick(&segment, &test_bank(g.segment_index()));
            }
        }
    }
    assert_eq!(g.mode, Mode::GameOver);
    g.controls.bits = FIRE;
    sample(&mut g, FIRE, 5000);
    g.tick(&segment, &test_bank(g.segment_index()));
    assert_eq!(g.mode, Mode::GameOver);
    sample(&mut g, 0, 5020);
    sample(&mut g, FIRE, 5040);
    g.tick(&segment, &test_bank(g.segment_index()));
    assert_eq!(g.mode, Mode::Playing);
    assert_eq!(g.lives, 3);
    assert_eq!(g.score, 0);
    assert_eq!(g.hud().distance, 0);
}
#[test]
fn partial_charge_scales_damage_and_full_charge_deals_four() {
    let mut g = running();
    g.fire_player(0);
    g.fire_player(127);
    g.fire_player(254);
    g.fire_player(255);
    assert_eq!(
        [
            g.shots[0].damage,
            g.shots[1].damage,
            g.shots[2].damage,
            g.shots[3].damage
        ],
        [1, 2, 3, 4]
    );
    assert_eq!(
        [
            g.shots[0].kind,
            g.shots[1].kind,
            g.shots[2].kind,
            g.shots[3].kind
        ],
        [1, 1, 1, 3]
    );
    g.update_shots(&test_bank(g.segment_index()));
    assert!(g.shots[..3]
        .iter()
        .all(|shot| shot.pos.z == PLAYER_Z + 18 + 48));
    assert_eq!(g.shots[3].pos.z, PLAYER_Z + 18 + 32);
}
#[test]
fn missile_launch_is_retained_when_the_game_audio_queue_is_full() {
    let mut g = running();
    while g.pop_audio().is_some() {}
    for _ in 0..AUDIO_CAPACITY {
        g.sound(Effect::EnemyFire, 0);
    }
    g.fire_player(255);
    for _ in 0..AUDIO_CAPACITY - 1 {
        assert_eq!(g.pop_audio().unwrap().effect, Effect::EnemyFire);
    }
    assert_eq!(
        g.pop_audio(),
        Some(AudioEvent {
            effect: Effect::MissileLaunch,
            pan: 0,
        })
    );
    assert!(g.pop_audio().is_none());
}
fn enemy(kind: u8, x: i16, y: i16, z: i16) -> Actor {
    Actor {
        pos: Vec3 { x, y, z },
        origin_x: x,
        origin_y: y,
        kind,
        hp: if kind == assets::TURRET { 6 } else { 3 },
        approach_speed: if kind == assets::DIVER { 12 } else { 8 },
        ..Actor::EMPTY
    }
}
#[test]
fn missile_random_acquisition_covers_every_candidate_count_without_bias() {
    let origin = Vec3 { x: 0, y: 0, z: 200 };
    let bank = test_bank(0);
    // The core is exposed only after both turrets die, so fourteen is the
    // largest possible candidate set: twelve actors and two boss turrets.
    for count in 0..=ACTORS + 2 {
        let mut g = running();
        for index in 0..count.min(ACTORS) {
            g.actors[index] = enemy(assets::FIGHTER, index as i16 * 10, 0, 900);
        }
        g.boss_started = count > ACTORS;
        g.boss_left = u8::from(count > ACTORS);
        g.boss_right = u8::from(count > ACTORS + 1);
        g.boss_core = 0;
        let seed = g.missile_rng;
        if count == 0 {
            let mut lock = 15;
            assert_eq!(g.missile_target(origin, &mut lock, &bank), None);
            assert_eq!((lock, g.missile_rng), (0, seed));
            continue;
        }
        let mut selections = [0; ACTORS + 3];
        for _ in 0..16 * count {
            let mut lock = 0;
            let (target, _) = g.missile_target(origin, &mut lock, &bank).unwrap();
            assert!((1..=count as u8).contains(&lock));
            selections[lock as usize - 1] += 1;
            if lock as usize <= ACTORS {
                assert_eq!(
                    target,
                    actor_center_sim(&g.actors[lock as usize - 1], &bank)
                );
            } else {
                assert_eq!(target.x, if lock == 13 { -130 } else { 130 });
            }
        }
        assert!(selections[..count].iter().all(|selected| *selected == 16));
        if count == 1 {
            assert_eq!(g.missile_rng, seed, "one candidate needs no random draw");
        }
    }
}
#[test]
fn missile_random_acquisition_is_reproducible_for_every_nonzero_seed() {
    let origin = Vec3 { x: 0, y: 0, z: 200 };
    let bank = test_bank(0);
    let mut seen = [false; 3];
    for seed in 1..=u8::MAX {
        let mut g = running();
        for index in 0..3 {
            g.actors[index] = enemy(assets::FIGHTER, index as i16 * 30, 0, 700);
        }
        g.missile_rng = seed;
        let mut replay = running();
        replay.actors = g.actors;
        replay.missile_rng = seed;
        let mut lock = 0;
        let mut replay_lock = 0;
        for _ in 0..12 {
            let target = g.missile_target(origin, &mut lock, &bank);
            assert_eq!(
                target,
                replay.missile_target(origin, &mut replay_lock, &bank)
            );
            assert_eq!((lock, g.missile_rng), (replay_lock, replay.missile_rng));
            assert!((1..=3).contains(&lock));
            seen[lock as usize - 1] = true;
            lock = 0;
            replay_lock = 0;
        }
    }
    assert_eq!(seen, [true; 3]);
}
#[test]
fn missile_keeps_its_random_lock_when_closer_targets_appear() {
    let mut g = running();
    let bank = test_bank(0);
    let origin = Vec3 { x: 0, y: 0, z: 200 };
    g.actors[0] = enemy(assets::FIGHTER, 0, 0, 300);
    g.actors[1] = enemy(assets::DIVER, -80, 0, 1600);
    g.actors[2] = enemy(assets::FIGHTER, 40, 0, 700);
    let mut lock = 0;
    assert_eq!(
        g.missile_target(origin, &mut lock, &bank),
        Some((g.actors[1].pos, 44))
    );
    assert_eq!((lock, g.missile_rng), (2, 0x26));
    g.actors[0].pos.z = 201;
    g.actors[2].pos.x = 0;
    g.actors[1].pos = Vec3 {
        x: 70,
        y: -20,
        z: 1500,
    };
    g.actors[1].approach_speed = 14;
    assert_eq!(
        g.missile_target(origin, &mut lock, &bank),
        Some((g.actors[1].pos, 46))
    );
    assert_eq!(
        (lock, g.missile_rng),
        (2, 0x26),
        "a valid lock consumes no randomness"
    );
}
#[test]
fn missile_candidates_exclude_empty_dead_passed_and_indestructible_actors() {
    let mut g = Game::preview_at_for_test(12);
    let bank = test_bank(12);
    let origin = Vec3 { x: 0, y: 0, z: 200 };
    g.actors[0] = enemy(assets::ROCK, 0, 0, 300);
    g.actors[0].hp = 255;
    g.actors[1] = enemy(assets::BUILDING, 0, 0, 300);
    g.actors[1].hp = 255;
    g.actors[2] = enemy(assets::FIGHTER, 0, 0, 199);
    g.actors[3] = enemy(assets::FIGHTER, 0, 0, 200);
    g.actors[4] = enemy(assets::FIGHTER, 0, 0, 400);
    g.actors[4].hp = 0;
    g.actors[5] = enemy(0, 0, 0, 500);
    g.actors[5].flags = assets::SMALL_DEBRIS;
    g.actors[6] = enemy(assets::ROCK, 70, -10, 800);
    g.actors[6].flags = assets::SMALL_DEBRIS;
    g.actors[6].hp = 2;
    for invalid_lock in 0..=6 {
        let mut lock = invalid_lock;
        assert_eq!(
            g.missile_target(origin, &mut lock, &bank),
            Some((
                Vec3 {
                    x: 70,
                    y: 0,
                    z: 800
                },
                40
            ))
        );
        assert_eq!(lock, 7);
        assert_eq!(g.missile_rng, 0xA5);
    }
}
#[test]
fn missile_reacquires_after_death_slot_reuse_and_passing_and_coasts_without_targets() {
    let mut g = running();
    let mut s = Shot {
        pos: Vec3 { x: 0, y: 0, z: 200 },
        kind: 3,
        damage: 4,
        ..Shot::EMPTY
    };
    let mut lock = 0;
    g.actors[0] = enemy(assets::FIGHTER, 100, 0, 700);
    g.steer_missile(&mut s, &mut lock, &test_bank(g.segment_index()));
    assert_eq!(s.dx, 1);
    assert_eq!(lock, 1);
    g.actors[0].kind = 0;
    g.steer_missile(&mut s, &mut lock, &test_bank(g.segment_index()));
    assert_eq!(lock, 0);
    assert_eq!(
        s.dx, 0,
        "lost target reduces steering without stale references"
    );
    g.actors[0] = enemy(assets::FIGHTER, -100, 0, 700);
    g.steer_missile(&mut s, &mut lock, &test_bank(g.segment_index()));
    assert_eq!(lock, 1);
    assert_eq!(s.dx, -1, "a reused slot is considered at its new position");
    g.actors[0].pos.z = 199;
    g.steer_missile(&mut s, &mut lock, &test_bank(g.segment_index()));
    assert_eq!(lock, 0);
    assert_eq!(
        s.dx, 0,
        "the seeker cannot turn back toward a passed target"
    );
    g.shots[0] = s;
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.shots[0].pos, Vec3 { x: 0, y: 0, z: 232 });
    for _ in 0..70 {
        g.update_shots(&test_bank(g.segment_index()));
    }
    assert_eq!(
        g.shots[0].kind, 0,
        "unguided missiles expire at the existing range limit"
    );
    assert_eq!(g.missile_lock(0), 0);
}
#[test]
fn missiles_acquire_on_first_update_and_can_share_the_same_target() {
    let mut g = running();
    let bank = test_bank(0);
    g.actors[0] = enemy(assets::FIGHTER, 70, 0, 1800);
    g.fire_player(255);
    assert_eq!(g.missile_lock(0), 0);
    assert_eq!(g.missile_rng, 0xA5);
    g.update_shots(&bank);
    assert_eq!(g.missile_lock(0), 1);
    g.fire_player(255);
    assert_eq!((g.missile_lock(0), g.missile_lock(1)), (1, 0));
    g.update_shots(&bank);
    assert_eq!((g.missile_lock(0), g.missile_lock(1)), (1, 1));
    assert_eq!(g.missile_rng, 0xA5);
}
#[test]
fn actor_slot_reuse_invalidates_all_missile_locks_to_its_previous_occupant() {
    let mut g = running();
    let bank = test_bank(0);
    g.actors[0] = enemy(assets::FIGHTER, 80, 0, 1000);
    g.actors[1] = enemy(assets::DIVER, -80, 0, 1200);
    for slot in 0..SHOTS {
        g.set_missile_lock(slot, 1 + (slot % 2) as u8);
    }
    g.kill_actor(0, &bank);
    g.spawn_actor(
        Spawn {
            kind: assets::FIGHTER,
            x: -80,
            ..Spawn::default()
        },
        0,
        &bank,
    );
    assert_eq!(g.actors[0].pos.x, -80);
    for slot in 0..SHOTS {
        assert_eq!(g.missile_lock(slot), if slot % 2 == 0 { 0 } else { 2 });
    }
    // Invalidating the odd half of every packed byte must preserve its even
    // neighbor, including both the first and last projectile slots.
    for slot in (0..SHOTS).step_by(2) {
        g.set_missile_lock(slot, 15);
    }
    g.kill_actor(1, &bank);
    g.spawn_actor(
        Spawn {
            kind: assets::DIVER,
            ..Spawn::default()
        },
        0,
        &bank,
    );
    for slot in 0..SHOTS {
        assert_eq!(g.missile_lock(slot), if slot % 2 == 0 { 15 } else { 0 });
    }
}
#[test]
fn missile_lock_clears_on_impact_and_range_expiry() {
    let bank = test_bank(0);
    for z in [700, 3000] {
        let mut g = running();
        g.actors[0] = enemy(assets::FIGHTER, 0, 0, z);
        g.shots[0] = Shot {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: if z == 700 { 668 } else { 2390 },
            },
            damage: 4,
            kind: 3,
            ..Shot::EMPTY
        };
        g.set_missile_lock(0, 1);
        g.update_shots(&bank);
        assert_eq!(g.shots[0].kind, 0);
        assert_eq!(g.missile_lock(0), 0);
    }
}
#[test]
fn missile_scene_resets_clear_locks_and_respawns_preserve_random_sequence() {
    let mut g = running();
    g.missile_targets.fill(0xef);
    g.missile_rng = 0x37;
    g.respawn();
    assert_eq!(g.missile_targets, [0; SHOTS / 2]);
    assert_eq!(g.missile_rng, 0x37);
    g.missile_targets.fill(0xef);
    g.reset_scene(Mode::Attract, 6, 100);
    assert_eq!(g.missile_targets, [0; SHOTS / 2]);
    assert_eq!(g.missile_rng, 0xA5);
    g.missile_targets.fill(0xef);
    g.missile_rng = 0x49;
    g.reset_run();
    assert_eq!(g.missile_targets, [0; SHOTS / 2]);
    assert_eq!(g.missile_rng, 0xA5);
}
#[test]
fn missile_guidance_reaches_moving_fighters_divers_and_distant_opposite_side_turrets() {
    for kind in [assets::FIGHTER, assets::DIVER, assets::TURRET] {
        for z in [1000, 1200, 1400, 1800] {
            let mut g = running();
            g.player.x = 100;
            g.player.y = if kind == assets::TURRET { 65 } else { -65 };
            g.actors[0] = if kind == assets::TURRET {
                enemy(kind, -95, -90, z)
            } else {
                enemy(kind, -80, 35, z)
            };
            g.fire_player(255);
            let initial_hp = g.actors[0].hp;
            let mut hit = false;
            for _ in 0..60 {
                let before = g.shots[0];
                g.update_actors(&test_bank(g.segment_index()));
                g.update_shots(&test_bank(g.segment_index()));
                let after = g.shots[0];
                assert!((after.dx - before.dx).abs() <= 1);
                assert!((after.dy - before.dy).abs() <= 1);
                assert!(after.dx.abs() <= 12 && after.dy.abs() <= 12);
                if g.actors[0].kind == 0 || g.actors[0].hp < initial_hp {
                    hit = true;
                    assert_eq!(g.shots[0].kind, 0);
                    if kind == assets::TURRET {
                        assert_eq!(g.actors[0].hp, 2, "missile damage remains four");
                    } else {
                        assert_eq!(g.score, 100, "missile kills use ordinary scoring");
                    }
                    break;
                }
            }
            assert!(hit, "missile missed kind {kind} at initial z {z}");
        }
    }
}
#[test]
fn missile_boss_targeting_tracks_live_turrets_then_exposed_core() {
    let mut g = running();
    g.boss_started = true;
    g.boss_z = 900;
    let origin = Vec3 { x: 0, y: 0, z: 200 };
    let mut lock = 0;
    assert_eq!(
        g.missile_target(origin, &mut lock, &test_bank(g.segment_index())),
        Some((
            Vec3 {
                x: 130,
                y: 30,
                z: 830
            },
            40
        ))
    );
    assert_eq!(lock, 14);
    g.boss_right = 0;
    g.boss_z = 680;
    assert_eq!(
        g.missile_target(origin, &mut lock, &test_bank(g.segment_index())),
        Some((
            Vec3 {
                x: -130,
                y: 30,
                z: 610
            },
            32
        ))
    );
    assert_eq!(lock, 13);
    g.boss_left = 0;
    assert_eq!(
        g.missile_target(origin, &mut lock, &test_bank(g.segment_index())),
        Some((Vec3 { x: 0, y: 0, z: 480 }, 32))
    );
    assert_eq!(lock, 15);
    g.fire_player(255);
    for _ in 0..20 {
        g.update_shots(&test_bank(g.segment_index()));
    }
    assert_eq!(g.boss_core, 116);
    g.boss_defeated = true;
    assert_eq!(
        g.missile_target(origin, &mut lock, &test_bank(g.segment_index())),
        None
    );
    assert_eq!(lock, 0);
}
#[test]
fn missile_rejects_shielded_dead_passed_and_inactive_boss_locks() {
    let mut g = running();
    let bank = test_bank(0);
    let origin = Vec3 { x: 0, y: 0, z: 200 };
    g.boss_started = true;
    g.boss_z = 680;
    let mut lock = 15;
    assert!(g.missile_target(origin, &mut lock, &bank).is_some());
    assert_eq!(lock, 14, "a shielded core lock reacquires a live turret");
    g.boss_left = 0;
    g.boss_right = 0;
    g.boss_core = 0;
    assert_eq!(g.missile_target(origin, &mut lock, &bank), None);
    assert_eq!(lock, 0);
    g.boss_core = 120;
    g.boss_z = origin.z - BOSS_CORE_Z;
    lock = 15;
    assert_eq!(g.missile_target(origin, &mut lock, &bank), None);
    assert_eq!(lock, 0, "targets at the missile depth are no longer ahead");
    g.boss_z = 680;
    g.boss_started = false;
    lock = 15;
    assert_eq!(g.missile_target(origin, &mut lock, &bank), None);
    assert_eq!(lock, 0);
}
#[test]
fn enemy_shot_boxes_cover_their_scaled_boundaries_in_every_roster() {
    for level in 0..assets::SEGMENT_COUNT {
        let bank = test_bank(level);
        for kind in 1..=bank.enemy_count() {
            let center_y = bank.enemy(kind).center_y as i16;
            for (offset, hit) in [(28, true), (29, false), (-28, true), (-29, false)] {
                for axis in 0..3 {
                    let mut g = running();
                    g.actors[0] = enemy(kind, 0, -center_y, 700);
                    let hp = g.actors[0].hp;
                    // Positive depth probes start at the rear boundary;
                    // negative probes finish at the front boundary.
                    g.shots[0] = Shot {
                        pos: Vec3 {
                            x: if axis == 0 { offset } else { 0 },
                            y: if axis == 1 { offset } else { 0 },
                            z: if axis == 2 {
                                700 + offset - if offset < 0 { 48 } else { 0 }
                            } else {
                                652
                            },
                        },
                        kind: 1,
                        damage: 1,
                        ..Shot::EMPTY
                    };
                    g.update_shots(&bank);
                    assert_eq!(
                        g.actors[0].hp,
                        hp - u8::from(hit),
                        "{level}/{kind}, axis {axis}, offset {offset}"
                    );
                    assert_eq!(g.shots[0].kind == 0, hit);
                }
            }
        }
    }
}
#[test]
fn scaled_ram_boxes_include_depth_while_obstacles_keep_their_original_depth() {
    for (kind, center_y, radius, depth) in [
        (assets::TURRET, 19, 27, assets::enemy_scaled(32)),
        (assets::ROCK, 20, 36, 32),
    ] {
        for (x, dz, hit) in [
            (radius, depth - 1, true),
            (radius + 1, 0, false),
            (0, depth, false),
        ] {
            let mut g = running();
            g.actors[0] = enemy(kind, x, -center_y, PLAYER_Z + dz + 8);
            g.update_actors(&test_bank(0));
            assert_eq!(
                g.shield,
                if hit { 70 } else { 100 },
                "kind {kind}, x {x}, depth {dz}"
            );
        }
    }
}
#[test]
#[cfg(feature = "host-tools")]
fn actors_fly_through_the_near_plane_until_their_mesh_is_fully_behind_it() {
    use crate::render::{Renderer, NEAR};
    for level in [0, 2, 6, 12] {
        let bank = test_bank(level);
        for kind in (1..=bank.enemy_count()).chain([assets::ROCK, assets::BUILDING]) {
            let depths = (0..bank.vertex_count(kind)).map(|index| bank.vertex(kind, index).z);
            let front = depths.clone().max().unwrap();
            let back = depths.min().unwrap();
            assert!(
                ACTOR_RETIRE_Z + front < NEAR,
                "level {level} kind {kind} can still be visible when retired"
            );
            // Straddling the plane keeps a clipped remainder; fully behind it
            // the renderer rejects every face until the sim retires the actor.
            let straddle = NEAR - front + 16;
            assert!(
                straddle + back < NEAR,
                "level {level} kind {kind} fixture does not straddle"
            );
            for (z, drawn) in [
                (80, true),
                (straddle, true),
                (NEAR - front - 1, false),
                (ACTOR_RETIRE_Z, false),
            ] {
                let mut g = running();
                g.actors[0] = enemy(kind, 0, 0, z);
                let mut renderer = Renderer::new();
                renderer.begin(0);
                g.render_combat(
                    &mut SceneBudget {
                        sink: &mut renderer,
                        count: 0,
                        limit: 174,
                        camera_y: 0,
                        color_override: 0,
                        extra_flags: 0,
                    },
                    &bank,
                );
                assert_eq!(
                    renderer.triangle_count > 0,
                    drawn,
                    "level {level} kind {kind} at z {z}"
                );
            }
        }
    }
}
#[test]
fn actors_and_pickups_retire_only_behind_the_near_plane() {
    use crate::render::NEAR;
    let bank = test_bank(0);
    for kind in [assets::FIGHTER, assets::DIVER, assets::TURRET, assets::ROCK] {
        let mut g = running();
        // The fastest approach is 14 units per tick, so this actor stays
        // above the retire depth; the other one is already behind it.
        g.actors[0] = enemy(kind, 0, 0, ACTOR_RETIRE_Z + 31);
        g.actors[1] = enemy(kind, 0, 0, ACTOR_RETIRE_Z - 1);
        g.update_actors(&bank);
        assert_ne!(
            g.actors[0].kind, 0,
            "kind {kind} retired while still drawable"
        );
        assert_eq!(
            g.actors[1].kind, 0,
            "kind {kind} survived behind the retire depth"
        );
        assert_eq!(g.shield, 100);
    }
    let mut g = running();
    g.pickups[0] = Pickup {
        pos: Vec3 {
            x: 0,
            y: 0,
            z: NEAR + SPEED,
        },
        kind: 2,
    };
    g.pickups[1] = Pickup {
        pos: Vec3 {
            x: 0,
            y: 0,
            z: NEAR + SPEED - 1,
        },
        kind: 1,
    };
    g.update_pickups();
    assert_eq!((g.pickups[0].kind, g.pickups[1].kind), (2, 0));
}
#[test]
fn scaled_enemy_proxies_cover_distant_and_budget_limited_rendering() {
    #[derive(Default)]
    struct Capture(std::vec::Vec<Vec3>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, _: u8, _: u8) {
            self.0.extend([a, b, c]);
        }
    }
    for level in 0..assets::SEGMENT_COUNT {
        let bank = test_bank(level);
        for kind in (1..=bank.enemy_count()).chain([assets::ROCK, assets::BUILDING]) {
            for (z, limit) in [(1800, 174), (700, 2)] {
                let mut g = running();
                g.actors[0] = enemy(kind, 0, 0, z);
                let mut capture = Capture::default();
                g.render_combat(
                    &mut SceneBudget {
                        sink: &mut capture,
                        count: 0,
                        limit,
                        camera_y: 0,
                        color_override: 0,
                        extra_flags: 0,
                    },
                    &bank,
                );
                assert_eq!(capture.0.len(), 6);
                let radius = match kind {
                    assets::FIGHTER => assets::enemy_scaled(22),
                    assets::ROCK | assets::BUILDING => 18,
                    _ => assets::enemy_scaled(18),
                };
                let center_y = actor_center(g.actors[0], &bank).y;
                let xs = capture.0.iter().map(|v| v.x);
                let ys = capture.0.iter().map(|v| v.y);
                assert_eq!((xs.clone().min(), xs.max()), (Some(-radius), Some(radius)));
                assert_eq!(
                    (ys.clone().min(), ys.max()),
                    (Some(center_y - radius), Some(center_y + radius))
                );
            }
        }
    }
}
#[test]
fn all_boss_turret_shots_keep_inclusive_bounds_and_ignore_destroyed_sides() {
    // Independently recorded scaled turret bounds: X +/-84..176,
    // Y -16..76, Z 514..706 when the boss is parked at Z 680.
    for stage in [3, 7, 11] {
        for side in [-1, 1] {
            for hp in [0u8, 1, 20] {
                for (x, y, old_z, inside) in [
                    (130, 30, 562, true),
                    (83, 30, 562, false),
                    (84, 30, 562, true),
                    (176, 30, 562, true),
                    (177, 30, 562, false),
                    (130, -17, 562, false),
                    (130, -16, 562, true),
                    (130, 76, 562, true),
                    (130, 77, 562, false),
                    (130, 30, 465, false),
                    (130, 30, 466, true),
                    (130, 30, 706, true),
                    (130, 30, 707, false),
                    (84, -16, 466, true),
                    (176, 76, 706, true),
                ] {
                    let mut game = running();
                    set_rail(&mut game, stage * SECTION_TICKS + BOSS_START_TICKS);
                    game.boss_started = true;
                    game.boss_z = 680;
                    if side < 0 {
                        game.boss_left = hp;
                    } else {
                        game.boss_right = hp;
                    }
                    game.shots[0] = Shot {
                        pos: Vec3 {
                            x: x * side,
                            y,
                            z: old_z,
                        },
                        kind: 1,
                        damage: 4,
                        ..Shot::EMPTY
                    };
                    game.update_shots(&test_bank(stage as u8));
                    let hit = inside && hp != 0;
                    let actual = if side < 0 {
                        (game.boss_left, game.boss_right)
                    } else {
                        (game.boss_right, game.boss_left)
                    };
                    assert_eq!(
                        actual,
                        (if hit { hp.saturating_sub(4) } else { hp }, 40),
                        "stage {stage}, side {side}, hp {hp}, ({x},{y},{old_z})"
                    );
                    assert_eq!(
                        (game.boss_core, game.boss_defeated, game.score),
                        (120, false, 0)
                    );
                    assert_eq!(game.shots[0].kind, if hit { 0 } else { 1 });
                    assert_eq!(
                        game.pop_audio().map(|event| (event.effect, event.pan)),
                        hit.then_some((Effect::Hit, (x * side).clamp(-127, 127) as i8))
                    );
                    assert!(game.pop_audio().is_none());
                }
            }
        }
    }
}

#[test]
fn all_boss_cores_absorb_shielded_shots_at_the_same_inclusive_bounds() {
    for stage in [3, 7, 11] {
        for (left, right) in [(20, 20), (0, 20), (20, 0), (0, 0)] {
            for (x, y, old_z, hit) in [
                (0, 0, 432, true),
                (-60, 0, 432, true),
                (60, 0, 432, true),
                (-61, 0, 432, false),
                (61, 0, 432, false),
                (0, -60, 432, true),
                (0, 60, 432, true),
                (0, -61, 432, false),
                (0, 61, 432, false),
                (0, 0, 335, false),
                (0, 0, 336, true),
                (0, 0, 576, true),
                (0, 0, 577, false),
                (-60, -60, 336, true),
                (60, 60, 576, true),
            ] {
                let mut game = running();
                set_rail(&mut game, stage * SECTION_TICKS + BOSS_START_TICKS);
                game.boss_started = true;
                game.boss_z = 680;
                game.boss_left = left;
                game.boss_right = right;
                game.shots[0] = Shot {
                    pos: Vec3 { x, y, z: old_z },
                    kind: 1,
                    damage: 4,
                    ..Shot::EMPTY
                };
                game.update_shots(&test_bank(stage as u8));
                assert_eq!((game.boss_left, game.boss_right), (left, right));
                assert_eq!(
                    game.boss_core,
                    if hit && left == 0 && right == 0 {
                        116
                    } else {
                        120
                    },
                    "stage {stage}, shields ({left},{right}), ({x},{y},{old_z})"
                );
                assert_eq!(game.shots[0].kind, if hit { 0 } else { 1 });
                assert_eq!(
                    game.pop_audio().map(|event| (event.effect, event.pan)),
                    hit.then_some((Effect::Hit, x as i8))
                );
                assert!(game.pop_audio().is_none());
            }
        }
    }
}

#[test]
fn all_boss_shots_keep_actor_priority_and_single_core_death_feedback() {
    for stage in [3, 7, 11] {
        let bank = test_bank(stage as u8);
        for side in [-1, 1] {
            let mut game = running();
            set_rail(&mut game, stage * SECTION_TICKS + BOSS_START_TICKS);
            game.boss_started = true;
            game.boss_z = 680;
            game.actors[0] = enemy(assets::FIGHTER, 130 * side, 30, 610);
            game.actors[0].hp = 6;
            game.shots[0] = Shot {
                pos: Vec3 {
                    x: 130 * side,
                    y: 30,
                    z: 562,
                },
                kind: 1,
                damage: 4,
                ..Shot::EMPTY
            };
            game.update_shots(&bank);
            assert_eq!(game.actors[0].hp, 2);
            assert_eq!(
                (game.boss_left, game.boss_right, game.boss_core),
                (40, 40, 120)
            );
            assert_eq!(game.shots[0].kind, 0);
            assert_eq!(game.pop_audio().unwrap().effect, Effect::Hit);
            assert!(game.pop_audio().is_none());
        }
        let mut game = running();
        set_rail(&mut game, stage * SECTION_TICKS + BOSS_START_TICKS);
        game.boss_started = true;
        game.boss_z = 680;
        game.boss_left = 0;
        game.boss_right = 0;
        game.boss_core = 4;
        let shot = Shot {
            pos: Vec3 { x: 0, y: 0, z: 432 },
            kind: 1,
            damage: 4,
            ..Shot::EMPTY
        };
        // An indestructible obstacle absorbs the shot before the exposed core.
        game.actors[0] = enemy(assets::ROCK, 0, -20, 480);
        game.actors[0].hp = 255;
        game.shots[0] = shot;
        game.update_shots(&bank);
        assert_eq!((game.boss_core, game.score, game.shots[0].kind), (4, 0, 0));
        assert!(game.pop_audio().is_none());
        game.actors[0] = Actor::EMPTY;
        game.shots[0] = shot;
        game.update_shots(&bank);
        assert_eq!(
            (game.boss_core, game.score, game.shots[0].kind),
            (0, 5000, 0)
        );
        assert!(game.boss_defeated);
        assert_eq!(game.pop_audio().unwrap().effect, Effect::Hit);
        assert_eq!(game.pop_audio().unwrap().effect, Effect::Death);
        assert!(game.pop_audio().is_none());
        game.shots[0] = shot;
        game.update_shots(&bank);
        assert_eq!((game.score, game.shots[0].kind), (5000, 1));
        assert!(game.pop_audio().is_none());
    }
}

#[test]
fn enlarged_carrier_targets_are_reachable_and_fire_from_the_visible_parts() {
    for side in [-1, 1] {
        for (x, hit) in [
            (83, false),
            (84, true),
            (100, true),
            (176, true),
            (177, false),
        ] {
            let mut g = running();
            g.boss_started = true;
            g.boss_z = 680;
            g.shots[0] = Shot {
                pos: Vec3 {
                    x: x * side,
                    y: 30,
                    z: 562,
                },
                kind: 1,
                damage: 1,
                ..Shot::EMPTY
            };
            g.update_shots(&test_bank(11));
            assert_eq!(
                if side < 0 { g.boss_left } else { g.boss_right },
                40 - u8::from(hit)
            );
        }
        // Center launches have room to turn during approach; at the parked
        // carrier the player can launch from the corresponding arena edge.
        for (boss_z, player_x) in [(900, 0), (680, 100 * side)] {
            let mut g = running();
            g.boss_started = true;
            g.boss_z = boss_z;
            g.player.x = player_x;
            if side < 0 {
                g.boss_right = 0;
            } else {
                g.boss_left = 0;
            }
            g.fire_player(255);
            for _ in 0..30 {
                g.update_shots(&test_bank(11));
            }
            assert_eq!(
                if side < 0 { g.boss_left } else { g.boss_right },
                36,
                "side {side}, boss_z {boss_z}"
            );
        }
    }
    let mut g = running();
    set_rail(&mut g, 11 * SECTION_TICKS + BOSS_START_TICKS);
    g.boss_z = 680;
    g.ticks = 40;
    g.update_boss();
    assert_eq!(
        g.shots[0].pos,
        Vec3 {
            x: -130,
            y: 66,
            z: 554
        }
    );
    assert_eq!(
        g.shots[1].pos,
        Vec3 {
            x: 130,
            y: 66,
            z: 554
        }
    );
    g.shots.fill(Shot::EMPTY);
    g.boss_left = 0;
    g.boss_right = 0;
    g.ticks = 50;
    g.update_boss();
    assert_eq!(
        g.shots[0].pos,
        Vec3 {
            x: -28,
            y: 0,
            z: 480
        }
    );
    assert_eq!(
        g.shots[1].kind, 0,
        "exposed carrier alternates fast core bursts"
    );
    g.shots.fill(Shot::EMPTY);
    g.ticks = 75;
    g.update_boss();
    assert_eq!(
        g.shots[0].pos,
        Vec3 {
            x: 28,
            y: 0,
            z: 480
        }
    );
}
#[test]
fn missiles_obey_obstacle_collision_and_boss_core_protection() {
    let mut g = running();
    g.actors[0] = enemy(assets::ROCK, 0, -20, 230);
    g.actors[0].hp = 255;
    g.fire_player(255);
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.shots[0].kind, 0);
    assert_eq!(g.actors[0].hp, 255);
    assert_eq!(g.score, 0);
    g.actors[0] = Actor::EMPTY;
    g.boss_started = true;
    g.boss_z = 680;
    g.shots[0] = Shot {
        pos: Vec3 { x: 0, y: 0, z: 450 },
        kind: 3,
        damage: 4,
        ..Shot::EMPTY
    };
    g.update_shots(&test_bank(g.segment_index()));
    assert_eq!(g.shots[0].kind, 0);
    assert_eq!(g.boss_core, 120, "a missile cannot damage a shielded core");
}
#[test]
fn missile_pool_priority_preserves_charge_and_ordinary_shot_rules() {
    let missile = Shot {
        pos: Vec3 { x: 0, y: 0, z: 198 },
        kind: 3,
        damage: 4,
        ..Shot::EMPTY
    };
    let mut g = running();
    g.missile_targets.fill(0xee);
    g.add_shot(missile);
    assert_eq!(g.shots[0].kind, 3);
    assert_eq!((g.missile_lock(0), g.missile_lock(1)), (0, 14));
    for (i, shot) in g.shots.iter_mut().enumerate() {
        *shot = Shot {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: 300 + i as i16 * 10,
            },
            kind: 1,
            ..Shot::EMPTY
        };
    }
    g.shots[2].kind = 2;
    g.add_shot(missile);
    assert_eq!(g.shots[2].kind, 3, "hostiles have first eviction priority");
    assert_eq!((g.missile_lock(2), g.missile_lock(3)), (0, 14));
    assert_eq!(g.shots[23].kind, 1);
    g.add_shot(missile);
    assert_eq!(
        g.shots[23].kind, 3,
        "then replace the farthest ordinary laser"
    );
    assert_eq!((g.missile_lock(22), g.missile_lock(23)), (14, 0));
    g.add_shot(Shot {
        kind: 1,
        ..Shot::EMPTY
    });
    assert_eq!(g.shots[23].kind, 3, "ordinary lasers never evict missiles");
    for (i, shot) in g.shots.iter_mut().enumerate() {
        *shot = Shot {
            pos: Vec3 {
                x: 0,
                y: 0,
                z: 300 + i as i16,
            },
            ..missile
        };
    }
    g.missile_targets.fill(0xdd);
    g.add_shot(missile);
    assert_eq!(
        g.shots[23].pos.z, 198,
        "all-missile saturation still fires the charge"
    );
    assert_eq!((g.missile_lock(22), g.missile_lock(23)), (13, 0));
    g.shots[0].kind = 2;
    g.bomb(&test_bank(g.segment_index()));
    assert_eq!(g.shots[0].kind, 0);
    assert_eq!(g.shots[23].kind, 3, "bombs clear only hostile shots");
}
#[test]
fn damage_flash_is_independent_of_immunity_and_resets_with_the_run() {
    let mut g = Game::new();
    g.reset_run();
    assert_eq!((g.invulnerability, g.hit_flash_ticks), (50, 0));
    g.invulnerability = 0;
    g.damage_player(12);
    assert_eq!((g.shield, g.hit_flash_ticks), (88, 35));
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    for _ in 0..10 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    g.damage_player(12);
    assert_eq!(
        (g.shield, g.hit_flash_ticks),
        (88, 25),
        "ignored hits do not restart the flash"
    );
    for _ in 0..25 {
        g.tick(&segment, &test_bank(g.segment_index()));
    }
    assert_eq!(g.hit_flash_ticks, 0);
    g.damage_player(12);
    assert_eq!(g.hit_flash_ticks, 35);
    g.mode = Mode::Victory;
    assert_eq!(
        g.player_appearance() >> 8,
        0,
        "victory never freezes a white hull"
    );
    g.respawn();
    assert_eq!((g.invulnerability, g.hit_flash_ticks), (100, 0));
    g.hit_flash_ticks = 35;
    g.reset_run();
    assert_eq!(g.hit_flash_ticks, 0);
}
#[test]
fn full_eighteen_stage_campaign_is_winnable_with_regular_joystick_input() {
    let banks: [_; assets::SEGMENT_COUNT as usize] =
        core::array::from_fn(|i| mission_segment(i as u8));
    let mut g = running();
    let mut visited = 0u32;
    let mut scenic_ticks = 0u16;
    for tick in 1..=90_000u32 {
        let section = g.segment_index();
        visited |= 1 << section;
        let segment = Segment::new(&banks[section as usize]).unwrap();
        #[cfg(feature = "host-tools")]
        let model_bytes = enemies::authored::model_bank(section);
        #[cfg(feature = "host-tools")]
        let bank = AssetBank::new(&model_bytes).unwrap();
        #[cfg(not(feature = "host-tools"))]
        let bank = test_bank(section);
        let mut target = Vec3 { x: 0, y: 0, z: 800 };
        if g.boss_started && !g.boss_defeated {
            target = if g.boss_left > 0 {
                Vec3 {
                    x: -100,
                    y: 30,
                    z: 610,
                }
            } else if g.boss_right > 0 {
                Vec3 {
                    x: 100,
                    y: 30,
                    z: 610,
                }
            } else {
                Vec3 { x: 0, y: 0, z: 480 }
            };
        } else {
            let mut nearest = 2400;
            for actor in &g.actors {
                if actor.kind != 0 && actor.hp != 255 && actor.pos.z > 300 && actor.pos.z < nearest
                {
                    target = actor_center(*actor, &bank);
                    nearest = actor.pos.z;
                }
            }
            // Follow collectable shields when no attack run is active.
            if nearest == 2400 {
                for pickup in &g.pickups {
                    if pickup.kind == 1 && pickup.pos.z > 200 {
                        target = pickup.pos;
                        break;
                    }
                }
            }
        }
        // The ship carries momentum: steer on where it will stop (about
        // eleven ticks of travel), as a pilot leads with the stick.
        let ahead_x = g.player.x + (g.flight.vel[0] >> 5);
        let ahead_y = g.player.y + (g.flight.vel[1] >> 5);
        let mut bits = FIRE;
        if target.x - ahead_x > 3 {
            bits |= RIGHT;
        } else if target.x - ahead_x < -3 {
            bits |= LEFT;
        }
        if target.y - ahead_y > 3 {
            bits |= UP;
        } else if target.y - ahead_y < -3 {
            bits |= DOWN;
        }
        // Defensive double taps use the same input path as the hardware; the
        // pilot dodges toward the target's side and commits to the roll's
        // direction while the hull spins, as the velocity model rewards.
        let dodge = if target.x > g.player.x { RIGHT } else { LEFT };
        match tick % 50 {
            49 | 1 => bits &= !(LEFT | RIGHT),
            0 | 2 => bits = (bits & !(LEFT | RIGHT)) | dodge,
            _ => {}
        }
        if g.roll_ticks > 0 {
            bits = (bits & !(LEFT | RIGHT))
                | if g.roll_direction < 0 { LEFT } else { RIGHT };
        }
        if g.boss_started && matches!(tick % 200, 0 | 2) {
            bits &= !FIRE;
        }
        sample(&mut g, bits, tick * 20);
        g.tick(&segment, &bank);
        if g.mode == Mode::Transition {
            scenic_ticks += 1;
        }
        while g.pop_audio().is_some() {}
        assert_ne!(
            g.mode,
            Mode::GameOver,
            "pilot lost all lives in section {} at {}",
            g.segment_index(),
            g.hud().distance
        );
        if g.mode == Mode::Victory {
            assert_eq!(visited, (1 << assets::SEGMENT_COUNT) - 1);
            assert!(
                tick >= MISSION_TICKS as u32
                    + (assets::SEGMENT_COUNT as u32 - 1) * TRANSITION_TICKS as u32
            );
            assert!(scenic_ticks >= (assets::SEGMENT_COUNT as u16 - 1) * TRANSITION_TICKS);
            assert!(g.score >= 5000);
            return;
        }
    }
    panic!(
        "mission did not finish: {:?}, section {} at {}, boss {}/{}/{}, lives {}",
        g.mode,
        g.segment_index(),
        g.hud().distance,
        g.boss_left,
        g.boss_right,
        g.boss_core,
        g.lives
    );
}
#[test]
fn level_rosters_have_distinct_movement_and_attack_tactics() {
    let player = Vec3 {
        x: 90,
        y: 30,
        z: PLAYER_Z,
    };
    let mut poses = [[[Vec3::default(); 4]; 3]; 4];
    let mut attacks = [[0u16; 3]; 4];
    for level in 0..4 {
        for kind in 1..=test_bank([0, 2, 6, 11][level as usize]).enemy_count() {
            let mut actor = enemy(kind, -90, 0, 1800);
            for age in 1..=180 {
                actor.age = age;
                attacks[level as usize][kind as usize - 1] +=
                    enemies::step(level, &mut actor, &player, 0) as u16;
                if let Some(sample) = [16, 64, 112, 152].iter().position(|&v| v == age) {
                    poses[level as usize][kind as usize - 1][sample] = actor.pos;
                }
            }
        }
    }
    assert_ne!(poses[0][0][0].x, -90, "patrol weaves");
    assert!(poses[0][1][1].x > poses[0][1][0].x, "diver pursues");
    assert_eq!(poses[0][2][2].x, -90, "ground gun holds its lane");
    assert!(poses[1][0][1].x > 0, "skimmer crosses the canyon");
    assert!(
        poses[1][1][0].y > 0 && poses[1][1][1].y < 0,
        "swooper changes altitude"
    );
    assert_eq!(attacks[1][2], 2, "wall battery fires a paired burst");
    assert_eq!(poses[2][0][0].z, 1800 - 16 * 6, "gunship approaches slowly");
    assert_eq!(
        poses[2][1][2].z, poses[2][1][3].z,
        "lance holds before rushing"
    );
    assert_eq!(attacks[2][1], 2, "lance fires twice during its hold");
    assert_eq!(attacks[2][2], 2, "bunker requests a fan attack");
    assert!(
        poses[3][0].iter().all(|p| (-122..=-58).contains(&p.x)),
        "guard holds the flank"
    );
    assert!(
        poses[3][1][1].x > poses[3][1][0].x,
        "rammer tracks the player"
    );
    assert_eq!(
        poses[3][1][2].x, poses[3][1][3].x,
        "rammer commits near the player"
    );
    assert_eq!(attacks[3][1], 0, "rammer has no gun");
}

#[test]
fn every_level_enemy_uses_banked_health_rewards_and_missile_guidance() {
    let mut exercised = 0;
    for level in 0..assets::SEGMENT_COUNT {
        let bank = test_bank(level);
        for kind in 1..=bank.enemy_count() {
            exercised += 1;
            let mut g = running();
            set_rail(&mut g, level as u16 * SECTION_TICKS);
            let spawn = Spawn {
                kind,
                x: 85,
                y: 25,
                ..Spawn::default()
            };
            g.spawn_actor(spawn, 0, &bank);
            assert_eq!(g.actors[0].hp, bank.enemy(kind).hp);
            let center = actor_center(g.actors[0], &bank);
            for _ in 0..bank.enemy(kind).hp {
                g.shots[0] = Shot {
                    pos: Vec3 {
                        z: center.z - 48,
                        ..center
                    },
                    damage: 1,
                    kind: 1,
                    ..Shot::EMPTY
                };
                g.update_shots(&bank);
            }
            assert_eq!(g.actors[0].kind, 0, "laser kill level {level} kind {kind}");
            assert_eq!(g.score, bank.enemy(kind).score as u32);
            assert_eq!(g.bomb_kills, 1);

            g.spawn_actor(spawn, 0, &bank);
            g.player.x = -100;
            g.player.y = -65;
            let initial_hp = g.actors[0].hp;
            g.fire_player(255);
            for _ in 0..100 {
                g.update_actors(&bank);
                g.update_shots(&bank);
                if g.actors[0].kind == 0 || g.actors[0].hp < initial_hp {
                    break;
                }
            }
            assert!(
                g.actors[0].kind == 0 || g.actors[0].hp < initial_hp,
                "missile failed level {level} kind {kind}"
            );

            g.actors = [Actor::EMPTY; ACTORS];
            g.spawn_actor(spawn, 0, &bank);
            g.bomb(&bank);
            assert_eq!(g.actors[0].kind, 0, "bomb kill level {level} kind {kind}");
        }
    }
    assert_eq!(exercised, 49);
}

#[test]
fn lance_guidance_uses_hold_and_rush_speed_and_fans_keep_one_audio_cue() {
    let bank = test_bank(6);
    let mut g = running();
    set_rail(&mut g, 6 * SECTION_TICKS);
    g.actors[0] = enemy(2, 50, 0, 904);
    g.actors[0].age = 112;
    g.update_actors(&bank);
    assert_eq!(g.missile_target(g.player, &mut 0, &bank).unwrap().1, 32);
    g.actors[0].age = 152;
    g.update_actors(&bank);
    assert_eq!(g.missile_target(g.player, &mut 0, &bank).unwrap().1, 46);
    g.enemy_attack(Vec3 { x: 0, y: 0, z: 900 }, 2);
    let mut velocities = [0; 3];
    for (at, shot) in g.shots.iter().filter(|s| s.kind == 2).enumerate() {
        velocities[at] = shot.dx;
    }
    velocities.sort();
    assert_eq!(velocities, [-2, 0, 2]);
    assert_eq!(g.pop_audio().unwrap().effect, Effect::EnemyFire);
    assert!(g.pop_audio().is_none());
}

#[test]
fn a_defeated_carrier_does_not_spawn_remaining_escorts() {
    let mut bytes = empty_segment(11);
    bytes[7] = 1;
    bytes[18] = assets::FIGHTER;
    let segment = Segment::new(&bytes).unwrap();
    let mut g = running();
    set_rail(&mut g, 11 * SECTION_TICKS);
    g.boss_defeated = true;
    g.tick(&segment, &test_bank(11));
    assert_eq!(g.spawn_cursor, 1);
    assert!(g.actors.iter().all(|a| a.kind == 0));
}

fn model_bank_for_tests() -> [u8; assets::COMMON_SIZE] {
    let mut data = test_bank_bytes(0);
    let mut cursor = assets::MODEL_DATA_OFFSET;
    for id in 0..assets::MODEL_COUNT as usize {
        let directory = 16 + id * 8;
        data[directory..directory + 2].copy_from_slice(&(cursor as u16).to_le_bytes());
        data[directory + 2] = 3;
        data[directory + 3] = if id == 4 { 17 } else { 12 };
        for vertex in [[-20i8, -10, -5], [20, -10, 5], [0, 20, 0]] {
            for axis in vertex {
                data[cursor] = axis as u8;
                cursor += 1;
            }
        }
        data[directory + 4..directory + 6].copy_from_slice(&(cursor as u16).to_le_bytes());
        for _ in 0..data[directory + 3] {
            data[cursor..cursor + 4].copy_from_slice(&[0, 1, 2, 15]);
            cursor += 4;
        }
    }
    data
}
#[test]
fn white_hit_flash_preserves_ship_and_exhaust_geometry_without_recoloring_the_scene() {
    struct Capture {
        faces: [([Vec3; 3], u8); 256],
        count: usize,
    }
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, _flags: u8) {
            self.faces[self.count] = ([a, b, c], color);
            self.count += 1;
        }
    }
    fn capture(game: &Game, bank: &AssetBank<'_>, segment: &Segment<'_>) -> Capture {
        let mut result = Capture {
            faces: [([Vec3::default(); 3], 0); 256],
            count: 0,
        };
        game.render(&mut result, bank, segment);
        result
    }
    let data = model_bank_for_tests();
    let bank = AssetBank::new(&data).unwrap();
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let mut g = running();
    g.ticks = 2; // This was a hidden phase under the old immunity blink.
    let normal = capture(&g, &bank, &segment);
    let hull_faces = bank.face_count(assets::PLAYER) as usize;
    g.invulnerability = 35;
    for timer in 0..=35 {
        g.hit_flash_ticks = timer;
        let actual = capture(&g, &bank, &segment);
        let white = timer != 0 && ((35 - timer) / 10) % 2 == 0;
        assert_eq!(
            actual.count, normal.count,
            "flash timer {timer} hid geometry"
        );
        for face in 0..hull_faces {
            assert_eq!(actual.faces[face].0, normal.faces[face].0);
            assert_eq!(
                actual.faces[face].1,
                if white { 1 } else { normal.faces[face].1 }
            );
        }
        assert_eq!(
            actual.faces[hull_faces..actual.count],
            normal.faces[hull_faces..normal.count],
            "flash timer {timer} changed exhaust or terrain"
        );
    }
    g.hit_flash_ticks = 0;
    for protection in [50, 35, 1, 100] {
        g.invulnerability = protection;
        let actual = capture(&g, &bank, &segment);
        assert_eq!(actual.faces[..actual.count], normal.faces[..normal.count]);
    }
    g.hit_flash_ticks = 35;
    g.mode = Mode::Victory;
    let victory = capture(&g, &bank, &segment);
    assert_eq!(victory.faces[..hull_faces], normal.faces[..hull_faces]);
    for mode in [Mode::Respawning, Mode::GameOver] {
        g.mode = mode;
        let dead = capture(&g, &bank, &segment);
        assert_eq!(
            dead.count,
            normal.count - hull_faces - 2,
            "destroyed ships retain the existing death sequence"
        );
    }
}
#[test]
fn fully_occupied_scene_keeps_critical_geometry_inside_command_budget() {
    struct CountScenery {
        total: usize,
        cloud_faces: usize,
    }
    impl GeometrySink for CountScenery {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, _: u8, _: u8) {
            self.total += 1;
            if a.z > 2400 && b.z > 2400 && c.z > 2400 {
                self.cloud_faces += 1;
            }
        }
    }
    let data = model_bank_for_tests();
    let bank = AssetBank::new(&data).unwrap();
    let mut g = running();
    g.boss_started = true;
    g.boss_z = 680;
    for a in &mut g.actors {
        *a = Actor {
            pos: Vec3 { x: 0, y: 0, z: 100 },
            kind: assets::TURRET,
            hp: 6,
            ..Actor::EMPTY
        };
    }
    for s in &mut g.shots {
        *s = Shot {
            pos: Vec3 { x: 0, y: 0, z: 300 },
            kind: 2,
            damage: 12,
            ..Shot::EMPTY
        };
    }
    for p in &mut g.pickups {
        *p = Pickup {
            pos: Vec3 { x: 0, y: 0, z: 400 },
            kind: 1,
        };
    }
    for e in &mut g.explosions {
        *e = Explosion {
            pos: Vec3 { x: 0, y: 0, z: 500 },
            ticks: 9,
            color: 7,
        };
    }
    let mut renderer = crate::render::Renderer::new();
    for theme in 0..4 {
        let mut scene = empty_segment(theme);
        scene[5] = theme;
        let segment = Segment::new(&scene).unwrap();
        for phase in 0..64 {
            set_rail(&mut g, ((theme as u16 * SECTION_TICKS + 63) & !63) + phase);
            let mut scenery = CountScenery {
                total: 0,
                cloud_faces: 0,
            };
            g.render(&mut scenery, &bank, &segment);
            assert_eq!(
                scenery.cloud_faces, 9,
                "busy scenes retain every cloud facet"
            );
            assert!(scenery.total <= 233);
            renderer.begin(g.sky_color());
            g.render(&mut renderer, &bank, &segment);
            renderer.finish();
            assert_eq!(renderer.dropped, 0, "terrain phase {phase}");
            assert!(renderer.triangle_count > 100);
            assert!(renderer.triangle_count <= 256);
        }
    }
}
#[cfg(feature = "host-tools")]
#[test]
fn real_enemy_rosters_fit_saturated_and_near_scenes_with_identical_staged_pixels() {
    use crate::render::{Renderer, BAND_PIXELS};
    for level in 0..assets::SEGMENT_COUNT {
        let models = enemies::authored::model_bank(level);
        let bank = AssetBank::new(&models).unwrap();
        let bytes = mission_segment(level);
        let segment = Segment::new(&bytes).unwrap();
        for &(x, y) in &[(0, 0), (-100, -65), (100, 65), (-100, 65), (100, -65)] {
            for (near, moving) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut g = running();
                set_rail(&mut g, level as u16 * SECTION_TICKS + 60);
                g.player.x = x;
                g.player.y = y;
                g.boss_started = g.stage_info.boss_kind != 0;
                g.boss_z = if near { 100 } else { 680 };
                for (i, actor) in g.actors.iter_mut().enumerate() {
                    let kind = 1 + i as u8 % bank.enemy_count();
                    *actor = enemy(
                        kind,
                        (i as i16 % 3 - 1) * 50,
                        20,
                        if near { 80 } else { 700 + i as i16 * 20 },
                    );
                    actor.hp = bank.enemy(kind).hp;
                    if moving {
                        let depth = actor.pos.z;
                        actor.pos.z = 1800;
                        // Include extrema, the sentry's feint, and pursuit
                        // commitment before stressing the near-plane pass.
                        let age = [16, 32, 48, 80, 96, 112, 128, 192, 255, 256, 124, 92][i];
                        for tick in 1..=age {
                            actor.age = tick;
                            enemies::step(level, actor, &g.player, i as u8);
                        }
                        actor.pos.z = depth;
                    }
                }
                for (i, shot) in g.shots.iter_mut().enumerate() {
                    *shot = Shot {
                        pos: Vec3 {
                            x: (i as i16 % 3 - 1) * 40,
                            y: 0,
                            z: if near { 40 } else { 300 },
                        },
                        kind: 2,
                        damage: 12,
                        ..Shot::EMPTY
                    };
                }
                for pickup in &mut g.pickups {
                    *pickup = Pickup {
                        pos: Vec3 {
                            x: 60,
                            y: 0,
                            z: 300,
                        },
                        kind: 1,
                    };
                }
                for explosion in &mut g.explosions {
                    *explosion = Explosion {
                        pos: Vec3 {
                            x: -60,
                            y: 20,
                            z: 300,
                        },
                        ticks: 10,
                        color: 7,
                    };
                }
                let mut direct = Renderer::new();
                let mut staged = Renderer::new();
                direct.begin(g.sky_color());
                g.render(&mut direct, &bank, &segment);
                direct.finish();
                staged.begin(g.sky_color());
                let raw_count = {
                    let mut stage = staged.scenery_stage();
                    g.stage_scenery(&mut stage, &segment);
                    stage.count()
                };
                let [dx, dy] = g.sky_parallax();
                let prefix = staged.project_scenery(raw_count, dx, dy);
                g.render_foreground(&mut staged, &bank, 174.min(233 - raw_count as u16));
                staged.scenery_last(prefix);
                staged.finish();
                assert_eq!(
                    (direct.dropped, staged.dropped),
                    (0, 0),
                    "level {level}, near {near}, moving {moving}"
                );
                assert_eq!(direct.triangle_count, staged.triangle_count);
                for band in 0..25 {
                    let mut a = [0; BAND_PIXELS];
                    let mut b = [0; BAND_PIXELS];
                    direct.render_band(band, &mut a);
                    staged.render_band(band, &mut b);
                    assert_eq!(
                            a, b,
                            "level {level}, near {near}, moving {moving}, player ({x},{y}), band {band}"
                        );
                }
            }
        }
    }
}

#[test]
fn saturated_bridge_weapons_preserve_foreground_and_match_staged_rendering() {
    use crate::render::{Renderer, BAND_PIXELS};

    let data = model_bank_for_tests();
    let bank = AssetBank::new(&data).unwrap();
    let hull_faces = bank.face_count(assets::PLAYER) as u16 + 2; // Ship and exhaust.
    let essential_faces = hull_faces + SHOTS as u16 * 2 + 6 * 2;
    let mut direct = Renderer::new();
    let mut staged = Renderer::new();
    let mut isolated_ship = Renderer::new();
    for section in 0..assets::SEGMENT_COUNT - 1 {
        let bytes = empty_segment(section);
        let segment = Segment::new(&bytes).unwrap();
        for x in [-100, 100] {
            for y in [-65, 65] {
                let mut game = running();
                game.mode = Mode::Transition;
                set_rail(&mut game, (section as u16 + 1) * SECTION_TICKS);
                game.player.x = x;
                game.player.y = y;
                for (index, shot) in game.shots.iter_mut().enumerate() {
                    *shot = Shot {
                        pos: Vec3 {
                            x: (index as i16 % 7 - 3) * 30,
                            y: (index as i16 % 5 - 2) * 22,
                            z: 198 + index as i16 * 83,
                        },
                        kind: if index & 1 == 0 { 1 } else { 3 },
                        damage: if index & 1 == 0 { 1 } else { 4 },
                        ..Shot::EMPTY
                    };
                }
                for (index, explosion) in game.explosions.iter_mut().enumerate() {
                    *explosion = Explosion {
                        pos: Vec3 {
                            x: (index as i16 - 3) * 36,
                            y: (index as i16 % 3 - 1) * 35,
                            z: 400 + index as i16 * 150,
                        },
                        ticks: 1 + index as u8 * 3,
                        color: 7,
                    };
                }
                for elapsed in 0..TRANSITION_TICKS {
                    game.transition_ticks = elapsed;
                    game.ticks = elapsed;
                    game.roll_ticks = 1 + (elapsed % 25) as u8;
                    game.roll_direction = if elapsed & 1 == 0 { -1 } else { 1 };
                    direct.begin(game.sky_color());
                    game.render(&mut direct, &bank, &segment);
                    direct.finish();

                    staged.begin(game.sky_color());
                    let raw_count = {
                        let mut stage = staged.scenery_stage();
                        game.stage_scenery(&mut stage, &segment);
                        stage.count()
                    };
                    let foreground_limit = 174.min(233 - raw_count as u16);
                    assert!(foreground_limit >= essential_faces);
                    let [dx, dy] = game.sky_parallax();
                    let prefix = staged.project_scenery(raw_count, dx, dy);
                    game.render_foreground(&mut staged, &bank, foreground_limit);
                    staged.scenery_last(prefix);
                    staged.finish();
                    assert_eq!(
                        (direct.dropped, staged.dropped),
                        (0, 0),
                        "bridge {section}, tick {elapsed}, player ({x},{y})"
                    );
                    assert_eq!(direct.triangle_count, staged.triangle_count);

                    if matches!(elapsed, 0 | 199 | 399) {
                        isolated_ship.begin(0);
                        game.render_foreground(&mut isolated_ship, &bank, hull_faces);
                        isolated_ship.finish();
                        let mut visible_ship_pixels = 0;
                        let mut ship_pixels = 0;
                        for band in 0..25 {
                            let mut expected = [0; BAND_PIXELS];
                            let mut actual = [0; BAND_PIXELS];
                            let mut ship = [0; BAND_PIXELS];
                            direct.render_band(band, &mut expected);
                            staged.render_band(band, &mut actual);
                            isolated_ship.render_band(band, &mut ship);
                            assert_eq!(
                                actual, expected,
                                "bridge {section}, tick {elapsed}, player ({x},{y}), band {band}"
                            );
                            for (&hull, &scene) in ship.iter().zip(&actual) {
                                if hull != 0 {
                                    ship_pixels += 1;
                                    visible_ship_pixels += usize::from(scene == hull);
                                }
                            }
                        }
                        assert!(ship_pixels > 0, "the rolled ship left the viewport");
                        // Other foreground can overlap a rolled hull at camera extremes;
                        // saturation must not remove all essential foreground.
                        assert!(visible_ship_pixels > 0,
                                "bridge {section}, tick {elapsed}, player ({x},{y}): only {visible_ship_pixels}/{ship_pixels} ship pixels visible");
                    }
                }
            }
        }
    }
}
include!("terrain_visibility_tests.rs");
