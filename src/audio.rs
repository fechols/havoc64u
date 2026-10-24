//! Four dedicated SID effects instruments; DAC music has separate ownership.
//! The IRQ ticks at 50 Hz without REU access or calls into overlays.
pub const SID_COUNT: usize = 4;
pub const CUE_COUNT: usize = 21;
pub const SID_BASES: [u16; SID_COUNT] = [0xd400, 0xd420, 0xd440, 0xd460];
pub trait SidBus {
    fn write(&mut self, sid: u8, reg: u8, value: u8);
}
mod effects;
mod patches;
pub(crate) use effects::queue_events;
pub use effects::{AudioEvent, Effect, EffectsEngine, EFFECT_COUNT, PRIORITIES};

/// Foreground routing for x_screen = 80 + 80*x/z: the middle eight pixels
/// acquire a stereo pair. Behind/at the camera, retain the source's side.
#[inline(never)]
pub fn spatial_pan(x: i16, z: i16) -> i8 {
    if x == 0 {
        return 0;
    }
    let side = if x < 0 { -100 } else { 100 };
    if z < 0 {
        return side;
    }
    let magnitude = x.unsigned_abs();
    // A centered coordinate cannot exceed 1638 in i16 depth. This cheap
    // high-byte bound makes the shift/add safe without a native divider.
    if (magnitude >> 8) as u8 >= 8 {
        return side;
    }
    if ((magnitude << 2) + magnitude) << 2 <= z as u16 {
        0
    } else {
        side
    }
}

#[cfg(test)]
mod spatial_tests {
    use super::spatial_pan;

    #[test]
    fn center_is_five_percent_at_every_depth_and_extremes_cannot_wrap() {
        for depth in [20, 180, 400, 2800, 32760] {
            let edge = depth / 20;
            for sign in [-1, 1] {
                assert_eq!(spatial_pan(sign * edge, depth), 0);
                assert_eq!(spatial_pan(sign * (edge + 1), depth), (sign * 100) as i8);
            }
        }
        for depth in [i16::MIN, -1, 0, 1, 8, i16::MAX] {
            assert_eq!(spatial_pan(i16::MIN, depth), -100);
            assert_eq!(spatial_pan(i16::MAX, depth), 100);
            assert_eq!(spatial_pan(0, depth), 0);
        }
        assert_eq!(spatial_pan(-1, 0), -100);
        assert_eq!(spatial_pan(1, -1), 100);
    }

    #[test]
    fn bounded_routing_matches_wide_projection_arithmetic_for_every_input_value() {
        let reference = |x: i16, z: i16| {
            if i32::from(x).abs() * 20 <= i32::from(z.max(0)) {
                0
            } else if x < 0 {
                -100
            } else {
                100
            }
        };
        for x in i16::MIN..=i16::MAX {
            let boundary = (i32::from(x).abs() * 20).min(i32::from(i16::MAX)) as i16;
            for z in [
                i16::MIN,
                -1,
                0,
                1,
                19,
                20,
                180,
                400,
                2800,
                i16::MAX,
                boundary.saturating_sub(1),
                boundary,
                boundary.saturating_add(1),
            ] {
                assert_eq!(spatial_pan(x, z), reference(x, z), "x{x}, z{z}");
            }
        }
        for z in i16::MIN..=i16::MAX {
            let edge = z.max(0) / 20;
            for x in [-edge - 1, -edge, 0, edge, edge + 1] {
                assert_eq!(spatial_pan(x, z), reference(x, z), "x{x}, z{z}");
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VideoStandard {
    Pal,
    Ntsc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MusicState {
    Title = 0,
    Lowlands = 1,
    Canyon = 2,
    Fortress = 3,
    Carrier = 4,
    Victory = 5,
    Defeat = 6,
    CrystalCurrent = 7,
    AshesAndSteel = 8,
    TempestDrive = 9,
    GorgeWarden = 10,
    CitadelHeart = 11,
    CarrierOverdrive = 12,
    AsteroidBelt = 13,
    ShatteredMoon = 14,
    ShipGraveyard = 15,
    OrbitalShipyard = 16,
    SpaceCarrier = 17,
    JumpGate = 18,
    Ascent = 19,
    GateGuardian = 20,
}

impl MusicState {
    /// Decode authored stage metadata without accepting an invalid enum value.
    pub const fn from_cue(cue: u8) -> Option<Self> {
        if cue <= Self::GateGuardian as u8 {
            // SAFETY: repr(u8) has every consecutive discriminant 0..=20.
            // Keep this bound tied to the final variant; decoding all 256
            // possible bytes is covered by the authored-suite playback test.
            Some(unsafe { core::mem::transmute::<u8, Self>(cue) })
        } else {
            None
        }
    }
}

// Rounded C6..B6 SID register values: Hz * 2^24 / native SID clock.
// These use the VIC/SID clocks (PAL 985248 Hz, NTSC 1022727 Hz), never turbo CPU Hz.
const PAL_OCTAVE: [u16; 12] = [
    17820, 18880, 20003, 21192, 22452, 23787, 25202, 26700, 28288, 29970, 31752, 33640,
];
const NTSC_OCTAVE: [u16; 12] = [
    17167, 18188, 19270, 20415, 21629, 22916, 24278, 25722, 27251, 28872, 30589, 32407,
];

// Expand the original rounded-octave calculation at compile time. This trades
// spare constant bytes for smaller, faster resident IRQ code on the target.
const fn expand_notes(octave: [u16; 12]) -> [u16; 72] {
    let mut notes = [0; 72];
    let mut relative = 0;
    while relative < notes.len() {
        let value = octave[relative % 12];
        let shift = 5 - relative / 12;
        notes[relative] = if shift == 0 {
            value
        } else {
            (value + (1 << (shift - 1))) >> shift
        };
        relative += 1;
    }
    notes
}

static PAL_NOTES: [u16; 72] = expand_notes(PAL_OCTAVE);
static NTSC_NOTES: [u16; 72] = expand_notes(NTSC_OCTAVE);

/// Return a native-clock SID frequency for MIDI notes 24..=95; zero otherwise.
#[inline(never)]
pub const fn note_frequency(standard: VideoStandard, note: u8) -> u16 {
    let relative = note.wrapping_sub(24);
    if relative >= 72 {
        return 0;
    }
    let notes = match standard {
        VideoStandard::Pal => &PAL_NOTES,
        VideoStandard::Ntsc => &NTSC_NOTES,
    };
    // The unsigned range check also rejects notes below 24 after wrapping.
    notes[relative as usize]
}
