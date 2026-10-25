//! Raster and motion regressions for the two physical cave crossings.
use super::*;
use crate::assets::STAGE_INFOS;
use crate::projection::project;
use crate::render::{Renderer, BAND_PIXELS, HEIGHT, WIDTH};
use std::vec::Vec;

#[derive(Default)]
struct Capture(Vec<([Vec3; 3], u8, u8)>);

impl GeometrySink for Capture {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        self.0.push(([a, b, c], color, flags));
    }
}

fn crossing(stage: usize, elapsed: u16) -> Capture {
    let mut scene = Capture::default();
    backdrop(&mut scene, &STAGE_INFOS[stage], elapsed, 0);
    scene
}

fn image(scene: &Capture, clear: u8, parallax: [i16; 2]) -> Vec<u8> {
    let mut renderer = Renderer::new();
    renderer.begin(clear);
    for &(mut points, color, flags) in &scene.0 {
        crate::render::drift_sky(&mut points, flags, parallax[0], parallax[1]);
        renderer.triangle(points[0], points[1], points[2], color, flags);
    }
    renderer.finish();
    assert_eq!(renderer.dropped, 0);
    let mut pixels = Vec::with_capacity(WIDTH * HEIGHT);
    let mut band = [0; BAND_PIXELS];
    for row in 0..25 {
        renderer.render_band(row, &mut band);
        pixels.extend_from_slice(&band);
    }
    pixels
}

fn normal_scene(theme: u8) -> Capture {
    let mut scene = Capture::default();
    for face in 0..14 {
        let (points, color) = backdrop_face(theme, face);
        scene.triangle(points[0], points[1], points[2], color, STATIC_SKY);
    }
    scene
}

fn posed_cave(elapsed: u16, entering: bool) -> Capture {
    let mut cave = normal_scene(4);
    for (points, _, _) in &mut cave.0 {
        cave_pose(points, elapsed, entering);
    }
    cave
}

const PARALLAX: [[i16; 2]; 5] = [[0, 0], [-100, -33], [-100, 33], [100, -33], [100, 33]];

#[test]
fn cave_mouth_rises_in_both_directions_and_widens_on_approach() {
    for entering in [true, false] {
        let mut previous_y = [i16::MAX; 42];
        let mut previous_width = 0;
        for elapsed in 0..400 {
            let cave = posed_cave(elapsed, entering);
            let projected: Vec<_> = cave.0.iter().flat_map(|f| f.0.map(project)).collect();
            for (index, &(_, y)) in projected.iter().enumerate() {
                assert!(
                    y <= previous_y[index],
                    "cave moved downward: entering={entering}, tick={elapsed}, vertex={index}"
                );
                previous_y[index] = y;
            }
            let left = projected.iter().map(|p| p.0).min().unwrap();
            let right = projected.iter().map(|p| p.0).max().unwrap();
            if entering {
                assert!(
                    right - left >= previous_width,
                    "mouth narrowed at {elapsed}"
                );
                previous_width = right - left;
            }
            if entering && elapsed == 0 {
                assert!(projected.iter().all(|p| p.1 >= HEIGHT as i16));
            }
            if !entering && elapsed == 399 {
                assert!(projected.iter().all(|p| p.1 < 0));
            }
        }
    }
    assert_eq!(posed_cave(399, true).0, normal_scene(4).0);
    assert_eq!(posed_cave(0, false).0, normal_scene(4).0);
}

#[test]
fn emitted_cave_faces_remain_connected_and_camera_facing() {
    for stage in 0..2 {
        for elapsed in 1..399 {
            let scene = crossing(stage, elapsed);
            assert!(scene.0.len() <= if stage == 0 { 30 } else { 28 });
            for &(points, color, _) in &scene.0 {
                let [a, b, c] = points;
                let signed_area = (b.x as i64 - a.x as i64) * (c.y as i64 - a.y as i64)
                    - (b.y as i64 - a.y as i64) * (c.x as i64 - a.x as i64);
                assert!(
                    signed_area < 0,
                    "back-facing layer at stage {stage}, tick {elapsed}"
                );
                assert!(color < 16);
                assert!(points.iter().all(|point| point.z > crate::render::NEAR));
            }
            let cave = &scene.0[scene.0.len() - 14..];
            assert_eq!(cave, posed_cave(elapsed, stage == 0).0);
            for fan in cave[5..].chunks_exact(3) {
                for pair in fan.windows(2) {
                    assert_eq!(pair[0].0[0], pair[1].0[0]);
                    assert_eq!(pair[0].0[2], pair[1].0[1]);
                }
            }
        }
    }
}

#[test]
fn moving_crystal_roots_stay_embedded_in_the_roof() {
    for entering in [true, false] {
        for elapsed in 0..400 {
            let cave = posed_cave(elapsed, entering);
            let roof = Capture(cave.0[..5].iter().map(|&(p, _, f)| (p, 1, f)).collect());
            for parallax in PARALLAX {
                let silhouette = image(&roof, 0, parallax);
                for fan in cave.0[5..].chunks_exact(3) {
                    let vertices: Vec<_> = fan.iter().flat_map(|face| face.0).collect();
                    for root in [
                        *vertices.iter().min_by_key(|p| p.x).unwrap(),
                        *vertices.iter().max_by_key(|p| p.x).unwrap(),
                    ] {
                        let mut sample = [root; 3];
                        crate::render::drift_sky(&mut sample, STATIC_SKY, parallax[0], parallax[1]);
                        let (x, y) = project(sample[0]);
                        if (0..WIDTH as i16).contains(&x) && (0..HEIGHT as i16).contains(&y) {
                            assert_eq!(
                                silhouette[y as usize * WIDTH + x as usize], 1,
                                "detached root: entering={entering}, tick={elapsed}, parallax={parallax:?}, at={x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn entry_opening_stays_dark_and_exit_does_not_drag_a_black_band() {
    for elapsed in 1..399 {
        let entry = crossing(0, elapsed);
        let exit = crossing(1, elapsed);
        for parallax in PARALLAX {
            let mut interior = [Vec3 {
                x: 0,
                y: 176,
                z: 2350,
            }; 3];
            cave_pose(&mut interior, elapsed, true);
            crate::render::drift_sky(&mut interior, STATIC_SKY, parallax[0], parallax[1]);
            let (x, y) = project(interior[0]);
            if (0..WIDTH as i16).contains(&x) && (0..HEIGHT as i16).contains(&y) {
                let pixels = image(&entry, sky_color(0, Some((4, elapsed))), parallax);
                assert_eq!(
                    pixels[y as usize * WIDTH + x as usize],
                    0,
                    "sky leaked into the entrance at tick {elapsed}, parallax {parallax:?}"
                );
            }
            if elapsed >= 100 {
                let pixels = image(&exit, sky_color(4, Some((1, elapsed))), parallax);
                assert!(!pixels.contains(&0),
                    "dark exit mask remains after the exterior lights, tick {elapsed}, parallax {parallax:?}");
            }
        }
    }
}

#[test]
fn cave_handoffs_preserve_adjacent_backdrops_without_a_last_frame_pop() {
    for stage in 0..2 {
        let info = STAGE_INFOS[stage];
        for (elapsed, theme) in [(0, info.theme), (399, info.next_theme)] {
            for parallax in PARALLAX {
                let expected = image(&normal_scene(theme), SKY_COLORS[theme as usize], parallax);
                let actual = image(
                    &crossing(stage, elapsed),
                    sky_color(info.theme, Some((info.next_theme, elapsed))),
                    parallax,
                );
                assert_eq!(
                    actual, expected,
                    "endpoint stage={stage}, tick={elapsed}, parallax={parallax:?}"
                );
            }
        }
        for parallax in PARALLAX {
            for (first, last) in [(0, 1), (398, 399)] {
                let a = image(
                    &crossing(stage, first),
                    sky_color(info.theme, Some((info.next_theme, first))),
                    parallax,
                );
                let b = image(
                    &crossing(stage, last),
                    sky_color(info.theme, Some((info.next_theme, last))),
                    parallax,
                );
                let changed = a.iter().zip(&b).filter(|(a, b)| a != b).count();
                assert!(changed < WIDTH * 8,
                    "handoff changes {changed} backdrop pixels: stage={stage}, ticks={first}->{last}, parallax={parallax:?}");
            }
        }
    }
}
