//! Resident, allocation-free dual-SID score and sound-effects player.
//!
//! Call `reset` during startup, then `tick` at 50 Hz from the audio timer,
//! independently of video frames. Reset/score validation live in the C64's
//! BOOT overlay; reload that overlay before invoking either after startup.
//! The main thread must mask that interrupt while calling `queue`, `set_music`,
//! or `load_score`; the engine deliberately has no global or implicit locking.
//! SID 0 is $D400 (left), SID 1 is $D420 (right). Each chip's third voice is
//! reserved for effects and is never routed through the music-owned filter.

pub const SID_BASES: [u16; 2] = [0xd400, 0xd420];
pub const SCORE_LENGTH: usize = 349;
pub const SCORE_ASSET_LENGTH: usize = 512;
const INSTRUMENT_OFFSET: usize = 9;
const PATTERN_OFFSET: usize = 29;
const QUEUE_CAPACITY: usize = 4;

pub trait SidBus {
    fn write(&mut self, sid: u8, reg: u8, value: u8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoStandard {
    Pal,
    Ntsc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MusicState {
    Exploration = 0,
    Combat = 1,
    Boss = 2,
    Victory = 3,
    Defeat = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Fire,
    EnemyFire,
    ChargedShot,
    Hit,
    Pickup,
    Roll,
    Bomb,
    Death,
}

impl Effect {
    pub const fn priority(self) -> u8 {
        match self {
            Self::Death | Self::Bomb => 4,
            Self::Hit | Self::ChargedShot => 3,
            Self::Pickup | Self::Roll => 2,
            Self::Fire | Self::EnemyFire => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioEvent {
    pub effect: Effect,
    /// Below -32: left; above 32: right; otherwise both reserved effects voices.
    pub pan: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreError {
    Truncated,
    Signature,
    Tempo,
    Instrument,
    Note,
}

#[derive(Clone, Copy)]
struct FxVoice {
    frequency: u16,
    effect: Effect,
    remaining: u8,
    age: u8,
}

const EMPTY_FX: FxVoice = FxVoice {
    frequency: 0,
    effect: Effect::Fire,
    remaining: 0,
    age: 0,
};
const EMPTY_EVENT: AudioEvent = AudioEvent {
    effect: Effect::Fire,
    pan: 0,
};

/// Full score, SID shadows, effect voices and queue occupy less than 512 bytes.
pub struct AudioEngine {
    score: [u8; SCORE_LENGTH],
    registers: [[u8; 25]; 2],
    effects: [FxVoice; 2],
    events: [AudioEvent; QUEUE_CAPACITY],
    standard: VideoStandard,
    music: MusicState,
    row: u8,
    phrase: u8,
    row_ticks: u8,
    event_count: u8,
    loaded: bool,
    initialized: bool,
    enabled: bool,
    restart_music: bool,
}

impl AudioEngine {
    pub const fn new(standard: VideoStandard) -> Self {
        Self {
            score: [0; SCORE_LENGTH],
            registers: [[0; 25]; 2],
            effects: [EMPTY_FX; 2],
            events: [EMPTY_EVENT; QUEUE_CAPACITY],
            standard,
            music: MusicState::Exploration,
            row: 0,
            phrase: 0,
            row_ticks: 0,
            event_count: 0,
            loaded: false,
            initialized: false,
            enabled: true,
            restart_music: false,
        }
    }

    /// Validate before replacing the resident score. Padding after byte 348 is
    /// ignored, so the REU asset can use a convenient 512-byte allocation.
    ///
    /// Layout: SMU1; five 50-Hz row durations; four instruments of waveform,
    /// pulse low/high, attack/decay, sustain/release; five 16-row patterns with
    /// four MIDI notes per row. Zero is a rest; supported notes are 24..=95.
    /// Defeat uses its asset pattern as the opening phrase, followed by seven
    /// resident continuation phrases without expanding the mutable score bank.
    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    #[inline(never)]
    pub fn load_score(&mut self, bytes: &[u8]) -> Result<(), ScoreError> {
        if bytes.len() < SCORE_LENGTH {
            return Err(ScoreError::Truncated);
        }
        if &bytes[..4] != b"SMU1" {
            return Err(ScoreError::Signature);
        }
        for &tempo in &bytes[4..9] {
            if !(3..=50).contains(&tempo) {
                return Err(ScoreError::Tempo);
            }
        }
        for voice in 0..4 {
            let offset = INSTRUMENT_OFFSET + voice * 5;
            if !matches!(bytes[offset], 0x10 | 0x20 | 0x40 | 0x80) || bytes[offset + 2] > 15 {
                return Err(ScoreError::Instrument);
            }
        }
        for &note in &bytes[PATTERN_OFFSET..SCORE_LENGTH] {
            if note != 0 && !(24..=95).contains(&note) {
                return Err(ScoreError::Note);
            }
        }
        self.score.copy_from_slice(&bytes[..SCORE_LENGTH]);
        self.loaded = true;
        self.restart_music = true;
        Ok(())
    }

    pub fn set_music(&mut self, state: MusicState) {
        if state != self.music {
            self.music = state;
            self.restart_music = true;
        }
    }

    pub const fn music(&self) -> MusicState {
        self.music
    }

    /// Queue an effect without affecting the four music voices. Equal-priority
    /// effects replace older effects; higher-priority effects retain their voice.
    /// Repeated pending gunfire in the same stereo lane is coalesced.
    pub fn queue(&mut self, event: AudioEvent) -> bool {
        if !self.enabled {
            return false;
        }
        if matches!(event.effect, Effect::Fire | Effect::EnemyFire) {
            for pending in &self.events[..self.event_count as usize] {
                if pending.effect == event.effect && lane(pending.pan) == lane(event.pan) {
                    return true;
                }
            }
        }
        if self.event_count as usize == QUEUE_CAPACITY {
            let mut lowest = 0;
            for index in 1..QUEUE_CAPACITY {
                if self.events[index].effect.priority() < self.events[lowest].effect.priority() {
                    lowest = index;
                }
            }
            if event.effect.priority() < self.events[lowest].effect.priority() {
                return false;
            }
            for index in lowest..QUEUE_CAPACITY - 1 {
                self.events[index] = self.events[index + 1];
            }
            self.event_count -= 1;
        }
        self.events[self.event_count as usize] = event;
        self.event_count += 1;
        true
    }

    /// Clear all hardware registers and restart the selected score from row 0.
    /// Volume is set once; there is no volume-register sample playback.
    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    #[inline(never)]
    pub fn reset<B: SidBus>(&mut self, bus: &mut B) {
        self.effects = [EMPTY_FX; 2];
        self.event_count = 0;
        self.row = 0;
        self.phrase = 0;
        self.row_ticks = 0;
        self.restart_music = false;
        self.enabled = true;
        self.initialize(bus);
    }

    /// Silence both chips until `reset` is called. No queued effects survive.
    pub fn silence<B: SidBus>(&mut self, bus: &mut B) {
        self.event_count = 0;
        self.effects = [EMPTY_FX; 2];
        self.enabled = false;
        for sid in 0..2 {
            self.write(bus, sid, 4, 0);
            self.write(bus, sid, 11, 0);
            self.write(bus, sid, 18, 0);
            self.write(bus, sid, 24, 0);
        }
    }

    /// One 20 ms audio tick. Does not allocate, read REU, or depend on rendering.
    pub fn tick<B: SidBus>(&mut self, bus: &mut B) {
        if !self.enabled || !self.initialized {
            return;
        }
        if self.loaded {
            self.tick_music(bus);
        }
        self.tick_effects(bus);
        for index in 0..self.event_count as usize {
            self.start_effect(bus, self.events[index]);
        }
        self.event_count = 0;
    }

    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    #[inline(never)]
    fn initialize<B: SidBus>(&mut self, bus: &mut B) {
        for sid in 0..2 {
            for reg in 0..25 {
                bus.write(sid as u8, reg as u8, 0);
                // Loop bounds exactly cover the two 25-register shadows.
                unsafe {
                    *self.registers.get_unchecked_mut(sid).get_unchecked_mut(reg) = 0;
                }
            }
            self.write(bus, sid, 21, 0);
            self.write(bus, sid, 22, 0x80);
            self.write(bus, sid, 23, 0x23); // Resonance 2; music voices 1/2 only.
            self.write(bus, sid, 24, 0x1c); // Low pass, volume 12, voice 3 enabled.
        }
        self.initialized = true;
    }

    #[inline(never)]
    fn write<B: SidBus>(&mut self, bus: &mut B, sid: usize, reg: usize, value: u8) {
        // Private callers only select SID 0/1 and registers 0..=24. Keeping
        // that invariant here avoids per-write bounds machinery on the 6502.
        let previous = unsafe { self.registers.get_unchecked_mut(sid).get_unchecked_mut(reg) };
        if *previous != value {
            *previous = value;
            bus.write(sid as u8, reg as u8, value);
        }
    }

    #[inline(always)]
    fn register(&self, sid: usize, reg: usize) -> u8 {
        // Same private SID/register invariant as write().
        unsafe { *self.registers.get_unchecked(sid).get_unchecked(reg) }
    }

    #[inline(always)]
    fn score_byte(&self, index: usize) -> u8 {
        // All callers derive offsets from four voices, five states and sixteen
        // rows; the score is validated before loaded can become true.
        unsafe { *self.score.get_unchecked(index) }
    }

    #[inline(never)]
    fn release_music<B: SidBus>(&mut self, bus: &mut B) {
        for sid in 0..2 {
            self.write(bus, sid, 4, self.register(sid, 4) & !1);
            self.write(bus, sid, 11, self.register(sid, 11) & !1);
        }
    }

    #[inline(never)]
    fn tick_music<B: SidBus>(&mut self, bus: &mut B) {
        if self.restart_music {
            // A full timer tick with gate low avoids immediate gate-off/on
            // retriggers when switching themes in the middle of a sustained note.
            self.release_music(bus);
            self.row = 0;
            self.phrase = 0;
            self.row_ticks = 0;
            self.restart_music = false;
            return;
        }
        if self.row_ticks == 0 {
            let state = self.music as usize;
            let offset = PATTERN_OFFSET + state * 64 + self.row as usize * 4;
            let transpose = phrase_transpose(self.music, self.phrase);
            for voice in 0..4 {
                let sid = voice & 1;
                let base = (voice >> 1) * 7;
                let instrument = INSTRUMENT_OFFSET + voice * 5;
                let note = if self.music == MusicState::Defeat && self.phrase != 0 {
                    if self.row & 1 == 0 {
                        let index = (self.phrase as usize - 1) * 32 + self.row as usize * 2 + voice;
                        // The phrase counter is 1..=7 here, the even row is
                        // 0..=14, and each sounding row has four voices.
                        unsafe { *DEFEAT_CONTINUATION.get_unchecked(index) }
                    } else {
                        0
                    }
                } else {
                    self.score_byte(offset + voice)
                };
                if note == 0 {
                    self.write(bus, sid, base + 4, self.register(sid, base + 4) & !1);
                    continue;
                }
                let note = (note as i16 + transpose as i16).clamp(24, 95) as u8;
                let frequency = note_frequency(self.standard, note);
                self.write(bus, sid, base, frequency as u8);
                self.write(bus, sid, base + 1, (frequency >> 8) as u8);
                self.write(bus, sid, base + 2, self.score_byte(instrument + 1));
                self.write(bus, sid, base + 3, self.score_byte(instrument + 2));
                self.write(bus, sid, base + 5, self.score_byte(instrument + 3));
                self.write(bus, sid, base + 6, self.score_byte(instrument + 4));
                self.write(bus, sid, base + 4, self.score_byte(instrument) | 1);
            }
            self.row_ticks = self.score_byte(4 + state);
            self.row += 1;
            if self.row == 16 {
                self.row = 0;
                self.phrase = (self.phrase + 1) & 7;
            }
        } else if self.row_ticks == 1 {
            self.release_music(bus);
        }
        self.row_ticks -= 1;
    }

    #[inline(never)]
    fn tick_effects<B: SidBus>(&mut self, bus: &mut B) {
        for sid in 0..2 {
            let mut fx = self.effects[sid];
            if fx.remaining == 0 {
                continue;
            }
            fx.remaining -= 1;
            fx.age += 1;
            if fx.remaining == 0 {
                self.write(bus, sid, 18, self.register(sid, 18) & !1);
            } else {
                fx.frequency = match fx.effect {
                    Effect::Fire | Effect::Death => fx.frequency - (fx.frequency >> 4),
                    Effect::EnemyFire => fx.frequency.saturating_add(fx.frequency >> 6),
                    Effect::ChargedShot | Effect::Bomb => fx.frequency - (fx.frequency >> 5),
                    Effect::Hit => fx.frequency - (fx.frequency >> 3),
                    Effect::Pickup => note_frequency(self.standard, 72 + (fx.age / 3) * 4),
                    Effect::Roll => {
                        let curve = if fx.age < 6 { fx.age } else { 12 - fx.age };
                        note_frequency(self.standard, 60 + curve * 3)
                    }
                };
                self.write(bus, sid, 14, fx.frequency as u8);
                self.write(bus, sid, 15, (fx.frequency >> 8) as u8);
            }
            self.effects[sid] = fx;
        }
    }

    #[inline(never)]
    fn start_effect<B: SidBus>(&mut self, bus: &mut B, event: AudioEvent) {
        let mask = lane(event.pan);
        // Centered effects acquire both voices together to preserve their image.
        for sid in 0..2 {
            if mask & (1 << sid) == 0 {
                continue;
            }
            let old = self.effects[sid];
            if old.remaining != 0 {
                if old.effect.priority() > event.effect.priority() {
                    return;
                }
                if matches!(event.effect, Effect::Fire | Effect::EnemyFire)
                    && old.effect == event.effect
                    && old.age < 2
                {
                    return;
                }
            }
        }
        let (note, waveform, attack_decay, sustain_release, duration) =
            effect_instrument(event.effect);
        let frequency = note_frequency(self.standard, note);
        for sid in 0..2 {
            if mask & (1 << sid) == 0 {
                continue;
            }
            self.write(bus, sid, 18, waveform);
            self.write(bus, sid, 14, frequency as u8);
            self.write(bus, sid, 15, (frequency >> 8) as u8);
            self.write(bus, sid, 16, 0);
            self.write(bus, sid, 17, 8);
            self.write(bus, sid, 19, attack_decay);
            self.write(bus, sid, 20, sustain_release);
            self.write(bus, sid, 18, waveform | 1);
            self.effects[sid] = FxVoice {
                frequency,
                effect: event.effect,
                remaining: duration,
                age: 0,
            };
        }
    }
}

const fn lane(pan: i8) -> u8 {
    if pan < -32 {
        1
    } else if pan > 32 {
        2
    } else {
        3
    }
}

fn effect_instrument(effect: Effect) -> (u8, u8, u8, u8, u8) {
    match effect {
        Effect::Fire => (88, 0x20, 0x02, 0x82, 4),
        Effect::EnemyFire => (80, 0x40, 0x02, 0x72, 6),
        Effect::ChargedShot => (76, 0x20, 0x03, 0xb3, 12),
        Effect::Hit => (65, 0x80, 0x02, 0x94, 9),
        Effect::Pickup => (72, 0x10, 0x02, 0x84, 15),
        Effect::Roll => (60, 0x40, 0x03, 0x82, 12),
        Effect::Bomb => (70, 0x80, 0x08, 0xd8, 24),
        Effect::Death => (66, 0x80, 0x09, 0xc9, 40),
    }
}

fn phrase_transpose(state: MusicState, phrase: u8) -> i8 {
    const FLIGHT: [i8; 8] = [0, 0, -3, -3, -5, -5, 2, 2];
    const BOSS: [i8; 8] = [0, 0, 1, 1, -2, -2, -1, -1];
    const VICTORY: [i8; 8] = [0, 0, 5, 5, 7, 7, 0, 0];
    match state {
        MusicState::Exploration | MusicState::Combat => FLIGHT[phrase as usize],
        MusicState::Boss => BOSS[phrase as usize],
        MusicState::Victory => VICTORY[phrase as usize],
        MusicState::Defeat => 0,
    }
}

// "Lost Wing": seven answering phrases after the score asset's opening lament.
// Four voices per sounding row; every intervening row is a breath. At 18 ticks
// per row, all eight 16-row phrases last 2,304 ticks / 46.08 seconds. Keeping
// these 224 immutable bytes resident preserves the 427-byte audio-state budget
// and lets the interrupt play the complete piece without REU transfers.
const DEFEAT_CONTINUATION: [u8; 224] = [
    // II. A warmer C/G answer, rising before returning to E minor.
    36, 67, 52, 60, 43, 69, 55, 64, 43, 71, 59, 67, 0, 74, 0, 71, 38, 72, 54, 62, 45, 69, 57, 66,
    40, 67, 55, 59, 0, 64, 0, 0,
    // III. Low-register response: A minor, E minor, C, then an unresolved B7.
    33, 64, 48, 57, 40, 60, 52, 64, 40, 59, 55, 67, 0, 62, 0, 59, 36, 64, 52, 60, 43, 67, 55, 64,
    35, 66, 51, 57, 0, 63, 0, 59,
    // IV. The melody reaches upward while the left harmony answers below it.
    40, 67, 59, 64, 47, 71, 55, 67, 43, 76, 59, 71, 0, 74, 0, 67, 33, 72, 52, 69, 40, 71, 57, 64,
    35, 69, 51, 66, 0, 66, 0, 63,
    // V. A brief major-colour bridge, with gaps in the lead for the arpeggio.
    36, 72, 55, 64, 43, 0, 52, 67, 38, 74, 57, 66, 45, 72, 54, 69, 31, 71, 50, 67, 38, 0, 55, 62,
    35, 69, 51, 66, 0, 63, 0, 59,
    // VI. The opening's descending shape returns with a different inner line.
    40, 71, 55, 67, 47, 67, 59, 64, 36, 69, 52, 64, 43, 64, 55, 60, 33, 67, 48, 64, 40, 62, 52, 57,
    35, 66, 51, 59, 0, 63, 0, 57,
    // VII. Quiet acceptance: rising bass beneath a descending upper melody.
    36, 67, 52, 60, 38, 66, 54, 62, 40, 64, 55, 59, 43, 62, 59, 67, 33, 60, 52, 57, 35, 59, 51, 54,
    40, 64, 55, 59, 0, 0, 0, 64,
    // VIII. A final A/D/G/B cadence, E-minor resolution, then a longer breath.
    33, 69, 52, 60, 38, 66, 57, 62, 31, 67, 50, 59, 38, 62, 55, 67, 35, 63, 54, 57, 47, 66, 51, 59,
    40, 64, 55, 59, 0, 0, 0, 0,
];

// Rounded C6..B6 SID register values: Hz * 2^24 / native SID clock.
// These use the VIC/SID clocks (PAL 985248 Hz, NTSC 1022727 Hz), never turbo CPU Hz.
const PAL_OCTAVE: [u16; 12] = [
    17820, 18880, 20003, 21192, 22452, 23787, 25202, 26700, 28288, 29970, 31752, 33640,
];
const NTSC_OCTAVE: [u16; 12] = [
    17167, 18188, 19270, 20415, 21629, 22916, 24278, 25722, 27251, 28872, 30589, 32407,
];

/// Return a native-clock SID frequency for MIDI notes 24..=95; zero otherwise.
pub fn note_frequency(standard: VideoStandard, note: u8) -> u16 {
    if !(24..=95).contains(&note) {
        return 0;
    }
    let relative = note - 24;
    let octave = relative / 12;
    let semitone = (relative % 12) as usize;
    let value = match standard {
        VideoStandard::Pal => PAL_OCTAVE[semitone],
        VideoStandard::Ntsc => NTSC_OCTAVE[semitone],
    };
    let shift = 5 - octave;
    if shift == 0 {
        value
    } else {
        (value + (1 << (shift - 1))) >> shift
    }
}

/// Original score authored for this project. Host asset tooling copies this
/// bank into REU; target code does not reference the constant or embed a copy.
pub const DEFAULT_SCORE: [u8; SCORE_ASSET_LENGTH] = make_score();

const fn make_score() -> [u8; SCORE_ASSET_LENGTH] {
    // Voice ordering is left bass, right melody, left harmony, right arpeggio.
    const PATTERNS: [[[u8; 4]; 16]; 5] = [
        [
            // Flight: E minor, broad intervals and space between melody phrases.
            [40, 64, 55, 76],
            [0, 0, 0, 71],
            [47, 67, 59, 79],
            [0, 71, 0, 76],
            [40, 69, 55, 78],
            [0, 67, 0, 71],
            [47, 64, 59, 76],
            [0, 0, 0, 79],
            [40, 62, 55, 74],
            [0, 64, 0, 71],
            [47, 67, 59, 76],
            [0, 0, 0, 79],
            [43, 71, 59, 83],
            [0, 69, 0, 78],
            [47, 67, 62, 79],
            [0, 66, 0, 78],
        ],
        [
            // Combat: syncopated bass and a rising answering figure.
            [40, 76, 55, 64],
            [40, 0, 0, 71],
            [47, 74, 59, 67],
            [0, 71, 0, 76],
            [40, 72, 55, 64],
            [0, 74, 0, 71],
            [47, 76, 59, 67],
            [47, 0, 0, 79],
            [43, 79, 59, 67],
            [43, 78, 0, 74],
            [50, 76, 62, 71],
            [0, 74, 0, 78],
            [45, 71, 60, 69],
            [0, 72, 0, 76],
            [47, 74, 62, 71],
            [47, 78, 0, 83],
        ],
        [
            // Carrier: close chromatic pressure over a low ostinato.
            [36, 72, 48, 60],
            [36, 73, 0, 67],
            [43, 72, 55, 63],
            [36, 70, 0, 67],
            [36, 67, 48, 60],
            [36, 0, 0, 66],
            [42, 66, 54, 63],
            [36, 67, 0, 66],
            [39, 75, 51, 63],
            [39, 74, 0, 70],
            [46, 72, 58, 66],
            [39, 71, 0, 70],
            [38, 70, 50, 62],
            [38, 67, 0, 69],
            [43, 66, 55, 65],
            [35, 71, 0, 71],
        ],
        [
            // Homeward: a major-key answer to the flight theme.
            [40, 64, 56, 76],
            [0, 68, 0, 71],
            [47, 71, 59, 80],
            [0, 0, 0, 76],
            [40, 76, 56, 83],
            [0, 0, 0, 80],
            [47, 75, 59, 83],
            [0, 71, 0, 78],
            [45, 73, 61, 81],
            [0, 71, 0, 76],
            [52, 68, 64, 80],
            [0, 0, 0, 76],
            [47, 66, 59, 78],
            [0, 68, 0, 75],
            [40, 64, 56, 76],
            [0, 0, 0, 0],
        ],
        [
            // Lost wing: sparse descending minor response.
            [40, 71, 55, 64],
            [0, 0, 0, 0],
            [0, 69, 0, 62],
            [0, 0, 0, 0],
            [36, 67, 52, 60],
            [0, 0, 0, 0],
            [0, 66, 0, 59],
            [0, 0, 0, 0],
            [33, 64, 48, 57],
            [0, 0, 0, 0],
            [0, 62, 0, 55],
            [0, 0, 0, 0],
            [35, 59, 50, 54],
            [0, 0, 0, 0],
            [40, 64, 55, 59],
            [0, 0, 0, 0],
        ],
    ];
    const INSTRUMENTS: [[u8; 5]; 4] = [
        [0x40, 0x00, 0x04, 0x02, 0x62],
        [0x20, 0x00, 0x08, 0x14, 0x73],
        [0x10, 0x00, 0x08, 0x26, 0x54],
        [0x40, 0x00, 0x02, 0x03, 0x42],
    ];
    let mut result = [0; SCORE_ASSET_LENGTH];
    result[0] = b'S';
    result[1] = b'M';
    result[2] = b'U';
    result[3] = b'1';
    result[4] = 10;
    result[5] = 7;
    result[6] = 6;
    result[7] = 10;
    result[8] = 18;
    let mut voice = 0;
    while voice < 4 {
        let mut field = 0;
        while field < 5 {
            result[INSTRUMENT_OFFSET + voice * 5 + field] = INSTRUMENTS[voice][field];
            field += 1;
        }
        voice += 1;
    }
    let mut state = 0;
    while state < 5 {
        let mut row = 0;
        while row < 16 {
            voice = 0;
            while voice < 4 {
                result[PATTERN_OFFSET + state * 64 + row * 4 + voice] = PATTERNS[state][row][voice];
                voice += 1;
            }
            row += 1;
        }
        state += 1;
    }
    result
}
