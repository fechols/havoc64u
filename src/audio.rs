//! Allocation-free, paged four-SID music and sound-effects player.
//!
//! The timer calls `tick` at 50 Hz. Foreground AUDIO code fills one external
//! page, then publishes it while the interrupt is masked. Keep that external
//! page unchanged until `page_request` returns another request. The interrupt
//! copies a ready page into the active page at a phrase boundary; it never reads
//! REU. Reset lives in BOOT and page validation lives in the AUDIO overlay.
//! SID 0/1 voice three remain exclusively reserved for the existing effects.
//!
//! With the `music-kernels` feature, each cue may bring its own per-tick
//! modulation kernel: on the target it is REU code the foreground copies into
//! a fixed resident slot at a cue change; on the host the same Rust runs
//! directly. Kernels only colour registers after the transport has written
//! the row, and derive all state from the transport position plus `scratch`,
//! which clears whenever page zero is consumed so every loop sounds the same.

pub use crate::music::ScoreError;
use crate::music::{
    validate_page, EVENT_OFFSET, INSTRUMENT_OFFSET, MUSIC_VOICES, PAGE_ROWS, PAGE_SIZE, SID_COUNT,
};

#[cfg(feature = "music-kernels")]
pub mod kernels;

pub const SID_BASES: [u16; SID_COUNT] = [0xd400, 0xd420, 0xd440, 0xd460];
const QUEUE_CAPACITY: usize = 4;
/// Kernel-owned state bytes; cleared at every loop start.
pub const SCRATCH_SIZE: usize = 16;
const NO_KERNEL: u8 = 0xff;
const DEFAULT_FILTER_CUTOFF: u8 = 0x80;

pub trait SidBus {
    fn write(&mut self, sid: u8, reg: u8, value: u8);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Effect {
    Fire,
    EnemyFire,
    MissileLaunch,
    Hit,
    Pickup,
    Roll,
    Bomb,
    Death,
    Footstep,
    BirdCall,
    Transport,
    TransportAway,
    Meteor,
}

impl Effect {
    pub const fn priority(self) -> u8 {
        match self {
            Self::Death | Self::Bomb => 4,
            Self::Hit | Self::MissileLaunch => 3,
            Self::Pickup | Self::Roll => 2,
            Self::Footstep
            | Self::BirdCall
            | Self::Transport
            | Self::TransportAway
            | Self::Meteor => 1,
            Self::Fire | Self::EnemyFire => 0,
        }
    }

    const fn is_scene(self) -> bool {
        matches!(
            self,
            Self::Footstep | Self::BirdCall | Self::Transport | Self::TransportAway | Self::Meteor
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AudioEvent {
    pub effect: Effect,
    /// Below -32: left; above 32: right; otherwise both reserved effects voices.
    pub pan: i8,
}

// Packed so the host image of the engine matches the byte-aligned MOS layout.
#[derive(Clone, Copy)]
#[repr(C, packed)]
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

/// Foreground request for the next external score page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRequest {
    pub cue: MusicState,
    pub page: u8,
}

/// The active page and audio state fit the fixed 432-byte target allocation.
/// The external page is separately owned by the foreground loader.
/// `repr(C)` with the only two-byte field first keeps the declared order and
/// a padding-free layout on both the host and the byte-aligned MOS target, so
/// a host engine image is a valid simulator fixture for kernel tests.
#[repr(C)]
#[cfg_attr(feature = "host-tools", derive(Clone))]
pub struct AudioEngine {
    underruns: u16,
    page: [u8; PAGE_SIZE],
    // 32-byte rows: the shadow offset equals the hardware register offset, so
    // every inlined accessor shares one shift instead of a multiply by 25.
    registers: [[u8; 32]; SID_COUNT],
    effects: [FxVoice; 2],
    events: [AudioEvent; QUEUE_CAPACITY],
    scratch: [u8; SCRATCH_SIZE],
    standard: VideoStandard,
    music: MusicState,
    playing: MusicState,
    kernel: u8,
    row: u8,
    row_ticks: u8,
    event_count: u8,
    loaded: bool,
    initialized: bool,
    enabled: bool,
    ready: bool,
    waiting: bool,
}

// image() exposes every byte, so reject any layout with padding at compile
// time. After the counter and arrays come four identity bytes, three
// transport bytes and five booleans; the nested records are padding-free.
const _: () = assert!(
    core::mem::size_of::<AudioEngine>()
        == 2 + PAGE_SIZE + 32 * SID_COUNT + 10 + 8 + SCRATCH_SIZE + 4 + 3 + 5
);

/// A valid host-side engine state captured by [`AudioEngine::snapshot`].
/// The external score page and SID bus state remain owned by the caller.
#[cfg(feature = "host-tools")]
pub struct AudioSnapshot {
    engine: AudioEngine,
}

impl AudioEngine {
    pub const fn new(standard: VideoStandard) -> Self {
        Self {
            underruns: 0,
            page: [0; PAGE_SIZE],
            registers: [[0; 32]; SID_COUNT],
            effects: [EMPTY_FX; 2],
            events: [EMPTY_EVENT; QUEUE_CAPACITY],
            scratch: [0; SCRATCH_SIZE],
            standard,
            music: MusicState::Title,
            playing: MusicState::Title,
            kernel: NO_KERNEL,
            row: 0,
            row_ticks: 0,
            event_count: 0,
            loaded: false,
            initialized: false,
            enabled: true,
            ready: false,
            waiting: false,
        }
    }

    /// Selecting the same cue preserves its transport position and queued page.
    /// A changed cue keeps playing the old page until its opening is published.
    pub fn set_music(&mut self, state: MusicState) {
        if state != self.music {
            self.music = state;
            self.ready = false;
        }
    }

    pub const fn music(&self) -> MusicState {
        self.music
    }
    pub const fn current_cue(&self) -> MusicState {
        self.playing
    }
    pub const fn underruns(&self) -> u16 {
        self.underruns
    }
    pub const fn current_page(&self) -> u8 {
        if self.loaded {
            self.page[5]
        } else {
            0xff
        }
    }

    /// Snapshot under an IRQ mask before filling the external page.
    pub fn page_request(&self) -> Option<PageRequest> {
        if !self.enabled || self.ready {
            return None;
        }
        Some(PageRequest {
            cue: self.music,
            page: if !self.loaded || self.playing != self.music {
                0
            } else {
                self.page[6]
            },
        })
    }

    /// The cue whose modulation kernel the foreground should load next, if the
    /// slot does not already hold it (or its recorded absence). Snapshot under
    /// an IRQ mask.
    pub fn kernel_request(&self) -> Option<MusicState> {
        (self.enabled && self.kernel & 0x7f != self.music as u8).then_some(self.music)
    }

    /// Call under an IRQ mask before overwriting the resident kernel slot; the
    /// interrupt stops calling the slot until `accept_kernel`.
    pub fn invalidate_kernel(&mut self) {
        self.kernel = NO_KERNEL;
    }

    /// Call under an IRQ mask once the slot holds the verified kernel for
    /// `cue`, or record that `cue` has none (bit seven marks the absence, so
    /// the slot never runs for it while the request stays satisfied).
    pub fn accept_kernel(&mut self, cue: MusicState, present: bool) {
        self.kernel = cue as u8 | if present { 0 } else { 0x80 };
    }

    pub const fn kernel_runs(&self) -> bool {
        // A written row is required: the cue-change gate-low tick has none,
        // and a nonzero row implies a loaded page.
        self.row != 0 && self.kernel == self.playing as u8
    }

    /// The engine's exact byte image: with the padding-free `repr(C)` layout
    /// this is also the MOS image, which the simulator kernel test loads.
    #[cfg(feature = "host-tools")]
    pub fn image(&self) -> &[u8] {
        // SAFETY: the struct is plain old data with no padding between fields.
        unsafe {
            core::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                core::mem::size_of::<Self>(),
            )
        }
    }

    /// Capture a typed engine state without exposing a raw-byte constructor.
    #[cfg(feature = "host-tools")]
    pub fn snapshot(&self) -> AudioSnapshot {
        AudioSnapshot {
            engine: self.clone(),
        }
    }

    /// Restore a captured engine state. The caller must retain the matching
    /// external score page and SID bus state; this does not write to the bus.
    #[cfg(feature = "host-tools")]
    pub fn restore(&mut self, snapshot: &AudioSnapshot) {
        *self = snapshot.engine.clone();
    }

    /// Run the loaded kernel once more on the current state, exactly as the
    /// tick would; host tooling records the result as a simulator fixture.
    #[cfg(all(feature = "host-tools", feature = "music-kernels"))]
    pub fn run_kernel_once<B: SidBus>(&mut self, bus: &mut B) -> bool {
        if !self.kernel_runs() {
            return false;
        }
        kernels::run(self, bus);
        true
    }

    /// Validate and publish the external page. The caller must mask the audio
    /// interrupt and keep these same bytes available to `tick` until consumed.
    /// Failed or obsolete requests leave the current score untouched.
    #[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
    #[inline(never)]
    pub fn publish_page(&mut self, request: PageRequest, bytes: &[u8]) -> Result<(), ScoreError> {
        if self.page_request() != Some(request) {
            return Err(ScoreError::Request);
        }
        validate_page(bytes, request.cue as u8, request.page)?;
        for voice in 0..MUSIC_VOICES {
            let event = bytes[EVENT_OFFSET + voice];
            if event == 1 || event & 0x80 != 0 {
                if !self.loaded || self.playing != request.cue {
                    return Err(ScoreError::Transition);
                }
                let at = INSTRUMENT_OFFSET + voice * 5;
                for field in 0..5 {
                    // Validation guarantees both instrument records exist. A
                    // volatile byte comparison avoids linking MOS's memcmp
                    // helper and its disproportionately large static scratch.
                    let previous = unsafe { self.page.as_ptr().add(at + field).read_volatile() };
                    let incoming = unsafe { bytes.as_ptr().add(at + field).read_volatile() };
                    if previous != incoming {
                        return Err(ScoreError::Transition);
                    }
                }
            }
        }
        self.ready = true;
        Ok(())
    }

    /// Queue an effect without affecting the music voices. Equal-priority
    /// effects replace older effects; higher-priority effects retain their voice.
    /// Repeated pending gunfire in the same stereo lane is coalesced.
    // Keep both foreground callers inside their loaded SIM/SCENERY overlays;
    // outlining this shared routine would consume scarce resident IRQ space.
    #[inline(always)]
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

    /// Cancel only scenery cues, leaving player effects and music untouched.
    /// Foreground SCENERY calls this with the interrupt masked. No SID access
    /// occurs here: the next audio tick releases any active scenery voice.
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    #[inline(never)]
    pub fn cancel_scene_effects(&mut self) {
        let mut retained = 0;
        for index in 0..self.event_count as usize {
            let event = self.events[index];
            if !event.effect.is_scene() {
                self.events[retained] = event;
                retained += 1;
            }
        }
        self.event_count = retained as u8;
        for voice in &mut self.effects {
            if voice.remaining != 0 && voice.effect.is_scene() {
                voice.remaining = 1;
            }
        }
    }

    /// Initialize all mapped chips, clear effects, and request the cue opening.
    /// This startup function requires the BOOT overlay on the target.
    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    #[inline(never)]
    pub fn reset<B: SidBus>(&mut self, bus: &mut B) {
        self.effects = [EMPTY_FX; 2];
        self.event_count = 0;
        self.row = 0;
        self.row_ticks = 0;
        self.underruns = 0;
        self.playing = self.music;
        self.loaded = false;
        self.ready = false;
        self.waiting = false;
        self.enabled = true;
        self.kernel = NO_KERNEL;
        self.scratch = [0; SCRATCH_SIZE];
        self.initialize(bus);
    }

    /// Silence every mapped chip until `reset`; discard pending pages/effects.
    pub fn silence<B: SidBus>(&mut self, bus: &mut B) {
        self.event_count = 0;
        self.effects = [EMPTY_FX; 2];
        self.ready = false;
        self.enabled = false;
        for sid in 0..SID_COUNT {
            self.write(bus, sid, 4, 0);
            self.write(bus, sid, 11, 0);
            self.write(bus, sid, 18, 0);
            self.write(bus, sid, 24, 0);
        }
    }

    /// One 20 ms timer tick. The external page is read only after publication.
    pub fn tick<B: SidBus>(&mut self, bus: &mut B, next_page: &[u8; PAGE_SIZE]) {
        if !self.enabled || !self.initialized {
            return;
        }
        self.tick_music(bus, next_page);
        self.tick_kernel(bus);
        self.tick_effects(bus);
        for index in 0..self.event_count as usize {
            self.start_effect(bus, self.events[index]);
        }
        self.event_count = 0;
    }

    /// Colour the row the transport has just written. The slot only runs for
    /// the cue it was loaded for, so a cue change never modulates the old tail
    /// with the new song's kernel.
    #[inline(always)]
    fn tick_kernel<B: SidBus>(&mut self, bus: &mut B) {
        #[cfg(feature = "music-kernels")]
        if self.kernel_runs() {
            kernels::run(self, bus);
        }
        #[cfg(not(feature = "music-kernels"))]
        let _ = bus;
    }

    #[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
    #[inline(never)]
    fn initialize<B: SidBus>(&mut self, bus: &mut B) {
        for sid in 0..SID_COUNT {
            for reg in 0..25 {
                bus.write(sid as u8, reg as u8, 0);
                // The loops exactly cover all four SID register shadows.
                unsafe {
                    *self.registers.get_unchecked_mut(sid).get_unchecked_mut(reg) = 0;
                }
            }
            self.write(bus, sid, 22, DEFAULT_FILTER_CUTOFF);
            // Extra chips have three music voices; effects bypass filters on 0/1.
            self.write(bus, sid, 23, if sid < 2 { 0x23 } else { 0x27 });
            self.write(bus, sid, 24, 0x1c);
        }
        self.initialized = true;
    }

    #[inline(never)]
    fn write<B: SidBus>(&mut self, bus: &mut B, sid: usize, reg: usize, value: u8) {
        // Private routing guarantees a mapped chip and register 0..=24.
        let previous = unsafe { self.registers.get_unchecked_mut(sid).get_unchecked_mut(reg) };
        if *previous != value {
            *previous = value;
            bus.write(sid as u8, reg as u8, value);
        }
    }

    /// Write a little-endian register pair (frequency or pulse width).
    #[inline(never)]
    fn write_word<B: SidBus>(&mut self, bus: &mut B, sid: usize, reg: usize, value: u16) {
        self.write(bus, sid, reg, value as u8);
        self.write(bus, sid, reg + 1, (value >> 8) as u8);
    }

    /// The active page's event for a row and voice.
    #[inline(never)]
    fn event_at(&self, row: usize, voice: usize) -> u8 {
        self.page_byte(EVENT_OFFSET + row * MUSIC_VOICES + voice)
    }

    #[inline(always)]
    fn register(&self, sid: usize, reg: usize) -> u8 {
        unsafe { *self.registers.get_unchecked(sid).get_unchecked(reg) }
    }

    #[inline(always)]
    fn page_byte(&self, index: usize) -> u8 {
        // Private callers use fixed header/instrument offsets or a row below
        // PAGE_ROWS. Page publication validates the complete fixed-size page.
        unsafe { *self.page.get_unchecked(index) }
    }

    #[inline(never)]
    fn release_music<B: SidBus>(&mut self, bus: &mut B) {
        for sid in 0..SID_COUNT {
            let end = if sid < 2 { 18 } else { 25 };
            let mut reg = 4;
            while reg < end {
                self.write(bus, sid, reg, self.register(sid, reg) & !1);
                reg += 7;
            }
        }
    }

    #[inline(never)]
    fn consume_page(&mut self, next_page: &[u8; PAGE_SIZE]) {
        self.page.copy_from_slice(next_page);
        self.playing = self.music;
        self.ready = false;
        self.loaded = true;
        self.waiting = false;
        self.row = 0;
        self.row_ticks = 0;
        #[cfg(feature = "music-kernels")]
        if self.page[5] == 0 {
            // Kernel state restarts with the loop, so each traversal is identical.
            self.scratch = [0; SCRATCH_SIZE];
        }
    }

    #[inline(never)]
    fn tick_music<B: SidBus>(&mut self, bus: &mut B, next_page: &[u8; PAGE_SIZE]) {
        if self.ready && (!self.loaded || self.playing != self.music) {
            self.release_music(bus);
            self.consume_page(next_page);
            #[cfg(feature = "music-kernels")]
            if self.playing == MusicState::Defeat {
                // Lost Wing has no kernel, so its baseline must not inherit
                // the preceding cue's modulation. Wait until it is active.
                for sid in 0..SID_COUNT {
                    self.write(bus, sid, 22, DEFAULT_FILTER_CUTOFF);
                }
            }
            return; // A complete gate-low tick before a new cue begins.
        }
        if !self.loaded {
            return;
        }
        if self.row_ticks != 0 {
            if self.row_ticks == 1 {
                if self.row as usize == PAGE_ROWS && !self.ready {
                    // Stall before the release tick. Held notes and SFX continue;
                    // count one underrun per incident, not once per timer tick.
                    // A cue change deliberately discards the old prefetched
                    // page, so waiting for its new opening is not starvation.
                    if !self.waiting && self.playing == self.music {
                        self.underruns = self.underruns.saturating_add(1);
                        self.waiting = true;
                    }
                    return;
                }
                for voice in 0..MUSIC_VOICES {
                    let event = if self.row as usize == PAGE_ROWS {
                        next_page[EVENT_OFFSET + voice]
                    } else {
                        self.event_at(self.row as usize, voice)
                    };
                    if event != 1 && event & 0x80 == 0 {
                        let (sid, base) = music_voice(voice);
                        self.write(bus, sid, base + 4, self.register(sid, base + 4) & !1);
                    }
                }
            }
            self.row_ticks -= 1;
            return;
        }
        if self.row as usize == PAGE_ROWS {
            // A cue request can invalidate the prefetched page after lookahead.
            if !self.ready {
                return;
            }
            self.consume_page(next_page);
        }
        for voice in 0..MUSIC_VOICES {
            let (sid, base) = music_voice(voice);
            let event = self.event_at(self.row as usize, voice);
            if event == 1 {
                continue;
            }
            if event == 0 {
                self.write(bus, sid, base + 4, self.register(sid, base + 4) & !1);
                continue;
            }
            let frequency = note_frequency(self.standard, event & 0x7f);
            let instrument = INSTRUMENT_OFFSET + voice * 5;
            self.write_word(bus, sid, base, frequency);
            self.write(bus, sid, base + 2, self.page_byte(instrument + 1));
            self.write(bus, sid, base + 3, self.page_byte(instrument + 2));
            self.write(bus, sid, base + 5, self.page_byte(instrument + 3));
            self.write(bus, sid, base + 6, self.page_byte(instrument + 4));
            self.write(bus, sid, base + 4, self.page_byte(instrument) | 1);
        }
        self.row += 1;
        self.row_ticks = self.page[7] - 1;
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
                    // The first tick is a noise crack; its pitch does not carry
                    // into the low pulse body. Later ticks settle only slightly.
                    Effect::Fire if fx.age == 1 => note_frequency(self.standard, 52),
                    // Enemy shots trade the noise crack for a higher, raspy body.
                    Effect::EnemyFire if fx.age == 1 => note_frequency(self.standard, 64),
                    // A falling noise clock makes the rocket roar recede.
                    Effect::Fire | Effect::MissileLaunch | Effect::Death | Effect::Meteor => {
                        fx.frequency - (fx.frequency >> 4)
                    }
                    Effect::Bomb => fx.frequency - (fx.frequency >> 5),
                    Effect::EnemyFire | Effect::Hit | Effect::Footstep => {
                        fx.frequency - (fx.frequency >> 3)
                    }
                    Effect::Pickup => note_frequency(self.standard, 72 + (fx.age / 3) * 4),
                    Effect::BirdCall => note_frequency(self.standard, 84 - fx.age),
                    Effect::Transport | Effect::TransportAway => fx.frequency,
                    Effect::Roll => {
                        let curve = if fx.age < 6 { fx.age } else { 12 - fx.age };
                        note_frequency(self.standard, 60 + curve * 3)
                    }
                };
                self.write_word(bus, sid, 14, fx.frequency);
                if fx.effect == Effect::Fire {
                    // Thicken the pulse from 12.5% to 31.25% to 50% duty.
                    // Set pitch/width before leaving noise, keeping the gate
                    // high and the music-owned filters completely untouched.
                    self.write(bus, sid, 17, fx.age * 3 - 1);
                    self.write(bus, sid, 18, 0x41);
                } else if fx.effect == Effect::EnemyFire && fx.age == 1 {
                    // Set the body pitch before switching to sawtooth; retain
                    // the gate so this is one shot, not a second attack.
                    self.write(bus, sid, 18, 0x21);
                } else if fx.effect == Effect::Meteor && fx.age == 4 {
                    // The falling whistle opens into a soft rushing tail,
                    // retaining the same envelope and continuous pitch sweep.
                    self.write(bus, sid, 18, 0x81);
                }
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
            self.write_word(bus, sid, 14, frequency);
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
        Effect::Fire => (88, 0x80, 0x02, 0xb1, 4),
        Effect::EnemyFire => (84, 0x80, 0x02, 0x72, 6),
        Effect::MissileLaunch => (76, 0x80, 0x24, 0xb3, 20),
        Effect::Hit => (65, 0x80, 0x02, 0x94, 9),
        Effect::Pickup => (72, 0x10, 0x02, 0x84, 15),
        Effect::Roll => (60, 0x40, 0x03, 0x82, 12),
        Effect::Bomb => (70, 0x80, 0x08, 0xd8, 24),
        Effect::Death => (66, 0x80, 0x09, 0xc9, 40),
        // Quiet, short envelopes leave room for music and stop before arrival.
        Effect::Footstep => (42, 0x80, 0x00, 0x51, 5),
        Effect::BirdCall => (84, 0x10, 0x01, 0x42, 10),
        Effect::Transport => (42, 0x40, 0x44, 0x52, 30),
        Effect::TransportAway => (35, 0x40, 0x44, 0x52, 30),
        Effect::Meteor => (84, 0x10, 0x14, 0x62, 21),
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
pub fn note_frequency(standard: VideoStandard, note: u8) -> u16 {
    let relative = note.wrapping_sub(24);
    if relative >= 72 {
        return 0;
    }
    let notes = match standard {
        VideoStandard::Pal => &PAL_NOTES,
        VideoStandard::Ntsc => &NTSC_NOTES,
    };
    // The unsigned range check also rejects notes below 24 after wrapping.
    unsafe { *notes.get_unchecked(relative as usize) }
}

#[inline(always)]
fn music_voice(voice: usize) -> (usize, usize) {
    const CHIPS: [u8; MUSIC_VOICES] = [0, 1, 0, 1, 2, 2, 2, 3, 3, 3];
    const BASES: [u8; MUSIC_VOICES] = [0, 0, 7, 7, 0, 7, 14, 0, 7, 14];
    // Every private caller loops over exactly MUSIC_VOICES lanes.
    unsafe {
        (
            *CHIPS.get_unchecked(voice) as usize,
            *BASES.get_unchecked(voice) as usize,
        )
    }
}
