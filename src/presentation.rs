//! Small display effects whose clock is independent of scene rendering.

// Each sample lasts two 20-ms audio ticks. The first byte packs VIC fine
// scroll: X in bits 0..2, absolute Y in bits 3..5. Idle Y is three.
const BOMB_SAMPLES: [[u8; 2]; 16] = [
    [6 | (6 << 3), 1],
    [0, 1],
    [6 | (1 << 3), 7],
    [6 << 3, 7],
    [4 | (5 << 3), 8],
    [1 << 3, 8],
    [4 | (2 << 3), 2],
    [5 << 3, 2],
    [2 | (4 << 3), 0],
    [2 << 3, 0],
    [2 | (2 << 3), 0],
    [4 << 3, 0],
    [2 | (3 << 3), 0],
    [2 << 3, 0],
    [2 | (3 << 3), 0],
    [4 << 3, 0],
];
// A player hit: 240 ms of two-pixel kicks with an 80-ms red border flash.
const HIT_SAMPLES: [[u8; 2]; 6] = [
    [2 | (4 << 3), 2],
    [2 << 3, 2],
    [2 | (3 << 3), 0],
    [4 << 3, 0],
    [2 | (3 << 3), 0],
    [2 << 3, 0],
];
const BOMB_TICKS: u8 = 32;
const HIT_TICKS: u8 = 12;
const HIT_PROFILE: u8 = 0x80;

/// A whole-display impulse: the 640-ms bomb shake with a single 320-ms warm
/// border flash, or the 240-ms player-hit shake with a red flash. One byte
/// holds the remaining ticks and, in bit 7, the hit profile. Only the fixed
/// 50-Hz audio clock advances it; display sampling is read-only.
#[derive(Clone, Copy, Default)]
#[repr(transparent)]
pub struct BombEffect {
    remaining: u8,
}

impl BombEffect {
    pub const fn new() -> Self {
        Self { remaining: 0 }
    }

    #[inline(always)]
    pub fn trigger(&mut self) {
        self.remaining = BOMB_TICKS;
    }

    /// A bomb shake already in progress outranks a hit; a hit restarts a hit.
    #[inline(always)]
    pub fn trigger_hit(&mut self) {
        if self.remaining == 0 || self.remaining & HIT_PROFILE != 0 {
            self.remaining = HIT_PROFILE | HIT_TICKS;
        }
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.remaining = 0;
    }

    #[inline(always)]
    pub fn tick(&mut self) {
        if self.remaining & !HIT_PROFILE != 0 {
            self.remaining -= 1;
            if self.remaining & !HIT_PROFILE == 0 {
                self.remaining = 0;
            }
        }
    }

    /// Return packed fine scroll followed by the C64 border palette index.
    #[inline(always)]
    pub fn sample(&self) -> [u8; 2] {
        if self.remaining == 0 {
            [3 << 3, 0]
        } else if self.remaining & HIT_PROFILE != 0 {
            // The mask proves the bound to the native compiler, as below.
            HIT_SAMPLES[(((HIT_TICKS - (self.remaining & !HIT_PROFILE)) >> 1) % 6) as usize]
        } else {
            // The mask also proves the bound to the native compiler, avoiding
            // an array bounds-check branch in the resident IRQ path.
            BOMB_SAMPLES[(((BOMB_TICKS - self.remaining) >> 1) & 15) as usize]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_expires_after_640_ms_and_restores_exact_idle_register_values() {
        let mut effect = BombEffect::new();
        assert_eq!(core::mem::size_of::<BombEffect>(), 1);
        assert_eq!(effect.sample(), [24, 0]);
        effect.trigger();
        for tick in 0..32 {
            assert_eq!(effect.remaining, 32 - tick);
            assert_ne!(effect.sample(), [24, 0]);
            effect.tick();
        }
        for _ in 0..100 {
            assert_eq!(effect.sample(), [24, 0]);
            effect.tick();
        }
    }

    #[test]
    fn border_has_one_warm_flash_then_stays_black_while_shake_tapers() {
        let mut effect = BombEffect::new();
        effect.trigger();
        for tick in 0..32 {
            let [scroll, border] = effect.sample();
            let expected = match tick {
                0..=3 => 1,
                4..=7 => 7,
                8..=11 => 8,
                12..=15 => 2,
                _ => 0,
            };
            assert_eq!(border, expected);
            let x = scroll & 7;
            let y = (scroll >> 3) as i8 - 3;
            assert_eq!(x & 1, 0, "preserve multicolor pixel alignment");
            let (max_x, max_y) = match tick {
                0..=7 => (6, 3),
                8..=15 => (4, 2),
                _ => (2, 1),
            };
            assert!(x <= max_x && y.abs() <= max_y);
            effect.tick();
        }
    }

    #[test]
    fn repeated_display_samples_do_not_advance_the_audio_clock() {
        let mut effect = BombEffect::new();
        effect.trigger();
        for _ in 0..16 {
            let sample = effect.sample();
            for _ in 0..60 {
                assert_eq!(effect.sample(), sample);
            }
            effect.tick();
            assert_eq!(effect.sample(), sample);
            effect.tick();
        }
        assert_eq!(effect.sample(), [24, 0]);
    }

    #[test]
    fn retrigger_restarts_the_impulse_and_reset_clears_it_immediately() {
        let mut effect = BombEffect::new();
        effect.trigger();
        let initial = effect.sample();
        for _ in 0..20 {
            effect.tick();
        }
        assert_ne!(effect.sample(), initial);
        effect.trigger();
        assert_eq!(effect.remaining, 32);
        assert_eq!(effect.sample(), initial);
        effect.reset();
        assert_eq!(effect.sample(), [24, 0]);
        effect.tick();
        assert_eq!(effect.sample(), [24, 0]);
    }

    #[test]
    fn player_hit_is_a_short_two_pixel_shake_with_a_red_flash() {
        let mut effect = BombEffect::new();
        effect.trigger_hit();
        for tick in 0..12 {
            let [scroll, border] = effect.sample();
            assert_eq!(border, if tick < 4 { 2 } else { 0 }, "tick {tick}");
            let x = scroll & 7;
            let y = (scroll >> 3) as i8 - 3;
            assert_eq!(x & 1, 0, "preserve multicolor pixel alignment");
            assert!(x <= 2 && y.abs() <= 1, "tick {tick}: x {x} y {y}");
            assert_ne!(effect.sample(), [24, 0]);
            effect.tick();
        }
        assert_eq!(effect.sample(), [24, 0]);
        assert_eq!(effect.remaining, 0);
        for _ in 0..40 {
            effect.tick();
            assert_eq!(effect.sample(), [24, 0]);
        }
    }

    #[test]
    fn a_bomb_shake_outranks_hits_and_a_hit_restarts_a_hit() {
        let mut effect = BombEffect::new();
        effect.trigger();
        let bomb = effect.sample();
        effect.trigger_hit();
        assert_eq!(effect.sample(), bomb, "a hit must not cut the bomb shake short");
        effect.reset();
        effect.trigger_hit();
        for _ in 0..8 {
            effect.tick();
        }
        effect.trigger_hit();
        assert_eq!(effect.remaining & 0x7f, 12);
        assert_eq!(effect.sample()[1], 2, "the hit flash restarts");
        effect.trigger();
        assert_eq!(effect.sample(), bomb, "a bomb replaces a hit");
    }
}
