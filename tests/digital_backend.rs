//! Production DLT4 decoder against independent REU/sampler models.
//! These are transport/PCM checks, not physical timing measurements.
#![allow(static_mut_refs)]
use havoc::digital as asset;
use std::sync::Mutex;
#[path = "../target-c64/src/digital.rs"]
mod backend;
static SERIAL: Mutex<()> = Mutex::new(());
static mut HARDWARE: Option<Hardware> = None;
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
    version: u8,
    voices: [Voice; 2],
    calibration_starts: usize,
    maximum_dma: usize,
    staging_write_ticks: u64,
    timer_read_ticks: u64,
    produced_during_playback: u64,
    ring_banks_written: [bool; 2],
    final_reu_byte_read: bool,
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
            version: 0x10,
            voices: [Voice::default(), Voice::default()],
            calibration_starts: 0,
            maximum_dma: 0,
            staging_write_ticks: 1,
            timer_read_ticks: 1,
            produced_during_playback: 0,
            ring_banks_written: [false; 2],
            final_reu_byte_read: false,
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
            let frames = (self.ticks - v.playback_start) * 6_250_000 / ((divider + 2) * 985_248);
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
                v.completion = self.ticks + (frames * (divider + 2) * 985_248).div_ceil(6_250_000);
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
            self.ring_banks_written[(dest - asset::RING_OFFSET as usize) >> 16] = true;
            if self.voices[0].playing {
                assert!(
                    count <= 256,
                    "steady publication and prefix clears are bounded"
                );
                self.produced_during_playback +=
                    (count / if self.ram[0xdf20] & 0x40 != 0 { 2 } else { 1 }) as u64;
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
    destination.copy_from_slice(&hw.reu[source as usize..source as usize + destination.len()]);
    hw.ticks += destination.len() as u64;
}

/// Intentionally does not call production header, delta, bit or rounding helpers.
fn reference(track: asset::Track) -> Vec<[i16; 2]> {
    let bytes = read_track(track);
    assert_eq!(&bytes[..4], b"DLT4");
    let samples = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let books = u16::from_le_bytes(bytes[18..20].try_into().unwrap()) as usize;
    let channels = bytes[21] as usize;
    let group = if channels == 1 { 8 } else { 4 };
    let segment = if books == 1 {
        samples
    } else {
        samples / books / group * group
    };
    let stream = 32 + books * 8 * 3 * channels;
    let mut out = Vec::with_capacity(samples);
    let mut previous = [0i32; 2];
    for frame in 0..samples {
        let book = (frame / segment).min(books - 1);
        let mut emitted = [0; 2];
        for channel in 0..channels {
            let bit = (frame * channels + channel) * 3;
            let code = (0..3)
                .map(|b| (((bytes[stream + (bit + b) / 8] >> ((bit + b) % 8)) & 1) as usize) << b)
                .sum::<usize>();
            let at = 32 + ((book * channels + channel) * 8 + code) * 3;
            let magnitude = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as i32;
            let delta = (if bytes[at + 2] != 0 {
                -magnitude
            } else {
                magnitude
            })
            .clamp(-32768, 32767);
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
    backend::service(state).unwrap();
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
            let mut state = backend::State::new();
            backend::init(&mut state, &mut calibration, true, track).unwrap();
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
            backend::start(&mut state);
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
        let alternate = asset::Track::GalacticDash;
        let mono = asset::Track::GalacticDashMono;
        backend::init(&mut state, &mut calibration, true, stereo).unwrap();
        backend::start(&mut state);
        let calibrations = hardware().calibration_starts;
        backend::request(&mut state, mono);
        for _ in 0..3 {
            visit(&mut state);
            assert_eq!(state.track, stereo);
        }
        backend::request(&mut state, stereo);
        for _ in 0..9 {
            visit(&mut state);
        }
        assert_eq!(
            hardware().voices[0].starts.len(),
            1,
            "cancel must retain active position"
        );
        backend::request(&mut state, mono);
        visit(&mut state);
        backend::request(&mut state, alternate);
        for index in 0..8 {
            backend::request(&mut state, alternate);
            visit(&mut state);
            assert_eq!(state.track, if index == 7 { alternate } else { stereo });
        }
        hardware().assert_profile(alternate, 1);
        // Put stereo in slot A again so slot B's future mono prefix contains
        // former PCM (and explicitly poison it to avoid accidental silence).
        backend::request(&mut state, stereo);
        for _ in 0..8 {
            visit(&mut state);
        }
        hardware().assert_profile(stereo, 0);
        let prefix = asset::RING_OFFSET as usize + 0x18000 - DELAY;
        hardware().reu[prefix..prefix + DELAY].fill(0x6b);
        backend::request(&mut state, mono);
        assert!(hardware().reu[prefix..prefix + DELAY]
            .iter()
            .all(|&b| b == 0));
        for index in 0..8 {
            visit(&mut state);
            assert_eq!(state.track, if index == 7 { mono } else { stereo });
        }
        hardware().assert_profile(mono, 1);
        for _ in 0..12 {
            visit(&mut state);
        }
        assert!(
            hardware().final_reu_byte_read,
            "slot B reaches the final REU byte"
        );
        backend::request(&mut state, alternate);
        for _ in 0..8 {
            visit(&mut state);
        }
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

#[test]
fn mapping_crc_and_deadline_faults_mute_and_stop_the_sampler() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        let track = asset::Track::GalacticDash;
        let descriptor = track.descriptor();
        let stream = (descriptor.asset_offset + descriptor.metadata.stream_offset) as usize;
        let mut state = backend::State::new();
        hardware().version = 0xff;
        assert_eq!(
            backend::init(&mut state, &mut backend::Calibration::new(), true, track),
            Err(backend::Fault::Mapping)
        );
        hardware().assert_stopped();
        hardware().version = 0x10;
        hardware().reu[asset::MUSIC_OFFSET as usize] ^= 1;
        assert_eq!(
            backend::init(&mut state, &mut backend::Calibration::new(), true, track),
            Err(backend::Fault::Metadata)
        );
        hardware().assert_stopped();
        hardware().reu[asset::MUSIC_OFFSET as usize] ^= 1;
        for offset in [0, 31, 32, 127, 128, 1023, 1024] {
            hardware().reu[stream + offset] ^= 1;
            assert_eq!(
                backend::init(&mut state, &mut backend::Calibration::new(), true, track),
                Err(backend::Fault::StreamCrc)
            );
            hardware().assert_stopped();
            hardware().reu[stream + offset] ^= 1;
        }
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
                backend::init(&mut state, &mut backend::Calibration::new(), true, track),
                Err(fault)
            );
            hardware().assert_stopped();
            hardware().reu[at] ^= 1;
        }
        backend::init(&mut state, &mut backend::Calibration::new(), true, track).unwrap();
        backend::start(&mut state);
        hardware().wait(2_000_000);
        assert_eq!(backend::service(&mut state), Err(backend::Fault::Underrun));
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
        backend::init(
            &mut state,
            &mut backend::Calibration::new(),
            true,
            asset::Track::GalacticDash,
        )
        .unwrap();
        backend::start(&mut state);
        hardware().timer_read_ticks = 16;
        for _ in 0..20 {
            hardware().wait(197_050);
            let before = hardware().produced_during_playback;
            backend::service(&mut state).unwrap();
            assert!(hardware().produced_during_playback - before < 10_000);
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
        backend::init(
            &mut state,
            &mut backend::Calibration::new(),
            true,
            asset::Track::GalacticDash,
        )
        .unwrap();
        backend::start(&mut state);
        hardware().wait(197_050);
        // Two PCM bytes now replace the old four-byte publication. Slow their
        // modeled stores enough that one bounded service cannot clear backlog.
        hardware().staging_write_ticks = 22;
        hardware().timer_read_ticks = 5;
        let before = hardware().produced_during_playback;
        assert_eq!(backend::service(&mut state), Ok(()));
        assert_eq!(hardware().produced_during_playback - before, 32_768);
        assert_eq!(state.fault, 0);
        assert_eq!(state.underruns, 0);
        assert!(state.minimum_lead > 2048);
        assert_eq!(hardware().ram[0xdf20], 0x43);
        assert_eq!(hardware().ram[0xdf40], 0x43);
        hardware().wait(2_000_000);
        assert_eq!(backend::service(&mut state), Err(backend::Fault::Underrun));
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
        let pending = asset::Track::GalacticDashMono;
        backend::init(&mut state, &mut backend::Calibration::new(), true, active).unwrap();
        backend::start(&mut state);
        let d = pending.descriptor();
        hardware().reu[(d.asset_offset + d.metadata.stream_offset + 1024) as usize] ^= 1;
        backend::request(&mut state, pending);
        // The corruption lies in the second 1024-byte CRC chunk. One mono
        // preparation visit decodes 1536 bytes; the next completes that chunk.
        assert_eq!(backend::service(&mut state), Ok(()));
        assert_eq!(backend::service(&mut state), Err(backend::Fault::StreamCrc));
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
