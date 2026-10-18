use havoc::audio::{
    note_frequency, AudioEngine, AudioEvent, Effect, MusicState, PageRequest, ScoreError, SidBus,
    VideoStandard, SID_BASES,
};
use havoc::music::{
    page_checksum, EVENT_OFFSET, INSTRUMENT_OFFSET, MUSIC_VOICES, PAGE_ROWS, PAGE_SIZE, SID_COUNT,
};
use std::ops::{Deref, DerefMut};

#[derive(Default)]
struct Bus {
    registers: [[u8; 25]; SID_COUNT],
    writes: Vec<(u8, u8, u8)>,
}

impl SidBus for Bus {
    fn write(&mut self, sid: u8, reg: u8, value: u8) {
        assert!((sid as usize) < SID_COUNT && reg < 25);
        self.registers[sid as usize][reg as usize] = value;
        self.writes.push((sid, reg, value));
    }
}

fn seal(page: &mut [u8; PAGE_SIZE]) {
    let checksum = page_checksum(page);
    page[10..12].copy_from_slice(&checksum.to_le_bytes());
}

fn page(cue: MusicState, index: u8, next: u8, ticks: u8) -> [u8; PAGE_SIZE] {
    let mut bytes = [0; PAGE_SIZE];
    bytes[..4].copy_from_slice(b"SPG2");
    bytes[4] = cue as u8;
    bytes[5] = index;
    bytes[6] = next;
    bytes[7] = ticks;
    bytes[8] = MUSIC_VOICES as u8;
    for voice in 0..MUSIC_VOICES {
        let at = INSTRUMENT_OFFSET + voice * 5;
        bytes[at..at + 5].copy_from_slice(&[0x40, 0, 8, 0x02, 0x73]);
    }
    seal(&mut bytes);
    bytes
}

fn fixture(cue: MusicState) -> [u8; PAGE_SIZE] {
    let mut bytes = page(cue, 0, 0, 10);
    for (voice, note) in [40, 64, 55, 76].into_iter().enumerate() {
        bytes[EVENT_OFFSET + voice] = note;
        for row in 1..PAGE_ROWS {
            bytes[EVENT_OFFSET + row * MUSIC_VOICES + voice] = 1;
        }
    }
    seal(&mut bytes);
    bytes
}

// Exercise all retained sound-effect regressions against a continuously serviced
// real player. Extra music lanes stay silent in these isolated effect fixtures.
struct TestPlayer {
    player: AudioEngine,
    next: [u8; PAGE_SIZE],
}
impl Deref for TestPlayer {
    type Target = AudioEngine;
    fn deref(&self) -> &AudioEngine {
        &self.player
    }
}
impl DerefMut for TestPlayer {
    fn deref_mut(&mut self) -> &mut AudioEngine {
        &mut self.player
    }
}
impl TestPlayer {
    fn service(&mut self) {
        if let Some(request) = self.player.page_request() {
            self.next = fixture(request.cue);
            self.player.publish_page(request, &self.next).unwrap();
        }
    }
    fn tick(&mut self, bus: &mut Bus) {
        self.service();
        self.player.tick(bus, &self.next);
    }
    fn reset(&mut self, bus: &mut Bus) {
        self.player.reset(bus);
        self.service();
        self.player.tick(bus, &self.next); // Prime the deliberate cue-change gap.
    }
}

fn engine() -> (TestPlayer, Bus) {
    engine_for(VideoStandard::Pal)
}
fn engine_for(standard: VideoStandard) -> (TestPlayer, Bus) {
    let mut player = TestPlayer {
        player: AudioEngine::new(standard),
        next: [0; PAGE_SIZE],
    };
    let mut bus = Bus::default();
    player.reset(&mut bus);
    (player, bus)
}
fn event(effect: Effect, pan: i8) -> AudioEvent {
    AudioEvent { effect, pan }
}
fn music_registers(bus: &Bus) -> [[u8; 14]; 2] {
    let mut result = [[0; 14]; 2];
    for sid in 0..2 {
        result[sid].copy_from_slice(&bus.registers[sid][..14]);
    }
    result
}

#[test]
fn resident_engine_and_external_page_fit_the_profile_allocations() {
    let engine = core::mem::size_of::<AudioEngine>();
    if SID_COUNT == 2 {
        assert!(engine + PAGE_SIZE <= 432, "{engine} + {PAGE_SIZE}");
    } else {
        assert!(engine <= 432, "{engine}");
        assert_eq!(PAGE_SIZE, 224);
    }
    assert_eq!(&SID_BASES[..2], &[0xd400, 0xd420]);
    assert_eq!(SID_BASES.len(), SID_COUNT);
}

#[test]
fn both_video_clocks_produce_concert_a_within_register_precision() {
    for (standard, clock) in [
        (VideoStandard::Pal, 985_248_u64),
        (VideoStandard::Ntsc, 1_022_727),
    ] {
        let frequency = note_frequency(standard, 69) as u64;
        // Error below half a hertz, measured without floating point.
        assert!((frequency * clock).abs_diff(440 * (1 << 24)) < (1 << 23));
        let mut previous = 0;
        for note in 24..=95 {
            let current = note_frequency(standard, note);
            assert!(current > previous);
            previous = current;
        }
        assert_eq!(note_frequency(standard, 23), 0);
        assert_eq!(note_frequency(standard, 96), 0);
    }
}

#[test]
fn pitch_lookup_preserves_every_previous_frequency_and_invalid_note() {
    // FNV-1a of all 256 little-endian results from the original octave/rounding
    // implementation. This also checks both invalid ranges, including 0/255.
    for (standard, expected) in [
        (VideoStandard::Pal, 0xec87_49a2),
        (VideoStandard::Ntsc, 0x67b1_1702),
    ] {
        let mut hash = 2_166_136_261u32;
        for note in 0..=255 {
            let frequency = note_frequency(standard, note);
            if !(24..=95).contains(&note) {
                assert_eq!(frequency, 0);
            }
            for byte in frequency.to_le_bytes() {
                hash = (hash ^ byte as u32).wrapping_mul(16_777_619);
            }
        }
        assert_eq!(hash, expected, "{standard:?} pitch compatibility");
    }
}

#[test]
fn stereo_routing_uses_only_reserved_effects_voices() {
    let (mut player, mut bus) = engine();
    player.tick(&mut bus);
    let music_before = [
        bus.registers[0][..14].to_vec(),
        bus.registers[1][..14].to_vec(),
    ];
    bus.writes.clear();
    player.queue(event(Effect::Fire, -100));
    player.tick(&mut bus);
    assert_eq!(bus.registers[0][18], 0x81);
    assert_eq!(bus.registers[1][18] & 1, 0);
    for sid in 0..2 {
        assert_eq!(&bus.registers[sid][..14], music_before[sid]);
        assert_eq!(bus.registers[sid][23] & 7, 3);
    }
    assert!(bus
        .writes
        .iter()
        .all(|&(_, register, _)| (14..=20).contains(&register)));
    player.queue(event(Effect::Pickup, 100));
    player.tick(&mut bus);
    assert_eq!(bus.registers[1][18], 0x11);
    player.queue(event(Effect::Bomb, 0));
    player.tick(&mut bus);
    assert_eq!(bus.registers[0][18], 0x81);
    assert_eq!(bus.registers[1][18], 0x81);
    assert_eq!(&bus.registers[0][14..21], &bus.registers[1][14..21]);
}

#[test]
fn priority_replacement_and_centered_acquisition_are_deterministic() {
    let (mut player, mut bus) = engine();
    player.queue(event(Effect::Death, -100));
    player.tick(&mut bus);
    player.queue(event(Effect::Pickup, 0));
    player.tick(&mut bus);
    assert_eq!(
        bus.registers[1][18] & 1,
        0,
        "center cue must acquire both voices"
    );
    player.queue(event(Effect::Fire, -100));
    player.tick(&mut bus);
    assert_eq!(
        bus.registers[0][18], 0x81,
        "ordinary shots cannot steal death"
    );
    player.queue(event(Effect::Bomb, 0));
    player.tick(&mut bus);
    assert_eq!(
        bus.registers[0][19], 0x08,
        "new equal-priority bomb replaces death"
    );
    assert_eq!(bus.registers[1][18], 0x81);
}

#[test]
fn queue_overflow_retains_priority_and_coalesces_auto_fire() {
    let (mut player, mut bus) = engine();
    for _ in 0..16 {
        assert!(player.queue(event(Effect::Fire, -100)));
    }
    assert!(player.queue(event(Effect::Pickup, 100)));
    assert!(player.queue(event(Effect::Hit, -100)));
    assert!(player.queue(event(Effect::Bomb, 0)));
    assert!(player.queue(event(Effect::Death, 0)));
    assert!(!player.queue(event(Effect::EnemyFire, 100)));
    player.tick(&mut bus);
    assert_eq!(bus.registers[0][19], 0x09);
    assert_eq!(bus.registers[1][19], 0x09);
}

#[test]
fn effects_finish_and_volume_is_never_used_for_digital_audio() {
    let (mut player, mut bus) = engine();
    player.queue(event(Effect::Fire, 0));
    bus.writes.clear();
    player.tick(&mut bus);
    for _ in 0..4 {
        player.tick(&mut bus);
    }
    assert_eq!(bus.registers[0][18] & 1, 0);
    assert_eq!(bus.registers[1][18] & 1, 0);
    assert!(bus.writes.iter().all(|&(_, register, _)| register != 24));
    player.silence(&mut bus);
    for sid in 0..2 {
        for control in [4, 11, 18, 24] {
            assert_eq!(bus.registers[sid][control], 0);
        }
    }
    assert!(!player.queue(event(Effect::Bomb, 0)));
    bus.writes.clear();
    player.tick(&mut bus);
    assert!(bus.writes.is_empty());
    player.reset(&mut bus);
    player.tick(&mut bus);
    assert_eq!(bus.registers[0][24], 0x1c);
    assert_eq!(bus.registers[0][4] & 1, 1);
}

#[test]
fn main_gun_has_noise_attack_pulse_sweep_and_release_on_both_video_clocks() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let (mut player, mut bus) = engine_for(standard);
        player.queue(event(Effect::Fire, 0));
        player.tick(&mut bus);
        for sid in 0..2 {
            let registers = bus.registers[sid];
            assert_eq!(
                u16::from_le_bytes([registers[14], registers[15]]),
                note_frequency(standard, 88)
            );
            assert_eq!(&registers[18..21], &[0x81, 0x02, 0xb1]);
        }

        let mut frequency = note_frequency(standard, 52);
        for (age, width) in [(1, 0x0200_u16), (2, 0x0500), (3, 0x0800)] {
            if age > 1 {
                frequency -= frequency >> 4;
            }
            bus.writes.clear();
            player.tick(&mut bus);
            for sid in 0..2 {
                let registers = bus.registers[sid];
                assert_eq!(
                    u16::from_le_bytes([registers[14], registers[15]]),
                    frequency
                );
                assert_eq!(u16::from_le_bytes([registers[16], registers[17]]), width);
                assert_eq!(registers[18], 0x41);
                let writes: Vec<_> = bus
                    .writes
                    .iter()
                    .filter(|&&(chip, reg, _)| chip == sid as u8 && (14..=20).contains(&reg))
                    .map(|&(_, reg, value)| (reg, value))
                    .collect();
                assert!(writes
                    .iter()
                    .all(|&(reg, value)| reg != 18 || value & 1 != 0));
                if age == 1 {
                    let pulse = writes
                        .iter()
                        .position(|&write| write == (18, 0x41))
                        .unwrap();
                    for register in [14, 15, 17] {
                        let setup = writes.iter().position(|&(reg, _)| reg == register).unwrap();
                        assert!(
                            setup < pulse,
                            "pitch and width must precede the pulse waveform"
                        );
                    }
                }
            }
        }
        bus.writes.clear();
        player.tick(&mut bus); // Release at 80 ms, retaining the pulse waveform.
        for sid in 0..2 {
            assert_eq!(bus.registers[sid][18], 0x40);
        }
        assert_eq!(
            bus.writes
                .iter()
                .copied()
                .filter(|&(_, reg, _)| reg >= 14)
                .collect::<Vec<_>>(),
            [(0, 18, 0x40), (1, 18, 0x40)]
        );
    }
}

#[test]
fn sustained_main_gun_releases_between_shots_without_changing_music_or_filters() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for music in [
            MusicState::Lowlands,
            MusicState::Canyon,
            MusicState::Carrier,
            MusicState::Victory,
            MusicState::Defeat,
        ] {
            let (mut player, mut bus) = engine_for(standard);
            let (mut baseline, mut baseline_bus) = engine_for(standard);
            player.set_music(music);
            baseline.set_music(music);
            for tick in 0..110 {
                if tick < 100 && tick % 5 == 0 {
                    player.queue(event(Effect::Fire, 0));
                }
                bus.writes.clear();
                baseline_bus.writes.clear();
                player.tick(&mut bus);
                baseline.tick(&mut baseline_bus);
                assert_eq!(music_registers(&bus), music_registers(&baseline_bus));
                for sid in 0..2 {
                    assert_eq!(
                        &bus.registers[sid][21..],
                        &baseline_bus.registers[sid][21..]
                    );
                    let expected = if tick >= 100 || tick % 5 == 4 {
                        0x40
                    } else if tick % 5 == 0 {
                        0x81
                    } else {
                        0x41
                    };
                    assert_eq!(bus.registers[sid][18], expected);
                }
                let non_effect_writes: Vec<_> = bus
                    .writes
                    .iter()
                    .copied()
                    .filter(|&(_, reg, _)| !(14..=20).contains(&reg))
                    .collect();
                assert_eq!(non_effect_writes, baseline_bus.writes);
                assert!(bus.writes.iter().all(|&(_, reg, _)| reg < 21));
                if tick >= 100 {
                    assert!(bus.writes.iter().all(|&(_, reg, _)| reg < 14));
                }
            }
        }
    }
}

#[test]
fn main_gun_keeps_early_retrigger_suppression_and_resets_its_pulse_width() {
    let (mut player, mut bus) = engine();
    player.queue(event(Effect::Fire, 0));
    player.tick(&mut bus);
    player.queue(event(Effect::Fire, 0));
    bus.writes.clear();
    player.tick(&mut bus);
    for sid in 0..2 {
        assert_eq!(bus.registers[sid][18], 0x41);
        assert_eq!(bus.registers[sid][17], 2);
    }
    assert!(bus
        .writes
        .iter()
        .all(|&(_, reg, value)| reg != 18 || value & 1 != 0));
    player.queue(event(Effect::Fire, 0));
    player.tick(&mut bus); // Retrigger becomes eligible at age two.
    for sid in 0..2 {
        assert_eq!(bus.registers[sid][18], 0x81);
        assert_eq!(&bus.registers[sid][16..18], &[0, 8]);
    }
}

#[test]
fn higher_priority_effect_interrupts_main_gun_and_restores_default_pulse_width() {
    let (mut player, mut bus) = engine();
    player.queue(event(Effect::Fire, 0));
    player.tick(&mut bus);
    player.tick(&mut bus);
    assert_eq!(bus.registers[0][17], 2);
    player.queue(event(Effect::Roll, 0));
    player.tick(&mut bus);
    let (mut baseline, mut baseline_bus) = engine();
    baseline.queue(event(Effect::Roll, 0));
    baseline.tick(&mut baseline_bus);
    for sid in 0..2 {
        assert_eq!(
            &bus.registers[sid][14..21],
            &baseline_bus.registers[sid][14..21]
        );
        assert_eq!(&bus.registers[sid][16..18], &[0, 8]);
    }
    player.queue(event(Effect::Fire, 0));
    player.tick(&mut bus);
    baseline.tick(&mut baseline_bus);
    for sid in 0..2 {
        assert_eq!(
            &bus.registers[sid][14..21],
            &baseline_bus.registers[sid][14..21]
        );
    }
}

#[test]
fn enemy_fire_has_a_noise_attack_falling_sawtooth_and_release_in_each_stereo_lane() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for pan in [-100, 0, 100] {
            let (mut player, mut bus) = engine_for(standard);
            let idle = bus.registers;
            player.queue(event(Effect::EnemyFire, pan));
            let mut frequency = note_frequency(standard, 84);
            for age in 0..=6 {
                if age == 1 {
                    frequency = note_frequency(standard, 64);
                } else if (2..6).contains(&age) {
                    frequency -= frequency >> 3;
                }
                bus.writes.clear();
                player.tick(&mut bus);
                for sid in 0..2 {
                    let writes: Vec<_> = bus
                        .writes
                        .iter()
                        .filter(|&&(chip, reg, _)| chip == sid as u8 && (14..=20).contains(&reg))
                        .map(|&(_, reg, value)| (reg, value))
                        .collect();
                    if (pan < 0 && sid == 1) || (pan > 0 && sid == 0) {
                        assert_eq!(&bus.registers[sid][14..21], &idle[sid][14..21]);
                        assert!(writes.is_empty());
                        continue;
                    }
                    let registers = bus.registers[sid];
                    assert_eq!(
                        u16::from_le_bytes([registers[14], registers[15]]),
                        frequency
                    );
                    assert_eq!(&registers[16..18], &[0, 8]);
                    assert_eq!(&registers[19..21], &[0x02, 0x72]);
                    assert_eq!(
                        registers[18],
                        match age {
                            0 => 0x81,
                            6 => 0x20,
                            _ => 0x21,
                        }
                    );
                    if (1..6).contains(&age) {
                        assert!(writes
                            .iter()
                            .all(|&(reg, value)| reg != 18 || value & 1 != 0));
                    }
                    if age == 1 {
                        let sawtooth = writes
                            .iter()
                            .position(|&write| write == (18, 0x21))
                            .unwrap();
                        for register in [14, 15] {
                            let pitch =
                                writes.iter().position(|&(reg, _)| reg == register).unwrap();
                            assert!(pitch < sawtooth, "body pitch must precede its waveform");
                        }
                    } else if age == 6 {
                        assert_eq!(writes, [(18, 0x20)]);
                    }
                }
            }
        }
    }
}

#[test]
fn enemy_fire_coalesces_rapid_shots_and_protects_the_first_body_tick() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let (mut player, mut bus) = engine_for(standard);
        player.queue(event(Effect::Fire, 100));
        for tick in 0..4 {
            for _ in 0..16 {
                assert!(player.queue(event(Effect::EnemyFire, -100)));
            }
            bus.writes.clear();
            player.tick(&mut bus);
            let attack = tick % 2 == 0;
            assert_eq!(bus.registers[0][18], if attack { 0x81 } else { 0x21 });
            assert_eq!(
                u16::from_le_bytes([bus.registers[0][14], bus.registers[0][15]]),
                note_frequency(standard, if attack { 84 } else { 64 })
            );
            if tick == 0 {
                assert_eq!(
                    &bus.registers[1][18..21],
                    &[0x81, 0x02, 0xb1],
                    "coalesced enemy shots must not evict the earlier player shot"
                );
            }
            if !attack {
                assert!(bus
                    .writes
                    .iter()
                    .all(|&(sid, reg, value)| sid != 0 || reg != 18 || value & 1 != 0));
            }
        }
    }
}

#[test]
fn higher_priority_effects_interrupt_enemy_fire_and_block_its_retrigger() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for effect in [Effect::Roll, Effect::Hit, Effect::Bomb, Effect::Death] {
            let (mut player, mut bus) = engine_for(standard);
            player.queue(event(Effect::EnemyFire, 0));
            player.tick(&mut bus);
            player.tick(&mut bus);
            player.queue(event(effect, 0));
            player.tick(&mut bus);
            let (mut baseline, mut baseline_bus) = engine_for(standard);
            baseline.queue(event(effect, 0));
            baseline.tick(&mut baseline_bus);
            for sid in 0..2 {
                assert_eq!(
                    &bus.registers[sid][14..21],
                    &baseline_bus.registers[sid][14..21]
                );
            }
            player.queue(event(Effect::EnemyFire, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            for sid in 0..2 {
                assert_eq!(
                    &bus.registers[sid][14..21],
                    &baseline_bus.registers[sid][14..21]
                );
            }
        }
    }
}

#[test]
fn sustained_enemy_fire_preserves_every_music_lane_filter_and_music_write() {
    let mut first = page(MusicState::Title, 0, 0, 5);
    for row in 0..PAGE_ROWS {
        for voice in 0..MUSIC_VOICES {
            first[EVENT_OFFSET + row * MUSIC_VOICES + voice] =
                48 + voice as u8 + (row % 4) as u8 * 2;
        }
    }
    seal(&mut first);
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let (mut player, mut bus) = ready_player(&first, standard);
        let (mut baseline, mut baseline_bus) = ready_player(&first, standard);
        for tick in 0..112 {
            if let Some(request) = player.page_request() {
                player.publish_page(request, &first).unwrap();
            }
            if let Some(request) = baseline.page_request() {
                baseline.publish_page(request, &first).unwrap();
            }
            if tick < 100 && tick % 3 == 0 {
                player.queue(event(Effect::EnemyFire, [-100, 0, 100][tick / 3 % 3]));
            }
            bus.writes.clear();
            baseline_bus.writes.clear();
            player.tick(&mut bus, &first);
            baseline.tick(&mut baseline_bus, &first);
            for sid in 0..SID_COUNT {
                let music_end = if sid < 2 { 14 } else { 21 };
                assert_eq!(
                    &bus.registers[sid][..music_end],
                    &baseline_bus.registers[sid][..music_end]
                );
                assert_eq!(
                    &bus.registers[sid][21..],
                    &baseline_bus.registers[sid][21..]
                );
                if tick == 0 {
                    for voice in 0..music_end / 7 {
                        assert_ne!(bus.registers[sid][voice * 7 + 4] & 1, 0);
                    }
                }
            }
            let music_writes: Vec<_> = bus
                .writes
                .iter()
                .copied()
                .filter(|&(sid, reg, _)| sid >= 2 || !(14..=20).contains(&reg))
                .collect();
            assert_eq!(music_writes, baseline_bus.writes);
            if tick >= 106 {
                assert_eq!(bus.registers[0][18], 0x20);
                assert_eq!(bus.registers[1][18], 0x20);
                assert_eq!(bus.writes, baseline_bus.writes);
            }
        }
        assert_eq!(player.underruns(), 0);
        assert_eq!(baseline.underruns(), 0);
    }
}

#[test]
fn missile_launch_descends_for_400_ms_and_blocks_fire_without_touching_music() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let (mut player, mut bus) = engine_for(standard);
        let (mut baseline, mut baseline_bus) = engine_for(standard);
        player.set_music(MusicState::Canyon);
        baseline.set_music(MusicState::Canyon);
        player.queue(event(Effect::MissileLaunch, 0));
        let mut frequency = note_frequency(standard, 76);
        for age in 0..=20 {
            if (1..20).contains(&age) {
                frequency -= frequency >> 4;
                player.queue(event(Effect::Fire, 0));
            }
            bus.writes.clear();
            baseline_bus.writes.clear();
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            for sid in 0..2 {
                let registers = bus.registers[sid];
                assert_eq!(
                    u16::from_le_bytes([registers[14], registers[15]]),
                    frequency
                );
                assert_eq!(&registers[16..18], &[0, 8]);
                assert_eq!(registers[18], if age == 20 { 0x80 } else { 0x81 });
                assert_eq!(&registers[19..21], &[0x24, 0xb3]);
                assert_eq!(&registers[21..], &baseline_bus.registers[sid][21..]);
            }
            assert_eq!(music_registers(&bus), music_registers(&baseline_bus));
            let non_effect_writes: Vec<_> = bus
                .writes
                .iter()
                .copied()
                .filter(|&(_, reg, _)| !(14..=20).contains(&reg))
                .collect();
            assert_eq!(non_effect_writes, baseline_bus.writes);
            if age > 0 {
                assert!(bus
                    .writes
                    .iter()
                    .all(|&(_, reg, _)| ![16, 17, 19, 20, 21, 22, 23, 24].contains(&reg)));
            }
        }
        player.queue(event(Effect::Fire, 0));
        player.tick(&mut bus);
        for sid in 0..2 {
            assert_eq!(&bus.registers[sid][18..21], &[0x81, 0x02, 0xb1]);
            assert_eq!(
                u16::from_le_bytes([bus.registers[sid][14], bus.registers[sid][15]]),
                note_frequency(standard, 88)
            );
        }
    }
}

#[test]
fn hit_bomb_and_death_can_preempt_missile_launch() {
    assert_eq!(Effect::MissileLaunch.priority(), 3);
    for effect in [Effect::Hit, Effect::Bomb, Effect::Death] {
        let (mut player, mut bus) = engine();
        player.queue(event(Effect::MissileLaunch, 0));
        player.tick(&mut bus);
        player.tick(&mut bus);
        player.queue(event(effect, 0));
        player.tick(&mut bus);
        let (mut baseline, mut baseline_bus) = engine();
        baseline.queue(event(effect, 0));
        baseline.tick(&mut baseline_bus);
        for sid in 0..2 {
            assert_eq!(
                &bus.registers[sid][14..21],
                &baseline_bus.registers[sid][14..21]
            );
        }
    }
}

#[test]
fn queued_missiles_reject_lower_priority_overflow_but_admit_bombs() {
    let (mut player, mut bus) = engine();
    for _ in 0..4 {
        assert!(player.queue(event(Effect::MissileLaunch, 0)));
    }
    assert!(!player.queue(event(Effect::Fire, 0)));
    assert!(!player.queue(event(Effect::Pickup, 0)));
    assert!(player.queue(event(Effect::Bomb, 0)));
    player.tick(&mut bus);
    for sid in 0..2 {
        assert_eq!(&bus.registers[sid][18..21], &[0x81, 0x08, 0xd8]);
    }
}

#[test]
fn scenic_cues_have_distinct_short_envelopes_and_preserve_the_entire_score() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let mut first = page(MusicState::Title, 0, 0, 3);
        for row in 0..PAGE_ROWS {
            for voice in 0..MUSIC_VOICES {
                first[EVENT_OFFSET + row * MUSIC_VOICES + voice] =
                    48 + voice as u8 + (row % 4) as u8 * 2;
            }
        }
        seal(&mut first);
        for (effect, note, waveform, duration) in [
            (Effect::Footstep, 42, 0x80, 5),
            (Effect::BirdCall, 84, 0x10, 10),
            (Effect::Transport, 42, 0x40, 30),
            (Effect::TransportAway, 35, 0x40, 30),
            (Effect::Meteor, 84, 0x10, 21),
        ] {
            for (pan, sid) in [(-100, 0), (100, 1)] {
                let (mut player, mut bus) = ready_player(&first, standard);
                let (mut baseline, mut baseline_bus) = ready_player(&first, standard);
                player.queue(event(effect, pan));
                let mut frequencies = Vec::new();
                for age in 0..=duration + 3 {
                    if let Some(request) = player.page_request() {
                        player.publish_page(request, &first).unwrap();
                    }
                    if let Some(request) = baseline.page_request() {
                        baseline.publish_page(request, &first).unwrap();
                    }
                    bus.writes.clear();
                    baseline_bus.writes.clear();
                    player.tick(&mut bus, &first);
                    baseline.tick(&mut baseline_bus, &first);
                    let registers = bus.registers[sid];
                    let waveform = if effect == Effect::Meteor && age >= 4 {
                        0x80
                    } else {
                        waveform
                    };
                    assert_eq!(registers[18], waveform | u8::from(age < duration));
                    if effect == Effect::Meteor && age == 4 {
                        assert!(bus.writes.contains(&(sid as u8, 18, 0x81)));
                        assert!(
                            bus.writes.iter().all(|&(chip, reg, value)| {
                                chip as usize != sid || reg != 18 || value & 1 != 0
                            }),
                            "meteor changes waveform without retriggering its gate"
                        );
                    }
                    assert!(registers[20] & 15 <= 2, "short SID release: {effect:?}");
                    assert!(registers[20] >> 4 <= 6, "scenery stays below weapon level");
                    if age < duration {
                        frequencies.push(u16::from_le_bytes([registers[14], registers[15]]));
                    }
                    let music_writes: Vec<_> = bus
                        .writes
                        .iter()
                        .copied()
                        .filter(|&(chip, reg, _)| chip >= 2 || !(14..=20).contains(&reg))
                        .collect();
                    assert_eq!(music_writes, baseline_bus.writes);
                    assert_eq!(bus.registers[1 - sid], baseline_bus.registers[1 - sid]);
                    for chip in 0..SID_COUNT {
                        let end = if chip < 2 { 14 } else { 21 };
                        assert_eq!(
                            &bus.registers[chip][..end],
                            &baseline_bus.registers[chip][..end]
                        );
                        assert_eq!(
                            &bus.registers[chip][21..],
                            &baseline_bus.registers[chip][21..]
                        );
                    }
                }
                assert_eq!(frequencies[0], note_frequency(standard, note));
                assert!(
                    frequencies.windows(2).all(|pair| {
                        if matches!(effect, Effect::Transport | Effect::TransportAway) {
                            pair[0] == pair[1]
                        } else {
                            pair[0] > pair[1]
                        }
                    }),
                    "{effect:?} pitch contour on {standard:?}"
                );
                assert_eq!(player.underruns(), 0);
            }
        }
    }
}

#[test]
fn scenic_priority_beats_gunfire_and_yields_to_player_actions() {
    for scene in [
        Effect::Footstep,
        Effect::BirdCall,
        Effect::Transport,
        Effect::TransportAway,
        Effect::Meteor,
    ] {
        assert_eq!(scene.priority(), 1);
        for fire in [Effect::Fire, Effect::EnemyFire] {
            assert_eq!(fire.priority(), 0);
            let (mut player, mut bus) = engine();
            let (mut baseline, mut baseline_bus) = engine();
            player.queue(event(fire, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            player.queue(event(scene, 0));
            baseline.queue(event(scene, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            player.queue(event(fire, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            assert_eq!(bus.registers, baseline_bus.registers);
        }
        for stronger in [
            Effect::Pickup,
            Effect::Roll,
            Effect::Hit,
            Effect::MissileLaunch,
            Effect::Bomb,
            Effect::Death,
        ] {
            let (mut player, mut bus) = engine();
            let (mut baseline, mut baseline_bus) = engine();
            player.queue(event(scene, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            player.queue(event(stronger, 0));
            baseline.queue(event(stronger, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            player.queue(event(scene, 0));
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            assert_eq!(bus.registers, baseline_bus.registers);
        }
        let (mut player, _) = engine();
        for _ in 0..4 {
            assert!(player.queue(event(scene, 0)));
        }
        assert!(!player.queue(event(Effect::Fire, 0)));
        assert!(player.queue(event(Effect::Roll, 0)));
    }
}

#[test]
fn scene_cancellation_releases_on_the_next_tick_and_retains_player_events() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let (mut player, mut bus) = engine_for(standard);
        let (mut baseline, mut baseline_bus) = engine_for(standard);
        player.queue(event(Effect::Transport, -100));
        player.queue(event(Effect::Roll, 100));
        baseline.queue(event(Effect::Roll, 100));
        player.tick(&mut bus);
        baseline.tick(&mut baseline_bus);
        bus.writes.clear();
        player.cancel_scene_effects();
        player.cancel_scene_effects();
        assert!(
            bus.writes.is_empty(),
            "foreground cancellation never touches SID"
        );
        assert_eq!(bus.registers[0][18] & 1, 1);
        player.tick(&mut bus);
        baseline.tick(&mut baseline_bus);
        assert_eq!(bus.registers[0][18] & 1, 0);
        assert_eq!(bus.registers[1], baseline_bus.registers[1]);

        for effect in [
            Effect::Footstep,
            Effect::Fire,
            Effect::Meteor,
            Effect::Pickup,
        ] {
            let pan = if matches!(effect, Effect::Footstep | Effect::Fire) {
                -100
            } else {
                100
            };
            player.queue(event(effect, pan));
            if matches!(effect, Effect::Fire | Effect::Pickup) {
                baseline.queue(event(effect, pan));
            }
        }
        player.cancel_scene_effects();
        for _ in 0..35 {
            player.tick(&mut bus);
            baseline.tick(&mut baseline_bus);
            assert_eq!(bus.registers, baseline_bus.registers);
        }
        player.queue(event(Effect::Meteor, 0));
        player.tick(&mut bus);
        player.queue(event(Effect::BirdCall, 0));
        player.cancel_scene_effects();
        player.tick(&mut bus);
        assert_eq!(bus.registers[0][18] & 1, 0);
        assert_eq!(bus.registers[1][18] & 1, 0);
    }
}

fn ready_player(first: &[u8; PAGE_SIZE], standard: VideoStandard) -> (AudioEngine, Bus) {
    let mut player = AudioEngine::new(standard);
    let mut bus = Bus::default();
    player.reset(&mut bus);
    let request = player.page_request().unwrap();
    assert_eq!(request.cue as u8, first[4]);
    player.publish_page(request, first).unwrap();
    player.tick(&mut bus, first); // Consume opening; retain its gate-low tick.
    bus.writes.clear();
    (player, bus)
}

fn lead(bus: &Bus) -> (u16, bool) {
    (
        u16::from_le_bytes([bus.registers[1][0], bus.registers[1][1]]),
        bus.registers[1][4] & 1 != 0,
    )
}

#[test]
fn playback_requires_explicit_startup_reset_and_a_published_opening() {
    let first = fixture(MusicState::Title);
    let mut player = AudioEngine::new(VideoStandard::Pal);
    let mut bus = Bus::default();
    player
        .publish_page(player.page_request().unwrap(), &first)
        .unwrap();
    player.tick(&mut bus, &first);
    assert!(bus.writes.is_empty());
    player.reset(&mut bus);
    player.tick(&mut bus, &first);
    assert!(!lead(&bus).1, "reset discarded the old pending page");
    player
        .publish_page(player.page_request().unwrap(), &first)
        .unwrap();
    player.tick(&mut bus, &first);
    assert!(!lead(&bus).1);
    player.tick(&mut bus, &first);
    assert!(lead(&bus).1);
}

#[test]
fn corrupt_or_obsolete_pages_do_not_replace_or_cancel_playback() {
    let first = fixture(MusicState::Title);
    let (mut player, mut bus) = ready_player(&first, VideoStandard::Pal);
    player.tick(&mut bus, &first);
    let request = player.page_request().unwrap();
    assert_eq!(
        player.publish_page(request, &first[..PAGE_SIZE - 1]),
        Err(ScoreError::Truncated)
    );
    for (offset, value, error) in [
        (0, 0, ScoreError::Signature),
        (4, MusicState::Canyon as u8, ScoreError::Bounds),
        (7, 0, ScoreError::Tempo),
        (8, 255, ScoreError::Profile),
        (INSTRUMENT_OFFSET, 0x81, ScoreError::Instrument),
        (INSTRUMENT_OFFSET + 2, 16, ScoreError::Instrument),
        (EVENT_OFFSET, 23, ScoreError::Note),
        (EVENT_OFFSET, 0x81, ScoreError::Note),
    ] {
        let mut invalid = first;
        invalid[offset] = value;
        seal(&mut invalid);
        assert_eq!(player.publish_page(request, &invalid), Err(error));
        assert_eq!(player.page_request(), Some(request));
    }
    let mut damaged = first;
    damaged[10] ^= 1;
    assert_eq!(
        player.publish_page(request, &damaged),
        Err(ScoreError::Checksum)
    );
    let frequency = lead(&bus).0;
    player.tick(&mut bus, &damaged); // Unpublished bytes cannot enter playback.
    assert_eq!(lead(&bus), (frequency, true));
    player.set_music(MusicState::Canyon);
    assert_eq!(player.current_cue(), MusicState::Title);
    assert_eq!(
        player.publish_page(request, &first),
        Err(ScoreError::Request)
    );
    assert_eq!(
        player.page_request(),
        Some(PageRequest {
            cue: MusicState::Canyon,
            page: 0
        })
    );
}

#[test]
fn holds_and_legato_keep_the_gate_while_articulated_notes_release_one_tick_early() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let mut first = page(MusicState::Title, 0, 0, 5);
        for (row, event) in [60, 1, 64 | 0x80, 67, 0].into_iter().enumerate() {
            first[EVENT_OFFSET + row * MUSIC_VOICES + 1] = event;
        }
        seal(&mut first);
        let (mut player, mut bus) = ready_player(&first, standard);
        let mut states = Vec::new();
        let mut off_ticks = Vec::new();
        for tick in 0..21 {
            bus.writes.clear();
            player.tick(&mut bus, &first);
            states.push(lead(&bus));
            if bus
                .writes
                .iter()
                .any(|&(sid, reg, value)| sid == 1 && reg == 4 && value & 1 == 0)
            {
                off_ticks.push(tick);
            }
        }
        assert_eq!(states[0], (note_frequency(standard, 60), true));
        assert_eq!(states[5], states[0]);
        assert_eq!(states[10], (note_frequency(standard, 64), true));
        assert_eq!(states[15], (note_frequency(standard, 67), true));
        assert!(!states[20].1);
        assert_eq!(off_ticks, [14, 19]);
    }
}

#[test]
fn page_boundary_holds_preserve_pitch_and_gate_without_adding_time() {
    let mut first = page(MusicState::Title, 0, 1, 5);
    first[EVENT_OFFSET + 1] = 60;
    for row in 1..PAGE_ROWS {
        first[EVENT_OFFSET + row * MUSIC_VOICES + 1] = 1;
    }
    seal(&mut first);
    let mut next = page(MusicState::Title, 1, 0, 5);
    for row in 0..PAGE_ROWS {
        next[EVENT_OFFSET + row * MUSIC_VOICES + 1] = 1;
    }
    seal(&mut next);
    let (mut player, mut bus) = ready_player(&first, VideoStandard::Pal);
    player
        .publish_page(player.page_request().unwrap(), &next)
        .unwrap();
    for tick in 0..=80 {
        player.tick(&mut bus, &next);
        assert_eq!(
            lead(&bus),
            (note_frequency(VideoStandard::Pal, 60), true),
            "tick {tick}"
        );
        assert_eq!(player.current_page(), if tick < 80 { 0 } else { 1 });
    }
    assert_eq!(player.underruns(), 0);
    assert_eq!(
        player.page_request(),
        Some(PageRequest {
            cue: MusicState::Title,
            page: 0
        })
    );
}

#[test]
fn an_instrument_cannot_change_beneath_a_boundary_hold_or_legato() {
    let first = fixture(MusicState::Title);
    let (mut player, _) = ready_player(&first, VideoStandard::Pal);
    for event in [1, 64 | 0x80] {
        let mut next = first;
        next[EVENT_OFFSET + 1] = event;
        next[INSTRUMENT_OFFSET + 5] = 0x20;
        seal(&mut next);
        assert_eq!(
            player.publish_page(player.page_request().unwrap(), &next),
            Err(ScoreError::Transition)
        );
    }
    let mut next = first;
    next[INSTRUMENT_OFFSET + 5] = 0x20;
    seal(&mut next);
    player
        .publish_page(player.page_request().unwrap(), &next)
        .unwrap();
    assert!(player.page_request().is_none());
}

#[test]
fn cue_openings_cannot_depend_on_notes_from_the_previous_cue() {
    let mut player = AudioEngine::new(VideoStandard::Pal);
    for event in [1, 64 | 0x80] {
        let mut first = fixture(MusicState::Title);
        first[EVENT_OFFSET + 1] = event;
        seal(&mut first);
        assert_eq!(
            player.publish_page(player.page_request().unwrap(), &first),
            Err(ScoreError::Transition)
        );
    }
}

#[test]
fn cue_change_keeps_old_music_until_ready_then_uses_one_gate_low_tick() {
    let first = fixture(MusicState::Title);
    let (mut player, mut bus) = ready_player(&first, VideoStandard::Pal);
    player.tick(&mut bus, &first);
    let old = lead(&bus);
    player.set_music(MusicState::Canyon);
    for _ in 0..5 {
        player.tick(&mut bus, &[0xff; PAGE_SIZE]);
        assert_eq!(lead(&bus), old);
    }
    let mut next = fixture(MusicState::Canyon);
    next[EVENT_OFFSET + 1] = 72;
    seal(&mut next);
    player
        .publish_page(player.page_request().unwrap(), &next)
        .unwrap();
    player.tick(&mut bus, &next);
    assert!(!lead(&bus).1);
    assert_eq!(player.current_cue(), MusicState::Canyon);
    player.tick(&mut bus, &next);
    assert_eq!(lead(&bus), (note_frequency(VideoStandard::Pal, 72), true));
}

#[test]
fn repeated_cue_selection_keeps_transport_and_reset_requests_its_opening() {
    let mut first = fixture(MusicState::Title);
    first[6] = 1;
    seal(&mut first);
    let (mut player, mut bus) = ready_player(&first, VideoStandard::Pal);
    let mut next = first;
    next[5] = 1;
    next[6] = 0;
    seal(&mut next);
    player
        .publish_page(player.page_request().unwrap(), &next)
        .unwrap();
    for _ in 0..161 {
        player.set_music(MusicState::Title);
        player.tick(&mut bus, &next);
    }
    assert_eq!(player.current_page(), 1);
    player.reset(&mut bus);
    assert_eq!(player.current_page(), 0xff);
    assert_eq!(
        player.page_request(),
        Some(PageRequest {
            cue: MusicState::Title,
            page: 0
        })
    );
    assert_eq!(player.underruns(), 0);
}

#[test]
fn a_starved_page_holds_transport_counts_once_and_keeps_effects_running() {
    let mut first = fixture(MusicState::Title);
    first[6] = 1;
    first[7] = 5;
    seal(&mut first);
    let (mut player, mut bus) = ready_player(&first, VideoStandard::Pal);
    for _ in 0..100 {
        player.tick(&mut bus, &[0xff; PAGE_SIZE]);
    }
    assert_eq!(player.underruns(), 1);
    assert_eq!(player.current_page(), 0);
    let held = lead(&bus);
    assert!(held.1);
    player.queue(event(Effect::Fire, 0));
    player.tick(&mut bus, &[0xff; PAGE_SIZE]);
    assert_eq!(bus.registers[0][18], 0x81);
    for _ in 0..4 {
        player.tick(&mut bus, &[0xff; PAGE_SIZE]);
    }
    assert_eq!(bus.registers[0][18], 0x40);
    assert_eq!(lead(&bus), held);
    assert_eq!(player.underruns(), 1);
    let mut next = first;
    next[5] = 1;
    next[6] = 0;
    next[EVENT_OFFSET + 1] = 72;
    seal(&mut next);
    player
        .publish_page(player.page_request().unwrap(), &next)
        .unwrap();
    player.tick(&mut bus, &next);
    assert!(
        !lead(&bus).1,
        "resume retains the required pre-attack release tick"
    );
    player.tick(&mut bus, &next);
    assert_eq!(player.current_page(), 1);
    assert_eq!(lead(&bus), (note_frequency(VideoStandard::Pal, 72), true));
    assert_eq!(player.underruns(), 1);
}

#[test]
fn every_compiled_music_lane_plays_without_borrowing_the_effects_voices() {
    let mut first = page(MusicState::Title, 0, 0, 10);
    for voice in 0..MUSIC_VOICES {
        first[EVENT_OFFSET + voice] = 48 + voice as u8;
        for row in 1..PAGE_ROWS {
            first[EVENT_OFFSET + row * MUSIC_VOICES + voice] = 1;
        }
    }
    seal(&mut first);
    let (mut player, mut bus) = ready_player(&first, VideoStandard::Pal);
    player.tick(&mut bus, &first);
    let music = bus.registers;
    for sid in 0..SID_COUNT {
        for voice in 0..if sid < 2 { 2 } else { 3 } {
            assert_ne!(music[sid][voice * 7 + 4] & 1, 0, "SID {sid} voice {voice}");
        }
        assert_eq!(music[sid][23] & 7, if sid < 2 { 3 } else { 7 });
    }
    assert_eq!(music[0][18] & 1, 0);
    assert_eq!(music[1][18] & 1, 0);
    player.queue(event(Effect::Bomb, 0));
    player.tick(&mut bus, &first);
    for sid in 0..SID_COUNT {
        let length = if sid < 2 { 14 } else { 21 };
        assert_eq!(&bus.registers[sid][..length], &music[sid][..length]);
        assert_eq!(&bus.registers[sid][21..], &music[sid][21..]);
    }
}

#[test]
fn lost_wing_ties_preserve_the_eighteen_tick_note_and_breath_spacing() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let mut first = page(MusicState::Title, 0, 0, 9);
        for (row, note) in [(0, 71), (4, 69), (8, 67), (12, 66)] {
            first[EVENT_OFFSET + row * MUSIC_VOICES + 1] = note;
            first[EVENT_OFFSET + (row + 1) * MUSIC_VOICES + 1] = 1;
        }
        seal(&mut first);
        let (mut player, mut bus) = ready_player(&first, standard);
        player
            .publish_page(player.page_request().unwrap(), &first)
            .unwrap();
        let mut attacks = Vec::new();
        let mut releases = Vec::new();
        for tick in 0..144 {
            bus.writes.clear();
            player.tick(&mut bus, &first);
            for &(sid, reg, value) in &bus.writes {
                if sid == 1 && reg == 4 {
                    if value & 1 != 0 {
                        attacks.push(tick);
                    } else {
                        releases.push(tick);
                    }
                }
            }
        }
        assert_eq!(attacks, [0, 36, 72, 108]);
        assert_eq!(releases, [17, 53, 89, 125]);
        assert_eq!(player.underruns(), 0);
    }
}
