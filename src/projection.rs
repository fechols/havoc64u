//! Perspective projection with a small, exact target-side vertex cache.
use crate::Vec3;

const fn reciprocal_table() -> [u16; 256] {
    let mut table = [0; 256];
    let mut i = 1;
    while i < 256 {
        let value = (65536u32 + i as u32 / 2) / i as u32;
        table[i] = if value > 65535 { 65535 } else { value as u16 };
        i += 1;
    }
    table
}
static RECIPROCAL: [u16; 256] = reciprocal_table();

#[derive(Clone, Copy)]
#[repr(C)]
struct Entry {
    point: Vec3,
    screen: [i16; 2],
}
#[cfg(test)]
const EMPTY: Entry = Entry {
    point: Vec3 { x: 0, y: 0, z: 0 },
    screen: [0; 2],
};
const _: [(); 10] = [(); core::mem::size_of::<Entry>()];

const BAND_ADDRESS: usize = 0x8c00;
const CACHE_ADDRESS: usize = BAND_ADDRESS;
const CACHE_BYTES: usize = core::mem::size_of::<[Entry; 64]>();
const _: () = {
    assert!(CACHE_BYTES == 640);
    assert!(CACHE_ADDRESS >= BAND_ADDRESS);
    assert!(CACHE_ADDRESS + CACHE_BYTES <= BAND_ADDRESS + crate::render::BAND_PIXELS);
    assert!(BAND_ADDRESS + crate::render::BAND_PIXELS <= 0x9100);
};

/// GEOMETRY temporarily owns the first 640 bytes of the pixel-band workspace.
/// PIXELS fills the entire band before drawing; the IRQ never accesses it.
#[cfg(target_arch = "mos")]
const CACHE: *mut [Entry; 64] = CACHE_ADDRESS as *mut [Entry; 64];

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
pub fn begin_frame() {
    #[cfg(target_arch = "mos")]
    // SAFETY: GEOMETRY owns this shared workspace until all projection is done.
    // Reset the previous pixel band's bytes before making cache references.
    // All fields are integers; z=0 cannot match a near-clipped scene vertex.
    unsafe {
        core::ptr::write_bytes(CACHE.cast::<u8>(), 0, CACHE_BYTES);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
pub fn project(point: Vec3) -> (i16, i16) {
    #[cfg(target_arch = "mos")]
    // SAFETY: begin_frame initializes the cache before geometry; the IRQ never
    // accesses it. No reference to a cache entry escapes this function.
    unsafe {
        project_cached(&mut *CACHE, point)
    }
    #[cfg(not(target_arch = "mos"))]
    project_uncached(point)
}

#[cfg(any(target_arch = "mos", test))]
#[inline(always)]
fn project_cached(cache: &mut [Entry; 64], point: Vec3) -> (i16, i16) {
    let mixed = (point.x as u16)
        .wrapping_add((point.y as u16).wrapping_mul(13))
        .wrapping_add((point.z as u16).wrapping_mul(7));
    let index = ((mixed ^ (mixed >> 6) ^ (mixed >> 12)) & 63) as usize;
    let entry = &mut cache[index];
    if entry.point == point {
        return (entry.screen[0], entry.screen[1]);
    }
    let result = project_uncached(point);
    *entry = Entry {
        point,
        screen: [result.0, result.1],
    };
    result
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
fn project_uncached(point: Vec3) -> (i16, i16) {
    let mut divisor = point.z as u16;
    let mut shift = 0;
    while divisor > 255 {
        divisor >>= 1;
        shift += 1;
    }
    let reciprocal = RECIPROCAL[divisor as usize] as i32;
    let x = 80 + ((((point.x as i32 * reciprocal) >> 4) * 80) >> (12 + shift));
    let y = 100 + ((((-i32::from(point.y) * reciprocal) >> 4) * 133) >> (12 + shift));
    (x.clamp(-8191, 8191) as i16, y.clamp(-8191, 8191) as i16)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_hits_and_collisions_preserve_projection() {
        let mut cache = [EMPTY; 64];
        let mut random = 0x21ab_6734u32;
        for _ in 0..10000 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let point = Vec3 {
                x: random as i16,
                y: (random >> 16) as i16,
                z: 32 + (random % 32736) as i16,
            };
            let expected = project_uncached(point);
            assert_eq!(project_cached(&mut cache, point), expected);
            assert_eq!(project_cached(&mut cache, point), expected);
        }
    }
}
