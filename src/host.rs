//! Original assets and portable package tools. Never linked into the C64 game.
use havoc::{assets::*, audio, digital};
use std::{boxed::Box, format, fs, path::Path, println, vec, vec::Vec};

/// Reproducible compressed source; the original recordings are not required to build.
pub use digital::TRACK_ASSETS as DIGITAL_TRACKS;

/// All campaign choices are authored here and baked into independently checked
/// model/encounter pages. None of this table enters the resident game image.
struct StageDesign {
    name: &'static str,
    info: StageInfo,
    ground: u8,
    waves: &'static [Wave],
    obstacle: u8,
}
// A separately placed formation: distance, types (zero ends the formation),
// half-width, height, inter-ship delay, and the pickup-bearing ship (255 = none).
type Wave = (u16, [u8; 3], i16, i16, u16, u8);
const F: u8 = FIGHTER;
const D: u8 = DIVER;
const T: u8 = TURRET;
const CAMPAIGN: [StageDesign; SEGMENT_COUNT as usize] = [
    StageDesign {
        name: "Lowland Approach",
        info: STAGE_INFOS[0],
        ground: 5,
        waves: &[],
        obstacle: ROCK,
    },
    StageDesign {
        name: "Crystal Cave",
        info: STAGE_INFOS[1],
        ground: 11,
        waves: &[
            (1100, [F, F, 0], 65, 20, 0, 1),
            (3400, [D, D, 0], 95, 40, 180, 255),
            (5900, [F, F, F], 85, -20, 120, 1),
            (8300, [D, F, 0], 110, 15, 220, 255),
            (11100, [F, F, 0], 60, 45, 0, 0),
            (14100, [D, D, 0], 85, -25, 200, 255),
            (17200, [F, F, F], 90, 25, 100, 1),
            (20500, [F, D, 0], 100, -15, 160, 255),
            (23900, [D, D, 0], 75, 35, 100, 1),
            (27300, [F, F, F], 80, 0, 140, 255),
        ],
        obstacle: ROCK,
    },
    StageDesign {
        name: "Iron Canyon",
        info: STAGE_INFOS[2],
        ground: 9,
        waves: &[
            (1200, [F, F, 0], 110, 0, 160, 1),
            (3900, [D, D, 0], 70, 0, 160, 255),
            (6600, [F, F, 0], 110, 0, 160, 255),
            (9300, [D, D, 0], 70, 0, 160, 255),
            (12000, [F, F, 0], 110, 0, 160, 1),
            (14700, [D, D, 0], 70, 0, 160, 255),
            (17400, [F, F, 0], 110, 0, 160, 255),
            (20100, [D, D, 0], 70, 0, 160, 255),
            (22800, [F, F, 0], 110, 0, 160, 1),
            (25500, [D, D, 0], 70, 0, 160, 255),
        ],
        obstacle: ROCK,
    },
    StageDesign {
        name: "Canyon Gate",
        info: STAGE_INFOS[3],
        ground: 9,
        waves: &[
            (1600, [F, D, 0], 90, 20, 240, 0),
            (4300, [D, F, 0], 75, -15, 200, 1),
            (10800, [F, F, 0], 100, 35, 300, 255),
            (16400, [D, D, 0], 85, 20, 200, 1),
            (21600, [F, D, 0], 105, -10, 320, 255),
            (26700, [D, F, 0], 90, 30, 250, 0),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Ashen Wastes",
        info: STAGE_INFOS[4],
        ground: 11,
        waves: &[
            (1000, [D, D, 0], 80, 50, 350, 1),
            (3500, [F, D, F], 90, -20, 180, 255),
            (6400, [D, D, 0], 110, 45, 160, 0),
            (9100, [F, F, 0], 70, -10, 100, 255),
            (12100, [D, D, D], 90, 35, 200, 1),
            (15400, [F, D, 0], 100, 5, 280, 255),
            (18300, [D, D, 0], 65, 50, 100, 1),
            (21300, [F, F, F], 90, -15, 160, 255),
            (24600, [D, F, 0], 110, 30, 300, 255),
            (27500, [D, D, 0], 75, 45, 200, 0),
        ],
        obstacle: ROCK,
    },
    StageDesign {
        name: "Foundry Belt",
        info: STAGE_INFOS[5],
        ground: 12,
        waves: &[
            (1200, [F, D, 0], 60, 30, 300, 1),
            (4000, [D, F, 0], 90, -15, 260, 255),
            (7000, [F, F, 0], 70, 20, 350, 0),
            (9700, [D, D, 0], 105, 10, 180, 255),
            (12700, [F, D, F], 85, 30, 180, 1),
            (15800, [F, F, 0], 65, -10, 300, 255),
            (18800, [D, F, 0], 95, 25, 180, 1),
            (21900, [F, D, 0], 75, 0, 340, 255),
            (25000, [D, D, 0], 105, 40, 180, 255),
            (27800, [F, F, 0], 60, 15, 240, 0),
        ],
        obstacle: BUILDING,
    },
    StageDesign {
        name: "Fortress Run",
        info: STAGE_INFOS[6],
        ground: 11,
        waves: &[
            (1200, [F, D, 0], 60, 30, 300, 1),
            (3900, [D, D, 0], 70, 25, 160, 255),
            (6600, [F, D, 0], 60, 30, 300, 255),
            (9300, [D, D, 0], 70, -25, 160, 255),
            (12000, [F, D, 0], 60, 30, 300, 1),
            (14700, [D, D, 0], 70, 25, 160, 255),
            (17400, [F, D, 0], 60, 30, 300, 255),
            (20100, [D, D, 0], 70, -25, 160, 255),
            (22800, [F, D, 0], 60, 30, 300, 1),
            (25500, [D, D, 0], 70, 25, 160, 255),
        ],
        obstacle: BUILDING,
    },
    StageDesign {
        name: "Citadel Core",
        info: STAGE_INFOS[7],
        ground: 11,
        waves: &[
            (1400, [F, D, 0], 65, 35, 320, 1),
            (4500, [D, D, 0], 100, -10, 180, 0),
            (11400, [F, F, 0], 85, 30, 350, 255),
            (16900, [D, F, 0], 105, 15, 260, 1),
            (22200, [F, D, 0], 75, -15, 320, 255),
            (27200, [D, D, 0], 95, 35, 220, 0),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Storm Front",
        info: STAGE_INFOS[8],
        ground: 14,
        waves: &[
            (1000, [F, F, 0], 110, 35, 120, 1),
            (3700, [D, D, 0], 80, -20, 240, 255),
            (6500, [F, D, F], 95, 30, 180, 1),
            (9300, [D, F, 0], 65, 0, 300, 255),
            (12100, [F, F, 0], 115, 45, 100, 0),
            (15300, [D, D, D], 85, -10, 220, 255),
            (18400, [F, D, 0], 105, 25, 280, 1),
            (21500, [F, F, 0], 75, 40, 160, 255),
            (24700, [D, F, D], 100, 5, 200, 1),
            (27900, [F, F, 0], 90, 30, 160, 255),
        ],
        obstacle: ROCK,
    },
    StageDesign {
        name: "Meteor Reach",
        info: STAGE_INFOS[9],
        ground: 4,
        waves: &[
            (1300, [D, F, 0], 110, -15, 320, 0),
            (4100, [F, F, F], 95, 35, 160, 1),
            (7200, [D, D, 0], 75, 10, 260, 255),
            (10200, [F, D, 0], 105, -20, 300, 1),
            (13300, [F, F, 0], 60, 45, 100, 255),
            (16200, [D, F, D], 100, 0, 180, 1),
            (19400, [F, F, 0], 115, 30, 140, 255),
            (22500, [D, D, 0], 80, -15, 250, 0),
            (25300, [F, D, F], 95, 40, 180, 255),
            (28200, [D, F, 0], 105, 10, 300, 1),
        ],
        obstacle: ROCK,
    },
    StageDesign {
        name: "Carrier Coast",
        info: STAGE_INFOS[10],
        ground: 6,
        waves: &[
            (1200, [F, D, 0], 75, 25, 220, 1),
            (4000, [F, F, F], 95, -10, 150, 255),
            (6900, [D, D, 0], 100, 35, 180, 0),
            (9800, [F, F, 0], 65, 10, 250, 255),
            (12900, [F, D, F], 85, 30, 180, 1),
            (16000, [D, F, 0], 110, -15, 300, 255),
            (19200, [F, F, 0], 80, 45, 120, 1),
            (22200, [D, D, 0], 105, 0, 220, 255),
            (25200, [F, F, F], 90, 30, 160, 1),
            (28200, [F, D, 0], 70, 10, 260, 255),
        ],
        obstacle: BUILDING,
    },
    StageDesign {
        name: "Carrier Assault",
        info: STAGE_INFOS[11],
        ground: 6,
        waves: &[
            (1800, [F, D, 0], 75, 25, 0, 1),
            (4700, [D, F, 0], 95, -15, 240, 0),
            (10800, [F, F, 0], 105, 35, 220, 255),
            (15000, [D, D, 0], 85, 10, 160, 1),
            (19200, [F, D, 0], 110, -10, 240, 255),
            (23500, [D, F, 0], 95, 30, 220, 0),
            (27800, [F, D, 0], 80, 15, 180, 1),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Asteroid Belt",
        info: STAGE_INFOS[12],
        ground: 0,
        waves: &[
            (1100, [F, F, 0], 90, 60, 180, 1),
            (3800, [D, F, 0], 105, -35, 240, 255),
            (6700, [F, F, F], 80, 85, 120, 1),
            (9600, [D, D, 0], 110, -50, 300, 0),
            (12600, [F, D, 0], 65, 30, 220, 255),
            (15500, [F, F, F], 100, -25, 160, 1),
            (18500, [D, F, 0], 115, 70, 240, 255),
            (21500, [F, D, 0], 75, -40, 180, 0),
            (24600, [D, D, 0], 95, 45, 220, 1),
            (27600, [F, F, F], 85, 0, 140, 255),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Shattered Moon",
        info: STAGE_INFOS[13],
        ground: 0,
        waves: &[
            (1300, [D, F, 0], 105, 75, 300, 0),
            (4200, [F, F, F], 75, -45, 160, 1),
            (7100, [D, D, 0], 115, 30, 200, 255),
            (10000, [F, D, 0], 65, 90, 280, 1),
            (12900, [F, F, 0], 100, -55, 120, 255),
            (15900, [D, F, D], 85, 55, 180, 0),
            (19000, [F, D, 0], 110, -20, 260, 255),
            (22000, [D, D, 0], 70, 80, 200, 1),
            (25000, [F, F, F], 95, -35, 160, 255),
            (28000, [D, F, 0], 80, 20, 240, 0),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Ship Graveyard",
        info: STAGE_INFOS[14],
        ground: 0,
        waves: &[
            (1000, [F, D, 0], 80, -25, 220, 1),
            (3900, [D, D, 0], 110, 65, 180, 255),
            (6900, [F, F, F], 90, 10, 140, 0),
            (9900, [D, F, 0], 65, -55, 300, 1),
            (13000, [F, D, F], 100, 80, 160, 255),
            (16000, [D, D, 0], 75, -15, 220, 0),
            (19000, [F, F, 0], 115, 40, 180, 1),
            (22000, [D, F, 0], 95, -45, 260, 255),
            (25100, [F, D, F], 80, 70, 140, 1),
            (28200, [D, D, 0], 105, 0, 220, 255),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Orbital Shipyard",
        info: STAGE_INFOS[15],
        ground: 0,
        waves: &[
            (1200, [F, F, 0], 65, 70, 220, 1),
            (4100, [D, F, D], 95, -40, 160, 255),
            (7000, [F, D, 0], 110, 20, 280, 0),
            (10000, [D, D, 0], 75, 85, 200, 1),
            (13000, [F, F, F], 85, -50, 120, 255),
            (16000, [D, F, 0], 105, 45, 260, 0),
            (19000, [F, F, 0], 70, -15, 180, 255),
            (22000, [F, D, F], 100, 75, 160, 1),
            (25000, [D, D, 0], 115, -35, 220, 255),
            (28000, [F, D, 0], 80, 30, 240, 0),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Space Carrier",
        info: STAGE_INFOS[16],
        ground: 0,
        waves: &[
            (1100, [F, F, F], 90, 60, 140, 1),
            (3800, [D, F, 0], 110, -35, 220, 0),
            (6500, [F, D, F], 75, 85, 160, 255),
            (9300, [D, D, 0], 100, -45, 200, 1),
            (12100, [F, F, 0], 65, 20, 180, 0),
            (14900, [D, F, D], 95, 70, 160, 255),
            (17700, [F, D, 0], 110, -20, 220, 1),
        ],
        obstacle: 0,
    },
    StageDesign {
        name: "Jump Gate",
        info: STAGE_INFOS[17],
        ground: 0,
        waves: &[
            (1400, [D, F, 0], 105, -40, 240, 0),
            (4100, [F, F, F], 80, 75, 140, 1),
            (6900, [D, D, 0], 110, 15, 200, 255),
            (9700, [F, D, F], 90, -55, 160, 1),
            (12500, [D, F, 0], 65, 90, 220, 0),
            (15300, [F, F, F], 100, -25, 140, 255),
            (18100, [D, D, 0], 75, 45, 180, 1),
        ],
        obstacle: 0,
    },
];

// Explicit flight-volume obstacles: distance, x, y, destructible debris.
// Large obstacles and small debris alternate independently of enemy waves;
// none uses the terrestrial ground-turret authoring shortcut.
const SPACE_HAZARDS: [&[(u16, i16, i16, bool)]; 6] = [
    &[
        (2400, -70, 75, false),
        (5200, 80, -45, true),
        (8200, 15, 35, true),
        (11200, 95, 85, false),
        (14300, -100, -35, true),
        (17400, 65, 5, false),
        (20500, -45, 95, true),
        (23600, -100, -50, false),
        (26700, 55, 55, true),
    ],
    &[
        (2700, 95, 60, true),
        (5600, -75, -50, false),
        (8600, 40, 90, false),
        (11600, -100, 20, true),
        (14700, 85, -35, false),
        (17800, -35, 70, true),
        (20800, 100, 0, false),
        (23900, -90, 95, true),
        (27000, 15, -55, true),
    ],
    &[
        (2500, -95, -40, false),
        (5500, 70, 80, true),
        (8500, -25, 40, false),
        (11500, 105, -25, true),
        (14500, -80, 95, false),
        (17500, 50, 0, true),
        (20600, 100, 65, true),
        (23700, -65, -55, false),
        (26800, 20, 75, true),
    ],
    &[
        (2600, 90, 85, false),
        (5600, -95, -30, true),
        (8600, 35, 45, true),
        (11600, -65, 80, false),
        (14600, 110, -50, true),
        (17600, -100, 15, false),
        (20600, 70, 95, true),
        (23600, -30, -40, false),
        (26600, 100, 30, true),
    ],
    &[
        (2400, -95, 80, false),
        (5200, 75, -45, true),
        (8000, -30, 25, true),
        (10800, 100, 70, false),
        (13600, -90, -35, true),
        (16400, 60, 85, false),
    ],
    &[
        (2700, 100, -35, true),
        (5500, -80, 75, false),
        (8300, 30, 10, true),
        (11100, -100, -50, false),
        (13900, 85, 90, true),
        (16700, -45, 40, false),
    ],
];

pub fn stage_info(index: u8) -> StageInfo {
    CAMPAIGN[index as usize].info
}
#[cfg(test)]
pub fn stage_name(index: u8) -> &'static str {
    CAMPAIGN[index as usize].name
}

fn put16(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
pub fn common_bank() -> [u8; COMMON_SIZE] {
    let mut b = [0; COMMON_SIZE];
    b[..MODEL_SIZE].copy_from_slice(&model_bank(0));
    AssetBank::new(&b).expect("generated common bank");
    b
}

/// A complete resident model image, including the section's local enemy stats.
pub fn model_bank(level: u8) -> [u8; MODEL_SIZE] {
    assert!(level < SEGMENT_COUNT);
    let mut b = [0; MODEL_SIZE];
    b[..4].copy_from_slice(b"SMA6");
    b[4] = MODEL_COUNT;
    b[5] = level;
    let info = stage_info(level);
    let profile = info.enemy_profile;
    b[6] = if info.boss_kind != 0 { 2 } else { 3 };
    b[7] = info.boss_kind;
    let hp = [[3, 2, 6], [2, 3, 4], [4, 2, 6], [3, 2, 6]][profile as usize];
    let speed = [[8, 12, 8], [10, 8, 8], [6, 8, 8], [8, 14, 8]][profile as usize];
    // Keep the lowland gun and fortress bunker seated on their original bases.
    // The cliff battery and carrier-mounted turrets scale around their origins.
    let ground_pivot_y = if info.boss_kind == 0 && profile != 1 {
        -8
    } else {
        0
    };
    // Boss stages reserve mesh slot three for the boss side units, which keep
    // the boss enlargement; every other enemy slot bakes at the smaller scale.
    let enemy_scale = |id: u8, value: i16| {
        if id == CARRIER || (id == TURRET && info.boss_kind != 0) {
            value * BOSS_SCALE
        } else {
            enemy_scaled(value)
        }
    };
    for slot in 0..3 {
        let at = ENEMY_SPECS_OFFSET + slot * ENEMY_SPEC_SIZE;
        b[at] = hp[slot];
        let center_y = if slot == 2 && profile != 1 { 10 } else { 0 };
        let pivot_y = if slot == 2 { ground_pivot_y } else { 0 };
        let id = FIGHTER + slot as u8;
        b[at + 1] = (pivot_y + enemy_scale(id, center_y - pivot_y)) as u8;
        b[at + 2] = enemy_scale(id, 19) as u8;
        b[at + 3] = enemy_scale(id, 18) as u8;
        put16(&mut b, at + 4, if slot == 2 { 250 } else { 100 });
        b[at + 6] = speed[slot];
        b[at + 7] = u8::from(slot == 1 && profile != 2);
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
    for (slot, model) in section_enemy_models(profile).into_iter().enumerate() {
        models[slot + 1] = model;
    }
    customize_stage_models(info, &mut models);
    let medal = medal_model(&models[PLAYER as usize]);
    models.push(medal);
    finish_model_surfaces(info, &mut models);
    // Bake the larger combat meshes so the C64 retains identity transforms and
    // the existing model-bank size. Slot three also supplies the boss turrets.
    for id in FIGHTER..=CARRIER {
        let pivot_y = if id == TURRET { ground_pivot_y } else { 0 };
        for vertex in &mut models[id as usize].0 {
            vertex[0] = enemy_scale(id, vertex[0]);
            vertex[1] = pivot_y + enemy_scale(id, vertex[1] - pivot_y);
            vertex[2] = enemy_scale(id, vertex[2]);
        }
    }
    for (id, (vertices, faces)) in models.iter().enumerate() {
        let d = 16 + id * 8;
        let shift = mesh_shift(vertices);
        put16(&mut b, d, cursor as u16);
        b[d + 2] = vertices.len() as u8;
        b[d + 3] = faces.len() as u8;
        b[d + 6] = shift;
        // SMA6 renders every face with backface culling; byte seven is reserved.
        b[d + 7] = 0;
        for vertex in vertices {
            for v in vertex {
                b[cursor] = quantize_coordinate(*v, shift) as u8;
                cursor += 1;
            }
        }
        put16(&mut b, d + 4, cursor as u16);
        for face in faces {
            b[cursor..cursor + 4].copy_from_slice(face);
            cursor += 4;
        }
        let (family, heading) = match id as u8 {
            PLAYER => (1, 0),
            FIGHTER => (1, 2),
            DIVER => (3, 2),
            CARRIER => (2, 0),
            ROCK if info.theme >= 10 => (3, 0),
            _ => (0, 0),
        };
        if family != 0 {
            // Use the final quantized geometry, including enemy/boss scaling.
            let extent = |axis| {
                let mut min = i16::MAX;
                let mut max = i16::MIN;
                for vertex in vertices {
                    let value = (quantize_coordinate(vertex[axis], shift) as i16) << shift;
                    min = min.min(value);
                    max = max.max(value);
                }
                let center = (min + max) / 2;
                (center, (center - min).max(max - center) as u16)
            };
            let (x, half_x) = extent(0);
            let (z, half_z) = extent(2);
            let at = SHADOW_SPECS_OFFSET + id * SHADOW_SPEC_SIZE;
            b[at] = family;
            b[at + 1] = heading;
            put16(&mut b, at + 2, x as u16);
            put16(&mut b, at + 4, z as u16);
            put16(&mut b, at + 6, half_x);
            put16(&mut b, at + 8, half_z);
        }
    }
    assert!(
        cursor <= SHADOW_SPECS_OFFSET,
        "model bank overlaps shadow descriptors"
    );
    AssetBank::new(&b).expect("generated model bank");
    b
}

type Model = (Vec<[i16; 3]>, Vec<[u8; 4]>);

/// Finish the explicitly authored surfaces before coordinate quantization.
/// Thin wings and gun plates have real reverse-wound back faces; solid hulls
/// have closed outward surfaces. No rendering exception is stored in the bank.
fn finish_model_surfaces(info: StageInfo, models: &mut [Model]) {
    fn reverse(model: &mut Model) {
        for face in &mut model.1 {
            face.swap(1, 2);
        }
    }
    fn backs(model: &mut Model, faces: core::ops::Range<usize>) {
        for index in faces {
            let mut face = model.1[index];
            face.swap(1, 2);
            model.1.push(face);
        }
    }
    fn cap(model: &mut Model, color: u8) {
        model.1.extend([[0, 1, 2, color], [0, 2, 3, color]]);
    }

    // The medal was built from the original player winding above. Preserve its
    // established relief while correcting the flying ship's inward shell.
    reverse(&mut models[PLAYER as usize]);
    reverse(&mut models[CRYSTAL as usize]);

    if info.theme < 10 {
        let fighter = &mut models[FIGHTER as usize];
        match info.enemy_profile {
            1 => backs(fighter, 4..10),
            2 => {
                backs(fighter, 10..12);
                let color = fighter.1[0][3];
                cap(fighter, color);
            }
            3 => {
                let color = fighter.1[9][3];
                fighter.1.extend([[0, 7, 4, color], [3, 5, 7, color]]);
            }
            _ => {}
        }
        if info.enemy_profile == 1 {
            backs(&mut models[DIVER as usize], 2..4);
        }
        let rock = &mut models[ROCK as usize];
        let color = rock.1[5][3];
        cap(rock, color);
    }

    let turret = &mut models[TURRET as usize];
    if info.enemy_profile == 1 {
        // The wall battery's mounting plate is its rear (+Z) cap.
        reverse(turret);
    } else {
        backs(turret, 10..11);
    }
    let color = turret.1[2][3];
    cap(turret, color);

    let building = &mut models[BUILDING as usize];
    let color = building.1[2][3];
    cap(building, color);

    let carrier = &mut models[CARRIER as usize];
    match info.boss_kind {
        4 => {} // Four already closed, outward-facing Dreadnought boxes.
        5 => reverse(carrier),
        _ => {
            // Replace the internal front rectangle with the pointed nose's
            // complete surface; the nose and bottom originally faced inward.
            let color = carrier.1[1][3];
            for face in &mut carrier.1[12..17] {
                face.swap(1, 2);
            }
            carrier.1.drain(..2);
            carrier.1.push([4, 5, 9, color]);
        }
    }
}

/// Round half away from zero so mirrored vertices stay exactly symmetric.
pub fn quantize_coordinate(value: i16, shift: u8) -> i8 {
    let magnitude = value.unsigned_abs() as u32;
    let rounded = if shift == 0 {
        magnitude
    } else {
        (magnitude + (1 << (shift - 1))) >> shift
    };
    assert!(
        rounded <= 127,
        "coordinate {value} does not fit shift {shift}"
    );
    if value < 0 {
        -(rounded as i8)
    } else {
        rounded as i8
    }
}

/// The smallest shift under which every coordinate of the mesh fits an `i8`.
/// Integer meshes within ±127 keep shift 0 and are stored exactly.
pub fn mesh_shift(vertices: &[[i16; 3]]) -> u8 {
    let largest = vertices
        .iter()
        .flat_map(|v| v.iter().map(|c| c.unsigned_abs() as u32))
        .max()
        .unwrap_or(0);
    for shift in 0..=MAX_MESH_SHIFT {
        let rounded = if shift == 0 {
            largest
        } else {
            (largest + (1 << (shift - 1))) >> shift
        };
        if rounded <= 127 {
            return shift;
        }
    }
    panic!("mesh extent {largest} exceeds the largest supported shift");
}

fn customize_stage_models(info: StageInfo, models: &mut [Model]) {
    match info.theme {
        4 => {
            // Crystal skimmer: a sharp diamond keel, long flat outriggers and
            // glass-blue facets, using the patrol movement family.
            models[FIGHTER as usize].0 = vec![
                [0, 0, -42],
                [-36, -4, 4],
                [36, -4, 4],
                [0, 18, 10],
                [0, -12, 10],
                [0, 0, 32],
            ];
            for (i, face) in models[FIGHTER as usize].1.iter_mut().enumerate() {
                face[3] = [3, 1, 14, 6][i % 4];
            }
            // The cave's obstacles are translucent crystal spires. Their rock
            // collision identity stays unchanged; only the checked mesh differs.
            for vertex in &mut models[ROCK as usize].0 {
                vertex[0] = vertex[0] * 3 / 4;
                vertex[1] = vertex[1] * 3 / 2;
                vertex[2] = vertex[2] * 3 / 4;
            }
            for (i, face) in models[ROCK as usize].1.iter_mut().enumerate() {
                face[3] = [3, 1, 14, 6, 3, 14][i];
            }
        }
        5 => {
            // Volcanic diver: high swept fins and a narrow ember-colored nose.
            models[DIVER as usize].0 = vec![
                [0, -4, -43],
                [-25, 6, 18],
                [25, 6, 18],
                [0, 31, 24],
                [0, -13, 7],
            ];
            for (i, face) in models[DIVER as usize].1.iter_mut().enumerate() {
                face[3] = [2, 8, 9, 2, 10, 8][i];
            }
            for vertex in &mut models[ROCK as usize].0 {
                vertex[1] = vertex[1] * 3 / 2;
            }
            for (i, face) in models[ROCK as usize].1.iter_mut().enumerate() {
                face[3] = [11, 8, 0, 8, 11, 0][i];
            }
        }
        8 | 9 => {
            // Storm interceptor: split swept lightning wings surrounding a
            // needle fuselage; the carrier-guard steering family is retained.
            models[FIGHTER as usize].0 = vec![
                [-36, -3, -12],
                [-9, 0, -38],
                [9, 0, -38],
                [36, -3, -12],
                [-29, 8, 31],
                [29, 8, 31],
                [0, 16, 3],
                [0, -10, 9],
            ];
            for (i, face) in models[FIGHTER as usize].1.iter_mut().enumerate() {
                face[3] = if info.theme == 8 {
                    [14, 3, 1, 6][i % 4]
                } else {
                    [4, 14, 10, 6][i % 4]
                };
            }
        }
        10..=15 => customize_space_models(info.theme, models),
        _ => {}
    }
    match info.boss_kind {
        1 => {
            // Gorge Warden: wide tracked base, low sloping armor and an ochre
            // command cab. Side mounts stay at +/-65,15,-35 before scaling.
            models[CARRIER as usize].0 = vec![
                [-95, -24, -72],
                [95, -24, -72],
                [90, -24, 65],
                [-90, -24, 65],
                [-65, 15, -45],
                [65, 15, -45],
                [52, 15, 45],
                [-52, 15, 45],
                [0, 30, 18],
                [0, -4, -82],
            ];
            for (i, face) in models[CARRIER as usize].1.iter_mut().enumerate() {
                face[3] = [9, 8, 11, 12, 9, 8, 11, 12, 7, 8, 7, 8, 2, 10, 10, 11, 11][i];
            }
        }
        2 => {
            // Citadel Heart: squared reactor platform and a tall luminous core,
            // retaining the same collision hardpoints as the other bosses.
            models[CARRIER as usize].0 = vec![
                [-88, -20, -68],
                [88, -20, -68],
                [88, -20, 68],
                [-88, -20, 68],
                [-65, 15, -45],
                [65, 15, -45],
                [45, 15, 50],
                [-45, 15, 50],
                [0, 58, 8],
                [0, -2, -78],
            ];
            for (i, face) in models[CARRIER as usize].1.iter_mut().enumerate() {
                face[3] = [
                    11, 12, 11, 12, 11, 12, 11, 12, 3, 14, 3, 1, 4, 14, 14, 11, 11,
                ][i];
            }
            // Two faceted reactor pylons replace the usual gun turrets.
            models[TURRET as usize].0 = vec![
                [-17, -8, -17],
                [17, -8, -17],
                [17, -8, 17],
                [-17, -8, 17],
                [-10, 24, -10],
                [10, 24, -10],
                [10, 24, 10],
                [-10, 24, 10],
                [0, 34, -17],
            ];
            for (i, face) in models[TURRET as usize].1.iter_mut().enumerate() {
                face[3] = [11, 14, 11, 3, 11, 14, 11, 3, 1, 3, 14][i];
            }
        }
        4 => {
            // Dreadnought: a long armored flight deck, paired engine nacelles
            // and raised bridge. Existing +/-65,15,-35 hardpoints sit on deck.
            let mut carrier = (Vec::new(), Vec::new());
            model_box(
                &mut carrier,
                [-105, -25, -220],
                [105, 15, 170],
                [6, 14, 15],
            );
            model_box(&mut carrier, [-28, 15, 15], [28, 60, 105], [6, 14, 3]);
            model_box(&mut carrier, [-165, -15, -95], [-105, 25, 205], [6, 14, 8]);
            model_box(&mut carrier, [105, -15, -95], [165, 25, 205], [6, 14, 8]);
            models[CARRIER as usize] = carrier;
            for (i, face) in models[TURRET as usize].1.iter_mut().enumerate() {
                face[3] = [11, 15, 6, 14][i % 4];
            }
        }
        5 => {
            // Gate Guardian: a closed, deep octagonal ring with an actual open
            // aperture. Side reactors and the vulnerable core retain their
            // established world-space collision coordinates.
            let outer = [
                [-140, -55],
                [-70, -110],
                [70, -110],
                [140, -55],
                [140, 55],
                [70, 110],
                [-70, 110],
                [-140, 55],
            ];
            let inner = [
                [-55, -25],
                [-28, -50],
                [28, -50],
                [55, -25],
                [55, 25],
                [28, 50],
                [-28, 50],
                [-55, 25],
            ];
            let mut gate = (Vec::new(), Vec::new());
            for z in [-35, 35] {
                for ring in [outer, inner] {
                    gate.0.extend(ring.map(|[x, y]| [x, y, z]));
                }
            }
            for i in 0..8u8 {
                let next = (i + 1) & 7;
                for (a, b, c, d, color) in [
                    (i, next, next + 8, i + 8, [14, 3, 6, 1][i as usize % 4]),
                    (i + 16, i + 24, next + 24, next + 16, 11),
                    (i, i + 16, next + 16, next, 12),
                    (i + 8, next + 8, next + 24, i + 24, 4),
                ] {
                    gate.1.push([a, b, c, color]);
                    gate.1.push([a, c, d, color]);
                }
            }
            models[CARRIER as usize] = gate;
            // The final stage never spawns crystals. Its spare slot holds a
            // compact child Guardian, leaving four hulls and every projectile
            // inside the foreground command budget. Half size supplies the
            // final phase without another mesh or a new transform interface.
            let mut child = (Vec::new(), Vec::new());
            for ring in [outer, inner] {
                child.0.extend(ring.map(|[x, y]| [x / 2, y / 2, 0]));
            }
            for i in 0..8u8 {
                let next = (i + 1) & 7;
                let color = [14, 3, 6, 1][i as usize % 4];
                for [a, b, c] in [[i, next, next + 8], [i, next + 8, i + 8]] {
                    child.1.push([a, b, c, color]);
                    child.1.push([a, c, b, color]);
                }
            }
            models[CRYSTAL as usize] = child;
            for (i, face) in models[TURRET as usize].1.iter_mut().enumerate() {
                face[3] = [4, 14, 3, 1][i % 4];
            }
        }
        _ => {}
    }
}

fn model_box(model: &mut Model, min: [i16; 3], max: [i16; 3], colors: [u8; 3]) {
    let base = model.0.len() as u8;
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    model.0.extend([
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y0, z1],
        [x0, y0, z1],
        [x0, y1, z0],
        [x1, y1, z0],
        [x1, y1, z1],
        [x0, y1, z1],
    ]);
    for (i, face) in [
        [0, 4, 1],
        [1, 4, 5],
        [1, 5, 2],
        [2, 5, 6],
        [2, 6, 3],
        [3, 6, 7],
        [3, 7, 0],
        [0, 7, 4],
        [4, 7, 6],
        [4, 6, 5],
        [0, 1, 2],
        [0, 2, 3],
    ]
    .into_iter()
    .enumerate()
    {
        model.1.push([
            base + face[0],
            base + face[1],
            base + face[2],
            colors[i % 3],
        ]);
    }
}

fn flight_mesh(vertices: [[i16; 3]; 6], colors: [u8; 4]) -> Model {
    let faces = [
        [0, 1, 3],
        [0, 3, 2],
        [0, 4, 1],
        [0, 2, 4],
        [1, 5, 3],
        [3, 5, 2],
        [1, 4, 5],
        [2, 5, 4],
    ];
    (
        vertices.to_vec(),
        faces
            .into_iter()
            .enumerate()
            .map(|(i, face)| [face[0], face[1], face[2], colors[i % 4]])
            .collect(),
    )
}

/// Bake a chipped, uneven rock with closed facets and fixed mineral shading.
/// The extrema stay exact so tumbling, fragment scaling and shadow bounds keep
/// the original obstacle dimensions. Relief and lighting are baked on the host.
fn asteroid_mesh(original: [[i16; 3]; 6], variant: usize) -> Model {
    // The runtime's coarse mesh uses this original vertex prefix and the same
    // eight-face connectivity as flight_mesh. Close rocks raise six uneven
    // face centers, replacing each of those faces with three chipped facets.
    let (mut vertices, coarse) = flight_mesh(original, [12, 15, 11, 11]);
    let mut facets = Vec::new();
    for (index, [a, b, c, _]) in coarse.into_iter().enumerate() {
        if index >= 6 {
            facets.push([a, b, c]);
            continue;
        }
        let variation = ((variant + index) % 3) as i32;
        let weights = [3 + variation, 4, 5 - variation];
        let corners = [a, b, c].map(|i| original[i as usize]);
        let center = core::array::from_fn(|axis| {
            let weighted = (0..3)
                .map(|i| i32::from(corners[i][axis]) * weights[i])
                .sum::<i32>();
            (weighted * (10 + variation) / 96) as i16
        });
        let raised = vertices.len() as u8;
        vertices.push(center);
        facets.extend([[a, b, raised], [b, c, raised], [c, a, raised]]);
    }
    let faces = facets
        .into_iter()
        .map(|face| {
            let [a, b, c] = face.map(|i| vertices[i as usize].map(i32::from));
            let ab: [i32; 3] = core::array::from_fn(|axis| b[axis] - a[axis]);
            let ac: [i32; 3] = core::array::from_fn(|axis| c[axis] - a[axis]);
            let normal = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let light = -normal[0] + 2 * normal[1] - normal[2];
            let magnitude = normal.iter().map(|n| n.abs()).sum::<i32>();
            let color = if light <= 0 {
                11
            } else if light < magnitude {
                12
            } else {
                15
            };
            [face[0], face[1], face[2], color]
        })
        .collect();
    (vertices, faces)
}

fn customize_space_models(theme: u8, models: &mut [Model]) {
    // Different silhouettes, not palette swaps: fork-wing interceptors, lunar
    // blades, salvaged drones, yard tenders, deck guards and gate sentinels.
    let fighters = [
        [
            [0, 0, -46],
            [-40, -4, 14],
            [40, -4, 14],
            [0, 11, 8],
            [0, -10, 20],
            [0, 0, 32],
        ],
        [
            [0, 4, -32],
            [-18, 0, -16],
            [18, 0, -16],
            [0, 35, 24],
            [0, -26, 24],
            [0, 0, 42],
        ],
        [
            [-8, 0, -40],
            [-42, 8, 20],
            [27, -5, 32],
            [4, 24, -4],
            [-5, -16, 6],
            [0, 0, 20],
        ],
        [
            [0, 0, -22],
            [-34, -8, -12],
            [34, -8, -12],
            [0, 22, 16],
            [0, -14, 16],
            [0, 0, 44],
        ],
        [
            [0, -3, -52],
            [-44, 2, 28],
            [44, 2, 28],
            [0, 18, 5],
            [0, -13, 5],
            [0, 4, 38],
        ],
        [
            [0, 0, -40],
            [-30, 12, 0],
            [30, 12, 0],
            [0, 30, 18],
            [0, -30, 18],
            [0, 0, 40],
        ],
    ];
    let divers = [
        [
            [0, 0, -42],
            [-25, -9, 22],
            [25, -9, 22],
            [0, 24, 7],
            [0, -16, 7],
            [0, 0, 31],
        ],
        [
            [0, -5, -50],
            [-36, 3, 10],
            [36, 3, 10],
            [0, 15, 30],
            [0, -15, 30],
            [0, 0, 38],
        ],
        [
            [0, 8, -25],
            [-24, -8, 18],
            [34, 0, 10],
            [-8, 28, 5],
            [8, -20, 5],
            [0, 0, 36],
        ],
        [
            [0, 0, -38],
            [-16, 10, 26],
            [16, 10, 26],
            [0, 32, 10],
            [0, -24, 10],
            [0, 0, 46],
        ],
        [
            [0, 0, -48],
            [-28, -3, 26],
            [28, -3, 26],
            [0, 20, -8],
            [0, -20, -8],
            [0, 0, 30],
        ],
        [
            [0, 0, -46],
            [-38, 0, 8],
            [38, 0, 8],
            [0, 26, 8],
            [0, -26, 8],
            [0, 0, 34],
        ],
    ];
    let palettes = [
        [8, 15, 9, 12],
        [15, 14, 11, 1],
        [9, 12, 8, 11],
        [7, 15, 8, 12],
        [6, 14, 15, 3],
        [4, 3, 14, 1],
    ];
    let index = theme as usize - 10;
    models[FIGHTER as usize] = flight_mesh(fighters[index], palettes[index]);
    models[DIVER as usize] = flight_mesh(divers[index], palettes[(index + 2) % 6]);
    // Center every orbital obstacle around its origin: the runtime tumbles
    // these and scales only the flagged small fragments around that center.
    let rocks = [
        [
            [0, 0, -35],
            [-38, -5, 0],
            [31, 5, 0],
            [0, 37, 0],
            [0, -37, 0],
            [0, 0, 35],
        ],
        [
            [0, 0, -27],
            [-34, 0, 0],
            [34, 0, 0],
            [0, 46, 4],
            [0, -46, -4],
            [0, 0, 27],
        ],
        [
            [-8, 0, -43],
            [-45, 0, -12],
            [45, 0, 12],
            [0, 19, 5],
            [0, -19, -5],
            [8, 0, 43],
        ],
        [
            [0, 0, -38],
            [-28, 0, 0],
            [28, 0, 0],
            [0, 29, 0],
            [0, -29, 0],
            [0, 0, 38],
        ],
        [
            [0, 0, -45],
            [-35, 0, -10],
            [35, 0, 10],
            [0, 22, 0],
            [0, -22, 0],
            [0, 0, 45],
        ],
        [
            [0, 0, -32],
            [-40, 0, 0],
            [40, 0, 0],
            [0, 40, 0],
            [0, -40, 0],
            [0, 0, 32],
        ],
    ];
    models[ROCK as usize] = asteroid_mesh(rocks[index], index);
}

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
        // The source player shell is still inward here, before surface finishing.
        // Reverse its upper winding for the medal's outward cockpit relief.
        faces.push([
            top_index[a as usize],
            top_index[c as usize],
            top_index[b as usize],
            color,
        ]);
    }
    // A bronze canopy stands above the original gold cockpit ridge. Separate
    // nozzles and short exhaust wedges make the twin engines readable on the
    // coin; the flying PLAYER's exhaust is procedural rather than mesh data.
    vertices.extend_from_slice(&[
        [-6, 12, -30],
        [6, 12, -30],
        [6, -10, -31],
        [-6, -10, -31],
        [0, 15, -34],
        [0, -9, -35],
        [-16, -21, -27],
        [-6, -21, -27],
        [-6, -31, -29],
        [-16, -31, -29],
        [-11, -23, -31],
        [-11, -40, -25],
        [-11, -40, -14],
        [6, -21, -27],
        [16, -21, -27],
        [16, -31, -29],
        [6, -31, -29],
        [11, -23, -31],
        [11, -40, -25],
        [11, -40, -14],
    ]);
    faces.extend_from_slice(&[
        // Closed bronze canopy with a small gold nose facet.
        [50, 54, 53, 9],
        [53, 54, 55, 9],
        [51, 52, 54, 9],
        [52, 55, 54, 9],
        [50, 51, 54, 7],
        [53, 55, 52, 8],
        [50, 52, 51, 8],
        [50, 53, 52, 8],
        // Left nozzle and its closed, short gold exhaust wedge.
        [56, 57, 60, 7],
        [57, 58, 60, 8],
        [58, 59, 60, 9],
        [59, 56, 60, 7],
        [56, 58, 57, 8],
        [56, 59, 58, 8],
        [58, 61, 59, 7],
        [58, 62, 61, 8],
        [59, 61, 62, 9],
        [58, 59, 62, 8],
        // Right nozzle and exhaust, separated by the original aft fin.
        [63, 64, 67, 7],
        [64, 65, 67, 8],
        [65, 66, 67, 9],
        [66, 63, 67, 7],
        [63, 65, 64, 8],
        [63, 66, 65, 8],
        [65, 68, 66, 7],
        [65, 69, 68, 8],
        [66, 68, 69, 9],
        [65, 66, 69, 8],
    ]);
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
    directory[..4].copy_from_slice(b"MDL2");
    directory[4] = SEGMENT_COUNT;
    directory[5] = 2;
    put16(&mut directory, 6, MODEL_SIZE as u16);
    put32(&mut directory, 8, MODEL_BANKS_OFFSET);
    for level in 0..SEGMENT_COUNT {
        put16(
            &mut directory,
            12 + level as usize * 2,
            checksum(&model_bank(level)),
        );
    }
    let sum = checksum(&directory[..MODEL_DIRECTORY_CHECKSUM_OFFSET]);
    put16(&mut directory, MODEL_DIRECTORY_CHECKSUM_OFFSET, sum);
    directory
}

/// Author the complete lateral movement envelope outside the visible near
/// plane. Use the final quantized meshes, every rendered roll and either LOD
/// silhouette; collision radii do not describe these hulls.
fn flying_lane_limits(index: u8) -> [(i16, i16); 2] {
    let models = model_bank(index);
    let bank = AssetBank::new(&models).expect("generated models");
    let profile = stage_info(index).enemy_profile;
    let mut lanes = [(0, i16::MAX); 2];
    for kind in [FIGHTER, DIVER] {
        // The cave proxy uses the unrotated mesh's own silhouette. Other
        // stages can fall back to a diamond even at close range.
        let mut half_x = if index == 1 {
            0
        } else {
            enemy_scaled(if kind == FIGHTER { 22 } else { 18 })
        };
        let phases = if bank.enemy(kind).render_mode == 1 {
            128
        } else {
            1
        };
        for phase in 0..phases {
            let rotation = havoc::math::RotationMatrix::new(0, 0, (phase * 2) as u8);
            for vertex in 0..bank.vertex_count(kind) {
                let point = rotation.apply(bank.vertex_bytes(kind, vertex), 8 - bank.shift(kind));
                half_x = half_x.max(point.x.abs());
            }
        }
        // Projection has equal horizontal focal length and viewport half-width,
        // so the visible near-plane rectangle extends to +/-NEAR in world X.
        let safe_center = half_x + havoc::render::NEAR + 1;
        let limits = if profile == 1 && kind == FIGHTER {
            // The skimmer finishes its 220-unit crossing at age 110, while its
            // center is still beyond z=629 even for a late admission. Keep its
            // existing crossing and bound the settled opposite-side lane.
            (17, 220 - safe_center - 16)
        } else {
            let travel = match (profile, kind) {
                (0, FIGHTER) => 64,
                (2, DIVER) => 0, // The sentry returns before its firing hold.
                _ => 32,         // Harmonic motion or the pursuer's bounded aim lane.
            };
            (safe_center + travel + 16, i16::MAX)
        };
        assert!(
            limits.0 <= 180 && limits.0 <= limits.1,
            "enemy lane exceeds flight bounds"
        );
        lanes[(kind - FIGHTER) as usize] = limits;
    }
    lanes
}

fn flying_lane_x(lanes: &[(i16, i16); 2], kind: u8, x: i16, wave: usize) -> i16 {
    if !matches!(kind, FIGHTER | DIVER) {
        return x;
    }
    let (min, max) = lanes[(kind - FIGHTER) as usize];
    let side = if x < 0 || (x == 0 && wave & 1 == 0) {
        -1
    } else {
        1
    };
    side * x.abs().clamp(min, max)
}

pub fn segment_bank(index: u8) -> [u8; SEGMENT_SIZE] {
    let design = &CAMPAIGN[index as usize];
    let info = design.info;
    let lanes = flying_lane_limits(index);
    let mut b = [0; SEGMENT_SIZE];
    b[..4].copy_from_slice(b"SMS2");
    b[4] = index;
    b[5] = info.enemy_profile;
    b[6] = design.ground;
    b[8..16].copy_from_slice(&[
        info.enemy_profile,
        info.music_cue,
        info.bridge_profile,
        info.boss_kind,
        info.theme,
        info.next_theme,
        info.previous_theme,
        info.reserved,
    ]);
    let mut events: Vec<Spawn> = Vec::new();
    if index == 0 {
        for wave in 0..10u16 {
            let base = 1200 + wave * 2700;
            for lane in 0..3i16 {
                let kind = if wave % 3 == 2 { DIVER } else { FIGHTER };
                events.push(Spawn {
                    distance: base + lane as u16 * 100,
                    kind,
                    x: flying_lane_x(&lanes, kind, (lane - 1) * 80, wave as usize),
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
    } else {
        for (wave, &(base, kinds, width, height, delay, reward)) in design.waves.iter().enumerate()
        {
            let count = kinds.iter().filter(|&&kind| kind != 0).count();
            for (lane, &kind) in kinds[..count].iter().enumerate() {
                let x = if count == 3 {
                    (lane as i16 - 1) * width
                } else if lane == 0 {
                    -width
                } else {
                    width
                };
                events.push(Spawn {
                    distance: base + lane as u16 * delay,
                    kind,
                    x: flying_lane_x(&lanes, kind, x, wave),
                    y: height,
                    flags: u8::from(lane as u8 == reward),
                });
            }
            if index < 12 && info.boss_kind == 0 && (info.enemy_profile != 0 || wave % 2 == 1) {
                events.push(Spawn {
                    distance: base + if info.enemy_profile == 1 { 800 } else { 1000 },
                    kind: T,
                    x: if wave % 2 == 0 { -90 } else { 90 },
                    y: if info.enemy_profile == 1 { 0 } else { -90 },
                    flags: 0,
                });
            }
        }
    }
    if design.obstacle != 0 {
        for i in 0..6u16 {
            events.push(Spawn {
                distance: 2200 + i * 4300 + u16::from(index != 0) * index as u16 * 40,
                kind: design.obstacle,
                x: if (i + index as u16) % 2 == 0 { -70 } else { 70 },
                y: -110,
                flags: 0,
            });
        }
    }
    if index >= 12 {
        for &(distance, x, y, destructible) in SPACE_HAZARDS[index as usize - 12] {
            events.push(Spawn {
                distance,
                kind: ROCK,
                x,
                y,
                flags: if destructible { SMALL_DEBRIS } else { 0 },
            });
        }
    }
    assert!(
        events.len() <= MAX_SEGMENT_SPAWNS as usize,
        "encounters exceed the runtime segment cache"
    );
    // Reserve room for the runtime's delayed second enemy wave, including the
    // late ground turrets. Scenery and supplies keep their authored timing.
    for event in &mut events {
        if matches!(event.kind, FIGHTER | DIVER | TURRET) {
            event.distance = (u32::from(event.distance) * 19 / 20) as u16;
        }
    }
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
    reu[17] = audio::SID_COUNT as u8;
    reu[MODEL_DIRECTORY_OFFSET as usize..MODEL_DIRECTORY_OFFSET as usize + MODEL_DIRECTORY_SIZE]
        .copy_from_slice(&model_directory());
    put16(&mut reu, 18, checksum(&common));
    reu[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE].copy_from_slice(&common);
    let directory = digital::catalog_directory();
    let catalog_start = digital::MUSIC_OFFSET as usize;
    reu[catalog_start..catalog_start + directory.len()].copy_from_slice(directory);
    for (track, bytes) in DIGITAL_TRACKS {
        digital::validate_asset(bytes, track).map_err(|e| format!("digital {track:?}: {e:?}"))?;
        let descriptor = track.descriptor();
        let start = descriptor.asset_offset as usize;
        if start + bytes.len() > DIGITAL_RING_OFFSET as usize {
            return Err(format!("digital {track:?} exceeds the soundtrack budget").into());
        }
        reu[start..start + bytes.len()].copy_from_slice(bytes);
    }
    for i in 0..SEGMENT_COUNT {
        let model = model_bank(i);
        let model_start = MODEL_BANKS_OFFSET as usize + i as usize * MODEL_SIZE;
        reu[model_start..model_start + MODEL_SIZE].copy_from_slice(&model);
        let segment = segment_bank(i);
        let p = SEGMENTS_OFFSET as usize + i as usize * SEGMENT_SIZE;
        reu[p..p + SEGMENT_SIZE].copy_from_slice(&segment);
        put16(
            &mut reu,
            SEGMENT_CHECKSUM_OFFSET + i as usize * 2,
            checksum(&segment[..SEGMENT_CACHE_SIZE]),
        );
    }
    let check = checksum(&reu[..PACKAGE_CHECKSUM_OFFSET]);
    put16(&mut reu, PACKAGE_CHECKSUM_OFFSET, check);
    check_package(&reu)?;
    fs::write(out.join("havoc.reu"), &reu)?;
    fs::write(out.join("havoc.cfg"), include_str!("../config/havoc.cfg"))?;
    let mut report = format!("H.A.V.O.C. package {PACKAGE_ID:08X}\n16 MiB REU; common {COMMON_OFFSET:06X} (3072 bytes; 2048 resident model bytes); eighteen checked 2048-byte stage model banks at {MODEL_BANKS_OFFSET:06X}; 64-byte model directory at {MODEL_DIRECTORY_OFFSET:06X}; eighteen 2048-byte segments at {SEGMENTS_OFFSET:06X}, each with a checksummed 512-byte runtime prefix (up to 62 encounters)\nEighteen 75-second stages, seventeen 8-second crossings: 24:46 scheduled gameplay plus victory reveal. Five bosses: Gorge Warden, Citadel Heart, Carrier, Dreadnought, Gate Guardian.\nFour enemy movement families with distinct terrestrial and orbital meshes; authored three-dimensional debris fields and an orbital ascent.\nFour SIDs: twelve effects oscillators, four independent filters, 21 effects; DAC-only music.\nDLT4 manifest soundtrack at $100000: 21 full-length recordings: 20 kHz stereo Hammond Rocker; 16 kHz mono Galactic Dash, Mission Failed, and all eighteen stage songs, with a 10 ms right-channel delay; two mutable 64 KiB PCM slots at $FE0000.\n");
    for (index, design) in CAMPAIGN.iter().enumerate() {
        report.push_str(&format!(
            "{:02}. {}: theme {}, enemy profile {}, boss {}, music {}\n",
            index + 1,
            design.name,
            design.info.theme,
            design.info.enemy_profile,
            design.info.boss_kind,
            design.info.music_cue
        ));
    }
    fs::write(out.join("assets.txt"), report)?;
    println!(
        "Baked {} ({} bytes)",
        out.join("havoc.reu").display(),
        reu.len()
    );
    Ok(())
}
pub fn check_package(reu: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if reu.len() != REU_SIZE as usize {
        return Err(format!("REU must contain exactly {REU_SIZE} bytes (16 MiB)").into());
    }
    validate_header(reu).map_err(|e| format!("package header: {e:?}"))?;
    digital::validate_catalog(reu).map_err(|e| format!("digital catalog: {e:?}"))?;
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
    if common[MODEL_SIZE..].iter().any(|&b| b != 0) {
        return Err("nonzero retired score padding".into());
    }
    if checksum(common) != u16_at(reu, 18) {
        return Err("common checksum".into());
    }
    for i in 0..SEGMENT_COUNT as usize {
        let p = SEGMENTS_OFFSET as usize + i * SEGMENT_SIZE;
        let data = &reu[p..p + SEGMENT_SIZE];
        let s = Segment::new(data).map_err(|e| format!("segment {i}: {e:?}"))?;
        if s.index() != i as u8
            || checksum(&data[..SEGMENT_CACHE_SIZE]) != u16_at(reu, SEGMENT_CHECKSUM_OFFSET + i * 2)
        {
            return Err("segment checksum/index".into());
        }
        let model = &reu[MODEL_BANKS_OFFSET as usize + i * MODEL_SIZE..];
        if model[7] != s.stage_info().boss_kind {
            return Err("stage/model boss identity mismatch".into());
        }
        if i > 0 && s.stage_info().previous_theme != CAMPAIGN[i - 1].info.theme
            || i + 1 < SEGMENT_COUNT as usize
                && s.stage_info().next_theme != CAMPAIGN[i + 1].info.theme
        {
            return Err("stage theme continuity".into());
        }
    }
    Ok(())
}
