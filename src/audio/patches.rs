//! Bounded synthesis programs and instruments, resident for every IRQ tick.
//! Frequency indices remove duplicate PAL/NTSC words to fund richer modulation.
//!
//! The v17 bank packs cutoffs to eight bits (the SID's low three cutoff bits
//! are inaudible), gives every voice its own pulse width, and moves the old
//! hard-coded program quirks into authored fields: a filter rate, a volume
//! punch, a gate retrigger tick and an accent kind. Every effect owns a whole
//! SID, so each instrument layers a dry transient, a tonal body and a filtered
//! or modulated tail across its three oscillators.
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub(super) struct Patch {
    pub notes: [u8; 3],
    pub waves: [u8; 3],
    pub ad: [u8; 3],
    pub sr: [u8; 3],
    /// Pulse width high byte per voice (low byte zero before PWM motion).
    pub pulse: [u8; 3],
    /// Initial, attack-target and body-target cutoff, each `11-bit >> 3`.
    pub cutoff: [u8; 3],
    pub attack: u8,
    pub release: u8,
    pub duration: u8,
    /// High nibble: SID mode; low three bits: voices routed into the filter.
    pub filter: u8,
    /// High nibble: resonance; low three bits: filter rate, the right shift
    /// applied to the remaining cutoff distance every tick (0 snaps).
    pub resonance: u8,
    /// High nibble: resonance after the program's timbre transition (zero
    /// preserves it); low nibble: punch ticks at full chip volume before the
    /// one-step-per-tick fall back to the mix level (zero: no accent).
    pub late_resonance: u8,
    /// Low nibble: mono/left program; high nibble: stereo right program.
    pub programs: u8,
}
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub(super) struct Program {
    /// Negative: `freq -= freq >> -slide` per tick; positive: `freq += freq >> slide`,
    /// saturating. `-1` halves the pitch every tick; `7` rises about 1 % per tick.
    pub slide: [i8; 3],
    pub pwm_rate: u8,
    pub pwm_depth: u8,
    pub change: u8,
    /// Zero retains the authored waveform. Noise always stays on its own voice.
    pub late_waves: [u8; 3],
    /// Tick whose gate is held low so the envelopes attack again; zero: none.
    pub retrigger: u8,
    /// Bits 0-1: pitch accent (see `ACCENT_*`); bit 2: move every voice's
    /// pulse width, not only pulse waveforms.
    pub accent: u8,
}
/// No pitch accent.
pub(super) const ACCENT_NONE: u8 = 0;
/// Every fourth tick the pitch steps up by an eighth (an arpeggio climb).
pub(super) const ACCENT_ARPEGGIO: u8 = 1;
/// Alternate four-tick groups render an octave lower (a siren toggle).
pub(super) const ACCENT_OCTAVE: u8 = 2;
/// Alternate two-tick groups render 1/32 above and below (a vibrato).
pub(super) const ACCENT_VIBRATO: u8 = 3;
/// Program flag: pulse-width motion applies to every voice.
pub(super) const ACCENT_PWM_ALL: u8 = 4;
pub(super) const FREQUENCY_SLOTS: usize = 24;
pub(super) const PROGRAM_COUNT: usize = 13;
const fn frequency_table() -> [[u16; 2]; FREQUENCY_SLOTS] {
    let notes = NOTES;
    let mut table = [[0; 2]; FREQUENCY_SLOTS];
    let mut i = 0;
    while i < notes.len() {
        table[i][0] = super::note_frequency(super::VideoStandard::Pal, notes[i]);
        table[i][1] = super::note_frequency(super::VideoStandard::Ntsc, notes[i]);
        i += 1;
    }
    table
}
// MIDI notes by slot. Deep thumps and subs sit in the first slots, the
// cannon and alarms around the fourth to sixth octaves, and the top slots
// double as bright noise rates. Slots 3 and 16 hold the cannon's E4 carrier
// and D5 modulator, so the table is unique rather than ascending.
//  0 G1   1 C2   2 G2   3 E4   4 C3   5 D3   6 G3   7 C4
//  8 D4   9 F4  10 F#4 11 G4  12 B4  13 C5  14 E5  15 G5
// 16 D5  17 B5  18 C6  19 C#6 20 D6  21 E6  22 G6  23 A#6
pub(super) const NOTES: [u8; FREQUENCY_SLOTS] = [
    31, 36, 43, 64, 48, 50, 55, 60, 62, 65, 66, 67, 71, 72, 76, 79, 74, 83, 84, 85, 86, 88, 91, 94,
];
// SID waveform bits: 16 triangle, 32 sawtooth, 64 pulse, 128 noise;
// +4 ring-modulates a triangle with the previous voice, +2 hard-syncs to it.
const TRI: u8 = 16;
const SAW: u8 = 32;
const PULSE: u8 = 64;
const NOISE: u8 = 128;
const RING: u8 = 4;
const SYNC: u8 = 2;
// Filter modes for the high nibble of `filter`; the low bits route voices.
const LP: u8 = 0x10;
const BP: u8 = 0x20;
const HP: u8 = 0x40;
/// Attack/decay and sustain/release nibbles in one byte.
const fn ad(attack: u8, decay: u8) -> u8 {
    (attack << 4) | decay
}
const fn sr(sustain: u8, release: u8) -> u8 {
    (sustain << 4) | release
}
/// Resonance (0..15) with the filter rate shift (0..7).
const fn res(resonance: u8, rate: u8) -> u8 {
    (resonance << 4) | rate
}
/// Late resonance (0 keeps it) with the punch tick count (0..15).
const fn late(resonance: u8, punch: u8) -> u8 {
    (resonance << 4) | punch
}
/// Left/mono and stereo-right program indices.
const fn programs(left: u8, right: u8) -> u8 {
    (right << 4) | left
}
// The native kernel (target-c64/asm/sfx.S) indexes these tables by effect,
// program and note; keep the exported names and the layouts asserted below.
#[cfg_attr(target_arch = "mos", link_section = ".sfx_patches")]
#[cfg_attr(target_arch = "mos", export_name = "sfx_frequencies")]
pub(super) static FREQUENCIES: [[u16; 2]; FREQUENCY_SLOTS] = frequency_table();
#[cfg_attr(target_arch = "mos", link_section = ".sfx_patches")]
#[cfg_attr(target_arch = "mos", export_name = "sfx_programs")]
pub(super) static PROGRAMS: [Program; PROGRAM_COUNT] = [
    // 0 cannon / footstep: the first voice drops a quarter per tick.
    Program {
        slide: [-2, 0, 0],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 0,
        accent: ACCENT_NONE,
    },
    // 1 enemy fire: two tonal voices zip upward, re-attacking once.
    Program {
        slide: [3, 3, 0],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 5,
        accent: ACCENT_NONE,
    },
    // 2 impact: a static noise crack, a falling metallic body and a thud.
    Program {
        slide: [0, -3, -2],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 0,
        accent: ACCENT_NONE,
    },
    // 3 explosion (left): noise and thump fall, the sub holds; a second
    // blast at tick five.
    Program {
        slide: [-4, -3, -6],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 5,
        accent: ACCENT_NONE,
    },
    // 4 explosion (right): decorrelated slides and a later second blast.
    Program {
        slide: [-3, -4, -7],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 6,
        accent: ACCENT_NONE,
    },
    // 5 cinematic: slow descent with pulse motion and a second detonation.
    Program {
        slide: [-6, -5, -4],
        pwm_rate: 13,
        pwm_depth: 3,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 8,
        accent: ACCENT_NONE,
    },
    // 6 rise: the tonal voices spin up; the pulse turns sawtooth midway.
    Program {
        slide: [0, 5, 4],
        pwm_rate: 11,
        pwm_depth: 3,
        change: 14,
        late_waves: [0, SAW, 0],
        retrigger: 0,
        accent: ACCENT_NONE,
    },
    // 7 siren: an octave toggle every four ticks, re-attacked at tick eight.
    Program {
        slide: [0, 0, 0],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 8,
        accent: ACCENT_OCTAVE,
    },
    // 8 chime: two plucks climbing an arpeggio with gentle pulse shimmer.
    Program {
        slide: [0, 0, 0],
        pwm_rate: 7,
        pwm_depth: 1,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 6,
        accent: ACCENT_ARPEGGIO,
    },
    // 9 wail: a falling vibrato with one re-attack.
    Program {
        slide: [-4, -4, -3],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 6,
        accent: ACCENT_VIBRATO,
    },
    // 10 swell up: slow rise with pulse motion and a second surge.
    Program {
        slide: [7, 7, 0],
        pwm_rate: 9,
        pwm_depth: 3,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 25,
        accent: ACCENT_NONE,
    },
    // 11 swell down: the departing transport.
    Program {
        slide: [-7, -7, 0],
        pwm_rate: 9,
        pwm_depth: 3,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 0,
        accent: ACCENT_NONE,
    },
    // 12 bird: a rising vibrato chirp, repeated once.
    Program {
        slide: [7, 0, 0],
        pwm_rate: 0,
        pwm_depth: 0,
        change: 255,
        late_waves: [0, 0, 0],
        retrigger: 7,
        accent: ACCENT_VIBRATO,
    },
];
#[cfg_attr(target_arch = "mos", link_section = ".sfx_patches")]
#[cfg_attr(target_arch = "mos", export_name = "sfx_patches")]
pub(super) static PATCHES: [Patch; 21] = [
    // Fire: a D5 pulse dropping a quarter per tick syncs an E4 carrier into
    // a deep "pew"; a two-frame noise click snaps the onset. Dry except the
    // carrier, whose low-pass closes behind it.
    Patch {
        notes: [16, 3, 23],
        waves: [PULSE, PULSE | SYNC, NOISE],
        ad: [ad(0, 3), ad(0, 2), ad(0, 1)],
        sr: [sr(8, 2), sr(6, 2), sr(0, 0)],
        pulse: [8, 8, 8],
        cutoff: [120, 160, 40],
        attack: 2,
        release: 5,
        duration: 7,
        filter: LP | 2,
        resonance: res(10, 1),
        late_resonance: late(0, 0),
        programs: programs(0, 0),
    },
    // EnemyFire: a sawtooth and a thin pulse zip upward, re-attacking once
    // into a ratchet, over a short noise tick.
    Patch {
        notes: [8, 4, 17],
        waves: [SAW, PULSE, NOISE],
        ad: [ad(0, 4), ad(0, 3), ad(0, 1)],
        sr: [sr(6, 3), sr(4, 2), sr(0, 1)],
        pulse: [8, 4, 8],
        cutoff: [90, 120, 50],
        attack: 3,
        release: 8,
        duration: 10,
        filter: LP | 2,
        resonance: res(8, 2),
        late_resonance: late(0, 0),
        programs: programs(1, 1),
    },
    // MissileLaunch: a band-passed noise whoosh opens while a pulse and a
    // triangle spin up; the pulse turns sawtooth as it climbs.
    Patch {
        notes: [14, 5, 11],
        waves: [NOISE, PULSE, TRI],
        ad: [ad(3, 6), ad(2, 4), ad(3, 6)],
        sr: [sr(10, 7), sr(10, 6), sr(8, 6)],
        pulse: [8, 4, 8],
        cutoff: [10, 120, 20],
        attack: 12,
        release: 24,
        duration: 36,
        filter: BP | 1,
        resonance: res(9, 3),
        late_resonance: late(0, 0),
        programs: programs(6, 6),
    },
    // EnemyHit: a heavy hull impact. A dry C3 pulse thud drops a quarter per
    // tick over a low-passed noise crunch at a C4 rate and a C2 triangle sub;
    // the filter closes fast and two punch ticks give it weight.
    Patch {
        notes: [4, 7, 1],
        waves: [PULSE, NOISE, TRI],
        ad: [ad(0, 5), ad(0, 5), ad(0, 6)],
        sr: [sr(0, 3), sr(2, 3), sr(0, 3)],
        pulse: [8, 8, 8],
        cutoff: [100, 60, 12],
        attack: 2,
        release: 9,
        duration: 14,
        filter: LP | 6,
        resonance: res(8, 1),
        late_resonance: late(0, 2),
        programs: programs(0, 0),
    },
    // Pickup: a C-E-G chord of plucks that re-attacks and climbs.
    Patch {
        notes: [13, 14, 15],
        waves: [PULSE, PULSE, TRI],
        ad: [ad(0, 6), ad(0, 6), ad(0, 6)],
        sr: [sr(8, 4), sr(8, 4), sr(8, 4)],
        pulse: [8, 8, 8],
        cutoff: [180, 220, 200],
        attack: 4,
        release: 12,
        duration: 16,
        filter: LP,
        resonance: res(0, 2),
        late_resonance: late(0, 1),
        programs: programs(8, 8),
    },
    // Roll: a band-pass whoosh sweeps up and back down through the noise
    // while a G3 pulse spins up with pulse motion and a ring-modulated C5
    // shimmers above it. Fits the 25-tick barrel roll.
    Patch {
        notes: [20, 6, 13],
        waves: [NOISE, PULSE, TRI | RING],
        ad: [ad(4, 8), ad(2, 4), ad(6, 6)],
        sr: [sr(10, 6), sr(12, 5), sr(8, 6)],
        pulse: [8, 8, 8],
        cutoff: [20, 180, 30],
        attack: 10,
        release: 20,
        duration: 26,
        filter: BP | 1,
        resonance: res(8, 2),
        late_resonance: late(0, 1),
        programs: programs(6, 6),
    },
    // Bomb: C2 pulse and G2 sawtooth sink under a bright noise rumble; the
    // low-pass opens fully then closes slowly; three punch ticks and a second
    // detonation at tick eight.
    Patch {
        notes: [1, 2, 14],
        waves: [PULSE, SAW, NOISE],
        ad: [ad(0, 8), ad(0, 8), ad(0, 9)],
        sr: [sr(10, 9), sr(8, 9), sr(6, 9)],
        pulse: [8, 8, 8],
        cutoff: [250, 255, 8],
        attack: 5,
        release: 48,
        duration: 85,
        filter: LP | 7,
        resonance: res(14, 4),
        late_resonance: late(0, 3),
        programs: programs(5, 5),
    },
    // EnemyExplosion: a G3-rate noise burst darkens into a rumble as its rate
    // falls, a dry C2 pulse thump drops an octave, a G1 sub holds under it;
    // the low-pass closes from fully open to about 850 Hz. Four punch ticks
    // and a re-blast on both sides.
    Patch {
        notes: [6, 1, 0],
        waves: [NOISE, PULSE, TRI],
        ad: [ad(0, 8), ad(0, 6), ad(0, 8)],
        sr: [sr(8, 7), sr(2, 5), sr(5, 6)],
        pulse: [8, 8, 8],
        cutoff: [255, 220, 18],
        attack: 4,
        release: 20,
        duration: 30,
        filter: LP | 5,
        resonance: res(11, 3),
        late_resonance: late(0, 4),
        programs: programs(3, 4),
    },
    // Footstep: a quarter-width C2 thud with a G1 triangle under it and a
    // short noise scuff, all low-passed.
    Patch {
        notes: [1, 7, 0],
        waves: [PULSE, NOISE, TRI],
        ad: [ad(0, 3), ad(0, 1), ad(0, 3)],
        sr: [sr(0, 2), sr(0, 1), sr(0, 2)],
        pulse: [4, 8, 8],
        cutoff: [30, 30, 10],
        attack: 2,
        release: 3,
        duration: 6,
        filter: LP | 3,
        resonance: res(8, 1),
        late_resonance: late(0, 0),
        programs: programs(0, 0),
    },
    // BirdCall: a G6 triangle chirps upward with vibrato and repeats; the
    // A#6 triangle answers; the third voice is silent.
    Patch {
        notes: [22, 23, 0],
        waves: [TRI, TRI, TRI],
        ad: [ad(1, 4), ad(1, 4), ad(0, 0)],
        sr: [sr(6, 3), sr(5, 3), sr(0, 0)],
        pulse: [8, 8, 8],
        cutoff: [180, 220, 200],
        attack: 2,
        release: 12,
        duration: 15,
        filter: LP,
        resonance: res(0, 2),
        late_resonance: late(0, 0),
        programs: programs(12, 12),
    },
    // Transport: a G2 sawtooth and D3 pulse swell upward through an opening
    // band-pass over a low noise rumble, surging again at tick 25.
    Patch {
        notes: [2, 5, 7],
        waves: [SAW, PULSE, NOISE],
        ad: [ad(8, 8), ad(8, 8), ad(8, 9)],
        sr: [sr(10, 8), sr(10, 8), sr(4, 8)],
        pulse: [8, 6, 8],
        cutoff: [8, 150, 30],
        attack: 25,
        release: 36,
        duration: 48,
        filter: BP | 7,
        resonance: res(10, 3),
        late_resonance: late(0, 0),
        programs: programs(10, 10),
    },
    // TransportAway: the same engine note sinking as the band-pass closes.
    Patch {
        notes: [2, 5, 7],
        waves: [SAW, PULSE, NOISE],
        ad: [ad(4, 8), ad(4, 8), ad(4, 9)],
        sr: [sr(10, 8), sr(10, 8), sr(4, 8)],
        pulse: [8, 6, 8],
        cutoff: [150, 90, 6],
        attack: 10,
        release: 36,
        duration: 48,
        filter: BP | 7,
        resonance: res(10, 3),
        late_resonance: late(0, 0),
        programs: programs(11, 11),
    },
    // Meteor: an incoming whistle, a falling F#4 sawtooth with vibrato over
    // band-passed noise and a sinking C3 triangle.
    Patch {
        notes: [20, 10, 4],
        waves: [NOISE, SAW, TRI],
        ad: [ad(2, 8), ad(2, 6), ad(2, 7)],
        sr: [sr(6, 7), sr(8, 6), sr(4, 6)],
        pulse: [8, 8, 8],
        cutoff: [20, 220, 15],
        attack: 10,
        release: 26,
        duration: 40,
        filter: BP | 3,
        resonance: res(11, 3),
        late_resonance: late(0, 0),
        programs: programs(9, 9),
    },
    // BossHit: the hull impact a fifth deeper and longer: a dry G2 pulse thud,
    // a G3-rate noise crunch and a G1 sub under a fast-closing low-pass;
    // three punch ticks.
    Patch {
        notes: [2, 6, 0],
        waves: [PULSE, NOISE, TRI],
        ad: [ad(0, 6), ad(0, 6), ad(0, 7)],
        sr: [sr(0, 4), sr(3, 4), sr(0, 4)],
        pulse: [8, 8, 8],
        cutoff: [90, 50, 8],
        attack: 3,
        release: 12,
        duration: 20,
        filter: LP | 6,
        resonance: res(9, 1),
        late_resonance: late(0, 3),
        programs: programs(0, 0),
    },
    // PlayerHit: a dry noise blast with a dissonant B4/F4 stab that drops
    // through a high-pass; two punch ticks.
    Patch {
        notes: [20, 12, 9],
        waves: [NOISE, SAW, PULSE],
        ad: [ad(0, 3), ad(0, 4), ad(0, 4)],
        sr: [sr(2, 2), sr(8, 3), sr(6, 3)],
        pulse: [8, 8, 8],
        cutoff: [40, 20, 60],
        attack: 3,
        release: 14,
        duration: 20,
        filter: HP | 6,
        resonance: res(10, 2),
        late_resonance: late(0, 2),
        programs: programs(2, 2),
    },
    // BossExplosion: a C3 sawtooth drags a hard-synced G3 pulse down in a
    // long power-down sweep over bright noise; the low-pass closes to a
    // rumble; three punch ticks and a second detonation.
    Patch {
        notes: [4, 6, 17],
        waves: [SAW, PULSE | SYNC, NOISE],
        ad: [ad(0, 8), ad(0, 8), ad(0, 9)],
        sr: [sr(8, 9), sr(8, 9), sr(6, 9)],
        pulse: [8, 8, 8],
        cutoff: [255, 255, 6],
        attack: 12,
        release: 60,
        duration: 100,
        filter: LP | 7,
        resonance: res(15, 4),
        late_resonance: late(0, 3),
        programs: programs(5, 5),
    },
    // PlayerDeath: a C4 sawtooth and a ring-modulated F#4 wail fall with
    // vibrato over noise as the low-pass closes; three punch ticks.
    Patch {
        notes: [7, 10, 20],
        waves: [SAW, TRI | RING, NOISE],
        ad: [ad(0, 8), ad(0, 8), ad(0, 9)],
        sr: [sr(8, 9), sr(7, 9), sr(6, 9)],
        pulse: [8, 8, 8],
        cutoff: [230, 255, 4],
        attack: 8,
        release: 42,
        duration: 80,
        filter: LP | 7,
        resonance: res(15, 3),
        late_resonance: late(0, 3),
        programs: programs(9, 9),
    },
    // MissileImpact: a mid-sized blast; noise and a G2 pulse thump with a C3
    // triangle under it, re-blasting once; two punch ticks.
    Patch {
        notes: [20, 2, 4],
        waves: [NOISE, PULSE, TRI],
        ad: [ad(0, 6), ad(0, 5), ad(0, 6)],
        sr: [sr(4, 5), sr(2, 4), sr(2, 4)],
        pulse: [8, 8, 8],
        cutoff: [200, 255, 30],
        attack: 3,
        release: 16,
        duration: 24,
        filter: LP | 5,
        resonance: res(10, 3),
        late_resonance: late(0, 2),
        programs: programs(3, 3),
    },
    // BossArrival: a C2 sawtooth and G2 pulse swell up through an opening
    // low-pass with a G3-rate rumble, surging again at tick 25.
    Patch {
        notes: [1, 2, 6],
        waves: [SAW, PULSE, NOISE],
        ad: [ad(8, 8), ad(8, 8), ad(8, 9)],
        sr: [sr(10, 8), sr(10, 8), sr(3, 8)],
        pulse: [8, 6, 8],
        cutoff: [6, 200, 60],
        attack: 24,
        release: 34,
        duration: 50,
        filter: LP | 7,
        resonance: res(14, 3),
        late_resonance: late(0, 0),
        programs: programs(10, 10),
    },
    // MissileReady: an E6 triangle bell with a ring-modulated C#6 partial and
    // a thin C6 pulse; the chime re-attacks and climbs.
    Patch {
        notes: [21, 19, 18],
        waves: [TRI, TRI | RING, PULSE],
        ad: [ad(0, 8), ad(0, 8), ad(0, 7)],
        sr: [sr(0, 4), sr(0, 4), sr(0, 4)],
        pulse: [8, 8, 4],
        cutoff: [180, 220, 200],
        attack: 2,
        release: 10,
        duration: 14,
        filter: LP,
        resonance: res(0, 2),
        late_resonance: late(0, 1),
        programs: programs(8, 8),
    },
    // LowShield: a B4 pulse siren toggling octaves, re-attacked at tick
    // eight, with a high-passed sawtooth and a noise tick behind it.
    Patch {
        notes: [12, 12, 20],
        waves: [PULSE, SAW, NOISE],
        ad: [ad(0, 2), ad(0, 2), ad(0, 1)],
        sr: [sr(10, 3), sr(8, 3), sr(0, 1)],
        pulse: [8, 8, 8],
        cutoff: [30, 40, 30],
        attack: 2,
        release: 14,
        duration: 18,
        filter: HP | 6,
        resonance: res(8, 1),
        late_resonance: late(0, 1),
        programs: programs(7, 7),
    },
];
pub(super) const BANK_BYTES: usize = core::mem::size_of::<[Patch; 21]>()
    + core::mem::size_of::<[[u16; 2]; FREQUENCY_SLOTS]>()
    + core::mem::size_of::<[Program; PROGRAM_COUNT]>();
const _: () = assert!(BANK_BYTES <= 768);
// Field offsets read by the native kernel.
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<Patch>() == 25 && size_of::<Program>() == 11);
    assert!(offset_of!(Patch, notes) == 0 && offset_of!(Patch, waves) == 3);
    assert!(offset_of!(Patch, ad) == 6 && offset_of!(Patch, sr) == 9);
    assert!(offset_of!(Patch, pulse) == 12 && offset_of!(Patch, cutoff) == 15);
    assert!(offset_of!(Patch, attack) == 18 && offset_of!(Patch, release) == 19);
    assert!(offset_of!(Patch, duration) == 20 && offset_of!(Patch, filter) == 21);
    assert!(offset_of!(Patch, resonance) == 22 && offset_of!(Patch, late_resonance) == 23);
    assert!(offset_of!(Patch, programs) == 24);
    assert!(offset_of!(Program, slide) == 0 && offset_of!(Program, pwm_rate) == 3);
    assert!(offset_of!(Program, pwm_depth) == 4 && offset_of!(Program, change) == 5);
    assert!(offset_of!(Program, late_waves) == 6 && offset_of!(Program, retrigger) == 9);
    assert!(offset_of!(Program, accent) == 10);
    // The kernel forms note * 4 + standard * 2 in one byte and indexes
    // programs through a sixteen-entry offset table.
    assert!(FREQUENCY_SLOTS * 4 <= 256 && PROGRAM_COUNT <= 16);
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_programs_stay_inside_the_native_kernel_contract() {
        assert_eq!(core::mem::size_of::<Patch>(), 25);
        assert_eq!(core::mem::size_of::<Program>(), 11);
        assert_eq!(BANK_BYTES, 21 * 25 + 24 * 4 + 13 * 11);
        let mut seen = [false; 128];
        for note in NOTES {
            assert!((24..=95).contains(&note) && !seen[note as usize], "unique playable notes");
            seen[note as usize] = true;
        }
        for p in PATCHES {
            assert!(p.notes.iter().all(|&n| (n as usize) < NOTES.len()));
            assert!(((p.programs & 15) as usize) < PROGRAMS.len());
            assert!(((p.programs >> 4) as usize) < PROGRAMS.len());
            assert!(p.release > 1 && p.release < p.duration);
            assert!(p.resonance & 8 == 0, "filter rate uses three bits");
            assert!(p.filter & 0x70 != 0 && p.filter & 8 == 0);
            assert!(
                p.late_resonance & 15 == 0 || (p.late_resonance & 15) < p.duration,
                "punch must end before the effect"
            );
            for wave in p.waves {
                valid_wave(wave);
            }
        }
        for program in PROGRAMS {
            assert!(program.slide.iter().all(|v| (-7..=7).contains(v)));
            assert!(matches!(program.pwm_depth, 0 | 1 | 3));
            assert!(program.accent & !7 == 0);
            assert!(program.retrigger == 0 || program.retrigger > 1);
            for wave in program.late_waves {
                valid_wave(wave);
            }
        }
    }
    #[test]
    fn all_pitched_trajectories_fit_without_saturation() {
        for standard in 0..2 {
            for (effect, patch) in PATCHES.iter().enumerate() {
                // Right-side mono uses the primary program, whereas stereo
                // right uses the accent. Backups share their channel's pitch.
                for (channel, program) in [
                    (0, patch.programs & 15),
                    (1, patch.programs & 15),
                    (1, patch.programs >> 4),
                ] {
                    let program = &PROGRAMS[program as usize];
                    for voice in 0..3 {
                        let base = FREQUENCIES[patch.notes[voice] as usize][standard];
                        let mut frequency = u32::from(base) + u32::from(base >> 9) * channel;
                        for age in 1..patch.duration {
                            let slide = program.slide[voice];
                            if slide < 0 {
                                frequency -= frequency >> -slide;
                            } else if slide > 0 {
                                frequency += frequency >> slide;
                            }
                            if program.accent & 3 == ACCENT_ARPEGGIO && age & 3 == 0 {
                                frequency += frequency >> 3;
                            }
                            let rendered = if program.accent & 3 == ACCENT_VIBRATO {
                                frequency + (frequency >> 5)
                            } else {
                                frequency
                            };
                            assert!(rendered <= u32::from(u16::MAX),
                                "effect {effect}, standard {standard}, channel {channel}, voice {voice}, age {age}: {frequency}");
                        }
                    }
                }
            }
        }
    }
    fn valid_wave(wave: u8) {
        assert_eq!(wave & 9, 0, "gate/test bits belong to the player");
        assert!(
            wave & 0x80 == 0 || wave == 0x80,
            "noise must not share a waveform"
        );
        assert!(
            wave & 4 == 0 || wave & 0x10 != 0,
            "ring modulation needs triangle"
        );
    }
}
