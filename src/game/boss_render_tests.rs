use super::*;
use crate::render::Renderer;

fn children(phase: u8) -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, 17, 0);
    game.boss_started = true;
    game.boss_phase = phase;
    for index in 0..game.boss_count() {
        game.bosses[index].pos = Vec3 {
            x: if phase == 1 {
                if index & 1 == 0 {
                    -74
                } else {
                    74
                }
            } else if index & 1 == 0 {
                -55
            } else {
                55
            },
            y: if phase == 1 {
                0
            } else if index < 2 {
                -40
            } else {
                40
            },
            z: 500,
        };
        game.bosses[index].left = BOSS_SIDE_HP >> phase;
        game.bosses[index].right = BOSS_SIDE_HP >> phase;
        game.bosses[index].core = BOSS_CORE_HP >> phase;
        game.bosses[index].flash = 0;
    }
    game
}

fn draw(game: &Game, bank: &AssetBank<'_>) -> Renderer {
    let mut renderer = Renderer::new();
    game.render_foreground(&mut renderer, bank, 174);
    assert_eq!(renderer.dropped, 0);
    assert!(renderer.triangle_count <= 174);
    renderer
}

#[test]
fn every_child_and_all_projectiles_fit_the_existing_foreground_capacity() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    for phase in [1, 2] {
        for player_y in [-65, 0, 65] {
            for exposed in [false, true] {
                let mut game = children(phase);
                game.player.y = player_y;
                if exposed {
                    for boss in &mut game.bosses {
                        boss.left = 0;
                        boss.right = 0;
                    }
                }
                game.boss_started = false;
                let player_count = draw(&game, &bank).triangle_count;
                game.boss_started = true;
                let healthy = game.bosses;
                let mut all_child_count = 0;
                for index in 0..game.boss_count() {
                    for (other, boss) in game.bosses.iter_mut().enumerate() {
                        boss.core = if other == index {
                            healthy[other].core
                        } else {
                            0
                        };
                    }
                    let child_count = draw(&game, &bank).triangle_count - player_count;
                    assert!(child_count >= 8, "phase {phase}, child {index} disappeared");
                    all_child_count += child_count;
                }
                game.bosses = healthy;
                let count = draw(&game, &bank).triangle_count;
                assert_eq!(count, player_count + all_child_count);
                for (index, shot) in game.shots.iter_mut().enumerate() {
                    *shot = Shot {
                        pos: Vec3 {
                            x: (index as i16 % 6 - 3) * 20,
                            y: (index as i16 / 6 - 2) * 15,
                            z: 300,
                        },
                        kind: 2,
                        damage: 6,
                        ..Shot::EMPTY
                    };
                }
                assert_eq!(draw(&game, &bank).triangle_count, count + 2 * SHOTS as u16);
                game.pickups.fill(Pickup {
                    pos: Vec3 { x: 0, y: 0, z: 400 },
                    kind: 1,
                });
                game.explosions.fill(Explosion {
                    pos: Vec3 { x: 0, y: 0, z: 400 },
                    ticks: 9,
                    color: 7,
                });
                draw(&game, &bank);
            }
        }
    }
}

#[test]
fn child_hulls_and_weakpoints_stay_on_screen_for_the_whole_camera_range() {
    struct Capture {
        min: (i16, i16),
        max: (i16, i16),
    }
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, _: u8, _: u8) {
            for vertex in [a, b, c] {
                assert!(vertex.z > crate::render::NEAR);
                let (x, y) = crate::projection::project(vertex);
                self.min.0 = self.min.0.min(x);
                self.min.1 = self.min.1.min(y);
                self.max.0 = self.max.0.max(x);
                self.max.1 = self.max.1.max(y);
            }
        }
    }
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    for phase in [1, 2] {
        for player_y in [-65, 65] {
            for exposed in [false, true] {
                let mut game = children(phase);
                if exposed {
                    for boss in &mut game.bosses {
                        boss.left = 0;
                        boss.right = 0;
                    }
                }
                let mut capture = Capture {
                    min: (i16::MAX, i16::MAX),
                    max: (i16::MIN, i16::MIN),
                };
                game.render_bosses(
                    &mut SceneBudget {
                        sink: &mut capture,
                        count: 0,
                        limit: 174,
                        camera_y: scene_camera_y(Mode::Playing, player_y, 0),
                        color_override: 0,
                        extra_flags: 0,
                    },
                    &bank,
                );
                assert!(capture.min.0 > 0 && capture.max.0 < crate::render::WIDTH as i16);
                assert!(capture.min.1 > 25 && capture.max.1 < 175);
            }
        }
    }
}

#[test]
fn normal_lasers_and_homing_missiles_reach_every_child_weakpoint_from_the_flight_corridor() {
    let bytes = enemies::authored::model_bank(17);
    let bank = AssetBank::new(&bytes).unwrap();
    for phase in [1, 2] {
        for index in 0..1 << phase {
            for part in 0..3 {
                for charge in [0, 255] {
                    let mut game = children(phase);
                    let center = game.bosses[index].pos;
                    // These independently recorded aim positions describe the
                    // visible weakpoints, rather than using the collision helper.
                    let (side_x, side_y) = if phase == 1 { (32, 7) } else { (16, 3) };
                    game.player.x = (center.x
                        + match part {
                            0 => -side_x,
                            1 => side_x,
                            _ => 0,
                        })
                    .clamp(-100, 100);
                    game.player.y = (center.y + if part == 2 { 0 } else { side_y }).clamp(-65, 65);
                    if part == 2 {
                        game.bosses[index].left = 0;
                        game.bosses[index].right = 0;
                    }
                    if charge == 255 {
                        // Missile acquisition is random among eligible targets.
                        // Leave this weakpoint as the sole candidate so the
                        // trajectory exercises each target, including outer sides.
                        for (other, boss) in game.bosses.iter_mut().enumerate() {
                            if other != index {
                                boss.core = 0;
                            }
                        }
                        if part == 0 {
                            game.bosses[index].right = 0;
                        }
                        if part == 1 {
                            game.bosses[index].left = 0;
                        }
                    }
                    let before = game.bosses[index];
                    game.fire_player(charge);
                    for _ in 0..64 {
                        game.update_shots(&bank);
                    }
                    let after = game.bosses[index];
                    let hp = |boss: BossInstance| match part {
                        0 => boss.left,
                        1 => boss.right,
                        _ => boss.core,
                    };
                    assert_eq!(
                        hp(after),
                        hp(before) - if charge == 255 { 4 } else { 1 },
                        "phase {phase}, child {index}, part {part}, charge {charge}"
                    );
                }
            }
        }
    }
}
