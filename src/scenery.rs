//! World-anchored scenery, emitted before projection by the SCENERY overlay.
//! Decorative crossings deliberately have no gameplay objects or collision state.
use crate::assets::Segment;
use crate::game::STATIC_SKY;
use crate::{GeometrySink, Vec3, EXTERIOR_TERRAIN, TWO_SIDED};

pub(crate) const BACKDROP_CAMERA_Y: i16 = 40;
pub(crate) const SKY_COLORS: [u8; 4] = [6, 8, 11, 4];
const BRIDGES: [u16; 3] = [3750, 7900, 12050];

/// Frame setup selects this color while the SCENERY overlay is loaded.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub fn sky_color(section: u8, transition: Option<(u8, u16)>) -> u8 {
    if let Some((from, elapsed)) = transition {
        const COLORS: [[u8; 4]; 3] = [[6, 14, 10, 8], [8, 9, 12, 11], [11, 6, 14, 4]];
        let stage = if elapsed < 100 {
            0
        } else if elapsed < 200 {
            1
        } else if elapsed < 300 {
            2
        } else {
            3
        };
        COLORS[from.min(2) as usize][stage]
    } else {
        SKY_COLORS[(section & 3) as usize]
    }
}

/// The shared foreground budget always leaves enough room for the complete world.
#[inline(always)]
pub fn raw_face_limit(transition: Option<(u8, u16)>) -> u16 {
    if transition.is_some() {
        80
    } else {
        54
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    left: i16,
    right: i16,
    edge: i16,
    lane: i16,
    style: u8,
}

#[inline(never)]
// Standalone arithmetic uses resident table space to keep SCENERY bounded.
#[cfg_attr(target_arch = "mos", link_section = ".tables.scenery_weight")]
fn weight(elapsed: u16) -> i16 {
    if elapsed >= 399 {
        256
    } else {
        ((elapsed * 41) >> 6) as i16
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn blend(a: i16, b: i16, amount: i16) -> i16 {
    a + crate::math::mul_q8(b - a, amount)
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub(crate) fn terrain_palette(style: u8) -> u16 {
    match style {
        0 => 0x0d05,
        1 => 0x0c09,
        _ => 0x0c0b,
    }
}

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
static PROFILE: [u8; 16] = [0, 1, 3, 2, 1, 0, 2, 4, 3, 1, 0, 1, 2, 4, 2, 1];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub(crate) fn terrain_height(row: u16, right: bool, style: u8) -> i16 {
    let phase = row.wrapping_add(if right { 7 } else { 0 });
    let height = PROFILE[(phase & 15) as usize] as i16;
    if style == 1 {
        96 + (height << 4)
    } else if style == 0 {
        -105 + (height << 5)
    } else {
        -105 + (height << 4)
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn row_at(row: u16, legacy_style: Option<u8>) -> Row {
    let mut from = legacy_style.unwrap_or(3);
    let mut to = from;
    let mut amount = 0;
    if legacy_style.is_none() {
        let tick = row * 64;
        for (section, start) in BRIDGES.iter().enumerate() {
            if tick < *start {
                from = section as u8;
                to = from;
                break;
            }
            if tick < *start + 400 {
                from = section as u8;
                to = from + 1;
                amount = weight(tick - *start);
                break;
            }
        }
    }
    // Most frames contain only one terrain style. Preserve the exact sampler
    // without computing duplicate heights and three identity interpolations.
    if from == to {
        return Row {
            left: terrain_height(row, false, from),
            right: terrain_height(row, true, from),
            edge: if from == 1 { 190 } else { 640 },
            lane: if legacy_style.is_none() && from == 3 {
                320
            } else {
                145
            },
            style: from,
        };
    }
    Row {
        left: blend(
            terrain_height(row, false, from),
            terrain_height(row, false, to),
            amount,
        ),
        right: blend(
            terrain_height(row, true, from),
            terrain_height(row, true, to),
            amount,
        ),
        edge: blend(
            if from == 1 { 190 } else { 640 },
            if to == 1 { 190 } else { 640 },
            amount,
        ),
        lane: if legacy_style.is_some() {
            145
        } else {
            blend(
                if from == 3 { 320 } else { 145 },
                if to == 3 { 320 } else { 145 },
                amount,
            )
        },
        style: if amount < 128 { from } else { to },
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn triangle(sink: &mut impl GeometrySink, points: [Vec3; 3], color: u8, camera_y: i16) {
    let points = points.map(|mut point| {
        point.y = point.y.saturating_sub(camera_y);
        point
    });
    // This private helper packs the exterior tag above the palette nibble to
    // keep its native calling convention compact. Public emissions stay separate.
    sink.triangle(
        points[0], points[1], points[2], color & 15,
        TWO_SIDED | (color & EXTERIOR_TERRAIN),
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn strip(
    sink: &mut impl GeometrySink,
    x0: [i16; 2],
    y0: [i16; 2],
    x1: [i16; 2],
    y1: [i16; 2],
    z: [i16; 2],
    colors: [u8; 2],
    camera_y: i16,
    flags: u8,
) {
    let a = Vec3 {
        x: x0[0],
        y: y0[0],
        z: z[0],
    };
    let b = Vec3 {
        x: x1[0],
        y: y1[0],
        z: z[0],
    };
    let c = Vec3 {
        x: x1[1],
        y: y1[1],
        z: z[1],
    };
    let d = Vec3 {
        x: x0[1],
        y: y0[1],
        z: z[1],
    };
    triangle(sink, [a, b, c], colors[0] | flags, camera_y);
    triangle(sink, [a, c, d], colors[1] | flags, camera_y);
}

/// Emit camera-space triangles. Legacy mode preserves title and attract terrain.
/// Gameplay samples every terrain edge from its absolute world row, including
/// incoming land ahead of a bridge and the terrain left behind its far end.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub fn emit(
    sink: &mut impl GeometrySink,
    segment: &Segment<'_>,
    scene_ticks: u16,
    section: u8,
    transition: Option<(u8, u16)>,
    camera_y: i16,
    legacy: bool,
) {
    let row = scene_ticks >> 6;
    let offset = ((scene_ticks & 63) * 8) as i16;
    let legacy_style = if legacy {
        Some(segment.terrain())
    } else {
        None
    };
    let mut first = row_at(row, legacy_style);
    for band in 0..4i16 {
        let world_row = row + band as u16;
        let last = row_at(world_row + 1, legacy_style);
        let color = if legacy {
            segment.ground_color()
        } else {
            [5, 9, 11, 6][first.style as usize]
        };
        let shade = if world_row & 1 == 0 {
            color
        } else if color == 5 {
            13
        } else if color == 6 && !legacy {
            14
        } else {
            11
        };
        let palette = terrain_palette(first.style);
        let facets = [palette as u8, (palette >> 8) as u8];
        let z = [40 + band * 512 - offset, 552 + band * 512 - offset];
        strip(
            sink,
            [-first.lane, -last.lane],
            [-105; 2],
            [first.lane, last.lane],
            [-105; 2],
            z,
            [shade; 2],
            camera_y,
            EXTERIOR_TERRAIN,
        );
        for side in [-1i16, 1] {
            let heights = if side < 0 {
                [first.left, last.left]
            } else {
                [first.right, last.right]
            };
            let edges = [side * first.edge, side * last.edge];
            strip(
                sink,
                [side * first.lane, side * last.lane],
                [-105; 2],
                edges,
                heights,
                z,
                facets,
                camera_y,
                EXTERIOR_TERRAIN,
            );
            if first.edge < 640 || last.edge < 640 {
                strip(
                    sink,
                    edges,
                    heights,
                    [side * 640; 2],
                    heights,
                    z,
                    facets,
                    camera_y,
                    EXTERIOR_TERRAIN,
                );
            } else if first.style == 2 && last.style == 2 {
                // Runway markings have world-anchored ends outside the canyon
                // caps, so both details never consume the same face reservation.
                strip(
                    sink,
                    [side * 95; 2],
                    [-103; 2],
                    [side * 100; 2],
                    [-103; 2],
                    z,
                    [15; 2],
                    camera_y,
                    EXTERIOR_TERRAIN,
                );
            }
        }
        first = last;
    }
    for face in 0..14 {
        let (mut points, mut color) = backdrop_face(section, face);
        if let Some((from, elapsed)) = transition {
            let (destination, next_color) = backdrop_face(from + 1, face);
            let amount = weight(elapsed);
            for corner in 0..3 {
                points[corner].x = blend(points[corner].x, destination[corner].x, amount);
                points[corner].y = blend(points[corner].y, destination[corner].y, amount);
                points[corner].z = blend(points[corner].z, destination[corner].z, amount);
            }
            if elapsed >= 200 {
                color = next_color;
            }
        }
        sink.triangle(
            points[0],
            points[1],
            points[2],
            color,
            TWO_SIDED | if transition.is_none() { STATIC_SKY } else { 0 },
        );
    }
    if let Some((from, elapsed)) = transition {
        wildlife(sink, scene_ticks, from, elapsed, camera_y);
    }
}

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static HORIZON_COLORS: [[u8; 2]; 4] = [[14, 12], [9, 2], [6, 12], [6, 11]];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static CLOUD_COLORS: [[u8; 2]; 4] = [[1, 15], [7, 10], [15, 12], [10, 14]];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
#[rustfmt::skip]
pub(crate) static HORIZONS: [[[i8; 2]; 15]; 4] = [
    [[-88, -34], [-33, -34], [-60, 17], [-45, -34], [3, -34], [-27, 9], [-8, -34], [40, -34], [12, 14], [26, -34], [73, -34], [51, 7], [61, -34], [88, -34], [74, 12]],
    [[-88, -34], [-7, -34], [-30, 21], [-88, -34], [-30, 21], [-73, 21], [6, -34], [88, -34], [75, 18], [6, -34], [75, 18], [25, 18], [-18, -34], [18, -34], [1, 4]],
    [[-88, -34], [-12, -34], [-12, 16], [-88, -34], [-12, 16], [-88, 16], [10, -34], [88, -34], [88, 12], [10, -34], [88, 12], [10, 12], [-15, -34], [15, -34], [-9, 24]],
    [[-88, -34], [-4, -34], [-40, 3], [-88, -34], [-40, 3], [-88, 3], [4, -34], [88, -34], [88, 0], [4, -34], [88, 0], [50, 0], [-14, -34], [14, -34], [3, 5]],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static CLOUD_CENTERS: [[[i8; 3]; 3]; 4] = [
    [[-67, 78, 96], [4, 110, 112], [89, 69, 127]],
    [[-72, 61, 96], [6, 96, 112], [98, 65, 127]],
    [[-72, 82, 96], [4, 67, 112], [103, 124, 127]],
    [[-67, 58, 96], [14, 101, 112], [102, 57, 127]],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static CLOUD_OUTLINES: [[[i8; 2]; 5]; 4] = [
    [[-27, 0], [-13, 11], [4, 15], [27, 2], [20, -6]],
    [[-37, 0], [-18, 5], [10, 9], [37, 2], [29, -6]],
    [[-46, 0], [-28, 15], [7, 16], [46, 1], [38, -7]],
    [[-33, 0], [-19, 5], [7, 7], [33, 1], [25, -5]],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub(crate) fn backdrop_face(section: u8, face: u8) -> ([Vec3; 3], u8) {
    let theme = (section & 3) as usize;
    let (outline, center, first, second, depth, color, camera_y): (
        &[[i8; 2]],
        [i8; 3],
        u8,
        u8,
        i16,
        u8,
        i16,
    );
    if face < 5 {
        outline = &HORIZONS[theme];
        center = [0; 3];
        first = face * 3;
        second = first + 1;
        depth = 2350;
        color = HORIZON_COLORS[theme][(face & 1) as usize];
        camera_y = BACKDROP_CAMERA_Y;
    } else {
        let cloud = if face >= 11 {
            2
        } else if face >= 8 {
            1
        } else {
            0
        };
        let facet = face - 5 - cloud * 3;
        outline = &CLOUD_OUTLINES[theme];
        center = CLOUD_CENTERS[theme][cloud as usize];
        first = 0;
        second = facet + 1;
        depth = center[2] as i16 * 32;
        color = CLOUD_COLORS[theme][(facet == 2) as usize];
        camera_y = 0;
    }
    let points = core::array::from_fn(|corner| {
        let vertex = if corner == 0 {
            first as usize
        } else {
            second as usize + corner - 1
        };
        let xy = outline[vertex];
        Vec3 {
            x: (center[0] as i16 + xy[0] as i16) * 32,
            y: (center[1] as i16 + xy[1] as i16) * 16 - camera_y,
            z: depth,
        }
    });
    (points, color)
}

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife.data")]
static CREATURE_POINTS: [[i8; 3]; 25] = [
    [-48, 32, -12],
    [48, 32, -12],
    [53, 62, 0],
    [-45, 68, 0],
    [38, 72, 17],
    [-33, 78, 17],
    [40, 55, 0],
    [60, 55, 0],
    [78, 100, 0],
    [62, 100, 0],
    [65, 96, -5],
    [98, 96, -5],
    [98, 110, -5],
    [70, 110, -5],
    [-90, 68, 5],
    [-40, 60, 12],
    [-30, 73, 5],
    [-15, 88, 5],
    [-5, 70, 5],
    [5, 70, 5],
    [18, 83, 5],
    [28, 68, 5],
    [86, 104, -8],
    [89, 104, -8],
    [87, 106, -8],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife.data")]
static CREATURE_FACES: [[u8; 4]; 12] = [
    [0, 1, 2, 0],
    [0, 2, 3, 0],
    [3, 2, 4, 1],
    [3, 4, 5, 1],
    [6, 7, 8, 0],
    [6, 8, 9, 1],
    [10, 11, 12, 0],
    [10, 12, 13, 1],
    [3, 14, 15, 0],
    [16, 17, 18, 1],
    [19, 20, 21, 1],
    [22, 23, 24, 2],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife")]
fn wildlife(
    sink: &mut impl GeometrySink,
    scene_ticks: u16,
    section: u8,
    elapsed: u16,
    camera_y: i16,
) {
    // Keep the audited entry point in the transition-only overlay tail. Every
    // decoration is generated from the scene clock, with no simulation state.
    if section == 1 {
        transport(sink, elapsed, camera_y);
        return;
    }
    if section == 2 {
        meteors(sink, elapsed);
        return;
    }
    // This crossing is fixed in the world: the camera closes eight units/tick.
    // It enters and leaves beyond the viewport, so no visible actor pops at a cut.
    let z = 3000 - elapsed as i16 * 8;
    let x = -3200 + elapsed as i16 * 16;
    if z > 180 {
        let row = row_at((scene_ticks + (z as u16 / 8)) >> 6, None);
        let height = if x < 0 { row.left } else { row.right };
        let lateral = x.abs();
        let floor = if lateral <= row.lane {
            -105
        } else if lateral >= row.edge {
            height
        } else {
            -105 + ((height + 105) as i32 * (lateral - row.lane) as i32
                / (row.edge - row.lane) as i32) as i16
        };
        let palette = [7, 1, 0];
        for leg in 0..4u8 {
            let hip = if leg & 1 == 0 { -65 } else { 65 };
            let depth = if leg < 2 { 20 } else { -20 };
            let stride = crate::math::sin8(
                (elapsed as u8)
                    .wrapping_mul(5)
                    .wrapping_add(if (leg ^ (leg >> 1)) & 1 == 0 { 0 } else { 128 }),
            ) / 6;
            strip(
                sink,
                [x + hip - 8, x + hip + stride - 7],
                [floor + 70, floor],
                [x + hip + 8, x + hip + stride + 7],
                [floor + 70, floor],
                [z + depth, z + depth + 1],
                [palette[0]; 2],
                camera_y,
                0,
            );
        }
        for face in CREATURE_FACES {
            let points = core::array::from_fn(|corner| {
                let p = CREATURE_POINTS[face[corner] as usize];
                Vec3 {
                    x: x + p[0] as i16 * 2,
                    y: floor + p[1] as i16 * 2,
                    z: z + p[2] as i16 * 2,
                }
            });
            triangle(sink, points, palette[face[3] as usize], camera_y);
        }
    }
    for bird in 0..3i16 {
        let x = -4700 + elapsed as i16 * 24 + bird * 180;
        let y = 1050 + bird * 110;
        let z = 2800 + bird * 160;
        let flap = crate::math::sin8(
            (elapsed as u8)
                .wrapping_mul(5)
                .wrapping_add(bird as u8 * 53),
        ) / 4;
        let body = Vec3 { x, y, z: z - 28 };
        let head = Vec3 {
            x: x + 12,
            y: y + 15,
            z: z - 38,
        };
        triangle(
            sink,
            [
                body,
                head,
                Vec3 {
                    x: x - 95,
                    y: y + flap,
                    z,
                },
            ],
            0,
            camera_y,
        );
        triangle(
            sink,
            [
                head,
                body,
                Vec3 {
                    x: x + 95,
                    y: y + flap,
                    z,
                },
            ],
            0,
            camera_y,
        );
    }
}

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife.data")]
static TRANSPORT_POINTS: [[i8; 2]; 20] = [
    [-100, 0],
    [-72, 28],
    [58, 28],
    [92, 8],
    [92, -8],
    [66, -20],
    [-72, -20],
    [-32, 0],
    [64, 7],
    [-55, 28],
    [-40, 50],
    [36, 50],
    [48, 28],
    [-72, 18],
    [-58, 18],
    [-64, 8],
    [-26, 12],
    [44, 12],
    [44, 0],
    [-26, 0],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife.data")]
static TRANSPORT_FACES: [[u8; 4]; 14] = [
    [0, 1, 7, 12],
    [1, 2, 7, 15],
    [2, 8, 7, 12],
    [2, 3, 8, 15],
    [3, 4, 8, 11],
    [8, 4, 5, 11],
    [7, 8, 5, 9],
    [0, 7, 6, 11],
    [7, 5, 6, 11],
    [9, 10, 11, 12],
    [9, 11, 12, 15],
    [13, 14, 15, 14],
    [16, 17, 18, 7],
    [16, 18, 19, 7],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife")]
fn transport(sink: &mut impl GeometrySink, elapsed: u16, camera_y: i16) {
    // At both boundaries the entire craft (including its broad hull) is beyond
    // the viewport. The camera passes beneath it during the central four seconds.
    if !(50..350).contains(&elapsed) {
        return;
    }
    let x = 4800 - elapsed as i16 * 24;
    let y = 700 + crate::math::sin8((elapsed as u8).wrapping_mul(2)) / 8;
    for face in TRANSPORT_FACES {
        let points = core::array::from_fn(|corner| {
            let p = TRANSPORT_POINTS[face[corner] as usize];
            Vec3 {
                x: x + p[0] as i16 * 8,
                y: y + p[1] as i16 * 8,
                z: 2000,
            }
        });
        triangle(sink, points, face[3], camera_y);
    }
    let pulse = 144 + crate::math::sin8((elapsed as u8).wrapping_mul(7)) / 4;
    for pod in [-480, 440] {
        let x = x + pod;
        let y = y - 210;
        // Three faceted engine faces and two tapered exhaust triangles per pod.
        strip(
            sink,
            [x - 120, x - 85],
            [y + 90, y - 35],
            [x + 120, x + 85],
            [y + 90, y - 35],
            [1980, 1980],
            [12, 11],
            camera_y,
            0,
        );
        triangle(
            sink,
            [
                Vec3 {
                    x: x - 85,
                    y: y - 35,
                    z: 1978,
                },
                Vec3 {
                    x: x + 85,
                    y: y - 35,
                    z: 1978,
                },
                Vec3 {
                    x,
                    y: y - 65,
                    z: 1978,
                },
            ],
            7,
            camera_y,
        );
        triangle(
            sink,
            [
                Vec3 {
                    x: x - 65,
                    y: y - 45,
                    z: 1982,
                },
                Vec3 {
                    x: x + 65,
                    y: y - 45,
                    z: 1982,
                },
                Vec3 {
                    x: x + 55,
                    y: y - pulse,
                    z: 1982,
                },
            ],
            10,
            camera_y,
        );
        triangle(
            sink,
            [
                Vec3 {
                    x: x - 30,
                    y: y - 45,
                    z: 1976,
                },
                Vec3 {
                    x: x + 30,
                    y: y - 45,
                    z: 1976,
                },
                Vec3 {
                    x: x + 25,
                    y: y - pulse / 2,
                    z: 1976,
                },
            ],
            1,
            camera_y,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife")]
fn meteors(sink: &mut impl GeometrySink, elapsed: u16) {
    for (index, start) in crate::transition::METEOR_STARTS.iter().enumerate() {
        let age = elapsed.wrapping_sub(*start);
        if age == 0 || age >= crate::transition::METEOR_DURATION {
            continue;
        }
        // Alternating left/right lanes also identify the two sound channels.
        let origin = if index & 1 == 0 {
            -600 + (index as i16 / 2) * 200
        } else {
            2200 - (index as i16 / 2) * 240
        };
        let x = origin - age as i16 * 12;
        let y = 1550 + (index as i16 % 3) * 90 - age as i16 * 12;
        let remaining = crate::transition::METEOR_DURATION - age;
        let width = age.min(remaining).min(8) as i16 * 5;
        let tail = if age < 24 {
            age as i16 * 28
        } else if remaining < 16 {
            remaining as i16 * 42
        } else {
            672
        };
        // These are camera-independent sky coordinates. A depth beyond the
        // horizon lets its silhouettes occlude streaks; no STATIC_SKY flags are
        // used because each head and its tapered trail move every frame.
        let a = Vec3 {
            x: x - width,
            y: y + width,
            z: 2600,
        };
        let b = Vec3 {
            x: x + width,
            y: y - width,
            z: 2600,
        };
        let tip = Vec3 {
            x: x + tail,
            y: y + tail,
            z: 2600,
        };
        sink.triangle(a, b, tip, 10, TWO_SIDED);
        sink.triangle(a, Vec3 { x, y, z: 2600 }, tip, 7, TWO_SIDED);
        sink.triangle(
            a,
            b,
            Vec3 {
                x: x - width,
                y: y - width,
                z: 2600,
            },
            if remaining < 8 { 7 } else { 1 },
            TWO_SIDED,
        );
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[derive(Default, Debug, PartialEq, Eq)]
    struct Capture(Vec<([Vec3; 3], u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }

    fn capture(ticks: u16, section: u8, transition: Option<(u8, u16)>, camera: i16) -> Capture {
        let mut bytes = [0; crate::assets::SEGMENT_CACHE_SIZE];
        bytes[..4].copy_from_slice(b"SMS1");
        bytes[4] = section;
        bytes[5] = section;
        bytes[6] = [5, 9, 11, 6][section as usize];
        let segment = Segment::new(&bytes).unwrap();
        let mut result = Capture::default();
        emit(
            &mut result,
            &segment,
            ticks,
            section,
            transition,
            camera,
            false,
        );
        result
    }

    #[test]
    fn all_bridge_ticks_fit_scenery_and_projected_command_reservations() {
        for (section, start) in BRIDGES.into_iter().enumerate() {
            for elapsed in 0..400 {
                let transition = Some((section as u8, elapsed));
                for camera in [24, 40, 56] {
                    let scene = capture(start + elapsed, section as u8, transition, camera);
                    assert!(
                        scene.0.len() <= 80,
                        "bridge {section} tick {elapsed}: {}",
                        scene.0.len()
                    );
                    assert!(scene.0.iter().all(|face| face.2 & STATIC_SKY == 0));
                    let mut renderer = crate::render::Renderer::new();
                    renderer.begin(sky_color(section as u8, transition));
                    for &(points, color, flags) in &scene.0 {
                        assert!(color < 16);
                        renderer.triangle(points[0], points[1], points[2], color, flags);
                    }
                    assert_eq!(renderer.dropped, 0);
                    // Retain room for the existing 174 foreground reservation.
                    // Bridge controls use far fewer than its full combat pool.
                    assert!(renderer.triangle_count <= 96);
                }
            }
        }
    }

    #[test]
    fn spatial_bridge_samples_have_identical_shared_edges_and_move_only_with_world() {
        for start in BRIDGES {
            for tick in start - 256..start + 657 {
                let row = tick >> 6;
                let amount = tick & 63;
                let first = row_at(row, None);
                let shared = row_at(row + 1, None);
                // Replacing the front row keeps the old far edge exactly.
                if amount == 63 {
                    assert_eq!(shared, row_at((tick + 1) >> 6, None));
                }
                for sample in [first, shared] {
                    assert!((190..=640).contains(&sample.edge));
                    assert!((-105..=160).contains(&sample.left));
                    assert!((-105..=160).contains(&sample.right));
                }
            }
        }
        // All vertices shared by a shoulder and its outer cap coincide, even
        // while the two ends of a band have different shoulder widths/heights.
        for (section, start) in BRIDGES.into_iter().enumerate() {
            for elapsed in [0, 1, 63, 100, 199, 255, 300, 399] {
                let scene = capture(
                    start + elapsed,
                    section as u8,
                    Some((section as u8, elapsed)),
                    40,
                );
                let terrain_count = capture(start + elapsed, section as u8, None, 40).0.len() - 14;
                let terrain = &scene.0[..terrain_count];
                for pair in terrain.chunks_exact(2) {
                    assert_eq!(pair[0].0[0], pair[1].0[0]);
                    assert_eq!(pair[0].0[2], pair[1].0[1]);
                }
            }
        }
    }

    #[test]
    fn transition_endpoints_match_backdrops_and_sky_of_adjacent_levels() {
        for (section, start) in BRIDGES.into_iter().enumerate() {
            for elapsed in [0, 399] {
                let expected_section = section as u8 + (elapsed != 0) as u8;
                let scene = capture(
                    start + elapsed,
                    section as u8,
                    Some((section as u8, elapsed)),
                    40,
                );
                let terrain_count = capture(start + elapsed, section as u8, None, 40).0.len() - 14;
                for (face, actual) in scene.0[terrain_count..terrain_count + 14]
                    .iter()
                    .enumerate()
                {
                    let (points, color) = backdrop_face(expected_section, face as u8);
                    assert_eq!((actual.0, actual.1), (points, color));
                }
                assert_eq!(
                    sky_color(section as u8, Some((section as u8, elapsed))),
                    SKY_COLORS[expected_section as usize]
                );
            }
            // Progress is in the terrain sampler's absolute clock, so a mode
            // handoff cannot change the terrain beneath an identical camera.
            let before = capture(start, section as u8, None, 40);
            let entry = capture(start, section as u8, Some((section as u8, 0)), 40);
            let terrain_count = before.0.len() - 14;
            assert_eq!(&before.0[..terrain_count], &entry.0[..terrain_count]);
            let end = capture(start + 400, section as u8 + 1, None, 40);
            assert!(end.0.len() <= 54);
        }
    }

    #[test]
    fn wildlife_is_deterministic_animated_and_never_marked_static() {
        let mut a = Capture::default();
        let mut b = Capture::default();
        let mut c = Capture::default();
        wildlife(&mut a, 3950, 0, 200, 40);
        wildlife(&mut b, 3950, 0, 200, 40);
        wildlife(&mut c, 3958, 0, 208, 40);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.0.len(), 26);
        assert!(a.0.iter().all(|face| face.2 == TWO_SIDED));
        assert_ne!(a.0[0].0, c.0[0].0, "feet must stride");
        assert_ne!(a.0[20].0, c.0[20].0, "wings must flap");
    }

    fn decoration(section: u8, elapsed: u16, camera: i16) -> Capture {
        let mut result = Capture::default();
        wildlife(
            &mut result,
            BRIDGES[section as usize] + elapsed,
            section,
            elapsed,
            camera,
        );
        result
    }

    #[test]
    fn bridges_have_distinct_deterministic_decorations_with_reserved_capacity() {
        let wildlife = decoration(0, 200, 40);
        let transport = decoration(1, 200, 40);
        let meteors = decoration(2, 200, 40);
        assert_eq!(wildlife.0.len(), 26);
        assert_eq!(transport.0.len(), 24);
        assert_eq!(meteors.0.len(), 6);
        assert_ne!(wildlife, transport);
        assert_ne!(transport, meteors);
        for section in 0..3 {
            for elapsed in 0..400 {
                let first = decoration(section, elapsed, 40);
                assert_eq!(first, decoration(section, elapsed, 40));
                assert!(first.0.len() <= 26);
                assert!(first.0.iter().all(|face| face.2 == TWO_SIDED));
            }
        }
    }

    #[test]
    fn transport_crosses_right_to_left_bobs_and_pulses_then_leaves_offscreen() {
        let start = decoration(1, 50, 40);
        let end = decoration(1, 349, 40);
        assert!(start
            .0
            .iter()
            .flat_map(|face| face.0)
            .all(|p| crate::projection::project(p).0 >= 160));
        assert!(end
            .0
            .iter()
            .flat_map(|face| face.0)
            .all(|p| crate::projection::project(p).0 < 0));
        assert!(decoration(1, 49, 40).0.is_empty());
        assert!(decoration(1, 350, 40).0.is_empty());
        let a = decoration(1, 200, 40);
        let b = decoration(1, 208, 40);
        assert!(a.0[0].0[0].x > b.0[0].0[0].x);
        assert_ne!(a.0[0].0[0].y, b.0[0].0[0].y, "hull must hover");
        assert_ne!(
            a.0[17].0[2].y - a.0[17].0[0].y,
            b.0[17].0[2].y - b.0[17].0[0].y,
            "exhaust must pulse independently"
        );
    }

    #[test]
    fn meteor_shower_is_sky_fixed_above_cockpit_and_has_smooth_boundaries() {
        assert!(decoration(2, 100, 40).0.is_empty());
        assert!(decoration(2, 380, 40).0.is_empty());
        let mut peak = 0;
        for elapsed in 0..400 {
            let scene = decoration(2, elapsed, 24);
            assert_eq!(scene, decoration(2, elapsed, 56));
            peak = peak.max(scene.0.len());
            for (points, _, _) in scene.0 {
                for point in points {
                    assert_eq!(point.z, 2600, "streaks remain behind the horizon");
                    assert!(
                        crate::projection::project(point).1 < 75,
                        "keep the sky show above gameplay"
                    );
                }
            }
        }
        assert_eq!(peak, 12, "four overlapping meteors build the shower");
        let first = decoration(2, 101, 40);
        let later = decoration(2, 120, 40);
        assert!(first.0[0].0[0].x > later.0[0].0[0].x);
        assert!(first.0[0].0[0].y > later.0[0].0[0].y);
        assert!(
            first.0[0].0[2].x - first.0[0].0[0].x < later.0[0].0[2].x - later.0[0].0[0].x,
            "trail grows into view"
        );
        // The first emitted meteor is the oldest, now on its last visible tick.
        let mut ending = Capture::default();
        meteors(&mut ending, 179);
        let first_tail = ending.0[0].0;
        assert!(first_tail[2].x - first_tail[0].x < 60);
    }

    #[test]
    fn meteor_sound_onsets_match_the_visible_head_hemisphere() {
        for (index, start) in crate::transition::METEOR_STARTS.into_iter().enumerate() {
            // The sound cue begins eight ticks after each head appears. Find
            // this meteor among all older, still-visible three-face streaks.
            let elapsed = start + 8;
            let prior = crate::transition::METEOR_STARTS[..index]
                .iter()
                .filter(|&&earlier| elapsed - earlier < crate::transition::METEOR_DURATION)
                .count();
            let scene = decoration(2, elapsed, 40);
            let head = scene.0[prior * 3 + 2].0;
            for point in head {
                let screen_x = crate::projection::project(point).0;
                assert!(
                    (0..160).contains(&screen_x),
                    "meteor {index} must be visible at sound onset"
                );
                assert_eq!(
                    screen_x < 80,
                    index & 1 == 0,
                    "meteor {index} head must match its left/right sound channel"
                );
            }
        }
    }

    #[test]
    fn coast_opens_continuously_into_a_wide_water_lane_without_changing_legacy_previews() {
        let start = BRIDGES[2];
        let mut previous = 145;
        for row in (start >> 6)..=((start + 464) >> 6) {
            let sample = row_at(row, None);
            assert!(sample.lane >= previous);
            assert!((145..=320).contains(&sample.lane));
            previous = sample.lane;
        }
        assert_eq!(previous, 320);
        for row in 195..254 {
            assert_eq!(row_at(row, None).lane, 320);
            assert_eq!(row_at(row, Some(3)).lane, 145);
        }
    }

    #[test]
    fn bridge_terrain_retains_exact_world_faces_through_scroll_and_row_replacement() {
        for start in BRIDGES {
            for tick in start - 128..start + 529 {
                let before = capture(tick, 0, None, 40);
                let after = capture(tick + 1, 0, None, 40);
                let after_terrain = &after.0[..after.0.len() - 14];
                for &(mut points, color, flags) in &before.0[..before.0.len() - 14] {
                    for point in &mut points {
                        point.z -= 8;
                    }
                    // Only a complete old near row is retired at a 64-tick
                    // boundary. All surviving world faces must remain exact.
                    if points.iter().map(|point| point.z).max().unwrap() > 40 {
                        assert!(
                            after_terrain.contains(&(points, color, flags)),
                            "world face changed at tick {tick}: {points:?}"
                        );
                    }
                }
            }
        }
    }
}
