use crate::Vec3;
const QUARTER: [i16; 65] = [
    0, 6, 13, 19, 25, 31, 38, 44, 50, 56, 62, 68, 74, 80, 86, 92, 98, 104, 109, 115, 121, 126, 132,
    137, 142, 147, 152, 157, 162, 167, 172, 177, 181, 185, 190, 194, 198, 202, 206, 209, 213, 216,
    220, 223, 226, 229, 231, 234, 237, 239, 241, 243, 245, 247, 248, 250, 251, 252, 253, 254, 255,
    255, 256, 256, 256,
];
#[inline(never)]
pub fn sin8(a: u8) -> i16 {
    let q = a >> 6;
    let p = (a & 63) as usize;
    match q {
        0 => QUARTER[p],
        1 => QUARTER[64 - p],
        2 => -QUARTER[p],
        _ => -QUARTER[64 - p],
    }
}
// Keep this small, phase-independent arithmetic helper resident so the
// geometry overlay retains room for the expanded game-state layout. The
// native body is the quarter-square kernel, exact to the reference below.
#[inline(never)]
pub fn mul_q8(a: i16, b: i16) -> i16 {
    #[cfg(target_arch = "mos")]
    {
        extern "C" {
            fn fast_mul_q8(a: i16, b: i16) -> i16;
        }
        // SAFETY: BOOT fills the quarter-square table before any phase runs;
        // the kernel touches only caller-saved registers.
        unsafe { fast_mul_q8(a, b) }
    }
    #[cfg(not(target_arch = "mos"))]
    {
        ((a as i32 * b as i32) >> 8) as i16
    }
}

/// Rotation shared by every vertex of one mesh: yaw, pitch and roll are
/// combined once into a row-major 3×3 Q8 matrix, so each vertex costs nine
/// products and a single rounding. Entries are clamped to ±256 (rounding can
/// otherwise reach 257 near the axes), which lets the native kernel treat the
/// high byte of an entry as a sign-or-one flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct RotationMatrix {
    pub m: [[i16; 3]; 3],
}

impl RotationMatrix {
    /// Sine/cosine pairs for yaw, pitch and roll, in the kernel's input order.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn new(yaw: u8, pitch: u8, roll: u8) -> Self {
        // Six out-of-line sin8 calls: iterator forms and inlined lookups both
        // compiled to several hundred bytes more on the target.
        let sincos = [
            sin8(yaw),
            sin8(yaw.wrapping_add(64)),
            sin8(pitch),
            sin8(pitch.wrapping_add(64)),
            sin8(roll),
            sin8(roll.wrapping_add(64)),
        ];
        #[cfg(target_arch = "mos")]
        {
            extern "C" {
                fn fast_mesh_matrix(sincos: *const [i16; 6], out: *mut [[i16; 3]; 3]);
            }
            let mut m = [[0i16; 3]; 3];
            // SAFETY: the kernel performs exactly `compose` with the resident
            // product core; BOOT built its multiply table. It uses the free
            // scratch at $9100 that no interrupt touches.
            unsafe { fast_mesh_matrix(&sincos, &mut m) };
            Self { m }
        }
        #[cfg(not(target_arch = "mos"))]
        {
            Self::compose(&sincos)
        }
    }

    /// The reference composition: `M = R_roll · R_pitch · R_yaw` with the pair
    /// rotations the game always used — yaw on (z, x), pitch on (y, z), roll on
    /// (x, y) — each product rounded like `mul_q8`, then clamped to ±256.
    pub fn compose(s: &[i16; 6]) -> Self {
        let [sy, cy, sp, cp, sr, cr] = *s;
        let t1 = mul_q8(sp, sy);
        let t2 = mul_q8(sp, cy);
        let m = [
            [
                mul_q8(cr, cy).wrapping_sub(mul_q8(sr, t1)),
                mul_q8(sr, cp).wrapping_neg(),
                mul_q8(cr, sy).wrapping_add(mul_q8(sr, t2)),
            ],
            [
                mul_q8(sr, cy).wrapping_add(mul_q8(cr, t1)),
                mul_q8(cr, cp),
                mul_q8(sr, sy).wrapping_sub(mul_q8(cr, t2)),
            ],
            [mul_q8(cp, sy).wrapping_neg(), mul_q8(sp, 256), mul_q8(cp, cy)],
        ];
        Self {
            m: m.map(|row| row.map(|entry| entry.clamp(-256, 256))),
        }
    }

    /// Rotate a stored `i8` vertex: `out_i = (Σ_j m[i][j]·v_j) >> shift`, where
    /// `shift` is 8 minus the mesh's coordinate shift (plus one for half-size
    /// instances), so the result is in model units. Sums stay below 2^17.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.geometry")]
    pub fn apply(&self, v: [i8; 3], shift: u8) -> Vec3 {
        #[cfg(target_arch = "mos")]
        {
            extern "C" {
                fn fast_rotate_vertex(
                    vertex: *const [i8; 3],
                    matrix: *const [[i16; 3]; 3],
                    out: *mut Vec3,
                    shift: u8,
                );
            }
            let mut out = Vec3 { x: 0, y: 0, z: 0 };
            // SAFETY: the kernel performs exactly the host arithmetic below on
            // the repr(C) matrix and vertex; BOOT built its multiply table.
            unsafe { fast_rotate_vertex(&v, &self.m, &mut out, shift) };
            out
        }
        #[cfg(not(target_arch = "mos"))]
        {
            let dot = |row: &[i16; 3]| -> i16 {
                let sum = row[0] as i32 * v[0] as i32
                    + row[1] as i32 * v[1] as i32
                    + row[2] as i32 * v[2] as i32;
                (sum >> shift) as i16
            };
            Vec3 {
                x: dot(&self.m[0]),
                y: dot(&self.m[1]),
                z: dot(&self.m[2]),
            }
        }
    }
}

/// Quotient-only division uses the compact LLVM-MOS SDK runtime on the target.
/// Avoid computing a remainder alongside it: that selects a much larger helper.
/// A zero divisor has the deterministic saturated result `u32::MAX`.
#[inline(never)]
pub fn div_u32(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 {
        return u32::MAX;
    }
    numerator / denominator
}

/// Signed division truncates toward zero, with wrapping MIN/-1 semantics.
#[inline(never)]
pub fn div_i32(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        return if numerator < 0 { 1 } else { -1 };
    }
    if numerator == i32::MIN && denominator == -1 {
        return i32::MIN;
    }
    numerator / denominator
}

/// Exact quotient when denominator is nonzero and the quotient fits in u16.
/// Projection depths and raster edge fractions prove those bounds at the call.
/// Sixteen restoring steps are faster than the general 32-bit SDK routine.
#[inline(never)]
pub fn div_u32_u16(numerator: u32, denominator: u16) -> u16 {
    #[cfg(target_arch = "mos")]
    {
        extern "C" {
            fn fast_bounded_division(numerator: u32, denominator: u16) -> u16;
        }
        // SAFETY: callers prove the nonzero divisor and bounded quotient.
        // The kernel implements this C ABI and touches only volatile registers.
        unsafe { fast_bounded_division(numerator, denominator) }
    }
    #[cfg(not(target_arch = "mos"))]
    {
        let mut remainder = (numerator >> 16) as u16;
        let mut quotient = numerator as u16;
        let mut bits = 16u8;
        while bits != 0 {
            let overflow = remainder & 0x8000 != 0;
            remainder = (remainder << 1) | (quotient >> 15);
            quotient <<= 1;
            if overflow || remainder >= denominator {
                remainder = remainder.wrapping_sub(denominator);
                quotient |= 1;
            }
            bits -= 1;
        }
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn division_matches_native_full_width_arithmetic() {
        for numerator in [0, 1, 31, 65535, 65536, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
            for denominator in [
                1,
                2,
                3,
                17,
                65535,
                65536,
                0x7fff_ffff,
                0x8000_0000,
                u32::MAX,
            ] {
                assert_eq!(div_u32(numerator, denominator), numerator / denominator);
                let signed_n = numerator as i32;
                let signed_d = denominator as i32;
                assert_eq!(div_i32(signed_n, signed_d), signed_n.wrapping_div(signed_d));
            }
        }
        let mut state = 0xa581_7c39u32;
        for _ in 0..10_000 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let numerator = state;
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let denominator = state | 1;
            assert_eq!(div_u32(numerator, denominator), numerator / denominator);
            assert_eq!(
                div_i32(numerator as i32, denominator as i32),
                (numerator as i32).wrapping_div(denominator as i32)
            );
        }
        assert_eq!(div_u32(17, 0), u32::MAX);
        assert_eq!(div_i32(17, 0), -1);
        assert_eq!(div_i32(-17, 0), 1);
    }

    #[test]
    fn bounded_division_matches_full_width_arithmetic() {
        let mut state = 0xb7c9_1203u32;
        for _ in 0..10_000 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let denominator = (state as u16).max(1);
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let numerator = state % (u32::from(denominator) << 16);
            assert_eq!(
                div_u32_u16(numerator, denominator),
                (numerator / u32::from(denominator)) as u16
            );
        }
        for depth_sum in 0..=98_301u32 {
            assert_eq!(div_u32_u16(depth_sum, 3), (depth_sum / 3) as u16);
        }
    }
}
