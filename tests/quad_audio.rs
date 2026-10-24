use havoc::audio::{
    note_frequency, AudioEvent, Effect, EffectsEngine, SidBus, VideoStandard, EFFECT_COUNT,
};

#[derive(Clone, Default)]
struct Bus {
    regs: [[u8; 25]; 4],
    writes: Vec<(u8, u8, u8)>,
}
impl SidBus for Bus {
    fn write(&mut self, sid: u8, reg: u8, value: u8) {
        assert!(sid < 4 && reg < 25);
        self.regs[sid as usize][reg as usize] = value;
        self.writes.push((sid, reg, value));
    }
}
fn event(effect: Effect, pan: i8) -> AudioEvent {
    AudioEvent { effect, pan }
}
fn setup() -> (EffectsEngine, Bus) {
    let mut e = EffectsEngine::new(VideoStandard::Pal);
    let mut b = Bus::default();
    e.reset(&mut b);
    b.writes.clear();
    (e, b)
}
fn active(b: &Bus) -> u8 {
    b.regs.iter().enumerate().fold(0, |mask, (sid, r)| {
        mask | if [4, 11, 18].iter().any(|&reg| r[reg] & 1 != 0) {
            1 << sid
        } else {
            0
        }
    })
}
fn cutoff(b: &Bus, sid: usize) -> u16 {
    ((b.regs[sid][22] as u16) << 3) | (b.regs[sid][21] & 7) as u16
}

#[test]
fn engine_fits_dedicated_page_and_cue_ids_are_checked() {
    assert_eq!(size_of::<EffectsEngine>(), 200);
    assert_eq!(size_of::<AudioEvent>(), 2);
    for id in 0..=255 {
        assert_eq!(Effect::from_id(id).is_some(), id < (EFFECT_COUNT as u8));
    }
}
#[test]
fn authored_pitch_profile_layers_transients_bodies_and_subs() {
    // Approved MIDI starting notes per voice, independent of the compact
    // patch indices: transient (often the noise rate), tonal body and the
    // third layer (sub, shimmer or carrier).
    let notes = [
        [74, 64, 94], // Fire
        [62, 48, 83], // EnemyFire
        [76, 50, 67], // MissileLaunch
        [48, 60, 36], // EnemyHit
        [72, 76, 79], // Pickup
        [86, 55, 72], // Roll
        [36, 43, 76], // Bomb
        [55, 36, 31], // EnemyExplosion
        [36, 60, 31], // Footstep
        [91, 94, 31], // BirdCall
        [43, 50, 60], // Transport
        [43, 50, 60], // TransportAway
        [86, 66, 48], // Meteor
        [43, 55, 31], // BossHit
        [86, 71, 65], // PlayerHit
        [48, 55, 83], // BossExplosion
        [60, 66, 86], // PlayerDeath
        [86, 43, 48], // MissileImpact
        [36, 43, 55], // BossArrival
        [88, 85, 84], // MissileReady
        [71, 71, 86], // LowShield
    ];
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for (id, pitches) in notes.into_iter().enumerate() {
            let effect = Effect::from_id(id as u8).unwrap();
            for pan in [-100, 0, 100] {
                let mut engine = EffectsEngine::new(standard);
                let mut bus = Bus::default();
                engine.reset(&mut bus);
                engine.queue(event(effect, pan));
                engine.tick(&mut bus);
                let mut chips = 0;
                for sid in 0..4 {
                    if bus.regs[sid][5] == 0 {
                        continue;
                    }
                    chips += 1;
                    for (voice, note) in pitches.into_iter().enumerate() {
                        let register = voice * 7;
                        let actual = u16::from_le_bytes([
                            bus.regs[sid][register],
                            bus.regs[sid][register + 1],
                        ]);
                        let base = note_frequency(standard, note);
                        let expected = base + (base >> 9) * (sid as u16 & 1);
                        assert_eq!(
                            actual, expected,
                            "{standard:?}, {effect:?}, pan {pan}, SID {sid}, voice {voice}"
                        );
                    }
                }
                assert_eq!(
                    chips,
                    if pan == 0 || effect.is_cinematic() {
                        2
                    } else {
                        1
                    }
                );
            }
        }
    }
}
#[test]
fn reset_initializes_all_four_chips_and_uninitialized_ticks_are_silent() {
    let mut e = EffectsEngine::new(VideoStandard::Pal);
    let mut b = Bus::default();
    e.tick(&mut b);
    assert!(b.writes.is_empty());
    e.reset(&mut b);
    for sid in 0..4 {
        assert_eq!(b.regs[sid][24], 8);
        assert_eq!(b.regs[sid][23], 0);
    }
    assert_eq!(active(&b), 0);
}
#[test]
fn every_effect_plays_layers_sweeps_and_finishes_on_both_standards() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for id in 0..EFFECT_COUNT as u8 {
            let mut e = EffectsEngine::new(standard);
            let mut b = Bus::default();
            e.reset(&mut b);
            let effect = Effect::from_id(id).unwrap();
            e.queue(event(effect, 0));
            e.tick(&mut b);
            assert_eq!(active(&b), 0, "gate-low attack {effect:?}");
            let mut cutoffs = Vec::new();
            let mut sounding = false;
            let mut punched = false;
            for _ in 0..110 {
                e.tick(&mut b);
                sounding |= active(&b) != 0;
                cutoffs.push(cutoff(&b, 0));
                for sid in 0..4 {
                    // Chip volume rests at the mix level and may only rise
                    // for an authored punch.
                    let volume = b.regs[sid][24] & 15;
                    assert!((8..=15).contains(&volume), "{effect:?} volume {volume}");
                    punched |= volume > 8;
                }
            }
            assert!(sounding, "{effect:?}");
            assert!(
                cutoffs.windows(2).filter(|w| w[0] != w[1]).count() >= 2,
                "{effect:?}"
            );
            assert_eq!(active(&b), 0, "{effect:?}");
            assert!(b.regs.iter().all(|r| r[23] == 0 && r[24] == 8));
            assert_eq!(
                punched,
                !matches!(
                    effect,
                    Effect::Fire
                        | Effect::EnemyFire
                        | Effect::MissileLaunch
                        | Effect::Footstep
                        | Effect::BirdCall
                        | Effect::Transport
                        | Effect::TransportAway
                        | Effect::Meteor
                        | Effect::BossArrival
                ),
                "{effect:?} punch"
            );
        }
    }
}
#[test]
fn directional_sounds_use_only_the_requested_side_and_center_is_stereo() {
    for (pan, allowed, count) in [
        (-127, 5, 1),
        (127, 10, 1),
        (0, 15, 2),
        (-32, 15, 2),
        (32, 15, 2),
    ] {
        let (mut e, mut b) = setup();
        e.queue(event(Effect::MissileLaunch, pan));
        e.tick(&mut b);
        e.tick(&mut b);
        let mask = active(&b);
        assert_eq!(mask & !allowed, 0);
        assert_eq!(mask.count_ones(), count);
        if count == 2 {
            assert_ne!(mask & 5, 0);
            assert_ne!(mask & 10, 0);
        }
    }
}

#[test]
fn every_effect_uses_at_most_one_chip_per_side() {
    for id in 0..EFFECT_COUNT as u8 {
        let effect = Effect::from_id(id).unwrap();
        for pan in [-100, 0, 100] {
            let (mut engine, mut bus) = setup();
            engine.queue(event(effect, pan));
            engine.tick(&mut bus);
            engine.tick(&mut bus);
            let mask = active(&bus);
            assert!((mask & 5).count_ones() <= 1, "{effect:?}, pan {pan}");
            assert!((mask & 10).count_ones() <= 1, "{effect:?}, pan {pan}");
            assert_eq!(
                mask.count_ones(),
                if pan == 0 || effect.is_cinematic() {
                    2
                } else {
                    1
                }
            );
        }
    }
}

#[test]
fn backup_chips_preserve_each_instruments_channel_registers_and_writes() {
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for id in 0..EFFECT_COUNT as u8 {
            let effect = Effect::from_id(id).unwrap();
            for pan in [-100, 0, 100] {
                let mut primary = EffectsEngine::new(standard);
                let mut backup = EffectsEngine::new(standard);
                let mut primary_bus = Bus::default();
                let mut backup_bus = Bus::default();
                primary.reset(&mut primary_bus);
                backup.reset(&mut backup_bus);
                // Occupy the first pair, forcing this effect into free backup chips.
                backup.queue(event(Effect::Fire, 0));
                backup.tick(&mut backup_bus);
                backup.tick(&mut backup_bus);
                primary.queue(event(effect, pan));
                backup.queue(event(effect, pan));
                let lane = if effect.is_cinematic() {
                    3
                } else {
                    event(effect, pan).lane()
                };
                for tick in 0..110 {
                    primary_bus.writes.clear();
                    backup_bus.writes.clear();
                    primary.tick(&mut primary_bus);
                    backup.tick(&mut backup_bus);
                    for channel in 0..2 {
                        if lane & (1 << channel) != 0 {
                            assert_eq!(primary_bus.regs[channel], backup_bus.regs[channel + 2],
                                "{standard:?}, {effect:?}, pan {pan}, tick {tick}, channel {channel}");
                        }
                    }
                    let writes = |bus: &Bus, first: u8| {
                        bus.writes
                            .iter()
                            .filter(|&&(sid, _, _)| {
                                sid >= first && sid < first + 2 && lane & (1 << (sid & 1)) != 0
                            })
                            .map(|&(sid, reg, value)| (sid & 1, reg, value))
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(
                        writes(&primary_bus, 0),
                        writes(&backup_bus, 2),
                        "{standard:?}, {effect:?}, pan {pan}, tick {tick}: backup writes differ"
                    );
                }
            }
        }
    }
}

#[test]
fn bomb_and_both_sides_of_ship_destruction_sound_together() {
    for at in [0, 1, 10, 40] {
        let (mut engine, mut bus) = setup();
        let (mut reference, mut reference_bus) = setup();
        engine.queue(event(Effect::Bomb, 0));
        reference.queue(event(Effect::Bomb, 0));
        for tick in 0..at + 5 {
            if tick == at {
                engine.queue(event(Effect::EnemyExplosion, -100));
                engine.queue(event(Effect::EnemyExplosion, 100));
            }
            engine.tick(&mut bus);
            reference.tick(&mut reference_bus);
            assert_eq!(
                bus.regs[..2],
                reference_bus.regs[..2],
                "bomb interrupted at {at}/{tick}"
            );
            if tick > at {
                assert_eq!(
                    active(&bus),
                    15,
                    "one of the explosions is missing at {at}/{tick}"
                );
                // EnemyExplosion: resonance 11 with its noise and sub routed.
                assert_eq!(bus.regs[2][23], 0xb5);
                assert_eq!(bus.regs[3][23], 0xb5);
            }
        }
    }
}

#[test]
fn repeated_bomb_restarts_its_existing_pair_and_keeps_other_effects_running() {
    for backup_pair in [false, true] {
        let (mut engine, mut bus) = setup();
        if backup_pair {
            engine.queue(event(Effect::Fire, 0));
            engine.tick(&mut bus);
            engine.tick(&mut bus);
        }
        engine.queue(event(Effect::Bomb, 0));
        for _ in 0..10 {
            engine.tick(&mut bus);
        }
        let bomb_mask = active(&bus);
        assert_eq!(bomb_mask, if backup_pair { 12 } else { 3 });
        engine.queue(event(Effect::EnemyExplosion, -100));
        engine.queue(event(Effect::EnemyExplosion, 100));
        engine.tick(&mut bus);
        engine.tick(&mut bus);
        let mut reference = engine.snapshot();
        let mut reference_bus = bus.clone();
        engine.queue(event(Effect::Bomb, 0));
        engine.tick(&mut bus);
        reference.tick(&mut reference_bus);
        assert_eq!(
            active(&bus),
            15 ^ bomb_mask,
            "retrigger needs a gate-low tick"
        );
        for sid in 0..4 {
            if bomb_mask & (1 << sid) == 0 {
                assert_eq!(
                    bus.regs[sid], reference_bus.regs[sid],
                    "retrigger stole an explosion"
                );
            } else {
                assert_eq!(cutoff(&bus, sid), 2000, "bomb did not restart its envelope");
            }
        }
        engine.tick(&mut bus);
        assert_eq!(active(&bus), 15);
    }
}

#[test]
fn four_directional_instruments_can_sweep_simultaneously() {
    let (mut e, mut b) = setup();
    for (effect, pan) in [
        (Effect::MissileLaunch, -100),
        (Effect::Roll, -100),
        (Effect::Meteor, 100),
        (Effect::Transport, 100),
    ] {
        e.queue(event(effect, pan));
    }
    e.tick(&mut b);
    e.tick(&mut b);
    assert_eq!(active(&b), 15);
    let before = (0..4).map(|sid| cutoff(&b, sid)).collect::<Vec<_>>();
    for _ in 0..4 {
        e.tick(&mut b);
    }
    for sid in 0..4 {
        assert_ne!(cutoff(&b, sid), before[sid]);
    }
    assert_ne!(cutoff(&b, 0), cutoff(&b, 2));
    for r in &b.regs {
        assert_ne!(r[23] & 7, 0, "instrument needs a filtered layer");
        for reg in [4, 11, 18] {
            assert_ne!(r[reg] & 1, 0);
        }
    }
}
#[test]
fn hero_effects_use_six_oscillators_and_distinct_stereo_sweeps() {
    for effect in [Effect::Bomb, Effect::BossExplosion, Effect::PlayerDeath] {
        let (mut e, mut b) = setup();
        e.queue(event(effect, -100));
        e.tick(&mut b);
        for _ in 0..15 {
            e.tick(&mut b);
        }
        assert_eq!(active(&b), 3);
        assert_ne!(cutoff(&b, 0), cutoff(&b, 1));
        for r in &b.regs[..2] {
            for reg in [4, 11, 18] {
                assert_eq!(r[reg] & 1, 1);
            }
        }
    }
}
#[test]
fn mass_kills_use_the_spare_pair_without_replacing_the_bomb() {
    let (mut e, mut b) = setup();
    let (mut reference, mut reference_bus) = setup();
    e.queue(event(Effect::Bomb, 0));
    reference.queue(event(Effect::Bomb, 0));
    for pan in [-100, 0, 100] {
        e.queue(event(Effect::EnemyExplosion, pan));
        e.queue(event(Effect::Fire, pan));
    }
    e.tick(&mut b);
    e.tick(&mut b);
    reference.tick(&mut reference_bus);
    reference.tick(&mut reference_bus);
    assert_eq!(active(&b), 15);
    assert_eq!(b.regs[..2], reference_bus.regs[..2]);
    assert!(b.regs[2..].iter().all(|r| r[23] == 0xb5));
    for _ in 0..5 {
        e.queue(event(Effect::EnemyExplosion, 0));
        e.tick(&mut b);
        reference.tick(&mut reference_bus);
        assert_eq!(b.regs[..2], reference_bus.regs[..2]);
    }
    // Death takes the lower-priority explosion pair and leaves the bomb
    // intact; the bomb's own gate-low re-detonation at tick eight lands on
    // the second tick here, identically in both engines.
    e.queue(event(Effect::PlayerDeath, 0));
    e.tick(&mut b);
    reference.tick(&mut reference_bus);
    assert_eq!(active(&b) & 12, 0, "death needs its gate-low tick");
    assert_eq!(b.regs[..2], reference_bus.regs[..2]);
    e.tick(&mut b);
    reference.tick(&mut reference_bus);
    assert_eq!(active(&b) & 12, 12, "death pair did not attack");
    assert_eq!(b.regs[..2], reference_bus.regs[..2], "the existing bomb keeps its pair");
}
#[test]
fn preempting_half_a_stereo_effect_releases_its_other_half() {
    let (mut e, mut b) = setup();
    e.queue(event(Effect::Pickup, 0));
    e.tick(&mut b);
    e.tick(&mut b);
    let original = active(&b);
    e.queue(event(Effect::MissileLaunch, -100));
    e.queue(event(Effect::BossArrival, 100));
    e.tick(&mut b);
    e.tick(&mut b);
    assert_eq!(active(&b), 15);
    e.queue(event(Effect::PlayerHit, -100));
    e.tick(&mut b);
    assert_eq!(
        active(&b) & original,
        0,
        "both old stereo halves stop together"
    );
    e.tick(&mut b);
    assert_eq!(active(&b).count_ones(), 3);
}
#[test]
fn full_queue_keeps_urgent_cues_and_coalesces_duplicates() {
    let (mut e, mut b) = setup();
    for _ in 0..100 {
        assert!(e.queue(event(Effect::Fire, 0)));
    }
    for id in 1..=8 {
        e.queue(event(Effect::from_id(id).unwrap(), 100));
    }
    assert!(e.queue(event(Effect::PlayerDeath, 0)));
    e.tick(&mut b);
    e.tick(&mut b);
    assert_eq!(active(&b), 15);
}
#[test]
fn scene_cancellation_is_ram_only_and_leaves_combat_running() {
    let (mut e, mut b) = setup();
    e.queue(event(Effect::Meteor, -100));
    e.queue(event(Effect::MissileLaunch, 100));
    e.tick(&mut b);
    e.tick(&mut b);
    let right = active(&b) & 10;
    e.queue(event(Effect::Transport, 0));
    b.writes.clear();
    e.cancel_scene_effects();
    assert!(b.writes.is_empty());
    e.tick(&mut b);
    assert_eq!(active(&b), right);
    for sid in [0, 2] {
        assert_eq!(b.regs[sid][23], 0);
    }
}
#[test]
fn snapshot_restores_queue_filters_and_subsequent_writes_exactly() {
    let (mut e, mut b) = setup();
    e.queue(event(Effect::Roll, 0));
    for _ in 0..8 {
        e.tick(&mut b);
    }
    e.queue(event(Effect::MissileImpact, 100));
    let snapshot = e.snapshot();
    let mut restored = EffectsEngine::new(VideoStandard::Ntsc);
    restored.restore(&snapshot);
    let mut other = b.clone();
    for _ in 0..110 {
        b.writes.clear();
        other.writes.clear();
        e.tick(&mut b);
        restored.tick(&mut other);
        assert_eq!(e.image(), restored.image());
        assert_eq!(b.writes, other.writes);
        assert_eq!(b.regs, other.regs);
    }
}
#[test]
fn silence_clears_every_gate_filter_and_queued_event() {
    let (mut e, mut b) = setup();
    e.queue(event(Effect::Bomb, 0));
    for _ in 0..8 {
        e.tick(&mut b);
    }
    e.queue(event(Effect::MissileLaunch, 0));
    e.silence(&mut b);
    for _ in 0..8 {
        e.tick(&mut b);
        assert_eq!(active(&b), 0);
    }
    assert!(b.regs.iter().all(|r| r[23] == 0 && r[24] == 8));
}
#[test]
fn pal_and_ntsc_pitches_match_in_hertz() {
    for note in 24..=95 {
        let pal = note_frequency(VideoStandard::Pal, note) as f64 * 985248.0;
        let ntsc = note_frequency(VideoStandard::Ntsc, note) as f64 * 1022727.0;
        assert!((pal - ntsc).abs() / pal < 0.002);
    }
    assert_eq!(note_frequency(VideoStandard::Pal, 23), 0);
    assert_eq!(note_frequency(VideoStandard::Pal, 96), 0);
}

#[test]
fn damage_priorities_outrank_weapons_and_scenery_without_becoming_hero_effects() {
    for (effect, expected) in [
        (Effect::Fire, 0),
        (Effect::EnemyFire, 0),
        (Effect::Meteor, 1),
        (Effect::EnemyExplosion, 2),
        (Effect::Pickup, 2),
        (Effect::Roll, 2),
        (Effect::MissileReady, 2),
        (Effect::MissileLaunch, 3),
        (Effect::MissileImpact, 3),
        (Effect::BossArrival, 3),
        (Effect::EnemyHit, 4),
        (Effect::BossHit, 4),
        (Effect::PlayerHit, 5),
        (Effect::LowShield, 5),
        (Effect::Bomb, 6),
        (Effect::BossExplosion, 7),
        (Effect::PlayerDeath, 8),
    ] {
        assert_eq!(effect.priority(), expected, "{effect:?}");
    }
    for damage in [
        Effect::EnemyHit,
        Effect::BossHit,
        Effect::PlayerHit,
        Effect::LowShield,
    ] {
        let (mut e, mut b) = setup();
        e.queue(event(damage, 0));
        e.tick(&mut b);
        e.tick(&mut b);
        assert_eq!(active(&b).count_ones(), 2, "{damage:?} needs a stereo pair");
        for (sid, registers) in b.regs.iter().enumerate() {
            if active(&b) & (1 << sid) != 0 {
                assert_ne!(registers[4] & 1, 0, "dry attack must be gated");
                assert_eq!(registers[23] & 1, 0, "attack must bypass the filter");
                assert_ne!(registers[23] & 6, 0, "retain a sweeping body");
            }
        }
    }
}

#[test]
fn damage_uses_the_spare_cinematic_pair_even_on_the_activation_tick() {
    for hero in [Effect::Bomb, Effect::BossExplosion] {
        for damage in [
            Effect::EnemyHit,
            Effect::BossHit,
            Effect::PlayerHit,
            Effect::LowShield,
        ] {
            for at in [0, 1, 10, 40] {
                let (mut engine, mut bus) = setup();
                let (mut reference, mut reference_bus) = setup();
                engine.queue(event(hero, 0));
                reference.queue(event(hero, 0));
                for tick in 0..=at + 2 {
                    if tick == at {
                        engine.queue(event(damage, 0));
                    }
                    engine.tick(&mut bus);
                    reference.tick(&mut reference_bus);
                    assert_eq!(
                        bus.regs[..2],
                        reference_bus.regs[..2],
                        "{hero:?}, {damage:?}, start {at}, tick {tick}: primary changed"
                    );
                    if tick == at + 1 {
                        assert_eq!(active(&bus) & 12, 12, "{damage:?} did not attack");
                        assert_ne!(
                            bus.regs[2..],
                            reference_bus.regs[2..],
                            "secondary pair still sounds only {hero:?}"
                        );
                    }
                }
                // A busy low-priority queue cannot erase that new damage cue.
                let mut clean = engine.snapshot();
                let mut clean_bus = bus.clone();
                for effect in [
                    Effect::Fire,
                    Effect::EnemyFire,
                    Effect::Transport,
                    Effect::Meteor,
                    Effect::EnemyExplosion,
                ] {
                    engine.queue(event(effect, 0));
                }
                engine.tick(&mut bus);
                clean.tick(&mut clean_bus);
                assert_eq!(
                    bus.regs, clean_bus.regs,
                    "{damage:?} was stolen by scenery/fire"
                );
            }
        }
    }
}

#[test]
fn player_damage_wins_a_shared_impact_batch_and_death_takes_only_one_pair() {
    let (mut engine, mut bus) = setup();
    engine.queue(event(Effect::Bomb, 0));
    engine.queue(event(Effect::EnemyHit, 0));
    engine.queue(event(Effect::PlayerHit, 0));
    engine.tick(&mut bus);
    engine.tick(&mut bus);
    let (mut reference, mut reference_bus) = setup();
    reference.queue(event(Effect::Bomb, 0));
    reference.queue(event(Effect::PlayerHit, 0));
    reference.tick(&mut reference_bus);
    reference.tick(&mut reference_bus);
    assert_eq!(bus.regs, reference_bus.regs);
    engine.queue(event(Effect::PlayerDeath, 0));
    engine.queue(event(Effect::PlayerHit, 0));
    engine.queue(event(Effect::BossHit, 0));
    engine.tick(&mut bus);
    assert_eq!(active(&bus), 3, "the existing bomb keeps its pair");
    engine.tick(&mut bus);
    assert_eq!(active(&bus), 15);
}

#[test]
fn repeated_laser_has_a_tonal_attack_and_cannot_erase_damage() {
    let (mut engine, mut bus) = setup();
    for tick in 0..80 {
        if tick % 5 == 0 {
            engine.queue(event(Effect::Fire, 0));
        }
        engine.tick(&mut bus);
        if tick % 5 == 1 {
            assert_ne!(active(&bus), 0, "laser attack {tick}");
            assert!(
                bus.regs
                    .iter()
                    .filter(|r| r[4] & 1 != 0)
                    .all(|r| r[4] & 0xf0 != 0x80),
                "laser attack became noise"
            );
        }
    }
    engine.queue(event(Effect::EnemyHit, 0));
    engine.tick(&mut bus);
    let mut clean = engine.snapshot();
    let mut clean_bus = bus.clone();
    engine.queue(event(Effect::Fire, 0));
    engine.queue(event(Effect::EnemyFire, 0));
    engine.tick(&mut bus);
    clean.tick(&mut clean_bus);
    // Other chips may still take laser voices; compare the newly audible dry hit chips.
    let damage_mask = clean_bus.regs.iter().enumerate().fold(0, |mask, (sid, r)| {
        mask | if r[4] & 1 != 0 && r[23] & 1 == 0 {
            1 << sid
        } else {
            0
        }
    });
    assert_ne!(damage_mask, 0);
    for sid in 0..4 {
        if damage_mask & (1 << sid) != 0 {
            assert_eq!(bus.regs[sid], clean_bus.regs[sid]);
        }
    }
}

#[test]
fn cinematic_effects_preserve_the_frozen_core_pair_register_and_write_streams() {
    // The v17 cinematic streams, including every ordered write, punch ramp,
    // re-detonation, release and silence over all 106 ticks. The bomb's
    // previously removed doubled pair is excluded from this contract.
    fn hash(h: &mut u64, bytes: impl IntoIterator<Item = u8>) {
        for byte in bytes {
            *h = (*h ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    for (standard, effect, expected) in [
        (VideoStandard::Pal, Effect::Bomb, 0x066fc43e7a6e91deu64),
        (VideoStandard::Ntsc, Effect::Bomb, 0x84107f9237f0d98fu64),
        (
            VideoStandard::Pal,
            Effect::BossExplosion,
            0xe1d82c71e76d41b3,
        ),
        (
            VideoStandard::Ntsc,
            Effect::BossExplosion,
            0x3acc61211e7369e2,
        ),
        (VideoStandard::Pal, Effect::PlayerDeath, 0xd39b4bcf0dd7141e),
        (VideoStandard::Ntsc, Effect::PlayerDeath, 0xa63738647f32b4ad),
    ] {
        for pan in [-100, 0, 100] {
            let mut engine = EffectsEngine::new(standard);
            let mut bus = Bus::default();
            engine.reset(&mut bus);
            engine.queue(event(effect, pan));
            let mut digest = 0xcbf29ce484222325;
            for tick in 0u8..106 {
                bus.writes.clear();
                engine.tick(&mut bus);
                hash(&mut digest, [tick]);
                hash(&mut digest, (bus.writes.len() as u16 * 3).to_le_bytes());
                hash(
                    &mut digest,
                    bus.writes
                        .iter()
                        .flat_map(|&(sid, reg, value)| [sid, reg, value]),
                );
                hash(&mut digest, bus.regs[..2].iter().flatten().copied());
            }
            assert_eq!(digest, expected, "{standard:?}, {effect:?}, pan {pan}");
        }
    }
}

#[test]
fn programs_reach_their_authored_stages() {
    // Every program path the v17 kernel implements, observed through the
    // chip registers: pitch motion, late waveforms, gate retriggers, the
    // octave and arpeggio accents and the volume punch ramp.
    fn pitches(bus: &Bus) -> [u16; 3] {
        [0, 7, 14].map(|r| u16::from_le_bytes([bus.regs[0][r], bus.regs[0][r + 1]]))
    }
    fn gates(bus: &Bus) -> [bool; 3] {
        [4, 11, 18].map(|r| bus.regs[0][r] & 1 != 0)
    }
    // Pitch motion on every voice that has a slide or an accent.
    for (effect, moving) in [
        (Effect::Fire, [true, false, false]),
        (Effect::EnemyFire, [true, true, false]),
        (Effect::MissileLaunch, [false, true, true]),
        (Effect::EnemyHit, [true, false, false]),
        (Effect::Pickup, [true, true, true]),
        (Effect::Roll, [false, true, true]),
        (Effect::Bomb, [true, true, true]),
        (Effect::LowShield, [true, true, true]),
        (Effect::BirdCall, [true, true, true]),
    ] {
        let (mut engine, mut bus) = setup();
        engine.queue(event(effect, -100));
        engine.tick(&mut bus);
        let initial = pitches(&bus);
        let mut changed = [false; 3];
        for _ in 0..12 {
            engine.tick(&mut bus);
            for voice in 0..3 {
                changed[voice] |= pitches(&bus)[voice] != initial[voice];
            }
        }
        assert_eq!(changed, moving, "{effect:?} pitch motion");
    }
    // The rise program turns its pulse into a sawtooth at tick 14.
    for effect in [Effect::Roll, Effect::MissileLaunch] {
        let (mut engine, mut bus) = setup();
        engine.queue(event(effect, -100));
        for tick in 0..=16 {
            engine.tick(&mut bus);
            let wave = bus.regs[0][11] & 0xf0;
            assert_eq!(wave, if tick < 14 { 64 } else { 32 }, "{effect:?} tick {tick}");
        }
    }
    // Retriggers hold every gate low for exactly their authored tick.
    for (effect, retrigger) in [
        (Effect::EnemyFire, 5),
        (Effect::EnemyExplosion, 5),
        (Effect::Pickup, 6),
        (Effect::LowShield, 8),
        (Effect::Bomb, 8),
        (Effect::BirdCall, 7),
    ] {
        let (mut engine, mut bus) = setup();
        engine.queue(event(effect, -100));
        for tick in 0..=retrigger + 1 {
            engine.tick(&mut bus);
            let expected = tick != 0 && tick != retrigger;
            assert_eq!(gates(&bus), [expected; 3], "{effect:?} gate at tick {tick}");
        }
    }
    // The stereo right explosion re-blasts one tick after the left.
    {
        let (mut engine, mut bus) = setup();
        engine.queue(event(Effect::EnemyExplosion, 0));
        for tick in 0..=7 {
            engine.tick(&mut bus);
            let left = bus.regs[0][4] & 1 != 0;
            let right = bus.regs[1][4] & 1 != 0;
            assert_eq!((left, right), (tick != 0 && tick != 5, tick != 0 && tick != 6), "tick {tick}");
        }
    }
    // The siren renders an octave lower on alternate four-tick groups.
    {
        let (mut engine, mut bus) = setup();
        engine.queue(event(Effect::LowShield, -100));
        engine.tick(&mut bus);
        let high = pitches(&bus)[0];
        for tick in 1..=12 {
            engine.tick(&mut bus);
            let expected = if tick & 4 != 0 { high >> 1 } else { high };
            assert_eq!(pitches(&bus)[0], expected, "siren tick {tick}");
        }
    }
    // The chime climbs an eighth every fourth tick.
    {
        let (mut engine, mut bus) = setup();
        engine.queue(event(Effect::Pickup, -100));
        engine.tick(&mut bus);
        let mut expected = pitches(&bus);
        for tick in 1..=12 {
            engine.tick(&mut bus);
            if tick & 3 == 0 {
                expected = expected.map(|f| f + (f >> 3));
            }
            assert_eq!(pitches(&bus), expected, "chime tick {tick}");
        }
    }
    // Punch: full volume for the authored ticks, then one step down per tick.
    for (effect, punch) in [(Effect::Bomb, 3), (Effect::EnemyHit, 2), (Effect::BossHit, 3)] {
        let (mut engine, mut bus) = setup();
        engine.queue(event(effect, -100));
        for tick in 0..=punch + 8 {
            engine.tick(&mut bus);
            let expected = if tick < punch { 15 } else { (15 - (tick - punch)).max(8) };
            assert_eq!(bus.regs[0][24] & 15, expected, "{effect:?} volume at tick {tick}");
        }
    }
    // A fired cannon stays dry on its transient and body; only the carrier
    // passes through the filter.
    let (mut engine, mut bus) = setup();
    engine.queue(event(Effect::Fire, -100));
    engine.tick(&mut bus);
    assert_eq!(bus.regs[0][23] & 7, 2);
    assert_eq!(bus.regs[0][4] & 0xfe, 64);
    assert_eq!(bus.regs[0][11] & 0xfe, 64 | 2);
    assert_eq!(bus.regs[0][18] & 0xfe, 128);
}