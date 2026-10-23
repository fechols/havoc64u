//! Foreground-only 24 kHz stereo delta decoder and Ultimate Audio sampler.
//! Adapted from secondreality-opt's audio_backend/ultimate_audio (Unlicense).
//! The IRQ never borrows State. Only its owner may replace this SID/DAC union.

use havoc::digital::{self as asset, Metadata, Track};
use havoc::digital_clock::{self as timing, SampleClock, RING_SAMPLES};

const STAGING: u16 = 0x8e00;
const BOOK_STAGING: u16 = STAGING + 768;
const CHECK_STAGING: u16 = STAGING + 1023;
const MAX_BATCH: u16 = 128;
const SAMPLER: u16 = 0xdf20;
const RIGHT: u16 = SAMPLER + 0x20;
const INITIAL_DIVIDER: u16 = 260;

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

/// Preserved outside the SID/DAC union so returning to the title does not
/// repeat the initial muted calibration. Timers may be restarted independently.
#[repr(C)]
pub struct Calibration {
    ticks: u32,
    divider: u16,
    valid: bool,
    pal: bool,
}

impl Calibration {
    pub const fn new() -> Self {
        Self {
            ticks: 0,
            divider: INITIAL_DIVIDER,
            valid: false,
            pal: false,
        }
    }
}

#[repr(C)]
pub struct State {
    metadata: Metadata,
    asset_offset: u32,
    clock: SampleClock,
    last_tick: u32,
    produced: u32,
    decoded: u32,
    stream_pos: u32,
    segment_left: u32,
    cache: [u8; 128],
    codebook: [i16; 32],
    cache_pos: u16,
    cache_len: u16,
    left: i16,
    right: i16,
    segment: u16,
    divider: u16,
    pub loops: u16,
    pub underruns: u16,
    pub minimum_lead: u16,
    crc: u8,
    running: bool,
    ready: bool,
    pub fault: u8,
    pub track: Track,
}

const _: () = assert!(core::mem::size_of::<State>() <= 400);

impl State {
    pub const fn new() -> Self {
        Self {
            metadata: Metadata {
                total_samples: 0,
                stream_bytes: 0,
                samples_per_segment: 0,
                stream_offset: 0,
                crc_offset: 0,
                codebook_count: 0,
            },
            asset_offset: 0,
            clock: SampleClock::new(),
            last_tick: 0,
            produced: 0,
            decoded: 0,
            stream_pos: 0,
            segment_left: 0,
            cache: [0; 128],
            codebook: [0; 32],
            cache_pos: 0,
            cache_len: 0,
            left: 0,
            right: 0,
            segment: 0,
            divider: INITIAL_DIVIDER,
            loops: 0,
            underruns: 0,
            minimum_lead: 0,
            crc: 0,
            running: false,
            ready: false,
            fault: 0,
            track: Track::GalacticDash,
        }
    }
}

#[inline(always)]
unsafe fn poke(address: u16, value: u8) {
    super::poke(address, value);
}
#[inline(always)]
unsafe fn peek(address: u16) -> u8 {
    super::peek(address)
}

#[inline(always)]
unsafe fn be24(address: u16, value: u32) {
    poke(address, (value >> 16) as u8);
    poke(address + 1, (value >> 8) as u8);
    poke(address + 2, value as u8);
}

#[inline(always)]
unsafe fn clear_sampler_irq() {
    poke(SAMPLER + 0x1f, 0xff);
}

/// Gating off alone holds the last DAC output. Muting is required as well.
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn stop(state: &mut State) {
    poke(SAMPLER + 1, 0);
    poke(RIGHT + 1, 0);
    poke(SAMPLER, 0);
    poke(RIGHT, 0);
    clear_sampler_irq();
    state.running = false;
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn failed(state: &mut State, fault: Fault) -> Fault {
    stop(state);
    state.ready = false;
    state.fault = fault as u8;
    if fault == Fault::Underrun {
        state.underruns = state.underruns.wrapping_add(1);
    }
    fault
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn configure(divider: u16, length: u32) {
    let mut channel = SAMPLER;
    loop {
        poke(channel, 0);
        poke(channel + 1, 0);
        poke(channel + 2, if channel == SAMPLER { 0 } else { 15 });
        poke(channel + 4, 1); // address-space selector: REU
        be24(
            channel + 5,
            asset::RING_OFFSET + if channel == SAMPLER { 0 } else { 2 },
        );
        be24(channel + 9, length);
        poke(channel + 14, (divider >> 8) as u8);
        poke(channel + 15, divider as u8);
        // Repeat precedes the terminal-length test, even with repeat off.
        // A one-shot must leave repeat B at zero.
        be24(channel + 17, 0);
        be24(channel + 21, 0);
        if channel == RIGHT {
            break;
        }
        channel = RIGHT;
    }
    clear_sampler_irq();
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn timer_init() {
    poke(0xdd0d, 0x7f); // CIA2 interrupts are NMIs, never sampler pacing IRQs.
    let _ = peek(0xdd0d);
    super::word(0xdd04, 0xffff);
    super::word(0xdd06, 0xffff);
    poke(0xdd0e, 0x11);
    poke(0xdd0f, 0x51); // B counts A underflows
}

#[inline(always)]
unsafe fn timer_word(address: u16) -> u16 {
    loop {
        let high = peek(address + 1);
        let low = peek(address);
        if high == peek(address + 1) {
            return ((high as u16) << 8) | low as u16;
        }
    }
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn timer_read() -> u32 {
    loop {
        let before = timer_word(0xdd06);
        let low = timer_word(0xdd04);
        if before == timer_word(0xdd06) {
            return ((before as u32) << 16) | low as u32;
        }
    }
}

/// Staging is at most 1024 bytes. The stereo ring spans two REU banks;
/// preserve its complete address. No IRQ issues REU transfers.
#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn stash(offset: u32, bytes: u16) {
    let destination = asset::RING_OFFSET + offset;
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
#[link_section = ".phase.audio"]
unsafe fn checksum_region(mut source: u32, mut bytes: u32, expected: u16) -> bool {
    let mut a = 0u16;
    let mut b = 0u16;
    while bytes != 0 {
        let count = bytes.min(1024) as u16;
        super::dma(source, STAGING, count);
        let mut i = 0;
        while i < count {
            a += peek(STAGING + i) as u16;
            if a >= 255 {
                a -= 255;
            }
            b += a;
            if b >= 255 {
                b -= 255;
            }
            i += 1;
        }
        source += count as u32;
        bytes -= count as u32;
    }
    ((b << 8) | a) == expected
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn measure(state: &mut State, frames: u16) -> Result<u32, Fault> {
    configure(state.divider, (frames as u32) << 2);
    let begin = timer_read();
    poke(SAMPLER, 0x55); // interleaved signed16; completion IRQ only on left
    poke(RIGHT, 0x51);
    let mut budget = 8_000_000u32;
    while peek(SAMPLER) == 0 {
        if begin.wrapping_sub(timer_read()) > 8_000_000 || budget == 0 {
            return Err(failed(state, Fault::CalibrationTimeout));
        }
        budget -= 1;
    }
    let ticks = begin.wrapping_sub(timer_read());
    stop(state);
    Ok(ticks)
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn measure_pair(state: &mut State) -> Result<u32, Fault> {
    let short = measure(state, 4096)?;
    let long = measure(state, 16384)?;
    if long <= short {
        return Err(failed(state, Fault::Calibration));
    }
    Ok(long - short)
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn calibrate(state: &mut State, saved: &mut Calibration, pal: bool) -> Result<(), Fault> {
    if saved.valid && saved.pal == pal {
        state.divider = saved.divider;
        if state.clock.calibrate(saved.ticks) {
            return Ok(());
        }
    }
    saved.valid = false;
    state.divider = INITIAL_DIVIDER;
    let mut ticks = measure_pair(state)?;
    let target = if pal { 504_447u32 } else { 523_636u32 };
    let step = if pal { 1937 } else { 2011 };
    let difference = ticks.abs_diff(target);
    let correction = timing::quotient_remainder(difference + step / 2, step)
        .0
        .min(65535) as u16;
    let candidate = if ticks >= target {
        INITIAL_DIVIDER.saturating_sub(correction).max(1)
    } else {
        INITIAL_DIVIDER.saturating_add(correction)
    };
    if candidate != INITIAL_DIVIDER {
        state.divider = candidate;
        let adjusted = measure_pair(state)?;
        if adjusted.abs_diff(target) < difference {
            ticks = adjusted;
        } else {
            state.divider = INITIAL_DIVIDER;
        }
    }
    if !state.clock.calibrate(ticks) {
        return Err(failed(state, Fault::Calibration));
    }
    saved.ticks = ticks;
    saved.divider = state.divider;
    saved.pal = pal;
    saved.valid = true;
    Ok(())
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn load_codebook(state: &mut State) {
    let index = state.segment as u32;
    let source = state.asset_offset + asset::HEADER_SIZE as u32 + (index << 6) + (index << 5);
    super::dma(source, BOOK_STAGING, 96);
    let mut destination = 0usize;
    let mut address = BOOK_STAGING;
    while destination < 32 {
        state.codebook[destination] =
            asset::decode_delta_entry([peek(address), peek(address + 1), peek(address + 2)]);
        destination += 1;
        address += 3;
    }
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn reset_decoder(state: &mut State) {
    state.decoded = 0;
    state.stream_pos = 0;
    state.cache_pos = 0;
    state.cache_len = 0;
    state.crc = 0;
    state.left = 0;
    state.right = 0;
    state.segment = 0;
    state.segment_left = state.metadata.samples_per_segment;
    load_codebook(state);
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn stream_byte(state: &mut State) -> Result<u8, Fault> {
    if state.cache_pos == state.cache_len {
        if state.stream_pos >= state.metadata.stream_bytes {
            return Err(failed(state, Fault::Metadata));
        }
        state.cache_len = (state.metadata.stream_bytes - state.stream_pos).min(128) as u16;
        let source = state.asset_offset + state.metadata.stream_offset + state.stream_pos;
        #[cfg(not(test))]
        super::dma(source, state.cache.as_mut_ptr() as u16, state.cache_len);
        #[cfg(test)]
        super::digital_test_fetch(source, &mut state.cache[..state.cache_len as usize]);
        state.stream_pos += state.cache_len as u32;
        state.cache_pos = 0;
        let mut i = 0usize;
        while i < state.cache_len as usize {
            state.crc = asset::crc_byte(state.crc, state.cache[i]);
            i += 1;
        }
        if state.stream_pos & 1023 == 0 || state.stream_pos == state.metadata.stream_bytes {
            let chunk = (state.stream_pos - 1) >> 10;
            super::dma(
                state.asset_offset + state.metadata.crc_offset + chunk,
                CHECK_STAGING,
                1,
            );
            if peek(CHECK_STAGING) != state.crc {
                return Err(failed(state, Fault::StreamCrc));
            }
        }
    }
    let value = state.cache[state.cache_pos as usize];
    state.cache_pos += 1;
    Ok(value)
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn decode_frame(state: &mut State) -> Result<(), Fault> {
    if state.decoded == state.metadata.total_samples {
        // Both predictors restart at the exact stereo frame count.
        // Ring counters, fractional clock phase and sampler gates continue.
        reset_decoder(state);
        state.loops = state.loops.wrapping_add(1);
    }
    if state.segment_left == 0 && state.segment + 1 < state.metadata.codebook_count {
        state.segment += 1;
        state.segment_left = state.metadata.samples_per_segment;
        load_codebook(state);
    }
    let codes = stream_byte(state)?;
    state.left = state
        .left
        .saturating_add(state.codebook[(codes & 15) as usize]);
    state.right = state
        .right
        .saturating_add(state.codebook[16 + (codes >> 4) as usize]);
    state.segment_left = state.segment_left.saturating_sub(1);
    state.decoded += 1;
    Ok(())
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn advance(state: &mut State) -> Result<u32, Fault> {
    let now = timer_read();
    let ticks = state.last_tick.wrapping_sub(now);
    state.last_tick = now;
    state.clock.advance(ticks);
    if timing::underrun(state.produced, state.clock.samples) {
        return Err(failed(state, Fault::Underrun));
    }
    state.clock.rebase(&mut state.produced);
    let lead = state.produced - state.clock.samples;
    state.minimum_lead = state.minimum_lead.min(lead as u16);
    Ok(state.clock.samples)
}

#[inline(never)]
#[link_section = ".phase.audio"]
unsafe fn produce(state: &mut State, samples: u16) -> Result<(), Fault> {
    let mut destination = STAGING;
    let mut remaining = samples;
    while remaining != 0 {
        decode_frame(state)?;
        poke(destination, state.left as u8);
        poke(destination + 1, (state.left >> 8) as u8);
        poke(destination + 2, state.right as u8);
        poke(destination + 3, (state.right >> 8) as u8);
        destination += 4;
        remaining -= 1;
    }
    if state.running {
        advance(state)?;
    }
    stash((state.produced & (RING_SAMPLES - 1)) << 2, samples << 2);
    state.produced += samples as u32;
    Ok(())
}

/// Initial invocation is made with CPU IRQs masked before simulation starts.
/// The caller retains Calibration separately and uses a fresh State on every
/// later SID -> DAC transition. I/O must be visible throughout every call.
#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn init(
    state: &mut State,
    saved: &mut Calibration,
    pal: bool,
    track: Track,
) -> Result<(), Fault> {
    stop(state);
    let descriptor = track.descriptor();
    state.track = track;
    state.asset_offset = descriptor.asset_offset;
    state.fault = 0;
    state.ready = false;
    state.loops = 0;
    state.underruns = 0;
    state.minimum_lead = RING_SAMPLES as u16;
    if peek(SAMPLER + 1) != 0x10 {
        return Err(failed(state, Fault::Mapping));
    }
    super::dma(state.asset_offset, STAGING, asset::HEADER_SIZE as u16);
    #[cfg(not(test))]
    let header = core::slice::from_raw_parts(STAGING as *const u8, asset::HEADER_SIZE);
    #[cfg(test)]
    let header_copy = {
        let mut bytes = [0u8; asset::HEADER_SIZE];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = peek(STAGING + i as u16);
        }
        bytes
    };
    #[cfg(test)]
    let header = &header_copy;
    state.metadata = match asset::parse_header(header, track) {
        Ok(metadata) => metadata,
        Err(_) => return Err(failed(state, Fault::Metadata)),
    };
    if !checksum_region(
        state.asset_offset + asset::HEADER_SIZE as u32,
        state.metadata.stream_offset - asset::HEADER_SIZE as u32,
        descriptor.codebook_checksum,
    ) {
        return Err(failed(state, Fault::CodebookChecksum));
    }
    if !checksum_region(
        state.asset_offset + state.metadata.crc_offset,
        (state.metadata.stream_bytes + 1023) >> 10,
        descriptor.crc_table_checksum,
    ) {
        return Err(failed(state, Fault::CrcTableChecksum));
    }
    timer_init();
    // Calibration reads known silence instead of arbitrary preloaded RAM.
    for i in 0..1024u16 {
        poke(STAGING + i, 0);
    }
    let mut offset = 0u32;
    while offset < asset::RING_BYTES {
        stash(offset, 1024);
        offset += 1024;
    }
    calibrate(state, saved, pal)?;
    state.produced = 0;
    reset_decoder(state);
    while state.produced < RING_SAMPLES {
        produce(state, MAX_BATCH)?;
    }
    state.ready = true;
    clear_sampler_irq();
    Ok(())
}

#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn start(state: &mut State) {
    if !state.ready || state.running || state.fault != 0 {
        return;
    }
    configure(state.divider, asset::RING_BYTES + 4);
    be24(SAMPLER + 21, asset::RING_BYTES);
    be24(RIGHT + 21, asset::RING_BYTES);
    poke(SAMPLER + 1, 32); // native unity gain
    poke(RIGHT + 1, 32);
    state.last_tick = timer_read();
    poke(SAMPLER, 0x53); // interleaved signed16, repeat; no CPU interrupt
    poke(RIGHT, 0x53);
    state.running = true;
}

#[inline(never)]
#[link_section = ".phase.audio"]
pub unsafe fn service(state: &mut State) -> Result<(), Fault> {
    if !state.running {
        return Ok(());
    }
    // AUDIO is loaded once per rendered game frame, not at 50+ Hz. Catch up
    // the entire missing lead in short batches; the old 512-sample budget
    // would supply only 21 ms of audio per game frame and inevitably underrun.
    // One ring is also a hard work bound. If a long frame needs more catch-up,
    // preserve the remaining safe lead and resume next frame; only the actual
    // reader guard is an underrun. This cannot monopolize the foreground.
    let mut budget = RING_SAMPLES as u16;
    loop {
        let consumed = advance(state)?;
        let count = timing::refill_samples(state.produced, consumed, MAX_BATCH);
        if count == 0 {
            return Ok(());
        }
        if budget == 0 {
            return Ok(());
        }
        let count = count.min(budget);
        produce(state, count)?;
        budget -= count;
    }
}
