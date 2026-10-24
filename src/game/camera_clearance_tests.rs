//! Host-side authoring audit. Bounds come from the production geometry path,
//! including quantization, roll and both distance/budget proxy silhouettes.
use super::*;
use crate::render::NEAR;
use std::vec::Vec;

const CAMERA_LOW: i32 = 24;
const CAMERA_HIGH: i32 = 56;
// At z=8 the 160x200 viewport is x=[-8,8], y=[-800/133,800/133].
// Keep the comparison rational so rounding never silently enlarges the gap.
const MARGIN_133: i32 = 133;

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: [i32; 3],
    max: [i32; 3],
}

impl Bounds {
    fn point(p: Vec3) -> Self {
        let p = [i32::from(p.x), i32::from(p.y), i32::from(p.z)];
        Self { min: p, max: p }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: core::array::from_fn(|axis| self.min[axis].min(other.min[axis])),
            max: core::array::from_fn(|axis| self.max[axis].max(other.max[axis])),
        }
    }

    fn at(self, p: Vec3) -> Self {
        let p = [i32::from(p.x), i32::from(p.y), i32::from(p.z)];
        Self {
            min: core::array::from_fn(|axis| self.min[axis] + p[axis]),
            max: core::array::from_fn(|axis| self.max[axis] + p[axis]),
        }
    }

    fn crosses_near(self) -> bool {
        self.min[2] <= i32::from(NEAR) && self.max[2] >= i32::from(NEAR)
    }

    /// Separation from the union of every legal chase/attract camera rectangle.
    /// One separating axis suffices; a negative result means overlap in XY.
    fn clearance_133(self, camera_low: i32, camera_high: i32) -> i32 {
        [
            (self.min[0] - 8) * 133,
            (-8 - self.max[0]) * 133,
            (self.min[1] - camera_high) * 133 - 800,
            (camera_low - self.max[1]) * 133 - 800,
        ]
        .into_iter()
        .max()
        .unwrap()
    }
}

#[derive(Default)]
struct Capture(Option<Bounds>);

impl GeometrySink for Capture {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, _: u8, _: u8) {
        for point in [a, b, c] {
            let point = Bounds::point(point);
            self.0 = Some(self.0.map_or(point, |old| old.union(point)));
        }
    }
}

struct Geometry {
    phases: Vec<[Bounds; 2]>,
    envelope: Bounds,
}

fn geometry(stage: u8, kind: u8, bank: &AssetBank<'_>) -> Geometry {
    let mut game = Game::new();
    game.reset_scene(Mode::Attract, stage, 0);
    game.actors[0] = Actor {
        kind,
        hp: bank.enemy(kind).hp,
        ..Actor::EMPTY
    };
    let mut phases = Vec::new();
    for age in 0..256 {
        game.actors[0].age = age;
        // A plain capture receives the full hull at 174 and forces the same
        // two-face proxy used by either distance or exhausted frame budgets.
        phases.push(core::array::from_fn(|proxy| {
            let mut capture = Capture::default();
            game.render_combat(
                &mut SceneBudget {
                    sink: &mut capture,
                    count: 0,
                    limit: if proxy == 0 { 174 } else { 2 },
                    camera_y: 0,
                    color_override: 0,
                    extra_flags: 0,
                },
                bank,
            );
            capture.0.expect("every enemy has geometry")
        }));
    }
    let envelope = phases
        .iter()
        .flatten()
        .copied()
        .reduce(Bounds::union)
        .unwrap();
    Geometry { phases, envelope }
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Fields are retained in the diagnostic Debug report.
struct Witness {
    spawn: u8,
    repeat: bool,
    seed: u8,
    kind: u8,
    age: u16,
    proxy: bool,
    bounds: Bounds,
}

#[test]
fn near_rectangle_predicate_detects_the_original_lowland_failure_and_exact_margin() {
    let original = Bounds {
        min: [-87, 28, 1],
        max: [-7, 53, 83],
    };
    assert!(original.crosses_near());
    assert!(original.clearance_133(40, 40) < 0);
    for (left, clear) in [(8, 0), (9, 133), (10, 266)] {
        let bound = Bounds {
            min: [left, 35, 7],
            max: [left + 20, 45, 9],
        };
        assert_eq!(bound.clearance_133(40, 40), clear);
    }
    let before_plane = Bounds {
        min: [-2, 38, 9],
        max: [2, 42, 10],
    };
    assert!(!before_plane.crosses_near());
    // The plane is not a slab: the one-unit margin only expands its XY edges.
    let behind_plane = Bounds {
        min: [-2, 38, 6],
        max: [2, 42, 7],
    };
    assert!(!behind_plane.crosses_near());
}

#[test]
fn every_authored_enemy_seed_and_repeat_has_a_safe_swept_flyby() {
    let player = Vec3 {
        x: 0,
        y: 0,
        z: PLAYER_Z,
    };
    let mut total = 0usize;
    let mut failures = Vec::new();
    for stage in 0..assets::SEGMENT_COUNT {
        let model_bytes = enemies::authored::model_bank(stage);
        let segment_bytes = enemies::authored::segment_bank(stage);
        let bank = AssetBank::new(&model_bytes).unwrap();
        let segment = Segment::new(&segment_bytes).unwrap();
        let shapes: Vec<_> = (1..=bank.enemy_count())
            .map(|kind| geometry(stage, kind, &bank))
            .collect();
        let mut game = Game::new();
        game.reset_scene(Mode::Attract, stage, 0);
        let mut minimum = i32::MAX;
        let mut closest = None;
        let mut stage_total = 0usize;
        let mut failed_variants = 0usize;
        let mut first_failure = None;
        for index in 0..segment.spawn_count() {
            let spawn = segment.spawn(index);
            if !matches!(spawn.kind, assets::FIGHTER | assets::DIVER | assets::TURRET) {
                continue;
            }
            let shape = &shapes[usize::from(spawn.kind - 1)];
            for repeat in [false, true] {
                let distance = spawn.distance + if repeat { ENEMY_REPEAT_DISTANCE } else { 0 };
                let late = (8 - distance % 8) % 8;
                let token = index | if repeat { 0x80 } else { 0 };
                for seed in 0..=u8::MAX {
                    game.actors.fill(Actor::EMPTY);
                    game.encounter_seed = seed;
                    game.spawn_encounter(spawn, late, &bank, token);
                    let mut actor = game.actors[0];
                    assert_eq!(actor.kind, spawn.kind);
                    total += 1;
                    stage_total += 1;
                    let mut failed = false;
                    // Independent actors cannot be destroyed by the player or
                    // hidden by pool exhaustion, death, or section transitions.
                    while actor.pos.z >= ACTOR_RETIRE_Z {
                        let previous = actor;
                        actor.age += 1;
                        enemies::step(segment.stage_info().enemy_profile, &mut actor, &player, 0);
                        assert!(actor.age < 800, "unbounded lifetime, stage {stage}");
                        if i32::from(previous.pos.z.min(actor.pos.z)) + shape.envelope.min[2]
                            > i32::from(NEAR)
                        {
                            continue;
                        }
                        // Check each rendering representation separately here;
                        // the union-envelope proof below also covers arbitrary
                        // mesh/proxy switches between consecutive frames.
                        for proxy in 0..2 {
                            let swept = shape.phases[previous.age as u8 as usize][proxy]
                                .at(previous.pos)
                                .union(shape.phases[actor.age as u8 as usize][proxy].at(actor.pos));
                            if !swept.crosses_near() {
                                continue;
                            }
                            let gap = swept.clearance_133(CAMERA_LOW, CAMERA_HIGH);
                            let witness = Witness {
                                spawn: index,
                                repeat,
                                seed,
                                kind: spawn.kind,
                                age: actor.age,
                                proxy: proxy != 0,
                                bounds: swept,
                            };
                            if gap < MARGIN_133 {
                                failed = true;
                                first_failure.get_or_insert(witness);
                            }
                            if gap < minimum {
                                minimum = gap;
                                closest = Some(witness);
                            }
                        }
                    }
                    failed_variants += usize::from(failed);
                }
            }
        }
        assert!(closest.is_some(), "stage {stage} has no audited flybys");
        std::println!(
            "stage {stage:02} {}: {failed_variants}/{stage_total} failed variants; minimum clearance {minimum}/133 world units; closest {closest:?}; first failure {first_failure:?}",
            enemies::authored::stage_name(stage)
        );
        if failed_variants != 0 {
            failures.push((stage, failed_variants, first_failure));
        }
    }
    assert!(total > 0, "campaign contains no auditable enemies");
    std::println!("audited {total} independently surviving enemy variants");
    assert!(failures.is_empty(), "unsafe stage variants: {failures:?}");
}

#[test]
fn pursuit_corridors_are_invariant_under_every_legal_player_input() {
    // Absolute paths must also fit the intervals used by the authoring proof.
    // Two periods include the u8 phase wrap and leave no unsampled phase.
    for (profile, kind, amplitude) in [
        (0, assets::FIGHTER, 64),
        (1, assets::DIVER, 32),
        (2, assets::FIGHTER, 32),
        (3, assets::FIGHTER, 32),
    ] {
        for origin in [-180, 180] {
            for age in 0..512 {
                let mut actor = Actor {
                    kind,
                    hp: 1,
                    origin_x: origin,
                    age,
                    pos: Vec3 {
                        x: origin,
                        y: 0,
                        z: 1200,
                    },
                    ..Actor::EMPTY
                };
                enemies::step(profile, &mut actor, &Vec3::default(), 0);
                assert!(
                    (origin - amplitude..=origin + amplitude).contains(&actor.pos.x),
                    "absolute path profile {profile}, kind {kind}, age {age}"
                );
            }
        }
    }
    // The steering map is monotone in both current X and target X. Checking
    // their endpoints at every phase proves this interval invariant by
    // induction for arbitrary sequences, including adversarial direction changes.
    for profile in [0, 3] {
        for origin in -180..=180 {
            for age in 0..256 {
                for x in [origin - 32, origin + 32] {
                    for player_x in [-100, 100] {
                        for z in [0, 1200] {
                            let mut actor = Actor {
                                kind: assets::DIVER,
                                hp: 1,
                                pos: Vec3 { x, y: 0, z },
                                origin_x: origin,
                                age,
                                ..Actor::EMPTY
                            };
                            enemies::step(
                                profile,
                                &mut actor,
                                &Vec3 {
                                    x: player_x,
                                    y: 0,
                                    z: PLAYER_Z,
                                },
                                0,
                            );
                            assert!((origin - 32..=origin + 32).contains(&actor.pos.x),
                                "profile {profile}, origin {origin}, age {age}, x {x}, target {player_x}, z {z}");
                        }
                    }
                }
            }
        }
    }
}

fn assert_crossing_finishes_before_near(stage: u8, kind: u8, shape: &Geometry) {
    let profile = assets::preview_stage_info(stage).enemy_profile;
    let player = Vec3 {
        x: 0,
        y: 0,
        z: PLAYER_Z,
    };
    for origin in [-115, 115] {
        for late in 0..1600u16 {
            let mut actor = Actor {
                kind,
                hp: 1,
                pos: Vec3 {
                    x: origin,
                    y: 0,
                    z: 1736 - late as i16,
                },
                origin_x: origin,
                age: late / 8,
                ..Actor::EMPTY
            };
            let settled = if profile == 1 {
                origin - origin.signum() * 220
            } else {
                origin
            };
            // 1736 is the lowest randomized entry depth. Larger entry depths
            // only delay the near-plane encounter; they cannot restart a feint.
            assert!(i32::from(actor.pos.z) + shape.envelope.min[2] > i32::from(NEAR));
            while actor.pos.z >= ACTOR_RETIRE_Z {
                let previous = actor;
                actor.age += 1;
                enemies::step(profile, &mut actor, &player, 0);
                let swept = shape
                    .envelope
                    .at(previous.pos)
                    .union(shape.envelope.at(actor.pos));
                if swept.crosses_near() {
                    assert_eq!((previous.pos.x, actor.pos.x), (settled, settled),
                        "stage {stage}, kind {kind}, late {late}, age {} crosses during inward travel", actor.age);
                }
                assert!(actor.age < 800);
            }
        }
    }
}

#[test]
fn authored_envelopes_prove_clearance_for_all_player_paths_and_late_handoffs() {
    // Unlike sampled gameplay, this proof discards Y entirely for flyers and
    // combines every pose with the entire reachable X interval. It therefore
    // covers every player Y, camera height, roll phase, and lateness 0..1599.
    for stage in 0..assets::SEGMENT_COUNT {
        let model_bytes = enemies::authored::model_bank(stage);
        let segment_bytes = enemies::authored::segment_bank(stage);
        let bank = AssetBank::new(&model_bytes).unwrap();
        let segment = Segment::new(&segment_bytes).unwrap();
        let profile = segment.stage_info().enemy_profile;
        let shapes: Vec<_> = (1..=bank.enemy_count())
            .map(|kind| geometry(stage, kind, &bank))
            .collect();
        if profile == 1 {
            assert_crossing_finishes_before_near(stage, assets::FIGHTER, &shapes[0]);
        } else if profile == 2 {
            assert_crossing_finishes_before_near(stage, assets::DIVER, &shapes[1]);
        }
        let mut game = Game::new();
        game.reset_scene(Mode::Attract, stage, 0);
        for index in 0..segment.spawn_count() {
            let spawn = segment.spawn(index);
            if !matches!(spawn.kind, assets::FIGHTER | assets::DIVER | assets::TURRET) {
                continue;
            }
            for repeat in [false, true] {
                for seed in 0..=u8::MAX {
                    game.actors.fill(Actor::EMPTY);
                    game.encounter_seed = seed;
                    game.spawn_encounter(spawn, 0, &bank, index | if repeat { 0x80 } else { 0 });
                    let actor = game.actors[0];
                    let mut bound = shapes[usize::from(spawn.kind - 1)].envelope.at(actor.pos);
                    bound.min[2] = i32::from(NEAR);
                    bound.max[2] = i32::from(NEAR);
                    if spawn.kind != assets::TURRET {
                        let (offset, amplitude) = match (profile, spawn.kind) {
                            (0, assets::FIGHTER) => (0, 64),
                            (1, assets::FIGHTER) => {
                                assert_ne!(actor.origin_x, 0);
                                (-i32::from(actor.origin_x.signum()) * 220, 0)
                            }
                            (2, assets::DIVER) => (0, 0),
                            _ => (0, 32),
                        };
                        bound.min[0] += offset - amplitude;
                        bound.max[0] += offset + amplitude;
                        // Infinite vertical freedom makes the horizontal proof
                        // independent of homing history and camera correlation.
                        bound.min[1] = -10000;
                        bound.max[1] = 10000;
                    }
                    assert!(bound.clearance_133(CAMERA_LOW, CAMERA_HIGH) >= MARGIN_133,
                        "stage {stage}, spawn {index}, kind {}, repeat {repeat}, seed {seed}, origin {:?}, envelope {bound:?}",
                        spawn.kind, actor.pos);
                }
            }
        }
    }
}

#[test]
fn camera_height_range_and_all_boss_components_remain_covered() {
    for player_y in -65..=65 {
        assert!(
            (CAMERA_LOW..=CAMERA_HIGH).contains(&i32::from(scene_camera_y(
                Mode::Playing,
                player_y,
                0
            )))
        );
    }
    for ticks in 0..1024 {
        assert!(
            (CAMERA_LOW..=CAMERA_HIGH).contains(&i32::from(scene_camera_y(
                Mode::Attract,
                0,
                ticks
            )))
        );
    }
    for stage in 0..assets::SEGMENT_COUNT {
        if assets::preview_stage_info(stage).boss_kind == 0 {
            continue;
        }
        let bytes = enemies::authored::model_bank(stage);
        let bank = AssetBank::new(&bytes).unwrap();
        let mut game = Game::new();
        game.reset_scene(Mode::Attract, stage, 0);
        game.stage_ticks = game.boss_start_ticks();
        for _ in 0..=150 {
            game.update_boss();
            game.shots.fill(Shot::EMPTY);
            for exposed in [false, true] {
                game.boss_left = if exposed { 0 } else { BOSS_SIDE_HP };
                game.boss_right = game.boss_left;
                let mut capture = Capture::default();
                game.render_combat(
                    &mut SceneBudget {
                        sink: &mut capture,
                        count: 0,
                        limit: 174,
                        camera_y: 0,
                        color_override: 0,
                        extra_flags: 0,
                    },
                    &bank,
                );
                let bounds = capture.0.expect("boss hull and side/core geometry");
                assert!(
                    bounds.min[2] > i32::from(NEAR),
                    "stage {stage}, boss z {}, exposed {exposed}: {bounds:?}",
                    game.boss_z
                );
            }
        }
        assert_eq!(game.boss_z, 680);
    }
}
