//! Opt-in visual fixtures from real authored encounters and the production renderer.
use super::*;
use crate::math::RotationMatrix;
use crate::render::{Renderer, BAND_PIXELS, HEIGHT, NEAR, PALETTE_RGB, WIDTH};
use std::{format, fs, path::Path, string::String, vec, vec::Vec};

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

fn model_bounds(bank: &AssetBank<'_>, kind: u8, age: u16) -> Bounds {
    let roll = if bank.enemy(kind).render_mode == 1 {
        (age as u8).wrapping_mul(2)
    } else {
        0
    };
    let matrix = RotationMatrix::new(0, 0, roll);
    let mut bounds = Bounds {
        min: Vec3 {
            x: i16::MAX,
            y: i16::MAX,
            z: i16::MAX,
        },
        max: Vec3 {
            x: i16::MIN,
            y: i16::MIN,
            z: i16::MIN,
        },
    };
    for index in 0..bank.vertex_count(kind) {
        let point = matrix.apply(bank.vertex_bytes(kind, index), 8 - bank.shift(kind));
        bounds.min.x = bounds.min.x.min(point.x);
        bounds.min.y = bounds.min.y.min(point.y);
        bounds.min.z = bounds.min.z.min(point.z);
        bounds.max.x = bounds.max.x.max(point.x);
        bounds.max.y = bounds.max.y.max(point.y);
        bounds.max.z = bounds.max.z.max(point.z);
    }
    bounds
}

// Signed distance to the finite near-plane rectangle, scaled by 133 so its
// vertical half-height (800/133) remains exact. Positive means separated.
fn clearance(bounds: Bounds, position: Vec3) -> Option<i32> {
    if position.z + bounds.min.z > NEAR || position.z + bounds.max.z < NEAR {
        return None;
    }
    let x = (position.x + bounds.min.x - NEAR).max(-NEAR - position.x - bounds.max.x) as i32 * 133;
    let y = ((position.y + bounds.min.y - 40) as i32 * 133 - 800)
        .max((40 - position.y - bounds.max.y) as i32 * 133 - 800);
    Some(x.max(y))
}

struct Pass {
    spawn: u8,
    repeated: bool,
    seed: u8,
    nearest: usize,
    clearance: i32,
    start_tick: u16,
    poses: Vec<Actor>,
}

fn closest_pass(stage: u8, kind: u8, segment: &Segment<'_>, bank: &AssetBank<'_>) -> Pass {
    let bounds: [Bounds; 256] = core::array::from_fn(|age| model_bounds(bank, kind, age as u16));
    let mut best: Option<Pass> = None;
    let mut game = Game::new();
    game.reset_scene(Mode::Attract, stage, 0);
    for spawn in 0..segment.spawn_count() {
        let record = segment.spawn(spawn);
        if record.kind != kind {
            continue;
        }
        for repeated in [false, true] {
            let distance = record.distance + if repeated { ENEMY_REPEAT_DISTANCE } else { 0 };
            let start_tick = (distance + 7) / 8;
            for seed in 0..=u8::MAX {
                game.encounter_seed = seed;
                game.actors.fill(Actor::EMPTY);
                game.spawn_encounter(
                    record,
                    start_tick * 8 - distance,
                    bank,
                    spawn | if repeated { 0x80 } else { 0 },
                );
                let mut actor = game.actors[0];
                assert_eq!(actor.kind, kind);
                let mut poses = Vec::new();
                let mut minimum = i32::MAX;
                let mut nearest = 0;
                while actor.pos.z >= ACTOR_RETIRE_Z {
                    if let Some(gap) = clearance(bounds[actor.age as u8 as usize], actor.pos) {
                        if gap < minimum {
                            minimum = gap;
                            nearest = poses.len();
                        }
                    }
                    poses.push(actor);
                    actor.age += 1;
                    enemies::step(
                        game.stage_info.enemy_profile,
                        &mut actor,
                        &Vec3 {
                            x: 0,
                            y: 0,
                            z: PLAYER_Z,
                        },
                        0,
                    );
                }
                if best.as_ref().is_none_or(|pass| minimum < pass.clearance) {
                    best = Some(Pass {
                        spawn,
                        repeated,
                        seed,
                        nearest,
                        clearance: minimum,
                        start_tick,
                        poses,
                    });
                }
            }
        }
    }
    best.expect("stage has the requested flyer")
}

fn raster(renderer: &mut Renderer) -> Vec<u8> {
    assert_eq!(renderer.dropped, 0);
    renderer.finish();
    let mut pixels = Vec::with_capacity(WIDTH * HEIGHT);
    for band in 0..25 {
        let mut row = [0; BAND_PIXELS];
        renderer.render_band(band, &mut row);
        pixels.extend(row.map(|ink| ink & 15));
    }
    pixels
}

fn write_ppm(path: &Path, pixels: &[u8], width: usize, height: usize) {
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    for &color in pixels {
        bytes.extend(PALETTE_RGB[color as usize]);
    }
    fs::write(path, bytes).unwrap();
}

#[test]
#[ignore = "exports close fly-by and projectile visual review fixtures to target/camera-clearance"]
fn export_close_flybys_and_hostile_bullets() {
    let out = Path::new("target/camera-clearance");
    fs::create_dir_all(out).unwrap();
    let cases = [
        (0, assets::FIGHTER),
        (2, assets::DIVER),
        (5, assets::FIGHTER),
        (8, assets::DIVER),
        (13, assets::FIGHTER),
    ];
    let offsets = [-12i16, -6, 0, 3, 6];
    let mut tiles = Vec::new();
    let mut manifest = String::from(
        "Production renderer, neutral player, camera Y=40.\n\
        Each fly-by row shows offsets -12, -6, 0, +3, +6 ticks from its closest mesh AABB pass.\n\
        Last row: enemy bullets at z=60,180,500; old radius 5 on left, new radius 4 on right.\n",
    );
    for (stage, kind) in cases {
        let models = enemies::authored::model_bank(stage);
        let bank = AssetBank::new(&models).unwrap();
        let records = enemies::authored::segment_bank(stage);
        let segment = Segment::new(&records).unwrap();
        let pass = closest_pass(stage, kind, &segment, &bank);
        assert!(
            pass.clearance >= 133,
            "stage {stage} has an unsafe selected fly-by: {}/133",
            pass.clearance
        );
        manifest.push_str(&format!("\nStage {stage}: {}, kind {kind}, spawn {}, repeat {}, seed {}, minimum clearance {}/133 world units\n",
            enemies::authored::stage_name(stage), pass.spawn, pass.repeated, pass.seed, pass.clearance));
        for offset in offsets {
            let index = (pass.nearest as i16 + offset).max(0) as usize;
            let mut game = Game::new();
            game.reset_scene(Mode::Playing, stage, pass.start_tick + index as u16);
            if let Some(&actor) = pass.poses.get(index) {
                game.actors[0] = actor;
            }
            let mut renderer = Renderer::new();
            renderer.begin(0);
            game.render_foreground(&mut renderer, &bank, 174);
            let pixels = raster(&mut renderer);
            let name = format!("stage-{stage:02}-kind-{kind}-offset-{offset:+}.ppm");
            write_ppm(&out.join(&name), &pixels, WIDTH, HEIGHT);
            manifest.push_str(&format!(
                "  {name}: age {}, position {:?}\n",
                game.actors[0].age, game.actors[0].pos
            ));
            tiles.push(pixels);
        }
    }
    for z in [60, 180, 500] {
        let mut game = Game::new();
        game.reset_scene(Mode::Playing, 0, 0);
        game.shots[0] = Shot {
            kind: 2,
            damage: 6,
            pos: Vec3 { x: 30, y: 40, z },
            ..Shot::EMPTY
        };
        let mut renderer = Renderer::new();
        renderer.begin(0);
        diamond(&mut renderer, Vec3 { x: -30, y: 0, z }, 5, 7);
        game.render_effects(&mut SceneBudget {
            sink: &mut renderer,
            count: 0,
            limit: 174,
            camera_y: 40,
            color_override: 0,
            extra_flags: 0,
        });
        let pixels = raster(&mut renderer);
        write_ppm(
            &out.join(format!("hostile-bullets-z-{z}.ppm")),
            &pixels,
            WIDTH,
            HEIGHT,
        );
        tiles.push(pixels);
    }
    // Aspect-correct nearest-neighbor enlargement, five columns and six rows.
    let cell_width = 324;
    let cell_height = 244;
    let width = 5 * cell_width;
    let height = 6 * cell_height;
    let mut sheet = vec![11u8; width * height];
    for (index, tile) in tiles.iter().enumerate() {
        for y in 0..240 {
            for x in 0..320 {
                sheet[(index / 5 * cell_height + 2 + y) * width + index % 5 * cell_width + 2 + x] =
                    tile[y * HEIGHT / 240 * WIDTH + x / 2];
            }
        }
    }
    write_ppm(&out.join("close-flybys.ppm"), &sheet, width, height);
    fs::write(out.join("visual-fixtures.txt"), manifest).unwrap();
}
