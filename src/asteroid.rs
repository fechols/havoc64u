//! A small, distant orbital rock uses its original six-vertex silhouette.
//! Close rocks add raised mineral facets; both share the same bounds and pivot.

use crate::assets::{AssetBank, ROCK};

pub(crate) static COARSE_FACES: [[u8; 4]; 8] = [
    [0, 1, 3, 12],
    [0, 3, 2, 15],
    [0, 4, 1, 11],
    [0, 2, 4, 11],
    [1, 5, 3, 12],
    [3, 5, 2, 15],
    [1, 4, 5, 11],
    [2, 5, 4, 11],
];

#[inline(always)]
pub(crate) fn coarse(bank: &AssetBank<'_>, id: u8, z: i16, half: bool) -> bool {
    id == ROCK && bank.vertex_count(id) == 12 && z >= if half { 160 } else { 320 }
}

#[cfg(target_arch = "mos")]
#[repr(C)]
pub(crate) struct MeshData {
    pub vertices: *const u8,
    pub faces: *const u8,
    pub vertex_count: u8,
    pub face_count: u8,
    pub shift: u8,
}

// Decode native mesh metadata once in resident code. This keeps the detail
// selector out of the nearly full GEOMETRY overlay; the coarse table is also
// resident, so this helper never refers into a temporarily loaded overlay.
#[cfg(target_arch = "mos")]
#[inline(never)]
pub(crate) fn mesh_data(bank: &AssetBank<'_>, id: u8, z: i16, half: bool) -> MeshData {
    let coarse = coarse(bank, id, z, half);
    MeshData {
        vertices: bank.vertex_data(id),
        faces: if coarse {
            COARSE_FACES.as_ptr().cast::<u8>()
        } else {
            bank.face_data(id)
        },
        vertex_count: if coarse { 6 } else { bank.vertex_count(id) },
        face_count: if coarse { 8 } else { bank.face_count(id) },
        shift: 8 - bank.shift(id) + half as u8,
    }
}
