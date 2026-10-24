//! Small, independently loaded orbital environments. Only visible hazards are
//! actors; these distant structures stay outside the player's flight corridor.
use super::{Decoration, SceneSink};
#[cfg(target_arch = "mos")]
use crate::GeometrySink;
use crate::Vec3;

// Small immutable orbital tables fit in the unused resident code window and
// avoid transferring the same surface data from the REU every scenery frame.
#[cfg_attr(target_arch = "mos", link_section = ".text.orbital_data")]
static STARS: [[i16; 2]; 10] = [
    [-2730, 1680],
    [-2160, -420],
    [-1560, 1960],
    [-790, 1130],
    [-240, -1310],
    [440, 1820],
    [1190, 630],
    [1740, -1160],
    [2320, 1510],
    [2860, 190],
];
#[cfg_attr(target_arch = "mos", link_section = ".text.orbital_data")]
static RING: [[i16; 2]; 8] = [
    [0, 256],
    [181, 181],
    [256, 0],
    [181, -181],
    [0, -256],
    [-181, -181],
    [-256, 0],
    [-181, 181],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn stars(sink: &mut SceneSink<'_>) {
    for [x, y] in STARS {
        sink.triangle(
            Vec3 { x, y, z: 3000 },
            Vec3 {
                x,
                y: y + 45,
                z: 3000,
            },
            Vec3 {
                x: x + 45,
                y,
                z: 3000,
            },
            1,
            crate::game::STATIC_SKY,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn quad(sink: &mut SceneSink<'_>, p: [Vec3; 4], color: u8) {
    sink.triangle(p[0], p[1], p[2], color, 0);
    sink.triangle(p[0], p[2], p[3], color, 0);
}

/// A physical thin panel has separate front and back surfaces. Both use the
/// ordinary one-sided renderer; there is no material or renderer exemption.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.shipyard")]
fn panel(sink: &mut SceneSink<'_>, p: [Vec3; 4], color: u8) {
    let back = [p[0], p[3], p[2], p[1]];
    quad(sink, p, color);
    quad(sink, back, color);
}

/// Author the exposed upper surface of a deck or mirrored floor strip.
#[inline(always)]
fn deck(sink: &mut SceneSink<'_>, p: [Vec3; 4], color: u8, flags: u8) {
    let points = if p[0].x < p[1].x {
        [p[0], p[3], p[2], p[1]]
    } else {
        p
    };
    sink.triangle(points[0], points[1], points[2], color, flags);
    sink.triangle(points[0], points[2], points[3], color, flags);
}

/// During a crossing both environments travel completely below the viewport.
/// The common starfield remains, so changing the loaded tail at tick 200 is
/// visually exact even when foreground rendering skips the midpoint itself.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn drop(context: &Decoration) -> i16 {
    if context.elapsed >= 399 {
        0
    } else {
        context.elapsed.min(400 - context.elapsed) as i16 * 32
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery_extra")]
fn clock(context: &Decoration) -> u16 {
    if context.elapsed >= 200 && context.elapsed < 400 {
        context.elapsed.wrapping_sub(400)
    } else {
        context.scene_ticks
    }
}

// Bake the fixed-radius bodies at compile time. Surface markings cost only
// translations on the 6510, and each body stays within 24 authored triangles.
const fn body_points<const N: usize>(mut points: [[i16; 2]; N], radius: i16) -> [[i16; 2]; N] {
    let mut i = 0;
    while i < N {
        points[i][0] = (points[i][0] as i32 * radius as i32 / 128) as i16;
        points[i][1] = (points[i][1] as i32 * radius as i32 / 128) as i16;
        i += 1;
    }
    points
}

const PLANET_POINTS: [[i16; 2]; 21] = body_points(
    [
        [0, 128],
        [64, 111],
        [111, 64],
        [128, 0],
        [111, -64],
        [64, -111],
        [0, -128],
        [-64, -111],
        [-111, -64],
        [-128, 0],
        [-111, 64],
        [-64, 111],
        [-38, 0],
        // Broad turquoise land/cloud banks, following the lit hemisphere's curve.
        [0, 84],
        [67, 88],
        [88, 60],
        [14, 48],
        [-12, -65],
        [32, -48],
        [55, -88],
        [12, -105],
    ],
    760,
);

const PLANET_FACES: [[u8; 4]; 16] = [
    [12, 0, 1, 14],
    [12, 1, 2, 14],
    [12, 2, 3, 14],
    [12, 3, 4, 14],
    [12, 4, 5, 14],
    [12, 5, 6, 14],
    [12, 6, 7, 6],
    [12, 7, 8, 6],
    [12, 8, 9, 6],
    [12, 9, 10, 6],
    [12, 10, 11, 6],
    [12, 11, 0, 6],
    [13, 14, 15, 3],
    [13, 15, 16, 3],
    [17, 18, 19, 3],
    [17, 19, 20, 3],
];

const MOON_POINTS: [[i16; 2]; 25] = body_points(
    [
        [0, 128],
        [64, 111],
        [111, 64],
        [128, 0],
        [111, -64],
        [64, -111],
        [0, -128],
        [-64, -111],
        [-111, -64],
        [-128, 0],
        [-111, 64],
        [-64, 111],
        [-38, 0],
        // A broad inset crater with a bright upper rim and a charcoal basin.
        [40, 80],
        [78, 78],
        [94, 61],
        [80, 38],
        [44, 36],
        [26, 55],
        // A smaller foreshortened crater on the shadowed half.
        [-91, -16],
        [-64, -9],
        [-47, -27],
        [-76, -35],
        // An offset fracture bends between the body's two poles.
        [-24, 35],
        [0, 18],
    ],
    560,
);

const MOON_FACES: [[u8; 4]; 20] = [
    [12, 0, 1, 15],
    [12, 1, 2, 15],
    [12, 2, 3, 12],
    [12, 3, 4, 12],
    [12, 4, 5, 12],
    [12, 5, 6, 12],
    [12, 6, 7, 11],
    [12, 7, 8, 11],
    [12, 8, 9, 11],
    [12, 9, 10, 11],
    [12, 10, 11, 12],
    [12, 11, 0, 12],
    [13, 14, 18, 12],
    [14, 15, 18, 11],
    [18, 15, 17, 11],
    [15, 16, 17, 15],
    [19, 20, 22, 12],
    [20, 21, 22, 15],
    [0, 24, 23, 0],
    [23, 24, 6, 0],
];

#[derive(Clone, Copy)]
struct BodyFace {
    xy: [[i16; 2]; 3],
    z: i16,
    color: u8,
}

const fn body_faces<const N: usize>(points: &[[i16; 2]], faces: &[[u8; 4]; N]) -> [BodyFace; N] {
    let mut result = [BodyFace {
        xy: [[0; 2]; 3],
        z: 0,
        color: 0,
    }; N];
    let mut i = 0;
    while i < N {
        let face = faces[i];
        result[i] = BodyFace {
            xy: [
                points[face[0] as usize],
                points[face[1] as usize],
                points[face[2] as usize],
            ],
            // Markings cover the disc; the open fracture covers both layers.
            z: if i < 12 {
                2800
            } else if face[3] == 0 {
                2768
            } else {
                2784
            },
            color: face[3],
        };
        i += 1;
    }
    result
}

#[cfg_attr(target_arch = "mos", link_section = ".text.orbital_data")]
static PLANET: [BodyFace; 16] = body_faces(&PLANET_POINTS, &PLANET_FACES);
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.moon.data")]
static MOON: [BodyFace; 20] = body_faces(&MOON_POINTS, &MOON_FACES);

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn body(sink: &mut SceneSink<'_>, x: i16, y: i16, faces: &[BodyFace]) {
    for face in faces {
        let p = |corner: usize| {
            let xy = face.xy[corner];
            Vec3 {
                x: x + xy[0],
                y: y + xy[1],
                z: face.z,
            }
        };
        sink.triangle(p(0), p(1), p(2), face.color, 0);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn planet(sink: &mut SceneSink<'_>, x: i16, y: i16) {
    body(sink, x, y, &PLANET);
}

const fn rock_points(radius: i16) -> [Vec3; 8] {
    [
        Vec3 {
            x: -radius / 4,
            y: radius,
            z: 0,
        },
        Vec3 {
            x: radius / 4,
            y: -radius,
            z: 0,
        },
        Vec3 {
            x: -radius,
            y: radius / 8,
            z: 0,
        },
        Vec3 {
            x: -radius / 3,
            y: -radius / 4,
            z: radius,
        },
        Vec3 {
            x: radius * 3 / 4,
            y: 0,
            z: radius / 2,
        },
        Vec3 {
            x: radius,
            y: -radius / 8,
            z: 0,
        },
        Vec3 {
            x: radius / 3,
            y: radius / 4,
            z: -radius,
        },
        Vec3 {
            x: -radius * 3 / 4,
            y: 0,
            z: -radius / 2,
        },
    ]
}

#[cfg_attr(target_arch = "mos", link_section = ".text.orbital_data")]
static ROCKS: [[Vec3; 8]; 3] = [rock_points(95), rock_points(130), rock_points(170)];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn rock(sink: &mut SceneSink<'_>, center: Vec3, kind: usize, color: u8) {
    if sink.shadow_enabled() {
        let radius = [95, 130, 170][kind];
        sink.shadow_caster(crate::shadow::Caster {
            center,
            half_x: radius,
            half_z: radius,
            shape: 3,
        });
    }
    let points: [Vec3; 8] = core::array::from_fn(|i| {
        let p = ROCKS[kind][i];
        Vec3 {
            x: center.x + p.x,
            y: center.y + p.y,
            z: center.z + p.z,
        }
    });
    // Extra edge facets are useful only once a rock spans several pixels.
    // The distant shell keeps the same extremal vertices and shading.
    let ring: &[usize] = if center.z >= 600 {
        &[2, 3, 5, 6]
    } else {
        &[2, 3, 4, 5, 6, 7]
    };
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[if i + 1 == ring.len() { 0 } else { i + 1 }];
        sink.triangle(
            points[0],
            points[a],
            points[b],
            if a == 6 { 15 } else { color },
            0,
        );
        sink.triangle(
            points[1],
            points[b],
            points[a],
            if a == 7 { 12 } else { 11 },
            0,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn asteroid_debris(sink: &mut SceneSink<'_>, ticks: u16, drop: i16) {
    for i in 0..2u16 {
        let z = 2000 - ((ticks.wrapping_add(i * 128) & 255) * 8) as i16;
        rock(
            sink,
            Vec3 {
                x: if i == 0 { -440 } else { 480 },
                y: 130 - drop,
                z,
            },
            0,
            12,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.ascent.entry")]
pub fn scenic_ascent(sink: &mut SceneSink<'_>, context: &Decoration) {
    let tick = context.elapsed;
    // The coast and clouds recede below the cockpit during the climb.
    if tick < 250 {
        super::land(sink, context, tick as i16 * 24);
    }
    if tick >= 100 {
        stars(sink);
    }
    if tick >= 250 {
        let remaining = if tick >= 399 { 0 } else { 400 - tick };
        let arrival = (remaining * remaining >> 3) as i16;
        // Level out at orbital height: the curved planet enters from the left
        // and recedes below the ship, reaching the next scene's exact position.
        planet(sink, -1040 - arrival, -540 + remaining as i16 * 4);
        asteroid_debris(sink, tick.wrapping_sub(400), arrival + context.camera_y);
    }
    // Thin cloud wisps rush downward while passing through the upper atmosphere.
    if (70..180).contains(&tick) {
        for i in 0..3i16 {
            let y = 1800 - ((tick as i16 * 23 + i * 370) & 2047);
            let x = -1400 + i * 1280;
            sink.triangle(
                Vec3 { x, y, z: 1700 },
                Vec3 {
                    x: x + 450,
                    y: y - 70,
                    z: 1700,
                },
                Vec3 {
                    x: x + 100,
                    y: y - 180,
                    z: 1700,
                },
                14,
                0,
            );
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.asteroid.entry")]
pub fn scenic_asteroid(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink);
    let travel = drop(context);
    planet(sink, -1040, -540 - travel);
    asteroid_debris(sink, clock(context), travel + context.camera_y);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.moon.entry")]
pub fn scenic_moon(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink);
    let travel = drop(context);
    body(sink, 720, 680 - travel, &MOON);
    let ticks = clock(context);
    for i in 0..3u16 {
        let z = 2150 - ((ticks.wrapping_add(i * 85) & 255) * 8) as i16;
        rock(
            sink,
            Vec3 {
                x: if i & 1 == 0 { -360 } else { 430 },
                y: -160 - travel - context.camera_y,
                z,
            },
            1,
            15,
        );
    }
}

// Identical closed wreck geometry is authored once, using each environment's
// original facet palette.
#[cfg_attr(target_arch = "mos", link_section = ".text.orbital_data")]
static HULL_POINTS: [[i16; 3]; 8] = [
    [-100, 0, 0],
    [100, 0, 0],
    [65, 90, 620],
    [-65, 90, 620],
    [65, -70, 620],
    [-65, -70, 620],
    [0, -120, -100],
    [210, 140, 770],
];
#[cfg_attr(target_arch = "mos", link_section = ".text.orbital_data")]
static HULL_FACES: [[u8; 4]; 12] = [
    [0, 3, 2, 0],
    [0, 2, 1, 0],
    [0, 6, 5, 1],
    [0, 5, 3, 1],
    [1, 2, 4, 1],
    [1, 4, 6, 1],
    [0, 1, 6, 2],
    [6, 4, 5, 1],
    [3, 5, 4, 3],
    [3, 4, 2, 3],
    [3, 2, 7, 4],
    [3, 7, 2, 4],
];

// Each compact decoder remains in its own loaded tail. A shared decoder would
// enlarge every terrestrial scenery image even though it never draws wrecks.
macro_rules! wreck {
    ($name:ident, $section:literal, $palette:expr) => {
        #[inline(never)]
        #[cfg_attr(target_arch = "mos", link_section = $section)]
        fn $name(sink: &mut SceneSink<'_>, x: i16, y: i16, z: i16) {
            if sink.shadow_enabled() {
                sink.shadow_caster(crate::shadow::Caster {
                    center: Vec3 {
                        x: x + 55,
                        y: y + 10,
                        z: z + 335,
                    },
                    half_x: 155,
                    half_z: 435,
                    shape: 2,
                });
            }
            for face in HULL_FACES {
                let points: [Vec3; 3] = core::array::from_fn(|corner| {
                    let p = HULL_POINTS[face[corner] as usize];
                    Vec3 {
                        x: x + p[0],
                        y: y + p[1],
                        z: z + p[2],
                    }
                });
                let color = $palette[face[3] as usize];
                sink.triangle(points[0], points[1], points[2], color, 0);
            }
        }
    };
}
wreck!(hull, ".phase.scenery.carrier", [15, 11, 8, 12, 12]);
wreck!(
    graveyard_hull,
    ".phase.scenery.graveyard",
    [15, 12, 8, 12, 15]
);

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.graveyard.entry")]
pub fn scenic_graveyard(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink);
    let travel = drop(context) + context.camera_y;
    let ticks = clock(context);
    let phase = ((ticks & 127) * 8) as i16;
    for i in 0..3u16 {
        let z = 52 + i as i16 * 1024 - phase;
        if z + 770 >= crate::render::NEAR {
            let x = if ((ticks >> 7).wrapping_add(i)) & 1 == 0 {
                -470
            } else {
                490
            };
            graveyard_hull(sink, x, 70 - travel, z);
        }
    }
    rock(
        sink,
        Vec3 {
            x: 900,
            y: 750 - travel,
            z: 2400,
        },
        2,
        11,
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.shipyard.entry")]
pub fn scenic_shipyard(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink);
    let travel = drop(context) + context.camera_y;
    let ticks = clock(context);
    for i in 0..3u16 {
        let z = 2150 - ((ticks.wrapping_add(i * 85) & 255) * 8) as i16;
        if sink.shadow_enabled() {
            sink.shadow_caster(crate::shadow::Caster {
                center: Vec3 {
                    x: 0,
                    y: 105 - travel,
                    z: z + 120,
                },
                half_x: 710,
                half_z: 120,
                shape: 2,
            });
        }
        for side in [-1i16, 1] {
            let x = side * 290;
            panel(
                sink,
                [
                    Vec3 {
                        x,
                        y: -180 - travel,
                        z,
                    },
                    Vec3 {
                        x: x + side * 28,
                        y: -180 - travel,
                        z,
                    },
                    Vec3 {
                        x: x + side * 28,
                        y: 360 - travel,
                        z: z + 30,
                    },
                    Vec3 {
                        x,
                        y: 360 - travel,
                        z: z + 30,
                    },
                ],
                12,
            );
            // Blue photovoltaic wings project outwards from each gantry.
            panel(
                sink,
                [
                    Vec3 {
                        x,
                        y: 110 - travel,
                        z: z + 50,
                    },
                    Vec3 {
                        x: x + side * 420,
                        y: 140 - travel,
                        z: z + 50,
                    },
                    Vec3 {
                        x: x + side * 420,
                        y: 140 - travel,
                        z: z + 240,
                    },
                    Vec3 {
                        x,
                        y: 110 - travel,
                        z: z + 240,
                    },
                ],
                6,
            );
        }
        panel(
            sink,
            [
                Vec3 {
                    x: -290,
                    y: 360 - travel,
                    z,
                },
                Vec3 {
                    x: 290,
                    y: 360 - travel,
                    z,
                },
                Vec3 {
                    x: 290,
                    y: 390 - travel,
                    z: z + 30,
                },
                Vec3 {
                    x: -290,
                    y: 390 - travel,
                    z: z + 30,
                },
            ],
            15,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.carrier.entry")]
pub fn scenic_carrier(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink);
    let travel = drop(context) + context.camera_y;
    let ticks = clock(context);
    let offset = ((ticks & 63) * 8) as i16;
    for i in 0..4i16 {
        let z = 40 + i * 512 - offset;
        deck(
            sink,
            [
                Vec3 {
                    x: -390,
                    y: -105 - travel,
                    z,
                },
                Vec3 {
                    x: 390,
                    y: -105 - travel,
                    z,
                },
                Vec3 {
                    x: 390,
                    y: -105 - travel,
                    z: z + 512,
                },
                Vec3 {
                    x: -390,
                    y: -105 - travel,
                    z: z + 512,
                },
            ],
            if (ticks >> 6).wrapping_add(i as u16) & 1 == 0 {
                11
            } else {
                12
            },
            crate::EXTERIOR_TERRAIN,
        );
        for side in [-1i16, 1] {
            deck(
                sink,
                [
                    Vec3 {
                        x: side * 112,
                        y: -103 - travel,
                        z,
                    },
                    Vec3 {
                        x: side * 120,
                        y: -103 - travel,
                        z,
                    },
                    Vec3 {
                        x: side * 120,
                        y: -103 - travel,
                        z: z + 512,
                    },
                    Vec3 {
                        x: side * 112,
                        y: -103 - travel,
                        z: z + 512,
                    },
                ],
                7,
                crate::EXTERIOR_TERRAIN,
            );
        }
    }
    // Keep an additional near row until its entire hull passes the camera.
    // Alternating sides follow the world row when the 128-tick window advances.
    // Keep their inward hull tips clear of the Dreadnought's +/-332 nacelles.
    let phase = ((ticks & 127) * 8) as i16;
    for i in 0..3u16 {
        let z = 52 + i as i16 * 1024 - phase;
        if z + 770 >= crate::render::NEAR {
            let x = if ((ticks >> 7).wrapping_add(i)) & 1 == 0 {
                -600
            } else {
                600
            };
            hull(sink, x, -40 - travel, z);
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.gate")]
fn gate_point(v: [i16; 2], radius: i16, travel: i16) -> Vec3 {
    Vec3 {
        x: crate::math::mul_q8(v[0], radius),
        y: crate::math::mul_q8(v[1], radius) + 100 - travel,
        z: 2300,
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.gate.entry")]
pub fn scenic_gate(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink);
    let travel = drop(context) + context.camera_y;
    for i in 0..8usize {
        let a = RING[i];
        let b = RING[(i + 1) & 7];
        quad(
            sink,
            [
                gate_point(a, 920, travel),
                gate_point(b, 920, travel),
                gate_point(b, 720, travel),
                gate_point(a, 720, travel),
            ],
            if i & 1 == 0 { 3 } else { 14 },
        );
    }
    // Twin power conduits lead toward the separate, destructible gate boss.
    for side in [-1i16, 1] {
        deck(
            sink,
            [
                Vec3 {
                    x: side * 300,
                    y: -180 - travel,
                    z: 120,
                },
                Vec3 {
                    x: side * 340,
                    y: -180 - travel,
                    z: 120,
                },
                Vec3 {
                    x: side * 600,
                    y: -400 - travel,
                    z: 2300,
                },
                Vec3 {
                    x: side * 550,
                    y: -400 - travel,
                    z: 2300,
                },
            ],
            14,
            0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GeometrySink;
    use std::{collections::BTreeMap, vec::Vec};

    #[derive(Default)]
    struct Capture(Vec<([Vec3; 3], u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }

    #[test]
    fn stationary_stars_are_white_sky_ink() {
        let mut scene = Capture::default();
        stars(&mut scene);
        assert_eq!(scene.0.len(), 10);
        assert!(scene
            .0
            .iter()
            .all(|&(_, color, flags)| color == 1 && flags == crate::game::STATIC_SKY));
        assert_eq!(crate::render::PALETTE_RGB[1], [255; 3]);
    }

    #[test]
    fn detailed_bodies_face_the_camera_and_keep_their_surface_palettes() {
        for moon in [false, true] {
            let mut scene = Capture::default();
            if moon {
                body(&mut scene, 720, 680, &MOON);
            } else {
                planet(&mut scene, -1040, -540);
            }
            assert!(scene.0.len() <= 24);
            let mut colors = std::collections::BTreeSet::new();
            for (p, color, flags) in scene.0 {
                let cross = (p[1].x - p[0].x) as i32 * (p[2].y - p[0].y) as i32
                    - (p[1].y - p[0].y) as i32 * (p[2].x - p[0].x) as i32;
                assert!(cross < 0, "body facet must survive backface culling: {p:?}");
                assert_eq!(flags, 0);
                colors.insert(color);
            }
            assert_eq!(
                colors,
                if moon {
                    [0, 11, 12, 15].into_iter().collect()
                } else {
                    [3, 6, 14].into_iter().collect()
                }
            );
        }
    }

    #[test]
    fn irregular_scenery_rocks_are_closed_outward_shells_with_gray_facets() {
        for (kind, z) in (0..3).flat_map(|kind| [0, 599, 600, 1600].map(|z| (kind, z))) {
            let mut scene = Capture::default();
            rock(
                &mut scene,
                Vec3 {
                    z,
                    ..Vec3::default()
                },
                kind,
                12,
            );
            assert_eq!(scene.0.len(), if z >= 600 { 8 } else { 12 });
            let mut edges = BTreeMap::new();
            for (p, color, _) in scene.0 {
                assert!([11, 12, 15].contains(&color));
                let v = p.map(|p| [p.x as i64, p.y as i64, (p.z - z) as i64]);
                let a = core::array::from_fn::<_, 3, _>(|i| v[1][i] - v[0][i]);
                let b = core::array::from_fn::<_, 3, _>(|i| v[2][i] - v[0][i]);
                let n = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                assert!((0..3).map(|i| n[i] * v[0][i]).sum::<i64>() > 0);
                for i in 0..3 {
                    *edges.entry((v[i], v[(i + 1) % 3])).or_insert(0) += 1;
                }
            }
            for (&(a, b), &count) in &edges {
                assert_eq!(count, 1);
                assert_eq!(edges.get(&(b, a)), Some(&1));
            }
        }
    }
}
