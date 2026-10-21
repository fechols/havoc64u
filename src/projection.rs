//! Perspective projection with a small, exact target-side vertex cache.
//!
//! `x' = 80 + 80·x/z` and `y' = 100 − 133·y/z`, computed as one 16×16 product
//! per axis: z is normalised to `d ∈ 128..=255` with a signed exponent `e`
//! (`z ≈ d·2^e`), each axis has a 128-entry table of `scale·32768/d`, and
//! `>> (15 + e)` moves the binary point back (y is negated first, so both
//! axes round the way the renderer always did). Projected points always lie at
//! or behind the near plane (z ≥ 8), so `e ≥ −4`; the tables are exact to
//! one part in 20,000, far finer than the pixel grid. The native kernel
//! `fast_project` performs the identical arithmetic.
use crate::Vec3;

const fn projection_table(scale: u32) -> [u16; 128] {
    let mut table = [0; 128];
    let mut i = 0;
    while i < 128 {
        let d = i as u32 + 128;
        table[i] = ((scale * 32768 + d / 2) / d) as u16;
        i += 1;
    }
    table
}
/// `80·32768/d` for `d = 128 + index`; shared with the native kernel.
#[no_mangle]
static PROJECT_X: [u16; 128] = projection_table(80);
/// `133·32768/d` for `d = 128 + index`; shared with the native kernel.
#[no_mangle]
static PROJECT_Y: [u16; 128] = projection_table(133);

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
#[cfg_attr(target_arch = "mos", link_section = ".tables.projection_reset")]
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
    // Multiplications by 13 and 7 written as shifted adds: same hash, no
    // 16-bit multiply helper on the target. The cache is a memo keyed by the
    // whole point, so the native emit kernel's cheaper byte-mix index yields
    // the same projections; only the hit rate could differ.
    let y = point.y as u16;
    let z = point.z as u16;
    let mixed = (point.x as u16)
        .wrapping_add((y << 3).wrapping_add(y << 2).wrapping_add(y))
        .wrapping_add((z << 3).wrapping_sub(z));
    let index = ((mixed ^ (mixed >> 6) ^ (mixed >> 12)) & 63) as usize;
    let entry = &mut cache[index];
    if entry.point == point {
        return (entry.screen[0], entry.screen[1]);
    }
    entry.point = point;
    #[cfg(target_arch = "mos")]
    {
        extern "C" {
            fn fast_project(point: *const Vec3, out: *mut [i16; 2]);
        }
        // SAFETY: the kernel performs exactly project_reference on the
        // repr(C) point held by the entry and fills its screen pair; BOOT
        // built its multiply table and PROJECT_X/Y are the resident tables.
        unsafe { fast_project(&entry.point, &mut entry.screen) };
    }
    #[cfg(not(target_arch = "mos"))]
    {
        let (x, y) = project_reference(point);
        entry.screen = [x, y];
    }
    (entry.screen[0], entry.screen[1])
}

#[cfg(not(target_arch = "mos"))]
#[inline(never)]
fn project_uncached(point: Vec3) -> (i16, i16) {
    project_reference(point)
}

/// The projection arithmetic itself; the native build runs it in assembly.
#[cfg(any(not(target_arch = "mos"), test))]
fn project_reference(point: Vec3) -> (i16, i16) {
    let mut d = point.z as u16;
    if d == 0 {
        return (80, 100);
    }
    let mut e = 0i8;
    while d > 255 {
        d >>= 1;
        e += 1;
    }
    while d < 128 {
        d <<= 1;
        e -= 1;
    }
    let shift = (15 + e) as u32;
    let x = (point.x as i32 * PROJECT_X[(d - 128) as usize] as i32) >> shift;
    // Negate before the shift so y rounds toward the top of the screen, the
    // direction the renderer always used.
    let y = (-(point.y as i32) * PROJECT_Y[(d - 128) as usize] as i32) >> shift;
    ((80 + x).clamp(-8191, 8191) as i16, (100 + y).clamp(-8191, 8191) as i16)
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
            // Full-range coordinates down to the near plane must not overflow.
            let near = crate::render::NEAR as u32;
            let point = Vec3 {
                x: random as i16,
                y: (random >> 16) as i16,
                z: (near + random % (32768 - near)) as i16,
            };
            let expected = project_uncached(point);
            assert_eq!(project_cached(&mut cache, point), expected);
            assert_eq!(project_cached(&mut cache, point), expected);
        }
        for y in [i16::MIN, -30001, -30000, -29999, 29999, 30000, 30001, i16::MAX] {
            for z in [crate::render::NEAR, 255, 256, 32767] {
                let (_, projected) = project_uncached(Vec3 { x: 0, y, z });
                let exact = (100 - (y as i64 * 133) / z as i64).clamp(-8191, 8191) as i16;
                if exact.abs() == 8191 {
                    assert_eq!(projected, exact, "y {y} z {z}");
                } else {
                    // Normalized reciprocals carry a small relative error.
                    assert!(
                        (projected - exact).abs() <= 2 + exact.abs() / 64,
                        "y {y} z {z}: {projected} vs {exact}"
                    );
                }
            }
        }
    }
}
