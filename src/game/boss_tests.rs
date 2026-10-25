use super::*;

fn encounter() -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, 17, SPACE_BOSS_START_TICKS);
    game.boss_started = true;
    game.bosses[0].pos.z = 680;
    game
}

fn finish_wave(game: &mut Game) {
    for boss in &mut game.bosses {
        boss.left = 0;
        boss.right = 0;
        boss.core = 0;
    }
    game.check_boss_death();
}

fn settle(game: &mut Game) {
    while game.boss_split_ticks != 0 {
        game.advance_boss_split();
    }
}

fn phase(phase: u8) -> Game {
    let mut game = encounter();
    for _ in 0..phase {
        finish_wave(&mut game);
        settle(&mut game);
    }
    game
}

fn hp(boss: BossInstance) -> (u8, u8, u8) {
    (boss.left, boss.right, boss.core)
}

fn shoot(game: &mut Game, bank: &AssetBank<'_>, index: usize, part: u8, damage: u8) {
    let target = game.bosses[index].part_position(part, game.boss_divisor());
    game.shots[0] = Shot {
        pos: Vec3 {
            z: target.z - 48,
            ..target
        },
        kind: 1,
        damage,
        ..Shot::EMPTY
    };
    game.update_shots(bank);
    assert_eq!(game.shots[0].kind, 0);
}

fn last_core_encounter(stage: u8, wave: u8, lives: u8) -> Game {
    let mut game = if stage == 17 {
        phase(wave)
    } else {
        let mut game = Game::new();
        game.reset_scene(Mode::Playing, stage, SECTION_TICKS);
        game.boss_started = true;
        game.bosses[0].pos.z = 680;
        game
    };
    let last = game.boss_count() - 1;
    for (index, boss) in game.bosses.iter_mut().enumerate() {
        boss.left = 0;
        boss.right = 0;
        boss.core = u8::from(index == last);
    }
    game.shield = 6;
    game.bombs = 1;
    game.lives = lives;
    game.score = 77;
    game.invulnerability = 0;
    game.roll_ticks = 0;
    game.presentation_event = PresentationEvent::None;
    while game.pop_audio().is_some() {}
    game
}

fn core_shot(game: &Game, index: usize, kind: u8) -> Shot {
    let target = game.bosses[index].part_position(2, game.boss_divisor());
    Shot {
        pos: Vec3 { z: target.z - if kind == 3 { 32 } else { 48 }, ..target },
        kind,
        damage: 4,
        ..Shot::EMPTY
    }
}

fn lethal_hostile_shot(game: &Game) -> Shot {
    Shot {
        pos: Vec3 { z: PLAYER_Z + 18, ..game.player },
        kind: 2,
        damage: 6,
        ..Shot::EMPTY
    }
}

#[test]
fn final_core_kills_retire_later_hostile_shots_before_they_can_take_a_life() {
    for (stage, wave) in [(3, 0), (7, 0), (11, 0), (16, 0), (17, 0), (17, 1), (17, 2)] {
        let bytes = enemies::authored::model_bank(stage);
        let bank = AssetBank::new(&bytes).unwrap();
        for lives in [1, 2] {
            for kind in [1, 3] {
                let mut game = last_core_encounter(stage, wave, lives);
                game.shots[0] = core_shot(&game, game.boss_count() - 1, kind);
                game.shots[1] = lethal_hostile_shot(&game);
                game.shots[SHOTS - 1] = lethal_hostile_shot(&game);
                game.update_shots(&bank);
                assert_eq!((game.mode, game.lives), (Mode::Playing, lives),
                    "stage {stage}, wave {wave}, lives {lives}, projectile {kind}");
                assert_eq!((game.shield, game.bombs), (100, 3));
                assert_eq!(game.presentation_event, PresentationEvent::None);
                assert_eq!(game.invulnerability, 0);
                assert!(game.shots.iter().all(|shot| shot.kind == 0));
                assert_eq!(game.pop_audio().unwrap().effect, Effect::BossExplosion);
                assert!(game.pop_audio().is_none());
                let splitting = stage == 17 && wave < 2;
                assert_eq!(game.boss_defeated, !splitting);
                assert_eq!(game.boss_phase, wave + u8::from(splitting));
                assert_eq!(game.boss_split_ticks, if splitting { 50 } else { 0 });
                let score = if splitting { 77 } else { 5077 };
                assert_eq!(game.score, score);
                game.shield = 73;
                game.bombs = 2;
                game.update_shots(&bank);
                assert_eq!((game.shield, game.bombs, game.score), (73, 2, score));
                assert!(game.pop_audio().is_none());
            }
        }
    }
}

#[test]
fn hostile_hits_before_the_final_core_kill_keep_their_existing_death_result() {
    for (stage, wave) in [(3, 0), (17, 0), (17, 1), (17, 2)] {
        let bytes = enemies::authored::model_bank(stage);
        let bank = AssetBank::new(&bytes).unwrap();
        for lives in [1, 2] {
            let mut game = last_core_encounter(stage, wave, lives);
            game.shots[0] = lethal_hostile_shot(&game);
            game.shots[1] = core_shot(&game, game.boss_count() - 1, 1);
            game.update_shots(&bank);
            assert_eq!(game.lives, lives - 1);
            assert_eq!(game.mode, if lives == 1 { Mode::GameOver } else { Mode::Respawning });
        }
    }
}

#[test]
fn destroying_one_child_does_not_retire_shots_while_another_core_lives() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    for wave in [1, 2] {
        let mut game = last_core_encounter(17, wave, 2);
        game.bosses[0].core = 1;
        game.shots[0] = core_shot(&game, 0, 1);
        game.shots[1] = lethal_hostile_shot(&game);
        game.update_shots(&bank);
        assert_eq!((game.bosses[0].core, game.bosses[game.boss_count() - 1].core), (0, 1));
        assert_eq!((game.mode, game.lives), (Mode::Respawning, 1));
        assert_eq!((game.boss_phase, game.boss_split_ticks, game.score), (wave, 0, 77));
        assert!(!game.boss_defeated);
    }
}

#[test]
fn later_friendly_projectiles_cannot_damage_newly_split_guardians() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    for wave in [0, 1] {
        let mut game = last_core_encounter(17, wave, 2);
        game.shots[0] = core_shot(&game, game.boss_count() - 1, 1);
        // Aim at a reactor in the next wave's initial parent position.
        let parent = game.bosses[0];
        let target = parent.part_position(0, 1 << (wave + 2));
        game.shots[1] = Shot {
            pos: Vec3 { z: target.z - 48, ..target },
            kind: 1,
            damage: 255,
            ..Shot::EMPTY
        };
        game.update_shots(&bank);
        assert_eq!((game.boss_phase, game.boss_split_ticks), (wave + 1, 50));
        let health = (BOSS_SIDE_HP >> (wave + 1), BOSS_SIDE_HP >> (wave + 1), BOSS_CORE_HP >> (wave + 1));
        assert!(game.bosses[..game.boss_count()].iter().all(|&boss| hp(boss) == health));
        assert!(game.shots.iter().all(|shot| shot.kind == 0));
    }
}

#[test]
fn guardian_waves_have_exact_original_half_and_quarter_health() {
    let mut game = encounter();
    let music = game.music_state();
    for (phase, count, health) in [
        (0, 1, (40, 40, 120)),
        (1, 2, (20, 20, 60)),
        (2, 4, (10, 10, 30)),
    ] {
        assert_eq!(game.boss_phase, phase);
        assert_eq!(game.boss_count(), count);
        assert!(!game.boss_defeated);
        assert_eq!(game.hud().boss_shield, 100);
        assert_eq!(game.music_state(), music);
        assert_eq!(game.score, 0);
        assert!(game.bosses[..count].iter().all(|&boss| hp(boss) == health));
        assert!(game.bosses[count..].iter().all(|boss| boss.core == 0));
        let total: u16 = game.bosses[..count]
            .iter()
            .map(|boss| u16::from(boss.left) + u16::from(boss.right) + u16::from(boss.core))
            .sum();
        assert_eq!(total, 200);
        if phase < 2 {
            finish_wave(&mut game);
            assert_eq!(game.boss_split_ticks, 50);
            settle(&mut game);
        }
    }
}

#[test]
fn each_child_requires_its_own_two_reactors_before_core_damage() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut game = phase(2);
    game.bosses[0].left = 0;
    game.bosses[0].right = 0;
    game.bosses[1].left = 0;
    shoot(&mut game, &bank, 0, 2, 4);
    shoot(&mut game, &bank, 1, 2, 4);
    assert_eq!(hp(game.bosses[0]), (0, 0, 26));
    assert_eq!(hp(game.bosses[1]), (0, 10, 30));
    assert_eq!((game.bosses[0].flash, game.bosses[1].flash), (21, 0));
    shoot(&mut game, &bank, 1, 1, 10);
    assert_eq!(hp(game.bosses[1]), (0, 0, 30));
    shoot(&mut game, &bank, 1, 2, 4);
    assert_eq!(hp(game.bosses[1]), (0, 0, 26));
    assert_eq!(hp(game.bosses[2]), (10, 10, 30));
    assert_eq!(hp(game.bosses[3]), (10, 10, 30));
}

#[test]
fn four_children_wait_for_both_medium_guardians_to_die() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut game = phase(1);
    for boss in &mut game.bosses[..2] {
        boss.left = 0;
        boss.right = 0;
        boss.core = 4;
    }
    game.shield = 12;
    game.bombs = 1;
    let parents = [game.bosses[0].pos, game.bosses[1].pos];
    shoot(&mut game, &bank, 0, 2, 4);
    assert_eq!((game.boss_phase, game.boss_split_ticks), (1, 0));
    assert_eq!((game.bosses[0].core, game.bosses[1].core), (0, 4));
    assert_eq!((game.shield, game.bombs, game.score), (12, 1, 0));
    assert!(!game.boss_defeated);
    shoot(&mut game, &bank, 1, 2, 4);
    assert_eq!((game.boss_phase, game.boss_split_ticks), (2, 50));
    assert_eq!((game.shield, game.bombs, game.score), (100, 3, 0));
    for (index, boss) in game.bosses.iter().enumerate() {
        assert_eq!(boss.pos, parents[index & 1]);
        assert_eq!(hp(*boss), (10, 10, 30));
    }
    settle(&mut game);
    assert_eq!(
        game.bosses.map(|boss| boss.pos),
        [
            Vec3 {
                x: -55,
                y: -40,
                z: 500
            },
            Vec3 {
                x: 55,
                y: -40,
                z: 500
            },
            Vec3 {
                x: -55,
                y: 40,
                z: 500
            },
            Vec3 {
                x: 55,
                y: 40,
                z: 500
            },
        ]
    );
}

#[test]
fn splitting_pauses_combat_for_fifty_ticks_but_keeps_flight_and_refills_only_once() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let segment_bytes = enemies::authored::segment_bank(17);
    let segment = Segment::new(&segment_bytes).unwrap();
    let mut game = encounter();
    game.spawn_cursor = segment.spawn_count();
    game.repeat_spawn_cursor = segment.spawn_count();
    game.shield = 9;
    game.bombs = 1;
    game.score = 1234;
    game.lives = 2;
    game.shots[0].kind = 2;
    game.shots[1].kind = 3;
    game.missile_targets.fill(15);
    game.actors[0].kind = assets::FIGHTER;
    game.actors[0].hp = 255;
    game.bosses[0].left = 0;
    game.bosses[0].right = 0;
    game.bosses[0].core = 8;
    game.controls.bits = FIRE;
    game.controls.flags |= BOMB_PENDING;
    let transition_stage_ticks = game.stage_ticks;
    game.tick(&segment, &bank);
    // The initiating frame clears its freshly fired laser and starts a full
    // fifty-tick pause; it must not consume the first interpolation tick.
    assert_eq!(game.boss_split_ticks, 50);
    assert_eq!(game.stage_ticks, transition_stage_ticks);
    assert_eq!(
        (game.shield, game.bombs, game.score, game.lives),
        (100, 3, 1234, 2)
    );
    assert!(game.shots.iter().all(|shot| shot.kind == 0));
    assert!(game.actors.iter().all(|actor| actor.kind == 0));
    assert!(game.missile_targets.iter().all(|&target| target == 0));
    game.shield = 73;
    game.bombs = 2;
    let stage_ticks = game.stage_ticks;
    let start = game.player;
    for remaining in (1..=50).rev() {
        assert_eq!(game.boss_split_ticks, remaining);
        let target = game.bosses[0].part_position(0, game.boss_divisor());
        assert!(game
            .hit_boss(
                target.z - 48,
                &Shot {
                    pos: target,
                    kind: 1,
                    damage: 255,
                    ..Shot::EMPTY
                }
            )
            .is_none());
        game.controls.bits = RIGHT | FIRE;
        game.controls.flags |= SHOT_PENDING | BOMB_PENDING;
        game.tick(&segment, &bank);
        assert_eq!(game.stage_ticks, stage_ticks);
        assert!(game.shots.iter().all(|shot| shot.kind == 0));
        assert_eq!(
            (game.shield, game.bombs, game.score, game.lives),
            (73, 2, 1234, 2)
        );
        assert_eq!(hp(game.bosses[0]), (20, 20, 60));
        assert_eq!(hp(game.bosses[1]), (20, 20, 60));
    }
    assert_eq!(game.boss_split_ticks, 0);
    assert!(game.player.x > start.x);
    assert_eq!(
        game.bosses[0].pos,
        Vec3 {
            x: -74,
            y: 0,
            z: 500
        }
    );
    assert_eq!(
        game.bosses[1].pos,
        Vec3 {
            x: 74,
            y: 0,
            z: 500
        }
    );
    game.tick(&segment, &bank);
    assert_eq!(game.stage_ticks, stage_ticks + 1);
    assert!(game.shots.iter().any(|shot| shot.kind == 1));
}

#[test]
fn both_child_phases_suppress_authored_and_repeated_escorts() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let segment_bytes = enemies::authored::segment_bank(17);
    let segment = Segment::new(&segment_bytes).unwrap();
    for current_phase in 0..=2 {
        let mut game = phase(current_phase);
        game.spawn_cursor = 0;
        game.repeat_spawn_cursor = 0;
        game.spawn_encounters(&segment, &bank);
        assert!(game.spawn_cursor > 0 && game.repeat_spawn_cursor > 0);
        assert_eq!(game.actors.iter().any(|actor| actor.kind != 0), current_phase == 0,
            "phase {current_phase} must consume delayed original and repeat records without restoring escorts");
    }
}

#[test]
fn a_bomb_killing_both_medium_cores_cannot_damage_the_new_wave() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut game = phase(1);
    for boss in &mut game.bosses[..2] {
        boss.left = 0;
        boss.right = 0;
        boss.core = 8;
    }
    game.bomb(&bank);
    assert_eq!(
        (
            game.boss_phase,
            game.boss_split_ticks,
            game.bombs,
            game.score
        ),
        (2, 50, 3, 0)
    );
    assert!(game.bosses.iter().all(|&boss| hp(boss) == (10, 10, 30)));
    let before = game.bosses;
    game.bomb(&bank);
    game.check_boss_death();
    assert_eq!(game.bosses, before);
    assert_eq!((game.boss_split_ticks, game.bombs, game.score), (50, 3, 0));
}

#[test]
fn damage_flashes_recover_independently_for_each_child() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut game = phase(2);
    shoot(&mut game, &bank, 1, 0, 1);
    assert_eq!(game.bosses.map(|boss| boss.flash), [0, 21, 0, 0]);
    game.prepare_damage_feedback_frame();
    game.ticks += 5;
    game.prepare_damage_feedback_frame();
    shoot(&mut game, &bank, 2, 1, 1);
    assert_eq!(game.bosses.map(|boss| boss.flash), [0, 15, 21, 0]);
    game.ticks += 5;
    game.prepare_damage_feedback_frame();
    assert_eq!(game.bosses.map(|boss| boss.flash), [0, 10, 20, 0]);
    game.ticks += 10;
    game.prepare_damage_feedback_frame();
    assert_eq!(game.bosses.map(|boss| boss.flash), [0, 0, 10, 0]);
    shoot(&mut game, &bank, 2, 1, 1);
    shoot(&mut game, &bank, 1, 0, 1);
    assert_eq!(game.bosses.map(|boss| boss.flash), [0, 21, 10, 0]);
}

#[test]
fn respawning_during_either_child_phase_restores_the_original_guardian() {
    for current_phase in [1, 2] {
        for splitting in [false, true] {
            let mut game = phase(current_phase);
            game.score = 1234;
            game.boss_split_ticks = if splitting { 25 } else { 0 };
            game.bosses[0].core = 1;
            game.bosses[0].flash = 21;
            game.shield = 1;
            game.invulnerability = 0;
            game.damage_player(1);
            assert_eq!(game.mode, Mode::Respawning);
            game.respawn();
            assert_eq!(
                (game.boss_phase, game.boss_split_ticks, game.stage_ticks),
                (0, 0, 0)
            );
            assert!(!game.boss_started && !game.boss_defeated);
            assert_eq!(hp(game.bosses[0]), (40, 40, 120));
            assert_eq!(
                game.bosses[0].pos,
                Vec3 {
                    x: 0,
                    y: 0,
                    z: 1800
                }
            );
            assert_eq!(game.bosses.map(|boss| boss.flash), [0; 4]);
            assert!(game.bosses[1..].iter().all(|boss| boss.core == 0));
            assert_eq!((game.score, game.lives), (1234, 2));
        }
    }
}

#[test]
fn only_the_last_small_core_awards_the_final_score_and_unlocks_victory() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    let segment_bytes = enemies::authored::segment_bank(17);
    let segment = Segment::new(&segment_bytes).unwrap();
    let mut game = phase(2);
    game.stage_ticks = SECTION_TICKS;
    game.shield = 19;
    game.bombs = 1;
    game.lives = 2;
    game.score = 77;
    for boss in &mut game.bosses {
        boss.left = 0;
        boss.right = 0;
        boss.core = 4;
    }
    for index in 0..3 {
        shoot(&mut game, &bank, index, 2, 4);
        assert!(!game.boss_defeated);
        assert_eq!(game.mode, Mode::Playing);
        assert_eq!(
            (game.score, game.shield, game.bombs, game.lives),
            (77, 19, 1, 2)
        );
    }
    shoot(&mut game, &bank, 3, 2, 4);
    assert!(game.boss_defeated);
    assert_eq!(
        (game.score, game.shield, game.bombs, game.lives),
        (5077, 100, 3, 2)
    );
    assert_eq!(game.hud().boss_shield, 0);
    game.shield = 71;
    game.bombs = 2;
    for _ in 0..3 {
        game.check_boss_death();
    }
    assert_eq!(
        (game.score, game.shield, game.bombs, game.lives),
        (5077, 71, 2, 2)
    );
    game.tick(&segment, &bank);
    assert_eq!(game.mode, Mode::Victory);
    game.tick(&segment, &bank);
    assert_eq!(game.score, 5077);
}
