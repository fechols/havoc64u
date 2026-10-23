use super::*;
use crate::{
    render::{BAND_PIXELS, TRIANGLE_CAPACITY},
    GeometrySink,
};
use std::vec::Vec;

fn caster(shape: u8) -> Caster {
    Caster {
        center: Vec3 {
            x: 0,
            y: 30,
            z: 480,
        },
        half_x: 80,
        half_z: 100,
        shape,
    }
}

fn base(renderer: &mut Renderer, count: usize) {
    for _ in 0..count {
        renderer.triangle(
            Vec3 {
                x: 0,
                y: 20,
                z: 128,
            },
            Vec3 {
                x: 20,
                y: -20,
                z: 128,
            },
            Vec3 {
                x: -20,
                y: -20,
                z: 128,
            },
            5,
            0,
        );
    }
    assert_eq!(renderer.triangle_count as usize, count);
}

fn pixels(renderer: &Renderer) -> Vec<u8> {
    let mut result = Vec::new();
    for band in 0..25 {
        let mut row = [0; BAND_PIXELS];
        renderer.render_band(band, &mut row);
        result.extend_from_slice(&row);
    }
    result
}

#[test]
fn shift_outlines_exactly_preserve_baked_template_scaling_and_heading() {
    const TEMPLATES: [[(i16, i16); 6]; 4] = [
        [
            (0, 64),
            (64, -16),
            (18, -56),
            (0, -64),
            (-18, -56),
            (-64, -16),
        ],
        [
            (-40, 64),
            (40, 64),
            (64, 32),
            (64, -64),
            (-64, -64),
            (-64, 32),
        ],
        [(0, 64), (64, 0), (0, -64), (-64, 0), (0, 0), (0, 0)],
        [(-64, 64), (64, 64), (64, -64), (-64, -64), (0, 0), (0, 0)],
    ];
    for extent in 1..=4096 {
        for family in 1..=4 {
            for heading in 0..4 {
                let source = Caster {
                    half_x: extent,
                    half_z: 4097 - extent,
                    shape: family | heading << 4,
                    ..caster(family)
                };
                let (actual, len) = footprint(source, -145);
                assert_eq!(len, if family < 3 { 6 } else { 4 });
                for index in 0..len {
                    let (a, b) = TEMPLATES[family as usize - 1][index];
                    let x = (a as i32 * source.half_x as i32 / 64) as i16;
                    let z = (b as i32 * source.half_z as i32 / 64) as i16;
                    let (x, z) = match heading {
                        1 => (z, -x),
                        2 => (-x, -z),
                        3 => (-z, x),
                        _ => (x, z),
                    };
                    assert_eq!(
                        actual[index],
                        Vec3 {
                            x,
                            y: -145,
                            z: 480 + z
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn casters_at_or_below_the_plane_emit_nothing() {
    for y in [-106, -105] {
        let mut renderer = Renderer::new();
        let mut frame = ShadowFrame::new();
        frame.push_caster(Caster {
            center: Vec3 {
                y,
                ..caster(1).center
            },
            ..caster(1)
        });
        frame.render(&mut renderer, -105, 11);
        assert_eq!(
            (
                frame.shadow_commands(),
                frame.rendered_casters(),
                frame.omitted_casters()
            ),
            (0, 0, 0)
        );
    }
}

#[test]
fn capacity_failure_rolls_back_the_entire_silhouette_even_after_near_clipping() {
    for source in [
        caster(1),
        Caster {
            center: Vec3 { x: 0, y: 30, z: 10 },
            half_x: 8,
            half_z: 12,
            shape: 4,
        },
    ] {
        let plane = if source.center.z == 10 { -2 } else { -105 };
        let mut reference = Renderer::new();
        let mut frame = ShadowFrame::new();
        frame.push_caster(source);
        frame.render(&mut reference, plane, 11);
        let required = frame.shadow_commands() as usize;
        assert!(required >= 2);
        for remaining in 0..=required {
            let mut renderer = Renderer::new();
            base(&mut renderer, TRIANGLE_CAPACITY - remaining);
            let checkpoint = renderer.checkpoint();
            let mut shadows = ShadowFrame::new();
            shadows.push_caster(source);
            shadows.render_unsorted(&mut renderer, plane, 11);
            assert_eq!(renderer.dropped, checkpoint.dropped);
            assert_eq!(shadows.base_commands(), checkpoint.triangle_count);
            if remaining < required {
                assert_eq!(renderer.checkpoint(), checkpoint);
                assert_eq!(
                    (shadows.shadow_commands(), shadows.omitted_casters()),
                    (0, 1)
                );
            } else {
                assert_eq!(shadows.shadow_commands() as usize, required);
                assert_eq!(shadows.rendered_casters(), 1);
            }
        }
    }
}

#[test]
fn fixed_ink_overlap_is_idempotent_and_does_not_need_receivers() {
    let render = |copies, color| {
        let mut renderer = Renderer::new();
        renderer.begin(15);
        let mut frame = ShadowFrame::new();
        for _ in 0..copies {
            frame.push_caster(caster(1));
        }
        frame.render(&mut renderer, -105, color);
        assert_eq!(frame.omitted_casters(), 0);
        pixels(&renderer)
    };
    for color in [0, 4, 5, 6, 9, 11] {
        let single = render(1, color);
        assert!(single.iter().filter(|&&pixel| pixel == color).count() > 30);
        assert_eq!(single, render(2, color));
    }
}

#[test]
fn invalid_descriptors_and_full_caster_pool_are_bounded() {
    let mut frame = ShadowFrame::new();
    for bad in [
        Caster {
            half_x: 0,
            ..caster(1)
        },
        Caster {
            half_z: -1,
            ..caster(1)
        },
        Caster {
            half_x: 4097,
            ..caster(1)
        },
        caster(0),
        caster(5),
    ] {
        frame.push_caster(bad);
        assert!(!frame.insert_caster(0, bad));
    }
    assert_eq!(frame.caster_count(), 0);
    for _ in 0..CASTER_CAPACITY {
        frame.push_caster(caster(1));
    }
    assert!(!frame.insert_caster(0, caster(2)));
    assert_eq!(frame.caster_count() as usize, CASTER_CAPACITY);
    assert_eq!(frame.omitted_casters(), 1);
    frame.reset();
    assert_eq!(
        (
            frame.caster_count(),
            frame.shadow_commands(),
            frame.omitted_casters()
        ),
        (0, 0, 0)
    );
}
