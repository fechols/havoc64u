use havoc::render::{draw_bar, draw_text, pack_band, Renderer, HUD_INK, NEAR};
use havoc::{GeometrySink, Vec3};

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn v(x: i16, y: i16, z: i16) -> Vec3 {
    Vec3 { x, y, z }
}

fn frame(renderer: &Renderer) -> Vec<u8> {
    let mut result = Vec::with_capacity(160 * 200);
    let mut band = [0; 1280];
    for row in 0..25 {
        renderer.render_band(row, &mut band);
        result.extend_from_slice(&band);
    }
    result
}

fn cell_colors(screen: u8, color: u8) -> [u8; 4] {
    [0, screen >> 4, screen & 15, color & 15]
}

fn decode(bitmap: &[u8; 320], screen: &[u8; 40], colors: &[u8; 40]) -> [u8; 1280] {
    let mut pixels = [0; 1280];
    for cell in 0..40 {
        let palette = cell_colors(screen[cell], colors[cell]);
        for row in 0..8 {
            for column in 0..4 {
                pixels[row * 160 + cell * 4 + column] =
                    palette[((bitmap[cell * 8 + row] >> (6 - column * 2)) & 3) as usize];
            }
        }
    }
    pixels
}

#[test]
fn adjacent_triangles_cover_rectangle_without_cracks_or_spill() {
    let mut renderer = Renderer::new();
    renderer.begin(0);
    renderer.triangle(v(-32, -32, 128), v(-32, 32, 128), v(32, 32, 128), 5, 0);
    renderer.triangle(v(-32, -32, 128), v(32, 32, 128), v(32, -32, 128), 5, 0);
    renderer.finish();
    let pixels = frame(&renderer);
    for y in 0..200 {
        for x in 0..160 {
            let expected = if (60..100).contains(&x) && (66..133).contains(&y) {
                5
            } else {
                0
            };
            assert_eq!(pixels[y * 160 + x], expected, "pixel {x},{y}");
        }
    }
}

#[test]
fn near_plane_clips_and_backfaces_are_culled() {
    let mut renderer = Renderer::new();
    // Clipping preserves the outward winding: one rear vertex leaves a quad,
    // two leave a triangle. Reversing any case must cull every output piece.
    for (points, commands) in [
        ([v(-32, -32, 128), v(0, 32, 128), v(32, -32, 128)], 1),
        ([v(-16, -16, NEAR / 2), v(0, 16, 64), v(16, -16, 64)], 2),
        (
            [v(-16, -16, NEAR / 2), v(0, 16, 64), v(16, -16, NEAR / 2)],
            1,
        ),
    ] {
        renderer.begin(0);
        renderer.triangle(points[0], points[1], points[2], 2, 0);
        assert_eq!(renderer.triangle_count, commands);
        assert!(frame(&renderer).contains(&2));
        for flags in 0..=u8::MAX {
            renderer.begin(0);
            renderer.triangle(points[0], points[2], points[1], 2, flags);
            assert_eq!(renderer.triangle_count, 0, "flags {flags} bypassed culling");
            assert_eq!(renderer.dropped, 0);
        }
    }
    renderer.begin(0);
    renderer.triangle(
        v(-20, -20, 0),
        v(0, 20, NEAR / 2),
        v(20, -20, NEAR - 1),
        2,
        0,
    );
    renderer.triangle(v(0, 0, 128), v(0, 0, 128), v(0, 0, 128), 2, 0);
    assert_eq!(renderer.triangle_count, 0);
}

#[test]
fn painter_sort_is_far_to_near_and_stable_for_ties() {
    let mut renderer = Renderer::new();
    renderer.begin(0);
    renderer.triangle(v(-32, -32, 128), v(0, 32, 128), v(32, -32, 128), 5, 0);
    renderer.triangle(v(-64, -64, 256), v(0, 64, 256), v(64, -64, 256), 2, 0);
    renderer.finish();
    assert_eq!(frame(&renderer)[100 * 160 + 80], 5);
    renderer.triangle(v(-32, -32, 128), v(0, 32, 128), v(32, -32, 128), 7, 0);
    renderer.finish();
    assert_eq!(frame(&renderer)[100 * 160 + 80], 7);
}

#[test]
fn command_overflow_is_bounded_and_resettable() {
    let mut renderer = Renderer::new();
    for _ in 0..300 {
        renderer.triangle(v(-32, -32, 128), v(0, 32, 128), v(32, -32, 128), 1, 0);
    }
    assert_eq!(renderer.triangle_count, 256);
    assert_eq!(renderer.dropped, 44);
    renderer.begin(6);
    assert_eq!(renderer.triangle_count, 0);
    assert_eq!(renderer.dropped, 0);
    assert!(frame(&renderer).iter().all(|&pixel| pixel == 6));
}

#[test]
fn extreme_offscreen_geometry_is_safe() {
    let mut renderer = Renderer::new();
    renderer.triangle(
        v(i16::MIN, i16::MIN, 32),
        v(0, i16::MAX, 32),
        v(i16::MAX, i16::MIN, 32),
        3,
        0,
    );
    renderer.triangle(
        v(i16::MIN, i16::MIN, i16::MIN),
        v(0, i16::MAX, i16::MAX),
        v(i16::MAX, i16::MIN, 32),
        2,
        0,
    );
    renderer.finish();
    assert!(frame(&renderer).iter().all(|&pixel| pixel < 16));
}

#[test]
fn native_cell_packing_preserves_every_color_when_it_fits() {
    let mut pixels = [0u8; 1280];
    for cell in 0..40 {
        let palette = [
            0,
            (cell % 15 + 1) as u8,
            ((cell + 5) % 15 + 1) as u8,
            ((cell + 10) % 15 + 1) as u8,
        ];
        for row in 0..8 {
            for col in 0..4 {
                pixels[row * 160 + cell * 4 + col] = palette[(row + col) % 4];
            }
        }
    }
    let mut bitmap = [0; 320];
    let mut screen = [0; 40];
    let mut colors = [0; 40];
    pack_band(
        &pixels,
        &[0; 40],
        &[0; 40],
        0,
        &mut bitmap,
        &mut screen,
        &mut colors,
    );
    assert_eq!(decode(&bitmap, &screen, &colors), pixels);
    let previous_bitmap = bitmap;
    let previous_screen = screen;
    let previous_colors = colors;
    pack_band(
        &pixels,
        &previous_screen,
        &previous_colors,
        0,
        &mut bitmap,
        &mut screen,
        &mut colors,
    );
    assert_eq!(bitmap, previous_bitmap);
    assert_eq!(screen, previous_screen);
    assert_eq!(colors, previous_colors);
}

#[test]
fn palette_overflow_protects_hud_and_keeps_retained_slots() {
    let mut pixels = [0u8; 1280];
    for row in 0..8 {
        for col in 0..4 {
            pixels[row * 160 + col] = if row < 4 {
                2
            } else if row < 6 {
                5
            } else {
                6
            };
        }
    }
    pixels[0] = HUD_INK | 1;
    let mut old_screen = [0; 40];
    let mut old_color = [0; 40];
    old_screen[0] = 0x52;
    old_color[0] = 6;
    let mut bitmap = [0; 320];
    let mut screen = [0; 40];
    let mut colors = [0; 40];
    pack_band(
        &pixels,
        &old_screen,
        &old_color,
        0,
        &mut bitmap,
        &mut screen,
        &mut colors,
    );
    assert_eq!(screen[0], 0x52);
    assert_eq!(colors[0], 1);
    assert_eq!(decode(&bitmap, &screen, &colors)[0], 1);
    assert!(colors.iter().all(|&value| value < 16));
}

#[test]
fn hud_text_and_bars_clip_to_the_current_band() {
    let mut band = [0; 1280];
    draw_text(0, &mut band, -1, 6, b"SHIELD", 1);
    assert!(band[..6 * 160].iter().all(|&pixel| pixel == 0));
    assert!(band[6 * 160..].iter().any(|&pixel| pixel == HUD_INK | 1));
    band.fill(0);
    draw_bar(1, &mut band, 156, 8, 16, 2, 50, 100, 5);
    assert_eq!(&band[156..160], &[HUD_INK | 5; 4]);
    assert!(band[2 * 160..].iter().all(|&pixel| pixel == 0));
    draw_bar(1, &mut band, 0, 8, 8, 1, 20, 0, 5);
    assert_eq!(&band[..8], &[11; 8]);
}

#[test]
fn cached_mesh_transforms_match_direct_reference_emissions() {
    use havoc::{
        assets::{AssetBank, MODEL_COUNT, MODEL_DATA_OFFSET},
        emit_mesh, math,
    };
    #[derive(Default)]
    struct Capture(Vec<(Vec3, Vec3, Vec3, u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push((a, b, c, color, flags));
        }
    }
    for vertex_count in [4u8, 40] {
        let mut bytes = baked_assets::model_bank(0);
        let face_offset = MODEL_DATA_OFFSET + vertex_count as usize * 3;
        for id in 0..MODEL_COUNT as usize {
            let at = 16 + id * 8;
            bytes[at..at + 2].copy_from_slice(&(MODEL_DATA_OFFSET as u16).to_le_bytes());
            bytes[at + 2] = vertex_count;
            bytes[at + 3] = 2;
            bytes[at + 4..at + 6].copy_from_slice(&(face_offset as u16).to_le_bytes());
            bytes[at + 6] = 1;
            bytes[at + 7] = 0;
        }
        for i in 0..vertex_count as usize {
            let point = [i as i8 * 3 - 20, i as i8 - 15, 12 - i as i8];
            for axis in 0..3 {
                bytes[MODEL_DATA_OFFSET + i * 3 + axis] = point[axis] as u8;
            }
        }
        bytes[face_offset..face_offset + 8].copy_from_slice(&[
            0,
            1,
            2,
            5,
            1,
            vertex_count - 1,
            2,
            7,
        ]);
        let bank = AssetBank::new(&bytes).unwrap();
        for (yaw, pitch, roll, half) in [(0, 0, 0, false), (128, 0, 0, false), (23, 51, 209, true)]
        {
            let position = v(70, -40, 700);
            let matrix = math::RotationMatrix::new(yaw, pitch, roll);
            let shift = 8 - bank.shift(0) + half as u8;
            let direct = |index| {
                let rotated = matrix.apply(bank.vertex_bytes(0, index), shift);
                v(
                    rotated.x.saturating_add(position.x),
                    rotated.y.saturating_add(position.y),
                    rotated.z.saturating_add(position.z),
                )
            };
            let mut actual = Capture::default();
            emit_mesh(&mut actual, &bank, 0, position, yaw, pitch, roll, half);
            let expected: Vec<_> = (0..2)
                .map(|face| {
                    let (indices, color) = bank.face(0, face);
                    (
                        direct(indices[0]),
                        direct(indices[1]),
                        direct(indices[2]),
                        color,
                        0,
                    )
                })
                .collect();
            assert_eq!(actual.0, expected);
        }
    }
}
