use havoc::{
    assets::{AssetBank, MEDAL, MODEL_COUNT, MODEL_SIZE, PLAYER},
    render::{Renderer, BAND_PIXELS, PALETTE_RGB},
    victory, GeometrySink, Vec3,
};
use std::collections::BTreeMap;

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

#[derive(Default)]
struct Capture(Vec<([Vec3; 3], u8, u8)>);
impl GeometrySink for Capture {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        self.0.push(([a, b, c], color, flags));
    }
}

#[test]
fn all_banks_include_a_closed_thick_coin_and_protruding_player_relief_within_budget() {
    for level in 0..havoc::assets::SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        assert_eq!(bytes.len(), MODEL_SIZE);
        let bank = AssetBank::new(&bytes).unwrap();
        assert_eq!(bank.mesh_count(), MODEL_COUNT);
        assert_eq!((bank.vertex_count(MEDAL), bank.face_count(MEDAL)), (50, 88));
        let mut edges = BTreeMap::<(u8, u8), (u8, i8)>::new();
        for face in 0..bank.face_count(MEDAL) {
            let (indices, color) = bank.face(MEDAL, face);
            assert!(matches!(color, 7 | 8 | 9));
            let [a, b, c] = indices.map(|index| bank.vertex(MEDAL, index));
            let ab = [b.x - a.x, b.y - a.y, b.z - a.z].map(i32::from);
            let ac = [c.x - a.x, c.y - a.y, c.z - a.z].map(i32::from);
            assert_ne!(
                [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0]
                ],
                [0; 3]
            );
            if face < 70 {
                for [from, to] in [
                    [indices[0], indices[1]],
                    [indices[1], indices[2]],
                    [indices[2], indices[0]],
                ] {
                    let edge = edges.entry((from.min(to), from.max(to))).or_default();
                    edge.0 += 1;
                    edge.1 += if from < to { 1 } else { -1 };
                }
            }
        }
        assert!(
            edges.values().all(|&edge| edge == (2, 0)),
            "coin body must be closed with consistent outward winding"
        );
        for index in 0..12 {
            assert_eq!(
                bank.vertex(MEDAL, index + 24).z - bank.vertex(MEDAL, index).z,
                18
            );
            assert_eq!(bank.vertex(MEDAL, index + 12).z, -14);
        }
        for (index, player_index) in [0, 2, 6, 7, 5, 1].into_iter().enumerate() {
            let base = bank.vertex(MEDAL, 37 + index as u8);
            let source = bank.vertex(PLAYER, player_index);
            assert_eq!(
                (base.x, base.y, base.z),
                (source.x * 44 / 36, (source.z - 4) * 44 / 36, -14)
            );
        }
        for (index, player_index) in [0, 1, 2, 3, 5, 6, 7].into_iter().enumerate() {
            let top = bank.vertex(MEDAL, 43 + index as u8);
            let source = bank.vertex(PLAYER, player_index);
            assert_eq!(
                (top.x, top.y, top.z),
                (source.x, source.z - 4, -26 - source.y / 2)
            );
        }
        assert_eq!(
            bank.vertex(MEDAL, 46).z,
            -32,
            "cockpit remains raised above the wings"
        );
        assert_eq!(
            bank.vertex(MEDAL, 49).z,
            -30,
            "aft fin retains shallower raised detail"
        );
    }
}

#[test]
fn ship_relief_has_one_attached_footprint_and_an_unbroken_outward_upper_hull() {
    let bytes = baked_assets::model_bank(3);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut edges = BTreeMap::<(u8, u8), (u8, i8)>::new();
    for face in 70..88 {
        let (indices, color) = bank.face(MEDAL, face);
        assert!(if face < 82 {
            color == 9
        } else {
            matches!(color, 7 | 8)
        });
        for [from, to] in [
            [indices[0], indices[1]],
            [indices[1], indices[2]],
            [indices[2], indices[0]],
        ] {
            let edge = edges.entry((from.min(to), from.max(to))).or_default();
            edge.0 += 1;
            edge.1 += if from < to { 1 } else { -1 };
        }
    }
    let mut footprint_edges = 0;
    for ((from, to), edge) in edges {
        if from < 43 && to < 43 {
            assert_eq!(edge.0, 1, "only the base attached to the coin is open");
            footprint_edges += 1;
        } else {
            assert_eq!(
                edge,
                (2, 0),
                "upper hull and bevel must share opposite edge winding"
            );
        }
    }
    assert_eq!(footprint_edges, 6);
    let top_index = [43, 44, 45, 46, u8::MAX, 47, 48, 49];
    for (offset, player_face) in [0, 1, 4, 5, 6, 7].into_iter().enumerate() {
        let ([a, b, c], _) = bank.face(PLAYER, player_face);
        let (indices, _) = bank.face(MEDAL, 82 + offset as u8);
        assert_eq!(
            indices,
            [
                top_index[a as usize],
                top_index[c as usize],
                top_index[b as usize]
            ]
        );
        let [a, b, c] = indices.map(|index| bank.vertex(MEDAL, index));
        let normal_z = i32::from(b.x - a.x) * i32::from(c.y - a.y)
            - i32::from(b.y - a.y) * i32::from(c.x - a.x);
        assert!(
            normal_z < 0,
            "upper hull must face the camera for backface culling"
        );
    }
}

#[test]
fn every_palette_color_fades_monotonically_to_black_at_two_seconds() {
    let luminance = |color: u8| {
        let [r, g, b] = PALETTE_RGB[color as usize].map(u32::from);
        r * 299 + g * 587 + b * 114
    };
    for color in 0..16 {
        assert_eq!(victory::fade_color(color, 0), color);
        let mut previous = luminance(color);
        for tick in 1..=victory::FADE_TICKS {
            let actual = luminance(victory::fade_color(color, tick));
            assert!(actual <= previous, "color {color} brightened at {tick}");
            previous = actual;
        }
        assert_eq!(victory::fade_color(color, victory::FADE_TICKS), 0);
        assert_eq!(victory::fade_color(color, u16::MAX), 0);
    }
}

#[test]
fn entire_reveal_and_idle_sway_keep_all_vertices_well_clear_of_near_plane() {
    let bytes = baked_assets::model_bank(3);
    let bank = AssetBank::new(&bytes).unwrap();
    for (tick, animation) in (0..=victory::REVEAL_TICKS)
        .map(|tick| (tick, tick))
        .chain((0..1024).map(|animation| (victory::REVEAL_TICKS, animation)))
    {
        let mut capture = Capture::default();
        victory::emit(&mut capture, &bank, tick, animation);
        assert_eq!(capture.0.len(), 88);
        if tick == victory::REVEAL_TICKS {
            for point in capture.0.iter().skip(70).flat_map(|(points, _, _)| points) {
                let (_, y) = havoc::projection::project(*point);
                assert!(
                    (28..177).contains(&y),
                    "raised ship overlaps captions at idle phase {animation}"
                );
            }
        }
        let mut renderer = Renderer::new();
        renderer.begin(0);
        for (points, color, flags) in capture.0 {
            assert!(
                points.iter().all(|p| p.z >= 64),
                "unsafe depth at {tick}/{animation}"
            );
            assert_eq!(flags, 0, "solid medal uses backface culling");
            renderer.triangle(points[0], points[1], points[2], color, flags);
        }
        assert_eq!(renderer.dropped, 0);
        assert!(renderer.triangle_count > 0 && renderer.triangle_count <= 88);
        if tick == victory::REVEAL_TICKS && animation % 4 == 0 {
            renderer.finish();
            let mut band = [0; BAND_PIXELS];
            for (row, indices) in [
                (0, [80, 80]),
                (12, [4 * 160, 4 * 160 + 159]),
                (24, [7 * 160 + 80, 7 * 160 + 80]),
            ] {
                renderer.render_band(row, &mut band);
                assert!(
                    indices
                        .into_iter()
                        .all(|index| matches!(band[index], 7 | 8 | 9)),
                    "idle medal stopped covering an edge at phase {animation}, band {row}"
                );
            }
        }
    }
}

#[test]
fn reveal_grows_to_crop_all_four_screen_edges_and_retains_visible_ship_relief() {
    let bytes = baked_assets::model_bank(3);
    let bank = AssetBank::new(&bytes).unwrap();
    let mut previous_width = 0;
    for ticks in [0, 100, 200, 300, 400] {
        let mut capture = Capture::default();
        victory::emit(&mut capture, &bank, ticks, 0);
        let xs: Vec<_> = capture
            .0
            .iter()
            .flat_map(|(points, _, _)| points)
            .map(|p| havoc::projection::project(*p).0)
            .collect();
        let width = xs.iter().max().unwrap() - xs.iter().min().unwrap();
        assert!(width > previous_width, "medal failed to zoom at {ticks}");
        previous_width = width;
    }
    let mut renderer = Renderer::new();
    renderer.begin(0);
    victory::emit(&mut renderer, &bank, 400, 0);
    renderer.finish();
    let mut pixels = Vec::new();
    for band in 0..25 {
        let mut row = [0; BAND_PIXELS];
        renderer.render_band(band, &mut row);
        pixels.extend_from_slice(&row);
    }
    for (x, y) in [(0, 100), (159, 100), (80, 0), (80, 199)] {
        assert!(
            matches!(pixels[y * 160 + x], 7 | 8 | 9),
            "medal does not cross edge {x}/{y}"
        );
    }
    assert!(
        pixels.iter().filter(|&&color| color == 7).count() > 1500,
        "raised ship's lit facets must remain visible"
    );
    assert!(
        pixels
            .chunks_exact(160)
            .skip(28)
            .take(149)
            .flat_map(|row| &row[30..130])
            .filter(|&&color| color == 9)
            .count()
            > 100,
        "ship's brown bevel must visibly establish its relief"
    );
}
