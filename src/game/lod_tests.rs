use super::tests::test_bank;
use super::*;
use crate::render::{Renderer, BAND_PIXELS};
use std::vec::Vec;

fn cave() -> Game {
    let mut game = Game::new();
    game.reset_scene(Mode::Playing, 1, 0);
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
