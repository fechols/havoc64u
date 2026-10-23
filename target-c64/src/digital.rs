//! Foreground DLT4 player. IRQs own effects, never these decoder contexts or REU.
use havoc::digital::{self as asset, Metadata, Track};
use havoc::digital_clock::{self as timing, SampleClock, RING_SAMPLES};

const STAGING: u16 = 0x8e00;
const BOOK_STAGING: u16 = STAGING + 768;
const CHECK_STAGING: u16 = STAGING + 1023;
const MAX_BATCH: u16 = 128;
// Nominal decoded audio duration, shared by both slots on each AUDIO visit.
// The validated profiles are 20 kHz stereo and 16 kHz mono.
const STEREO_FRAME_BUDGET: u16 = 5_000;
const MONO_FRAME_BUDGET: u16 = 4_000;
const SAMPLER: u16 = 0xdf20;
const RIGHT: u16 = SAMPLER + 0x20;
const NONE: u8 = 255;
const DELAY: u32 = 160;

// The input cache holds 32 bytes and the reservoir at most seven bits. Avoid
// a general divide for every short cache run on the native processor.
const fn available_codes() -> [u8; 264] {
    let mut values = [0; 264];
    let mut bits = 0;
    while bits < values.len() {
        values[bits] = (bits / 3) as u8;
        bits += 1;
    }
    values
}
#[link_section = ".phase.audio.data"]
static AVAILABLE_CODES: [u8; 264] = available_codes();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Fault {
    Mapping = 1,
    CalibrationTimeout = 2,
    Calibration = 3,
    Metadata = 4,
    Underrun = 5,
    StreamCrc = 6,
    CodebookChecksum = 7,
    CrcTableChecksum = 8,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Rate {
    ticks: u32,
    divider: u16,
}
impl Rate {
    const EMPTY: Self = Self {
        ticks: 0,
        divider: 0,
    };
}
/// Survives music backend replacement. Both rates are measured before playback.
pub struct Calibration {
    rates: [Rate; 2],
    valid: bool,
    pal: bool,
}
impl Calibration {
    pub const fn new() -> Self {
        Self {
            rates: [Rate::EMPTY; 2],
            valid: false,
            pal: false,
        }
    }
}

/// Fixed layout shared with digital_decode_run. Predictor precision is independent
/// of the rounded publication format; bit state survives cache and batch edges.
#[repr(C)]
struct Kernel {
    book: [i16; 16],
    samples: [i16; 2],
    reservoir: u16,
    bits: u8,
    channels: u8,
}
const _: () = assert!(core::mem::size_of::<Kernel>() == 40);
#[cfg(not(target_arch = "mos"))]
impl Kernel {
    const EMPTY: Self = Self {
        book: [0; 16],
        samples: [0; 2],
        reservoir: 0,
        bits: 0,
        channels: 2,
    };
}

#[repr(C)]
struct Decoder {
    metadata: Metadata,
    asset_offset: u32,
    produced: u32,
    decoded: u32,
    stream_pos: u32,
    segment_left: u32,
    kernel: Kernel,
    cache: [u8; 32],
    segment: u16,
    loops: u16,
    cache_pos: u8,
    cache_len: u8,
    crc: u8,
    track: Track,
}
#[cfg(not(target_arch = "mos"))]
impl Decoder {
    const EMPTY: Self = Self {
        metadata: Metadata::EMPTY,
        asset_offset: 0,
        produced: 0,
        decoded: 0,
        stream_pos: 0,
        segment_left: 0,
        kernel: Kernel::EMPTY,
        cache: [0; 32],
        segment: 0,
        loops: 0,
        cache_pos: 0,
        cache_len: 0,
        crc: 0,
        track: Track::GalacticDash,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Transport {
    // clear() zero-initializes the state.
    Stopped = 0,
    Priming,
    Running,
}

#[repr(C)]
pub struct State {
    decoders: [Decoder; 2],
    clock: SampleClock,
    last_tick: u32,
    rates: [Rate; 2],
    pub loops: u16,
    pub underruns: u16,
    pub minimum_lead: u16,
    active: u8,
    pending: u8,
    transport: Transport,
    pub fault: u8,
    pub track: Track,
}
const _: () = assert!(core::mem::size_of::<State>() <= 392);
#[cfg(not(target_arch = "mos"))]
impl State {
    pub const fn new() -> Self {
        Self {
            decoders: [Decoder::EMPTY, Decoder::EMPTY],
            clock: SampleClock::new(),
            last_tick: 0,
            rates: [Rate::EMPTY; 2],
            loops: 0,
            underruns: 0,
            minimum_lead: RING_SAMPLES as u16,
            active: 0,
            pending: NONE,
            transport: Transport::Stopped,
            fault: 0,
            track: Track::GalacticDash,
        }
    }
}

impl State {
    #[inline(always)]
    pub fn is_running(&self) -> bool {
        self.transport == Transport::Running
    }
}

#[inline(always)]
unsafe fn poke(a: u16, v: u8) {
    super::poke(a, v);
}
#[inline(always)]
unsafe fn peek(a: u16) -> u8 {
    super::peek(a)
}
#[inline(always)]
unsafe fn be24(a: u16, v: u32) {
    poke(a, (v >> 16) as u8);
    poke(a + 1, (v >> 8) as u8);
    poke(a + 2, v as u8);
}
#[inline(always)]
unsafe fn clear_sampler_irq() {
    poke(SAMPLER + 0x1f, 255);
}
#[inline(always)]
fn rate_index(channels: u8) -> usize {
    if channels == 2 {
        0
    } else {
        1
    }
}
#[inline(always)]
fn ring(slot: u8, channels: u8) -> u32 {
    asset::RING_OFFSET + ((slot as u32) << 16) + if channels == 1 { 0x8000 } else { 0 }
}

#[inline(always)]
unsafe fn mute() {
    poke(SAMPLER + 1, 0);
    poke(RIGHT + 1, 0);
    poke(SAMPLER, 0);
    poke(RIGHT, 0);
    clear_sampler_irq();
}
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn stop(state: &mut State) {
    mute();
    state.transport = Transport::Stopped;
}
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn failed(state: &mut State, fault: Fault) -> Fault {
    stop(state);
    state.fault = fault as u8;
    if fault == Fault::Underrun {
        state.underruns = state.underruns.wrapping_add(1);
    }
    fault
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn configure(slot: u8, channels: u8, divider: u16, frames: u32, repeating: bool) {
    let base = ring(slot, channels);
    let bytes = if channels == 2 { frames << 1 } else { frames };
    let mut channel = SAMPLER;
    loop {
        let right = channel == RIGHT;
        let delay = if repeating && channels == 1 && right {
            DELAY
        } else {
            0
        };
        poke(channel, 0);
        poke(channel + 1, 0);
        poke(channel + 2, if right { 15 } else { 0 });
        poke(channel + 4, 1);
        be24(
            channel + 5,
            base + if channels == 2 && right { 1 } else { 0 } - delay,
        );
        be24(
            channel + 9,
            bytes + delay + if repeating { channels as u32 } else { 0 },
        );
        poke(channel + 14, (divider >> 8) as u8);
        poke(channel + 15, divider as u8);
        be24(channel + 17, delay);
        be24(channel + 21, if repeating { bytes + delay } else { 0 });
        if right {
            break;
        }
        channel = RIGHT;
    }
    clear_sampler_irq();
}

#[inline(never)]
#[link_section = ".phase.boot"]
unsafe fn timer_init() {
    poke(0xdd0d, 0x7f);
    let _ = peek(0xdd0d);
    super::word(0xdd04, 65535);
    super::word(0xdd06, 65535);
    poke(0xdd0e, 0x11);
    poke(0xdd0f, 0x51);
}
#[inline(always)]
unsafe fn timer_word(a: u16) -> u16 {
    loop {
        let h = peek(a + 1);
        let l = peek(a);
        if h == peek(a + 1) {
            return ((h as u16) << 8) | l as u16;
        }
    }
}
#[inline(always)]
unsafe fn timer_read() -> u32 {
    loop {
        let h = timer_word(0xdd06);
        let l = timer_word(0xdd04);
        if h == timer_word(0xdd06) {
            return ((h as u32) << 16) | l as u32;
        }
    }
}
#[inline(always)]
unsafe fn stash(destination: u32, bytes: u16) {
    super::word(0xdf02, STAGING);
    poke(0xdf04, destination as u8);
    poke(0xdf05, (destination >> 8) as u8);
    poke(0xdf06, (destination >> 16) as u8);
    super::word(0xdf07, bytes);
    poke(0xdf09, 0);
    poke(0xdf0a, 0);
    poke(0xdf01, 0x90);
}
#[inline(never)]
#[link_section = ".phase.boot"]
unsafe fn checksum_region(mut source: u32, mut bytes: u32, expected: u16) -> bool {
    let (mut a, mut b) = (0u16, 0u16);
    while bytes != 0 {
        let count = bytes.min(1024) as u16;
        super::dma(source, STAGING, count);
        for i in 0..count {
            a += peek(STAGING + i) as u16;
            if a >= 255 {
                a -= 255;
            }
            b += a;
            if b >= 255 {
                b -= 255;
            }
        }
        source += count as u32;
        bytes -= count as u32;
    }
    ((b << 8) | a) == expected
}
#[inline(never)]
#[link_section = ".phase.boot"]
unsafe fn validate(track: Track) -> Result<(), Fault> {
    let d = asset::descriptor_startup(track);
    super::dma(d.asset_offset, STAGING, 32);
    #[cfg(not(test))]
    let header = core::slice::from_raw_parts(STAGING as *const u8, 32);
    #[cfg(test)]
    let copy = {
        let mut b = [0; 32];
        for i in 0..32 {
            b[i] = peek(STAGING + i as u16);
        }
        b
    };
    #[cfg(test)]
    let header = &copy;
    asset::parse_header(header, track).map_err(|_| Fault::Metadata)?;
    if !checksum_region(
        d.asset_offset + 32,
        d.metadata.stream_offset - 32,
        d.codebook_checksum,
    ) {
        return Err(Fault::CodebookChecksum);
    }
    if !checksum_region(
        d.asset_offset + d.metadata.crc_offset,
        (d.metadata.stream_bytes + 1023) >> 10,
        d.crc_table_checksum,
    ) {
        return Err(Fault::CrcTableChecksum);
    }
    Ok(())
}
#[inline(never)]
#[link_section = ".phase.boot"]
unsafe fn measure(channels: u8, divider: u16, frames: u16, wide: bool) -> Result<u32, Fault> {
    // All one-shot extents fit u16 (the wide clear is only four frames).
    // Keep the shift narrow: LLVM otherwise turns selects into a 32-bit multiply.
    let length = (frames
        << if wide {
            2
        } else if channels == 2 {
            1
        } else {
            0
        }) as u32;
    for channel in [SAMPLER, RIGHT] {
        poke(channel, 0);
        poke(channel + 1, 0);
        poke(channel + 2, 0);
        poke(channel + 4, 1);
        be24(
            channel + 5,
            asset::RING_OFFSET
                + if channel == RIGHT && channels == 2 {
                    if wide {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                },
        );
        be24(channel + 9, length);
        be24(channel + 17, 0);
        be24(channel + 21, 0);
        poke(channel + 14, (divider >> 8) as u8);
        poke(channel + 15, divider as u8);
    }
    clear_sampler_irq();
    let control = if wide {
        0x50
    } else if channels == 2 {
        0x40
    } else {
        0
    };
    let begin = timer_read();
    poke(SAMPLER, control | 5);
    poke(RIGHT, control | 1);
    let mut budget = 8_000_000u32;
    while peek(SAMPLER) == 0 {
        if begin.wrapping_sub(timer_read()) > 8_000_000 || budget == 0 {
            mute();
            return Err(Fault::CalibrationTimeout);
        }
        budget -= 1;
    }
    let ticks = begin.wrapping_sub(timer_read());
    mute();
    Ok(ticks)
}
#[inline(never)]
#[link_section = ".phase.boot"]
unsafe fn measure_pair(channels: u8, divider: u16) -> Result<u32, Fault> {
    let a = measure(channels, divider, 4096, false)?;
    let b = measure(channels, divider, 16384, false)?;
    if b <= a {
        Err(Fault::Calibration)
    } else {
        Ok(b - a)
    }
}
#[inline(never)]
#[link_section = ".phase.boot"]
unsafe fn calibrate(channels: u8, pal: bool) -> Result<Rate, Fault> {
    let initial = if channels == 2 { 312u16 } else { 390 };
    let target = if channels == 2 {
        if pal {
            605_336
        } else {
            628_363
        }
    } else if pal {
        756_670
    } else {
        785_454
    };
    let step = if pal { 1937 } else { 2011 };
    let mut divider = initial;
    let mut ticks = measure_pair(channels, divider)?;
    let difference = ticks.abs_diff(target);
    let mut remainder = difference + step / 2;
    let mut correction = 0u16;
    while remainder >= step {
        remainder -= step;
        correction += 1;
    }
    let candidate = if ticks >= target {
        initial.saturating_sub(correction).max(1)
    } else {
        initial.saturating_add(correction)
    };
    if candidate != initial {
        let adjusted = measure_pair(channels, candidate)?;
        if adjusted.abs_diff(target) < difference {
            ticks = adjusted;
            divider = candidate;
        }
    }
    if !(250_000..=1_100_000).contains(&ticks) {
        return Err(Fault::Calibration);
    }
    Ok(Rate { ticks, divider })
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn load_book(d: &mut Decoder) {
    let per = if d.kernel.channels == 2 { 48u32 } else { 24 };
    // At most 1024 books: the complete offset fits the native sixteen-bit word.
    let offset = d.segment * 24u16;
    let offset = if d.kernel.channels == 2 {
        offset << 1
    } else {
        offset
    };
    super::dma(
        d.asset_offset + 32 + offset as u32,
        BOOK_STAGING,
        per as u16,
    );
    for i in 0..(d.kernel.channels as usize * 8) {
        let a = BOOK_STAGING + i as u16 * 3;
        d.kernel.book[i] = asset::decode_delta_entry([peek(a), peek(a + 1), peek(a + 2)]);
    }
}
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn reset_decoder(d: &mut Decoder) {
    d.decoded = 0;
    d.stream_pos = 0;
    d.cache_pos = 0;
    d.cache_len = 0;
    d.crc = 0;
    d.kernel.samples = [0; 2];
    d.kernel.reservoir = 0;
    d.kernel.bits = 0;
    d.segment = 0;
    d.segment_left = d.metadata.samples_per_segment;
    load_book(d);
}
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn prepare(d: &mut Decoder, slot: u8, track: Track) {
    let descriptor = track.descriptor();
    core::ptr::write_bytes(d, 0, 1);
    d.track = track;
    d.metadata = descriptor.metadata;
    d.asset_offset = descriptor.asset_offset;
    d.kernel.channels = d.metadata.channels;
    if d.kernel.channels == 1 {
        for i in 0..DELAY as u16 {
            poke(STAGING + i, 0);
        }
        stash(ring(slot, 1) - DELAY, DELAY as u16);
    }
    reset_decoder(d);
}
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn refill_cache(d: &mut Decoder) -> Result<(), Fault> {
    if d.cache_pos < d.cache_len {
        return Ok(());
    }
    if d.stream_pos >= d.metadata.stream_bytes {
        return Err(Fault::Metadata);
    }
    d.cache_len = (d.metadata.stream_bytes - d.stream_pos).min(32) as u8;
    let source = d.asset_offset + d.metadata.stream_offset + d.stream_pos;
    #[cfg(not(test))]
    super::dma(source, d.cache.as_mut_ptr() as u16, d.cache_len as u16);
    #[cfg(test)]
    super::digital_test_fetch(source, &mut d.cache[..d.cache_len as usize]);
    for &b in &d.cache[..d.cache_len as usize] {
        d.crc = asset::crc_byte(d.crc, b);
    }
    d.stream_pos += d.cache_len as u32;
    d.cache_pos = 0;
    if d.stream_pos & 1023 == 0 || d.stream_pos == d.metadata.stream_bytes {
        super::dma(
            d.asset_offset + d.metadata.crc_offset + ((d.stream_pos - 1) >> 10),
            CHECK_STAGING,
            1,
        );
        if peek(CHECK_STAGING) != d.crc {
            return Err(Fault::StreamCrc);
        }
    }
    Ok(())
}

#[cfg(target_arch = "mos")]
extern "C" {
    fn digital_decode_run(kernel: *mut Kernel, input: *const u8, output: *mut u8, frames: u8)
        -> u8;
}

#[inline(always)]
#[cfg(not(target_arch = "mos"))]
pub fn rounded_sample(sample: i16) -> u8 {
    ((sample as i32 + 128) >> 8).clamp(-128, 127) as i8 as u8
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn decode(d: &mut Decoder, frames: u16) -> Result<(), Fault> {
    let mut left = frames;
    let mut output = STAGING;
    while left != 0 {
        if d.decoded == d.metadata.total_samples {
            reset_decoder(d);
            d.loops = d.loops.wrapping_add(1);
        }
        if d.segment_left == 0 && d.segment + 1 < d.metadata.codebook_count {
            d.segment += 1;
            d.segment_left = d.metadata.samples_per_segment;
            load_book(d);
        }
        // A final frame can reside entirely in the bit reservoir after consuming
        // the last byte. Never demand a nonexistent byte merely to finish it.
        let needed = 3 * d.kernel.channels;
        if d.kernel.bits < needed && d.cache_pos == d.cache_len {
            refill_cache(d)?;
        }
        let bits = (d.cache_len - d.cache_pos) as usize * 8 + d.kernel.bits as usize;
        let codes = *AVAILABLE_CODES.get_unchecked(bits) as u16;
        let available = if d.kernel.channels == 2 {
            codes >> 1
        } else {
            codes
        };
        let segment = if d.segment + 1 == d.metadata.codebook_count {
            d.metadata.total_samples - d.decoded
        } else {
            d.segment_left
        };
        let count = left
            .min(available)
            .min(segment.min(128) as u16)
            .min((d.metadata.total_samples - d.decoded).min(128) as u16);
        if count == 0 {
            return Err(Fault::Metadata);
        }
        #[cfg(target_arch = "mos")]
        {
            let used = digital_decode_run(
                &mut d.kernel,
                d.cache.as_ptr().add(d.cache_pos as usize),
                output as *mut u8,
                count as u8,
            );
            d.cache_pos += used;
        }
        #[cfg(not(target_arch = "mos"))]
        {
            let mut at = output;
            for _ in 0..count {
                for channel in 0..d.kernel.channels as usize {
                    if d.kernel.bits < 3 {
                        d.kernel.reservoir |=
                            (d.cache[d.cache_pos as usize] as u16) << d.kernel.bits;
                        d.cache_pos += 1;
                        d.kernel.bits += 8;
                    }
                    let code = (d.kernel.reservoir & 7) as usize;
                    d.kernel.reservoir >>= 3;
                    d.kernel.bits -= 3;
                    d.kernel.samples[channel] =
                        d.kernel.samples[channel].saturating_add(d.kernel.book[channel * 8 + code]);
                    poke(at, rounded_sample(d.kernel.samples[channel]));
                    at += 1;
                }
            }
        }
        d.decoded += count as u32;
        d.segment_left = d.segment_left.saturating_sub(count as u32);
        left -= count;
        output += if d.kernel.channels == 2 {
            count << 1
        } else {
            count
        };
    }
    Ok(())
}
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn advance(state: &mut State) -> Result<u32, Fault> {
    let now = timer_read();
    let ticks = state.last_tick.wrapping_sub(now);
    state.last_tick = now;
    state.clock.advance(ticks);
    let d = &mut state.decoders[state.active as usize];
    if timing::underrun(d.produced, state.clock.samples) {
        return Err(Fault::Underrun);
    }
    state.clock.rebase(&mut d.produced);
    state.minimum_lead = state
        .minimum_lead
        .min((d.produced - state.clock.samples) as u16);
    Ok(state.clock.samples)
}
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn produce(state: &mut State, slot: u8, frames: u16) -> Result<(), Fault> {
    decode(&mut state.decoders[slot as usize], frames)?;
    if state.is_running() {
        advance(state)?;
    }
    let d = &mut state.decoders[slot as usize];
    let offset = d.produced & (RING_SAMPLES - 1);
    let bytes = if d.kernel.channels == 2 {
        frames << 1
    } else {
        frames
    };
    stash(
        ring(slot, d.kernel.channels)
            + if d.kernel.channels == 2 {
                offset << 1
            } else {
                offset
            },
        bytes,
    );
    d.produced += frames as u32;
    Ok(())
}

/// Called while BOOT is loaded; no AUDIO code is callable from here.
#[inline(never)]
#[link_section = ".phase.boot"]
pub unsafe fn bootstrap(saved: &mut Calibration, pal: bool) -> Result<(), Fault> {
    mute();
    if peek(SAMPLER + 1) != 0x10 {
        return Err(Fault::Mapping);
    }
    timer_init();
    if !checksum_region(
        asset::MUSIC_OFFSET,
        asset::DIRECTORY_BYTES as u32,
        asset::DIRECTORY_CHECKSUM,
    ) {
        return Err(Fault::Metadata);
    }
    // Iterate IDs so adding recordings does not materialize a Track::ALL copy
    // in scarce resident constant RAM. Descriptors remain in the BOOT image.
    for id in 0..Track::ALL.len() {
        validate(Track::from_id(id as u8).ok_or(Fault::Metadata)?)?;
    }
    for i in 0..1024 {
        poke(STAGING + i, 0);
    }
    let mut offset = 0;
    while offset < asset::RING_BYTES {
        stash(asset::RING_OFFSET + offset, 1024);
        offset += 1024;
    }
    measure(2, 312, 4, true)?;
    saved.rates[0] = calibrate(2, pal)?;
    saved.rates[1] = calibrate(1, pal)?;
    saved.pal = pal;
    saved.valid = true;
    Ok(())
}
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn clear(state: *mut State) {
    core::ptr::write_bytes(state, 0, 1);
    (*state).pending = NONE;
    (*state).minimum_lead = RING_SAMPLES as u16;
}
/// Prepare without decoding PCM. service() primes the ring over bounded visits.
/// The target already validated/calibrated in BOOT; tests exercise that path here.
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn init(
    state: &mut State,
    saved: &mut Calibration,
    pal: bool,
    track: Track,
) -> Result<(), Fault> {
    stop(state);
    clear(state);
    let result = (|| {
        #[cfg(test)]
        if !saved.valid || saved.pal != pal {
            bootstrap(saved, pal)?;
        }
        if !saved.valid || saved.pal != pal {
            return Err(Fault::Calibration);
        }
        state.rates = saved.rates;
        prepare(&mut state.decoders[0], 0, track);
        state.track = track;
        state.transport = Transport::Priming;
        Ok(())
    })();
    result.map_err(|fault| failed(state, fault))
}
#[inline(never)]
#[link_section = ".phase.audio"]
// Only service() activates a fully primed ring, with IRQs masked across both
// gate writes so the two sampler clocks cannot start on different IRQ ticks.
unsafe fn start(state: &mut State) {
    let channels = state.decoders[state.active as usize].kernel.channels;
    let rate = state.rates[rate_index(channels)];
    configure(state.active, channels, rate.divider, RING_SAMPLES, true);
    state.clock.calibrate(rate.ticks);
    poke(SAMPLER + 1, 32);
    poke(RIGHT + 1, 32);
    state.last_tick = timer_read();
    let control = if channels == 2 { 0x43 } else { 0x03 };
    poke(SAMPLER, control);
    poke(RIGHT, control);
    state.transport = Transport::Running;
}
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn request(state: &mut State, track: Track) {
    if state.transport == Transport::Stopped {
        return;
    }
    if state.track == track {
        state.pending = NONE;
        return;
    }
    if state.transport == Transport::Priming {
        prepare(&mut state.decoders[state.active as usize], state.active, track);
        state.track = track;
        state.pending = NONE;
        return;
    }
    if state.pending != NONE && state.decoders[state.pending as usize].track == track {
        return;
    }
    let slot = state.active ^ 1;
    prepare(&mut state.decoders[slot as usize], slot, track);
    state.pending = slot;
}
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn service(state: &mut State) -> Result<(), Fault> {
    if state.transport == Transport::Stopped {
        return Ok(());
    }
    let result = (|| {
        let channels = state.decoders[state.active as usize].kernel.channels;
        let mut budget = if channels == 2 {
            STEREO_FRAME_BUDGET
        } else {
            MONO_FRAME_BUDGET
        };
        if state.transport == Transport::Priming {
            while budget != 0 {
                let remaining = RING_SAMPLES - state.decoders[state.active as usize].produced;
                if remaining == 0 {
                    break;
                }
                let count = remaining.min(MAX_BATCH as u32).min(budget as u32) as u16;
                produce(state, state.active, count)?;
                budget -= count;
            }
            if state.decoders[state.active as usize].produced == RING_SAMPLES {
                #[cfg(not(test))]
                super::disable_irq();
                start(state);
                #[cfg(not(test))]
                super::enable_irq();
            }
            return Ok(());
        }
        advance(state)?;
        loop {
            // produce() advances the clock after decoding and before DMA.
            // Using that observation here is conservative: publication can
            // only move the reader forward and create additional free space.
            let consumed = state.clock.samples;
            let count = timing::refill_samples(
                state.decoders[state.active as usize].produced,
                consumed,
                MAX_BATCH,
            );
            if count == 0 || budget == 0 {
                break;
            }
            let count = count.min(budget);
            produce(state, state.active, count)?;
            budget -= count;
        }
        // Convert the unused duration once, rounding down. The inner loops
        // only subtract frame counts; no per-batch division or extra timer.
        let mut pending_budget = if state.pending == NONE
            || state.decoders[state.pending as usize].kernel.channels == channels
        {
            budget
        } else if channels == 2 {
            timing::quotient_remainder((budget << 2) as u32, 5).0 as u16
        } else {
            budget + (budget >> 2)
        };
        while state.pending != NONE && pending_budget != 0 {
            let slot = state.pending;
            let remaining = RING_SAMPLES - state.decoders[slot as usize].produced;
            if remaining == 0 {
                break;
            }
            let count = remaining.min(MAX_BATCH as u32).min(pending_budget as u32) as u16;
            produce(state, slot, count)?;
            pending_budget -= count;
        }
        if state.pending != NONE && state.decoders[state.pending as usize].produced == RING_SAMPLES
        {
            #[cfg(not(test))]
            super::disable_irq();
            stop(state);
            state.active = state.pending;
            state.pending = NONE;
            state.track = state.decoders[state.active as usize].track;
            state.minimum_lead = RING_SAMPLES as u16;
            start(state);
            #[cfg(not(test))]
            super::enable_irq();
        }
        state.loops = state.decoders[state.active as usize].loops;
        Ok(())
    })();
    result.map_err(|fault| failed(state, fault))
}
