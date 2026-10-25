use super::tests::{empty_segment, test_bank};
use super::*;

fn running(stage: u8) -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, stage, 0);
    game.invulnerability = 0;
    game
}

fn sample(game: &mut Game, bits: u8) {
    game.sample_input(InputSample {
        bits,
        pressed: 0,
        ms: game.controls.now.wrapping_add(20),
    });
}

fn step(game: &mut Game, bits: u8) {
    sample(game, bits);
    let bytes = empty_segment(game.stage_id);
    let segment = Segment::new(&bytes).unwrap();
    game.tick(&segment, &test_bank(game.stage_id));
}

#[test]
fn short_tap_gives_exactly_two_seconds_then_four_seconds_and_never_queues() {
    let mut game = running(0);
    sample(&mut game, BOOST);
    sample(&mut game, 0); // A tap entirely between simulation ticks survives.
    for tick in 0..100 {
        step(&mut game, if tick & 1 == 0 { BOOST } else { 0 });
        assert!(game.boost_active(), "active tick {tick}");
        assert_eq!(game.boost_cycle, 300 - tick);
    }
    assert_eq!(game.stage_ticks, 200);
    assert_eq!(game.extra_travel_ticks, 100);
    for tick in 0..200 {
        // Including a fresh press on the final cooling tick.
        step(&mut game, if tick & 1 == 1 { BOOST } else { 0 });
        assert!(!game.boost_active(), "cooldown tick {tick}");
        assert_eq!(game.boost_cycle, 200 - tick);
    }
    for _ in 0..10 {
        step(&mut game, BOOST);
        assert_eq!(game.boost_cycle, 0, "holding cannot restart");
    }
    step(&mut game, 0);
    step(&mut game, BOOST);
    assert_eq!(game.boost_cycle, 300);
}

#[test]
fn boost_doubles_travel_and_dust_without_accelerating_weapons_or_rolls() {
    let mut normal = running(0);
    let mut boosted = running(0);
    normal.controls.roll_pending = 1;
    boosted.controls.roll_pending = 1;
    for tick in 0..50 {
        step(&mut normal, FIRE | UP | RIGHT);
        step(&mut boosted, FIRE | UP | RIGHT | BOOST);
        assert_eq!(boosted.ticks, normal.ticks);
        assert_eq!(boosted.shot_cooldown, normal.shot_cooldown);
        assert_eq!(boosted.roll_ticks, normal.roll_ticks);
        assert_eq!(boosted.roll_cooldown, normal.roll_cooldown);
        assert_eq!(boosted.bombs, normal.bombs);
        assert_eq!(boosted.stage_ticks, normal.stage_ticks * 2);
        assert_eq!(boosted.travel_ticks(), normal.travel_ticks() * 2);
        if tick == 9 {
            assert!(boosted.player.x > normal.player.x);
            assert!(boosted.player.y > normal.player.y);
        }
    }
}

#[test]
fn pause_freezes_burst_and_discards_paused_presses() {
    let mut game = running(0);
    step(&mut game, BOOST);
    let frozen = (game.boost_cycle, game.ticks, game.travel_ticks());
    for bits in [0, BOOST, 0, BOOST] {
        game.sample_paused_input(bits, game.controls.now.wrapping_add(5000));
        assert_eq!((game.boost_cycle, game.ticks, game.travel_ticks()), frozen);
        assert_eq!(game.controls.flags & BOOST_PENDING, 0);
    }
    step(&mut game, BOOST);
    assert_eq!(game.boost_cycle, 299);
    game.boost_cycle = 0;
    game.controls.flags &= !BOOST_ACTIVE;
    game.sample_paused_input(0, 30_000);
    game.sample_paused_input(BOOST, 30_020);
    step(&mut game, BOOST);
    assert_eq!(game.boost_cycle, 0);
}

#[test]
fn crossings_preserve_burst_and_clamp_distance_before_checkpoint_handoff() {
    let mut game = running(0);
    game.stage_ticks = SECTION_TICKS - 1;
    step(&mut game, BOOST);
    assert_eq!((game.mode, game.stage_ticks, game.transition_ticks),
        (Mode::Transition, SECTION_TICKS, 0));
    assert_eq!(game.boost_cycle, 300);
    game.transition_ticks = TRANSITION_TICKS - 1;
    step(&mut game, 0);
    assert_eq!((game.mode, game.stage_id, game.stage_ticks, game.transition_ticks),
        (Mode::Playing, 1, 0, 0));
    assert_eq!(game.boost_cycle, 299);
    step(&mut game, 0);
    assert_eq!(game.stage_ticks, 2);
}

#[test]
fn boost_cannot_bypass_boss_gate_or_change_combat_separation() {
    let mut game = running(3);
    game.stage_ticks = SECTION_TICKS;
    game.boss_started = true;
    game.bosses[0].pos.z = 680;
    step(&mut game, BOOST);
    assert_eq!((game.mode, game.stage_ticks, game.bosses[0].pos.z), (Mode::Playing, SECTION_TICKS, 680));
    assert!(game.boost_active());
    assert_eq!(game.scenery_ticks(), SECTION_TICKS);
    assert_eq!(game.travel_ticks(), 2);
}

#[test]
fn boost_is_cancelled_on_death_and_requires_new_press_after_respawn() {
    let mut game = running(0);
    step(&mut game, BOOST);
    game.shield = 1;
    game.invulnerability = 0;
    game.damage_player(15);
    assert_eq!(game.mode, Mode::Respawning);
    assert_eq!(game.boost_cycle, 0);
    assert!(!game.boost_active());
    game.respawn();
    step(&mut game, BOOST);
    assert_eq!(game.boost_cycle, 0);
    step(&mut game, 0);
    step(&mut game, BOOST);
    assert_eq!(game.boost_cycle, 300);
    game.reset_run();
    assert_eq!((game.boost_cycle, game.extra_travel_ticks), (0, 0));
}

#[test]
fn menu_boost_only_wakes_attract_and_victory_clears_active_feedback() {
    let mut game = Game::new();
    step(&mut game, BOOST);
    assert_eq!(game.mode, Mode::Title);
    game.reset_scene(Mode::Attract, 0, 0);
    step(&mut game, 0);
    step(&mut game, BOOST);
    assert_eq!(game.mode, Mode::Title);
    assert_eq!(game.boost_cycle, 0);
    let mut game = running(17);
    game.stage_ticks = SECTION_TICKS - 1;
    game.boss_defeated = true;
    step(&mut game, BOOST);
    assert_eq!(game.mode, Mode::Victory);
    assert_eq!(game.boost_cycle, 0);
    assert!(!game.hud().boost_active);
}

#[test]
fn boosted_relative_motion_keeps_enemy_age_and_effect_lifetimes_in_real_ticks() {
    let bank = test_bank(0);
    let mut game = running(0);
    step(&mut game, BOOST);
    let spawn = Spawn { kind: assets::FIGHTER, distance: 0, x: 60, y: 20, flags: 0 };
    game.spawn_actor_at(spawn, 8, &bank, 1000);
    assert_eq!(game.actors[0].age, 0);
    assert_eq!(game.actors[0].pos.z, 992);
    let mut expected = game.actors[0];
    expected.age += 1;
    expected.pos.z -= 8;
    unsafe { enemies::enemy_lowlands(&mut expected, &game.player, 0); }
    let before = game.actors[0].pos.z;
    game.update_actors(&bank);
    assert_eq!(game.actors[0].pos, expected.pos);
    assert_eq!(game.actors[0].age, 1);
    assert_eq!(game.actors[0].approach_speed, (before - expected.pos.z) as u8);
    game.shots[0] = Shot { kind: 2, pos: Vec3 { x: 90, y: 0, z: 300 }, ..Shot::EMPTY };
    game.shots[1] = Shot { kind: 1, pos: Vec3 { x: -90, y: 0, z: 300 }, ..Shot::EMPTY };
    game.update_shots(&bank);
    assert_eq!((game.shots[0].pos.z, game.shots[1].pos.z), (274, 348));
    game.pickups[0] = Pickup { kind: 1, pos: Vec3 { x: 90, y: 0, z: 600 } };
    game.explosions[0] = Explosion { ticks: 18, color: 8, pos: Vec3 { x: 90, y: 0, z: 600 } };
    game.update_pickups();
    game.update_explosions();
    assert_eq!((game.pickups[0].pos.z, game.explosions[0].pos.z, game.explosions[0].ticks), (584, 584, 17));
}

#[test]
fn gauge_exhaust_and_travel_clock_follow_phase_and_wrap_safely() {
    let mut game = running(12);
    assert_eq!(game.hud().boost, 255);
    game.ticks = u16::MAX;
    game.extra_travel_ticks = u16::MAX;
    step(&mut game, BOOST);
    assert_eq!(game.travel_ticks(), 0);
    assert_eq!(game.hud().boost, 255);
    for _ in 0..50 { step(&mut game, 0); }
    assert_eq!(game.hud().boost, 127);
    let burning = game.exhaust_appearance();
    game.controls.flags &= !BOOST_ACTIVE;
    let normal = game.exhaust_appearance();
    assert_eq!(burning & 255, (normal & 255) * 2);
    assert!([1, 14].contains(&(burning >> 8)));
    game.boost_cycle = 200;
    assert_eq!(game.hud().boost, 0);
    game.boost_cycle = 100;
    assert_eq!(game.hud().boost, 127);
}

#[test]
fn boosted_tick_processes_every_crossed_encounter_without_extra_ai_age() {
    let mut game = running(0);
    let mut bytes = empty_segment(0);
    bytes[7] = 2;
    for (index, distance) in [8u16, 16].into_iter().enumerate() {
        let at = 16 + index * 8;
        bytes[at..at + 2].copy_from_slice(&distance.to_le_bytes());
        bytes[at + 2] = assets::FIGHTER;
    }
    sample(&mut game, BOOST);
    game.tick(&Segment::new(&bytes).unwrap(), &test_bank(0));
    assert_eq!(game.spawn_cursor, 2);
    assert_eq!(game.actors.iter().filter(|a| a.kind != 0).count(), 2);
    assert_eq!((game.actors[0].age, game.actors[1].age), (1, 1));
}

#[test]
fn boosted_projectiles_and_pickups_still_contact_the_player() {
    let mut game = running(0);
    step(&mut game, BOOST);
    game.shots[0] = Shot { kind: 2, damage: 6,
        pos: Vec3 { x: 0, y: 0, z: 201 }, ..Shot::EMPTY };
    game.update_shots(&test_bank(0));
    assert_eq!((game.shield, game.shots[0].kind), (94, 0));
    game.pickups[0] = Pickup { kind: 1, pos: Vec3 { x: 0, y: 0, z: 195 } };
    game.update_pickups();
    assert_eq!((game.shield, game.pickups[0].kind), (100, 0));
}
