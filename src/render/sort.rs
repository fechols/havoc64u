//! Stable depth sorting with scratch shared with the completed projection pass.

use super::Triangle;

#[repr(C)]
#[cfg(not(target_arch = "mos"))]
struct Scratch {
    first: [u8; 256],
    second: [u8; 256],
    counts: [u16; 16],
    positions: [u16; 16],
    visited: [u8; 32],
    saved: Triangle,
}

#[cfg(not(target_arch = "mos"))]
const _: [(); 624] = [(); core::mem::size_of::<Scratch>()];

#[cfg(not(target_arch = "mos"))]
impl Scratch {
    const EMPTY: Self = Self {
        first: [0; 256],
        second: [0; 256],
        counts: [0; 16],
        positions: [0; 16],
        visited: [0; 32],
        saved: Triangle::EMPTY,
    };
}

#[inline(always)]
#[cfg(not(target_arch = "mos"))]
fn digit(depth: i16, shift: u8) -> usize {
    ((!(depth as u16 ^ 0x8000) >> shift) & 15) as usize
}

/// The logical scan order of the native sort: commands [prefix, pivot) first
/// (the foreground), then [0, prefix) (the scenery staged before it), then
/// the rest. Both zero means emission order. The pivot is modulo 256.
#[cfg(target_arch = "mos")]
static mut SORT_SEGMENTS: [u8; 2] = [0; 2];

#[cfg(target_arch = "mos")]
#[inline(always)]
pub(super) fn set_segments(prefix: u8, pivot: u8) {
    // SAFETY: GEOMETRY is the only writer and finish() the only reader, both
    // on the main thread.
    unsafe {
        SORT_SEGMENTS = [prefix, pivot];
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.shadow.final")]
pub(super) fn depth_sort(triangles: &mut [Triangle]) {
    #[cfg(target_arch = "mos")]
    {
        // The kernel leaves the commands in emission order and writes the
        // sorted order (command index per position) to $7600 for the raster
        // preparation — also for a single command, whose position 0 it
        // records; its key tables and buckets use the retired projection
        // cache and the rest of the pixel band, which PIXELS refills later.
        // Ties keep the logical order given by the scenery segments.
        extern "C" {
            fn fast_depth_sort(triangles: *mut Triangle, count: u16, segments: u16);
        }
        unsafe {
            let segments = u16::from_le_bytes(SORT_SEGMENTS);
            fast_depth_sort(triangles.as_mut_ptr(), triangles.len() as u16, segments);
        }
        // SHADOW is complete; the projection cache is retired until next frame.
    }
    #[cfg(not(target_arch = "mos"))]
    {
        if triangles.len() < 2 {
            return;
        }
        let mut scratch = Scratch::EMPTY;
        sort_with_scratch(triangles, &mut scratch);
    }
}

#[inline(never)]
#[cfg(not(target_arch = "mos"))]
fn sort_with_scratch(triangles: &mut [Triangle], scratch: &mut Scratch) {
    sort_indices(triangles, scratch);
    apply_order(triangles, scratch);
}

#[cfg(not(target_arch = "mos"))]
fn sort_indices(triangles: &[Triangle], scratch: &mut Scratch) {
    debug_assert!(triangles.len() <= 256);
    let count = triangles.len();
    for index in 0..count {
        scratch.first[index] = index as u8;
    }
    for pass in 0..4u8 {
        let (source, destination) = if pass & 1 == 0 {
            (&scratch.first, &mut scratch.second)
        } else {
            (&scratch.second, &mut scratch.first)
        };
        scratch.counts.fill(0);
        for &index in &source[..count] {
            scratch.counts[digit(triangles[index as usize].depth, pass * 4)] += 1;
        }
        let mut next = 0u16;
        for bucket in 0..16 {
            scratch.positions[bucket] = next;
            next += scratch.counts[bucket];
        }
        for &index in &source[..count] {
            let bucket = digit(triangles[index as usize].depth, pass * 4);
            destination[scratch.positions[bucket] as usize] = index;
            scratch.positions[bucket] += 1;
        }
    }
}

#[cfg(not(target_arch = "mos"))]
fn apply_order(triangles: &mut [Triangle], scratch: &mut Scratch) {
    let count = triangles.len();
    // Four passes leave destination->source indices in first. Each command is
    // moved at most once; all 256 index values are valid, so use visited bits.
    scratch.visited.fill(0);
    for start in 0..count {
        if scratch.visited[start >> 3] & (1 << (start & 7)) != 0 {
            continue;
        }
        scratch.saved = triangles[start];
        let mut destination = start;
        loop {
            scratch.visited[destination >> 3] |= 1 << (destination & 7);
            let source = scratch.first[destination] as usize;
            if source == start {
                triangles[destination] = scratch.saved;
                break;
            }
            triangles[destination] = triangles[source];
            destination = source;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    fn signatures(triangles: &[Triangle]) -> Vec<([i16; 3], [i16; 3], i16, u8, u8)> {
        triangles
            .iter()
            .map(|t| (t.x, t.y, t.depth, t.color, t.reserved))
            .collect()
    }

    #[test]
    fn full_capacity_radix_matches_stable_signed_depth_sort() {
        let mut random = 0x3198_7685u32;
        for count in [0, 1, 2, 15, 16, 17, 63, 127, 255, 256] {
            for pattern in 0..6 {
                let mut actual = Vec::new();
                for index in 0..count {
                    random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let depth = match pattern {
                        0 => 7,
                        1 => index as i16,
                        2 => -(index as i16),
                        3 => (index as i16 & 3) * 4096,
                        4 => [i16::MIN, i16::MAX, -1, 0][index & 3],
                        _ => random as i16,
                    };
                    actual.push(Triangle {
                        x: [index as i16, 99, -43],
                        y: [-17, index as i16, 200],
                        depth,
                        color: (index & 15) as u8,
                        reserved: index as u8,
                    });
                }
                let mut expected = actual.clone();
                expected.sort_by(|a, b| b.depth.cmp(&a.depth));
                depth_sort(&mut actual);
                assert_eq!(signatures(&actual), signatures(&expected));
                depth_sort(&mut actual);
                assert_eq!(signatures(&actual), signatures(&expected));
            }
        }
    }
}
