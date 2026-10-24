use super::{
    patches::{
        ACCENT_ARPEGGIO, ACCENT_OCTAVE, ACCENT_PWM_ALL, ACCENT_VIBRATO, FREQUENCIES, PATCHES,
        PROGRAMS,
    },
    SidBus, VideoStandard,
};
pub const EFFECT_COUNT: usize = 21;
const QUEUE_CAPACITY: usize = 8;
const VOLUME: u8 = 8;
const PUNCH_VOLUME: u8 = 15;

/// Chip volume for an instance of `age` with `punch` ticks at full level,
/// then one step down per tick to the mix level.
#[inline(always)]
fn punch_volume(punch: u8, age: u8) -> u8 {
    if age < punch {
        PUNCH_VOLUME
    } else {
        PUNCH_VOLUME
            .saturating_sub(age - punch)
            .max(VOLUME)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Effect {
    Fire,
    EnemyFire,
    MissileLaunch,
    EnemyHit,
    Pickup,
    Roll,
    Bomb,
    EnemyExplosion,
    Footstep,
    BirdCall,
    Transport,
    TransportAway,
    Meteor,
    BossHit,
    PlayerHit,
    BossExplosion,
    PlayerDeath,
    MissileImpact,
    BossArrival,
    MissileReady,
    LowShield,
}
impl Effect {
    pub const fn from_id(id: u8) -> Option<Self> {
        if id < EFFECT_COUNT as u8 {
            Some(unsafe { core::mem::transmute::<u8, Self>(id) })
        } else {
            None
        }
    }
    pub const fn priority(self) -> u8 {
        match self {
            Self::PlayerDeath => 8,
            Self::BossExplosion => 7,
            Self::Bomb => 6,
            Self::PlayerHit | Self::LowShield => 5,
            Self::EnemyHit | Self::BossHit => 4,
            Self::MissileLaunch | Self::MissileImpact | Self::BossArrival => 3,
            Self::EnemyExplosion | Self::Pickup | Self::Roll | Self::MissileReady => 2,
            Self::Fire | Self::EnemyFire => 0,
            _ => 1,
        }
    }
    pub const fn is_cinematic(self) -> bool {
        self as u8 == Self::Bomb as u8 || (self as u8).wrapping_sub(Self::BossExplosion as u8) <= 1
    }
    pub const fn is_scene(self) -> bool {
        matches!(
            self,
            Self::Footstep | Self::BirdCall | Self::Transport | Self::TransportAway | Self::Meteor
        )
    }
    pub const fn is_gun(self) -> bool {
        matches!(self, Self::Fire | Self::EnemyFire)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct AudioEvent {
    pub effect: Effect,
    /// Below -32 is left, above 32 right; otherwise acquire a stereo pair.
    pub pan: i8,
}
impl AudioEvent {
    pub const fn lane(self) -> u8 {
        if self.pan < -32 {
            1
        } else if self.pan > 32 {
            2
        } else {
            3
        }
    }
    pub fn coalesces(self, other: Self) -> bool {
        self.effect == other.effect && self.lane() == other.lane()
    }
}
const EMPTY_EVENT: AudioEvent = AudioEvent {
    effect: Effect::Fire,
    pan: 0,
};
/// Effect priorities by ID, generated from `Effect::priority` for the native
/// kernel's allocation and queue decisions.
#[cfg_attr(target_arch = "mos", link_section = ".text.sfx_priorities")]
#[cfg_attr(target_arch = "mos", export_name = "sfx_priorities")]
pub static PRIORITIES: [u8; EFFECT_COUNT] = {
    let mut table = [0; EFFECT_COUNT];
    let mut id = 0;
    while id < EFFECT_COUNT {
        if let Some(effect) = Effect::from_id(id as u8) {
            table[id] = effect.priority();
        }
        id += 1;
    }
    table
};
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Instance {
    mask: u8,
    effect: Effect,
    age: u8,
    remaining: u8,
    frequency: [u16; 3],
    cutoff: u16,
    program: u8,
}
const EMPTY: Instance = Instance {
    mask: 0,
    effect: Effect::Fire,
    age: 0,
    remaining: 0,
    frequency: [0; 3],
    cutoff: 0,
    program: 0,
};

/// Byte-aligned on both host and MOS; fits the 256-byte page at $CD00 below
/// the DAC state, after the IRQ static-frame backup.
#[derive(Clone)]
#[repr(C, packed)]
pub struct EffectsEngine {
    instances: [Instance; 4],
    registers: [u8; 128],
    events: [AudioEvent; QUEUE_CAPACITY],
    event_count: u8,
    standard: VideoStandard,
    enabled: bool,
    rotation: u8,
}
const _: () = assert!(core::mem::size_of::<EffectsEngine>() <= 224);
// The native kernel (target-c64/asm/sfx.S) addresses these fields directly.
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<Instance>() == 13 && size_of::<AudioEvent>() == 2);
    assert!(offset_of!(Instance, mask) == 0 && offset_of!(Instance, effect) == 1);
    assert!(offset_of!(Instance, age) == 2 && offset_of!(Instance, remaining) == 3);
    assert!(offset_of!(Instance, frequency) == 4 && offset_of!(Instance, cutoff) == 10);
    assert!(offset_of!(Instance, program) == 12);
    assert!(offset_of!(AudioEvent, effect) == 0 && offset_of!(AudioEvent, pan) == 1);
    assert!(offset_of!(EffectsEngine, instances) == 0);
    assert!(offset_of!(EffectsEngine, registers) == 52);
    assert!(offset_of!(EffectsEngine, events) == 180);
    assert!(offset_of!(EffectsEngine, event_count) == 196);
    assert!(offset_of!(EffectsEngine, standard) == 197);
    assert!(offset_of!(EffectsEngine, enabled) == 198);
    assert!(offset_of!(EffectsEngine, rotation) == 199);
    assert!(VideoStandard::Pal as u8 == 0 && VideoStandard::Ntsc as u8 == 1);
    assert!(Effect::Fire as u8 == 0 && Effect::EnemyFire as u8 == 1);
    assert!(Effect::Bomb as u8 == 6 && Effect::BossExplosion as u8 == 15);
    assert!(Effect::PlayerDeath as u8 == 16 && VOLUME == 8 && QUEUE_CAPACITY == 8);
};
impl EffectsEngine {
    pub const fn new(standard: VideoStandard) -> Self {
        Self {
            instances: [EMPTY; 4],
            registers: [0; 128],
            events: [EMPTY_EVENT; 8],
            event_count: 0,
            standard,
            enabled: false,
            rotation: 0,
        }
    }
    #[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
    #[inline(never)]
    pub fn reset<B: SidBus>(&mut self, bus: &mut B) {
        self.instances = [EMPTY; 4];
        self.event_count = 0;
        self.rotation = 0;
        for sid in 0..4u8 {
            for reg in 0..25 {
                bus.write(sid as u8, reg as u8, 0);
            }
            for reg in 0..32 {
                self.registers[(sid as usize) * 32 + reg] = 0;
            }
            self.write(bus, sid, 24, VOLUME);
        }
        self.enabled = true;
    }
    pub fn silence<B: SidBus>(&mut self, bus: &mut B) {
        self.stop(bus, 15);
        self.event_count = 0;
    }
    pub fn queue(&mut self, event: AudioEvent) -> bool {
        queue_events(&mut self.events, 0, &mut self.event_count, event)
    }
    #[inline(always)]
    fn remove(&mut self, index: u8) -> AudioEvent {
        remove_event(&mut self.events, 0, &mut self.event_count, index)
    }
    #[inline(always)]
    fn event_at(&self, index: u8) -> AudioEvent {
        // Only called for indices below event_count, maintained in 0..=8.
        unsafe { *self.events.get_unchecked(index as usize) }
    }
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    #[inline(never)]
    pub fn cancel_scene_effects(&mut self) {
        let mut i = 0;
        while i < self.event_count {
            if self.event_at(i).effect.is_scene() {
                self.remove(i);
            } else {
                i += 1;
            }
        }
        for instance in &mut self.instances {
            if instance.effect.is_scene() && instance.mask != 0 {
                instance.remaining = 1;
            }
        }
    }
    // All callers use a bounded chip 0..4 and register 0..25. A 32-byte
    // stride avoids multiplication and runtime bounds machinery on the 6502.
    #[inline(never)]
    fn write<B: SidBus>(&mut self, bus: &mut B, sid: u8, reg: u8, value: u8) {
        let cached = unsafe {
            self.registers
                .get_unchecked_mut(((sid << 5) | reg) as usize)
        };
        if *cached != value {
            *cached = value;
            bus.write(sid, reg, value);
        }
    }
    fn word<B: SidBus>(&mut self, bus: &mut B, sid: u8, reg: u8, value: u16) {
        self.write(bus, sid, reg, value as u8);
        self.write(bus, sid, reg + 1, (value >> 8) as u8);
    }
    fn stop<B: SidBus>(&mut self, bus: &mut B, mask: u8) {
        for sid in 0..4u8 {
            if mask & (1 << sid) == 0 {
                continue;
            }
            for voice in 0..3u8 {
                self.write(bus, sid, voice * 7 + 4, 0);
            }
            self.write(bus, sid, 21, 0);
            self.write(bus, sid, 22, 0);
            self.write(bus, sid, 23, 0);
            self.write(bus, sid, 24, VOLUME);
            // Mask is the liveness marker; retiring an instrument needs no
            // copy of the unused payload. Start overwrites it before reuse.
            self.instances[sid as usize].mask = 0;
        }
    }
    /// Advances every oscillator/filter independently of foreground rendering.
    #[inline(never)]
    pub fn tick<B: SidBus>(&mut self, bus: &mut B) {
        if !self.enabled {
            return;
        }
        for sid in 0..4u8 {
            let mut fx = self.instances[sid as usize];
            if fx.mask == 0 {
                continue;
            }
            if fx.remaining <= 1 {
                self.stop(bus, fx.mask);
                continue;
            }
            fx.remaining -= 1;
            fx.age += 1;
            let p = unsafe { PATCHES.get_unchecked(fx.effect as usize) };
            let program = unsafe { PROGRAMS.get_unchecked(fx.program as usize) };
            let target = u16::from(if fx.age < p.attack {
                p.cutoff[1]
            } else {
                p.cutoff[2]
            }) << 3;
            // Stereo modulation follows the channel, not the available chip.
            // Each side's second SID is an interchangeable overlap slot.
            let target = (target + ((sid & 1) as u16) * 23).min(2047);
            // The authored rate is the shift applied to the remaining distance.
            if fx.cutoff != target {
                let distance = if fx.cutoff < target {
                    target - fx.cutoff
                } else {
                    fx.cutoff - target
                };
                let step = (distance >> (p.resonance & 7)).max(1);
                if fx.cutoff < target {
                    fx.cutoff += step;
                } else {
                    fx.cutoff -= step;
                }
            }
            self.write(bus, sid, 21, (fx.cutoff & 7) as u8);
            self.write(bus, sid, 22, (fx.cutoff >> 3) as u8);
            if fx.age == program.change && p.late_resonance & 0xf0 != 0 {
                self.write(bus, sid, 23, (p.late_resonance & 0xf0) | (p.filter & 7));
            }
            let punch = p.late_resonance & 15;
            if punch != 0 {
                self.write(bus, sid, 24, (p.filter & 0xf0) | punch_volume(punch, fx.age));
            }
            let accent = program.accent & 3;
            for voice in 0..3u8 {
                let base = voice * 7;
                let mut wave = p.waves[voice as usize];
                if fx.age >= program.change && program.late_waves[voice as usize] != 0 {
                    wave = program.late_waves[voice as usize];
                }
                let slide = program.slide[voice as usize];
                let mut freq = unsafe {
                    core::ptr::read_unaligned(
                        core::ptr::addr_of!(fx.frequency)
                            .cast::<u16>()
                            .add(voice as usize),
                    )
                };
                if slide < 0 {
                    freq -= freq >> (-slide as u8);
                } else if slide > 0 {
                    freq = freq.saturating_add(freq >> (slide as u8));
                }
                if accent == ACCENT_ARPEGGIO && fx.age & 3 == 0 {
                    freq = freq.saturating_add(freq >> 3);
                }
                unsafe {
                    core::ptr::write_unaligned(
                        core::ptr::addr_of_mut!(fx.frequency)
                            .cast::<u16>()
                            .add(voice as usize),
                        freq,
                    );
                }
                let rendered = if accent == ACCENT_OCTAVE && fx.age & 4 != 0 {
                    freq >> 1
                } else if accent == ACCENT_VIBRATO {
                    if fx.age & 2 != 0 {
                        freq.saturating_add(freq >> 5)
                    } else {
                        freq - (freq >> 5)
                    }
                } else {
                    freq
                };
                if slide != 0 || accent != 0 {
                    self.word(bus, sid, base, rendered);
                }
                if program.pwm_rate != 0
                    && (wave & 0x40 != 0 || program.accent & ACCENT_PWM_ALL != 0)
                {
                    let phase = fx
                        .age
                        .wrapping_mul(program.pwm_rate)
                        .wrapping_add((sid & 1) * 37);
                    let triangle = if phase & 128 == 0 { phase } else { 255 - phase };
                    self.word(
                        bus,
                        sid,
                        base + 2,
                        (u16::from(p.pulse[voice as usize]) << 8).wrapping_add(
                            if program.pwm_depth == 1 {
                                (triangle as u16) * 2
                            } else {
                                (triangle as u16) * 8
                            },
                        ),
                    );
                }
                // A retrigger tick holds the gate low so the envelopes attack again.
                let gate = fx.age < p.release && fx.age != program.retrigger;
                self.write(bus, sid, base + 4, wave | u8::from(gate));
            }
            self.instances[sid as usize] = fx;
        }
        let mut admitted = 0;
        while self.event_count != 0 {
            let mut best = 0;
            for i in 1..self.event_count {
                if self.event_at(i).effect.priority() > self.event_at(best).effect.priority() {
                    best = i;
                }
            }
            let event = self.remove(best);
            admitted |= self.start(bus, event, admitted);
        }
    }
    #[inline(never)]
    fn start<B: SidBus>(&mut self, bus: &mut B, event: AudioEvent, admitted: u8) -> u8 {
        let mut best = 0;
        let mut victims = 0;
        let mut best_cost = u8::MAX;
        let mut best_age = 0;
        let lane = if event.effect.is_cinematic() {
            3
        } else {
            event.lane()
        };
        // A repeated bomb replaces its own pair, leaving the overlap pair free.
        let mut restart = 0;
        if event.effect == Effect::Bomb {
            // Every stereo instance owns exactly one left chip. Checking its
            // two fixed slots avoids a 16-bit iterator on the native CPU.
            if self.instances[0].effect == Effect::Bomb {
                restart = self.instances[0].mask;
            }
            if self.instances[2].effect == Effect::Bomb {
                restart |= self.instances[2].mask;
            }
        }
        // Four stereo combinations or two chips on a requested side. Even hero
        // cues use just one chip per channel. Rotate equivalent free slots.
        // Mono masks repeat after two candidates, irrespective of rotation.
        for candidate in 0..if lane == 3 { 4u8 } else { 2u8 } {
            let n = (candidate + self.rotation) & 3;
            let mask = if lane == 1 {
                1 << ((n & 1) * 2)
            } else if lane == 2 {
                2 << ((n & 1) * 2)
            } else {
                (n + 1) * 3
            };
            if restart != 0 && mask != restart {
                continue;
            }
            let mut full = mask;
            for sid in 0..4u8 {
                if mask & (1 << sid) != 0 {
                    full |= self.instances[sid as usize].mask;
                }
            }
            if full & admitted != 0 {
                continue;
            }
            let mut valid = true;
            let mut highest = 0;
            let mut occupied = 0;
            let mut youngest = 255;
            for sid in 0..4u8 {
                let old = self.instances[sid as usize];
                if full & (1 << sid) == 0 || old.mask == 0 {
                    continue;
                }
                let priority = old.effect.priority();
                if priority > event.effect.priority()
                    || (event.effect.is_gun() && old.effect == event.effect && old.age < 2)
                {
                    valid = false;
                    break;
                }
                highest = highest.max(priority);
                occupied += 1;
                youngest = youngest.min(old.age);
            }
            // Idle costs zero; every occupied allocation costs at least one.
            let cost = highest * 8 + occupied;
            if valid && (cost < best_cost || cost == best_cost && youngest > best_age) {
                best_cost = cost;
                best_age = youngest;
                best = mask;
                victims = full;
                // A free candidate has cost zero and age 255: no later
                // candidate can improve it, including the tie-breaker.
                if cost == 0 {
                    break;
                }
            }
        }
        if best == 0 {
            return 0;
        }
        self.stop(bus, victims);
        let p = unsafe { PATCHES.get_unchecked(event.effect as usize) };
        for sid in 0..4u8 {
            if best & (1 << sid) == 0 {
                continue;
            }
            let mut frequency = [0; 3];
            for voice in 0..3u8 {
                let base = voice * 7;
                let pitch = unsafe {
                    FREQUENCIES.get_unchecked(p.notes[voice as usize] as usize)
                        [self.standard as usize]
                };
                let pitch = pitch + (pitch >> 9) * ((sid & 1) as u16);
                frequency[voice as usize] = pitch;
                self.word(bus, sid, base, pitch);
                self.word(bus, sid, base + 2, u16::from(p.pulse[voice as usize]) << 8);
                self.write(bus, sid, base + 5, p.ad[voice as usize]);
                self.write(bus, sid, base + 6, p.sr[voice as usize]);
                // Keep a complete gate-low tick before the next attack.
                self.write(bus, sid, base + 4, p.waves[voice as usize]);
            }
            let cutoff = u16::from(p.cutoff[0]) << 3;
            self.write(bus, sid, 21, (cutoff & 7) as u8);
            self.write(bus, sid, 22, (cutoff >> 3) as u8);
            self.write(bus, sid, 23, (p.resonance & 0xf0) | (p.filter & 7));
            let punch = p.late_resonance & 15;
            self.write(
                bus,
                sid,
                24,
                (p.filter & 0xf0) | if punch != 0 { punch_volume(punch, 0) } else { VOLUME },
            );
            self.instances[sid as usize] = Instance {
                mask: best,
                effect: event.effect,
                age: 0,
                remaining: p.duration,
                frequency,
                cutoff,
                program: if lane == 3 && sid & 1 != 0 {
                    p.programs >> 4
                } else {
                    p.programs & 15
                },
            };
        }
        self.rotation = (self.rotation + 1) & 3;
        best
    }
    #[cfg(feature = "host-tools")]
    pub fn image(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts((self as *const Self).cast(), core::mem::size_of::<Self>())
        }
    }
    #[cfg(feature = "host-tools")]
    pub fn snapshot(&self) -> Self {
        self.clone()
    }
    #[cfg(feature = "host-tools")]
    pub fn restore(&mut self, snapshot: &Self) {
        *self = snapshot.clone();
    }
}

// Shared by the foreground game queue and the IRQ queue. All owners maintain
// head<8, count<=8, and remove only live indices. Masked byte indices avoid
// expensive 16-bit indexing in the bounded 6502 queues. This stays compiled
// Rust natively: an external callee would make every SIM caller of
// Game::sound spill around the call under interprocedural register allocation.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".sfx_helpers")]
pub(crate) fn queue_events(
    events: &mut [AudioEvent; 8],
    head: u8,
    count: &mut u8,
    event: AudioEvent,
) -> bool {
    let mut lowest = 0u8;
    for offset in 0..*count {
        let i = ((head + offset) & 7) as usize;
        let low = ((head + lowest) & 7) as usize;
        if events[i].coalesces(event) {
            return true;
        }
        if events[i].effect.priority() < events[low].effect.priority() {
            lowest = offset;
        }
    }
    if *count == 8 {
        if event.effect.priority() < events[((head + lowest) & 7) as usize].effect.priority() {
            return false;
        }
        remove_event(events, head, count, lowest);
    }
    events[((head + *count) & 7) as usize] = event;
    *count += 1;
    true
}
// Resident: the queue helper above and cancel_scene_effects share it; the
// IRQ engine's own event removal runs in the assembly kernel.
#[inline(never)]
fn remove_event(events: &mut [AudioEvent; 8], head: u8, count: &mut u8, index: u8) -> AudioEvent {
    let event = events[((head + index) & 7) as usize];
    *count -= 1;
    for offset in index..*count {
        let i = (head + offset) & 7;
        events[i as usize] = events[((i + 1) & 7) as usize];
    }
    // Bytes outside count are never read; leave the retired slot untouched.
    event
}
