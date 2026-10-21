#![cfg(feature = "host-tools")]

use havoc::{
    assets,
    music::{self, BANK_HEADER_SIZE, EVENT_OFFSET, INSTRUMENT_OFFSET, MUSIC_VOICES, PAGE_SIZE},
    soundtrack::{self, HOLD, ROWS_PER_BAR, VOICES},
};
use std::collections::BTreeSet;

#[test]
fn suite_has_the_authored_forms_and_independent_clock_durations() {
    let suite = soundtrack::cues();
    assert_eq!(suite.len(), 21);
    let expected = [
        (32, 6, 3072),
        (32, 6, 3072),
        (40, 5, 3200),
        (40, 5, 3200),
        (48, 5, 3840),
        (24, 7, 2688),
        (16, 9, 2304),
        (32, 6, 3072),
        (32, 6, 3072),
        (32, 6, 3072),
        (32, 5, 2560),
        (32, 5, 2560),
        (32, 5, 2560),
        (32, 5, 2560),
        (32, 6, 3072),
        (32, 7, 3584),
        (32, 5, 2560),
        (32, 5, 2560),
        (32, 6, 3072),
        (5, 5, 400),
        (32, 5, 2560),
    ];
    let mut names = BTreeSet::new();
    for (cue, (bars, ticks, duration)) in suite.iter().zip(expected) {
        assert!(names.insert(cue.name));
        assert_eq!(cue.bars.len(), bars, "{} form", cue.name);
        assert_eq!(cue.row_ticks, ticks, "{} tempo", cue.name);
        assert_eq!(cue.duration_ticks(), duration, "{} duration", cue.name);
        assert!(
            (24..=95).contains(&cue.bars[0].rows[0][1]),
            "the hook starts immediately"
        );
    }
    assert!(suite[5].bars.last().unwrap().rows[12..]
        .iter()
        .flatten()
        .all(|&x| x == 0));
    assert!(suite[6].bars.last().unwrap().rows[12..]
        .iter()
        .flatten()
        .all(|&x| x == 0));
}

#[test]
fn bank_round_trips_all_cues_and_only_strips_optional_orchestration() {
    let bank = soundtrack::score_bank();
    let suite = soundtrack::cues();
    assert_eq!(assets::u32_at(&bank, 8) as usize, bank.len());
    assert!(bank[16 + music::CUE_COUNT * 8..BANK_HEADER_SIZE]
        .iter()
        .all(|&x| x == 0));
    assert_eq!(
        bank.len(),
        if cfg!(feature = "four-sid") {
            146_528
        } else {
            62_944
        }
    );
    assert!(bank.len() <= music::BANK_CAPACITY as usize);
    let mut expected_offset = BANK_HEADER_SIZE;
    for (index, cue) in suite.iter().enumerate() {
        let info = music::cue_info(&bank[..BANK_HEADER_SIZE], index as u8).unwrap();
        assert_eq!(info.offset as usize, expected_offset);
        assert_eq!(info.pages as usize, cue.bars.len());
        assert_eq!(info.row_ticks, cue.row_ticks);
        let end = expected_offset + info.pages as usize * PAGE_SIZE;
        assert_eq!(info.checksum, assets::checksum(&bank[expected_offset..end]));
        for (page_index, (page, authored)) in bank[expected_offset..end]
            .chunks_exact(PAGE_SIZE)
            .zip(&cue.bars)
            .enumerate()
        {
            music::validate_page(page, index as u8, page_index as u8).unwrap();
            assert_eq!(page[6] as usize, (page_index + 1) % cue.bars.len());
            assert_eq!(page[7], cue.row_ticks);
            assert_eq!(page[music::FILTER_OFFSET], authored.filter);
            for voice in 0..MUSIC_VOICES {
                assert_eq!(
                    &page[INSTRUMENT_OFFSET + voice * 5..INSTRUMENT_OFFSET + (voice + 1) * 5],
                    &authored.instruments[voice]
                );
            }
            for row in 0..ROWS_PER_BAR {
                assert_eq!(
                    &page[EVENT_OFFSET + row * MUSIC_VOICES
                        ..EVENT_OFFSET + (row + 1) * MUSIC_VOICES],
                    &authored.rows[row][..MUSIC_VOICES]
                );
            }
        }
        expected_offset = end;
    }
    assert_eq!(expected_offset, bank.len(), "no unowned score bytes");
}

#[test]
fn every_cue_payload_keeps_its_published_checksums() {
    // Lost Wing retains its pre-expansion SMU2 payload in both profiles; the
    // other twenty cues carry the produced score. Any change to a note,
    // instrument, filter target or page link must be a deliberate republish.
    let expected = if cfg!(feature = "four-sid") {
        [
            38531, 512, 64174, 35058, 39356, 33967, 25190, 59886, 19350, 48525, 3712, 59866,
            10096, 12466, 35448, 40978, 42760, 37503, 47833, 8652, 15101,
        ]
    } else {
        [
            4100, 55587, 62212, 23317, 52142, 53213, 46112, 61879, 49160, 6697, 11474, 48593,
            41170, 48867, 33569, 57674, 61441, 64467, 32115, 1671, 48869,
        ]
    };
    let bank = soundtrack::score_bank();
    for (cue, checksum) in expected.into_iter().enumerate() {
        assert_eq!(
            music::cue_info(&bank, cue as u8).unwrap().checksum,
            checksum,
            "cue {cue}"
        );
    }
    let lost_wing = if cfg!(feature = "four-sid") { 25190 } else { 46112 };
    assert_eq!(expected[havoc::audio::MusicState::Defeat as usize], lost_wing);
}

fn echo_delay(cue: &soundtrack::Cue) -> usize {
    if cue.row_ticks > 5 {
        2
    } else {
        3
    }
}

fn produced(index: usize) -> bool {
    index >= havoc::audio::MusicState::CrystalCurrent as usize
}

#[test]
fn every_cue_opens_its_loop_without_holds_or_slurs() {
    // Page zero must publish on a cue change, which rejects any voice that
    // starts the loop with a hold or legato, including echoes of the last bar.
    for cue in soundtrack::cues() {
        for (voice, &event) in cue.bars[0].rows[0].iter().enumerate() {
            assert!(
                event != HOLD && event & 0x80 == 0,
                "{} voice{voice} opens with a tie",
                cue.name
            );
        }
    }
}

#[test]
fn produced_cues_echo_the_lead_on_the_opposite_sid_voice() {
    for (index, cue) in soundtrack::cues().into_iter().enumerate() {
        let lead: Vec<u8> = cue
            .bars
            .iter()
            .flat_map(|bar| bar.rows.iter().map(|row| row[1]))
            .collect();
        let echo: Vec<u8> = cue
            .bars
            .iter()
            .flat_map(|bar| bar.rows.iter().map(|row| row[6]))
            .collect();
        if !produced(index) {
            assert!(
                cue.bars.iter().all(|bar| bar.instruments[6] == [0x10, 0x00, 0x08, 0x24, 0x43]),
                "{} keeps its cello counterline",
                cue.name
            );
            continue;
        }
        let delay = echo_delay(&cue);
        assert!(
            cue.bars.iter().all(|bar| bar.instruments[6] == [0x40, 0x00, 0x06, 0x02, 0x34]),
            "{} echo instrument",
            cue.name
        );
        for at in 0..lead.len() {
            let expected = if at < delay { 0 } else { lead[at - delay] };
            assert_eq!(echo[at], expected, "{} echo row {at}", cue.name);
        }
    }
}

#[test]
fn space_drums_have_kick_bodies_falling_snares_and_phrase_rolls() {
    use havoc::audio::MusicState;
    let suite = soundtrack::cues();
    for cue in &suite[MusicState::AsteroidBelt as usize..] {
        for (index, bar) in cue.bars.iter().enumerate() {
            let drums = &bar.instruments[2];
            assert_eq!(drums[0], 0x80, "{} noise kit", cue.name);
            assert_eq!(drums[4] & 0xf0, 0, "{} drums decay to silence", cue.name);
            assert!(drums[3] >= 4, "{} drums have a real decay", cue.name);
            // Snares slur down a fifth on the following sixteenth.
            for at in [4, 12] {
                let snare = bar.rows[at][2];
                assert!(snare > HOLD && snare & 0x80 == 0, "{} bar {index} snare", cue.name);
                if index % 8 != 7 || at == 4 {
                    assert_eq!(bar.rows[at + 1][2], (snare - 7) | 0x80, "{} bar {index}", cue.name);
                }
            }
            if index % 8 == 7 {
                let roll: Vec<u8> = bar.rows[12..].iter().map(|row| row[2]).collect();
                assert_eq!(roll, [64, 66, 68, 70], "{} bar {index} roll", cue.name);
            }
            // The tuned sub-octave kick body sits under both kicks.
            assert_eq!(bar.instruments[8], [0x40, 0x00, 0x08, 0x04, 0x04]);
            for at in [0, 8] {
                let body = bar.rows[at][8];
                assert!((24..=35).contains(&body), "{} bar {index} kick body {body}", cue.name);
                assert_eq!(bar.rows[at + 1][8], 0);
            }
        }
    }
}

#[test]
fn pulse_widths_step_through_each_phrase_and_strings_swell_into_reprises() {
    let steps = [0u16, 0x80, 0x100, 0x180, 0x200, 0x180, 0x100, 0x80];
    let width = |instrument: &[u8; 5]| u16::from(instrument[2]) << 8 | u16::from(instrument[1]);
    for (index, cue) in soundtrack::cues().into_iter().enumerate() {
        if index == havoc::audio::MusicState::Defeat as usize {
            continue;
        }
        for (bar_index, bar) in cue.bars.iter().enumerate() {
            let base_bass = width(&bar.instruments[0]) - steps[bar_index % 8];
            assert!(
                base_bass == 0x300 || base_bass == 0x400,
                "{} bar {bar_index} bass width",
                cue.name
            );
            if produced(index) {
                assert_eq!(bar.instruments[1][0], 0x40);
                let first = width(&cue.bars[bar_index / 8 * 8].instruments[1]);
                assert_eq!(width(&bar.instruments[1]), first + steps[bar_index % 8], "{} bar {bar_index}", cue.name);
            }
        }
        if produced(index) && cue.bars.len() == 32 {
            for section in [1, 3] {
                let sustain: Vec<u8> = cue.bars[section * 8..section * 8 + 8]
                    .iter()
                    .map(|bar| bar.instruments[4][4] >> 4)
                    .collect();
                assert_eq!(sustain, [3, 3, 4, 4, 5, 5, 6, 6], "{} section {section}", cue.name);
            }
            for bar in &cue.bars[22..24] {
                for voice in [3, 4, 5, 7] {
                    assert!(bar.rows.iter().all(|row| row[voice] == 0), "{} breakdown", cue.name);
                }
                assert!(bar.rows.iter().any(|row| row[0] > HOLD));
                assert!(bar.rows.iter().any(|row| row[1] > HOLD));
            }
            for section_start in [0, 8, 16, 24] {
                let bar = &cue.bars[section_start];
                assert_eq!(bar.instruments[9], [0x80, 0x00, 0x08, 0x09, 0x08]);
                assert_eq!(bar.rows[0][9], 80);
                assert_eq!(&[bar.rows[1][9], bar.rows[2][9], bar.rows[3][9]], &[HOLD; 3]);
            }
        }
    }
}

#[test]
fn produced_bass_lines_walk_chromatically_into_chord_changes() {
    for (index, cue) in soundtrack::cues().into_iter().enumerate() {
        let bars = cue.bars.len();
        let mut approaches = 0;
        for (bar_index, bar) in cue.bars.iter().enumerate() {
            let next = cue.bars[(bar_index + 1) % bars].rows[0][0];
            let walk = bar.rows[15][0];
            if !produced(index) {
                assert!(walk <= HOLD, "{} has no bass approach", cue.name);
                continue;
            }
            assert!(next > HOLD && next & 0x80 == 0, "{} bar {bar_index} bass re-attacks", cue.name);
            if walk > HOLD {
                assert_eq!((walk as i16 - next as i16).abs(), 1, "{} bar {bar_index} approach", cue.name);
                approaches += 1;
            }
        }
        if produced(index) {
            assert!(approaches >= bars / 2, "{} walks into its changes", cue.name);
        }
    }
}

#[test]
fn ring_modulation_colours_only_the_extra_voices_of_four_cues() {
    use havoc::audio::MusicState as M;
    for (index, cue) in soundtrack::cues().into_iter().enumerate() {
        let state = M::from_cue(index as u8).unwrap();
        let coloured = matches!(
            state,
            M::ShatteredMoon | M::JumpGate | M::AshesAndSteel | M::OrbitalShipyard
        );
        for bar in &cue.bars {
            for (voice, instrument) in bar.instruments.iter().enumerate() {
                assert!(music::valid_control(instrument[0]), "{} voice{voice}", cue.name);
                if voice < 4 {
                    assert_eq!(instrument[0] & 0x0f, 0, "{} core voice{voice} is plain", cue.name);
                }
            }
            assert_eq!(
                bar.instruments[7][0] == 0x10 | music::RING,
                coloured,
                "{} ring modulation",
                cue.name
            );
        }
        if matches!(state, M::AshesAndSteel | M::OrbitalShipyard) {
            assert!(
                cue.bars.iter().filter(|bar| bar.rows[4][7] == 93 && bar.rows[12][7] == 93).count() >= 24,
                "{} anvil backbeats",
                cue.name
            );
        }
    }
}

#[test]
fn filter_programme_opens_through_the_first_eight_and_dips_before_each_reprise() {
    use havoc::audio::MusicState as M;
    for (index, cue) in soundtrack::cues().into_iter().enumerate() {
        let targets: Vec<u8> = cue.bars.iter().map(|bar| bar.filter).collect();
        match M::from_cue(index as u8).unwrap() {
            M::Defeat => assert!(targets.iter().all(|&t| t == 0), "Lost Wing is untouched"),
            M::Ascent => assert!(targets.windows(2).all(|w| w[0] < w[1]), "the ascent opens"),
            state if produced(index) => {
                assert!(targets.iter().all(|&t| t != 0));
                assert!(targets[..8].windows(2).all(|w| w[0] < w[1]), "{} opens", cue.name);
                let dip = *targets[16..24].iter().min().unwrap();
                assert!(dip < targets[8] && dip < targets[24], "{} dips", cue.name);
                assert_eq!(targets[24], 0xd0, "{} snaps open at the reprise", cue.name);
                let pumping = targets[8..16].windows(2).filter(|w| w[0] != w[1]).count();
                if matches!(
                    state,
                    M::AshesAndSteel | M::GorgeWarden | M::CitadelHeart | M::OrbitalShipyard | M::SpaceCarrier | M::GateGuardian
                ) {
                    assert!(targets[9] - targets[8] >= 0x40, "{} pumps", cue.name);
                }
                assert!(pumping >= 7);
            }
            _ => assert!(
                targets.iter().all(|&t| (0x70..=0x98).contains(&t)),
                "{} breathes gently",
                cue.name
            ),
        }
    }
}

#[test]
fn space_suite_has_distinct_hooks_and_core_percussion_with_a_breathing_middle_eight() {
    use havoc::audio::MusicState;
    let suite = soundtrack::cues();
    let mut hooks = BTreeSet::new();
    for cue in &suite[MusicState::AsteroidBelt as usize..] {
        let hook: Vec<_> = cue.bars[..4]
            .iter()
            .flat_map(|bar| bar.rows.iter().map(|row| row[1]))
            .collect();
        assert!(hooks.insert(hook), "{} reuses another space hook", cue.name);
        assert!(
            cue.bars.iter().all(|bar| bar.instruments[2][0] == 0x80),
            "{} loses its core percussion in the two-SID profile",
            cue.name
        );
        assert!(
            cue.bars
                .iter()
                .all(|bar| bar.rows[4][2] > HOLD && bar.rows[12][2] > HOLD),
            "{} needs audible snare backbeats",
            cue.name
        );
        if cue.bars.len() == 32 {
            let attacks = |section: usize| {
                cue.bars[section * 8..section * 8 + 8]
                    .iter()
                    .flat_map(|bar| &bar.rows)
                    .filter(|row| row[2] > HOLD)
                    .count()
            };
            assert!(
                attacks(2) < attacks(0),
                "{} middle eight does not breathe",
                cue.name
            );
        }
    }
    let ascent = &suite[MusicState::Ascent as usize];
    assert_eq!(ascent.duration_ticks(), 8 * 50);
    assert!(ascent.bars[4].rows[0][1] > ascent.bars[0].rows[0][1]);
}

#[test]
fn all_parts_have_valid_articulation_across_page_boundaries_and_loops() {
    let mut enhanced_attacks = [0usize; 6];
    for cue in soundtrack::cues() {
        let mut gated = [false; VOICES];
        let mut previous_instruments = cue.bars[0].instruments;
        // The second traversal checks the actual looping boundary too.
        for bar in cue.bars.iter().chain(&cue.bars) {
            for voice in 0..VOICES {
                let first = bar.rows[0][voice];
                if previous_instruments[voice] != bar.instruments[voice] {
                    assert!(
                        first != HOLD && first & 0x80 == 0,
                        "{} changes a held instrument",
                        cue.name
                    );
                }
            }
            for row in &bar.rows {
                for (voice, &event) in row.iter().enumerate() {
                    match event {
                        0 => gated[voice] = false,
                        HOLD => assert!(gated[voice], "{} voice{voice} holds a rest", cue.name),
                        _ => {
                            assert!((24..=95).contains(&(event & 0x7f)));
                            if event & 0x80 != 0 {
                                assert!(
                                    gated[voice],
                                    "{} voice{voice} slurs from silence",
                                    cue.name
                                );
                            }
                            gated[voice] = true;
                            if voice >= 4 {
                                enhanced_attacks[voice - 4] += 1;
                            }
                        }
                    }
                }
            }
            previous_instruments = bar.instruments;
        }
    }
    assert!(enhanced_attacks.into_iter().all(|attacks| attacks > 0));
}

#[test]
fn developed_sections_are_not_whole_phrase_transpositions() {
    for (index, cue) in soundtrack::cues().into_iter().enumerate() {
        if index == havoc::audio::MusicState::Defeat as usize
            || index == havoc::audio::MusicState::Ascent as usize
        {
            continue;
        }
        let mut shapes = BTreeSet::new();
        let mut held_lead_rows = 0;
        for section in cue.bars.chunks_exact(8) {
            let lead: Vec<_> = section
                .iter()
                .flat_map(|bar| bar.rows.iter().map(|row| row[1]))
                .collect();
            let first = lead.iter().copied().find(|&event| event > HOLD).unwrap() & 0x7f;
            let shape: Vec<i16> = lead
                .into_iter()
                .map(|event| match event {
                    0 => -200,
                    HOLD => {
                        held_lead_rows += 1;
                        -100
                    }
                    _ => (event & 0x7f) as i16 - first as i16,
                })
                .collect();
            assert!(
                shapes.insert(shape),
                "{} repeats a transposed eight-bar melody",
                cue.name
            );
        }
        assert!(shapes.len() >= 3, "{} needs a contrasting form", cue.name);
        assert!(
            held_lead_rows >= 64,
            "{} needs room for its melody to sing",
            cue.name
        );
    }
}

#[test]
fn campaign_tracks_keep_a_complete_core_and_six_complementary_parts() {
    for cue in soundtrack::cues().into_iter().skip(7) {
        for voice in 0..VOICES {
            assert!(
                cue.bars
                    .iter()
                    .flat_map(|bar| &bar.rows)
                    .any(|row| row[voice] > HOLD),
                "{} voice{voice} never enters",
                cue.name
            );
        }
        assert!(
            cue.bars.iter().all(|bar| bar.instruments[1][0] == 0x40),
            "{} needs its pulse lead in both SID profiles",
            cue.name
        );
        assert!(
            cue.bars.iter().any(|bar| bar
                .rows
                .iter()
                .enumerate()
                .any(|(row, event)| row & 1 == 1 && event[0] > HOLD)),
            "{} needs a syncopated core bass",
            cue.name
        );
        assert!(
            cue.bars
                .iter()
                .any(|bar| bar.rows.iter().all(|row| row[3] > HOLD)),
            "{} needs its fast core arpeggio",
            cue.name
        );
    }
}

#[test]
fn lost_wing_preserves_every_original_core_note_and_breath() {
    // Independent transcription of the previous asset opening plus its seven
    // resident continuation phrases; this catches accidental rewriting while
    // moving the piece from18-tick rows to9-tick sixteenths.
    let original = [
        [40, 71, 55, 64],
        [0, 69, 0, 62],
        [36, 67, 52, 60],
        [0, 66, 0, 59],
        [33, 64, 48, 57],
        [0, 62, 0, 55],
        [35, 59, 50, 54],
        [40, 64, 55, 59],
        [36, 67, 52, 60],
        [43, 69, 55, 64],
        [43, 71, 59, 67],
        [0, 74, 0, 71],
        [38, 72, 54, 62],
        [45, 69, 57, 66],
        [40, 67, 55, 59],
        [0, 64, 0, 0],
        [33, 64, 48, 57],
        [40, 60, 52, 64],
        [40, 59, 55, 67],
        [0, 62, 0, 59],
        [36, 64, 52, 60],
        [43, 67, 55, 64],
        [35, 66, 51, 57],
        [0, 63, 0, 59],
        [40, 67, 59, 64],
        [47, 71, 55, 67],
        [43, 76, 59, 71],
        [0, 74, 0, 67],
        [33, 72, 52, 69],
        [40, 71, 57, 64],
        [35, 69, 51, 66],
        [0, 66, 0, 63],
        [36, 72, 55, 64],
        [43, 0, 52, 67],
        [38, 74, 57, 66],
        [45, 72, 54, 69],
        [31, 71, 50, 67],
        [38, 0, 55, 62],
        [35, 69, 51, 66],
        [0, 63, 0, 59],
        [40, 71, 55, 67],
        [47, 67, 59, 64],
        [36, 69, 52, 64],
        [43, 64, 55, 60],
        [33, 67, 48, 64],
        [40, 62, 52, 57],
        [35, 66, 51, 59],
        [0, 63, 0, 57],
        [36, 67, 52, 60],
        [38, 66, 54, 62],
        [40, 64, 55, 59],
        [43, 62, 59, 67],
        [33, 60, 52, 57],
        [35, 59, 51, 54],
        [40, 64, 55, 59],
        [0, 0, 0, 64],
        [33, 69, 52, 60],
        [38, 66, 57, 62],
        [31, 67, 50, 59],
        [38, 62, 55, 67],
        [35, 63, 54, 57],
        [47, 66, 51, 59],
        [40, 64, 55, 59],
        [0, 0, 0, 0],
    ];
    let cue = soundtrack::cues().remove(havoc::audio::MusicState::Defeat as usize);
    assert_eq!(cue.duration_ticks(), 2304);
    let rows: Vec<_> = cue.bars.iter().flat_map(|bar| bar.rows.iter()).collect();
    for (notes, sounding) in original.iter().zip(rows.chunks_exact(4)) {
        assert_eq!(&sounding[0][..4], notes);
        for voice in 0..4 {
            assert_eq!(sounding[1][voice], if notes[voice] == 0 { 0 } else { HOLD });
        }
        assert_eq!(&sounding[2][..4], &[0; 4]);
        assert_eq!(&sounding[3][..4], &[0; 4]);
    }
    for bar in &cue.bars {
        assert_eq!(
            &bar.instruments[..4],
            &[
                [0x40, 0x00, 0x04, 0x02, 0x62],
                [0x20, 0x00, 0x08, 0x14, 0x73],
                [0x10, 0x00, 0x08, 0x26, 0x54],
                [0x40, 0x00, 0x02, 0x03, 0x42],
            ]
        );
        assert!(bar.rows.iter().all(|row| row[8] == 0 && row[9] == 0));
    }
}

#[derive(Default)]
struct PlaybackBus {
    registers: [[u8; 25]; music::SID_COUNT],
    effect_writes: usize,
}

impl havoc::audio::SidBus for PlaybackBus {
    fn write(&mut self, sid: u8, register: u8, value: u8) {
        assert!((sid as usize) < music::SID_COUNT && register < 25);
        self.registers[sid as usize][register as usize] = value;
        if sid < 2 && (14..21).contains(&register) {
            self.effect_writes += 1;
        }
    }
}

fn sounding_music(bus: &PlaybackBus) -> [Option<u16>; MUSIC_VOICES] {
    std::array::from_fn(|voice| {
        // Public hardware contract: the core quartet uses voices one/two of
        // SID 0/1; each additional SID (2/3) contributes three music voices.
        let (sid, base) = if voice < 4 {
            (voice & 1, (voice >> 1) * 7)
        } else {
            (2 + (voice - 4) / 3, (voice - 4) % 3 * 7)
        };
        let registers = &bus.registers[sid];
        (registers[base + 4] & 1 != 0)
            .then(|| u16::from_le_bytes([registers[base], registers[base + 1]]))
    })
}

#[test]
fn authored_suite_loops_exactly_with_five_fps_page_service_on_both_video_clocks() {
    use havoc::audio::{
        note_frequency, AudioEngine, AudioEvent, Effect, MusicState, VideoStandard,
    };
    let bank = soundtrack::score_bank();
    let cues = soundtrack::cues();
    let states = [
        MusicState::Title,
        MusicState::Lowlands,
        MusicState::Canyon,
        MusicState::Fortress,
        MusicState::Carrier,
        MusicState::Victory,
        MusicState::Defeat,
        MusicState::CrystalCurrent,
        MusicState::AshesAndSteel,
        MusicState::TempestDrive,
        MusicState::GorgeWarden,
        MusicState::CitadelHeart,
        MusicState::CarrierOverdrive,
        MusicState::AsteroidBelt,
        MusicState::ShatteredMoon,
        MusicState::ShipGraveyard,
        MusicState::OrbitalShipyard,
        MusicState::SpaceCarrier,
        MusicState::JumpGate,
        MusicState::Ascent,
        MusicState::GateGuardian,
    ];
    assert_eq!(states.len(), music::CUE_COUNT);
    for (index, state) in states.iter().enumerate() {
        assert_eq!(MusicState::from_cue(index as u8), Some(*state));
    }
    for invalid in states.len()..=255 {
        assert_eq!(MusicState::from_cue(invalid as u8), None);
    }
    for standard in [VideoStandard::Pal, VideoStandard::Ntsc] {
        for (cue, state) in cues.iter().zip(states) {
            let info = music::cue_info(&bank[..BANK_HEADER_SIZE], state as u8).unwrap();
            let mut player = AudioEngine::new(standard);
            let mut bus = PlaybackBus::default();
            let mut next_page = [0u8; PAGE_SIZE];
            player.reset(&mut bus);
            player.set_music(state);
            #[cfg(feature = "music-kernels")]
            {
                // The foreground loads the slot before publishing page zero.
                assert_eq!(player.kernel_request(), Some(state));
                player.accept_kernel(state, havoc::audio::kernels::has_kernel(state));
                assert_eq!(player.kernel_request(), None);
            }
            let opening = player.page_request().unwrap();
            assert_eq!((opening.cue, opening.page), (state, 0));
            let start = info.offset as usize;
            next_page.copy_from_slice(&bank[start..start + PAGE_SIZE]);
            player.publish_page(opening, &next_page).unwrap();
            player.tick(&mut bus, &next_page); // Deliberate cue-change gate-low tick.
            assert_eq!(player.current_cue(), state);
            bus.effect_writes = 0;

            let loop_ticks = cue.duration_ticks();
            let page_ticks = ROWS_PER_BAR * cue.row_ticks as usize;
            let mut trace = Vec::with_capacity(loop_ticks * 2);
            let mut cutoffs = Vec::with_capacity(loop_ticks * 2);
            let mut visited_pages = Vec::new();
            for tick in 0..loop_ticks * 2 {
                // Main-thread work happens only once per ten audio ticks: five
                // frames/second, slower than the nominal six-FPS rendering.
                if tick % 10 == 0 {
                    let before = (player.current_page(), player.page_request());
                    player.set_music(state);
                    assert_eq!(
                        (player.current_page(), player.page_request()),
                        before,
                        "idempotent selection discarded a pending page"
                    );
                    if let Some(request) = player.page_request() {
                        assert_eq!(request.cue, state);
                        let at = info.offset as usize + request.page as usize * PAGE_SIZE;
                        next_page.copy_from_slice(&bank[at..at + PAGE_SIZE]);
                        player
                            .publish_page(request, &next_page)
                            .unwrap_or_else(|error| {
                                panic!("{} {standard:?} page{}: {error:?}", cue.name, request.page)
                            });
                    }
                }
                if tick % 50 == 0 {
                    assert!(player.queue(AudioEvent {
                        effect: Effect::Fire,
                        pan: 0
                    }));
                }
                player.tick(&mut bus, &next_page);
                let expected_page = (tick / page_ticks) % cue.bars.len();
                assert_eq!(
                    player.current_page() as usize,
                    expected_page,
                    "{} {standard:?} tick{tick}",
                    cue.name
                );
                assert_eq!(player.current_cue(), state);
                assert_eq!(player.underruns(), 0, "{} {standard:?}", cue.name);
                let sounding = sounding_music(&bus);
                if tick % cue.row_ticks as usize == 0 {
                    let row = tick / cue.row_ticks as usize % ROWS_PER_BAR;
                    for (voice, &event) in cue.bars[expected_page].rows[row][..MUSIC_VOICES]
                        .iter()
                        .enumerate()
                    {
                        match event {
                            0 => assert_eq!(sounding[voice], None),
                            HOLD => assert!(sounding[voice].is_some()),
                            // A kernel's portamento glides into slurred notes.
                            _ if cfg!(feature = "music-kernels") && event & 0x80 != 0 => {
                                assert!(sounding[voice].is_some())
                            }
                            _ => assert_eq!(
                                sounding[voice],
                                Some(note_frequency(standard, event & 0x7f)),
                                "{} voice{voice} tick{tick}",
                                cue.name
                            ),
                        }
                    }
                }
                if tick % page_ticks == 0 {
                    visited_pages.push(player.current_page());
                }
                cutoffs.push(bus.registers[0][22]);
                trace.push(sounding);
            }
            let expected_visits: Vec<_> = (0..2).flat_map(|_| 0..info.pages).collect();
            assert_eq!(visited_pages, expected_visits);
            assert_eq!(
                &trace[..loop_ticks],
                &trace[loop_ticks..],
                "{} {standard:?} changed on its second traversal",
                cue.name
            );
            assert!(bus.effect_writes > 0, "effects must run during the score");
            let moving = cutoffs.iter().any(|&c| c != 0x80);
            if cfg!(feature = "music-kernels") && state != MusicState::Defeat {
                assert!(moving, "{} sweeps its filter", cue.name);
                // Each bar's authored target is reached before the bar ends,
                // apart from the wah kernels' deliberate overshoot on attacks.
                for (bar, authored) in cue.bars.iter().enumerate() {
                    let end = loop_ticks + (bar + 1) * page_ticks - 1;
                    let cutoff = cutoffs[end];
                    assert!(
                        cutoff >= authored.filter && cutoff <= authored.filter.saturating_add(0x40),
                        "{} bar {bar}: cutoff {cutoff:#x} target {:#x}",
                        cue.name,
                        authored.filter
                    );
                }
            } else {
                assert!(!moving, "{} must not touch the filter", cue.name);
            }
        }
    }
}
