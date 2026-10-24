use super::tests::test_bank;
use super::*;
use crate::render::{Renderer, BAND_PIXELS};
use std::vec::Vec;

fn cave() -> Game {
    stage(1)
}

fn stage(level: u8) -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, level, 0);
    game
}

fn actor(kind: u8, x: i16, z: i16) -> Actor {
    Actor {
        kind,
        hp: 3,
        pos: Vec3 { x, y: 0, z },
        ..Actor::EMPTY
    }
}

fn draw(game: &Game, bank: &AssetBank<'_>, sink: &mut impl GeometrySink, limit: u16) -> u16 {
    let mut budget = SceneBudget {
        sink,
        count: 0,
        limit,
        camera_y: 0,
        color_override: 0,
        extra_flags: 0,
    };
    game.render_combat(&mut budget, bank);
    assert_eq!(budget.limit, limit);
    budget.count
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

#[derive(Default)]
struct Capture(Vec<([Vec3; 3], u8)>);

impl GeometrySink for Capture {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, _flags: u8) {
        self.0.push(([a, b, c], color));
    }
}

#[test]
fn every_stage_actor_keeps_its_model_palette_at_distance_and_budget_boundaries() {
    for level in 0..assets::SEGMENT_COUNT {
        let bank = test_bank(level);
        for kind in [
            assets::FIGHTER,
            assets::DIVER,
            assets::TURRET,
            assets::BUILDING,
            assets::ROCK,
        ] {
            for small in [false, true] {
                if small && (kind != assets::ROCK || level < 12) {
                    continue;
                }
                let mut game = stage(level);
                game.actors[0] = actor(kind, 0, 1200);
                if matches!(kind, assets::BUILDING | assets::ROCK) {
                    game.actors[0].hp = if small { 2 } else { 255 };
                }
                if small {
                    game.actors[0].flags = assets::SMALL_DEBRIS;
                }
                let mut detailed = Capture::default();
                assert_eq!(
                    draw(&game, &bank, &mut detailed, 200),
                    bank.face_count(kind) as u16,
                    "stage {level}, kind {kind} stays detailed at z=1200"
                );
                assert!(detailed.0.len() > 2);
                let mut distance_colors = None;
                for (z, limit) in [(1201, 200), (700, 2)] {
                    game.actors[0].pos.z = z;
                    let mut capture = Capture::default();
                    assert_eq!(draw(&game, &bank, &mut capture, limit), 2);
                    let colors: Vec<_> = capture.0.iter().map(|face| face.1).collect();
                    for color in &colors {
                        assert!(
                            detailed.0.iter().any(|face| face.1 == *color),
                            "stage {level}, kind {kind}, small {small}, z {z}: foreign color {color}"
                        );
                    }
                    if let Some(expected) = &distance_colors {
                        assert_eq!(
                            &colors, expected,
                            "stage {level}, kind {kind}, small {small}"
                        );
                    } else {
                        distance_colors = Some(colors);
                    }

                    // HP is gameplay state, not a palette selector. This also
                    // covers the former gray proxy used by invulnerable hazards.
                    let hp = game.actors[0].hp;
                    game.actors[0].hp = if hp == 255 { 3 } else { 255 };
                    let mut other_hp = Capture::default();
                    draw(&game, &bank, &mut other_hp, limit);
                    assert_eq!(other_hp.0, capture.0);
                    game.actors[0].hp = hp;

                    // A damage flash still overrides both facets, and clearing
                    // it restores the exact normal geometry and model colors.
                    game.actor_hit_flash_ticks[0] = ENEMY_HIT_FLASH_TICKS + 1;
                    let mut flashed = Capture::default();
                    draw(&game, &bank, &mut flashed, limit);
                    assert_eq!(flashed.0.len(), capture.0.len());
                    for (normal, white) in capture.0.iter().zip(&flashed.0) {
                        assert_eq!(normal.0, white.0);
                        assert_eq!(white.1, 1);
                    }
                    game.actor_hit_flash_ticks[0] = 0;
                    let mut restored = Capture::default();
                    draw(&game, &bank, &mut restored, limit);
                    assert_eq!(restored.0, capture.0);
                }
            }
        }
    }
}

#[test]
fn accent_first_models_use_the_visible_hull_colors_in_their_proxies() {
    for (level, kind, expected) in [
        (2, assets::DIVER, [9, 7]),
        (3, assets::DIVER, [9, 7]),
        (10, assets::FIGHTER, [11, 14]),
        (11, assets::FIGHTER, [11, 14]),
        (14, assets::DIVER, [3, 15]),
    ] {
        let bank = test_bank(level);
        for (z, limit) in [(1201, 200), (700, 2)] {
            let mut game = stage(level);
            game.actors[0] = actor(kind, 0, z);
            let mut capture = Capture::default();
            assert_eq!(draw(&game, &bank, &mut capture, limit), 2);
            assert_eq!(
                [capture.0[0].1, capture.0[1].1],
                expected,
                "stage {level}, kind {kind}, z {z}"
            );
        }
    }
}

#[test]
fn every_stage_reserves_complete_model_colored_proxies_for_later_actors() {
    for level in 0..assets::SEGMENT_COUNT {
        let bank = test_bank(level);
        for z in [700, 1201] {
            let mut game = stage(level);
            game.actors[0] = actor(assets::FIGHTER, -180, z);
            game.actors[ACTORS - 1] = actor(assets::DIVER, 180, z);
            let mut renderer = Renderer::new();
            assert_eq!(draw(&game, &bank, &mut renderer, 4), 4);
            assert_eq!((renderer.triangle_count, renderer.dropped), (4, 0));
            let mut expected = Renderer::new();
            for slot in [0, ACTORS - 1] {
                let mut single = stage(level);
                single.actors[slot] = game.actors[slot];
                assert_eq!(draw(&single, &bank, &mut expected, 2), 2);
            }
            assert_eq!(
                pixels(&mut renderer),
                pixels(&mut expected),
                "stage {level}, z {z}"
            );
        }
    }
}

#[test]
fn cave_flying_proxies_keep_their_model_palettes_and_silhouette_extents() {
    let bank = test_bank(1);
    for kind in [assets::FIGHTER, assets::DIVER] {
        let model_palette: Vec<_> = (0..bank.face_count(kind))
            .map(|face| bank.face(kind, face).1)
            .collect();
        let vertices: Vec<_> = (0..bank.vertex_count(kind))
            .map(|vertex| bank.vertex(kind, vertex))
            .collect();
        let bounds = |points: &[Vec3]| {
            (
                points.iter().map(|p| p.x).min().unwrap(),
                points.iter().map(|p| p.x).max().unwrap(),
                points.iter().map(|p| p.y).min().unwrap(),
                points.iter().map(|p| p.y).max().unwrap(),
            )
        };
        for (z, limit) in [(1201, 200), (700, 2)] {
            let mut game = cave();
            game.actors[0] = actor(kind, 0, z);
            let mut capture = Capture::default();
            assert_eq!(draw(&game, &bank, &mut capture, limit), 2);
            assert_eq!(capture.0.len(), 2);
            let points: Vec<_> = capture.0.iter().flat_map(|face| face.0).collect();
            assert_eq!(bounds(&points), bounds(&vertices), "kind {kind}, z {z}");
            for (_, color) in &capture.0 {
                assert_ne!(*color, 2, "the cave's flying enemies have no red facets");
                assert!(model_palette.contains(color), "kind {kind}, color {color}");
            }
            assert_ne!(capture.0[0].1, capture.0[1].1, "retain the faceted shading");
        }
    }
}

#[test]
fn cave_flying_proxies_survive_culling_and_replace_over_budget_meshes_completely() {
    let bank = test_bank(1);
    for kind in [assets::FIGHTER, assets::DIVER] {
        for (z, limit) in [(1201, 200), (700, 2)] {
            let mut game = cave();
            game.actors[0] = actor(kind, 0, z);
            if z == 700 {
                let mut detailed = Renderer::new();
                assert!(draw(&game, &bank, &mut detailed, 200) > 2);
            }
            let mut renderer = Renderer::new();
            assert_eq!(draw(&game, &bank, &mut renderer, limit), 2);
            assert_eq!((renderer.triangle_count, renderer.dropped), (2, 0));
            let mut capture = Capture::default();
            draw(&game, &bank, &mut capture, 2);
            let mut expected = Renderer::new();
            for ([a, b, c], color) in &capture.0 {
                expected.triangle(*a, *b, *c, *color, 0);
            }
            let actual = pixels(&mut renderer);
            assert_eq!(actual, pixels(&mut expected), "kind {kind}, z {z}");
            assert!(
                !actual.contains(&2),
                "no generic red proxy or partial mesh remains"
            );
            for (_, color) in &capture.0 {
                assert!(
                    actual.contains(color),
                    "missing facet for kind {kind}, z {z}"
                );
            }
        }
    }
}

#[test]
fn cave_flying_proxies_reserve_a_complete_later_actor_under_a_tight_budget() {
    let bank = test_bank(1);
    for z in [700, 1201] {
        let mut game = cave();
        game.actors[0] = actor(assets::FIGHTER, -180, z);
        game.actors[ACTORS - 1] = actor(assets::DIVER, 180, z);
        let mut renderer = Renderer::new();
        assert_eq!(draw(&game, &bank, &mut renderer, 4), 4);
        assert_eq!((renderer.triangle_count, renderer.dropped), (4, 0));
        let mut expected = Renderer::new();
        for slot in [0, ACTORS - 1] {
            let mut single = cave();
            single.actors[slot] = game.actors[slot];
            assert_eq!(draw(&single, &bank, &mut expected, 2), 2);
        }
        assert_eq!(pixels(&mut renderer), pixels(&mut expected), "z {z}");
    }
}
