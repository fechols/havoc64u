use super::tests::{empty_segment, test_bank};
use super::*;

fn select_story(enabled: bool) -> Game {
    let mut game = Game::new();
    if enabled {
        game.toggle_story_mode();
    }
    game
}

fn sample(game: &mut Game, bits: u8, ms: u16) {
    game.sample_input(InputSample {
        bits,
        pressed: 0,
        ms,
    });
}

fn tick(game: &mut Game) {
    let stage = game.segment_index();
    let bytes = empty_segment(stage);
    game.tick(&Segment::new(&bytes).unwrap(), &test_bank(stage));
}

#[test]
fn title_story_toggle_changes_only_selection_and_menu_activity() {
    fn title() -> Game {
        let mut game = Game::new();
        game.menu_ticks = ATTRACT_TICKS - 1;
        game.menu_music_galactic = true;
        game.controls.now = 12_345;
        game.controls.fire_armed = true;
        game.player.x = 37;
        game.score = 987;
        game.encounter_seed = 91;
        game
    }
    let mut game = title();
    let mut expected = title();
    assert!(!game.story_mode);
    assert!(!game.hud().story_mode);
    for selected in [true, false, true] {
        game.toggle_story_mode();
        expected.story_mode = selected;
        expected.menu_activity = true;
        assert_eq!(game, expected, "toggle must preserve every other field");
        assert_eq!(game.hud().story_mode, selected);
    }
}

#[test]
fn story_toggle_is_ignored_in_every_non_title_mode() {
    for mode in [
        Mode::Playing,
        Mode::Transition,
        Mode::Respawning,
        Mode::Victory,
        Mode::GameOver,
        Mode::Attract,
    ] {
        for selected in [false, true] {
            for activity in [false, true] {
                let mut game = select_story(selected);
                let mut expected = select_story(selected);
                for value in [&mut game, &mut expected] {
                    value.mode = mode;
                    value.menu_ticks = 127;
                    value.menu_activity = activity;
                    value.lives = 2;
                    value.controls.bits = FIRE | LEFT;
                    value.controls.start_pending = true;
                }
                game.toggle_story_mode();
                assert_eq!(game, expected, "ignored toggle changed {mode:?}");
                assert_eq!(game.hud().story_mode, selected);
            }
        }
    }
}

#[test]
fn queued_start_uses_story_selection_when_the_fire_edge_is_consumed() {
    for original in [false, true] {
        let mut game = select_story(original);
        sample(&mut game, FIRE, 20);
        assert!(game.controls.start_pending);
        game.toggle_story_mode();
        assert!(
            game.controls.start_pending,
            "selection must not consume Fire"
        );
        tick(&mut game);
        assert_eq!(game.mode, Mode::Playing);
        assert_eq!(game.story_mode, !original);
        assert_eq!(game.lives, if original { 3 } else { 99 });
        assert_eq!(game.controls.bits, FIRE);
        assert!(!game.controls.start_pending);
    }
}

#[test]
fn held_fire_does_not_start_a_run_when_story_selection_changes() {
    let mut game = Game::new();
    game.controls.bits = FIRE;
    sample(&mut game, FIRE, 20);
    game.toggle_story_mode();
    tick(&mut game);
    assert_eq!(game.mode, Mode::Title);
    assert_eq!(game.menu_ticks, 0);
    assert!(game.story_mode);
    sample(&mut game, 0, 40);
    sample(&mut game, FIRE, 60);
    tick(&mut game);
    assert_eq!(game.mode, Mode::Playing);
    assert_eq!(game.lives, 99);
}

#[test]
fn toggling_resets_title_inactivity_and_selection_survives_the_attract_tour() {
    let mut game = Game::new();
    game.menu_ticks = ATTRACT_TICKS - 1;
    game.toggle_story_mode();
    tick(&mut game);
    assert_eq!((game.mode, game.menu_ticks), (Mode::Title, 0));
    assert!(!game.menu_activity);
    for _ in 0..ATTRACT_TICKS - 1 {
        tick(&mut game);
    }
    assert_eq!(game.mode, Mode::Title);
    tick(&mut game);
    assert_eq!(game.mode, Mode::Attract);
    assert!(game.story_mode);
    for next in [1, 6, 10, 12, 14, 16] {
        game.menu_ticks = ATTRACT_TICKS - 1;
        tick(&mut game);
        assert_eq!((game.mode, game.stage_id), (Mode::Attract, next));
        assert!(game.story_mode);
    }
    game.menu_ticks = ATTRACT_TICKS - 1;
    tick(&mut game);
    assert_eq!(game.mode, Mode::Title);
    assert!(game.hud().story_mode);
    sample(&mut game, FIRE, 1000);
    tick(&mut game);
    assert_eq!((game.mode, game.lives), (Mode::Playing, 99));
}

#[test]
fn story_selection_survives_scene_resets_but_a_new_game_uses_normal_mode() {
    for selected in [false, true] {
        let mut game = select_story(selected);
        for mode in [
            Mode::Playing,
            Mode::Respawning,
            Mode::Transition,
            Mode::Victory,
            Mode::GameOver,
            Mode::Attract,
            Mode::Title,
        ] {
            game.reset_scene(mode, 7, 1200);
            assert_eq!(game.story_mode, selected, "reset to {mode:?}");
            assert_eq!(game.hud().story_mode, selected);
        }
        game.reset_run();
        assert_eq!(game.lives, if selected { 99 } else { 3 });
        assert_eq!(game.story_mode, selected);
        game.reset_in_place();
        assert_eq!(
            game,
            Game::new(),
            "fresh initialization clears prior selection"
        );
    }
    assert!(!Game::new().story_mode);
    assert!(!Game::new().hud().story_mode);
}

#[test]
fn every_boss_checkpoint_rewards_one_life_up_to_the_selected_cap() {
    for selected in [false, true] {
        let cap = if selected { 99 } else { 3 };
        for stage in [3, 7, 11, 16] {
            for lives in 1..=cap {
                let mut game = select_story(selected);
                game.reset_run();
                game.stage_id = stage;
                game.stage_ticks = SECTION_TICKS - 1;
                game.stage_info = assets::preview_stage_info(stage);
                game.boss_started = true;
                game.boss_defeated = true;
                game.lives = lives;
                game.shield = 17;
                game.bombs = 1;
                game.score = 5000;
                tick(&mut game);
                let rewarded = (lives + 1).min(cap);
                assert_eq!((game.mode, game.lives), (Mode::Transition, rewarded));
                assert_eq!((game.shield, game.bombs, game.score), (17, 1, 5000));
                assert_eq!(game.story_mode, selected);
                tick(&mut game);
                assert_eq!(game.lives, rewarded, "reward must not repeat during bridge");
                game.transition_ticks = TRANSITION_TICKS - 1;
                tick(&mut game);
                assert_eq!(
                    (game.mode, game.stage_id, game.lives),
                    (Mode::Playing, stage + 1, rewarded)
                );
                assert_eq!(game.story_mode, selected);
            }
        }
    }
}

#[test]
fn every_available_life_decrements_and_checkpoint_respawn_keeps_the_remainder() {
    for selected in [false, true] {
        let cap = if selected { 99 } else { 3 };
        for stage in [0, 7, 17] {
            for lives in 1..=cap {
                let mut game = select_story(selected);
                game.reset_run();
                game.stage_id = stage;
                game.stage_info = assets::preview_stage_info(stage);
                game.stage_ticks = 1234;
                game.invulnerability = 0;
                game.lives = lives;
                game.damage_player(100);
                assert_eq!(game.lives, lives - 1);
                assert_eq!(game.story_mode, selected);
                if lives == 1 {
                    assert_eq!(game.mode, Mode::GameOver);
                    continue;
                }
                assert_eq!(game.mode, Mode::Respawning);
                for _ in 0..75 {
                    tick(&mut game);
                }
                assert_eq!(
                    (game.mode, game.stage_id, game.stage_ticks),
                    (Mode::Playing, stage, 0)
                );
                assert_eq!(
                    game.lives,
                    lives - 1,
                    "checkpoint must not refill starting lives"
                );
                assert_eq!(game.story_mode, selected);
            }
        }
    }
}

#[test]
fn game_over_requires_a_fresh_fire_edge_and_restarts_with_the_selected_lives() {
    for selected in [false, true] {
        let mut game = select_story(selected);
        game.reset_run();
        game.stage_id = 7;
        game.stage_info = assets::preview_stage_info(7);
        game.stage_ticks = 1234;
        game.score = 2000;
        game.lives = 1;
        game.invulnerability = 0;
        sample(&mut game, FIRE, 20);
        game.damage_player(100);
        assert_eq!((game.mode, game.lives), (Mode::GameOver, 0));
        assert!(!game.controls.start_pending);
        sample(&mut game, FIRE, 40);
        tick(&mut game);
        assert_eq!(game.mode, Mode::GameOver);
        sample(&mut game, 0, 60);
        sample(&mut game, FIRE, 80);
        tick(&mut game);
        assert_eq!(
            (game.mode, game.stage_id, game.stage_ticks),
            (Mode::Playing, 0, 0)
        );
        assert_eq!(game.lives, if selected { 99 } else { 3 });
        assert_eq!((game.score, game.shield, game.bombs), (0, 100, 3));
        assert_eq!(game.story_mode, selected);
    }
}

#[test]
fn victory_return_and_the_following_run_preserve_story_selection() {
    for selected in [false, true] {
        let mut game = select_story(selected);
        game.reset_run();
        game.stage_id = assets::SEGMENT_COUNT - 1;
        game.stage_info = assets::preview_stage_info(game.stage_id);
        game.stage_ticks = SECTION_TICKS - 1;
        game.boss_started = true;
        game.boss_defeated = true;
        game.boss_left = 0;
        game.boss_right = 0;
        game.boss_core = 0;
        game.lives = 2;
        tick(&mut game);
        assert_eq!(game.mode, Mode::Victory);
        assert_eq!(game.story_mode, selected);
        game.menu_ticks = crate::victory::REVEAL_TICKS;
        sample(&mut game, FIRE, 20);
        tick(&mut game);
        assert_eq!(game.mode, Mode::Title);
        assert_eq!(game.story_mode, selected);
        sample(&mut game, FIRE, 40);
        tick(&mut game);
        assert_eq!(
            game.mode,
            Mode::Title,
            "held victory-dismiss Fire is consumed"
        );
        sample(&mut game, 0, 60);
        sample(&mut game, FIRE, 80);
        tick(&mut game);
        assert_eq!(game.mode, Mode::Playing);
        assert_eq!(game.lives, if selected { 99 } else { 3 });
        assert_eq!(game.hud().story_mode, selected);
    }
}
