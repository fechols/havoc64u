#![no_std]

#[cfg(test)]
extern crate self as havoc;
#[cfg(test)]
extern crate std;

pub mod assets;
pub mod audio;
pub mod game;
pub mod hud;
pub mod math;
pub mod music;
pub mod presentation;
pub mod projection;
pub mod render;
pub mod scenery;
pub mod sky_cache;
#[cfg(feature = "host-tools")]
pub mod soundtrack;
pub mod space_dust;
pub mod transition;
pub mod victory;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Vec3 {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

pub const TWO_SIDED: u8 = 1;
// Geometry proven above/in front of every exterior terrain face preserves
// its coverage against terrain without changing other foreground ordering.
pub(crate) const TERRAIN_CLEAR_GEOMETRY: u8 = 0x20;
pub(crate) const EXTERIOR_TERRAIN: u8 = 0x40;
pub trait GeometrySink {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8);
}

/// Emit a compact REU mesh in camera coordinates. Angles wrap in 256 steps.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
pub fn emit_mesh(
    sink: &mut impl GeometrySink,
    bank: &assets::AssetBank<'_>,
    id: u8,
    position: Vec3,
    yaw: u8,
    pitch: u8,
    roll: u8,
    scale_q8: i16,
) {
    // Current original models use at most ten vertices. A small per-object
    // cache saves repeated rotations without exhausting the 1 KiB C64 stack.
    // Larger compatible meshes retain a bounded, on-demand fallback.
    const CACHED_VERTICES: usize = 32;
    let mut vertices = [Vec3 { x: 0, y: 0, z: 0 }; CACHED_VERTICES];
    let transform = MeshTransform {
        position,
        rotation: math::PreparedRotation::new(yaw, pitch, roll),
        scale_q8,
        id,
    };
    for index in 0..(bank.vertex_count(id) as usize).min(CACHED_VERTICES) {
        vertices[index] = transformed_mesh_vertex(bank, &transform, index as u8);
    }
    for face in 0..bank.face_count(id) {
        let (indices, color) = bank.face(id, face);
        let mut points = [Vec3 { x: 0, y: 0, z: 0 }; 3];
        for corner in 0..3 {
            let index = indices[corner];
            points[corner] = if (index as usize) < CACHED_VERTICES {
                vertices[index as usize]
            } else {
                transformed_mesh_vertex(bank, &transform, index)
            };
        }
        // The solid medallion has consistently outward-facing triangles;
        // culling its concealed faces also avoids painter-order artifacts.
        let flags = if id == assets::MEDAL { 0 } else { TWO_SIDED };
        sink.triangle(points[0], points[1], points[2], color, flags);
    }
}

struct MeshTransform {
    position: Vec3,
    rotation: math::PreparedRotation,
    scale_q8: i16,
    id: u8,
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn transformed_mesh_vertex(
    bank: &assets::AssetBank<'_>,
    transform: &MeshTransform,
    index: u8,
) -> Vec3 {
    let v = bank.vertex(transform.id, index);
    // Multiplication by Q8 identity is exact for every i16, including negative
    // coordinates. All shipped meshes use it, so avoid three wide multiplies.
    let v = if transform.scale_q8 == 256 {
        v
    } else {
        Vec3 {
            x: math::mul_q8(v.x, transform.scale_q8),
            y: math::mul_q8(v.y, transform.scale_q8),
            z: math::mul_q8(v.z, transform.scale_q8),
        }
    };
    let v = transform.rotation.apply(v);
    Vec3 {
        x: v.x.saturating_add(transform.position.x),
        y: v.y.saturating_add(transform.position.y),
        z: v.z.saturating_add(transform.position.z),
    }
}
