use havoc::audio::{
    note_frequency, AudioEngine, AudioEvent, Effect, MusicState, ScoreError, SidBus, VideoStandard,
    DEFAULT_SCORE, SCORE_LENGTH, SID_BASES,
};

#[derive(Default)]
struct Bus {
    registers: [[u8; 25]; 2],
    writes: Vec<(u8, u8, u8)>,
}

impl SidBus for Bus {
    fn write(&mut self, sid: u8, reg: u8, value: u8) {
        assert!(sid < 2 && reg < 25);
        self.registers[sid as usize][reg as usize] = value;
        self.writes.push((sid, reg, value));
    }
}

fn engine() -> (AudioEngine, Bus) {
    let mut engine = AudioEngine::new(VideoStandard::Pal);
    engine.load_score(&DEFAULT_SCORE).unwrap();
    let mut bus = Bus::default();
    engine.reset(&mut bus);
    (engine, bus)
}

fn event(effect: Effect, pan: i8) -> AudioEvent {
    AudioEvent { effect, pan }
}

#[test]
fn resident_engine_fits_reserved_audio_budget() {
    assert!(core::mem::size_of::<AudioEngine>() <= 512);
    assert_eq!(SID_BASES, [0xd400, 0xd420]);
}

#[test]
fn playback_requires_explicit_startup_reset() {
    let mut player = AudioEngine::new(VideoStandard::Pal);
    let mut bus = Bus::default();
    player.load_score(&DEFAULT_SCORE).unwrap();
    player.tick(&mut bus);
    assert!(bus.writes.is_empty());
    player.reset(&mut bus);
    player.tick(&mut bus);
    assert_eq!(bus.registers[0][4] & 1, 1);
}

#[test]
fn score_validation_rejects_corruption_without_losing_current_music() {
    let (mut player, mut bus) = engine();
    assert_eq!(
        player.load_score(&DEFAULT_SCORE[..SCORE_LENGTH - 1]),
        Err(ScoreError::Truncated)
    );
    for (offset, value, error) in [
        (0, 0, ScoreError::Signature),
        (4, 0, ScoreError::Tempo),
        (9, 0x81, ScoreError::Instrument),
        (11, 0xff, ScoreError::Instrument),
        (29, 23, ScoreError::Note),
        (SCORE_LENGTH - 1, 96, ScoreError::Note),
    ] {
        let mut bytes = DEFAULT_SCORE;
        bytes[offset] = value;
        assert_eq!(player.load_score(&bytes), Err(error));
    }
    player.tick(&mut bus);
    assert_ne!(bus.registers[0][4] & 1, 0);
    assert_ne!(bus.registers[1][4] & 1, 0);
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
    assert_eq!(bus.registers[0][18], 0x21);
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
fn all_five_original_themes_play_and_retrigger_at_fifty_hz() {
    let (mut player, mut bus) = engine();
    let mut signatures = Vec::new();
    for state in [
        MusicState::Exploration,
        MusicState::Combat,
        MusicState::Boss,
        MusicState::Victory,
        MusicState::Defeat,
    ] {
        player.set_music(state);
        player.reset(&mut bus);
        bus.writes.clear();
        for _ in 0..160 {
            player.tick(&mut bus);
        }
        assert_eq!(player.music(), state);
        let gates: Vec<_> = bus
            .writes
            .iter()
            .copied()
            .filter(|&(sid, reg, value)| sid == 1 && reg == 4 && value & 1 != 0)
            .collect();
        assert!(gates.len() >= 5);
        assert!(bus.writes.iter().all(|&(_, reg, _)| reg < 14));
        signatures.push(bus.writes.clone());
    }
    for i in 0..signatures.len() {
        for j in i + 1..signatures.len() {
            assert_ne!(signatures[i], signatures[j]);
        }
    }
}

#[test]
fn music_gate_release_precedes_each_retrigger_by_one_audio_tick() {
    let (mut player, mut bus) = engine();
    player.tick(&mut bus);
    assert_eq!(bus.registers[1][4] & 1, 1);
    for _ in 0..8 {
        player.tick(&mut bus);
    }
    assert_eq!(bus.registers[1][4] & 1, 1);
    player.tick(&mut bus);
    assert_eq!(bus.registers[1][4] & 1, 0);
    player.tick(&mut bus); // Row 1 is a rest for the melody.
    assert_eq!(bus.registers[1][4] & 1, 0);
    for _ in 0..10 {
        player.tick(&mut bus);
    }
    assert_eq!(bus.registers[1][4] & 1, 1);
}

fn music_registers(bus: &Bus) -> [[u8; 14]; 2] {
    let mut registers = [[0; 14]; 2];
    for (sid, values) in registers.iter_mut().enumerate() {
        values.copy_from_slice(&bus.registers[sid][..14]);
    }
    registers
}

fn music_ticks(player: &mut AudioEngine, bus: &mut Bus, ticks: usize) -> Vec<[[u8; 14]; 2]> {
    (0..ticks)
        .map(|_| {
            player.tick(bus);
            music_registers(bus)
        })
        .collect()
}

#[test]
fn defeat_has_eight_distinct_phrases_before_repeating_at_46_seconds() {
    const ROW_TICKS: usize = 18;
    const PHRASE_TICKS: usize = 16 * ROW_TICKS;
    const LOOP_TICKS: usize = 8 * PHRASE_TICKS;
    assert_eq!(LOOP_TICKS, 2_304); // 46.08 seconds at the independent 50 Hz clock.
    assert_eq!(DEFAULT_SCORE[8] as usize, ROW_TICKS);

    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        let mut player = AudioEngine::new(standard);
        player.load_score(&DEFAULT_SCORE).unwrap();
        player.set_music(MusicState::Defeat);
        let mut bus = Bus::default();
        player.reset(&mut bus);
        bus.writes.clear();
        let playback = music_ticks(&mut player, &mut bus, LOOP_TICKS * 2);
        assert_eq!(&playback[..LOOP_TICKS], &playback[LOOP_TICKS..]);
        let phrases: Vec<_> = playback[..LOOP_TICKS].chunks_exact(PHRASE_TICKS).collect();
        let mut melody_shapes = Vec::new();
        for (index, phrase) in phrases.iter().enumerate() {
            for previous in &phrases[..index] {
                assert_ne!(phrase, previous, "defeat phrase {index} repeats early");
            }
            let melody: Vec<_> = phrase
                .iter()
                .step_by(ROW_TICKS * 2)
                .map(|row| {
                    if row[1][4] & 1 == 0 {
                        None
                    } else {
                        let frequency = u16::from_le_bytes([row[1][0], row[1][1]]);
                        Some(
                            (24..=95)
                                .find(|&note| note_frequency(standard, note) == frequency)
                                .expect("all melody notes must be in the supported score range"),
                        )
                    }
                })
                .collect();
            let first = melody[0].expect("every phrase begins with a melody") as i16;
            let shape: Vec<_> = melody
                .into_iter()
                .map(|note| note.map(|value| value as i16 - first))
                .collect();
            assert!(
                !melody_shapes.contains(&shape),
                "phrases must vary the melody, not only transpose it"
            );
            melody_shapes.push(shape);
        }
        let mut previous = [[0; 14]; 2];
        for (tick, registers) in playback.iter().enumerate() {
            for sid in 0..2 {
                for control in [4, 11] {
                    let gated = registers[sid][control] & 1 != 0;
                    if gated && previous[sid][control] & 1 == 0 {
                        assert_eq!(tick % (ROW_TICKS * 2), 0);
                    }
                    if tick % ROW_TICKS == ROW_TICKS - 1 {
                        assert!(!gated, "note must release before the next row");
                    }
                }
            }
            previous = *registers;
        }
        assert!(bus.writes.iter().all(|&(_, register, _)| register < 14));
    }
}

#[test]
fn defeat_restarts_its_opening_after_music_changes_and_reset() {
    const PHRASE_TICKS: usize = 16 * 18;
    let (mut player, mut bus) = engine();
    player.set_music(MusicState::Defeat);
    player.reset(&mut bus);
    let opening = music_ticks(&mut player, &mut bus, PHRASE_TICKS);
    music_ticks(&mut player, &mut bus, PHRASE_TICKS * 3 + 70);
    player.set_music(MusicState::Combat);
    player.tick(&mut bus);
    player.tick(&mut bus);
    player.set_music(MusicState::Defeat);
    player.tick(&mut bus); // State changes deliberately spend one tick gate-low.
    for sid in 0..2 {
        for control in [4, 11] {
            assert_eq!(bus.registers[sid][control] & 1, 0);
        }
    }
    assert_eq!(music_ticks(&mut player, &mut bus, PHRASE_TICKS), opening);
    music_ticks(&mut player, &mut bus, PHRASE_TICKS * 5 + 19);
    player.reset(&mut bus);
    assert_eq!(music_ticks(&mut player, &mut bus, PHRASE_TICKS), opening);
}

#[test]
fn other_four_theme_register_traces_match_the_renamed_baseline() {
    // FNV-1a of every music register on every 50 Hz tick through the complete
    // eight-phrase score cycle, captured before the defeat-score extension.
    for (standard, expected) in [
        (
            VideoStandard::Pal,
            [
                0x9c3c_9656_30c4_d5cd,
                0xec3b_4b8e_3bf7_fee1,
                0x7424_4b46_c539_2bd5,
                0x7c66_17f4_aa21_bbed,
            ],
        ),
        (
            VideoStandard::Ntsc,
            [
                0x9475_b5d0_10c8_1fed,
                0x963b_1b69_6051_59f1,
                0x4050_8c27_ef14_36f5,
                0xd14e_5482_c3f6_3285,
            ],
        ),
    ] {
        for (state, expected_hash) in [
            MusicState::Exploration,
            MusicState::Combat,
            MusicState::Boss,
            MusicState::Victory,
        ]
        .into_iter()
        .zip(expected)
        {
            let mut player = AudioEngine::new(standard);
            player.load_score(&DEFAULT_SCORE).unwrap();
            player.set_music(state);
            let mut bus = Bus::default();
            player.reset(&mut bus);
            let mut hash = 14_695_981_039_346_656_037_u64;
            for _ in 0..16 * 8 * DEFAULT_SCORE[4 + state as usize] as usize {
                player.tick(&mut bus);
                for registers in music_registers(&bus) {
                    for value in registers {
                        hash ^= value as u64;
                        hash = hash.wrapping_mul(1_099_511_628_211);
                    }
                }
            }
            assert_eq!(hash, expected_hash, "{standard:?} {state:?}");
        }
    }
}
