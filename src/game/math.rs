//! Small collision kernels with identical host and 6502 behavior.

/// The inclusive segment overlaps the target interval. Bounds wrap exactly as
/// release-mode 16-bit arithmetic; ordinary gameplay coordinates cannot wrap.
#[inline(always)]
pub(super) fn swept_z(from: i16, to: i16, target: i16, radius: i16) -> bool {
    #[cfg(target_arch = "mos")]
    unsafe {
        extern "C" {
            fn fast_swept_z(from: i16, to: i16, target: i16, radius: i16) -> bool;
        }
        // The assembly implements this C ABI, uses only volatile registers,
        // and returns exactly zero or one for every combination of i16 inputs.
        fast_swept_z(from, to, target, radius)
    }
    #[cfg(not(target_arch = "mos"))]
    {
        from.min(to) <= target.wrapping_add(radius) && from.max(to) >= target.wrapping_sub(radius)
    }
}
