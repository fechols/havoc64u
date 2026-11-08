//! Two-player co-op: seating, independent ships, credit, damage and respawns.
use super::tests::{empty_segment, test_bank};
use super::*;

const J2: usize = DEVICE_JOY2 as usize;
const J1: usize = DEVICE_JOY1 as usize;
const KEYS: usize = DEVICE_KEYBOARD as usize;

/// One sample of all three devices; `pressed` marks fresh edges.
fn feed(game: &mut Game, held: [u8; 3], pressed: [u8; 3]) {
    let ms = game.controls.now.wrapping_add(20);
    game.sample_devices(&DeviceSample { bits: held, pressed, ms });
}

fn press(game: &mut Game, device: usize, bits: u8) {
    let mut held = [0; 3];
    held[device] = bits;
    feed(game, held, held);
    feed(game, [0; 3], [0; 3]);
}

fn step(game: &mut Game, held: [u8; 3]) {
    feed(game, held, [0; 3]);
    let bytes = empty_segment(game.stage_id);
    let segment = Segment::new(&bytes).unwrap();
    game.tick(&segment, &test_bank(game.stage_id));
}

/// Start a run from the title with `device`, as the native loop does.
fn started(device: usize) -> Game {
    let mut game = Game::new();
    press(&mut game, device, FIRE);
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    game.tick(&segment, &test_bank(0));
    assert_eq!(game.mode, Mode::Playing);
    assert_eq!(game.devices, device as u8 + 1);
    game.invulnerability = 0;
    game
}

fn coop(first: usize, second: usize) -> Game {
    let mut game = started(first);
    press(&mut game, second, FIRE);
    assert_eq!(game.devices, (first as u8 + 1) | (second as u8 + 1) << 4);
    game.reserve.invulnerability = 0;
    game
}

#[test]
fn the_starting_device_becomes_the_first_player_and_flies_alone() {
    for device in [J2, J1, KEYS] {
        let mut game = started(device);
        assert!(!game.reserve_present());
        assert_eq!(game.ship_slots(), 1);
        // Other devices no longer steer the first ship.
        let x = game.player.x;
        let mut held = [0; 3];
        held[(device + 1) % 3] = RIGHT;
        for _ in 0..10 {
            step(&mut game, held);
        }
        assert_eq!(game.player.x, x, "device {device}");
        held = [0; 3];
        held[device] = RIGHT;
        for _ in 0..10 {
            step(&mut game, held);
        }
        assert!(game.player.x > x);
    }
}

#[test]
fn any_other_device_seats_the_second_player_with_a_fresh_ship() {
    for (first, second) in [(J2, J1), (J2, KEYS), (KEYS, J1), (KEYS, J2), (J1, J2), (J1, KEYS)] {
        let mut game = started(first);
        game.score = 40;
        press(&mut game, second, FIRE);
        assert!(game.reserve_present(), "{first} {second}");
        let ship = game.reserve;
        assert_eq!((ship.lives, ship.shield, ship.bombs, ship.score), (3, 100, 3, 0));
        assert_eq!(ship.player, Vec3 { x: SECOND_SHIP_X, y: 0, z: PLAYER_Z });
        assert_eq!(ship.invulnerability, 50);
        assert_eq!(game.score, 40, "the first player's ship is untouched");
        assert!(!game.swapped);
        // A third device cannot take the occupied seat.
        let third = 3 - first - second;
        press(&mut game, third, FIRE);
        assert_eq!(game.devices, (first as u8 + 1) | (second as u8 + 1) << 4);
    }
}

#[test]
fn story_mode_seats_the_second_player_with_ninety_nine_lives() {
    let mut game = Game::new();
    game.toggle_story_mode();
    press(&mut game, J2, FIRE);
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    game.tick(&segment, &test_bank(0));
    assert_eq!(game.lives, 99);
    press(&mut game, KEYS, FIRE);
    assert_eq!(game.reserve.lives, 99);
}

#[test]
fn joining_waits_for_flight_and_a_new_run_starts_alone() {
    let mut game = Game::new();
    // In menus every device drives the first player.
    press(&mut game, KEYS, UP);
    assert!(game.menu_activity);
    assert_eq!(game.devices, 0);
    let mut game = coop(J2, KEYS);
    game.mode = Mode::GameOver;
    press(&mut game, J1, FIRE);
    assert_eq!(game.devices, J1 as u8 + 1, "the restarting device takes the first seat");
    let bytes = empty_segment(0);
    let segment = Segment::new(&bytes).unwrap();
    game.tick(&segment, &test_bank(0));
    assert_eq!(game.mode, Mode::Playing);
    assert_eq!(game.devices, J1 as u8 + 1);
    assert!(!game.reserve_present());
}

#[test]
fn each_player_steers_their_own_ship() {
    let mut game = coop(J2, KEYS);
    let (one, two) = (game.player, game.reserve.player);
    let mut held = [0; 3];
    held[KEYS] = LEFT | UP;
    for _ in 0..20 {
        step(&mut game, held);
    }
    assert_eq!(game.player, one);
    assert!(game.reserve.player.x < two.x && game.reserve.player.y > two.y);
    held = [RIGHT, 0, 0];
    for _ in 0..20 {
        step(&mut game, held);
    }
    assert!(game.player.x > one.x);
    assert!(!game.swapped);
}

#[test]
fn second_player_shots_are_marked_and_their_kills_score_for_them() {
    let mut game = coop(J2, KEYS);
    let bank = test_bank(0);
    let mut held = [0; 3];
    held[KEYS] = FIRE;
    step(&mut game, held);
    let slot = game.shots.iter().position(|shot| shot.kind == 1).unwrap();
    assert_eq!(game.shots[slot].pos.x, game.reserve.player.x);
    assert_eq!(game.missile_targets[slot] & 0x80, 0x80);
    assert_eq!(game.missile_lock(slot), 0);
    // An enemy straight ahead of the second ship's shot.
    game.shots = [Shot::EMPTY; SHOTS];
    game.missile_targets = [0; SHOTS];
    game.actors[0] = Actor {
        pos: Vec3 { x: SECOND_SHIP_X, y: 0, z: 520 },
        origin_x: SECOND_SHIP_X,
        kind: assets::FIGHTER,
        hp: 1,
        ..Actor::EMPTY
    };
    game.shots[0] = Shot { pos: Vec3 { x: SECOND_SHIP_X, y: 0, z: 500 }, damage: 4, kind: 1, ..Shot::EMPTY };
    game.missile_targets[0] = 0x80;
    game.update_shots(&bank);
    assert_eq!(game.actors[0].kind, 0);
    assert_eq!((game.score, game.kills), (0, 0));
    assert_eq!(game.reserve.kills, 1);
    assert!(game.reserve.score > 0);
    assert!(!game.swapped);
}

#[test]
fn hostile_fire_damages_only_the_ship_it_reaches_and_rolls_still_deflect() {
    let mut game = coop(J2, KEYS);
    let bank = test_bank(0);
    let hostile = |x| Shot { pos: Vec3 { x, y: 0, z: PLAYER_Z + 18 }, damage: 12, kind: 2, ..Shot::EMPTY };
    game.shots[0] = hostile(SECOND_SHIP_X);
    game.update_shots(&bank);
    assert_eq!((game.shield, game.reserve.shield), (100, 88));
    game.reserve.invulnerability = 0;
    game.reserve.roll_ticks = 10;
    game.shots[0] = hostile(SECOND_SHIP_X);
    game.update_shots(&bank);
    assert_eq!(game.reserve.shield, 88, "a rolling ship deflects the shot");
    game.shots[0] = hostile(0);
    game.update_shots(&bank);
    assert_eq!((game.shield, game.reserve.shield), (88, 88));
    // Overlapping ships both take the hit.
    game.invulnerability = 0;
    game.reserve.roll_ticks = 0;
    game.reserve.player.x = 0;
    game.shots[0] = hostile(0);
    game.update_shots(&bank);
    assert_eq!((game.shield, game.reserve.shield), (76, 76));
}

#[test]
fn ramming_enemies_and_pickups_find_the_second_ship() {
    let mut game = coop(J2, KEYS);
    let bank = test_bank(0);
    game.actors[0] = Actor {
        pos: Vec3 { x: SECOND_SHIP_X, y: 0, z: PLAYER_Z + SPEED },
        origin_x: SECOND_SHIP_X,
        kind: assets::FIGHTER,
        hp: 3,
        approach_speed: 8,
        ..Actor::EMPTY
    };
    game.update_actors(&bank);
    assert_eq!(game.actors[0].kind, 0);
    assert_eq!((game.shield, game.reserve.shield), (100, 85));
    game.reserve.bombs = 1;
    game.pickups[0] = Pickup { pos: Vec3 { x: SECOND_SHIP_X, y: 0, z: PLAYER_Z + SPEED }, kind: 2 };
    game.update_pickups();
    assert_eq!(game.pickups[0].kind, 0);
    assert_eq!((game.bombs, game.reserve.bombs), (3, 2));
    assert!(game.reserve.score > 0 && game.score == 0);
}

#[test]
fn a_downed_ship_returns_in_place_while_its_partner_flies_on() {
    let mut game = coop(J2, KEYS);
    for _ in 0..30 {
        step(&mut game, [0; 3]);
    }
    let rail = game.stage_ticks;
    game.reserve.shield = 5;
    game.swap_seats();
    game.damage_player(10);
    game.swap_seats();
    assert_eq!(game.mode, Mode::Playing, "the stage keeps scrolling");
    assert_eq!((game.reserve.lives, game.reserve.respawn_ticks), (2, 75));
    assert_eq!(game.flying_ship(1), None);
    assert_eq!(game.ships_near(Vec3 { x: SECOND_SHIP_X, y: 0, z: PLAYER_Z }, 12), 0);
    let mut held = [0; 3];
    held[KEYS] = FIRE | BOOST;
    for _ in 0..74 {
        step(&mut game, held);
        assert!(!game.boost_active(), "a downed ship cannot boost the world");
    }
    assert!(game.shots.iter().all(|shot| shot.kind != 1), "a downed ship cannot fire");
    step(&mut game, [0; 3]);
    let ship = game.reserve;
    assert_eq!((ship.respawn_ticks, ship.shield, ship.invulnerability), (0, 100, 100));
    assert_eq!(ship.player, Vec3 { x: SECOND_SHIP_X, y: 0, z: PLAYER_Z });
    assert!(game.stage_ticks > rail + 70, "no checkpoint rewind");
}

#[test]
fn both_ships_down_rewinds_to_the_checkpoint_together() {
    let mut game = coop(J2, KEYS);
    for _ in 0..30 {
        step(&mut game, [0; 3]);
    }
    game.reserve.shield = 0;
    game.swap_seats();
    game.shield = 0;
    game.damage_player(1);
    game.swap_seats();
    assert_eq!(game.reserve.respawn_ticks, 75);
    game.shield = 1;
    game.damage_player(10);
    assert_eq!(game.mode, Mode::Respawning);
    for _ in 0..80 {
        step(&mut game, [0; 3]);
    }
    assert_eq!(game.mode, Mode::Playing);
    assert!(game.stage_ticks < 10, "rewound to the checkpoint: {}", game.stage_ticks);
    assert_eq!((game.lives, game.reserve.lives), (2, 2));
    assert_eq!((game.respawn_ticks, game.reserve.respawn_ticks), (0, 0));
    assert_eq!((game.shield, game.reserve.shield), (100, 100));
    assert_eq!(game.reserve.player.x, SECOND_SHIP_X);
    assert_eq!(game.player.x, 0);
}

#[test]
fn a_player_out_of_lives_retires_and_may_rejoin_until_both_are_out() {
    let mut game = coop(J2, KEYS);
    game.reserve.lives = 1;
    game.reserve.shield = 1;
    game.reserve.score = 99;
    game.swap_seats();
    game.damage_player(10);
    game.swap_seats();
    assert_eq!(game.mode, Mode::Playing);
    assert!(!game.reserve_present(), "the second seat is free again");
    // The same (or any free) device rejoins with a fresh ship.
    press(&mut game, KEYS, FIRE);
    assert!(game.reserve_present());
    assert_eq!((game.reserve.lives, game.reserve.score), (3, 0));
    // The first player retires while the second still flies.
    game.reserve.invulnerability = 0;
    game.lives = 1;
    game.shield = 1;
    game.damage_player(10);
    assert_eq!(game.mode, Mode::Playing);
    assert_eq!(game.lives, 0);
    assert_eq!(game.flying_ship(0), None);
    game.update_ships(&test_bank(0), false);
    assert_eq!(game.view_y(), game.reserve.player.y);
    // ...and the first player's own device brings them back.
    press(&mut game, J2, FIRE);
    assert_eq!((game.lives, game.shield), (3, 100));
    // Game over only once neither ship has a life left.
    game.invulnerability = 0;
    game.lives = 1;
    game.shield = 1;
    game.damage_player(10);
    game.reserve.lives = 1;
    game.reserve.shield = 1;
    game.swap_seats();
    game.damage_player(10);
    game.swap_seats();
    assert_eq!(game.mode, Mode::GameOver);
}

#[test]
fn either_ship_boosting_speeds_the_world_but_steering_boost_stays_per_ship() {
    let mut game = coop(J2, KEYS);
    let mut held = [0; 3];
    held[KEYS] = BOOST;
    feed(&mut game, held, held);
    step(&mut game, [0; 3]);
    assert!(game.boost_active());
    assert!(!game.ship_boost());
    assert!(game.reserve.controls.flags & BOOST_ACTIVE != 0);
    let (rail, travel) = (game.stage_ticks, game.extra_travel_ticks);
    step(&mut game, [0; 3]);
    assert_eq!(game.stage_ticks, rail + 2);
    assert_eq!(game.extra_travel_ticks, travel + 1);
    // Both boosting still advances boosted travel once per tick.
    feed(&mut game, [BOOST, 0, 0], [BOOST, 0, 0]);
    step(&mut game, [0; 3]);
    assert!(game.ship_boost());
    let travel = game.extra_travel_ticks;
    step(&mut game, [0; 3]);
    assert_eq!(game.extra_travel_ticks, travel + 1);
}

#[test]
fn the_camera_frames_both_ships_and_stays_within_the_tested_range() {
    let mut game = coop(J2, KEYS);
    let bank = test_bank(0);
    let view = |game: &mut Game| {
        game.update_ships(&bank, false);
        game.view_y()
    };
    game.player.y = 65;
    game.reserve.player.y = -65;
    assert_eq!(view(&mut game), 0);
    game.reserve.player.y = 65;
    assert_eq!(view(&mut game), 65);
    game.reserve.respawn_ticks = 10;
    game.player.y = -30;
    assert_eq!(view(&mut game), -30, "a downed ship leaves the frame");
    game.lives = 0;
    game.reserve.respawn_ticks = 0;
    assert_eq!(view(&mut game), 65, "a retired first player leaves the frame");
    assert_eq!(game.view_x(), game.reserve.player.x);
}

#[test]
fn both_ships_render_with_the_second_in_red_trim() {
    #[derive(Default)]
    struct Capture(std::vec::Vec<(u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, _: Vec3, _: Vec3, _: Vec3, color: u8, flags: u8) {
            self.0.push((color, flags));
        }
    }
    let bank = test_bank(0);
    let ship = bank.face_count(assets::PLAYER) as usize + 4;
    let mut game = coop(J2, KEYS);
    game.mode = Mode::Transition;
    let mut one = Capture::default();
    game.render_foreground(&mut one, &bank, 174);
    let first: std::vec::Vec<_> = one.0[..ship].to_vec();
    let second: std::vec::Vec<_> = one.0[ship..2 * ship].to_vec();
    assert!(first.iter().chain(&second).all(|&(_, flags)| flags & crate::PLAYER_GEOMETRY != 0));
    assert!(first.iter().any(|&(color, _)| color == 6 || color == 14));
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(b.0, SECOND_SHIP_COLORS[a.0 as usize]);
    }
    assert!(second.iter().all(|&(color, _)| color != 6 && color != 14));
    // A downed second ship is not drawn.
    game.reserve.respawn_ticks = 5;
    let mut alone = Capture::default();
    game.render_foreground(&mut alone, &bank, 174);
    assert_eq!(&alone.0[..ship], &first[..]);
    assert!(alone.0[ship..].iter().all(|&(_, flags)| flags & crate::PLAYER_GEOMETRY == 0));
}

#[test]
fn pausing_keeps_each_seated_players_held_controls() {
    let mut game = coop(J2, KEYS);
    let mut held = [0; 3];
    held[J2] = FIRE;
    held[KEYS] = LEFT;
    game.sample_paused_devices(&held, game.controls.now.wrapping_add(20));
    assert_eq!((game.controls.bits, game.reserve.controls.bits), (FIRE, LEFT));
    assert!(!game.swapped);
}

#[test]
fn a_reused_actor_slot_drops_second_player_locks_but_keeps_their_owner() {
    let mut game = coop(J2, KEYS);
    let bank = test_bank(0);
    // Slots 0..3 are taken; slot 3 is free and locked by both players' shots.
    for slot in 0..3 {
        game.actors[slot] = Actor { kind: assets::FIGHTER, hp: 1, ..Actor::EMPTY };
    }
    game.actors[3] = Actor::EMPTY;
    game.missile_targets[0] = 0x80 | 4;
    game.missile_targets[1] = 4;
    game.missile_targets[2] = 0x80 | 2;
    game.spawn_actor(Spawn { distance: 0, kind: assets::FIGHTER, x: 0, y: 0, flags: 0 }, 0, &bank);
    assert_ne!(game.actors[3].kind, 0);
    assert_eq!(game.missile_targets[0], 0x80, "the second player's lock is dropped, its owner kept");
    assert_eq!(game.missile_targets[1], 0);
    assert_eq!(game.missile_targets[2], 0x80 | 2, "other targets keep their locks");
}

#[test]
fn second_player_gestures_during_the_rewind_wait_are_discarded() {
    let mut game = coop(J2, KEYS);
    for _ in 0..30 {
        step(&mut game, [0; 3]);
    }
    game.reserve.shield = 0;
    game.swap_seats();
    game.shield = 0;
    game.damage_player(1);
    game.swap_seats();
    game.shield = 1;
    game.damage_player(10);
    assert_eq!(game.mode, Mode::Respawning);
    let kept = FIRE_ARMED | LEFT_ARMED | RIGHT_ARMED | BOOST_ACTIVE;
    let mut gestured = false;
    while game.mode == Mode::Respawning {
        press(&mut game, KEYS, FIRE);
        press(&mut game, KEYS, RIGHT);
        press(&mut game, KEYS, RIGHT);
        let controls = game.reserve.controls;
        gestured |= controls.flags & !kept != 0 || controls.roll_pending != 0;
        step(&mut game, [0; 3]);
    }
    assert!(gestured, "the presses reached the second seat");
    assert_eq!(game.mode, Mode::Playing);
    assert_eq!(game.reserve.controls.flags & !kept, 0);
    assert_eq!(game.reserve.controls.roll_pending, 0);
}

#[test]
fn enemy_fire_alternates_between_flying_ships_and_skips_a_downed_one() {
    let mut game = coop(J2, KEYS);
    let first = [game.player.x, game.player.y];
    let second = [game.reserve.player.x, game.reserve.player.y];
    assert_ne!(first, second);
    let mut aims = [[0i16; 2]; 4];
    for aim in &mut aims {
        step(&mut game, [0; 3]);
        *aim = game.aim;
    }
    for pair in aims.windows(2) {
        assert!(pair[0] == first || pair[0] == second, "{:?}", pair[0]);
        assert_ne!(pair[0], pair[1], "successive ticks aim at alternate ships");
    }
    game.reserve.respawn_ticks = 75;
    for _ in 0..4 {
        step(&mut game, [0; 3]);
        assert_eq!(game.aim, [game.player.x, game.player.y]);
    }
}
