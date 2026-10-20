//! Small, independently loaded orbital environments. Only visible hazards are
//! actors; these distant structures stay outside the player's flight corridor.
use super::{Decoration, SceneSink};
#[cfg(target_arch = "mos")]
use crate::GeometrySink;
use crate::{Vec3, TWO_SIDED};

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared.data")]
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared.data")]
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
fn stars(sink: &mut SceneSink<'_>, context: &Decoration) {
    for [x, y] in STARS {
        sink.triangle(
            Vec3 { x, y, z: 3000 },
            Vec3 {
                x: x + 45,
                y,
                z: 3000,
            },
            Vec3 {
                x,
                y: y + 45,
                z: 3000,
            },
            15,
            TWO_SIDED
                | if context.elapsed >= 400 {
                    crate::game::STATIC_SKY
                } else {
                    0
                },
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn quad(sink: &mut SceneSink<'_>, p: [Vec3; 4], color: u8) {
    sink.triangle(p[0], p[1], p[2], color, TWO_SIDED);
    sink.triangle(p[0], p[2], p[3], color, TWO_SIDED);
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn clock(context: &Decoration) -> u16 {
    if context.elapsed >= 200 && context.elapsed < 400 {
        context.elapsed.wrapping_sub(400)
    } else {
        context.scene_ticks
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn planet(sink: &mut SceneSink<'_>, x: i16, y: i16, radius: i16, color: u8) {
    let center = Vec3 { x, y, z: 2800 };
    for i in 0..8usize {
        let a = RING[i];
        let b = RING[(i + 1) & 7];
        sink.triangle(
            center,
            Vec3 {
                x: x + crate::math::mul_q8(a[0], radius),
                y: y + crate::math::mul_q8(a[1], radius),
                z: 2800,
            },
            Vec3 {
                x: x + crate::math::mul_q8(b[0], radius),
                y: y + crate::math::mul_q8(b[1], radius),
                z: 2800,
            },
            if i < 4 { color } else { 6 },
            TWO_SIDED,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn rock(sink: &mut SceneSink<'_>, center: Vec3, radius: i16, color: u8) {
    let top = Vec3 {
        y: center.y + radius,
        ..center
    };
    let bottom = Vec3 {
        y: center.y - radius,
        ..center
    };
    let ring = [
        Vec3 {
            x: center.x - radius,
            ..center
        },
        Vec3 {
            z: center.z + radius,
            ..center
        },
        Vec3 {
            x: center.x + radius,
            ..center
        },
        Vec3 {
            z: center.z - radius,
            ..center
        },
    ];
    for i in 0..4usize {
        let next = (i + 1) & 3;
        sink.triangle(top, ring[i], ring[next], color, TWO_SIDED);
        sink.triangle(bottom, ring[next], ring[i], 11, TWO_SIDED);
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
            95,
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
        stars(sink, context);
    }
    if tick >= 250 {
        let remaining = if tick >= 399 { 0 } else { 400 - tick };
        let arrival = (remaining * remaining >> 3) as i16;
        // Level out at orbital height: the curved planet enters from the left
        // and recedes below the ship, reaching the next scene's exact position.
        planet(sink, -1040 - arrival, -540 + remaining as i16 * 4, 760, 14);
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
                TWO_SIDED,
            );
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.asteroid.entry")]
pub fn scenic_asteroid(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink, context);
    let travel = drop(context);
    planet(sink, -1040, -540 - travel, 760, 14);
    asteroid_debris(sink, clock(context), travel + context.camera_y);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.moon.entry")]
pub fn scenic_moon(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink, context);
    let travel = drop(context);
    // The two displaced halves and dark crack read as a shattered moon.
    planet(sink, 720, 680 - travel, 560, 15);
    quad(
        sink,
        [
            Vec3 {
                x: 660,
                y: 1270 - travel,
                z: 2700,
            },
            Vec3 {
                x: 770,
                y: 1270 - travel,
                z: 2700,
            },
            Vec3 {
                x: 640,
                y: 100 - travel,
                z: 2700,
            },
            Vec3 {
                x: 510,
                y: 100 - travel,
                z: 2700,
            },
        ],
        0,
    );
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
            130,
            15,
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.space_shared")]
fn hull(sink: &mut SceneSink<'_>, x: i16, y: i16, z: i16, color: u8) {
    // Long broken hull: sloping top, torn bow and one exposed side.
    let a = Vec3 { x: x - 100, y, z };
    let b = Vec3 { x: x + 100, y, z };
    let c = Vec3 {
        x: x + 65,
        y: y + 90,
        z: z + 620,
    };
    let d = Vec3 {
        x: x - 65,
        y: y + 90,
        z: z + 620,
    };
    quad(sink, [a, b, c, d], color);
    quad(
        sink,
        [a, d, Vec3 { y: d.y - 160, ..d }, Vec3 { y: a.y - 120, ..a }],
        11,
    );
    sink.triangle(
        a,
        b,
        Vec3 {
            x,
            y: y - 120,
            z: z - 100,
        },
        8,
        TWO_SIDED,
    );
    sink.triangle(
        d,
        c,
        Vec3 {
            x: x + 210,
            y: y + 140,
            z: z + 770,
        },
        12,
        TWO_SIDED,
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.graveyard.entry")]
pub fn scenic_graveyard(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink, context);
    let travel = drop(context) + context.camera_y;
    let ticks = clock(context);
    for i in 0..3u16 {
        let z = 1800 - ((ticks.wrapping_add(i * 85) & 255) * 8) as i16;
        hull(
            sink,
            if i & 1 == 0 { -470 } else { 490 },
            70 - travel,
            z,
            12,
        );
    }
    rock(
        sink,
        Vec3 {
            x: 900,
            y: 750 - travel,
            z: 2400,
        },
        170,
        11,
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.shipyard.entry")]
pub fn scenic_shipyard(sink: &mut SceneSink<'_>, context: &Decoration) {
    stars(sink, context);
    let travel = drop(context) + context.camera_y;
    let ticks = clock(context);
    for i in 0..3u16 {
        let z = 2150 - ((ticks.wrapping_add(i * 85) & 255) * 8) as i16;
        for side in [-1i16, 1] {
            let x = side * 290;
            quad(
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
            quad(
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
        quad(
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
    stars(sink, context);
    let travel = drop(context) + context.camera_y;
    let ticks = clock(context);
    let offset = ((ticks & 63) * 8) as i16;
    for i in 0..4i16 {
        let z = 40 + i * 512 - offset;
        quad(
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
        );
        for side in [-1i16, 1] {
            quad(
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
            );
        }
    }
    for i in 0..2u16 {
        let z = 2100 - ((ticks.wrapping_add(i * 128) & 255) * 8) as i16;
        hull(sink, if i == 0 { -330 } else { 330 }, -40 - travel, z, 15);
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
    stars(sink, context);
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
        quad(
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
        );
    }
}
