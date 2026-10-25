//! Production DLT5 decoder against independent REU/sampler models.
//! These are transport/PCM checks, not physical timing measurements.
#![allow(static_mut_refs)]
use havoc::digital as asset;
use std::sync::Mutex;
#[path = "../target-c64/src/digital.rs"]
mod backend;
static SERIAL: Mutex<()> = Mutex::new(());
static mut HARDWARE: Option<Hardware> = None;
/// Production leaves this optional stream integrity hook disabled.
static mut DIGITAL_STREAM_CRC: u8 = 0;
unsafe fn rd(p: *const u8) -> u8 {
    core::ptr::read_volatile(p)
}
const DELAY: usize = 160;

fn read_track(track: asset::Track) -> Vec<u8> {
    asset::TRACK_ASSETS
        .iter()
        .find(|(id, _)| *id == track)
        .unwrap()
        .1
        .to_vec()
}

#[derive(Clone, Copy, Debug)]
struct Playback {
    emitted_at: usize,
    start: u32,
    repeat_a: u32,
    repeat_b: u32,
    length: u32,
    control: u8,
    divider: u64,
}
#[derive(Clone, Copy, Debug)]
struct PcmWrite {
    slot: usize,
    channels: usize,
    frames: usize,
}
struct Voice {
    completion: u64,
    one_shot: bool,
    playing: bool,
    playback_start: u64,
    played: u64,
    position: u32,
    sample_low: u8,
    emitted: Vec<i16>,
    starts: Vec<Playback>,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            completion: 0,
            one_shot: false,
            playing: false,
            playback_start: 0,
            played: 0,
            position: 0,
            sample_low: 0xa5,
            emitted: Vec::new(),
            starts: Vec::new(),
        }
    }
}
struct Hardware {
    ram: Vec<u8>,
    reu: Vec<u8>,
    ticks: u64,
    bus_hz: u64,
    version: u8,
    voices: [Voice; 2],
    calibration_starts: usize,
    maximum_dma: usize,
    staging_write_ticks: u64,
    timer_read_ticks: u64,
    slot_channels: [usize; 2],
    pcm_writes: Vec<PcmWrite>,
    ring_banks_written: [bool; 2],
    final_reu_byte_read: bool,
    sampler_writes: Vec<(u64, u16, u8)>,
    decoder_fetches: Vec<(u32, usize)>,
}
impl Hardware {
    fn new() -> Self {
        let mut reu = vec![0; asset::REU_SIZE as usize];
        reu[asset::MUSIC_OFFSET as usize..asset::MUSIC_OFFSET as usize + asset::DIRECTORY_BYTES]
            .copy_from_slice(asset::catalog_directory());
        for track in asset::Track::ALL {
            let bytes = read_track(track);
            let at = track.descriptor().asset_offset as usize;
            reu[at..at + bytes.len()].copy_from_slice(&bytes);
        }
        Self {
            ram: vec![0; 65536],
            reu,
            ticks: 0,
            bus_hz: 985_248,
            version: 0x10,
            voices: [Voice::default(), Voice::default()],
            calibration_starts: 0,
            maximum_dma: 0,
            staging_write_ticks: 1,
            timer_read_ticks: 1,
            slot_channels: [0; 2],
            pcm_writes: Vec::new(),
            ring_banks_written: [false; 2],
            final_reu_byte_read: false,
            sampler_writes: Vec::new(),
            decoder_fetches: Vec::new(),
        }
    }
    fn be24(&self, a: usize) -> u32 {
        ((self.ram[a] as u32) << 16) | ((self.ram[a + 1] as u32) << 8) | self.ram[a + 2] as u32
    }
    fn divider(&self, c: usize) -> u64 {
        let a = 0xdf2e + c * 32;
        ((self.ram[a] as u64) << 8) | self.ram[a + 1] as u64
    }
    fn width(control: u8) -> u32 {
        if control & 0x30 != 0 {
            2
        } else {
            1
        }
    }
    fn stride(control: u8) -> u32 {
        Self::width(control) * if control & 0x40 != 0 { 2 } else { 1 }
    }
    fn flush(&mut self) {
        for channel in 0..2 {
            let base = 0xdf20 + channel * 32;
            let control = self.ram[base];
            let start = self.be24(base + 5);
            let length = self.be24(base + 9);
            let repeat_a = self.be24(base + 17);
            let repeat_b = self.be24(base + 21);
            let divider = self.divider(channel);
            let width = Self::width(control);
            let stride = Self::stride(control);
            let v = &mut self.voices[channel];
            if !v.playing && !v.one_shot {
                continue;
            }
            // Synthetic backend delay intentionally exercises the correction.
            let frames =
                (self.ticks - v.playback_start) * 6_250_000 / ((divider + 2) * self.bus_hz);
            while v.played < frames {
                if v.position == repeat_b && control & 2 != 0 {
                    v.position = repeat_a;
                }
                if v.position == length && v.one_shot {
                    break;
                }
                assert!(v.position < length, "sampler reached terminal length");
                let at = (start + v.position) as usize;
                assert!(
                    at >= asset::RING_OFFSET as usize && at + width as usize <= self.reu.len(),
                    "sampler address {at:x}"
                );
                if at + width as usize == self.reu.len() {
                    self.final_reu_byte_read = true;
                }
                let high = if width == 2 {
                    v.sample_low = self.reu[at];
                    self.reu[at + 1]
                } else {
                    self.reu[at]
                };
                let sample = i16::from_le_bytes([v.sample_low, high]);
                if v.playing && self.ram[base + 1] != 0 {
                    v.emitted.push(sample);
                }
                v.position += stride;
                v.played += 1;
            }
        }
    }
    fn wait(&mut self, ticks: u64) {
        self.ticks += ticks;
        self.flush();
    }
    fn poke(&mut self, a: u16, value: u8) {
        self.ticks += if (0x8e00..0x9000).contains(&a) {
            self.staging_write_ticks
        } else {
            1
        };
        // Muting/configuration changes also occur between two sampler outputs.
        if (0xdf20..0xdf60).contains(&a) {
            self.flush();
            self.sampler_writes.push((self.ticks, a, value));
        }
        self.ram[a as usize] = value;
        if a == 0xdf20 || a == 0xdf40 {
            let channel = ((a - 0xdf20) / 32) as usize;
            let base = a as usize;
            let start = self.be24(base + 5);
            let length = self.be24(base + 9);
            let repeat_a = self.be24(base + 17);
            let repeat_b = self.be24(base + 21);
            let divider = self.divider(channel);
            let stride = Self::stride(value);
            let frames = length as u64 / stride as u64;
            let v = &mut self.voices[channel];
            v.one_shot = value & 3 == 1;
            v.playing = value & 3 == 3;
            if value & 1 != 0 {
                v.playback_start = self.ticks;
                v.played = 0;
                v.position = 0;
            }
            if v.one_shot {
                assert_eq!(self.ram[base + 1], 0, "calibration must be muted");
                if channel == 0 {
                    self.calibration_starts += 1;
                }
                v.completion =
                    self.ticks + (frames * (divider + 2) * self.bus_hz).div_ceil(6_250_000);
            }
            if v.playing {
                assert!(value == 0x43 || value == 0x03, "8-bit playback mode");
                assert_eq!(self.ram[base + 1], 32);
                assert_eq!(self.ram[base + 2], if channel == 0 { 0 } else { 15 });
                assert_eq!(self.ram[base + 4], 1);
                assert_eq!(
                    v.sample_low, 0,
                    "signed16 silence must clear the stale low byte"
                );
                v.starts.push(Playback {
                    emitted_at: v.emitted.len(),
                    start,
                    repeat_a,
                    repeat_b,
                    length,
                    control: value,
                    divider,
                });
                if channel == 1 {
                    assert_eq!(self.divider(0), divider);
                    assert!(self.voices[1].playback_start - self.voices[0].playback_start <= 1);
                }
            }
        } else if a == 0xdf01 && value == 0x90 {
            self.flush();
            let source = u16::from_le_bytes([self.ram[0xdf02], self.ram[0xdf03]]) as usize;
            let dest = self.ram[0xdf04] as usize
                | ((self.ram[0xdf05] as usize) << 8)
                | ((self.ram[0xdf06] as usize) << 16);
            let count = u16::from_le_bytes([self.ram[0xdf07], self.ram[0xdf08]]) as usize;
            assert!((1..=1024).contains(&count));
            assert!(
                dest >= asset::RING_OFFSET as usize && dest + count <= asset::REU_SIZE as usize
            );
            self.maximum_dma = self.maximum_dma.max(count);
            let slot = (dest - asset::RING_OFFSET as usize) >> 16;
            let offset = (dest - asset::RING_OFFSET as usize) & 65535;
            self.ring_banks_written[slot] = true;
            if count == 1024 {
                assert!(
                    !self.voices[0].playing,
                    "large writes belong to boot clearing"
                );
                assert!(self.ram[source..source + count].iter().all(|&b| b == 0));
            } else if self.slot_channels[slot] == 1 && offset == 32768 - DELAY && count == DELAY {
                assert!(self.ram[source..source + count].iter().all(|&b| b == 0));
            } else {
                // The test caller assigns each slot's requested profile. Do not
                // infer pending writes from the active sampler's channel width.
                let channels = self.slot_channels[slot];
                assert!((1..=2).contains(&channels), "PCM slot has no owner");
                assert_eq!(count % channels, 0);
                assert!(count <= 128 * channels, "PCM batch exceeds 128 frames");
                let base = if channels == 1 { 32768 } else { 0 };
                assert!(offset >= base && offset + count <= base + 32768 * channels);
                self.pcm_writes.push(PcmWrite {
                    slot,
                    channels,
                    frames: count / channels,
                });
            }
            self.reu[dest..dest + count].copy_from_slice(&self.ram[source..source + count]);
            self.ticks += count as u64;
        }
    }
    fn peek(&mut self, a: u16) -> u8 {
        self.ticks += if (0xdd04..=0xdd07).contains(&a) {
            self.timer_read_ticks
        } else {
            1
        };
        match a {
            0xdf20 => {
                self.flush();
                (self.voices[0].one_shot && self.ticks >= self.voices[0].completion) as u8
            }
            0xdf21 => self.version,
            0xdd04..=0xdd07 => {
                let counter = u32::MAX.wrapping_sub(self.ticks as u32);
                (counter >> ((a - 0xdd04) * 8)) as u8
            }
            _ => self.ram[a as usize],
        }
    }
    fn assert_stopped(&self) {
        for b in [0xdf20, 0xdf40] {
            assert_eq!(self.ram[b], 0);
            assert_eq!(self.ram[b + 1], 0);
        }
    }
    fn assert_profile(&self, track: asset::Track, slot: u32) {
        let stereo = track.descriptor().metadata.channels == 2;
        let l = self.voices[0].starts.last().unwrap();
        let r = self.voices[1].starts.last().unwrap();
        let ring = asset::RING_OFFSET + (slot << 16) + if stereo { 0 } else { 0x8000 };
        assert_eq!(
            (l.start, r.start),
            (
                ring,
                if stereo {
                    ring + 1
                } else {
                    ring - DELAY as u32
                }
            )
        );
        assert_eq!(
            (l.repeat_a, r.repeat_a),
            (0, if stereo { 0 } else { DELAY as u32 })
        );
        let length = if stereo { 65536 } else { 32768 };
        let delay = if stereo { 0 } else { DELAY as u32 };
        assert_eq!((l.repeat_b, r.repeat_b), (length, length + delay));
        assert_eq!(
            (l.length, r.length),
            (
                length + if stereo { 2 } else { 1 },
                length + delay + if stereo { 2 } else { 1 }
            )
        );
        assert_eq!(
            (l.control, r.control),
            if stereo { (0x43, 0x43) } else { (3, 3) }
        );
        assert_eq!(l.divider, r.divider);
    }
}
unsafe fn hardware() -> &'static mut Hardware {
    HARDWARE.as_mut().unwrap()
}
unsafe fn peek(a: u16) -> u8 {
    hardware().peek(a)
}
unsafe fn poke(a: u16, v: u8) {
    hardware().poke(a, v)
}
unsafe fn word(a: u16, v: u16) {
    poke(a, v as u8);
    poke(a + 1, (v >> 8) as u8)
}
unsafe fn dma(source: u32, destination: u16, count: u16) {
    let hw = hardware();
    assert!((1..=1024).contains(&count));
    hw.maximum_dma = hw.maximum_dma.max(count as usize);
    hw.ram[destination as usize..destination as usize + count as usize]
        .copy_from_slice(&hw.reu[source as usize..source as usize + count as usize]);
    hw.ticks += count as u64;
}
unsafe fn digital_test_fetch(source: u32, destination: &mut [u8]) {
    let hw = hardware();
    assert!(destination.len() <= 32);
    hw.decoder_fetches.push((source, destination.len()));
    destination.copy_from_slice(&hw.reu[source as usize..source as usize + destination.len()]);
    hw.ticks += destination.len() as u64;
}

unsafe fn init(
    state: &mut backend::State,
    calibration: &mut backend::Calibration,
    pal: bool,
    track: asset::Track,
) -> Result<(), backend::Fault> {
    hardware().slot_channels = [track.descriptor().metadata.channels as usize, 0];
    let before = hardware().pcm_writes.len();
    let result = backend::init(state, calibration, pal, track);
    assert_eq!(
        hardware().pcm_writes.len(),
        before,
        "init must not decode PCM"
    );
    result
}

unsafe fn request(state: &mut backend::State, track: asset::Track) {
    if track != state.track {
        let slot = if state.is_running() {
            let active = ((hardware().voices[0].starts.last().unwrap().start - asset::RING_OFFSET)
                >> 16) as usize;
            active ^ 1
        } else {
            0
        };
        hardware().slot_channels[slot] = track.descriptor().metadata.channels as usize;
    }
    let before = hardware().pcm_writes.len();
    backend::request(state, track);
    assert_eq!(
        hardware().pcm_writes.len(),
        before,
        "request must not decode PCM"
    );
}

unsafe fn service(state: &mut backend::State) -> Result<(), backend::Fault> {
    let before = hardware().pcm_writes.len();
    let active_slot = state.is_running().then(|| {
        ((hardware().voices[0].starts.last().unwrap().start - asset::RING_OFFSET) >> 16) as usize
    });
    let result = backend::service(state);
    let writes = &hardware().pcm_writes[before..];
    // Independent exact units of 1/80,000 second, including startup and both
    // slots. This also checks partially completed work on a faulting visit.
    let duration: usize = writes
        .iter()
        .map(|write| write.frames * if write.channels == 2 { 4 } else { 5 })
        .sum();
    assert!(
        duration <= 20_000,
        "service published {duration}/80000 seconds: {writes:?}"
    );
    if let Some(active) = active_slot {
        if let Some(pending_at) = writes.iter().position(|write| write.slot != active) {
            assert!(
                writes[pending_at..]
                    .iter()
                    .all(|write| write.slot != active),
                "pending preparation must follow active servicing"
            );
        }
    }
    result
}

unsafe fn prime(state: &mut backend::State) -> usize {
    for visit in 1..=9 {
        service(state).unwrap();
        if state.is_running() {
            return visit;
        }
        hardware().assert_stopped();
    }
    panic!("initial ring did not start within nine bounded visits");
}

/// Intentionally does not call production header, delta, bit or rounding helpers.
fn reference(track: asset::Track) -> Vec<[i16; 2]> {
    let bytes = read_track(track);
    assert_eq!(&bytes[..4], b"DLT5");
    let samples = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let books = u16::from_le_bytes(bytes[18..20].try_into().unwrap()) as usize;
    let channels = bytes[21] as usize;
    let group = if channels == 1 { 8 } else { 4 };
    let groups = samples.div_ceil(group);
    let short = groups / books * group;
    let long = groups % books;
    let long_end = long * (short + group);
    let stream = 32 + books * 8 * 2 * channels;
    let mut out = Vec::with_capacity(samples);
    let mut previous = [0i32; 2];
    for frame in 0..samples {
        let book = if frame < long_end {
            frame / (short + group)
        } else {
            long + (frame - long_end) / short
        };
        let mut emitted = [0; 2];
        for channel in 0..channels {
            let bit = (frame * channels + channel) * 3;
            let code = (0..3)
                .map(|b| (((bytes[stream + (bit + b) / 8] >> ((bit + b) % 8)) & 1) as usize) << b)
                .sum::<usize>();
            let at = 32 + ((book * channels + channel) * 8 + code) * 2;
            let delta = i16::from_le_bytes([bytes[at], bytes[at + 1]]) as i32;
            previous[channel] = (previous[channel] + delta).clamp(-32768, 32767);
            emitted[channel] =
                ((previous[channel] + 128).div_euclid(256).clamp(-128, 127) * 256) as i16;
        }
        if channels == 1 {
            emitted[1] = emitted[0];
        }
        out.push(emitted);
    }
    out
}
unsafe fn assert_playback(track: asset::Track, epoch: usize, expected: &[[i16; 2]]) {
    let mono = track.descriptor().metadata.channels == 1;
    for channel in 0..2 {
        let voice = &hardware().voices[channel];
        let start = voice.starts[epoch].emitted_at;
        let end = voice
            .starts
            .get(epoch + 1)
            .map_or(voice.emitted.len(), |p| p.emitted_at);
        for (index, &actual) in voice.emitted[start..end].iter().enumerate() {
            let wanted = if mono && channel == 1 && index < DELAY {
                0
            } else {
                let index = index - if mono && channel == 1 { DELAY } else { 0 };
                expected[index % expected.len()][channel]
            };
            assert_eq!(
                actual, wanted,
                "{track:?} epoch {epoch} channel {channel} frame {index}"
            );
        }
    }
}
unsafe fn visit(state: &mut backend::State) {
    hardware().wait(197_050);
    hardware().ram[0x8e00..0x9200].fill(0xa5);
    service(state).unwrap();
}

unsafe fn handoff(state: &mut backend::State, track: asset::Track) -> usize {
    let previous = state.track;
    let starts = hardware().voices[0].starts.len();
    for index in 1..=200 {
        request(state, track);
        visit(state);
        assert!(
            state.is_running(),
            "preparation must keep the active track running"
        );
        if state.track == track {
            assert_eq!(hardware().voices[0].starts.len(), starts + 1);
            return index;
        }
        assert_eq!(state.track, previous);
        assert_eq!(hardware().voices[0].starts.len(), starts);
    }
    panic!("pending track did not complete within 200 bounded visits");
}

#[test]
fn stereo_pcm_at_the_mono_prefix_address_is_counted() {
    let mut hw = Hardware::new();
    hw.slot_channels[0] = 2;
    hw.ram[0x8e00..0x8e00 + DELAY].fill(0x5a);
    hw.ram[0xdf02..0xdf04].copy_from_slice(&0x8e00u16.to_le_bytes());
    let destination = asset::RING_OFFSET + (32768 - DELAY) as u32;
    hw.ram[0xdf04..0xdf07].copy_from_slice(&destination.to_le_bytes()[..3]);
    hw.ram[0xdf07..0xdf09].copy_from_slice(&(DELAY as u16).to_le_bytes());
    hw.poke(0xdf01, 0x90);
    assert_eq!(hw.pcm_writes.len(), 1);
    let write = hw.pcm_writes[0];
    assert_eq!((write.slot, write.channels, write.frames), (0, 2, 80));
}

#[test]
fn initial_fill_is_bounded_muted_and_starts_once_when_full() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut calibration = backend::Calibration::new();
        for (track, quota, visits, tail) in [
            (asset::Track::HammondRocker, 5000usize, 7, 8),
            (asset::Track::GalacticDash, 4000usize, 9, 32),
        ] {
            let mut state = backend::State::new();
            init(&mut state, &mut calibration, true, track).unwrap();
            assert!(!state.is_running());
            hardware().assert_stopped();
            let starts = hardware().voices[0].starts.len();
            let mut total = 0;
            for index in 1..=visits {
                request(&mut state, track); // Repeated selection must retain progress.
                hardware().wait(197_050);
                let before = hardware().pcm_writes.len();
                service(&mut state).unwrap();
                let writes = &hardware().pcm_writes[before..];
                let count: usize = writes.iter().map(|write| write.frames).sum();
                assert_eq!(count, quota.min(32768 - total));
                assert!(writes.iter().all(|write| write.slot == 0));
                if count == quota {
                    assert_eq!(writes.last().unwrap().frames, tail);
                }
                total += count;
                assert_eq!(state.is_running(), index == visits);
                assert_eq!(
                    hardware().voices[0].starts.len(),
                    starts + usize::from(index == visits)
                );
                if index != visits {
                    hardware().assert_stopped();
                }
            }
            assert_eq!(total, 32768);
            hardware().assert_profile(track, 0);
            for _ in 0..3 {
                service(&mut state).unwrap();
            }
            assert_eq!(hardware().voices[0].starts.len(), starts + 1);
            backend::stop(&mut state);
        }
        HARDWARE = None;
    }
}

#[test]
fn replacing_initial_track_restarts_priming_and_stopping_cancels_it() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut calibration = backend::Calibration::new();
        let mut state = backend::State::new();
        service(&mut state).unwrap();
        assert!(!state.is_running());
        assert!(hardware().pcm_writes.is_empty());
        hardware().assert_stopped();
        init(
            &mut state,
            &mut calibration,
            true,
            asset::Track::HammondRocker,
        )
        .unwrap();
        service(&mut state).unwrap();
        service(&mut state).unwrap();
        request(&mut state, asset::Track::GalacticDash);
        assert_eq!(state.track, asset::Track::GalacticDash);
        assert_eq!(
            prime(&mut state),
            9,
            "replacement needs a complete fresh ring"
        );
        assert_eq!(hardware().voices[0].starts.len(), 1);
        hardware().assert_profile(asset::Track::GalacticDash, 0);
        backend::stop(&mut state);
        let before = hardware().pcm_writes.len();
        request(&mut state, asset::Track::HammondRocker);
        for _ in 0..10 {
            service(&mut state).unwrap();
        }
        assert!(!state.is_running());
        assert_eq!(hardware().pcm_writes.len(), before);
        hardware().assert_stopped();

        init(
            &mut state,
            &mut calibration,
            true,
            asset::Track::HammondRocker,
        )
        .unwrap();
        service(&mut state).unwrap();
        backend::stop(&mut state);
        let before = hardware().pcm_writes.len();
        for _ in 0..10 {
            service(&mut state).unwrap();
        }
        assert!(!state.is_running());
        assert_eq!(hardware().pcm_writes.len(), before);
        assert_eq!(hardware().voices[0].starts.len(), 1);
        hardware().assert_stopped();
        HARDWARE = None;
    }
}

#[test]
fn mixed_rate_handoffs_share_the_quarter_second_budget() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut calibration = backend::Calibration::new();
        let mut state = backend::State::new();
        init(
            &mut state,
            &mut calibration,
            true,
            asset::Track::HammondRocker,
        )
        .unwrap();
        prime(&mut state);
        for track in [asset::Track::GalacticDash, asset::Track::HammondRocker] {
            let mut mixed_visits = 0;
            let mut full_visits = 0;
            let mut pending_frames = 0;
            for _ in 0..200 {
                request(&mut state, track);
                let before = hardware().pcm_writes.len();
                visit(&mut state);
                let writes = &hardware().pcm_writes[before..];
                let mono: usize = writes
                    .iter()
                    .filter(|w| w.channels == 1)
                    .map(|w| w.frames)
                    .sum();
                let stereo: usize = writes
                    .iter()
                    .filter(|w| w.channels == 2)
                    .map(|w| w.frames)
                    .sum();
                pending_frames += if track.descriptor().metadata.channels == 1 {
                    mono
                } else {
                    stereo
                };
                assert!(pending_frames <= 32768);
                if mono != 0 && stereo != 0 {
                    mixed_visits += 1;
                    // A pending frame must fit, but no whole pending frame of
                    // unused budget may remain unless its ring just completed.
                    let used = mono * 5 + stereo * 4;
                    // A complete pending ring now waits for the outgoing fade;
                    // track identity changes only after that fade reaches zero.
                    if pending_frames < 32768 {
                        let next_cost = if track.descriptor().metadata.channels == 1 {
                            5
                        } else {
                            4
                        };
                        assert!(20_000 - used < next_cost);
                        full_visits += 1;
                    }
                }
                if state.track == track {
                    break;
                }
            }
            assert_eq!(state.track, track, "mixed-rate handoff must complete");
            assert!(mixed_visits > 0 && full_visits > 0);
        }
        backend::stop(&mut state);
        HARDWARE = None;
    }
}

#[test]
fn actual_backend_loops_all_profiles_through_shared_scratch_reuse() {
    let _serial = SERIAL.lock().unwrap();
    assert!(core::mem::size_of::<backend::State>() <= 392);
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut calibration = backend::Calibration::new();
        let mut count = 0;
        for track in asset::Track::ALL {
            let expected = reference(track);
            hardware().decoder_fetches.clear();
            let mut state = backend::State::new();
            init(&mut state, &mut calibration, true, track).unwrap();
            if count == 0 {
                count = hardware().calibration_starts;
                assert!(count >= 5);
            }
            assert_eq!(
                hardware().calibration_starts,
                count,
                "both profile calibrations are reused"
            );
            let epoch = hardware().voices[0].starts.len();
            prime(&mut state);
            hardware().assert_profile(track, 0);
            let start = hardware().voices[1].starts[epoch].emitted_at;
            while hardware().voices[1].emitted.len() - start < expected.len() + 4096 {
                visit(&mut state);
            }
            backend::stop(&mut state);
            assert!(state.loops >= 1);
            assert_eq!(state.underruns, 0);
            assert!(state.minimum_lead > 2048);
            hardware().assert_stopped();
            assert!(hardware().maximum_dma <= 1024);
            if track.descriptor().metadata.channels == 2 {
                assert!(expected.iter().any(|p| p[0] != p[1]));
            }
            assert_playback(track, epoch, &expected);
            let descriptor = track.descriptor();
            let start = descriptor.asset_offset + 32;
            let stream = descriptor.asset_offset + descriptor.metadata.stream_offset;
            let per = 16 * descriptor.metadata.channels as usize;
            let book_fetches: Vec<_> = hardware()
                .decoder_fetches
                .iter()
                .copied()
                .filter(|(address, _)| start <= *address && *address < stream)
                .collect();
            let books = descriptor.metadata.codebook_count as usize;
            assert!(
                book_fetches.len() > books,
                "loop must reload the first book"
            );
            for (book, &(address, count)) in book_fetches.iter().enumerate() {
                assert_eq!(
                    (address, count),
                    (start + ((book % books) * per) as u32, per)
                );
            }
            // The expanded allocation puts every mono and stereo table beyond
            // 64 KiB. Check the exact boundary fetch in addition to the full
            // sequence above, including the final book and loop reset.
            assert!(books * per > 65536, "{track:?}: exercise wide book offsets");
            assert_eq!(
                book_fetches[65536 / per],
                (start + 65536, per),
                "{track:?}: codebook address must not wrap at 64 KiB"
            );
        }
        HARDWARE = None;
    }
}

#[test]
fn pending_requests_cancel_replace_and_handoff_without_recalibration() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut calibration = backend::Calibration::new();
        let mut state = backend::State::new();
        let stereo = asset::Track::HammondRocker;
        let alternate = asset::Track::LowlandApproach;
        let mono = asset::Track::GalacticDash;
        init(&mut state, &mut calibration, true, stereo).unwrap();
        prime(&mut state);
        let calibrations = hardware().calibration_starts;
        request(&mut state, mono);
        for _ in 0..3 {
            visit(&mut state);
            assert_eq!(state.track, stereo);
        }
        request(&mut state, stereo);
        for _ in 0..9 {
            visit(&mut state);
        }
        assert_eq!(
            hardware().voices[0].starts.len(),
            1,
            "cancel must retain active position"
        );
        request(&mut state, mono);
        visit(&mut state);
        request(&mut state, alternate);
        handoff(&mut state, alternate);
        hardware().assert_profile(alternate, 1);
        // Put stereo in slot A again, then poison slot B's delay prefix to
        // verify that preparing another mono track clears the reused slot.
        request(&mut state, stereo);
        handoff(&mut state, stereo);
        hardware().assert_profile(stereo, 0);
        let prefix = asset::RING_OFFSET as usize + 0x18000 - DELAY;
        hardware().reu[prefix..prefix + DELAY].fill(0x6b);
        request(&mut state, mono);
        assert!(hardware().reu[prefix..prefix + DELAY]
            .iter()
            .all(|&b| b == 0));
        handoff(&mut state, mono);
        hardware().assert_profile(mono, 1);
        for _ in 0..12 {
            visit(&mut state);
        }
        assert!(
            hardware().final_reu_byte_read,
            "slot B reaches the final REU byte"
        );
        request(&mut state, alternate);
        handoff(&mut state, alternate);
        hardware().assert_profile(alternate, 0);
        for _ in 0..4 {
            visit(&mut state);
        }
        backend::stop(&mut state);
        assert_eq!(hardware().calibration_starts, calibrations);
        assert_eq!(state.underruns, 0);
        for (epoch, track) in [stereo, alternate, stereo, mono, alternate]
            .into_iter()
            .enumerate()
        {
            assert_playback(track, epoch, &reference(track));
        }
        HARDWARE = None;
    }
}

unsafe fn prepare_pending(state: &mut backend::State, track: asset::Track) -> u64 {
    let pending_slot = (((hardware().voices[0].starts.last().unwrap().start - asset::RING_OFFSET)
        >> 16) as usize)
        ^ 1;
    let mut frames = 0;
    for _ in 0..100 {
        request(state, track);
        let before = hardware().pcm_writes.len();
        service(state).unwrap();
        frames += hardware().pcm_writes[before..]
            .iter()
            .filter(|write| write.slot == pending_slot)
            .map(|write| write.frames)
            .sum::<usize>();
        if frames == 32768 {
            return hardware().ticks;
        }
    }
    panic!("pending ring did not become ready");
}

#[test]
fn ready_track_waits_for_a_one_second_paired_fade_on_both_clocks_and_timer_wrap() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        for pal in [true, false] {
            HARDWARE = Some(Hardware::new());
            let hz = if pal { 985_248 } else { 1_022_727 };
            hardware().bus_hz = hz;
            let mut state = backend::State::new();
            let active = asset::Track::HammondRocker;
            let next = asset::Track::GalacticDash;
            init(&mut state, &mut backend::Calibration::new(), pal, active).unwrap();
            // The one-second fade straddles the hardware counter's wrap.
            hardware().ticks = (1u64 << 32) - hz / 2;
            prime(&mut state);
            hardware().sampler_writes.clear();
            request(&mut state, next);
            let prepared = prepare_pending(&mut state, next);
            assert_eq!(state.track, active, "priming cannot bypass the fade");
            assert!(hardware().ram[0xdf21] > 0);
            assert_eq!(hardware().voices[0].starts.len(), 1);
            for _ in 0..220 {
                request(&mut state, next);
                hardware().wait(hz / 200);
                service(&mut state).unwrap();
                if state.track == next {
                    break;
                }
            }
            assert_eq!(state.track, next);
            assert_eq!(hardware().voices[0].starts.len(), 2);
            let history = &hardware().sampler_writes;
            let left: Vec<_> = history
                .iter()
                .filter(|&&(_, a, _)| a == 0xdf21)
                .map(|&(tick, _, value)| (tick, value))
                .collect();
            let right: Vec<_> = history
                .iter()
                .filter(|&&(_, a, _)| a == 0xdf41)
                .map(|&(tick, _, value)| (tick, value))
                .collect();
            assert_eq!(left.len(), right.len());
            assert!(
                left.len() <= 35,
                "only changed gain steps and handoff writes are needed"
            );
            for (index, (&(lt, lv), &(rt, rv))) in left.iter().zip(&right).enumerate() {
                assert_eq!(lv, rv);
                // Configuration also clears already-muted channels separately.
                if lv != 0 || index == 0 || left[index - 1].1 != 0 {
                    assert_eq!(rt, lt + 1, "gain changes must remain paired");
                }
            }
            assert_eq!(
                left.last().unwrap().1,
                32,
                "new track starts at full volume"
            );
            let fade = &left[..left.len() - 1];
            assert!(fade.windows(2).all(|p| p[1].1 <= p[0].1));
            let muted = fade.iter().find(|&&(_, value)| value == 0).unwrap().0;
            assert!(muted - prepared >= hz * 99 / 100, "fade ended early");
            assert!(muted - prepared <= hz * 102 / 100, "fade took too long");
            assert!(
                hardware().voices[0].playback_start > muted,
                "new music must wait until old music is silent"
            );
            for _ in 0..3 {
                hardware().wait(hz / 5);
                service(&mut state).unwrap();
            }
            assert_playback(next, 1, &reference(next));
            assert_eq!(state.underruns, 0);
            backend::stop(&mut state);
            HARDWARE = None;
        }
    }
}

#[test]
fn pending_replacement_preserves_fade_and_active_request_cancels_even_after_silence() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut state = backend::State::new();
        let active = asset::Track::HammondRocker;
        let first = asset::Track::GalacticDash;
        let second = asset::Track::LowlandApproach;
        init(&mut state, &mut backend::Calibration::new(), true, active).unwrap();
        prime(&mut state);
        request(&mut state, first);
        prepare_pending(&mut state, first);
        hardware().wait(350_000);
        service(&mut state).unwrap();
        let partial = hardware().ram[0xdf21];
        assert!(partial > 0 && partial < 32);
        request(&mut state, first);
        assert_eq!(
            hardware().ram[0xdf21],
            partial,
            "repeated request reset the fade"
        );
        request(&mut state, second);
        assert_eq!(
            hardware().ram[0xdf21],
            partial,
            "replacement reset the fade"
        );
        hardware().wait(700_000);
        service(&mut state).unwrap();
        assert_eq!(state.track, active, "replacement is not primed yet");
        assert_eq!(
            hardware().ram[0xdf21],
            0,
            "replacement delayed an existing fade"
        );
        assert_eq!(hardware().ram[0xdf41], 0);
        assert_eq!(hardware().voices[0].starts.len(), 1);
        request(&mut state, active);
        assert_eq!(hardware().ram[0xdf21], 32);
        assert_eq!(hardware().ram[0xdf41], 32);
        for _ in 0..10 {
            request(&mut state, active);
            visit(&mut state);
            assert_eq!(hardware().ram[0xdf21], 32);
        }
        assert_eq!(
            hardware().voices[0].starts.len(),
            1,
            "cancellation restarted playback"
        );
        assert_eq!(state.track, active);
        // A fresh request keeps normal gain until its replacement is primed.
        request(&mut state, first);
        for _ in 0..3 {
            visit(&mut state);
            assert_eq!(hardware().ram[0xdf21], 32);
            assert_eq!(state.track, active);
        }
        handoff(&mut state, first);
        assert_eq!(state.track, first);
        assert_eq!(hardware().ram[0xdf21], 32);
        assert_eq!(hardware().voices[0].starts.len(), 2);
        assert_eq!(state.underruns, 0);
        backend::stop(&mut state);
        HARDWARE = None;
    }
}
#[test]
fn mapping_crc_and_deadline_faults_mute_and_stop_the_sampler() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let track = asset::Track::HammondRocker;
        let descriptor = track.descriptor();
        let stream = (descriptor.asset_offset + descriptor.metadata.stream_offset) as usize;
        let mut state = backend::State::new();
        hardware().version = 0xff;
        assert_eq!(
            init(&mut state, &mut backend::Calibration::new(), true, track),
            Err(backend::Fault::Mapping)
        );
        hardware().assert_stopped();
        hardware().version = 0x10;
        hardware().reu[asset::MUSIC_OFFSET as usize] ^= 1;
        assert_eq!(
            init(&mut state, &mut backend::Calibration::new(), true, track),
            Err(backend::Fault::Metadata)
        );
        hardware().assert_stopped();
        hardware().reu[asset::MUSIC_OFFSET as usize] ^= 1;
        // Production skips the stream checksum; the debug hook retains fault coverage.
        hardware().reu[stream] ^= 1;
        init(&mut state, &mut backend::Calibration::new(), true, track).unwrap();
        assert_eq!(service(&mut state), Ok(()));
        backend::stop(&mut state);
        hardware().reu[stream] ^= 1;
        DIGITAL_STREAM_CRC = 1;
        for offset in [0, 31, 32, 127, 128, 1023, 1024] {
            hardware().reu[stream + offset] ^= 1;
            init(&mut state, &mut backend::Calibration::new(), true, track).unwrap();
            assert_eq!(service(&mut state), Err(backend::Fault::StreamCrc));
            hardware().assert_stopped();
            let before = hardware().pcm_writes.len();
            request(&mut state, asset::Track::GalacticDash);
            for _ in 0..10 {
                let _ = service(&mut state);
            }
            assert!(!state.is_running());
            assert_eq!(
                hardware().pcm_writes.len(),
                before,
                "faulted playback cannot resume"
            );
            hardware().assert_stopped();
            hardware().reu[stream + offset] ^= 1;
        }
        DIGITAL_STREAM_CRC = 0;
        for (offset, fault) in [
            (21, backend::Fault::Metadata),
            (32 + 24, backend::Fault::CodebookChecksum),
            (
                descriptor.metadata.crc_offset as usize,
                backend::Fault::CrcTableChecksum,
            ),
        ] {
            let at = descriptor.asset_offset as usize + offset;
            hardware().reu[at] ^= 1;
            assert_eq!(
                init(&mut state, &mut backend::Calibration::new(), true, track),
                Err(fault)
            );
            hardware().assert_stopped();
            hardware().reu[at] ^= 1;
        }
        init(&mut state, &mut backend::Calibration::new(), true, track).unwrap();
        prime(&mut state);
        hardware().wait(2_000_000);
        assert_eq!(service(&mut state), Err(backend::Fault::Underrun));
        assert_eq!(state.fault, backend::Fault::Underrun as u8);
        assert_eq!(state.underruns, 1);
        hardware().assert_stopped();
        HARDWARE = None;
    }
}

#[test]
fn refill_finishes_when_clock_queries_also_advance_playback() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut state = backend::State::new();
        init(
            &mut state,
            &mut backend::Calibration::new(),
            true,
            asset::Track::HammondRocker,
        )
        .unwrap();
        prime(&mut state);
        hardware().timer_read_ticks = 16;
        for _ in 0..20 {
            hardware().wait(197_050);
            service(&mut state).unwrap();
        }
        assert_eq!(state.underruns, 0);
        assert!(state.minimum_lead > 2048);
        backend::stop(&mut state);
        HARDWARE = None;
    }
}

#[test]
fn refill_work_limit_returns_with_safe_lead_and_real_underrun_still_faults() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut state = backend::State::new();
        init(
            &mut state,
            &mut backend::Calibration::new(),
            true,
            asset::Track::HammondRocker,
        )
        .unwrap();
        prime(&mut state);
        hardware().wait(300_000);
        // Two PCM bytes now replace the old four-byte publication. Slow their
        // modeled stores enough that one bounded service cannot clear backlog.
        hardware().staging_write_ticks = 22;
        hardware().timer_read_ticks = 5;
        let before = hardware().pcm_writes.len();
        assert_eq!(service(&mut state), Ok(()));
        assert_eq!(
            hardware().pcm_writes[before..]
                .iter()
                .map(|w| w.frames)
                .sum::<usize>(),
            5000
        );
        assert_eq!(state.fault, 0);
        assert_eq!(state.underruns, 0);
        assert!(state.minimum_lead > 2048);
        assert_eq!(hardware().ram[0xdf20], 0x43);
        assert_eq!(hardware().ram[0xdf40], 0x43);
        hardware().wait(2_000_000);
        assert_eq!(service(&mut state), Err(backend::Fault::Underrun));
        hardware().assert_stopped();
        HARDWARE = None;
    }
}

#[test]
fn corrupted_pending_stream_stops_both_voices_before_handoff() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut state = backend::State::new();
        let active = asset::Track::HammondRocker;
        let pending = asset::Track::GalacticDash;
        init(&mut state, &mut backend::Calibration::new(), true, active).unwrap();
        prime(&mut state);
        let d = pending.descriptor();
        hardware().reu[(d.asset_offset + d.metadata.stream_offset + 1024) as usize] ^= 1;
        DIGITAL_STREAM_CRC = 1;
        request(&mut state, pending);
        // The corruption lies in the second 1024-byte CRC chunk. One mono
        // preparation visit decodes 1500 bytes; the next completes that chunk.
        assert_eq!(service(&mut state), Ok(()));
        assert_eq!(service(&mut state), Err(backend::Fault::StreamCrc));
        DIGITAL_STREAM_CRC = 0;
        assert_eq!(state.track, active);
        assert_eq!(hardware().voices[0].starts.len(), 1);
        hardware().assert_stopped();
        HARDWARE = None;
    }
}

#[test]
fn publication_rounding_covers_every_predictor_value() {
    for value in i16::MIN..=i16::MAX {
        let expected = ((value as i32 + 128).div_euclid(256).clamp(-128, 127)) as i8;
        assert_eq!(backend::rounded_sample(value) as i8, expected, "{value}");
    }
}
