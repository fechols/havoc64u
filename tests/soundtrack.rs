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
    assert_eq!(suite.len(), 13);
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
            95_104
        } else {
            40_832
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
        if index == havoc::audio::MusicState::Defeat as usize {
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
        // the UltiSID pair; each extra physical SID contributes three voices.
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
    ];
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
        }
    }
}
