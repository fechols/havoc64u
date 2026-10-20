//! World-anchored scenery, emitted before projection by the SCENERY overlay.
//! Decorative crossings deliberately have no gameplay objects or collision state.
use crate::assets::{Segment, StageInfo};
use crate::game::STATIC_SKY;
use crate::{GeometrySink, Vec3, EXTERIOR_TERRAIN, TWO_SIDED};
mod space;

/// Native scenery always writes the renderer's bounded staging buffer. Keeping
/// that type concrete also keeps the opaque tail ABI to two thin references.
/// Host captures retain the same geometry API through their trait object.
#[cfg(target_arch = "mos")]
pub type SceneSink<'a> = crate::render::SceneryStage<'a>;
#[cfg(not(target_arch = "mos"))]
pub type SceneSink<'a> = dyn GeometrySink + 'a;

pub(crate) const BACKDROP_CAMERA_Y: i16 = 40;
#[cfg_attr(target_arch = "mos", link_section = ".tables.scenery_data")]
pub(crate) static SKY_COLORS: [u8; 10] = [6, 8, 11, 4, 0, 11, 8, 11, 6, 4];

/// Frame setup selects this color while the SCENERY overlay is loaded.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub fn sky_color(theme: u8, transition: Option<(u8, u16)>) -> u8 {
    if theme >= 10 {
        return 0;
    }
    if let Some((next, elapsed)) = transition {
        if next >= 10 {
            return if elapsed < 80 {
                SKY_COLORS[theme as usize]
            } else if elapsed < 180 {
                6
            } else {
                0
            };
        }
    }
    let from = SKY_COLORS[theme as usize];
    if let Some((next_theme, elapsed)) = transition {
        let to = SKY_COLORS[next_theme as usize];
        if from == to || elapsed < 100 {
            from
        } else if elapsed < 200 {
            14
        } else if elapsed < 300 && next_theme != 4 {
            12
        } else {
            // The cave facets switch on at the midpoint. Darken the opening
            // with them, keeping its overlapping roof/crystals to three inks.
            to
        }
    } else {
        from
    }
}

#[test]
fn cave_entry_darkens_with_its_midpoint_facets() {
    assert_eq!(sky_color(0, Some((4, 0))), 6);
    assert_eq!(sky_color(0, Some((4, 100))), 14);
    assert_eq!(sky_color(0, Some((4, 199))), 14);
    assert_eq!(sky_color(0, Some((4, 200))), 0);
    assert_eq!(sky_color(0, Some((4, 399))), 0);
    assert_eq!(sky_color(4, Some((1, 200))), 12);
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
    TERRAIN_PALETTES[style as usize]
}

// These small, immutable lookups use resident table space instead of being
// transferred with every scenery frame. The linker checks their shared budget.
#[cfg_attr(target_arch = "mos", link_section = ".tables.scenery_data")]
static TERRAIN_PALETTES: [u16; 10] = [
    0x0d05, 0x0c09, 0x0c0b, 0x0c0b, 0x040b, 0x0b08, 0x0c09, 0x0f0c, 0x0e0b, 0x0a04,
];
#[cfg_attr(target_arch = "mos", link_section = ".tables.scenery_data")]
static GROUND_COLORS: [u8; 10] = [5, 9, 11, 6, 11, 11, 9, 12, 11, 4];
#[cfg_attr(target_arch = "mos", link_section = ".tables.scenery_data")]
static GROUND_SHADES: [u8; 10] = [13, 11, 11, 14, 4, 8, 11, 15, 14, 10];
static PROFILE: [u8; 16] = [0, 1, 3, 2, 1, 0, 2, 4, 3, 1, 0, 1, 2, 4, 2, 1];
#[cfg_attr(target_arch = "mos", link_section = ".tables.scenery_data")]
static HEIGHT_BASE: [i16; 10] = [-105, 96, -105, -105, -70, -80, -60, -40, -90, -100];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
static HEIGHT_RISE: [u8; 50] = [
    0, 32, 64, 96, 128, // lowland
    0, 16, 32, 48, 64, // canyon
    0, 16, 32, 48, 64, // fortress
    0, 16, 32, 48, 64, // coast
    0, 48, 96, 144, 192, // crystal
    0, 48, 96, 144, 192, // ash
    0, 24, 48, 72, 96, // foundry
    0, 32, 64, 96, 128, // citadel
    0, 40, 80, 120, 160, // storm
    0, 48, 96, 144, 192, // meteor
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub(crate) fn terrain_height(row: u16, right: bool, style: u8) -> i16 {
    let phase = row.wrapping_add(if right { 7 } else { 0 });
    let height = PROFILE[(phase & 15) as usize];
    // The five possible rises fit in a byte. Sampling a compact table keeps
    // the native path identical for every theme without per-sample branches.
    let base = HEIGHT_BASE[style as usize];
    let rise = HEIGHT_RISE[(style * 5 + height) as usize];
    base + rise as i16
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn row_at(row: u16, start: u16, info: &StageInfo, legacy: bool) -> Row {
    let tick = row * 64;
    let mut from = info.theme;
    let mut to = from;
    let mut amount = 0;
    if !legacy {
        if start > 0 && tick < start {
            from = info.previous_theme;
            to = info.theme;
            amount = weight(tick.saturating_sub(start - 400));
        } else if info.next_theme < 10 && tick >= start + 3750 {
            to = info.next_theme;
            amount = weight(tick - start - 3750);
        }
    }
    let lane = |theme| if !legacy && theme == 3 { 320 } else { 145 };
    let edge = |theme| if theme == 1 { 190 } else { 640 };
    // Most frames contain one terrain style. Avoid duplicate samples and blends.
    if from == to {
        return Row {
            left: terrain_height(row, false, from),
            right: terrain_height(row, true, from),
            edge: edge(from),
            lane: lane(from),
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
        edge: blend(edge(from), edge(to), amount),
        lane: blend(lane(from), lane(to), amount),
        style: if amount < 128 { from } else { to },
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn triangle(sink: &mut SceneSink<'_>, points: [Vec3; 3], color: u8, camera_y: i16) {
    let points = points.map(|mut point| {
        point.y = point.y.saturating_sub(camera_y);
        point
    });
    // This private helper packs the exterior tag above the palette nibble to
    // keep its native calling convention compact. Public emissions stay separate.
    sink.triangle(
        points[0],
        points[1],
        points[2],
        color & 15,
        TWO_SIDED | (color & EXTERIOR_TERRAIN),
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn strip(
    sink: &mut SceneSink<'_>,
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

// Keep both sides in one native body. Inlining this sizeable strip setup into
// the two-element side loop duplicates its branch and argument-marshalling code.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn emit_side(
    sink: &mut SceneSink<'_>,
    first: &Row,
    last: &Row,
    right: bool,
    z: [i16; 2],
    facets: [u8; 2],
    camera_y: i16,
) {
    let heights = if right {
        [first.right, last.right]
    } else {
        [first.left, last.left]
    };
    let mut edges = [first.edge, last.edge];
    let mut lanes = [first.lane, last.lane];
    if !right {
        edges[0] = -edges[0];
        edges[1] = -edges[1];
        lanes[0] = -lanes[0];
        lanes[1] = -lanes[1];
    }
    strip(
        sink,
        lanes,
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
            [if right { 640 } else { -640 }; 2],
            heights,
            z,
            facets,
            camera_y,
            EXTERIOR_TERRAIN,
        );
    } else if matches!(first.style, 2 | 6 | 7) && first.style == last.style {
        // World-anchored runway ends remain outside the canyon caps, so both
        // details never consume the same face reservation.
        strip(
            sink,
            [if right { 95 } else { -95 }; 2],
            [-103; 2],
            [if right { 100 } else { -100 }; 2],
            [-103; 2],
            z,
            [15; 2],
            camera_y,
            EXTERIOR_TERRAIN,
        );
    }
}

/// Emit camera-space triangles. Legacy mode preserves title and attract terrain.
/// Gameplay samples every terrain edge from its absolute world row, including
/// incoming land ahead of a bridge and the terrain left behind its far end.
#[inline(always)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub fn emit(
    sink: &mut SceneSink<'_>,
    segment: &Segment<'_>,
    scene_ticks: u16,
    section: u8,
    transition: Option<(u8, u16)>,
    camera_y: i16,
    legacy: bool,
) {
    let context = Decoration {
        scene_ticks,
        elapsed: transition.map_or(400, |(_, tick)| tick),
        camera_y,
        stage: section,
        info: segment.stage_info(),
        ground_color: segment.ground_color(),
        legacy,
    };
    if context.info.theme >= 10 || (transition.is_some() && context.info.bridge_profile == 3) {
        decoration(sink, &context);
        return;
    }
    land(sink, &context, 0);
    if transition.is_some() {
        decoration(sink, &context);
    }
}

/// Move the clock by complete terrain periods. Heights repeat every sixteen
/// rows and alternating floor shades every two, so this preserves old pixels.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn stage_origin(section: u8, legacy: bool) -> u16 {
    if section == 0 {
        0
    } else {
        1024 + (section as u16 * if legacy { 678 } else { 54 } & 1023)
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn land(sink: &mut SceneSink<'_>, context: &Decoration, drop: i16) {
    let Decoration {
        scene_ticks: local,
        elapsed,
        camera_y,
        stage: section,
        info,
        ground_color,
        legacy,
    } = *context;
    let stage_start = stage_origin(section, legacy);
    let scene_ticks = stage_start + local;
    let camera_y = camera_y + drop;
    let transition = elapsed < 400 && info.next_theme < 10;
    let row = scene_ticks >> 6;
    let offset = ((scene_ticks & 63) * 8) as i16;
    let mut first = row_at(row, stage_start, &info, legacy);
    for band in 0..4i16 {
        let world_row = row + band as u16;
        let last = row_at(world_row + 1, stage_start, &info, legacy);
        let color = if legacy {
            ground_color
        } else {
            GROUND_COLORS[first.style as usize]
        };
        let shade = if world_row & 1 == 0 {
            color
        } else if legacy {
            if color == 5 {
                13
            } else {
                11
            }
        } else {
            GROUND_SHADES[first.style as usize]
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
        for right in [false, true] {
            emit_side(sink, &first, &last, right, z, facets, camera_y);
        }
        first = last;
    }
    for face in 0..14 {
        let (mut points, mut color) = backdrop_face(info.theme, face);
        if transition {
            let (destination, next_color) = backdrop_face(info.next_theme, face);
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
        if drop != 0 {
            for point in &mut points {
                point.y -= drop;
            }
        }
        sink.triangle(
            points[0],
            points[1],
            points[2],
            color,
            TWO_SIDED | if elapsed >= 400 { STATIC_SKY } else { 0 },
        );
    }
}

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static HORIZON_COLORS: [[u8; 2]; 10] = [
    [14, 12],
    [9, 2],
    [6, 12],
    [6, 11],
    [11, 6],
    [8, 0],
    [9, 12],
    [12, 15],
    [11, 14],
    [10, 12],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static CLOUD_COLORS: [[u8; 2]; 10] = [
    [1, 15],
    [7, 10],
    [15, 12],
    [10, 14],
    [3, 11],
    [8, 0],
    [15, 12],
    [15, 3],
    [1, 15],
    [15, 10],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
#[rustfmt::skip]
pub(crate) static HORIZONS: [[[i8; 2]; 15]; 10] = [
    [[-88, -34], [-33, -34], [-60, 17], [-45, -34], [3, -34], [-27, 9], [-8, -34], [40, -34], [12, 14], [26, -34], [73, -34], [51, 7], [61, -34], [88, -34], [74, 12]],
    [[-88, -34], [-7, -34], [-30, 21], [-88, -34], [-30, 21], [-73, 21], [6, -34], [88, -34], [75, 18], [6, -34], [75, 18], [25, 18], [-18, -34], [18, -34], [1, 4]],
    [[-88, -34], [-12, -34], [-12, 16], [-88, -34], [-12, 16], [-88, 16], [10, -34], [88, -34], [88, 12], [10, -34], [88, 12], [10, 12], [-15, -34], [15, -34], [-9, 24]],
    [[-88, -34], [-4, -34], [-40, 3], [-88, -34], [-40, 3], [-88, 3], [4, -34], [88, -34], [88, 0], [4, -34], [88, 0], [50, 0], [-14, -34], [14, -34], [3, 5]],
    // Cave: two sloping walls meet a connected, three-facet ceiling. The
    // farther hanging crystals are naturally masked where they enter the rock.
    [[-88, -34], [-48, 58], [-88, 127], [-88, 127], [-48, 58], [0, 74], [-88, 127], [0, 74], [88, 127], [0, 74], [48, 58], [88, 127], [48, 58], [88, -34], [88, 127]],
    [[-88, -34], [-24, -34], [-54, 30], [-88, -34], [-54, 30], [-72, 12], [-30, -34], [46, -34], [12, 22], [28, -34], [88, -34], [68, 27], [28, -34], [68, 27], [48, 8]],
    [[-88, -34], [-30, -34], [-30, 21], [-88, -34], [-30, 21], [-88, 21], [-35, -34], [35, -34], [-3, 30], [26, -34], [88, -34], [88, 16], [26, -34], [88, 16], [26, 16]],
    [[-88, -34], [-20, -34], [-20, 24], [-88, -34], [-20, 24], [-88, 8], [20, -34], [88, -34], [88, 8], [20, -34], [88, 8], [20, 24], [-24, -34], [24, -34], [0, 32]],
    [[-88, -34], [-24, -34], [-49, 9], [-64, -34], [-4, -34], [-21, 24], [-8, -34], [46, -34], [28, 5], [21, -34], [88, -34], [67, 20], [55, -34], [88, -34], [81, 7]],
    [[-88, -34], [-26, -34], [-54, 18], [-61, -34], [-5, -34], [-21, 6], [-10, -34], [34, -34], [5, 29], [18, -34], [77, -34], [51, 15], [60, -34], [88, -34], [81, 26]],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static CLOUD_CENTERS: [[[i8; 3]; 3]; 10] = [
    [[-67, 78, 96], [4, 110, 112], [89, 69, 127]],
    [[-72, 61, 96], [6, 96, 112], [98, 65, 127]],
    [[-72, 82, 96], [4, 67, 112], [103, 124, 127]],
    [[-67, 58, 96], [14, 101, 112], [102, 57, 127]],
    [[-48, 106, 96], [4, 121, 112], [75, 125, 127]],
    [[-73, 73, 96], [5, 104, 112], [96, 80, 127]],
    [[-68, 93, 96], [9, 78, 112], [100, 117, 127]],
    [[-71, 94, 96], [4, 80, 112], [97, 113, 127]],
    [[-67, 67, 96], [8, 91, 112], [103, 72, 127]],
    [[-74, 93, 96], [10, 112, 112], [97, 80, 127]],
];
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub(crate) static CLOUD_OUTLINES: [[[i8; 2]; 5]; 10] = [
    [[-27, 0], [-13, 11], [4, 15], [27, 2], [20, -6]],
    [[-37, 0], [-18, 5], [10, 9], [37, 2], [29, -6]],
    [[-46, 0], [-28, 15], [7, 16], [46, 1], [38, -7]],
    [[-33, 0], [-19, 5], [7, 7], [33, 1], [25, -5]],
    [[-16, 0], [-11, -48], [0, -68], [8, -38], [18, 0]],
    [[-42, 0], [-28, 9], [7, 12], [42, 1], [33, -8]],
    [[-35, 0], [-21, 12], [8, 13], [35, 1], [28, -7]],
    [[-30, 0], [-18, 8], [5, 11], [30, 1], [21, -6]],
    [[-46, 0], [-27, 10], [9, 13], [46, 2], [35, -8]],
    [[-28, 0], [-16, 4], [6, 8], [28, 1], [19, -4]],
];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
pub(crate) fn backdrop_face(section: u8, face: u8) -> ([Vec3; 3], u8) {
    let theme = section as usize;
    // Retain checked entry bounds before forming compact table addresses. The
    // zero-sized face slots validate without adding another resident lookup.
    let _ = HORIZON_COLORS[theme];
    let _ = [(); 14][face as usize];
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
        // All themes contain exactly 15 contiguous vertices. Multiplying the
        // bounded theme in a byte avoids the native wide multiply by 30.
        let vertices = section * 15;
        // SAFETY: theme < 10 above; each selected row contains 15 [i8; 2]s.
        outline = unsafe {
            core::slice::from_raw_parts(
                HORIZONS.as_ptr().cast::<[i8; 2]>().add(vertices as usize),
                15,
            )
        };
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
        let outline_offset = section * 10;
        let center_offset = section * 9 + cloud * 3;
        // SAFETY: theme < 10 and cloud is 0..=2. Array rows are contiguous,
        // byte-aligned, and the bounded offsets select complete rows/centers.
        unsafe {
            outline = core::slice::from_raw_parts(
                CLOUD_OUTLINES
                    .as_ptr()
                    .cast::<i8>()
                    .add(outline_offset as usize)
                    .cast::<[i8; 2]>(),
                5,
            );
            center = *CLOUD_CENTERS
                .as_ptr()
                .cast::<i8>()
                .add(center_offset as usize)
                .cast::<[i8; 3]>();
        }
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

/// Immutable inputs shared by each separately composed decoration tail.
#[repr(C)]
pub struct Decoration {
    scene_ticks: u16,
    elapsed: u16,
    camera_y: i16,
    stage: u8,
    info: StageInfo,
    ground_color: u8,
    legacy: bool,
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
fn decoration(sink: &mut SceneSink<'_>, context: &Decoration) {
    #[cfg(target_arch = "mos")]
    unsafe {
        extern "Rust" {
            fn scenic_decoration(sink: &mut SceneSink<'_>, context: &Decoration);
        }
        scenic_decoration(sink, context);
    }
    #[cfg(not(target_arch = "mos"))]
    {
        if context.info.theme >= 10 {
            let theme = if context.elapsed >= 200 && context.elapsed < 400 {
                context.info.next_theme
            } else {
                context.info.theme
            };
            match theme {
                10 => space::scenic_asteroid(sink, context),
                11 => space::scenic_moon(sink, context),
                12 => space::scenic_graveyard(sink, context),
                13 => space::scenic_shipyard(sink, context),
                14 => space::scenic_carrier(sink, context),
                _ => space::scenic_gate(sink, context),
            }
        } else {
            match context.info.bridge_profile {
                0 => scenic_wildlife(sink, context),
                1 => scenic_transport(sink, context),
                2 => scenic_meteors(sink, context),
                _ => space::scenic_ascent(sink, context),
            }
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.wildlife.entry")]
pub fn scenic_wildlife(sink: &mut SceneSink<'_>, context: &Decoration) {
    wildlife(sink, context);
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.transport.entry")]
pub fn scenic_transport(sink: &mut SceneSink<'_>, context: &Decoration) {
    transport(sink, context.elapsed, context.camera_y);
}
#[inline(never)]
#[cfg_attr(target_arch = "mos", no_mangle)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.meteors.entry")]
pub fn scenic_meteors(sink: &mut SceneSink<'_>, context: &Decoration) {
    meteors(sink, context.elapsed);
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
fn wildlife(sink: &mut SceneSink<'_>, context: &Decoration) {
    let Decoration {
        scene_ticks,
        elapsed,
        camera_y,
        stage,
        info,
        ..
    } = *context;
    // This crossing is fixed in the world: the camera closes eight units/tick.
    // It enters and leaves beyond the viewport, so no visible actor pops at a cut.
    let z = 3000 - elapsed as i16 * 8;
    let x = -3200 + elapsed as i16 * 16;
    if z > 180 {
        let row = row_at(
            (stage_origin(stage, false) + scene_ticks + (z as u16 / 8)) >> 6,
            stage_origin(stage, false),
            &info,
            false,
        );
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

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.transport.data")]
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.transport.data")]
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.transport")]
fn transport(sink: &mut SceneSink<'_>, elapsed: u16, camera_y: i16) {
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.meteors")]
fn meteors(sink: &mut SceneSink<'_>, elapsed: u16) {
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
    use crate::assets::STAGE_INFOS;
    use std::vec::Vec;
    const BRIDGES: [u16; 11] = [
        3750, 7900, 12050, 16200, 20350, 24500, 28650, 32800, 36950, 41100, 45250,
    ];

    #[test]
    fn byte_height_arithmetic_preserves_all_signed_profiles() {
        let scales = [32, 16, 16, 16, 48, 48, 24, 32, 40, 48];
        for style in 0..10u8 {
            for row in [0u16, 1, 7, 15, 16, 1023, 65528, 65535] {
                for side in [false, true] {
                    let phase = row.wrapping_add(if side { 7 } else { 0 }) & 15;
                    let expected = HEIGHT_BASE[style as usize]
                        + PROFILE[phase as usize] as i16 * scales[style as usize];
                    assert_eq!(terrain_height(row, side, style), expected);
                }
            }
            for row in 0..16 {
                let expected = HEIGHT_BASE[style as usize]
                    + PROFILE[row as usize] as i16 * scales[style as usize];
                assert_eq!(terrain_height(row, false, style), expected);
            }
        }
    }

    #[test]
    fn compact_backdrop_addresses_match_every_authored_face() {
        for theme in 0..10usize {
            for face in 0..14usize {
                // Frozen safe indexing oracle, independent of the compact
                // byte offsets used by the native table decoder.
                let expected = if face < 5 {
                    (
                        core::array::from_fn(|corner| {
                            let xy = HORIZONS[theme][face * 3 + corner];
                            Vec3 {
                                x: xy[0] as i16 * 32,
                                y: xy[1] as i16 * 16 - BACKDROP_CAMERA_Y,
                                z: 2350,
                            }
                        }),
                        HORIZON_COLORS[theme][face & 1],
                    )
                } else {
                    let cloud = (face - 5) / 3;
                    let facet = (face - 5) % 3;
                    let center = CLOUD_CENTERS[theme][cloud];
                    let vertices = [0, facet + 1, facet + 2];
                    (
                        core::array::from_fn(|corner| {
                            let xy = CLOUD_OUTLINES[theme][vertices[corner]];
                            Vec3 {
                                x: (center[0] as i16 + xy[0] as i16) * 32,
                                y: (center[1] as i16 + xy[1] as i16) * 16,
                                z: center[2] as i16 * 32,
                            }
                        }),
                        CLOUD_COLORS[theme][(facet == 2) as usize],
                    )
                };
                assert_eq!(backdrop_face(theme as u8, face as u8), expected);
            }
        }
    }

    #[test]
    #[should_panic]
    fn compact_backdrop_rejects_invalid_theme_before_table_reads() {
        backdrop_face(10, 0);
    }

    #[test]
    #[should_panic]
    fn compact_backdrop_rejects_invalid_face_before_table_reads() {
        backdrop_face(9, 14);
    }

    fn context(stage: u8, elapsed: u16, camera_y: i16) -> Decoration {
        Decoration {
            scene_ticks: 3750 + elapsed,
            elapsed,
            camera_y,
            stage,
            info: STAGE_INFOS[stage as usize],
            ground_color: GROUND_COLORS[STAGE_INFOS[stage as usize].theme as usize],
            legacy: false,
        }
    }

    #[derive(Default, Debug, PartialEq, Eq)]
    struct Capture(Vec<([Vec3; 3], u8, u8)>);
    impl GeometrySink for Capture {
        fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
            self.0.push(([a, b, c], color, flags));
        }
    }

    #[test]
    fn sign_selected_sides_match_original_coordinates_across_every_stage_and_seam() {
        for (stage, info) in STAGE_INFOS[..12].iter().enumerate() {
            let start = stage as u16 * 4150;
            for row in start.saturating_sub(400) / 64..=(start + 4350) / 64 {
                for legacy in [false, true] {
                    let first = row_at(row, start, info, legacy);
                    let last = row_at(row + 1, start, info, legacy);
                    for side in [-1i16, 1] {
                        let mut actual = Capture::default();
                        let mut expected = Capture::default();
                        emit_side(&mut actual, &first, &last, side > 0, [320, 832], [3, 7], 37);
                        let heights = if side < 0 {
                            [first.left, last.left]
                        } else {
                            [first.right, last.right]
                        };
                        let edges = [side * first.edge, side * last.edge];
                        strip(
                            &mut expected,
                            [side * first.lane, side * last.lane],
                            [-105; 2],
                            edges,
                            heights,
                            [320, 832],
                            [3, 7],
                            37,
                            EXTERIOR_TERRAIN,
                        );
                        if first.edge < 640 || last.edge < 640 {
                            strip(
                                &mut expected,
                                edges,
                                heights,
                                [side * 640; 2],
                                heights,
                                [320, 832],
                                [3, 7],
                                37,
                                EXTERIOR_TERRAIN,
                            );
                        } else if matches!(first.style, 2 | 6 | 7) && first.style == last.style {
                            strip(
                                &mut expected,
                                [side * 95; 2],
                                [-103; 2],
                                [side * 100; 2],
                                [-103; 2],
                                [320, 832],
                                [15; 2],
                                37,
                                EXTERIOR_TERRAIN,
                            );
                        }
                        assert_eq!(actual, expected, "stage {stage}, row {row}, side {side}");
                    }
                }
            }
        }
    }

    fn capture(ticks: u16, section: u8, transition: Option<(u8, u16)>, camera: i16) -> Capture {
        let mut bytes = [0; crate::assets::SEGMENT_CACHE_SIZE];
        bytes[..4].copy_from_slice(b"SMS2");
        bytes[4] = section;
        let info = STAGE_INFOS[section as usize];
        bytes[5] = info.enemy_profile;
        bytes[6] = GROUND_COLORS[info.theme as usize];
        bytes[8..16].copy_from_slice(&[
            info.enemy_profile,
            info.music_cue,
            info.bridge_profile,
            info.boss_kind,
            info.theme,
            info.next_theme,
            info.previous_theme,
            0,
        ]);
        let segment = Segment::new(&bytes).unwrap();
        let mut result = Capture::default();
        emit(
            &mut result,
            &segment,
            ticks - section as u16 * 4150,
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
                    renderer.begin(sky_color(
                        STAGE_INFOS[section].theme,
                        Some((STAGE_INFOS[section].next_theme, elapsed)),
                    ));
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
        for (section, start) in BRIDGES.into_iter().enumerate() {
            for tick in start - 256..start + 657 {
                let row = tick >> 6;
                let amount = tick & 63;
                let first = row_at(row, section as u16 * 4150, &STAGE_INFOS[section], false);
                let shared = row_at(row + 1, section as u16 * 4150, &STAGE_INFOS[section], false);
                // Replacing the front row keeps the old far edge exactly.
                if amount == 63 {
                    assert_eq!(
                        shared,
                        row_at(
                            (tick + 1) >> 6,
                            section as u16 * 4150,
                            &STAGE_INFOS[section],
                            false
                        )
                    );
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
                let expected_theme = STAGE_INFOS[section + (elapsed != 0) as usize].theme;
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
                    let (points, color) = backdrop_face(expected_theme, face as u8);
                    assert_eq!((actual.0, actual.1), (points, color));
                }
                assert_eq!(
                    sky_color(
                        STAGE_INFOS[section].theme,
                        Some((STAGE_INFOS[section].next_theme, elapsed))
                    ),
                    SKY_COLORS[expected_theme as usize]
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
        wildlife(&mut a, &context(0, 200, 40));
        wildlife(&mut b, &context(0, 200, 40));
        wildlife(&mut c, &context(0, 208, 40));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.0.len(), 26);
        assert!(a.0.iter().all(|face| face.2 == TWO_SIDED));
        assert_ne!(a.0[0].0, c.0[0].0, "feet must stride");
        assert_ne!(a.0[20].0, c.0[20].0, "wings must flap");
    }

    fn decoration(profile: u8, elapsed: u16, camera: i16) -> Capture {
        let stage = [0, 2, 4][profile as usize];
        let mut result = Capture::default();
        super::decoration(&mut result, &context(stage, elapsed, camera));
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
        let stage = 9;
        let start = BRIDGES[stage];
        let info = STAGE_INFOS[stage];
        let mut previous = 145;
        for row in (start >> 6)..=((start + 464) >> 6) {
            let sample = row_at(row, stage as u16 * 4150, &info, false);
            assert!(sample.lane >= previous);
            assert!((145..=320).contains(&sample.lane));
            previous = sample.lane;
        }
        assert_eq!(previous, 320);
        for row in ((start + 464) >> 6)..((start + 3000) >> 6) {
            assert_eq!(row_at(row, 10 * 4150, &STAGE_INFOS[10], false).lane, 320);
            assert_eq!(row_at(row, 10 * 4150, &STAGE_INFOS[10], true).lane, 145);
        }
    }

    #[test]
    fn bridge_terrain_retains_exact_world_faces_through_scroll_and_row_replacement() {
        for (section, start) in BRIDGES.into_iter().enumerate() {
            for tick in start - 128..start + 529 {
                let before = capture(tick, section as u8, None, 40);
                let after = capture(tick + 1, section as u8, None, 40);
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

    #[test]
    fn checked_metadata_handoff_keeps_surviving_world_faces_identical() {
        for (stage, start) in BRIDGES.into_iter().enumerate() {
            let old = capture(start + 399, stage as u8, None, 40);
            let new = capture(start + 400, stage as u8 + 1, None, 40);
            for &(mut points, color, flags) in &old.0[..old.0.len() - 14] {
                for point in &mut points {
                    point.z -= 8;
                }
                if points.iter().map(|point| point.z).max().unwrap() > 40 {
                    assert!(
                        new.0.contains(&(points, color, flags)),
                        "stage handoff {stage}"
                    );
                }
            }
        }
    }

    #[test]
    fn ten_themes_have_distinct_fixed_backdrops_and_fit_ordinary_geometry() {
        let mut backgrounds = Vec::new();
        for theme in 0..10 {
            let background: Vec<_> = (0..14).map(|face| backdrop_face(theme, face)).collect();
            assert!(
                !backgrounds.contains(&background),
                "duplicate theme {theme}"
            );
            backgrounds.push(background);
        }
        for stage in 0..12 {
            for phase in 0..64 {
                let capture = capture(stage * 4150 + 1000 + phase, stage as u8, None, 40);
                assert!(capture.0.len() <= 54, "stage {stage}, phase {phase}");
                assert!(capture.0.iter().all(|face| face.1 < 16));
            }
        }
    }

    #[test]
    fn rebased_clocks_preserve_terrestrial_rows_shades_and_scroll() {
        for section in 0..12u8 {
            let info = STAGE_INFOS[section as usize];
            for legacy in [false, true] {
                let old_start = section as u16 * if legacy { 3750 } else { 4150 };
                let new_start = stage_origin(section, legacy);
                for local in 0..4150 {
                    let old = old_start + local;
                    let new = new_start + local;
                    assert_eq!(old & 1023, new & 1023);
                    assert_eq!(old & 63, new & 63);
                    for ahead in 0..5 {
                        assert_eq!(
                            row_at((old >> 6) + ahead, old_start, &info, legacy),
                            row_at((new >> 6) + ahead, new_start, &info, legacy),
                            "stage {section}, tick {local}, legacy {legacy}",
                        );
                    }
                }
            }
        }
    }

    fn orbital_capture(section: u8, local: u16, elapsed: Option<u16>, camera: i16) -> Capture {
        let mut result = Capture::default();
        let context = Decoration {
            scene_ticks: local,
            elapsed: elapsed.unwrap_or(400),
            camera_y: camera,
            stage: section,
            info: STAGE_INFOS[section as usize],
            ground_color: 0,
            legacy: false,
        };
        super::decoration(&mut result, &context);
        result
    }

    fn pixels(capture: &Capture) -> Vec<u8> {
        let mut renderer = crate::render::Renderer::new();
        renderer.begin(0);
        for &(p, color, flags) in &capture.0 {
            renderer.triangle(p[0], p[1], p[2], color, flags);
        }
        renderer.finish();
        assert_eq!(renderer.dropped, 0);
        let mut result = Vec::new();
        let mut band = [0; 1280];
        for row in 0..25 {
            renderer.render_band(row, &mut band);
            result.extend_from_slice(&band);
        }
        result
    }

    #[test]
    fn orbital_scenes_are_distinct_black_space_with_bounded_geometry() {
        let mut samples = Vec::new();
        for section in 12..18u8 {
            assert_eq!(sky_color(STAGE_INFOS[section as usize].theme, None), 0);
            for tick in 0..256 {
                for camera in [24, 40, 56] {
                    let scene = orbital_capture(section, tick, None, camera);
                    assert!(scene.0.len() <= 54, "stage {section}, tick {tick}");
                    assert!(scene
                        .0
                        .iter()
                        .all(|f| f.1 < 16 && f.2 & EXTERIOR_TERRAIN == 0));
                    let mut renderer = crate::render::Renderer::new();
                    renderer.begin(0);
                    for &(p, color, flags) in &scene.0 {
                        renderer.triangle(p[0], p[1], p[2], color, flags);
                    }
                    assert_eq!(renderer.dropped, 0);
                    assert!(renderer.triangle_count <= 80);
                }
            }
            let image = pixels(&orbital_capture(section, 100, None, 40));
            assert!(
                !samples.contains(&image),
                "orbital stage {section} duplicates an earlier scene"
            );
            assert!(image.iter().filter(|&&p| p == 0).count() > 8000);
            samples.push(image);
        }
    }

    #[test]
    fn orbital_midpoint_tail_swap_has_exactly_the_same_visible_starfield() {
        for section in 12..17u8 {
            for camera in [24, 40, 56] {
                let before = pixels(&orbital_capture(section, 3949, Some(199), camera));
                let after = pixels(&orbital_capture(section, 3950, Some(200), camera));
                assert_eq!(before, after, "crossing after stage {section}");
            }
            // The final tick and incoming checkpoint advance their world phase
            // by one tick; no additional local clock is reset at the handoff.
            let a = orbital_capture(section, 4149, Some(399), 40);
            let b = orbital_capture(section + 1, 65535, None, 40);
            assert_eq!(
                a.0.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
                b.0.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn ascent_recedes_from_coast_into_matching_asteroid_backdrop() {
        let outgoing = capture(11 * 4150 + 3750, 11, None, 40);
        let entry = orbital_capture(11, 3750, Some(0), 40);
        assert_eq!(
            outgoing.0.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
            entry.0.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
        );
        for tick in 0..400 {
            for camera in [24, 40, 56] {
                let scene = orbital_capture(11, 3750 + tick, Some(tick), camera);
                assert!(scene.0.len() <= 80, "ascent tick {tick}");
                assert!(scene.0.iter().all(|f| f.2 & STATIC_SKY == 0));
                let mut renderer = crate::render::Renderer::new();
                renderer.begin(sky_color(3, Some((10, tick))));
                for &(p, color, flags) in &scene.0 {
                    renderer.triangle(p[0], p[1], p[2], color, flags);
                }
                assert_eq!(renderer.dropped, 0);
                assert!(renderer.triangle_count <= 96, "ascent tick {tick}");
            }
        }
        assert_eq!(sky_color(3, Some((10, 0))), SKY_COLORS[3]);
        assert_eq!(sky_color(3, Some((10, 180))), 0);
        assert_eq!(
            pixels(&orbital_capture(11, 3999, Some(249), 40)),
            pixels(&orbital_capture(11, 4000, Some(250), 40)),
        );
        assert!(pixels(&orbital_capture(11, 4050, Some(300), 40)).contains(&14));
        assert_eq!(
            pixels(&orbital_capture(11, 4149, Some(399), 40)),
            pixels(&orbital_capture(12, 65535, None, 40)),
        );
        // At terrain retirement every surviving vertex has passed below the
        // visible viewport, so removing its raw records cannot pop a silhouette.
        let mut land = Capture::default();
        let context = Decoration {
            scene_ticks: 3999,
            elapsed: 249,
            camera_y: 24,
            stage: 11,
            info: STAGE_INFOS[11],
            ground_color: 6,
            legacy: false,
        };
        super::land(&mut land, &context, 249 * 24);
        assert!(land.0.iter().all(|f| f
            .0
            .iter()
            .all(|&p| p.z < 32 || crate::projection::project(p).1 >= 200)));
    }
}
