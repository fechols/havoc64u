use super::*;
use crate::render::{Renderer, BAND_PIXELS, NEAR, TRIANGLE_CAPACITY};
use std::vec::Vec;

fn point(x: i16, y: i16, z: i16) -> Vec3 {
    Vec3 { x, y, z }
}

fn front() -> [Vec3; 3] {
    [point(-24, -16, 128), point(0, 16, 128), point(24, -16, 128)]
}

fn clipped() -> [Vec3; 3] {
    [
        point(-16, -16, NEAR / 2),
        point(0, 16, 64),
        point(16, -16, 64),
    ]
}

fn emit(sink: &mut impl GeometrySink, points: [Vec3; 3], color: u8) {
    sink.triangle(points[0], points[1], points[2], color, 0);
}

fn budget(renderer: &mut Renderer, limit: u16) -> SceneBudget<'_, Renderer> {
    SceneBudget {
        sink: renderer,
        count: 0,
        limit,
        camera_y: 0,
        color_override: 0,
        extra_flags: 0,
    }
}

fn pixels(renderer: &mut Renderer) -> Vec<u8> {
    renderer.finish();
    let mut result = Vec::new();
    for band in 0..25 {
        let mut row = [0; BAND_PIXELS];
        renderer.render_band(band, &mut row);
        result.extend(row);
    }
    result
}

/// Four visible triangles form a flat square. Optional explicit reverse faces
/// consume model storage but cannot consume projected capacity from this view.
/// All slots share the fixture so actor LOD tests need no host authoring feature.
fn planar_bank(backface_copies: u8) -> [u8; assets::MODEL_SIZE] {
    let mut bytes = [0; assets::MODEL_SIZE];
    bytes[..4].copy_from_slice(b"SMA6");
    bytes[4] = assets::MODEL_COUNT;
    bytes[6] = 3;
    for enemy in 0..3 {
        let at = assets::ENEMY_SPECS_OFFSET + enemy * assets::ENEMY_SPEC_SIZE;
        bytes[at] = 3;
        bytes[at + 2] = 24;
        bytes[at + 3] = 24;
        bytes[at + 4] = 100;
        bytes[at + 6] = 8;
    }
    let vertices = [
        [-24i8, -16, 0],
        [-24, 16, 0],
        [24, 16, 0],
        [24, -16, 0],
        [0, 0, 0],
    ];
    let start = assets::MODEL_DATA_OFFSET;
    let faces = start + vertices.len() * assets::VERTEX_BYTES;
    for mesh in 0..assets::MODEL_COUNT as usize {
        let at = 16 + mesh * 8;
        bytes[at..at + 2].copy_from_slice(&(start as u16).to_le_bytes());
        bytes[at + 2] = vertices.len() as u8;
        bytes[at + 3] = 4 + backface_copies;
        bytes[at + 4..at + 6].copy_from_slice(&(faces as u16).to_le_bytes());
    }
    for (index, vertex) in vertices.iter().enumerate() {
        for (axis, &value) in vertex.iter().enumerate() {
            bytes[start + index * 3 + axis] = value as u8;
        }
    }
    for index in 0..4 + backface_copies as usize {
        let edge = (index & 3) as u8;
        let next = (edge + 1) & 3;
        let indices = if index < 4 {
            [edge, next, 4]
        } else {
            [edge, 4, next]
        };
        bytes[faces + index * 4..faces + index * 4 + 3].copy_from_slice(&indices);
        bytes[faces + index * 4 + 3] = 5;
    }
    bytes
}

fn actor(x: i16) -> Actor {
    Actor {
        kind: assets::FIGHTER,
        hp: 3,
        pos: point(x, 0, 500),
        ..Actor::EMPTY
    }
}

#[test]
fn rejected_faces_cost_nothing_and_near_clipping_costs_both_commands() {
    let mut renderer = Renderer::new();
    let mut scene = budget(&mut renderer, 2);
    let start = scene.begin_group();
    let [a, b, c] = front();
    emit(&mut scene, [a, c, b], 7);
    emit(&mut scene, front().map(|p| point(p.x + 4000, p.y, p.z)), 7);
    emit(&mut scene, [a, a, a], 7);
    assert_eq!(scene.count, 0);
    emit(&mut scene, clipped(), 5);
    assert_eq!(scene.count, 2);
    assert!(scene.finish_group(start));
    assert_eq!(scene.sink.triangle_count, 2);
    assert_eq!(
        scene.sink.remaining_commands(),
        TRIANGLE_CAPACITY as u16 - 2
    );
}

#[test]
fn exact_limit_mesh_accepts_late_backfaces_without_truncation() {
    for backfaces in [0, 4, 12] {
        let bytes = planar_bank(backfaces);
        let bank = AssetBank::new(&bytes).unwrap();
        let mut renderer = Renderer::new();
        let mut scene = budget(&mut renderer, 4);
        let start = scene.begin_group();
        emit_mesh(
            &mut scene,
            &bank,
            assets::FIGHTER,
            point(0, 0, 128),
            0,
            0,
            0,
            false,
        );
        assert_eq!(scene.count, 4, "extra backs: {backfaces}");
        assert!(scene.finish_group(start));
        assert_eq!((scene.sink.triangle_count, scene.sink.dropped), (4, 0));
        assert!(pixels(&mut renderer).contains(&5));
    }
}

#[test]
fn over_budget_group_restores_prior_geometry_and_its_accounting() {
    let mut renderer = Renderer::new();
    let mut expected = Renderer::new();
    emit(&mut expected, front(), 5);
    {
        let mut scene = budget(&mut renderer, 2);
        let initial = scene.begin_group();
        emit(&mut scene, front(), 5);
        assert!(scene.finish_group(initial));
        let candidate = scene.begin_group();
        emit(&mut scene, clipped(), 7);
        assert_eq!(scene.count, 3);
        assert!(!scene.finish_group(candidate));
        assert_eq!(
            (scene.count, scene.sink.triangle_count, scene.sink.dropped),
            (1, 1, 0)
        );
        assert_eq!(scene.sink.remaining_commands(), 255);
    }
    assert_eq!(pixels(&mut renderer), pixels(&mut expected));
}

#[test]
fn physical_capacity_failure_rolls_back_partial_clip_and_dropped_counter() {
    let mut renderer = Renderer::new();
    for _ in 0..TRIANGLE_CAPACITY - 1 {
        emit(&mut renderer, front(), 5);
    }
    // Preserve an earlier diagnostic count as well as the accepted commands.
    renderer.dropped = 7;
    let mut scene = budget(&mut renderer, 8);
    let candidate = scene.begin_group();
    emit(&mut scene, clipped(), 7);
    assert_eq!(
        (scene.count, scene.sink.triangle_count, scene.sink.dropped),
        (1, 256, 8)
    );
    assert!(!scene.finish_group(candidate));
    assert_eq!(
        (scene.count, scene.sink.triangle_count, scene.sink.dropped),
        (0, 255, 7)
    );
    assert_eq!(scene.sink.remaining_commands(), 1);
    let last = scene.begin_group();
    emit(&mut scene, front(), 5);
    assert!(scene.finish_group(last));
    assert_eq!(scene.sink.remaining_commands(), 0);
    let rejected = scene.begin_group();
    emit(&mut scene, front(), 7);
    assert!(!scene.finish_group(rejected));
    assert_eq!(
        (scene.count, scene.sink.triangle_count, scene.sink.dropped),
        (1, 256, 7)
    );
    assert_eq!(scene.sink.remaining_commands(), 0);
}

#[test]
fn failed_full_actor_becomes_a_complete_proxy_and_preserves_later_active_actor() {
    let bytes = planar_bank(0);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut game = Game::new();
    game.actors[0] = actor(-90);
    game.actors[ACTORS - 1] = actor(90);
    let mut renderer = Renderer::new();
    {
        let mut scene = budget(&mut renderer, 4);
        game.render_combat(&mut scene, &bank);
        assert_eq!(
            (scene.count, scene.sink.triangle_count, scene.sink.dropped),
            (4, 4, 0)
        );
        assert_eq!(scene.limit, 4);
    }
    let mut expected = Renderer::new();
    for slot in [0, ACTORS - 1] {
        diamond(
            &mut expected,
            game.actors[slot].pos,
            assets::enemy_scaled(22),
            2,
        );
    }
    assert_eq!(pixels(&mut renderer), pixels(&mut expected));
}

#[test]
fn extra_mesh_backfaces_and_empty_actor_slots_do_not_reduce_accepted_detail() {
    let mut reference = None;
    for backfaces in [0, 4, 12] {
        let bytes = planar_bank(backfaces);
        let bank = AssetBank::new(&bytes).unwrap();
        let mut game = Game::new();
        game.actors[0] = actor(-90);
        game.actors[ACTORS - 1] = actor(90);
        let mut renderer = Renderer::new();
        {
            let mut scene = budget(&mut renderer, 6);
            game.render_combat(&mut scene, &bank);
            assert_eq!(
                (scene.count, scene.sink.triangle_count, scene.sink.dropped),
                (6, 6, 0)
            );
        }
        let actual = pixels(&mut renderer);
        assert!(actual.contains(&5), "the first actor retains its full mesh");
        assert!(
            actual.contains(&2),
            "the last actor retains its complete proxy"
        );
        if let Some(expected) = &reference {
            assert_eq!(&actual, expected, "extra backs changed LOD: {backfaces}");
        } else {
            reference = Some(actual);
        }
    }
}
