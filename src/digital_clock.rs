//! Exact sampler time and stereo REU-ring frame arithmetic shared with host tests.
//! All out-of-line target work belongs to the foreground AUDIO overlay.
//! Sample counters count stereo frames: one left/right pair at 24 kHz.

pub const CALIBRATION_SAMPLES: u32 = 12_288;
pub const RING_SAMPLES: u32 = 32_768;
pub const GUARD_SAMPLES: u32 = 2_048;
pub const REBASE_SAMPLES: u32 = 0x0100_0000;
const MAX_TICKS: u32 = 262_144;

/// Private division avoids pulling the SDK's large generic divmod helper into
/// resident RAM. No multiplication, division or variable-width shift is used.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
pub fn quotient_remainder(mut numerator: u32, denominator: u32) -> (u32, u32) {
    if denominator == 0 {
        return (u32::MAX, numerator);
    }
    let mut divisor = denominator;
    let mut bit = 1u32;
    while divisor <= (numerator >> 1) {
        divisor <<= 1;
        bit <<= 1;
    }
    let mut quotient = 0;
    while bit != 0 {
        if numerator >= divisor {
            numerator -= divisor;
            quotient |= bit;
        }
        divisor >>= 1;
        bit >>= 1;
    }
    (quotient, numerator)
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SampleClock {
    pub samples: u32,
    remainder: u32,
    calibration_ticks: u32,
}

impl SampleClock {
    pub const fn new() -> Self {
        Self {
            samples: 0,
            remainder: 0,
            calibration_ticks: 1,
        }
    }

    /// Bounds cover both supported bus clocks at the fixed 24 kHz rate and
    /// leave ample room for a faulty initial sampler-divider estimate.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
    pub fn calibrate(&mut self, ticks: u32) -> bool {
        if !(250_000..=1_100_000).contains(&ticks) {
            return false;
        }
        self.samples = 0;
        self.remainder = 0;
        self.calibration_ticks = ticks;
        true
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
    pub fn advance(&mut self, mut ticks: u32) {
        while ticks != 0 {
            let part = ticks.min(MAX_TICKS);
            // 12288 = 8192 + 4096. At MAX_TICKS the product is $C0000000;
            // adding the validated remainder cannot overflow u32.
            let scaled = (part << 13) + (part << 12) + self.remainder;
            let (whole, remainder) = quotient_remainder(scaled, self.calibration_ticks);
            self.samples = self.samples.wrapping_add(whole);
            self.remainder = remainder;
            ticks -= part;
        }
    }

    /// Both counters lose the same whole-ring multiple, preserving the DMA
    /// address, occupancy and fractional clock phase during indefinite loops.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
    pub fn rebase(&mut self, produced: &mut u32) {
        while self.samples >= REBASE_SAMPLES && *produced >= REBASE_SAMPLES {
            self.samples -= REBASE_SAMPLES;
            *produced -= REBASE_SAMPLES;
        }
    }
}

#[inline(always)]
pub fn underrun(produced: u32, consumed: u32) -> bool {
    consumed + GUARD_SAMPLES >= produced
}

#[inline(always)]
pub fn refill_samples(produced: u32, consumed: u32, maximum: u16) -> u16 {
    let limit = consumed + RING_SAMPLES - GUARD_SAMPLES;
    if produced >= limit {
        return 0;
    }
    let free = limit - produced;
    // Leave one batch of hysteresis. Reading the timer, dividing its exact
    // ratio and returning through the service loop also consume real time;
    // chasing every final sample would never finish on the native CPU.
    if free < maximum as u32 {
        return 0;
    }
    let until_wrap = RING_SAMPLES - (produced & (RING_SAMPLES - 1));
    free.min(until_wrap).min(maximum as u32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_division_matches_native_including_extremes() {
        let mut random = 0x79a3_1234u32;
        for denominator in [
            1,
            2,
            1937,
            2011,
            504_447,
            523_636,
            1_100_000,
            0x8000_0000,
            u32::MAX,
        ] {
            for numerator in [0, 1, denominator - 1, denominator, u32::MAX] {
                assert_eq!(
                    quotient_remainder(numerator, denominator),
                    (numerator / denominator, numerator % denominator)
                );
            }
            for _ in 0..1000 {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                assert_eq!(
                    quotient_remainder(random, denominator),
                    (random / denominator, random % denominator)
                );
            }
        }
    }

    #[test]
    fn exact_ratio_is_independent_of_service_cadence() {
        for calibration in [504_447, 523_636, 501_003] {
            let mut fast = SampleClock::new();
            let mut slow = SampleClock::new();
            assert!(fast.calibrate(calibration));
            assert!(slow.calibrate(calibration));
            let total = 543_780_000u32;
            for _ in 0..54_378 {
                fast.advance(10_000);
            }
            slow.advance(total);
            let exact = total as u64 * CALIBRATION_SAMPLES as u64 / calibration as u64;
            assert_eq!(fast.samples as u64, exact);
            assert_eq!(slow.samples, fast.samples);
            assert_eq!(slow.remainder, fast.remainder);
        }
    }

    #[test]
    fn rebases_keep_ring_positions_and_fractional_phase_for_days() {
        let mut clock = SampleClock::new();
        assert!(clock.calibrate(504_447));
        let mut elapsed = 0u64;
        let mut removed = 0u64;
        for _ in 0..200_000 {
            clock.advance(985_248);
            elapsed += 985_248;
            let mut produced = clock.samples + RING_SAMPLES - GUARD_SAMPLES;
            let before = produced & (RING_SAMPLES - 1);
            let old = clock.samples;
            clock.rebase(&mut produced);
            removed += (old - clock.samples) as u64;
            assert_eq!(produced & (RING_SAMPLES - 1), before);
            assert_eq!(produced - clock.samples, RING_SAMPLES - GUARD_SAMPLES);
            assert_eq!(
                clock.samples as u64 + removed,
                elapsed * CALIBRATION_SAMPLES as u64 / 504_447
            );
        }
    }

    #[test]
    fn stereo_refills_respect_reader_guard_and_ring_wrap() {
        assert_eq!(refill_samples(RING_SAMPLES, 0, 256), 0);
        assert_eq!(refill_samples(RING_SAMPLES, GUARD_SAMPLES + 77, 256), 0);
        assert_eq!(refill_samples(RING_SAMPLES, GUARD_SAMPLES + 255, 256), 0);
        assert_eq!(refill_samples(RING_SAMPLES, GUARD_SAMPLES + 256, 256), 256);
        assert_eq!(refill_samples(65_530, 40_000, 256), 6);
        assert!(!underrun(RING_SAMPLES, RING_SAMPLES - GUARD_SAMPLES - 1));
        assert!(underrun(RING_SAMPLES, RING_SAMPLES - GUARD_SAMPLES));
        assert_eq!(((RING_SAMPLES - 1) << 2) + 3, 131_071);
        assert_eq!(refill_samples(RING_SAMPLES, GUARD_SAMPLES + 127, 128), 0);
        assert_eq!(refill_samples(RING_SAMPLES, GUARD_SAMPLES + 128, 128), 128);
    }
}
