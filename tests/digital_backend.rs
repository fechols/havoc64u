//! Runs the actual target decoder against deterministic REU/sampler MMIO.
//! This validates transport and decoded samples, not analog hardware timing.
#![allow(static_mut_refs)]

use havoc::digital as asset;
use std::sync::Mutex;

#[path = "../target-c64/src/digital.rs"]
mod backend;

static SERIAL: Mutex<()> = Mutex::new(());
static mut HARDWARE: Option<Hardware> = None;

const TRACKS: [asset::Track; 2] = [asset::Track::GalacticDash, asset::Track::HammondRocker];

fn read_track(track: asset::Track) -> Vec<u8> {
    let name = match track {
        asset::Track::GalacticDash => "galactic-dash.dlt",
        asset::Track::HammondRocker => "hammond-rocker.dlt",
    };
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/music")
            .join(name),
    )
    .unwrap()
}

#[derive(Default)]
struct Voice {
    completion: u64,
    one_shot: bool,
    playing: bool,
    playback_start: u64,
    played: u64,
    position: u32,
    emitted: Vec<i16>,
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
}

impl Hardware {
    fn new() -> Self {
        let mut reu = vec![0u8; asset::REU_SIZE as usize];
        for track in TRACKS {
            let bytes = read_track(track);
            let base = track.descriptor().asset_offset as usize;
            reu[base..base + bytes.len()].copy_from_slice(&bytes);
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
        }
    }
    fn be24(&self, address: usize) -> u32 {
        ((self.ram[address] as u32) << 16)
            | ((self.ram[address + 1] as u32) << 8)
            | self.ram[address + 2] as u32
    }
    fn divider(&self, channel: usize) -> u64 {
        let at = 0xdf2e + channel * 0x20;
        ((self.ram[at] as u64) << 8) | self.ram[at + 1] as u64
    }
    fn flush(&mut self) {
        for channel in 0..2 {
            let base = 0xdf20 + channel * 0x20;
            let control = self.ram[base];
            let start = self.be24(base + 5);
            let length = self.be24(base + 9);
            let repeat_a = self.be24(base + 17);
            let repeat_b = self.be24(base + 21);
            let divider = self.divider(channel);
            let voice = &mut self.voices[channel];
            if !voice.playing {
                continue;
            }
            // Synthetic two-tick pipeline overhead exercises calibration.
            let frames =
                (self.ticks - voice.playback_start) * 6_250_000 / ((divider + 2) * 985_248);
            let stride = if control & 0x40 != 0 { 4 } else { 2 };
            while voice.played < frames {
                if voice.position == repeat_b && control & 2 != 0 {
                    voice.position = repeat_a;
                }
                assert!(voice.position < length, "sampler reached terminal length");
                let offset = (start + voice.position) as usize;
                assert!(offset >= asset::RING_OFFSET as usize && offset + 2 <= self.reu.len());
                voice
                    .emitted
                    .push(i16::from_le_bytes([self.reu[offset], self.reu[offset + 1]]));
                voice.position += stride;
                voice.played += 1;
            }
        }
    }
    fn wait(&mut self, ticks: u64) {
        self.ticks += ticks;
        self.flush();
    }
    fn poke(&mut self, address: u16, value: u8) {
        self.ticks += if (0x8e00..0x9000).contains(&address) {
            self.staging_write_ticks
        } else {
            1
        };
        if address == 0xdf20 || address == 0xdf40 {
            self.flush();
        }
        self.ram[address as usize] = value;
        if address == 0xdf20 || address == 0xdf40 {
            let channel = ((address - 0xdf20) / 0x20) as usize;
            let base = address as usize;
            let frames = self.be24(base + 9) as u64 / 4;
            let divider = self.divider(channel);
            let voice = &mut self.voices[channel];
            voice.one_shot = value == if channel == 0 { 0x55 } else { 0x51 };
            voice.playing = value == 0x53;
            if voice.one_shot {
                assert_eq!(self.ram[base + 1], 0, "calibration must be muted");
                if channel == 0 {
                    self.calibration_starts += 1;
                }
                voice.completion = self.ticks + frames * (divider + 2) * 985_248 / 6_250_000;
            }
            if voice.playing {
                assert_eq!(self.ram[base + 1], 32);
                assert_eq!(self.ram[base + 2], if channel == 0 { 0 } else { 15 });
                assert_eq!(self.ram[base + 4], 1);
                voice.playback_start = self.ticks;
                voice.played = 0;
                voice.position = 0;
                assert_eq!(self.be24(base + 5), asset::RING_OFFSET + channel as u32 * 2);
                assert_eq!(self.be24(base + 9), asset::RING_BYTES + 4);
                assert_eq!(self.be24(base + 17), 0);
                assert_eq!(self.be24(base + 21), asset::RING_BYTES);
                if channel == 1 {
                    assert_eq!(self.divider(0), divider);
                    assert!(self.voices[1].playback_start - self.voices[0].playback_start <= 1);
                }
            }
        } else if address == 0xdf01 && value == 0x90 {
            self.flush();
            let source = u16::from_le_bytes([self.ram[0xdf02], self.ram[0xdf03]]) as usize;
            let destination = self.ram[0xdf04] as usize
                | ((self.ram[0xdf05] as usize) << 8)
                | ((self.ram[0xdf06] as usize) << 16);
            let count = u16::from_le_bytes([self.ram[0xdf07], self.ram[0xdf08]]) as usize;
            assert!(count != 0 && count <= 1024);
            assert!(
                destination >= asset::RING_OFFSET as usize
                    && destination + count <= asset::REU_SIZE as usize
            );
            self.maximum_dma = self.maximum_dma.max(count);
            self.ring_banks_written[(destination - asset::RING_OFFSET as usize) >> 16] = true;
            if self.voices[0].playing {
                assert!(count <= 512 && count % 4 == 0);
                self.produced_during_playback += (count / 4) as u64;
            }
            self.reu[destination..destination + count]
                .copy_from_slice(&self.ram[source..source + count]);
            self.ticks += count as u64;
        }
    }
    fn peek(&mut self, address: u16) -> u8 {
        self.ticks += if (0xdd04..=0xdd07).contains(&address) {
            self.timer_read_ticks
        } else {
            1
        };
        match address {
            0xdf20 => (self.voices[0].one_shot && self.ticks >= self.voices[0].completion) as u8,
            0xdf21 => self.version,
            0xdd04..=0xdd07 => {
                let counter = u32::MAX.wrapping_sub(self.ticks as u32);
                (counter >> ((address - 0xdd04) * 8)) as u8
            }
            _ => self.ram[address as usize],
        }
    }
    fn assert_stopped(&self) {
        for base in [0xdf20, 0xdf40] {
            assert_eq!(self.ram[base], 0);
            assert_eq!(self.ram[base + 1], 0);
        }
    }
}

unsafe fn hardware() -> &'static mut Hardware {
    HARDWARE.as_mut().unwrap()
}
unsafe fn peek(address: u16) -> u8 {
    hardware().peek(address)
}
unsafe fn poke(address: u16, value: u8) {
    hardware().poke(address, value);
}
unsafe fn word(address: u16, value: u16) {
    poke(address, value as u8);
    poke(address + 1, (value >> 8) as u8);
}
unsafe fn dma(source: u32, destination: u16, count: u16) {
    let hw = hardware();
    assert!(count <= 1024);
    hw.maximum_dma = hw.maximum_dma.max(count as usize);
    hw.ram[destination as usize..destination as usize + count as usize]
        .copy_from_slice(&hw.reu[source as usize..source as usize + count as usize]);
    hw.ticks += count as u64;
}
unsafe fn digital_test_fetch(source: u32, destination: &mut [u8]) {
    let hw = hardware();
    assert!(destination.len() <= 128);
    destination.copy_from_slice(&hw.reu[source as usize..source as usize + destination.len()]);
    hw.ticks += destination.len() as u64;
}

fn reference(track: asset::Track) -> Vec<[i16; 2]> {
    let bytes = read_track(track);
    let metadata = asset::parse_header(&bytes, track).unwrap();
    let mut samples = Vec::with_capacity(metadata.total_samples as usize);
    let mut accumulator = [0i16; 2];
    for i in 0..metadata.total_samples as usize {
        let segment =
            (i / metadata.samples_per_segment as usize).min(metadata.codebook_count as usize - 1);
        let packed = bytes[metadata.stream_offset as usize + i];
        for channel in 0..2 {
            let code = ((packed >> (channel * 4)) & 15) as usize;
            let at = asset::HEADER_SIZE + segment * 96 + channel * 48 + code * 3;
            let magnitude = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as i32;
            let delta = if bytes[at + 2] == 0 {
                magnitude
            } else {
                -magnitude
            }
            .clamp(-32768, 32767) as i16;
            accumulator[channel] = accumulator[channel].saturating_add(delta);
        }
        samples.push(accumulator);
    }
    samples
}

#[test]
fn actual_backend_switches_and_loops_both_tracks_through_overlay_scratch_reuse() {
    let _serial = SERIAL.lock().unwrap();
    assert!(core::mem::size_of::<backend::State>() <= 400);
    unsafe {
        HARDWARE = Some(Hardware::new());
        let mut calibration = backend::Calibration::new();
        let mut calibrations = 0;
        for track in TRACKS {
            let expected = reference(track);
            let mut state = backend::State::new();
            backend::init(&mut state, &mut calibration, true, track).unwrap();
            assert_eq!(state.track, track);
            if calibrations == 0 {
                calibrations = hardware().calibration_starts;
            }
            assert!(calibrations >= 2);
            assert_eq!(hardware().calibration_starts, calibrations);
            for voice in &mut hardware().voices {
                voice.emitted.clear();
            }
            backend::start(&mut state);
            while hardware().voices[1].emitted.len() < expected.len() + 4096 {
                // 5 FPS is deliberately slower than the game; source's original
                // 512-sample-per-call service would fail this test immediately.
                hardware().wait(197_050);
                hardware().ram[0x8e00..0x9200].fill(0xa5);
                backend::service(&mut state).unwrap();
            }
            backend::stop(&mut state);
            assert!(state.loops >= 1);
            assert_eq!(state.underruns, 0);
            assert!(state.minimum_lead > 2048);
            hardware().assert_stopped();
            assert_eq!(hardware().ring_banks_written, [true, true]);
            assert!(hardware().maximum_dma <= 1024);
            assert!(
                expected.iter().any(|frame| frame[0] != frame[1]),
                "stereo asset collapsed to mono"
            );
            for channel in 0..2 {
                for (i, &actual) in hardware().voices[channel].emitted.iter().enumerate() {
                    assert_eq!(
                        actual,
                        expected[i % expected.len()][channel],
                        "{track:?} channel {channel} frame {i}"
                    );
                }
            }
        }
        // SID mode may overwrite the union; a new DAC state must reuse the
        // separately retained calibration and start at the song beginning.
        let mut next = backend::State::new();
        backend::init(
            &mut next,
            &mut calibration,
            true,
            asset::Track::GalacticDash,
        )
        .unwrap();
        assert_eq!(hardware().calibration_starts, calibrations);
        assert_eq!(next.loops, 0);
        assert_eq!(next.track, asset::Track::GalacticDash);
        HARDWARE = None;
    }
}

#[test]
fn mapping_crc_and_deadline_faults_mute_and_stop_the_sampler() {
    let _serial = SERIAL.lock().unwrap();
    unsafe {
        HARDWARE = Some(Hardware::new());
        hardware().version = 0xff;
        let mut state = backend::State::new();
        let mut calibration = backend::Calibration::new();
        let track = asset::Track::GalacticDash;
        let descriptor = track.descriptor();
        let stream = descriptor.asset_offset + descriptor.metadata.stream_offset;
        assert_eq!(
            backend::init(&mut state, &mut calibration, true, track),
            Err(backend::Fault::Mapping)
        );
        hardware().version = 0x10;
        for offset in [0, 127, 128, 1023, 1024] {
            hardware().reu[stream as usize + offset] ^= 1;
            assert_eq!(
                backend::init(&mut state, &mut calibration, true, track),
                Err(backend::Fault::StreamCrc)
            );
            hardware().assert_stopped();
            hardware().reu[stream as usize + offset] ^= 1;
        }
        for (offset, fault) in [
            (21, backend::Fault::Metadata),
            (asset::HEADER_SIZE + 48, backend::Fault::CodebookChecksum),
            (
                descriptor.metadata.crc_offset as usize,
                backend::Fault::CrcTableChecksum,
            ),
        ] {
            let at = descriptor.asset_offset as usize + offset;
            hardware().reu[at] ^= 1;
            assert_eq!(
                backend::init(&mut state, &mut calibration, true, track),
                Err(fault)
            );
            hardware().assert_stopped();
            hardware().reu[at] ^= 1;
        }
        backend::init(&mut state, &mut calibration, true, track).unwrap();
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
        let mut calibration = backend::Calibration::new();
        backend::init(
            &mut state,
            &mut calibration,
            true,
            asset::Track::GalacticDash,
        )
        .unwrap();
        backend::start(&mut state);
        // Account for native clock-query/arithmetic overhead. Each query
        // advances several samples, so chasing an exact zero deficit without
        // batch hysteresis would exhaust the one-ring service work bound.
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
        let mut calibration = backend::Calibration::new();
        backend::init(
            &mut state,
            &mut calibration,
            true,
            asset::Track::GalacticDash,
        )
        .unwrap();
        backend::start(&mut state);
        hardware().wait(197_050);
        // Model a decoder only slightly faster than playback. Its initial
        // backlog cannot be cleared in one ring's worth of foreground work.
        hardware().staging_write_ticks = 8;
        hardware().timer_read_ticks = 5;
        let before = hardware().produced_during_playback;
        assert_eq!(backend::service(&mut state), Ok(()));
        assert_eq!(hardware().produced_during_playback - before, 32_768);
        assert_eq!(state.fault, 0);
        assert_eq!(state.underruns, 0);
        assert!(state.minimum_lead > 2048);
        assert_eq!(hardware().ram[0xdf21], 32);
        assert_eq!(hardware().ram[0xdf20], 0x53);
        assert_eq!(hardware().ram[0xdf40], 0x53);
        hardware().wait(2_000_000);
        assert_eq!(backend::service(&mut state), Err(backend::Fault::Underrun));
        hardware().assert_stopped();
        HARDWARE = None;
    }
}
