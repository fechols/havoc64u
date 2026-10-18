//! Original assets and portable package tools. Never linked into the C64 game.
use havoc::{assets::*, music, soundtrack};
use std::{boxed::Box, format, fs, path::Path, println, vec, vec::Vec};

fn put16(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
pub fn common_bank() -> [u8; COMMON_SIZE] {
    let mut b = [0; COMMON_SIZE];
    b[..MODEL_SIZE].copy_from_slice(&model_bank(0));
    let score = soundtrack::score_bank();
    b[MODEL_SIZE..MODEL_SIZE + music::BANK_HEADER_SIZE]
        .copy_from_slice(&score[..music::BANK_HEADER_SIZE]);
    AssetBank::new(&b).expect("generated common bank");
    b
}

/// A complete resident model image, including the section's local enemy stats.
pub fn model_bank(level: u8) -> [u8; MODEL_SIZE] {
    assert!(level < SEGMENT_COUNT);
    let mut b = [0; MODEL_SIZE];
    b[..4].copy_from_slice(b"SMA3");
    b[4] = MODEL_COUNT;
    b[5] = level;
    b[6] = if level == 3 { 2 } else { 3 };
    let hp = [[3, 2, 6], [2, 3, 4], [4, 2, 6], [3, 2, 6]][level as usize];
    let speed = [[8, 12, 8], [10, 8, 8], [6, 8, 8], [8, 14, 8]][level as usize];
    // Keep the lowland gun and fortress bunker seated on their original bases.
    // The cliff battery and carrier-mounted turrets scale around their origins.
    let ground_pivot_y = if matches!(level, 0 | 2) { -8 } else { 0 };
    for slot in 0..3 {
        let at = ENEMY_SPECS_OFFSET + slot * ENEMY_SPEC_SIZE;
        b[at] = hp[slot];
        let center_y = if slot == 2 && level != 1 { 10 } else { 0 };
        let pivot_y = if slot == 2 { ground_pivot_y } else { 0 };
        b[at + 1] = (pivot_y + (center_y - pivot_y) * ENEMY_SCALE) as u8;
        b[at + 2] = (19 * ENEMY_SCALE) as u8;
        b[at + 3] = (18 * ENEMY_SCALE) as u8;
        put16(&mut b, at + 4, if slot == 2 { 250 } else { 100 });
        b[at + 6] = speed[slot];
        b[at + 7] = u8::from(slot == 1 && level != 2);
    }
    let mut cursor = MODEL_DATA_OFFSET;
    let mut models: Vec<(Vec<[i16; 3]>, Vec<[u8; 4]>)> = vec![
        // Kestrel: swept delta wings, raised cockpit, split aft fins.
        (
            vec![
                [0, 0, 36],
                [-36, -2, -16],
                [36, -2, -16],
                [0, 12, -3],
                [0, -7, -14],
                [-10, 0, -26],
                [10, 0, -26],
                [0, 8, -27],
            ],
            vec![
                [0, 1, 3, 15],
                [0, 3, 2, 1],
                [0, 4, 1, 12],
                [0, 2, 4, 12],
                [1, 5, 3, 14],
                [2, 3, 6, 14],
                [3, 5, 7, 6],
                [3, 7, 6, 6],
                [4, 5, 1, 11],
                [4, 2, 6, 11],
                [4, 6, 5, 11],
                [5, 6, 7, 8],
            ],
        ),
        (
            vec![
                [0, 0, -30],
                [-27, 0, 14],
                [27, 0, 14],
                [0, 10, 8],
                [0, -7, 8],
                [0, 0, 25],
            ],
            vec![
                [0, 1, 3, 2],
                [0, 3, 2, 10],
                [0, 4, 1, 9],
                [0, 2, 4, 9],
                [1, 5, 3, 8],
                [3, 5, 2, 10],
                [1, 4, 5, 11],
                [2, 5, 4, 11],
            ],
        ),
        (
            vec![
                [0, 0, -32],
                [-18, 0, 20],
                [18, 0, 20],
                [0, 18, 13],
                [0, -10, 13],
            ],
            vec![
                [0, 1, 3, 4],
                [0, 3, 2, 10],
                [0, 4, 1, 6],
                [0, 2, 4, 6],
                [1, 4, 3, 12],
                [2, 3, 4, 12],
            ],
        ),
        (
            vec![
                [-20, -8, -20],
                [20, -8, -20],
                [20, -8, 20],
                [-20, -8, 20],
                [-12, 12, -12],
                [12, 12, -12],
                [12, 12, 12],
                [-12, 12, 12],
                [0, 18, -28],
            ],
            vec![
                [0, 4, 1, 12],
                [1, 4, 5, 15],
                [1, 5, 2, 11],
                [2, 5, 6, 12],
                [2, 6, 3, 11],
                [3, 6, 7, 12],
                [3, 7, 0, 11],
                [0, 7, 4, 12],
                [4, 7, 6, 15],
                [4, 6, 5, 15],
                [4, 8, 5, 2],
            ],
        ),
        (
            vec![
                [-85, -16, -70],
                [85, -16, -70],
                [70, -16, 80],
                [-70, -16, 80],
                [-60, 15, -45],
                [60, 15, -45],
                [45, 15, 65],
                [-45, 15, 65],
                [0, 38, 30],
                [0, 0, -86],
            ],
            vec![
                [0, 4, 1, 12],
                [1, 4, 5, 15],
                [1, 5, 2, 11],
                [2, 5, 6, 12],
                [2, 6, 3, 12],
                [3, 6, 7, 15],
                [3, 7, 0, 11],
                [0, 7, 4, 12],
                [4, 7, 8, 14],
                [7, 6, 8, 6],
                [6, 5, 8, 14],
                [5, 4, 8, 15],
                [0, 1, 9, 2],
                [0, 9, 4, 10],
                [1, 5, 9, 10],
                [0, 3, 2, 11],
                [0, 2, 1, 11],
            ],
        ),
        (
            vec![
                [0, 20, 0],
                [-13, 0, 0],
                [0, 0, -13],
                [13, 0, 0],
                [0, 0, 13],
                [0, -20, 0],
            ],
            vec![
                [0, 1, 2, 3],
                [0, 2, 3, 1],
                [0, 3, 4, 7],
                [0, 4, 1, 13],
                [5, 2, 1, 6],
                [5, 3, 2, 14],
                [5, 4, 3, 8],
                [5, 1, 4, 5],
            ],
        ),
        (
            vec![
                [-22, 0, -22],
                [22, 0, -22],
                [22, 0, 22],
                [-22, 0, 22],
                [-22, 60, -22],
                [22, 60, -22],
                [22, 60, 22],
                [-22, 60, 22],
            ],
            vec![
                [0, 4, 1, 12],
                [1, 4, 5, 15],
                [1, 5, 2, 11],
                [2, 5, 6, 12],
                [2, 6, 3, 11],
                [3, 6, 7, 12],
                [3, 7, 0, 11],
                [0, 7, 4, 12],
                [4, 7, 6, 15],
                [4, 6, 5, 15],
            ],
        ),
        (
            vec![
                [-32, 0, -25],
                [28, 0, -30],
                [35, 0, 28],
                [-26, 0, 30],
                [-8, 42, 0],
                [15, 27, -12],
            ],
            vec![
                [0, 4, 1, 9],
                [1, 4, 5, 8],
                [1, 5, 2, 12],
                [2, 5, 4, 15],
                [2, 4, 3, 12],
                [3, 4, 0, 11],
            ],
        ),
    ];
    for (slot, model) in section_enemy_models(level).into_iter().enumerate() {
        models[slot + 1] = model;
    }
    let medal = medal_model(&models[PLAYER as usize]);
    models.push(medal);
    // Bake the larger combat meshes so the C64 retains identity transforms and
    // the existing model-bank size. Slot three also supplies the boss turrets.
    for id in FIGHTER..=CARRIER {
        let pivot_y = if id == TURRET { ground_pivot_y } else { 0 };
        for vertex in &mut models[id as usize].0 {
            vertex[0] *= ENEMY_SCALE;
            vertex[1] = pivot_y + (vertex[1] - pivot_y) * ENEMY_SCALE;
            vertex[2] *= ENEMY_SCALE;
        }
    }
    for (id, (vertices, faces)) in models.iter().enumerate() {
        let d = 16 + id * 8;
        put16(&mut b, d, cursor as u16);
        b[d + 2] = vertices.len() as u8;
        b[d + 3] = faces.len() as u8;
        for vertex in vertices {
            for v in vertex {
                put16(&mut b, cursor, *v as u16);
                cursor += 2;
            }
        }
        put16(&mut b, d + 4, cursor as u16);
        for face in faces {
            b[cursor..cursor + 4].copy_from_slice(face);
            cursor += 4;
        }
    }
    assert!(cursor <= MODEL_SIZE, "model bank exceeds resident arena");
    AssetBank::new(&b).expect("generated model bank");
    b
}

type Model = (Vec<[i16; 3]>, Vec<[u8; 4]>);

/// A closed twelve-sided coin with a raised relief of the player's Kestrel.
/// Clockwise rings face the camera (-Z); reverse winding closes the back.
fn medal_model(player: &Model) -> Model {
    let circle = [
        [0, 100],
        [50, 87],
        [87, 50],
        [100, 0],
        [87, -50],
        [50, -87],
        [0, -100],
        [-50, -87],
        [-87, -50],
        [-100, 0],
        [-87, 50],
        [-50, 87],
    ];
    // Use the actual PLAYER footprint and upper hull, viewed nose-up. The
    // underside stays buried in the coin; the cockpit and aft fin retain relief.
    let footprint = [0usize, 2, 6, 7, 5, 1];
    let top_vertices = [0usize, 1, 2, 3, 5, 6, 7];
    let top_index = [43u8, 44, 45, 46, u8::MAX, 47, 48, 49];
    let mut vertices = Vec::new();
    // Cache the most frequently shared front/bevel rings first.
    for (radius, z) in [(120, -8), (104, -14), (120, 10)] {
        for [x, y] in circle {
            vertices.push([x * radius / 100, y * radius / 100, z]);
        }
    }
    vertices.push([0, 0, -14]); // 36: coin front center.
    for index in footprint {
        let [x, _, z] = player.0[index];
        vertices.push([x * 44 / 36, (z - 4) * 44 / 36, -14]);
    }
    for index in top_vertices {
        let [x, y, z] = player.0[index];
        vertices.push([x, z - 4, -26 - y / 2]);
    }
    let mut faces = Vec::new();
    for i in 0u8..12 {
        let next = (i + 1) % 12;
        let bevel = if i >= 9 || i <= 1 { 7 } else { 8 };
        let side = if i >= 8 { 8 } else { 9 };
        // Thick, flat edge and the sloping bevel are separate surfaces.
        faces.push([i, i + 24, next + 24, side]);
        faces.push([i, next + 24, next, side]);
        faces.push([i, next, next + 12, bevel]);
        faces.push([i, next + 12, i + 12, bevel]);
        faces.push([36, i + 12, next + 12, 8]);
    }
    // Triangulate the convex back without adding a redundant center vertex.
    for i in 1u8..11 {
        faces.push([24, 24 + i + 1, 24 + i, 9]);
    }
    for i in 0u8..6 {
        let next = (i + 1) % 6;
        let top = top_index[footprint[i as usize]];
        let top_next = top_index[footprint[next as usize]];
        // The wider foot forms a brown bevel around the complete silhouette.
        faces.push([37 + i, 37 + next, top_next, 9]);
        faces.push([37 + i, top_next, top, 9]);
    }
    for (face, color) in [(0, 7), (1, 7), (4, 7), (5, 8), (6, 7), (7, 8)] {
        let [a, b, c, _] = player.1[face];
        // PLAYER renders two-sided; reverse its upper winding for the solid
        // medal's backface culling. Shaded aft facets show the cockpit ridge.
        faces.push([
            top_index[a as usize],
            top_index[c as usize],
            top_index[b as usize],
            color,
        ]);
    }
    assert_eq!((vertices.len(), faces.len()), (50, 88));
    (vertices, faces)
}

fn section_enemy_models(level: u8) -> Vec<Model> {
    match level {
        0 => vec![], // The original lowland patrol, diver and ground turret.
        1 => vec![
            // Ravine skimmer: narrow keel with two long outboard engine rails.
            (
                vec![
                    [0, 0, -28],
                    [-12, 5, -8],
                    [12, 5, -8],
                    [0, -7, 20],
                    [-38, 0, -20],
                    [-38, 0, 26],
                    [38, 0, -20],
                    [38, 0, 26],
                ],
                vec![
                    [0, 1, 2, 14],
                    [0, 3, 1, 6],
                    [0, 2, 3, 6],
                    [1, 3, 2, 15],
                    [1, 4, 5, 3],
                    [1, 5, 3, 14],
                    [2, 7, 6, 3],
                    [2, 3, 7, 14],
                    [0, 4, 1, 11],
                    [0, 2, 6, 11],
                ],
            ),
            // Cliff swooper: high gull wings around a hanging armored cockpit.
            (
                vec![
                    [0, -3, -35],
                    [-18, 19, 7],
                    [-34, 34, 23],
                    [18, 19, 7],
                    [34, 34, 23],
                    [0, -17, 12],
                    [0, 8, 29],
                ],
                vec![
                    [0, 1, 6, 8],
                    [0, 6, 3, 10],
                    [1, 2, 6, 7],
                    [3, 6, 4, 7],
                    [0, 5, 1, 9],
                    [0, 3, 5, 9],
                    [1, 5, 6, 12],
                    [3, 6, 5, 12],
                ],
            ),
            // Wall battery: vertical mounting plate and a forward cannon wedge.
            (
                vec![
                    [-15, -28, 14],
                    [15, -28, 14],
                    [15, 28, 14],
                    [-15, 28, 14],
                    [-11, -18, -5],
                    [11, -18, -5],
                    [11, 18, -5],
                    [-11, 18, -5],
                    [0, 0, -34],
                ],
                vec![
                    [0, 1, 4, 11],
                    [1, 5, 4, 12],
                    [1, 2, 5, 11],
                    [2, 6, 5, 12],
                    [2, 3, 6, 11],
                    [3, 7, 6, 15],
                    [3, 0, 7, 11],
                    [0, 4, 7, 12],
                    [4, 5, 8, 2],
                    [5, 6, 8, 8],
                    [6, 7, 8, 15],
                    [7, 4, 8, 12],
                ],
            ),
        ],
        2 => vec![
            // Siege gunship: broad rectangular armor and two forward gun pods.
            (
                vec![
                    [-32, -7, -18],
                    [32, -7, -18],
                    [32, -7, 22],
                    [-32, -7, 22],
                    [-24, 11, -12],
                    [24, 11, -12],
                    [24, 11, 18],
                    [-24, 11, 18],
                    [-22, 0, -38],
                    [22, 0, -38],
                ],
                vec![
                    [0, 4, 1, 11],
                    [1, 4, 5, 12],
                    [1, 5, 2, 11],
                    [2, 5, 6, 12],
                    [2, 6, 3, 11],
                    [3, 6, 7, 12],
                    [3, 7, 0, 11],
                    [0, 7, 4, 15],
                    [4, 7, 6, 15],
                    [4, 6, 5, 12],
                    [0, 8, 4, 2],
                    [1, 5, 9, 2],
                ],
            ),
            // Lance interceptor: exceptionally long needle nose and small fins.
            (
                vec![
                    [0, 0, -48],
                    [-12, 0, 21],
                    [12, 0, 21],
                    [0, 9, 12],
                    [0, -8, 16],
                    [0, 0, 34],
                ],
                vec![
                    [0, 1, 3, 7],
                    [0, 3, 2, 15],
                    [0, 4, 1, 8],
                    [0, 2, 4, 8],
                    [1, 5, 3, 2],
                    [3, 5, 2, 10],
                    [1, 4, 5, 11],
                    [2, 5, 4, 11],
                ],
            ),
            // Fan bunker: low trapezoid armored roof and a broad gun aperture.
            (
                vec![
                    [-34, -8, -25],
                    [34, -8, -25],
                    [34, -8, 25],
                    [-34, -8, 25],
                    [-24, 15, -18],
                    [24, 15, -18],
                    [18, 15, 18],
                    [-18, 15, 18],
                    [0, 7, -39],
                ],
                vec![
                    [0, 4, 1, 12],
                    [1, 4, 5, 15],
                    [1, 5, 2, 11],
                    [2, 5, 6, 12],
                    [2, 6, 3, 11],
                    [3, 6, 7, 12],
                    [3, 7, 0, 11],
                    [0, 7, 4, 12],
                    [4, 7, 6, 15],
                    [4, 6, 5, 15],
                    [4, 8, 5, 2],
                ],
            ),
        ],
        3 => vec![
            // Carrier guard: split forward pincers surrounding a shielded hub.
            (
                vec![
                    [-30, 0, -28],
                    [-10, 0, -12],
                    [10, 0, -12],
                    [30, 0, -28],
                    [-23, 0, 25],
                    [23, 0, 25],
                    [0, 13, 8],
                    [0, -10, 8],
                ],
                vec![
                    [0, 4, 1, 4],
                    [1, 4, 6, 14],
                    [1, 6, 2, 15],
                    [2, 6, 5, 14],
                    [2, 5, 3, 4],
                    [4, 5, 6, 6],
                    [0, 1, 7, 11],
                    [1, 2, 7, 12],
                    [2, 3, 7, 11],
                    [4, 7, 5, 11],
                ],
            ),
            // Reactor rammer: a pointed drill body with a tall triangular fin.
            (
                vec![
                    [0, 0, -42],
                    [-17, -5, 18],
                    [17, -5, 18],
                    [0, 27, 11],
                    [0, -12, 11],
                    [0, 0, 27],
                ],
                vec![
                    [0, 1, 3, 2],
                    [0, 3, 2, 7],
                    [0, 4, 1, 8],
                    [0, 2, 4, 8],
                    [1, 5, 3, 10],
                    [3, 5, 2, 15],
                    [1, 4, 5, 11],
                    [2, 5, 4, 11],
                ],
            ),
        ],
        _ => unreachable!(),
    }
}

pub fn model_directory() -> [u8; MODEL_DIRECTORY_SIZE] {
    let mut directory = [0; MODEL_DIRECTORY_SIZE];
    directory[..4].copy_from_slice(b"MDL1");
    directory[4] = SEGMENT_COUNT;
    directory[5] = 1;
    put16(&mut directory, 6, MODEL_SIZE as u16);
    put32(&mut directory, 8, MODEL_BANKS_OFFSET);
    for level in 0..SEGMENT_COUNT {
        put16(
            &mut directory,
            12 + level as usize * 2,
            checksum(&model_bank(level)),
        );
    }
    let sum = checksum(&directory[..30]);
    put16(&mut directory, 30, sum);
    directory
}

pub fn segment_bank(index: u8) -> [u8; SEGMENT_SIZE] {
    let mut b = [0; SEGMENT_SIZE];
    b[..4].copy_from_slice(b"SMS1");
    b[4] = index;
    b[5] = index;
    b[6] = [5, 9, 11, 6][index as usize];
    let mut events: Vec<Spawn> = Vec::new();
    if index == 0 {
        for wave in 0..10u16 {
            let base = 1200 + wave * 2700;
            for lane in 0..3i16 {
                events.push(Spawn {
                    distance: base + lane as u16 * 100,
                    kind: if wave % 3 == 2 { DIVER } else { FIGHTER },
                    x: (lane - 1) * 80,
                    y: if wave % 2 == 0 { 35 } else { -25 },
                    flags: if wave % 4 == 0 && lane == 1 { 1 } else { 0 },
                });
            }
            if wave % 2 == 1 {
                events.push(Spawn {
                    distance: base + 1200,
                    kind: TURRET,
                    x: if wave % 4 == 1 { -95 } else { 95 },
                    y: -90,
                    flags: 0,
                });
            }
        }
    } else if index < 3 {
        for wave in 0..10u16 {
            let base = 1200 + wave * 2700;
            for lane in 0..2i16 {
                let side = if lane == 0 { -1 } else { 1 };
                let fortress_pair = index == 2 && wave % 2 == 0;
                events.push(Spawn {
                    distance: base + lane as u16 * if fortress_pair { 300 } else { 160 },
                    kind: if wave % 2 == 0 && (!fortress_pair || lane == 0) {
                        FIGHTER
                    } else {
                        DIVER
                    },
                    x: side
                        * if index == 1 && wave % 2 == 0 {
                            110
                        } else if fortress_pair {
                            60
                        } else {
                            70
                        },
                    y: if fortress_pair {
                        30
                    } else if index == 2 {
                        side * 25
                    } else {
                        0
                    },
                    flags: u8::from(wave % 4 == 0 && lane == 1),
                });
            }
            events.push(Spawn {
                distance: base + if index == 1 { 800 } else { 1000 },
                kind: TURRET,
                x: if wave % 2 == 0 { -90 } else { 90 },
                y: if index == 1 { 0 } else { -90 },
                flags: 0,
            });
        }
    } else {
        for (wave, distance) in [3200, 6400, 12800, 19200, 25600].into_iter().enumerate() {
            let side = if wave % 2 == 0 { -1 } else { 1 };
            events.push(Spawn {
                distance,
                kind: FIGHTER,
                x: side * 75,
                y: 25,
                flags: 0,
            });
            events.push(Spawn {
                distance,
                kind: DIVER,
                x: -side * 75,
                y: -15,
                flags: u8::from(wave % 2 == 0),
            });
        }
    }
    if index < 3 {
        for i in 0..6u16 {
            events.push(Spawn {
                distance: 2200 + i * 4300,
                kind: if index == 2 { BUILDING } else { ROCK },
                x: if i % 2 == 0 { -70 } else { 70 },
                y: -110,
                flags: 0,
            });
        }
    }
    assert!(
        events.len() <= MAX_SEGMENT_SPAWNS as usize,
        "encounters exceed the runtime segment cache"
    );
    events.sort_by_key(|e| e.distance);
    b[7] = events.len() as u8;
    for (i, e) in events.iter().enumerate() {
        let p = 16 + i * 8;
        put16(&mut b, p, e.distance);
        b[p + 2] = e.kind;
        b[p + 3] = e.flags;
        put16(&mut b, p + 4, e.x as u16);
        put16(&mut b, p + 6, e.y as u16);
    }
    Segment::new(&b).expect("generated segment");
    b
}

pub fn bake(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(out)?;
    let common = common_bank();
    let mut reu = vec![0; REU_SIZE as usize];
    reu[..4].copy_from_slice(b"SM64");
    put32(&mut reu, 4, PACKAGE_ID);
    put32(&mut reu, 8, COMMON_OFFSET);
    put32(&mut reu, 12, SEGMENTS_OFFSET);
    reu[16] = SEGMENT_COUNT;
    reu[17] = music::SID_COUNT as u8;
    reu[MODEL_DIRECTORY_OFFSET as usize..MODEL_DIRECTORY_OFFSET as usize + MODEL_DIRECTORY_SIZE]
        .copy_from_slice(&model_directory());
    put16(&mut reu, 18, checksum(&common));
    reu[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE].copy_from_slice(&common);
    let score = soundtrack::score_bank();
    reu[MUSIC_OFFSET as usize..MUSIC_OFFSET as usize + score.len()].copy_from_slice(&score);
    for i in 0..4u8 {
        let model = model_bank(i);
        let model_start = MODEL_BANKS_OFFSET as usize + i as usize * MODEL_SIZE;
        reu[model_start..model_start + MODEL_SIZE].copy_from_slice(&model);
        let segment = segment_bank(i);
        let p = SEGMENTS_OFFSET as usize + i as usize * SEGMENT_SIZE;
        reu[p..p + SEGMENT_SIZE].copy_from_slice(&segment);
        put16(
            &mut reu,
            20 + i as usize * 2,
            checksum(&segment[..SEGMENT_CACHE_SIZE]),
        );
    }
    let check = checksum(&reu[..30]);
    put16(&mut reu, 30, check);
    check_package(&reu)?;
    fs::write(out.join("havoc.reu"), &reu)?;
    fs::write(
        out.join("havoc.cfg"),
        if cfg!(feature = "four-sid") {
            include_str!("../config/havoc-4sid.cfg")
        } else {
            include_str!("../config/havoc.cfg")
        },
    )?;
    fs::write(out.join("assets.txt"),format!("H.A.V.O.C. package {PACKAGE_ID:08X}\n16 MiB REU; common {COMMON_OFFSET:06X} (3072 bytes; 2048 resident model bytes); four checked 2048-byte level model banks at {MODEL_BANKS_OFFSET:06X}; model directory at {MODEL_DIRECTORY_OFFSET:06X}; four 2048-byte segments at {SEGMENTS_OFFSET:06X}, each with a checksummed 512-byte runtime prefix (up to 62 encounters)\nEleven original section-exclusive enemies: 3 lowland, 3 canyon, 3 fortress, 2 carrier escorts.\n{} SIDs: {} music voices + 2 effects voices. Seven original cues; {} bytes of paged score data at {MUSIC_OFFSET:06X}.\nOriginal models and music; no external ROMs or samples.\n", music::SID_COUNT, music::MUSIC_VOICES, score.len()))?;
    println!(
        "Baked {} ({} bytes)",
        out.join("havoc.reu").display(),
        reu.len()
    );
    Ok(())
}
pub fn check_package(reu: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if reu.len() != REU_SIZE as usize {
        return Err("REU must be exactly16MiB".into());
    }
    validate_header(reu).map_err(|e| format!("package header: {e:?}"))?;
    let common = &reu[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE];
    let bank = AssetBank::new(common).map_err(|e| format!("common bank: {e:?}"))?;
    if bank.level() != 0 {
        return Err("common bank must contain lowland models".into());
    }
    let directory = &reu
        [MODEL_DIRECTORY_OFFSET as usize..MODEL_DIRECTORY_OFFSET as usize + MODEL_DIRECTORY_SIZE];
    validate_model_directory(directory).map_err(|e| format!("model directory: {e:?}"))?;
    for level in 0..SEGMENT_COUNT {
        let start = MODEL_BANKS_OFFSET as usize + level as usize * MODEL_SIZE;
        let data = &reu[start..start + MODEL_SIZE];
        let models = AssetBank::new(data).map_err(|e| format!("model bank {level}: {e:?}"))?;
        if models.level() != level || checksum(data) != model_checksum(directory, level) {
            return Err("model checksum/index".into());
        }
        if level == 0 && data != &common[..MODEL_SIZE] {
            return Err("common and lowland model bank differ".into());
        }
    }
    let music_bank = &reu[MUSIC_OFFSET as usize..MUSIC_OFFSET as usize + 0x10000];
    if bank.audio()[..music::BANK_HEADER_SIZE] != music_bank[..music::BANK_HEADER_SIZE] {
        return Err("music directory does not match common assets".into());
    }
    validate_soundtrack(music_bank)?;
    if checksum(common) != u16_at(reu, 18) {
        return Err("common checksum".into());
    }
    for i in 0..4 {
        let p = SEGMENTS_OFFSET as usize + i * SEGMENT_SIZE;
        let data = &reu[p..p + SEGMENT_SIZE];
        let s = Segment::new(data).map_err(|e| format!("segment {i}: {e:?}"))?;
        if s.index() != i as u8 || checksum(&data[..SEGMENT_CACHE_SIZE]) != u16_at(reu, 20 + i * 2)
        {
            return Err("segment checksum/index".into());
        }
    }
    Ok(())
}

pub fn validate_soundtrack(bank: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut end = music::BANK_HEADER_SIZE;
    for cue in 0..music::CUE_COUNT as u8 {
        let info = music::cue_info(bank, cue).map_err(|e| format!("music directory: {e:?}"))?;
        if info.offset as usize != end {
            return Err("music page directory is not contiguous".into());
        }
        end += info.pages as usize * music::PAGE_SIZE;
        if end > bank.len() {
            return Err("truncated music bank".into());
        }
        let pages = &bank[info.offset as usize..end];
        if checksum(pages) != info.checksum {
            return Err("music cue checksum".into());
        }
        for (index, page) in pages.chunks_exact(music::PAGE_SIZE).enumerate() {
            music::validate_page(page, cue, index as u8)
                .map_err(|e| format!("music cue {cue} page {index}: {e:?}"))?;
            if page[6] != ((index + 1) % info.pages as usize) as u8 || page[7] != info.row_ticks {
                return Err("music page transport mismatch".into());
            }
            let previous = &pages
                [((index + info.pages as usize - 1) % info.pages as usize) * music::PAGE_SIZE..];
            for voice in 0..music::MUSIC_VOICES {
                let event = page[music::EVENT_OFFSET + voice];
                let at = music::INSTRUMENT_OFFSET + voice * 5;
                if (event == 1 || event & 128 != 0) && page[at..at + 5] != previous[at..at + 5] {
                    return Err("music changes instrument under a boundary tie".into());
                }
                if index == 0 && (event == 1 || event & 128 != 0) {
                    return Err("music opening needs an attack or rest".into());
                }
            }
        }
    }
    if u32_at(bank, 8) as usize != end {
        return Err("music bank length mismatch".into());
    }
    Ok(())
}
