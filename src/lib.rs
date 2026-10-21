#![no_std]

#[cfg(test)]
extern crate self as havoc;
#[cfg(test)]
extern crate std;

pub mod assets;
pub mod audio;
pub mod cells;
pub mod game;
pub mod hud;
pub mod math;
pub mod music;
pub mod presentation;
pub mod projection;
pub mod render;
pub mod scenery;
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
    /// Draw one mesh instance. The default is the reference path below; the
    /// native scene sink overrides it with the asm mesh kernel.
    #[allow(clippy::too_many_arguments)]
    fn mesh(
        &mut self,
        bank: &assets::AssetBank<'_>,
        id: u8,
        position: Vec3,
        yaw: u8,
        pitch: u8,
        roll: u8,
        half: bool,
    ) where
        Self: Sized,
    {
        emit_mesh_reference(self, bank, id, position, yaw, pitch, roll, half);
    }
}

/// Emit a compact REU mesh in camera coordinates. Angles wrap in 256 steps.
/// `half` draws the instance at half size (small debris fragments).
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub fn emit_mesh(
    sink: &mut impl GeometrySink,
    bank: &assets::AssetBank<'_>,
    id: u8,
    position: Vec3,
    yaw: u8,
    pitch: u8,
    roll: u8,
    half: bool,
) {
    sink.mesh(bank, id, position, yaw, pitch, roll, half);
}

/// The reference mesh path: every vertex rotated once, faces indexing the
/// results and reaching the sink one triangle at a time.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
pub fn emit_mesh_reference(
    sink: &mut impl GeometrySink,
    bank: &assets::AssetBank<'_>,
    id: u8,
    position: Vec3,
    yaw: u8,
    pitch: u8,
    roll: u8,
    half: bool,
) {
    // Every vertex is transformed once into this array; the native build
    // keeps the array small to spare the 1 KiB software stack and recomputes
    // the medal's remaining vertices on demand (Victory only).
    const CACHED_VERTICES: usize = if cfg!(target_arch = "mos") { 32 } else { 128 };
    let mut vertices = [Vec3 { x: 0, y: 0, z: 0 }; CACHED_VERTICES];
    let transform = MeshTransform {
        position,
        rotation: math::RotationMatrix::new(yaw, pitch, roll),
        // Stored coordinates are i8 in units of 1 << shift; the matrix is Q8.
        shift: 8 - bank.shift(id) + half as u8,
        id,
    };
    for index in 0..(bank.vertex_count(id) as usize).min(CACHED_VERTICES) {
        vertices[index] = transformed_mesh_vertex(bank, &transform, index as u8);
    }
    let flags = if bank.one_sided(id) { 0 } else { TWO_SIDED };
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
        sink.triangle(points[0], points[1], points[2], color, flags);
    }
}

struct MeshTransform {
    position: Vec3,
    rotation: math::RotationMatrix,
    shift: u8,
    id: u8,
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn transformed_mesh_vertex(
    bank: &assets::AssetBank<'_>,
    transform: &MeshTransform,
    index: u8,
) -> Vec3 {
    let v = transform
        .rotation
        .apply(bank.vertex_bytes(transform.id, index), transform.shift);
    Vec3 {
        x: v.x.saturating_add(transform.position.x),
        y: v.y.saturating_add(transform.position.y),
        z: v.z.saturating_add(transform.position.z),
    }
}
