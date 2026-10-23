use super::*;
use std::vec::Vec;

fn triangle(x: [i16; 3], y: [i16; 3], depth: i16, color: u8, flags: u8) -> Triangle {
    Triangle {
        x,
        y,
        depth,
        color,
        reserved: flags,
    }
}

fn signature(command: Triangle) -> ([i16; 3], [i16; 3], i16, u8, u8) {
    (
        command.x,
        command.y,
        command.depth,
        command.color,
        command.reserved,
    )
}

const BASE_COUNT: usize = 4;
fn source_scene(ink: u8) -> Vec<Triangle> {
    std::vec![
        triangle(
            [18, 142, 18],
            [12, 12, 184],
            500,
            5,
            crate::EXTERIOR_TERRAIN
        ),
        triangle(
            [64, 95, 64],
            [88, 88, 118],
            500,
            1,
            crate::TERRAIN_CLEAR_GEOMETRY
        ),
        // Ordinary objects do not cut or reshape the flat shadow layer.
        triangle([105, 155, 155], [45, 45, 145], 300, 5, 0),
        triangle([2, 15, 2], [24, 24, 39], 600, 5, 0),
        // Deliberately unordered depths: the suffix never participates in sorting.
        triangle(
            [24, 150, 24],
            [46, 46, 185],
            800,
            ink,
            crate::EXTERIOR_TERRAIN
        ),
        triangle([8, 37, 8], [20, 20, 72], 16, ink, crate::EXTERIOR_TERRAIN),
        triangle(
            [42, 145, 42],
            [36, 36, 192],
            900,
            ink,
            crate::EXTERIOR_TERRAIN
        ),
    ]
}

fn scene(ink: u8) -> Renderer {
    let commands = source_scene(ink);
    let mut renderer = Renderer::new();
    renderer.begin(6);
    renderer.triangle_count = commands.len() as u16;
    renderer.triangles[..commands.len()].copy_from_slice(&commands);
    renderer.finish_ground_shadows(BASE_COUNT as u16);
    renderer
}

fn coverage(command: Triangle, band: u8) -> [bool; BAND_PIXELS] {
    let mut renderer = Renderer::new();
    renderer.triangle_count = 1;
    renderer.triangles[0] = Triangle {
        color: 1,
        reserved: 0,
        ..command
    };
    let mut pixels = [0; BAND_PIXELS];
    renderer.render_band(band, &mut pixels);
    pixels.map(|pixel| pixel != 0)
}

/// Independent pixel ownership: sort the base, then paint every silhouette.
/// Only the existing terrain-clear coverage protects ordinary scene pixels.
fn expected(band: u8, ink: u8) -> [u8; BAND_PIXELS] {
    let commands = source_scene(ink);
    let mut order: Vec<usize> = (0..BASE_COUNT).collect();
    order.sort_by_key(|&index| core::cmp::Reverse(commands[index].depth));
    order.extend(BASE_COUNT..commands.len());
    let mut output = [6; BAND_PIXELS];
    for index in order {
        let command = commands[index];
        let mask = coverage(command, band);
        for pixel in 0..BAND_PIXELS {
            if !mask[pixel] {
                continue;
            }
            let old = output[pixel];
            if command.reserved & crate::TERRAIN_CLEAR_GEOMETRY != 0 {
                output[pixel] = command.color | TERRAIN_CLEAR_INK;
            } else if command.reserved & crate::EXTERIOR_TERRAIN == 0
                || old & TERRAIN_CLEAR_INK == 0
            {
                output[pixel] = command.color | (old & TERRAIN_CLEAR_INK);
            }
        }
    }
    output
}

#[test]
fn ground_layer_preserves_base_ties_and_leaves_suffix_unsorted() {
    let renderer = scene(9);
    let commands = source_scene(9);
    let expected_order = [3, 0, 1, 2, 4, 5, 6];
    let actual: Vec<_> = renderer.triangles[..renderer.triangle_count as usize]
        .iter()
        .copied()
        .map(signature)
        .collect();
    let expected: Vec<_> = expected_order
        .iter()
        .map(|&index| signature(commands[index]))
        .collect();
    assert_eq!(actual, expected);

    // Prefix and suffix boundaries remain valid across the 8-bit index limit.
    for base_count in [0, 1, 255, 256] {
        let mut full = Renderer::new();
        full.triangle_count = 256;
        for (index, command) in full.triangles.iter_mut().enumerate() {
            *command = triangle([index as i16, 160, 0], [0, 0, 200], index as i16, 0, 0);
        }
        full.finish_ground_shadows(base_count);
        let actual: Vec<_> = full.triangles.iter().map(|t| t.x[0]).collect();
        let expected: Vec<_> = (0..base_count as i16)
            .rev()
            .chain(base_count as i16..256)
            .collect();
        assert_eq!(actual, expected, "base_count={base_count}");
    }
}

#[test]
fn all_band_paths_draw_flat_shadows_and_preserve_existing_actor_coverage() {
    for ink in [0, 9, 11, 12] {
        let renderer = scene(ink);
        let mut protected = 0;
        let mut shadowed_object = 0;
        let commands = source_scene(ink);
        for band in 0..25 {
            let oracle = expected(band, ink);
            let player = coverage(commands[1], band);
            let object = coverage(commands[2], band);
            for pixel in 0..BAND_PIXELS {
                protected += usize::from(player[pixel] && oracle[pixel] == (1 | TERRAIN_CLEAR_INK));
                shadowed_object += usize::from(object[pixel] && oracle[pixel] == ink);
            }
            let mut pixels = [0; BAND_PIXELS];
            renderer.render_band(band, &mut pixels);
            assert_eq!(pixels, oracle, "classic band {band}");
            assert!(pixels.iter().all(|pixel| pixel & HUD_INK == 0));
            #[cfg(feature = "span-buffer")]
            {
                renderer.render_band_spans(band, &mut pixels, None);
                assert_eq!(pixels, oracle, "span band {band}");
            }
            let mut cells = crate::cells::CellBand::new();
            renderer.render_band_cells(band, &mut cells, None);
            let mut bitmap = [0; 320];
            let mut screen = [0; 40];
            let mut colors = [0; 40];
            cells.finish(&[0; 40], &[0; 40], &mut bitmap, &mut screen, &mut colors);
            pixels = crate::cells::unpack_band(&bitmap, &screen, &colors, 6);
            assert_eq!(pixels, oracle.map(|pixel| pixel & 15), "cell band {band}");
            #[cfg(feature = "span-buffer")]
            {
                renderer.render_band_span_cells(band, &mut cells, None);
                cells.finish(&[0; 40], &[0; 40], &mut bitmap, &mut screen, &mut colors);
                pixels = crate::cells::unpack_band(&bitmap, &screen, &colors, 6);
                assert_eq!(
                    pixels,
                    oracle.map(|pixel| pixel & 15),
                    "span cell band {band}"
                );
            }
        }
        assert!(protected > 100, "player protection must be exercised");
        assert!(
            shadowed_object > 100,
            "objects must not become shadow receivers"
        );
    }
}

#[test]
fn prepared_ground_layer_matches_reference_through_hidden_bands() {
    for hidden in [false, true] {
        let mut renderer = scene(11);
        let mut pass = renderer.raster_pass().unwrap();
        for band in 0..25 {
            let paint = !hidden || band % 3 != 1;
            let mut pixels = [0xa5; BAND_PIXELS];
            assert_eq!(pass.next_band(&mut pixels, paint), Some(band));
            assert_eq!(
                pixels,
                if paint {
                    expected(band, 11)
                } else {
                    [0xa5; BAND_PIXELS]
                },
                "band {band}, hidden={hidden}"
            );
        }
        assert_eq!(pass.next_band(&mut [0; BAND_PIXELS], true), None);
    }
}

fn clipped() -> [Vec3; 3] {
    [
        Vec3 { x: 0, y: 12, z: 4 },
        Vec3 {
            x: 12,
            y: -12,
            z: 64,
        },
        Vec3 {
            x: -12,
            y: -12,
            z: 64,
        },
    ]
}

#[test]
fn ground_shadow_append_uses_ordinary_near_clipping_without_receivers() {
    let [a, b, c] = clipped();
    for color in 0..16 {
        let mut ordinary = Renderer::new();
        ordinary.triangle(a, b, c, color, crate::EXTERIOR_TERRAIN);
        let mut shadow = Renderer::new();
        shadow.append_ground_shadow_triangle(a, b, c, color);
        assert_eq!(shadow.triangle_count, 2);
        assert_eq!(shadow.dropped, 0);
        let actual: Vec<_> = shadow.triangles[..2]
            .iter()
            .copied()
            .map(signature)
            .collect();
        let expected: Vec<_> = ordinary.triangles[..2]
            .iter()
            .copied()
            .map(signature)
            .collect();
        assert_eq!(actual, expected);
        assert!(shadow.triangles[..2]
            .iter()
            .all(|t| t.color == color && t.reserved == crate::EXTERIOR_TERRAIN));
    }
}

#[test]
fn ground_shadow_overflow_can_restore_the_complete_base_scene() {
    let mut renderer = Renderer::new();
    renderer.triangle_count = 255;
    for command in &mut renderer.triangles[..255] {
        *command = triangle([0, 160, 0], [0, 0, 200], 100, 5, 0);
    }
    let base: Vec<_> = renderer.triangles[..255]
        .iter()
        .copied()
        .map(signature)
        .collect();
    renderer.dropped = 3;
    let checkpoint = renderer.checkpoint();
    let [a, b, c] = clipped();
    renderer.append_ground_shadow_triangle(a, b, c, 11);
    assert_eq!((renderer.triangle_count, renderer.dropped), (256, 4));
    renderer.rollback(checkpoint);
    assert_eq!(renderer.checkpoint(), checkpoint);
    assert_eq!(
        renderer.triangles[..255]
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        base
    );
}
