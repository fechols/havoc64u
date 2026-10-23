//! Per-song modulation kernels: the "micro-players" swapped in from the REU.
//!
//! Every kernel is plain Rust over the shared transport. On the host `run`
//! dispatches by cue so the playback tests hear exactly what ships; on the MOS
//! target the same bodies are inlined into one argument-free `extern "C"` shim
//! per cue (`target-c64/src/kernels.rs`), each linked at the resident slot
//! address and stored as its own REU image. Kernels touch SID registers only
//! through the engine's deduplicating writes, read the row the transport has
//! already written, and keep all state in `scratch` or the register shadows,
//! so a kernel never needs cleanup: the next attack rewrites its voice.
//!
//! Building blocks are `#[inline(always)]` and lean: the transport position is
//! captured once per tick, register pairs go through the resident
//! `write_word`, and the vibrato depth is a pitch-dependent shift rather than a
//! multiply. A kernel's size is the sum of the blocks it uses, which is what
//! makes per-song code cheaper than one resident player with every feature.

use super::{music_voice, note_frequency, MusicEngine, MusicState, SidBus};
use crate::music::{FILTER_OFFSET, INSTRUMENT_OFFSET, MUSIC_VOICES, PAGE_ROWS, SID_COUNT};

const BASS: usize = 0;
const LEAD: usize = 1;
const DRUMS: usize = 2;
const ARP: usize = 3;
const BELL: usize = 7;
const ACCENT: usize = 9;

// Scratch layout shared by every kernel.
const LFO: usize = 0;
const VIBRATO: usize = 1;
const PORTAMENTO: usize = 4;

/// Dispatch to the loaded cue's kernel.
#[cfg(not(target_arch = "mos"))]
#[inline(never)]
pub fn run<B: SidBus>(engine: &mut MusicEngine, bus: &mut B) {
    match engine.playing {
        MusicState::Title => title(engine, bus),
        MusicState::Lowlands => lowlands(engine, bus),
        MusicState::Canyon => canyon(engine, bus),
        MusicState::Fortress => fortress(engine, bus),
        MusicState::Carrier => carrier(engine, bus),
        MusicState::Victory => victory(engine, bus),
        MusicState::Defeat => {}
        MusicState::CrystalCurrent => crystal_current(engine, bus),
        MusicState::AshesAndSteel => ashes_and_steel(engine, bus),
        MusicState::TempestDrive => tempest_drive(engine, bus),
        MusicState::GorgeWarden => gorge_warden(engine, bus),
        MusicState::CitadelHeart => citadel_heart(engine, bus),
        MusicState::CarrierOverdrive => carrier_overdrive(engine, bus),
        MusicState::AsteroidBelt => asteroid_belt(engine, bus),
        MusicState::ShatteredMoon => shattered_moon(engine, bus),
        MusicState::ShipGraveyard => ship_graveyard(engine, bus),
        MusicState::OrbitalShipyard => orbital_shipyard(engine, bus),
        MusicState::SpaceCarrier => space_carrier(engine, bus),
        MusicState::JumpGate => jump_gate(engine, bus),
        MusicState::Ascent => ascent(engine, bus),
        MusicState::GateGuardian => gate_guardian(engine, bus),
    }
}

/// The resident slot entry; the linker aliases it to the slot origin and the
/// foreground copies the playing cue's image there before `accept_kernel`.
/// It takes no arguments: every shim modulates the fixed resident engine.
#[cfg(target_arch = "mos")]
#[inline(always)]
pub fn run<B: SidBus>(_engine: &mut MusicEngine, _bus: &mut B) {
    extern "C" {
        fn music_kernel_tick();
    }
    unsafe { music_kernel_tick() }
}

/// Cues that ship a kernel image; Lost Wing keeps its untouched elegy.
pub const fn has_kernel(cue: MusicState) -> bool {
    !matches!(cue, MusicState::Defeat)
}

#[inline(always)]
const fn is_attack(event: u8) -> bool {
    event > 1 && event & 0x80 == 0
}

#[inline(always)]
fn triangle(phase: u8) -> u8 {
    if phase < 128 {
        phase
    } else {
        255 - phase
    }
}

/// The transport position a kernel tick colours: the row the transport wrote
/// last and the ticks elapsed since it did (zero on the tick that wrote it).
#[derive(Clone, Copy)]
struct At {
    row: usize,
    tick: u8,
}

impl MusicEngine {
    #[inline(always)]
    fn at(&self) -> At {
        At {
            // `kernel_runs` guarantees a written row, so this never underflows.
            row: self.row as usize - 1,
            tick: self.page[7] - 1 - self.row_ticks,
        }
    }

    #[inline(always)]
    fn instrument(&self, voice: usize, field: usize) -> u8 {
        self.page_byte(INSTRUMENT_OFFSET + voice * 5 + field)
    }

    #[inline(always)]
    fn voice_frequency(&self, voice: usize) -> u16 {
        let (sid, base) = music_voice(voice);
        u16::from_le_bytes([self.register(sid, base), self.register(sid, base + 1)])
    }

    #[inline(always)]
    fn set_voice_frequency<B: SidBus>(&mut self, bus: &mut B, voice: usize, frequency: u16) {
        let (sid, base) = music_voice(voice);
        self.write_word(bus, sid, base, frequency);
    }

    #[inline(always)]
    fn set_cutoff<B: SidBus>(&mut self, bus: &mut B, cutoff: u8) {
        for sid in 0..SID_COUNT {
            self.write(bus, sid, 22, cutoff);
        }
    }

    #[inline(always)]
    fn word(&self, slot: usize) -> u16 {
        u16::from_le_bytes([self.scratch[slot], self.scratch[slot + 1]])
    }

    #[inline(always)]
    fn set_word(&mut self, slot: usize, value: u16) {
        self.scratch[slot] = value as u8;
        self.scratch[slot + 1] = (value >> 8) as u8;
    }

    /// Advance the shared low-frequency phase once per tick.
    #[inline(always)]
    fn lfo(&mut self) -> u8 {
        let phase = self.scratch[LFO].wrapping_add(1);
        self.scratch[LFO] = phase;
        phase
    }

    /// Move every chip's cutoff towards the bar's authored target. A zero
    /// target leaves the filter wherever the previous bar settled it.
    #[inline(always)]
    fn glide_filter<B: SidBus>(&mut self, bus: &mut B, step: u8) {
        let target = self.page_byte(FILTER_OFFSET);
        if target == 0 {
            return;
        }
        let current = self.register(0, 22);
        let next = if current < target {
            current.saturating_add(step).min(target)
        } else {
            current.saturating_sub(step).max(target)
        };
        self.set_cutoff(bus, next);
    }

    /// The bass-driven "wah": each attack on `voice` throws the cutoff above
    /// the bar's target and it falls back exponentially, so the filter opens
    /// on every bass note the way Tel's 8580 basslines do.
    #[inline(always)]
    fn wah_filter<B: SidBus>(&mut self, bus: &mut B, at: At, voice: usize, amount: u8) {
        let target = self.page_byte(FILTER_OFFSET);
        if target == 0 {
            return;
        }
        let current = self.register(0, 22);
        let next = if at.tick == 0 && is_attack(self.event_at(at.row, voice)) {
            target.saturating_add(amount)
        } else if current > target {
            current - ((current - target + 3) >> 2)
        } else {
            current.saturating_add(2).min(target)
        };
        self.set_cutoff(bus, next);
    }

    /// Triangle pulse-width modulation around the instrument's own width.
    /// `spread` scales the swing (2: about twelve percent, 3: a quarter) and
    /// `rate` the speed. Non-pulse instruments are left untouched.
    #[inline(always)]
    fn pulse_shimmer<B: SidBus>(&mut self, bus: &mut B, voice: usize, spread: u8, rate: u8) {
        if self.instrument(voice, 0) & 0xf0 != 0x40 {
            return;
        }
        let phase = self.scratch[LFO].wrapping_mul(rate);
        let width = u16::from_le_bytes([self.instrument(voice, 1), self.instrument(voice, 2)]);
        let width = (width + ((triangle(phase) as u16) << spread)).min(0xf00);
        let (sid, base) = music_voice(voice);
        self.write_word(bus, sid, base + 2, width);
    }

    /// Delayed vibrato on anything held long enough. The swing is a shift of
    /// the triangle by the pitch's octave band, so it stays between roughly
    /// one and two percent across the lead's range without a multiply;
    /// `depth` halves it per step. The first swinging tick sits at the centre.
    #[inline(always)]
    fn vibrato<B: SidBus>(&mut self, bus: &mut B, at: At, voice: usize, delay: u8, depth: u8, rate: u8) {
        if at.tick == 0 {
            let event = self.event_at(at.row, voice);
            if event == 0 {
                self.set_word(VIBRATO + 1, 0);
                return;
            }
            if event > 1 {
                self.set_word(VIBRATO + 1, note_frequency(self.standard, event & 0x7f));
                if event & 0x80 == 0 {
                    self.scratch[VIBRATO] = 0;
                }
            }
        }
        let base = self.word(VIBRATO + 1);
        if base == 0 {
            return;
        }
        let age = self.scratch[VIBRATO].saturating_add(1);
        self.scratch[VIBRATO] = age;
        if age <= delay {
            return;
        }
        let phase = 64u8.wrapping_add((age - delay - 1).wrapping_mul(rate));
        let band = (base >> 13) as u8;
        let swing = ((triangle(phase) as u16) << band) >> depth;
        let centre = (64u16 << band) >> depth;
        self.set_voice_frequency(bus, voice, base + swing - centre);
    }

    /// Portamento into slurred notes: the pitch closes the remaining distance
    /// by a `shift` fraction each tick and lands exactly. Attacks jump. When a
    /// kernel also uses `vibrato`, call this afterwards: the glide overrides
    /// the swing until it lands, then the vibrato resumes around the target.
    #[inline(always)]
    fn portamento<B: SidBus>(&mut self, bus: &mut B, at: At, voice: usize, shift: u8) {
        if at.tick == 0 {
            let event = self.event_at(at.row, voice);
            if event == 0 {
                self.set_word(PORTAMENTO, 0);
                return;
            }
            if event > 1 {
                let target = note_frequency(self.standard, event & 0x7f);
                self.set_word(PORTAMENTO + 2, target);
                if event & 0x80 == 0 || self.word(PORTAMENTO) == 0 {
                    self.set_word(PORTAMENTO, target);
                }
            }
        }
        let current = self.word(PORTAMENTO);
        let target = self.word(PORTAMENTO + 2);
        if current == 0 || current == target {
            return;
        }
        let round = (1u16 << shift) - 1;
        let next = if current < target {
            current + ((target - current + round) >> shift)
        } else {
            current - ((current - target + round) >> shift)
        };
        self.set_word(PORTAMENTO, next);
        self.set_voice_frequency(bus, voice, next);
    }

    /// Chord-safe double-time arpeggio: on odd ticks the arpeggio voice plays
    /// the next row's authored pitch, so the chip-arp flutter only ever uses
    /// tones the composer wrote. Even ticks (including the row's first) keep
    /// the transport's pitch.
    #[inline(always)]
    fn double_time<B: SidBus>(&mut self, bus: &mut B, at: At, voice: usize) {
        if at.row + 1 >= PAGE_ROWS {
            return;
        }
        let current = self.event_at(at.row, voice);
        let next = self.event_at(at.row + 1, voice);
        if !is_attack(current) || !is_attack(next) || next == current {
            return;
        }
        let pitch = if at.tick & 1 == 0 { current } else { next };
        self.set_voice_frequency(bus, voice, note_frequency(self.standard, pitch));
    }

    /// Tick-rate drum program for the noise kit: kicks turn into a pulse body
    /// sliding down fast after the first noise tick, snares (and slurred snare
    /// drops) fall in pitch, hats are left crisp. The next attack restores the
    /// instrument, so nothing here needs undoing.
    #[inline(always)]
    fn drum_program<B: SidBus>(&mut self, bus: &mut B, at: At, voice: usize) {
        let event = self.event_at(at.row, voice);
        if event <= 1 || at.tick == 0 {
            return;
        }
        let pitch = event & 0x7f;
        let frequency = self.voice_frequency(voice);
        if pitch <= 45 {
            if at.tick == 1 {
                let (sid, base) = music_voice(voice);
                self.write_word(bus, sid, base + 2, 0x0800);
                self.write(bus, sid, base + 4, 0x41);
                self.set_voice_frequency(bus, voice, note_frequency(self.standard, pitch + 12));
            } else {
                self.set_voice_frequency(bus, voice, frequency - (frequency >> 2));
            }
        } else if pitch < 80 {
            self.set_voice_frequency(bus, voice, frequency - (frequency >> 3));
        }
    }

    /// While a ring-modulated bell or anvil rings, sweep its modulator (the
    /// chip's accent oscillator) downwards so the clang's partials glide.
    #[inline(always)]
    fn ring_sweep<B: SidBus>(&mut self, bus: &mut B, at: At, carrier: usize, modulator: usize) {
        if carrier >= MUSIC_VOICES || modulator >= MUSIC_VOICES {
            return;
        }
        let event = self.event_at(at.row, carrier);
        if event == 0 || (at.tick == 0 && is_attack(event)) {
            return;
        }
        let frequency = self.voice_frequency(modulator);
        self.set_voice_frequency(bus, modulator, frequency - (frequency >> 4));
    }
}

// Per-song kernels. Each one is a different sound engine personality built
// from the blocks above; the numbers are the baked constants a resident,
// data-driven player would otherwise have to carry for every song.

#[inline(always)]
pub fn title<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 1);
    e.vibrato(bus, at, LEAD, 12, 1, 3);
    e.pulse_shimmer(bus, BASS, 2, 1);
}

#[inline(always)]
pub fn lowlands<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 1);
    e.vibrato(bus, at, LEAD, 10, 1, 3);
    e.portamento(bus, at, LEAD, 2);
    e.pulse_shimmer(bus, LEAD, 2, 1);
}

#[inline(always)]
pub fn canyon<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x30);
    e.vibrato(bus, at, LEAD, 8, 1, 4);
}

#[inline(always)]
pub fn fortress<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.pulse_shimmer(bus, LEAD, 3, 2);
    e.vibrato(bus, at, LEAD, 10, 2, 4);
}

#[inline(always)]
pub fn carrier<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    e.lfo();
    e.glide_filter(bus, 1);
    e.pulse_shimmer(bus, LEAD, 2, 1);
    e.pulse_shimmer(bus, BASS, 2, 1);
}

#[inline(always)]
pub fn victory<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 1);
    e.vibrato(bus, at, LEAD, 8, 1, 2);
    e.portamento(bus, at, LEAD, 3);
}

#[inline(always)]
pub fn crystal_current<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.double_time(bus, at, ARP);
    e.pulse_shimmer(bus, LEAD, 2, 3);
}

#[inline(always)]
pub fn ashes_and_steel<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x30);
    e.pulse_shimmer(bus, LEAD, 3, 2);
    e.ring_sweep(bus, at, BELL, ACCENT);
}

#[inline(always)]
pub fn tempest_drive<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.pulse_shimmer(bus, LEAD, 3, 4);
    e.vibrato(bus, at, LEAD, 6, 1, 4);
}

#[inline(always)]
pub fn gorge_warden<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x40);
    e.pulse_shimmer(bus, LEAD, 2, 2);
}

#[inline(always)]
pub fn citadel_heart<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x28);
    e.pulse_shimmer(bus, LEAD, 3, 1);
    e.double_time(bus, at, ARP);
}

#[inline(always)]
pub fn carrier_overdrive<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.double_time(bus, at, ARP);
    e.pulse_shimmer(bus, LEAD, 2, 3);
    e.vibrato(bus, at, LEAD, 8, 2, 5);
}

#[inline(always)]
pub fn asteroid_belt<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.double_time(bus, at, ARP);
    e.drum_program(bus, at, DRUMS);
    e.pulse_shimmer(bus, LEAD, 2, 3);
}

#[inline(always)]
pub fn shattered_moon<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.vibrato(bus, at, LEAD, 6, 0, 2);
    e.portamento(bus, at, LEAD, 2);
    e.ring_sweep(bus, at, BELL, ACCENT);
}

#[inline(always)]
pub fn ship_graveyard<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.vibrato(bus, at, LEAD, 8, 1, 2);
    e.portamento(bus, at, LEAD, 3);
}

#[inline(always)]
pub fn orbital_shipyard<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x38);
    e.drum_program(bus, at, DRUMS);
    e.pulse_shimmer(bus, LEAD, 3, 2);
    e.ring_sweep(bus, at, BELL, ACCENT);
}

#[inline(always)]
pub fn space_carrier<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x30);
    e.drum_program(bus, at, DRUMS);
    e.pulse_shimmer(bus, LEAD, 2, 1);
    e.double_time(bus, at, ARP);
}

#[inline(always)]
pub fn jump_gate<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.pulse_shimmer(bus, LEAD, 2, 2);
    e.vibrato(bus, at, LEAD, 8, 1, 3);
    e.ring_sweep(bus, at, BELL, ACCENT);
}

#[inline(always)]
pub fn ascent<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.glide_filter(bus, 2);
    e.vibrato(bus, at, LEAD, 6, 1, 3);
    e.portamento(bus, at, LEAD, 2);
}

#[inline(always)]
pub fn gate_guardian<B: SidBus>(e: &mut MusicEngine, bus: &mut B) {
    let at = e.at();
    e.lfo();
    e.wah_filter(bus, at, BASS, 0x30);
    e.double_time(bus, at, ARP);
    e.drum_program(bus, at, DRUMS);
    e.pulse_shimmer(bus, LEAD, 3, 4);
}
