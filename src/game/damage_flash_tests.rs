use super::tests::{empty_segment, test_bank};
use super::*;

fn running_at(stage: u8) -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, stage, 0);
    game
}

fn target(kind: u8, x: i16, z: i16) -> Actor {
    Actor {
        kind,
        hp: 20,
        pos: Vec3 { x, y: 0, z },
        ..Actor::EMPTY
    }
}

fn shoot(game: &mut Game, bank: &AssetBank<'_>, pos: Vec3, kind: u8, damage: u8) {
    game.shots[0] = Shot {
        pos: Vec3 {
            z: pos.z - 20,
            ..pos
        },
        kind,
        damage,
        ..Shot::EMPTY
    };
    game.update_shots(bank);
    assert_eq!(game.shots[0].kind, 0, "fixture must hit its target");
}

fn next_feedback_frame(game: &mut Game, elapsed_ticks: u16) {
    game.ticks = game.ticks.wrapping_add(elapsed_ticks);
    game.prepare_damage_feedback_frame();
}

#[test]
fn enemy_damage_flashes_for_lasers_charged_shots_missiles_and_bombs() {
    let bank = test_bank(0);
    for (kind, damage) in [(1, 1), (1, 3), (3, 4), (0, 8)] {
        let mut game = running_at(0);
        game.actors[0] = target(assets::FIGHTER, 0, 520);
        game.actors[1] = target(assets::DIVER, 250, 520);
        if kind == 0 {
            game.bomb(&bank);
        } else {
            let pos = actor_center_sim(&game.actors[0], &bank);
            shoot(&mut game, &bank, pos, kind, damage);
        }
        assert_eq!(
            (game.actors[0].hp, game.actor_hit_flash_ticks[0]),
            (20 - damage, 21)
        );
        assert_eq!(
            (game.actors[1].hp, game.actor_hit_flash_ticks[1]),
            if kind == 0 { (12, 21) } else { (20, 0) },
            "projectile hits must not flash another actor"
        );
    }
}

#[test]
fn blocked_and_zero_damage_hits_do_not_start_or_refresh_an_enemy_flash() {
    let bank = test_bank(0);
    for kind in [1, 3] {
        let mut game = running_at(0);
        game.actors[0] = target(assets::ROCK, 0, 520);
        game.actors[0].hp = 255;
        let pos = actor_center_sim(&game.actors[0], &bank);
        shoot(&mut game, &bank, pos, kind, 4);
        assert_eq!((game.actors[0].hp, game.actor_hit_flash_ticks[0]), (255, 0));
        game.bomb(&bank);
        assert_eq!((game.actors[0].hp, game.actor_hit_flash_ticks[0]), (255, 0));

        game.actors[0] = target(assets::FIGHTER, 0, 520);
        game.actor_hit_flash_ticks[0] = 14;
        let pos = actor_center_sim(&game.actors[0], &bank);
        shoot(&mut game, &bank, pos, kind, 0);
        assert_eq!((game.actors[0].hp, game.actor_hit_flash_ticks[0]), (20, 14));
    }
}

#[test]
fn small_debris_flashes_on_a_surviving_hit_and_still_dies_immediately() {
    let mut game = running_at(12);
    let bank = test_bank(12);
    game.spawn_actor(
        Spawn {
            kind: assets::ROCK,
            flags: assets::SMALL_DEBRIS,
            ..Spawn::default()
        },
        1280,
        &bank,
    );
    let pos = actor_center_sim(&game.actors[0], &bank);
    shoot(&mut game, &bank, pos, 1, 1);
    assert_eq!((game.actors[0].hp, game.actor_hit_flash_ticks[0]), (1, 21));
    shoot(&mut game, &bank, pos, 1, 1);
    assert_eq!(game.actors[0].kind, 0);
    assert!(game.explosions.iter().any(|effect| effect.ticks > 0));
}

#[test]
fn every_boss_flashes_from_turret_and_exposed_core_damage_only() {
    for stage in [3, 7, 11, 16, 17] {
        let bank = test_bank(stage);
        for (kind, damage) in [(1, 1), (1, 3), (3, 4)] {
            for part in [-1, 0, 1] {
                let mut game = running_at(stage);
                game.boss_started = true;
                game.boss_z = 680;
                let pos = if part == 0 {
                    game.boss_left = 0;
                    game.boss_right = 0;
                    Vec3 {
                        x: 0,
                        y: 0,
                        z: game.boss_z + BOSS_CORE_Z,
                    }
                } else {
                    Vec3 {
                        x: part * BOSS_TURRET_X,
                        y: BOSS_TURRET_Y,
                        z: game.boss_z + BOSS_TURRET_Z,
                    }
                };
                shoot(&mut game, &bank, pos, kind, damage);
                assert_eq!(game.boss_hit_flash_ticks, 21, "stage {stage}, part {part}");
                assert_eq!(
                    if part == 0 {
                        game.boss_core
                    } else if part < 0 {
                        game.boss_left
                    } else {
                        game.boss_right
                    },
                    if part == 0 {
                        BOSS_CORE_HP - damage
                    } else {
                        BOSS_SIDE_HP - damage
                    }
                );
                game.prepare_damage_feedback_frame();
                shoot(&mut game, &bank, pos, kind, damage);
                assert_eq!(
                    game.boss_hit_flash_ticks, 20,
                    "repeated boss damage must not extend the white phase"
                );
            }

            let mut game = running_at(stage);
            game.boss_started = true;
            game.boss_z = 680;
            let core = Vec3 {
                x: 0,
                y: 0,
                z: game.boss_z + BOSS_CORE_Z,
            };
            shoot(&mut game, &bank, core, kind, damage);
            assert_eq!(
                (game.boss_core, game.boss_hit_flash_ticks),
                (BOSS_CORE_HP, 0)
            );
            game.boss_hit_flash_ticks = 3;
            shoot(&mut game, &bank, core, kind, damage);
            assert_eq!(
                (game.boss_core, game.boss_hit_flash_ticks),
                (BOSS_CORE_HP, 3)
            );
        }
    }
}

#[test]
fn boss_bomb_flash_preserves_shielding_and_immediate_destruction() {
    for stage in [3, 7, 11, 16, 17] {
        let bank = test_bank(stage);
        let mut game = running_at(stage);
        game.boss_started = true;
        game.boss_left = 4;
        game.boss_right = 4;
        game.bomb(&bank);
        assert_eq!(
            (game.boss_left, game.boss_right, game.boss_core),
            (0, 0, BOSS_CORE_HP)
        );
        assert_eq!(game.boss_hit_flash_ticks, 21);
        game.boss_hit_flash_ticks = 2;
        game.bomb(&bank);
        assert_eq!(
            (game.boss_core, game.boss_hit_flash_ticks),
            (BOSS_CORE_HP - 8, 2)
        );
        game.boss_core = 8;
        game.bomb(&bank);
        assert!(game.boss_defeated);
        assert_eq!(game.score, 5000);
        assert!(game.explosions.iter().any(|effect| effect.ticks > 0));
        game.explosions = [Explosion::EMPTY; 6];
        assert!(capture(&game, &bank, 200).is_empty());
    }
}

#[test]
fn projectile_and_control_bomb_hits_start_ten_full_white_ticks_at_the_first_frame() {
    let bank = test_bank(0);
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    for bomb in [false, true] {
        let mut game = running_at(0);
        game.actors[0] = target(assets::FIGHTER, 0, 520);
        if bomb {
            game.controls.bomb_pending = true;
        } else {
            game.shots[0] = Shot {
                pos: Vec3 { x: 0, y: 0, z: 480 },
                kind: 1,
                damage: 1,
                ..Shot::EMPTY
            };
        }
        game.tick(&segment, &bank);
        assert_eq!(
            game.actor_hit_flash_ticks[0], 21,
            "new damage must receive the full duration"
        );
        assert_eq!(game.actors[0].hp, if bomb { 12 } else { 19 });
        game.prepare_damage_feedback_frame();
        assert_eq!(game.actor_hit_flash_ticks[0], 20);
        for remaining in (10..20).rev() {
            game.tick(&segment, &bank);
            game.prepare_damage_feedback_frame();
            assert_eq!(game.actor_hit_flash_ticks[0], remaining);
        }
        for remaining in (0..10).rev() {
            game.tick(&segment, &bank);
            game.prepare_damage_feedback_frame();
            assert_eq!(game.actor_hit_flash_ticks[0], remaining);
        }
    }
}

#[test]
fn sustained_actor_damage_alternates_white_and_normal_without_extending_or_queueing() {
    let bank = test_bank(0);
    let mut game = running_at(0);
    game.actors[0] = target(assets::FIGHTER, 0, 520);
    game.actors[0].hp = 200;
    let pos = actor_center_sim(&game.actors[0], &bank);
    let normal = capture(&game, &bank, 200);
    for cycle in 0..2 {
        let start_hp = game.actors[0].hp;
        shoot(&mut game, &bank, pos, 1, 1);
        shoot(&mut game, &bank, pos, 1, 1);
        assert_eq!(game.actors[0].hp, start_hp - 2);
        assert_eq!(game.actor_hit_flash_ticks[0], 21);
        game.prepare_damage_feedback_frame();
        assert_white_geometry(&normal, &capture(&game, &bank, 200));
        for elapsed in 1..=20 {
            shoot(&mut game, &bank, pos, 1, 1);
            next_feedback_frame(&mut game, 1);
            assert_eq!(game.actor_hit_flash_ticks[0], 20 - elapsed);
            if elapsed < 10 {
                assert_white_geometry(&normal, &capture(&game, &bank, 200));
            } else {
                assert_eq!(capture(&game, &bank, 200), normal);
            }
        }
        assert_eq!(game.actors[0].hp, start_hp - 22);
        next_feedback_frame(&mut game, 1);
        assert_eq!(
            game.actor_hit_flash_ticks[0], 0,
            "cycle {cycle}: no queued flash"
        );
        assert_eq!(capture(&game, &bank, 200), normal);
    }
}

#[test]
fn sustained_boss_damage_alternates_and_other_parts_cannot_bypass_recovery() {
    for stage in [3, 7, 11, 16, 17] {
        let bank = test_bank(stage);
        let mut game = running_at(stage);
        game.boss_started = true;
        game.boss_z = 680;
        game.boss_left = 100;
        game.boss_right = 100;
        let normal = capture(&game, &bank, 300);
        let left = Vec3 {
            x: -BOSS_TURRET_X,
            y: BOSS_TURRET_Y,
            z: game.boss_z + BOSS_TURRET_Z,
        };
        let right = Vec3 {
            x: BOSS_TURRET_X,
            ..left
        };
        shoot(&mut game, &bank, left, 1, 1);
        game.prepare_damage_feedback_frame();
        assert_white_geometry(&normal, &capture(&game, &bank, 300));
        for elapsed in 1..=20 {
            shoot(
                &mut game,
                &bank,
                if elapsed & 1 == 0 { left } else { right },
                1,
                1,
            );
            next_feedback_frame(&mut game, 1);
            assert_eq!(game.boss_hit_flash_ticks, 20 - elapsed, "stage {stage}");
            if elapsed < 10 {
                assert_white_geometry(&normal, &capture(&game, &bank, 300));
            } else {
                assert_eq!(capture(&game, &bank, 300), normal);
            }
        }
        assert_eq!((game.boss_left, game.boss_right), (89, 90));
        next_feedback_frame(&mut game, 1);
        assert_eq!(
            game.boss_hit_flash_ticks, 0,
            "hits during recovery must not queue"
        );
        shoot(&mut game, &bank, right, 1, 1);
        assert_eq!(game.boss_hit_flash_ticks, 21);
        game.prepare_damage_feedback_frame();
        assert_white_geometry(&normal, &capture(&game, &bank, 300));
    }
}

#[test]
fn actor_slots_and_boss_feedback_advance_independently() {
    let bank = test_bank(0);
    let mut game = running_at(0);
    game.actor_hit_flash_ticks = [0, 1, 5, 10, 11, 15, 20, 21, 0, 8, 18, 21];
    game.boss_hit_flash_ticks = 14;
    next_feedback_frame(&mut game, 4);
    assert_eq!(
        game.actor_hit_flash_ticks,
        [0, 0, 1, 6, 10, 11, 16, 20, 0, 4, 14, 20]
    );
    assert_eq!(game.boss_hit_flash_ticks, 10);
    game.actors[0] = target(assets::FIGHTER, 0, 520);
    let pos = actor_center_sim(&game.actors[0], &bank);
    let previous = game.actor_hit_flash_ticks;
    shoot(&mut game, &bank, pos, 1, 1);
    assert_eq!(game.actor_hit_flash_ticks[0], 21);
    assert_eq!(game.actor_hit_flash_ticks[1..], previous[1..]);
    assert_eq!(game.boss_hit_flash_ticks, 10);
}

#[test]
fn simulation_catch_up_cannot_skip_white_or_normal_frames() {
    let bank = test_bank(0);
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    let mut game = running_at(0);
    game.actors[0] = target(assets::FIGHTER, 0, 1500);
    game.actors[0].hp = 100;
    game.damage_actor(0, 1);
    for _ in 0..30 {
        game.tick(&segment, &bank);
    }
    assert_eq!(
        game.actor_hit_flash_ticks[0], 21,
        "simulation cannot consume unseen feedback"
    );
    game.prepare_damage_feedback_frame();
    assert_eq!(game.actor_hit_flash_ticks[0], 20);
    game.damage_actor(0, 1);
    next_feedback_frame(&mut game, 1000);
    assert_eq!(
        game.actor_hit_flash_ticks[0], 10,
        "a long frame must show normal colors"
    );
    let normal = capture(&game, &bank, 200);
    assert!(!normal.is_empty());
    game.damage_actor(0, 1);
    assert_eq!(game.actor_hit_flash_ticks[0], 10);
    assert_eq!(capture(&game, &bank, 200), normal);
    next_feedback_frame(&mut game, 9);
    assert_eq!(
        game.actor_hit_flash_ticks[0], 1,
        "discard excess time at the white boundary"
    );
    game.damage_actor(0, 1);
    next_feedback_frame(&mut game, 1000);
    assert_eq!(game.actor_hit_flash_ticks[0], 0);
    assert_eq!(capture(&game, &bank, 200), normal);
    game.damage_actor(0, 1);
    next_feedback_frame(&mut game, 1000);
    assert_eq!(
        game.actor_hit_flash_ticks[0], 20,
        "a new flash also gets its full duration"
    );
    assert_white_geometry(&normal, &capture(&game, &bank, 200));
}

#[test]
fn feedback_clock_handles_tick_wrap_and_stays_still_during_pause() {
    let mut game = running_at(0);
    game.ticks = u16::MAX - 4;
    game.prepare_damage_feedback_frame();
    game.actor_hit_flash_ticks[0] = 21;
    game.boss_hit_flash_ticks = 21;
    game.prepare_damage_feedback_frame();
    assert_eq!(
        (game.actor_hit_flash_ticks[0], game.boss_hit_flash_ticks),
        (20, 20)
    );
    for ms in [1000, 1200, 1600] {
        game.sample_paused_input(0, ms);
        game.prepare_damage_feedback_frame();
        assert_eq!(
            (game.actor_hit_flash_ticks[0], game.boss_hit_flash_ticks),
            (20, 20)
        );
    }
    next_feedback_frame(&mut game, 8);
    assert_eq!(game.ticks, 3);
    assert_eq!(
        (game.actor_hit_flash_ticks[0], game.boss_hit_flash_ticks),
        (12, 12)
    );
    next_feedback_frame(&mut game, 2);
    assert_eq!(
        (game.actor_hit_flash_ticks[0], game.boss_hit_flash_ticks),
        (10, 10)
    );
    for _ in 0..3 {
        game.prepare_damage_feedback_frame();
        assert_eq!(
            (game.actor_hit_flash_ticks[0], game.boss_hit_flash_ticks),
            (10, 10)
        );
    }
    next_feedback_frame(&mut game, 10);
    assert_eq!(
        (game.actor_hit_flash_ticks[0], game.boss_hit_flash_ticks),
        (0, 0)
    );
}

#[test]
fn flashes_clear_on_slot_reuse_respawn_run_reset_and_stage_transition() {
    let bank = test_bank(0);
    let mut game = running_at(0);
    game.ticks = 300;
    game.damage_feedback_tick = 296;
    game.actor_hit_flash_ticks[0] = 20;
    game.actor_hit_flash_ticks[1] = 6;
    game.spawn_actor(
        Spawn {
            kind: assets::FIGHTER,
            ..Spawn::default()
        },
        0,
        &bank,
    );
    assert_eq!(
        (game.actor_hit_flash_ticks[0], game.actor_hit_flash_ticks[1]),
        (0, 6)
    );
    assert_eq!(
        game.damage_feedback_tick, 296,
        "slot reuse must not reset the shared clock"
    );
    for reset in [Game::respawn, Game::reset_run, Game::reset_in_place] {
        game.actor_hit_flash_ticks = [20; ACTORS];
        game.boss_hit_flash_ticks = 20;
        game.ticks = 400;
        game.damage_feedback_tick = 350;
        reset(&mut game);
        assert_eq!(game.actor_hit_flash_ticks, [0; ACTORS]);
        assert_eq!(game.boss_hit_flash_ticks, 0);
        assert_eq!(game.damage_feedback_tick, game.ticks);
    }
    game.reset_scene(Mode::Playing, 0, SECTION_TICKS - 1);
    game.actor_hit_flash_ticks[0] = 20;
    game.boss_hit_flash_ticks = 20;
    let bytes = empty_segment(0);
    game.tick(&Segment::new(&bytes).unwrap(), &bank);
    assert_eq!(game.mode, Mode::Transition);
    assert_eq!(game.actor_hit_flash_ticks, [0; ACTORS]);
    assert_eq!(game.boss_hit_flash_ticks, 0);
    assert_eq!(game.damage_feedback_tick, game.ticks);
}

type Triangle = ([Vec3; 3], u8, u8);

fn capture(game: &Game, bank: &AssetBank<'_>, limit: u16) -> std::vec::Vec<Triangle> {
    #[derive(Default)]
    struct Capture(std::vec::Vec<Triangle>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }
    let mut capture = Capture::default();
    let mut budget = SceneBudget {
        sink: &mut capture,
        count: 0,
        limit,
        camera_y: 17,
        color_override: 0,
        extra_flags: 0,
    };
    game.render_combat(&mut budget, bank);
    assert_eq!(
        budget.color_override, 0,
        "combat must restore normal colors"
    );
    capture.0
}

fn assert_white_geometry(normal: &[Triangle], flashed: &[Triangle]) {
    assert!(!normal.is_empty());
    assert_eq!(normal.len(), flashed.len());
    for (before, after) in normal.iter().zip(flashed) {
        assert_eq!((before.0, before.2), (after.0, after.2));
        assert_eq!(
            after.1, 1,
            "all target faces use the player's white palette color"
        );
    }
}

#[test]
fn enemy_meshes_and_proxies_only_change_the_damaged_targets_colors() {
    for stage in [0, 1, 2, 5, 8, 12] {
        let bank = test_bank(stage);
        for kind in (1..=bank.enemy_count()).chain([assets::ROCK]) {
            for (z, limit) in [(700, 200), (1800, 200), (700, 4)] {
                let mut game = running_at(stage);
                game.actors[0] = target(kind, -200, z);
                if kind == assets::ROCK {
                    game.actors[0].flags = assets::SMALL_DEBRIS;
                }
                let normal_target = capture(&game, &bank, limit);
                game.actors[1] = target(assets::FIGHTER, 200, z);
                let normal = capture(&game, &bank, limit);
                game.actor_hit_flash_ticks[0] = 21;
                game.prepare_damage_feedback_frame();
                let flashed = capture(&game, &bank, limit);
                assert_white_geometry(
                    &normal[..normal_target.len()],
                    &flashed[..normal_target.len()],
                );
                assert_eq!(
                    &normal[normal_target.len()..],
                    &flashed[normal_target.len()..]
                );
                next_feedback_frame(&mut game, 10);
                assert_eq!(game.actor_hit_flash_ticks[0], 10);
                assert_eq!(capture(&game, &bank, limit), normal);
                next_feedback_frame(&mut game, 10);
                assert_eq!(game.actor_hit_flash_ticks[0], 0);
                assert_eq!(capture(&game, &bank, limit), normal);
            }
        }
    }
}

#[test]
fn whole_boss_mesh_turrets_and_exposed_core_flash_without_recoloring_other_objects() {
    for stage in [3, 7, 11, 16, 17] {
        let bank = test_bank(stage);
        for (left, right) in [(40, 40), (0, 40), (40, 0), (0, 0)] {
            let mut game = running_at(stage);
            game.boss_started = true;
            game.boss_z = 680;
            game.boss_left = left;
            game.boss_right = right;
            let boss_count = capture(&game, &bank, 300).len();
            game.shots[0] = Shot {
                kind: 1,
                damage: 1,
                pos: Vec3 { x: 0, y: 0, z: 300 },
                ..Shot::EMPTY
            };
            game.actors[0] = target(assets::FIGHTER, 200, 700);
            let normal = capture(&game, &bank, 300);
            game.boss_hit_flash_ticks = 21;
            game.prepare_damage_feedback_frame();
            let flashed = capture(&game, &bank, 300);
            assert_white_geometry(&normal[..boss_count], &flashed[..boss_count]);
            assert_eq!(&normal[boss_count..], &flashed[boss_count..]);
            next_feedback_frame(&mut game, 10);
            assert_eq!(game.boss_hit_flash_ticks, 10);
            let recovering = capture(&game, &bank, 300);
            // The exposed core animates its normal palette with simulation time.
            // Compare recovery against an idle boss at the same instant.
            game.boss_hit_flash_ticks = 0;
            assert_eq!(recovering, capture(&game, &bank, 300));
            game.boss_hit_flash_ticks = 10;
            next_feedback_frame(&mut game, 10);
            assert_eq!(game.boss_hit_flash_ticks, 0);
            assert_eq!(capture(&game, &bank, 300), normal);
        }
    }
}
