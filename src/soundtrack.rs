//! Host-authored H.A.V.O.C. suite. Nothing in this module belongs in C64 RAM.
//!
//! A bar is sixteen sixteenths. Melodies below are written as eight eighth-note
//! positions: `H` ties the preceding note, `R` breathes. The accompaniment is
//! orchestrated from explicit harmonic progressions; no whole-track transpose
//! cycle substitutes for a composed development or contrasting section.

extern crate std;

use self::std::{vec, vec::Vec};

pub use crate::music::CUE_COUNT;
pub const VOICES: usize = 10;
pub const ROWS_PER_BAR: usize = 16;
pub const HOLD: u8 = 1;
const H: u8 = HOLD;
const R: u8 = 0;

/// The first four parts form a complete two-SID arrangement. The other six
/// parts are optional orchestration, never substitutes for the principal tune.
#[derive(Clone, Debug)]
pub struct ScoreBar {
    pub instruments: [[u8; 5]; VOICES],
    pub rows: [[u8; VOICES]; ROWS_PER_BAR],
}

#[derive(Clone, Debug)]
pub struct Cue {
    pub name: &'static str,
    pub row_ticks: u8,
    pub bars: Vec<ScoreBar>,
}

impl Cue {
    pub fn duration_ticks(&self) -> usize {
        self.bars.len() * ROWS_PER_BAR * self.row_ticks as usize
    }
}

#[derive(Clone, Copy)]
struct Chord {
    bass: u8,
    // Close-position root, third and fifth. Inversions use a separate bass.
    tones: [u8; 3],
}

const fn chord(bass: u8, tones: [u8; 3]) -> Chord {
    Chord { bass, tones }
}

const EM: Chord = chord(40, [52, 55, 59]);
const EMB: Chord = chord(35, [52, 55, 59]);
const C: Chord = chord(36, [48, 52, 55]);
const G: Chord = chord(31, [55, 59, 62]);
const D: Chord = chord(38, [50, 54, 57]);
const AM: Chord = chord(33, [57, 60, 64]);
const B7: Chord = chord(35, [51, 57, 59]);
const FSS: Chord = chord(30, [54, 57, 60]);
const E: Chord = chord(40, [52, 56, 59]);
const EBASS: Chord = chord(35, [52, 56, 59]);
const A: Chord = chord(33, [57, 61, 64]);
const B: Chord = chord(35, [47, 51, 54]);
const CSM: Chord = chord(37, [49, 52, 56]);
const FSM: Chord = chord(30, [54, 57, 61]);
const CM: Chord = chord(36, [48, 51, 55]);
const CMB: Chord = chord(31, [48, 51, 55]);
const AB: Chord = chord(32, [56, 60, 63]);
const EF: Chord = chord(39, [51, 55, 58]);
const FM: Chord = chord(29, [53, 56, 60]);
const G7: Chord = chord(31, [55, 59, 65]);
const DS: Chord = chord(38, [50, 53, 56]);
const BF: Chord = chord(34, [46, 50, 53]);
const BM: Chord = chord(35, [47, 50, 54]);
const F: Chord = chord(29, [53, 57, 60]);
const CS7: Chord = chord(37, [49, 53, 59]);
const GS7: Chord = chord(32, [56, 60, 66]);

#[derive(Clone, Copy)]
enum Texture {
    Overture,
    Flight,
    Canyon,
    Fortress,
    Carrier,
    Homeward,
    Crystal,
    Forge,
    Tempest,
    Siege,
    Reactor,
    Overdrive,
}

// Per-voice waveform, pulse low/high, AD, SR. The lead is intentionally above
// the inner parts; extra sustained voices use low sustain levels so ten parts
// do not obscure the melody or the two independently reserved effects voices.
const INSTRUMENTS: [[u8; 5]; VOICES] = [
    [0x40, 0x00, 0x04, 0x02, 0x62], // Core: round pulse bass.
    [0x20, 0x00, 0x08, 0x14, 0x83], // Core: singing saw lead.
    [0x10, 0x00, 0x08, 0x26, 0x43], // Core: soft triangle inner voice.
    [0x40, 0x00, 0x02, 0x03, 0x32], // Core: light plucked arpeggio.
    [0x20, 0x00, 0x08, 0x58, 0x35], // Extra: upper strings.
    [0x20, 0x00, 0x08, 0x68, 0x25], // Extra: lower strings.
    [0x10, 0x00, 0x08, 0x24, 0x43], // Extra: tenor/cello counterline.
    [0x10, 0x00, 0x08, 0x05, 0x23], // Extra: flute/bell answer.
    [0x40, 0x00, 0x08, 0x02, 0x22], // Extra: low pitched orchestral pulse.
    [0x80, 0x00, 0x08, 0x03, 0x12], // Extra: restrained noise accents.
];

/// Place a note with an explicit duration; unspecified rows remain rests.
fn note(bar: &mut ScoreBar, voice: usize, at: usize, pitch: u8, length: usize) {
    assert!(voice < VOICES && at < ROWS_PER_BAR && at + length <= ROWS_PER_BAR);
    assert!(pitch == 0 || (24..=95).contains(&pitch));
    if pitch == 0 || length == 0 {
        return;
    }
    bar.rows[at][voice] = pitch;
    for row in &mut bar.rows[at + 1..at + length] {
        row[voice] = HOLD;
    }
}

fn melody(bar: &mut ScoreBar, line: [u8; 8], legato: bool) {
    let mut previous = 0;
    for (eighth, pitch) in line.into_iter().enumerate() {
        let at = eighth * 2;
        match pitch {
            R => previous = 0,
            H => {
                assert!(
                    previous != 0,
                    "a melody tie needs an earlier note in its bar"
                );
                bar.rows[at][1] = HOLD;
                bar.rows[at + 1][1] = HOLD;
            }
            _ => {
                note(bar, 1, at, pitch, 2);
                // Connected passing notes, rather than legato on every attack.
                if legato
                    && eighth & 1 == 1
                    && previous != 0
                    && (pitch as i16 - previous as i16).abs() <= 2
                    && pitch != previous
                {
                    bar.rows[at][1] |= 0x80;
                }
                previous = pitch;
            }
        }
    }
}

fn arrange(
    harmony: Chord,
    lead: [u8; 8],
    texture: Texture,
    bar_index: usize,
    section: usize,
) -> ScoreBar {
    let mut bar = ScoreBar {
        instruments: INSTRUMENTS,
        rows: [[0; VOICES]; ROWS_PER_BAR],
    };
    let [root, third, fifth] = harmony.tones;
    let bass_fifth = if fifth >= 60 { fifth - 24 } else { fifth - 12 };
    let electronic = matches!(
        texture,
        Texture::Crystal
            | Texture::Forge
            | Texture::Tempest
            | Texture::Siege
            | Texture::Reactor
            | Texture::Overdrive
    );
    let lyrical = matches!(texture, Texture::Flight | Texture::Homeward)
        || matches!(texture, Texture::Overture) && section == 2
        || matches!(texture, Texture::Fortress) && section == 2
        || electronic && section == 2;
    melody(&mut bar, lead, lyrical);
    bar.instruments[1] = match texture {
        Texture::Flight => [0x40, 0x00, 0x06, 0x25, 0x84],
        Texture::Canyon => [0x20, 0x00, 0x08, 0x03, 0x83],
        Texture::Fortress if !lyrical => [0x40, 0x00, 0x05, 0x04, 0x83],
        Texture::Carrier => [0x40, 0x00, 0x03, 0x13, 0x83],
        Texture::Homeward => [0x20, 0x00, 0x08, 0x25, 0x84],
        Texture::Crystal => [0x40, 0x80, 0x01, 0x03, 0x83],
        Texture::Forge => [0x40, 0x00, 0x03, 0x02, 0x83],
        Texture::Tempest => [0x40, 0x00, 0x05, 0x04, 0x84],
        Texture::Siege => [0x40, 0x80, 0x02, 0x02, 0x83],
        Texture::Reactor => [0x40, 0x80, 0x04, 0x03, 0x83],
        Texture::Overdrive => [0x40, 0x80, 0x01, 0x02, 0x84],
        _ => INSTRUMENTS[1],
    };

    if lyrical {
        note(&mut bar, 0, 0, harmony.bass, 7);
        note(&mut bar, 0, 8, bass_fifth, 6);
        note(&mut bar, 2, 0, third, 8);
        note(&mut bar, 2, 8, fifth, 7);
    } else {
        let bass_rhythm = match texture {
            Texture::Canyon => [0, 3, 8, 11],
            Texture::Carrier => [0, 4, 6, 10],
            Texture::Crystal | Texture::Tempest => [0, 3, 8, 11],
            Texture::Forge | Texture::Siege => [0, 3, 7, 10],
            Texture::Reactor | Texture::Overdrive => [0, 6, 9, 12],
            _ => [0, 4, 8, 12],
        };
        for (i, at) in bass_rhythm.into_iter().enumerate() {
            let pitch = if i == 2 { bass_fifth } else { harmony.bass };
            note(&mut bar, 0, at, pitch, 2);
        }
        note(&mut bar, 2, 0, third, 6);
        note(&mut bar, 2, 8, fifth, 6);
    }

    let arpeggio = if bar_index & 1 == 0 {
        [
            root + 12,
            fifth + 12,
            third + 12,
            fifth + 12,
            root + 24,
            fifth + 12,
            third + 12,
            fifth + 12,
        ]
    } else {
        [
            third + 12,
            fifth + 12,
            root + 24,
            fifth + 12,
            third + 12,
            root + 12,
            fifth + 12,
            third + 12,
        ]
    };
    for (i, pitch) in arpeggio.into_iter().enumerate() {
        // Leave actual gaps in the orchestration, particularly in lyrical B
        // sections and phrase cadences; the melody must be able to breathe.
        if !(lyrical && (i == 3 || i == 7)) {
            note(&mut bar, 3, i * 2, pitch, 1);
        }
    }
    if electronic && !lyrical {
        // The added sixteenth-note answers give the new suite its brisk chip
        // arpeggios while leaving the four original accompaniment voices intact.
        for i in 0..8 {
            let pitch = [third + 12, root + 24, fifth + 12][(i + section) % 3];
            note(&mut bar, 3, i * 2 + 1, pitch, 1);
        }
        bar.instruments[3] = [0x40, 0x00, 0x01, 0x02, 0x22];
    }

    // The optional ensemble grows over a phrase instead of being permanently
    // tutti. Its harmony is already supplied by the four compatible parts.
    if bar_index % 8 >= 2 || section > 0 {
        note(&mut bar, 4, 0, third + 12, 14);
        note(&mut bar, 5, 0, root, 14);
    }
    if section > 0 && bar_index % 4 != 0 {
        let counter = if bar_index & 1 == 0 {
            [fifth, third, root, third]
        } else {
            [root, third, fifth, third]
        };
        for (beat, pitch) in counter.into_iter().enumerate() {
            note(&mut bar, 6, beat * 4, pitch, 3);
        }
    }
    // Only answer in the spaces left by the lead, or at its phrase-ending
    // sustained note. This avoids a second continuously competing melody.
    if lead[6] == R || lead[6] == H {
        note(&mut bar, 7, 12, fifth + 12, 2);
        note(&mut bar, 7, 14, third + 12, 2);
    }
    if !lyrical || section >= 3 {
        note(&mut bar, 8, 0, harmony.bass, 1);
        note(&mut bar, 8, 8, harmony.bass + 12, 1);
        if electronic
            || matches!(
                texture,
                Texture::Canyon | Texture::Fortress | Texture::Carrier
            )
        {
            note(&mut bar, 9, 4, 76, 1);
            note(&mut bar, 9, 12, 71, 1);
        } else if bar_index % 8 == 0 {
            note(&mut bar, 9, 0, 82, 1);
        }
    }
    bar
}

fn composed(
    name: &'static str,
    row_ticks: u8,
    texture: Texture,
    harmonies: &[[Chord; 8]],
    melodies: &[[[u8; 8]; 8]],
) -> Cue {
    assert_eq!(harmonies.len(), melodies.len());
    let mut bars = Vec::with_capacity(harmonies.len() * 8);
    for (section, (harmony, melody)) in harmonies.iter().zip(melodies).enumerate() {
        for local in 0..8 {
            bars.push(arrange(
                harmony[local],
                melody[local],
                texture,
                section * 8 + local,
                section,
            ));
        }
    }
    Cue {
        name,
        row_ticks,
        bars,
    }
}

fn title() -> Cue {
    // Overture: declaration, climbing answer, G-major cantabile, broad reprise.
    let harmony = [
        [EM, EM, C, G, AM, EM, B7, B7],
        [EM, G, C, D, AM, EM, FSS, B7],
        [G, D, EM, C, AM, C, FSS, B7],
        [EM, C, G, D, AM, EMB, B7, EM],
    ];
    let lead = [
        [
            [64, H, 67, H, 71, H, 69, 67],
            [64, H, H, H, 62, 64, 67, H],
            [67, H, 72, H, 71, 69, 67, 64],
            [62, H, 67, H, 71, H, H, H],
            [69, H, 72, 71, 69, H, 67, 64],
            [67, H, 71, H, 76, H, 74, 71],
            [69, H, 66, H, 63, H, 66, 69],
            [71, H, H, H, 69, 66, 63, R],
        ],
        [
            [76, H, 74, 71, 67, H, 71, H],
            [74, H, 71, H, 67, H, 69, 71],
            [72, H, 76, H, 79, H, 76, 74],
            [78, H, 76, 74, 69, H, 66, H],
            [76, H, 72, H, 69, H, 71, 72],
            [71, 69, 67, H, 64, H, 67, 71],
            [72, H, 69, H, 66, 69, 72, 74],
            [75, H, 78, H, 83, H, 78, 75],
        ],
        [
            [71, H, H, H, 74, H, 76, 74],
            [69, H, H, H, 66, H, 64, 62],
            [67, H, 71, H, 76, H, H, H],
            [76, 74, 72, H, 67, H, H, R],
            [72, H, 71, 69, 64, H, 67, 69],
            [67, H, 64, H, 60, H, 62, 64],
            [66, H, 69, H, 72, H, 69, 66],
            [63, H, H, H, 66, H, 71, H],
        ],
        [
            [76, H, 79, H, 83, H, 81, 79],
            [76, H, H, H, 79, 76, 72, H],
            [74, H, 79, H, 83, H, 81, 79],
            [78, H, 76, 74, 69, H, H, H],
            [76, H, 72, 71, 69, H, 72, 76],
            [71, H, 67, H, 64, H, 67, 71],
            [75, H, 78, H, 81, 78, 75, 71],
            [76, H, H, H, 71, H, 67, R],
        ],
    ];
    composed(
        "Wings Against the Dawn",
        6,
        Texture::Overture,
        &harmony,
        &lead,
    )
}

fn lowland() -> Cue {
    // The unhurried flight theme owns the wide intervals; an ascending bass
    // progression in the middle eight supports a different, descending tune.
    let harmony = [
        [EM, EM, C, G, AM, C, D, B7],
        [EM, G, C, D, AM, EM, FSS, B7],
        [C, D, EM, G, AM, C, FSS, B7],
        [EM, C, G, D, AM, EM, B7, EM],
    ];
    let lead = [
        [
            [64, H, H, H, 67, H, 71, H],
            [69, H, 67, H, 64, H, H, R],
            [67, H, 72, H, 76, H, H, H],
            [74, H, 71, H, 67, H, 69, 71],
            [72, H, H, H, 71, 69, 67, H],
            [64, H, 67, H, 72, H, 71, 69],
            [66, H, 69, H, 74, H, 72, 69],
            [71, H, H, H, 69, H, 66, R],
        ],
        [
            [67, H, 71, H, 76, H, H, H],
            [74, H, 71, 69, 67, H, 71, H],
            [72, H, 76, H, 79, H, 76, 74],
            [78, H, H, H, 76, 74, 69, H],
            [76, H, 72, H, 69, H, 67, 64],
            [71, H, 67, H, 64, H, 62, 64],
            [66, H, 69, H, 72, H, 74, 72],
            [71, H, 69, 66, 63, H, 66, R],
        ],
        [
            [76, H, H, H, 74, H, 72, H],
            [74, H, H, H, 72, H, 69, H],
            [71, H, H, H, 69, H, 67, H],
            [67, H, 62, H, 59, H, H, R],
            [64, H, 67, 69, 72, H, 71, 69],
            [67, H, H, H, 64, H, 62, 60],
            [66, H, 69, H, 72, H, 69, 66],
            [63, H, 66, H, 71, H, H, R],
        ],
        [
            [76, H, H, H, 79, H, 83, H],
            [81, H, 79, H, 76, H, 74, 72],
            [74, H, 79, H, 83, H, H, H],
            [81, H, 78, H, 74, H, 72, 69],
            [76, H, 72, H, 69, H, H, H],
            [71, H, 67, H, 64, H, 67, 71],
            [69, H, 66, H, 63, H, 66, 71],
            [76, H, H, H, 71, H, 64, R],
        ],
    ];
    composed("The Wide Horizon", 6, Texture::Flight, &harmony, &lead)
}

fn canyon() -> Cue {
    // Five eight-bar spans: bounding subject, answer, open-sky contrast,
    // sequential pursuit, and a changed final statement. The phrase rhythm
    // deliberately crosses the syncopated accompaniment's accents.
    let harmony = [
        [EM, D, C, B7, AM, EM, FSS, B7],
        [EM, G, C, D, AM, C, FSS, B7],
        [G, D, EM, C, AM, G, D, B7],
        [AM, D, G, C, FSS, B7, EM, B7],
        [EM, D, C, G, AM, EM, B7, B7],
    ];
    let lead = [
        [
            [64, 67, 71, H, 69, 67, 64, H],
            [66, 69, 74, H, 72, 69, 66, R],
            [67, 72, 76, H, 74, 72, 67, H],
            [66, H, 63, 66, 71, H, 69, 66],
            [64, 69, 72, H, 71, 69, 67, 64],
            [67, H, 71, H, 76, 74, 71, 67],
            [66, 69, 72, H, 74, 72, 69, 66],
            [63, H, 66, H, 71, H, H, R],
        ],
        [
            [76, H, 71, 67, 64, H, 67, 71],
            [74, H, 71, 67, 62, H, 67, 71],
            [72, 76, 79, H, 76, 74, 72, 67],
            [74, H, 78, H, 81, 78, 76, 74],
            [76, 72, 69, H, 64, 67, 69, 72],
            [76, H, 72, H, 67, 64, 62, 60],
            [66, 69, 72, 74, 78, H, 76, 72],
            [75, H, 71, H, 66, H, 63, R],
        ],
        [
            [79, H, H, H, 78, 76, 74, H],
            [78, H, H, H, 76, 74, 72, 69],
            [76, H, 74, 71, 67, H, H, H],
            [72, H, 71, 69, 67, H, H, R],
            [69, H, 72, H, 76, H, 72, 71],
            [74, H, 71, H, 67, H, 69, 71],
            [69, H, 66, H, 62, H, 64, 66],
            [63, H, 66, H, 71, H, H, R],
        ],
        [
            [69, 72, 76, 72, 69, H, 67, 64],
            [69, 74, 78, 74, 69, H, 66, 62],
            [71, 74, 79, 74, 71, H, 69, 67],
            [72, 76, 79, 76, 72, H, 71, 67],
            [72, H, 69, 66, 60, 66, 69, 72],
            [75, H, 71, 69, 66, 63, 66, 71],
            [76, 74, 71, 67, 64, H, 67, 71],
            [78, H, 75, H, 71, H, 69, 66],
        ],
        [
            [76, 79, 83, H, 81, 79, 76, H],
            [78, 81, 86, H, 84, 81, 78, R],
            [79, 76, 72, H, 76, 79, 84, H],
            [83, H, 79, H, 74, 71, 74, 79],
            [81, H, 76, 72, 69, H, 72, 76],
            [79, 76, 71, H, 67, H, 71, 76],
            [78, H, 75, 71, 69, H, 66, 63],
            [71, H, 66, H, 63, H, H, R],
        ],
    ];
    composed("Stone and Thunder", 5, Texture::Canyon, &harmony, &lead)
}

fn fortress() -> Cue {
    // A minor martial subject frames a broad C/G-major middle eight. The
    // penultimate section contracts the theme into an ascending sequence;
    // the return answers it with descending, rather than transposed, phrases.
    let harmony = [
        [AM, EM, FSS, B7, AM, C, FSS, B7],
        [EM, EMB, C, G, AM, EM, FSS, B7],
        [C, G, AM, EM, C, D, G, B7],
        [AM, D, G, C, AM, FSS, B7, B7],
        [EM, C, G, D, AM, EM, FSS, B7],
    ];
    let lead = [
        [
            [69, H, 69, 72, 76, H, 72, H],
            [71, H, 71, 67, 64, H, 67, 71],
            [72, H, 69, 66, 60, H, 66, 69],
            [71, H, 71, 75, 78, H, 75, 71],
            [76, H, 72, 69, 64, H, 69, 72],
            [79, H, 76, H, 72, 71, 67, H],
            [78, H, 72, 69, 66, H, 69, 72],
            [75, H, 71, H, 66, H, H, R],
        ],
        [
            [64, 67, 71, H, 76, H, 71, H],
            [74, H, 71, 67, 64, H, 67, 71],
            [72, 76, 79, H, 84, H, 79, 76],
            [83, H, 79, 74, 71, H, 74, 79],
            [81, H, 76, 72, 69, H, 72, 76],
            [79, H, 76, 71, 67, H, 71, 76],
            [78, H, 74, 72, 69, H, 66, 69],
            [75, H, 78, H, 83, H, 78, 75],
        ],
        [
            [76, H, H, H, 79, H, 76, H],
            [74, H, 71, H, 67, H, H, H],
            [72, H, H, H, 76, H, 72, 71],
            [71, H, 67, H, 64, H, H, R],
            [67, H, 72, H, 76, H, 79, H],
            [78, H, 76, 74, 69, H, 66, H],
            [74, H, 71, H, 67, H, 69, 71],
            [69, H, 66, H, 63, H, 66, R],
        ],
        [
            [64, 69, 72, H, 76, 72, 69, H],
            [66, 69, 74, H, 78, 74, 69, H],
            [67, 71, 74, H, 79, 74, 71, H],
            [67, 72, 76, H, 79, 76, 72, H],
            [72, 76, 81, H, 76, 72, 69, 64],
            [72, 69, 66, H, 60, 66, 69, 72],
            [75, 78, 83, H, 81, 78, 75, 71],
            [78, H, 75, 71, 69, H, 66, 63],
        ],
        [
            [83, H, 79, H, 76, 74, 71, H],
            [84, H, 79, 76, 72, H, 76, 79],
            [83, H, 79, 74, 71, H, 74, 79],
            [81, H, 78, 74, 69, H, 74, 78],
            [81, 76, 72, H, 69, H, 72, 76],
            [79, 76, 71, H, 67, H, 64, 67],
            [66, H, 69, H, 72, 74, 78, H],
            [75, H, 71, H, 66, H, 63, R],
        ],
    ];
    composed("Citadel in Flames", 5, Texture::Fortress, &harmony, &lead)
}

fn carrier() -> Cue {
    // The antagonist starts in C minor, retaining the old carrier's semitone
    // pressure. Its six sections alternate threat and resistance, travel to
    // E-flat and E minor, then return to a fully written C-minor confrontation.
    let harmony = [
        [CM, CM, AB, G7, FM, CM, DS, G7],
        [CM, EF, AB, BF, FM, CMB, DS, G7],
        [EF, BF, CM, AB, FM, EF, DS, G7],
        [EM, C, AM, B7, EM, G, FSS, B7],
        [FM, BF, EF, AB, DS, G7, CM, G7],
        [CM, AB, EF, BF, FM, CMB, DS, G7],
    ];
    let lead = [
        [
            [72, H, 73, 72, 70, H, 67, H],
            [72, 73, 72, H, 67, H, 66, 67],
            [75, H, 74, 72, 68, H, 72, 75],
            [71, H, 70, 67, 65, H, 67, 71],
            [72, H, 68, 65, 60, H, 65, 68],
            [67, H, 72, H, 75, 74, 72, 67],
            [68, H, 65, 62, 56, H, 62, 65],
            [71, H, 74, H, 79, H, 74, 71],
        ],
        [
            [72, H, 75, H, 79, H, 77, 75],
            [70, H, 75, H, 79, H, 82, H],
            [80, H, 79, 77, 75, H, 72, H],
            [77, H, 74, H, 70, H, 72, 74],
            [77, H, 80, H, 84, H, 80, 77],
            [79, H, 75, H, 72, H, 70, 67],
            [74, H, 77, H, 80, H, 77, 74],
            [83, H, 79, H, 74, H, 71, R],
        ],
        [
            [79, H, H, H, 82, H, 79, H],
            [77, H, 74, H, 70, H, H, H],
            [75, H, 79, H, 84, H, H, H],
            [84, 82, 80, H, 75, H, H, R],
            [80, H, 77, H, 72, H, 74, 77],
            [79, H, 75, H, 70, H, 72, 75],
            [77, H, 74, H, 68, H, 65, 62],
            [71, H, 74, H, 79, H, H, R],
        ],
        [
            [64, H, 67, H, 71, H, 69, 67],
            [64, H, 67, H, 72, H, 76, H],
            [76, H, 72, H, 69, H, 71, 72],
            [75, H, 78, H, 83, H, 78, 75],
            [76, H, 79, H, 83, H, 81, 79],
            [86, H, 83, H, 79, H, 76, 74],
            [78, H, 74, 72, 69, H, 66, 69],
            [75, H, 71, H, 66, H, 63, R],
        ],
        [
            [77, 80, 84, H, 80, 77, 72, H],
            [77, 82, 86, H, 82, 77, 74, H],
            [79, 82, 87, H, 82, 79, 75, H],
            [80, 84, 87, H, 84, 80, 75, H],
            [80, H, 77, 74, 68, 65, 62, H],
            [83, H, 79, 74, 71, 74, 79, 83],
            [84, 83, 79, H, 75, 72, 70, 67],
            [71, 74, 79, H, 77, 74, 71, 67],
        ],
        [
            [84, H, 85, 84, 82, H, 79, H],
            [84, H, 80, H, 75, H, 77, 80],
            [82, H, 79, H, 75, 74, 70, H],
            [81, H, 77, 74, 70, H, 74, 77],
            [80, H, 77, 72, 68, H, 72, 77],
            [79, H, 75, H, 72, H, 70, 67],
            [68, 71, 74, 77, 80, H, 77, 74],
            [83, H, 79, H, 74, H, 71, R],
        ],
    ];
    composed("Against the Iron Sky", 5, Texture::Carrier, &harmony, &lead)
}

fn victory() -> Cue {
    // The flight theme finally receives its major third. After the fanfare,
    // a C-sharp-minor recollection gives the closing E-major cadence meaning.
    let harmony = [
        [E, E, A, B, CSM, A, FSM, B],
        [CSM, FSM, A, E, FSM, A, B, B7],
        [E, A, E, B, A, EBASS, B, E],
    ];
    let lead = [
        [
            [64, H, 68, H, 71, H, H, H],
            [76, H, H, H, 75, 71, 68, H],
            [73, H, 76, H, 81, H, 80, 76],
            [78, H, 75, H, 71, H, 73, 75],
            [76, H, 80, H, 85, H, 83, 80],
            [81, H, 76, H, 73, H, 71, 69],
            [73, H, 69, H, 66, H, 68, 69],
            [71, H, 75, H, 78, H, H, R],
        ],
        [
            [76, H, H, H, 73, H, 71, H],
            [73, H, 69, H, 66, H, H, H],
            [69, H, 73, H, 76, H, 73, 71],
            [68, H, H, H, 64, H, 66, 68],
            [69, H, 73, H, 78, H, 76, 73],
            [76, H, 73, H, 69, H, 71, 73],
            [75, H, 78, H, 83, H, 81, 78],
            [75, H, 71, H, 69, H, 66, R],
        ],
        [
            [76, H, 80, H, 83, H, H, H],
            [81, H, 80, 76, 73, H, 76, 81],
            [80, H, 76, H, 71, H, 68, H],
            [78, H, 75, H, 71, H, 73, 75],
            [76, H, 73, H, 69, H, H, H],
            [71, H, 68, H, 64, H, 68, 71],
            [75, H, 78, H, 83, H, 78, 75],
            [76, H, H, H, H, H, R, R],
        ],
    ];
    let mut cue = composed("Homeward, at Last", 7, Texture::Homeward, &harmony, &lead);
    // The final tonic actually rests before the opening returns; all ensemble
    // parts respect the cadence rather than rattling through a silent lead.
    for row in &mut cue.bars.last_mut().unwrap().rows[12..] {
        *row = [0; VOICES];
    }
    cue
}

fn crystal_current() -> Cue {
    // D-major glassy hook, B-minor answer, spacious G-major middle eight,
    // then a rewritten high-register return with its own descending cadence.
    let harmony = [
        [D, BM, G, A, D, G, EM, A],
        [BM, FSM, G, D, EM, BM, G, A],
        [G, A, D, BM, G, EM, A, A],
        [D, G, BM, A, EM, G, A, D],
    ];
    let lead = [
        [
            [74, H, 78, 81, H, 78, 76, 74],
            [71, H, 74, 78, H, 76, 74, 71],
            [74, H, 79, H, 78, 76, 74, H],
            [73, H, 76, 81, H, 79, 76, R],
            [78, H, 81, H, 86, 81, 78, 74],
            [79, H, 78, 74, 71, H, 74, 79],
            [76, H, 79, 83, H, 81, 79, 76],
            [76, 73, 69, H, 73, H, 76, R],
        ],
        [
            [78, 74, 71, H, 74, H, 78, 81],
            [81, H, 78, 73, 69, H, 73, 78],
            [79, 78, 74, H, 71, H, 74, 78],
            [78, H, 76, 74, 69, H, 66, R],
            [67, H, 71, 74, 76, H, 79, 78],
            [78, H, 74, 71, 66, H, 71, 74],
            [79, H, 83, 81, 79, H, 78, 74],
            [76, H, 73, 71, 69, H, H, R],
        ],
        [
            [71, H, H, H, 74, H, 79, H],
            [76, H, H, H, 73, H, 69, H],
            [78, H, 76, 74, 69, H, H, H],
            [74, H, 71, H, 66, H, H, R],
            [67, H, 71, H, 74, H, 78, 79],
            [83, H, H, H, 81, 79, 76, H],
            [81, H, 79, 76, 73, H, 71, 69],
            [73, H, H, H, 76, H, H, R],
        ],
        [
            [86, H, 81, 78, 74, H, 78, 81],
            [83, H, 79, 78, 74, H, 79, 83],
            [86, 83, 81, H, 78, H, 74, 71],
            [81, H, 76, 73, 69, H, 73, 76],
            [79, 83, 86, H, 83, 79, 76, H],
            [83, H, 79, 74, 71, H, 74, 79],
            [81, 79, 76, H, 73, H, 69, 73],
            [78, H, 74, H, 69, H, H, R],
        ],
    ];
    composed("Crystal Current", 6, Texture::Crystal, &harmony, &lead)
}

fn ashes_and_steel() -> Cue {
    // A-minor clipped hammer motif against offbeat bass; the middle eight
    // opens into C major before the last phrase climbs out of the foundry.
    let harmony = [
        [AM, F, C, E, AM, G, F, E],
        [AM, C, F, E, AM, F, D, E],
        [C, G, F, AM, C, D, F, E],
        [AM, F, C, G, F, D, E, AM],
    ];
    let lead = [
        [
            [69, H, 72, 69, 76, H, 72, R],
            [69, H, 72, 69, 77, H, 76, 72],
            [67, H, 72, 76, 79, H, 76, R],
            [68, H, 71, 76, 80, H, 76, 71],
            [81, H, 76, 72, 69, H, 72, R],
            [79, H, 74, 71, 67, H, 71, 74],
            [77, 76, 72, H, 69, H, 65, 69],
            [68, H, 71, H, 76, H, H, R],
        ],
        [
            [76, 72, 69, H, 64, H, 69, 72],
            [76, 79, 84, H, 79, H, 76, 72],
            [77, H, 81, 84, 81, 77, 72, H],
            [80, H, 76, H, 71, 68, 64, R],
            [69, H, 72, 76, 81, H, 79, 76],
            [77, H, 76, 72, 69, H, 72, 77],
            [78, 74, 69, H, 66, 69, 74, H],
            [80, H, 78, 76, 71, H, 68, R],
        ],
        [
            [76, H, H, H, 72, H, 67, H],
            [71, H, H, H, 74, H, 79, H],
            [77, H, 76, 72, 69, H, H, H],
            [72, H, 69, H, 64, H, H, R],
            [67, H, 72, H, 76, H, 79, H],
            [81, H, 78, H, 74, H, 69, H],
            [77, H, 72, 69, 65, H, 69, 72],
            [71, H, H, H, 68, H, 64, R],
        ],
        [
            [81, H, 84, 81, 88, H, 84, 81],
            [84, H, 81, 77, 72, H, 77, 81],
            [79, 84, 88, H, 84, H, 79, 76],
            [86, H, 83, 79, 74, H, 79, 83],
            [84, H, 81, 77, 76, 72, 69, H],
            [78, H, 74, 69, 66, H, 69, 74],
            [80, 76, 71, H, 68, H, 71, 76],
            [81, H, 76, H, 72, H, 69, R],
        ],
    ];
    composed("Ashes and Steel", 6, Texture::Forge, &harmony, &lead)
}

fn tempest_drive() -> Cue {
    // F-sharp-minor launch-and-answer hook; the relative-major bridge rises
    // over long notes before a descending, wind-buffeted final statement.
    let harmony = [
        [FSM, D, A, E, BM, FSM, D, CS7],
        [FSM, A, BM, CS7, D, A, BM, CS7],
        [A, E, D, FSM, BM, D, E, CS7],
        [FSM, D, A, E, BM, D, CS7, FSM],
    ];
    let lead = [
        [
            [78, H, 81, 80, 78, H, 73, 76],
            [78, H, 81, 86, 85, H, 81, 78],
            [76, H, 81, 85, 83, H, 81, 76],
            [80, H, 83, 88, 86, H, 83, R],
            [78, H, 74, 71, 74, H, 78, 81],
            [81, H, 78, 73, 69, H, 73, 78],
            [86, 85, 81, H, 78, H, 74, 78],
            [77, H, 80, H, 83, H, 80, R],
        ],
        [
            [73, 78, 81, H, 85, 81, 78, H],
            [73, 76, 81, H, 85, 83, 81, H],
            [74, 78, 83, H, 86, 83, 78, 74],
            [77, H, 80, 83, 85, H, 83, R],
            [86, H, 81, 78, 74, H, 78, 81],
            [85, H, 81, 76, 73, H, 76, 81],
            [83, H, 78, 74, 71, H, 74, 78],
            [80, H, 77, 73, 71, H, 73, R],
        ],
        [
            [81, H, H, H, 85, H, 88, H],
            [83, H, H, H, 80, H, 76, H],
            [78, H, 81, H, 86, H, H, H],
            [85, H, 81, 78, 73, H, H, R],
            [74, H, 78, H, 83, H, 81, 78],
            [81, H, H, H, 78, H, 74, H],
            [80, H, 83, H, 88, H, 86, 83],
            [85, H, 83, 80, 77, H, H, R],
        ],
        [
            [90, H, 88, 85, 81, H, 78, 81],
            [86, H, 85, 81, 78, H, 74, 78],
            [88, 85, 81, H, 76, H, 73, 76],
            [83, H, 80, 76, 71, H, 76, 80],
            [86, H, 83, 78, 74, H, 71, 74],
            [81, H, 78, 74, 69, H, 74, 78],
            [83, 80, 77, H, 73, H, 71, 73],
            [78, H, H, H, 73, H, 69, R],
        ],
    ];
    composed("Tempest Drive", 6, Texture::Tempest, &harmony, &lead)
}

fn gorge_warden() -> Cue {
    // C-minor cannon calls use deliberately uneven phrase lengths. A lighter
    // E-flat-major response gives the core quartet room before the last salvo.
    let harmony = [
        [CM, AB, CM, G7, FM, CM, DS, G7],
        [CM, EF, AB, G7, FM, BF, DS, G7],
        [EF, BF, AB, CM, FM, EF, DS, G7],
        [CM, AB, FM, G7, CM, DS, G7, CM],
    ];
    let lead = [
        [
            [60, H, 67, 72, H, 70, 67, R],
            [68, H, 75, 80, H, 79, 75, 72],
            [72, 67, 60, H, 63, 67, 72, H],
            [71, H, 74, 79, H, 77, 74, R],
            [65, H, 72, 77, H, 75, 72, 68],
            [75, H, 72, 67, H, 63, 60, H],
            [68, 65, 62, H, 65, H, 68, 74],
            [71, H, 67, H, 62, H, H, R],
        ],
        [
            [79, H, 75, H, 72, 70, 67, 63],
            [75, H, 79, H, 82, 79, 75, 70],
            [80, 79, 75, H, 72, H, 68, 72],
            [79, 77, 74, H, 71, H, 67, R],
            [77, H, 80, H, 84, 80, 77, 72],
            [82, H, 77, 74, 70, H, 74, 77],
            [80, H, 77, 74, 68, H, 65, 62],
            [71, 74, 79, H, 83, H, 79, R],
        ],
        [
            [79, H, H, H, 75, H, 70, H],
            [74, H, H, H, 77, H, 82, H],
            [80, H, 79, 75, 72, H, H, H],
            [75, H, 72, H, 67, H, H, R],
            [68, H, 72, H, 77, H, 80, H],
            [82, H, 79, H, 75, H, 70, H],
            [77, H, 74, 68, 65, H, 62, H],
            [67, H, H, H, 71, H, 74, R],
        ],
        [
            [84, H, 79, 75, H, 72, 67, H],
            [80, H, 75, 72, H, 68, 63, R],
            [77, 80, 84, H, 80, H, 77, 72],
            [83, H, 79, 74, H, 71, 67, R],
            [79, H, 75, 72, 70, 67, 63, H],
            [74, H, 68, 65, 62, H, 65, 68],
            [71, 74, 79, H, 77, 74, 71, 67],
            [72, H, 67, H, 63, H, 60, R],
        ],
    ];
    composed("Gorge Warden", 5, Texture::Siege, &harmony, &lead)
}

fn citadel_heart() -> Cue {
    // C-sharp-minor mechanical heartbeat alternates short answers and long
    // rising calls; the A-major bridge reveals a warmer line under the armor.
    let harmony = [
        [CSM, A, FSM, GS7, CSM, E, A, GS7],
        [CSM, E, A, B, FSM, CSM, A, GS7],
        [A, E, FSM, CSM, A, B, E, GS7],
        [CSM, A, E, B, FSM, A, GS7, CSM],
    ];
    let lead = [
        [
            [73, H, 73, 76, H, 80, 78, 76],
            [73, H, 73, 76, H, 81, 80, 76],
            [73, H, 69, 66, H, 69, 73, 78],
            [72, H, 75, 80, H, 78, 75, R],
            [80, H, 76, 73, H, 68, 71, 73],
            [76, H, 80, 83, H, 81, 80, 76],
            [81, H, 76, 73, H, 69, 73, 76],
            [78, 75, 72, H, 68, H, H, R],
        ],
        [
            [85, 83, 80, H, 76, H, 73, 68],
            [83, H, 80, 76, 71, H, 76, 80],
            [81, 80, 76, H, 73, H, 69, 73],
            [83, H, 78, 75, 71, H, 75, 78],
            [81, H, 78, 73, 69, H, 73, 78],
            [80, H, 76, 73, 68, 71, 73, 76],
            [81, H, 85, 88, 85, H, 81, 76],
            [84, H, 80, 75, 72, H, 75, R],
        ],
        [
            [76, H, H, H, 81, H, 85, H],
            [83, H, 80, H, 76, H, H, H],
            [78, H, H, H, 81, H, 78, 73],
            [76, H, 73, H, 68, H, H, R],
            [69, H, 73, H, 76, H, 81, H],
            [78, H, 83, H, 87, H, 85, 83],
            [88, H, 83, 80, 76, H, 71, 76],
            [75, H, H, H, 72, H, 68, R],
        ],
        [
            [85, H, 85, 80, H, 76, 73, 76],
            [88, H, 85, 81, H, 76, 73, 69],
            [83, 80, 76, H, 71, H, 76, 80],
            [87, H, 83, 78, 75, H, 78, 83],
            [85, H, 81, 78, 73, H, 69, 73],
            [81, 76, 73, H, 69, H, 73, 76],
            [84, H, 80, 78, 75, H, 72, 68],
            [73, H, 76, H, 80, H, 73, R],
        ],
    ];
    composed("Citadel Heart", 5, Texture::Reactor, &harmony, &lead)
}

fn carrier_overdrive() -> Cue {
    // E-minor pursuit trades with a C-minor threat, then resolves into the
    // brighter G-major bridge. The finale combines new melodic answers rather
    // than duplicating either the existing carrier cue or an earlier section.
    let harmony = [
        [EM, C, D, B7, EM, G, AM, B7],
        [CM, AB, EF, G7, FM, CM, DS, G7],
        [G, D, EM, C, AM, G, FSS, B7],
        [EM, C, G, D, AM, EM, B7, EM],
    ];
    let lead = [
        [
            [76, 79, 78, H, 76, H, 71, 74],
            [76, 79, 84, H, 83, H, 79, 76],
            [78, 81, 86, H, 84, H, 81, 78],
            [75, H, 78, 83, 81, H, 78, R],
            [83, H, 79, 76, 74, 71, 67, H],
            [79, H, 83, 86, 83, H, 79, 74],
            [81, 79, 76, H, 72, H, 69, 72],
            [75, H, 71, H, 66, H, 63, R],
        ],
        [
            [72, H, 75, 79, H, 78, 75, 72],
            [80, H, 79, 75, H, 72, 68, 72],
            [82, H, 79, 75, 70, H, 75, 79],
            [83, H, 79, 74, 71, H, 74, R],
            [77, 80, 84, H, 80, 77, 72, H],
            [79, H, 75, 72, 67, H, 70, 72],
            [74, H, 77, 80, 77, H, 74, 68],
            [71, 74, 79, H, 83, H, H, R],
        ],
        [
            [79, H, H, H, 83, H, 86, H],
            [81, H, H, H, 78, H, 74, H],
            [76, H, 79, H, 83, H, H, H],
            [84, H, 79, 76, 72, H, H, R],
            [76, H, 72, H, 69, H, 67, 64],
            [74, H, 79, H, 83, H, 81, 79],
            [78, H, 74, 72, 69, H, 66, H],
            [75, H, H, H, 71, H, 66, R],
        ],
        [
            [88, 86, 83, H, 79, H, 76, 79],
            [84, H, 88, 86, 84, H, 79, 76],
            [86, 83, 79, H, 74, H, 71, 74],
            [81, H, 78, 74, 69, H, 74, 78],
            [84, 81, 76, H, 72, H, 69, 72],
            [83, H, 79, 76, 71, H, 67, 71],
            [78, 75, 71, H, 69, 66, 63, 66],
            [76, H, 71, H, 67, H, 64, R],
        ],
    ];
    composed("Carrier Overdrive", 5, Texture::Overdrive, &harmony, &lead)
}

// Every sounding four-voice tuple from the existing 46.08-second Lost Wing,
// in its original order. Two 9-tick sixteenths sound, then two breathe, exactly
// preserving each old 18-tick note row and its 18-tick rest. Two new bars are
// one original phrase; the composition has not been shortened or transposed.
const LOST_WING: [[u8; 4]; 64] = [
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

fn defeat() -> Cue {
    let mut bars = Vec::with_capacity(16);
    for (bar_index, notes) in LOST_WING.chunks_exact(4).enumerate() {
        let mut instruments = INSTRUMENTS;
        instruments[..4].copy_from_slice(&[
            [0x40, 0x00, 0x04, 0x02, 0x62],
            [0x20, 0x00, 0x08, 0x14, 0x73],
            [0x10, 0x00, 0x08, 0x26, 0x54],
            [0x40, 0x00, 0x02, 0x03, 0x42],
        ]);
        let mut bar = ScoreBar {
            instruments,
            rows: [[0; VOICES]; ROWS_PER_BAR],
        };
        for (beat, pitches) in notes.iter().enumerate() {
            for (voice, &pitch) in pitches.iter().enumerate() {
                note(&mut bar, voice, beat * 4, pitch, 2);
            }
            // Gentle optional colour enters after the original opening. Its
            // breaths coincide with the quartet; there is no percussion here.
            if (4..12).contains(&bar_index) {
                if pitches[2] != 0 {
                    note(&mut bar, 4, beat * 4, pitches[2] + 12, 2);
                }
                note(&mut bar, 5, beat * 4, pitches[3], 2);
            }
            if (6..10).contains(&bar_index) && beat & 1 == 0 && pitches[0] != 0 {
                note(&mut bar, 6, beat * 4, pitches[0] + 12, 2);
            }
            if pitches[1] == 0 && pitches[3] != 0 {
                note(&mut bar, 7, beat * 4, pitches[3] + 12, 2);
            }
        }
        bars.push(bar);
    }
    Cue {
        name: "Lost Wing",
        row_ticks: 9,
        bars,
    }
}

#[derive(Clone, Copy)]
enum SpaceTexture {
    Asteroids,
    Moon,
    Graveyard,
    Shipyard,
    Dreadnought,
    Gate,
    Ascent,
    Guardian,
}

// Original melodies inspired by Jeroen Tel's tuneful, syncopated SID writing.
// The compatible quartet is bass, lead, drums, arpeggio; the expanded ensemble
// adds harmony and answering lines. No effects voice is borrowed for drums.
fn arrange_space(
    harmony: Chord,
    lead: [u8; 8],
    texture: SpaceTexture,
    index: usize,
    section: usize,
) -> ScoreBar {
    let mut bar = arrange(harmony, lead, Texture::Crystal, index, section);
    let spacious = section == 2;
    bar.instruments[0] = [0x40, 0x00, 0x03, 0x02, 0x42];
    bar.instruments[1] = match texture {
        SpaceTexture::Asteroids => [0x40, 0x80, 0x01, 0x02, 0x83],
        SpaceTexture::Moon => [0x40, 0x00, 0x06, 0x14, 0x84],
        SpaceTexture::Graveyard => [0x40, 0x00, 0x07, 0x25, 0x74],
        SpaceTexture::Shipyard => [0x40, 0x80, 0x02, 0x02, 0x83],
        SpaceTexture::Dreadnought => [0x40, 0x00, 0x03, 0x03, 0x83],
        SpaceTexture::Gate => [0x40, 0x00, 0x05, 0x14, 0x84],
        SpaceTexture::Ascent => [0x40, 0x80, 0x04, 0x03, 0x84],
        SpaceTexture::Guardian => [0x40, 0x00, 0x01, 0x02, 0x83],
    };
    bar.instruments[2] = [0x80, 0x00, 0x08, 0x01, 0x12];
    for row in &mut bar.rows {
        row[0] = R;
        row[2] = R;
    }
    let rhythm = match texture {
        SpaceTexture::Asteroids | SpaceTexture::Gate | SpaceTexture::Ascent => [0, 3, 8, 11],
        SpaceTexture::Moon | SpaceTexture::Graveyard => [0, 6, 9, 12],
        SpaceTexture::Shipyard | SpaceTexture::Guardian => [0, 3, 7, 10],
        SpaceTexture::Dreadnought => [0, 4, 7, 11],
    };
    let fifth = harmony.tones[2] - 12;
    for (beat, at) in rhythm.into_iter().enumerate() {
        let pitch = match beat {
            1 if !spacious => harmony.bass + 12,
            2 => fifth,
            _ => harmony.bass,
        };
        note(&mut bar, 0, at, pitch, if spacious { 3 } else { 2 });
    }
    // Low noise attacks reinforce the pulse bass; high short attacks are hats,
    // and lower backbeats make snares. The contrasting middle eight breathes.
    for (at, pitch) in [
        (0, 40),
        (2, 88),
        (4, 67),
        (6, 86),
        (8, 43),
        (10, 88),
        (12, 70),
        (14, 86),
    ] {
        if !spacious || at % 4 == 0 {
            note(&mut bar, 2, at, pitch, 1);
        }
    }
    if index % 8 == 7 && !spacious {
        for (at, pitch) in [(13, 76), (15, 82)] {
            note(&mut bar, 2, at, pitch, 1);
        }
    }
    if index & 1 != 0 {
        for (beat, pitch) in harmony.tones.into_iter().enumerate() {
            note(&mut bar, 6, beat * 4, pitch, 3);
        }
    }
    bar
}

fn space_composed(
    name: &'static str,
    row_ticks: u8,
    texture: SpaceTexture,
    harmony: [[Chord; 8]; 4],
    lead: [[[u8; 8]; 8]; 4],
) -> Cue {
    let mut bars = Vec::with_capacity(32);
    for section in 0..4 {
        for local in 0..8 {
            bars.push(arrange_space(
                harmony[section][local],
                lead[section][local],
                texture,
                section * 8 + local,
                section,
            ));
        }
    }
    Cue {
        name,
        row_ticks,
        bars,
    }
}

fn asteroid_belt() -> Cue {
    // E-minor skipping hook, an answering descent, open C-major middle eight,
    // then a varied high return whose dominant drops cleanly into the loop.
    space_composed(
        "Slingshot Through the Belt",
        5,
        SpaceTexture::Asteroids,
        [
            [EM, C, G, D, EM, AM, FSS, B7],
            [EM, G, AM, C, D, EM, C, B7],
            [C, G, D, EM, AM, C, D, B7],
            [EM, C, G, D, AM, EM, FSS, B7],
        ],
        [
            [
                [76, 71, H, 79, 76, H, 74, 71],
                [72, 67, H, 76, 79, H, 76, R],
                [74, 71, H, 79, 83, 81, 79, H],
                [78, 74, H, 69, 74, H, 76, 78],
                [76, 71, H, 79, 83, H, 81, 79],
                [81, 76, H, 72, 69, H, 72, 76],
                [78, 72, H, 69, 66, 69, 72, H],
                [75, H, 71, 66, 63, H, H, R],
            ],
            [
                [83, H, 81, 79, 76, 74, 71, H],
                [79, H, 74, 71, 67, H, 71, 74],
                [76, 72, 69, H, 72, 76, 81, H],
                [79, 76, 72, H, 67, H, 72, R],
                [78, 76, 74, 69, 66, H, 69, 74],
                [76, 74, 71, H, 67, H, 64, 67],
                [72, 76, H, 79, 84, H, 79, 76],
                [78, H, 75, 71, 69, 66, 63, R],
            ],
            [
                [76, H, H, H, 72, H, 67, H],
                [74, H, H, H, 71, H, 67, H],
                [69, H, 74, H, 78, H, H, H],
                [79, H, 76, H, 71, H, H, R],
                [72, H, H, 76, 81, H, 79, 76],
                [79, H, 76, H, 72, H, 71, 67],
                [74, H, 78, H, 81, H, 78, 74],
                [75, H, H, H, 71, H, 66, R],
            ],
            [
                [83, 79, H, 86, 83, H, 81, 79],
                [84, 79, H, 76, 72, H, 76, 79],
                [83, H, 79, 74, 71, 74, 79, H],
                [81, H, 78, 74, 69, H, 74, R],
                [81, 79, 76, H, 72, H, 69, 72],
                [79, 76, 71, H, 67, 64, 67, 71],
                [78, 74, 72, H, 69, 66, 69, 72],
                [75, H, 78, 75, 71, H, 66, R],
            ],
        ],
    )
}

fn shattered_moon() -> Cue {
    // Broad D-major orbiting intervals answer the frantic asteroid mission.
    space_composed(
        "Silver Fractures",
        6,
        SpaceTexture::Moon,
        [
            [D, BM, G, D, EM, G, A, A],
            [BM, FSM, G, D, EM, BM, G, A],
            [G, D, EM, BM, G, EM, A, A],
            [D, G, BM, FSM, EM, G, A, D],
        ],
        [
            [
                [74, H, 81, H, 78, 76, 74, H],
                [78, H, 74, H, 71, H, 69, 71],
                [79, H, 83, H, 81, 79, 74, H],
                [78, H, H, H, 74, H, 69, R],
                [76, H, 79, 83, 86, H, 83, 79],
                [83, H, 79, H, 74, H, 71, 74],
                [81, H, 76, 73, 69, H, 73, 76],
                [73, H, 76, H, 81, H, H, R],
            ],
            [
                [83, 81, 78, H, 74, H, 71, H],
                [81, 78, 73, H, 69, H, 66, R],
                [79, 78, 74, H, 71, 74, 79, H],
                [81, 78, H, 74, 69, H, 66, 69],
                [79, 83, H, 86, 83, 79, 76, H],
                [78, H, 74, 71, 66, 71, 74, H],
                [79, 83, 86, H, 83, H, 81, 79],
                [81, H, 79, 76, 73, H, 69, R],
            ],
            [
                [79, H, H, H, 74, H, 71, H],
                [78, H, 74, H, 69, H, H, H],
                [76, H, H, H, 79, H, 83, H],
                [81, H, 78, 74, 71, H, H, R],
                [74, H, 79, H, 83, H, 86, H],
                [83, H, H, 79, 76, H, 74, 71],
                [76, H, 81, H, 85, H, 83, 81],
                [76, H, H, H, 73, H, 69, R],
            ],
            [
                [86, H, 81, H, 78, 81, 86, H],
                [83, H, 86, H, 83, 79, 74, H],
                [83, 81, 78, H, 74, 71, 74, H],
                [81, 78, 73, H, 69, H, 73, 78],
                [83, H, 79, 76, 71, H, 76, 79],
                [86, H, 83, 79, 74, H, 79, R],
                [85, 81, 76, H, 73, H, 76, 81],
                [78, H, H, H, 74, H, 69, R],
            ],
        ],
    )
}

fn ship_graveyard() -> Cue {
    // C minor, sighing semitones and descending answers; the E-flat middle
    // eight briefly lights the wreckage before the narrower opening returns.
    space_composed(
        "Echoes of Lost Fleets",
        7,
        SpaceTexture::Graveyard,
        [
            [CM, AB, FM, G7, CM, EF, DS, G7],
            [FM, CM, AB, EF, DS, G7, CM, G7],
            [EF, BF, AB, CM, FM, EF, DS, G7],
            [CM, AB, FM, G7, EF, CMB, DS, G7],
        ],
        [
            [
                [72, H, H, 71, 72, H, 67, R],
                [68, H, H, 72, 75, H, 74, 72],
                [77, H, 76, 77, 72, H, 68, H],
                [71, H, H, 70, 67, H, 65, R],
                [75, H, 74, 72, 67, H, H, H],
                [70, H, H, 75, 79, H, 77, 75],
                [74, H, 71, 68, 65, H, 62, H],
                [67, H, 71, H, 74, H, H, R],
            ],
            [
                [80, H, 77, 72, 68, H, 65, H],
                [79, H, 75, 72, 67, H, 63, R],
                [80, H, 79, 75, 72, H, 68, H],
                [79, H, 77, 75, 70, H, 67, 70],
                [77, H, 74, 68, 65, H, 62, H],
                [79, H, 77, 74, 71, H, 67, R],
                [75, 74, 72, H, 67, H, 63, H],
                [74, H, 71, 67, 65, H, H, R],
            ],
            [
                [75, H, H, H, 79, H, 82, H],
                [81, H, H, H, 77, H, 74, H],
                [80, H, 79, 75, 72, H, H, H],
                [79, H, H, 75, 72, H, 70, R],
                [77, H, H, H, 80, H, 84, H],
                [82, H, 79, H, 75, H, H, H],
                [80, H, 77, 74, 71, H, 68, H],
                [71, H, H, H, 74, H, 79, R],
            ],
            [
                [84, H, 83, 84, 79, H, 75, H],
                [80, H, H, 79, 75, H, 72, R],
                [80, H, 77, 76, 77, H, 72, H],
                [83, H, 82, 79, 74, H, 71, H],
                [82, H, H, 79, 75, H, 70, R],
                [79, H, 75, 72, 67, H, 63, H],
                [74, H, 71, 68, 65, 62, 65, H],
                [71, H, H, H, 67, H, 62, R],
            ],
        ],
    )
}

fn orbital_shipyard() -> Cue {
    // A-minor work-song rhythm over short industrial bass stabs. The opening
    // jumps down a sixth; the return turns it upward without recycling a span.
    space_composed(
        "Rivets in Orbit",
        5,
        SpaceTexture::Shipyard,
        [
            [AM, F, C, G, AM, D, FSS, B7],
            [AM, EM, F, C, D, AM, FSS, B7],
            [C, G, F, AM, C, D, G, B7],
            [AM, F, C, G, D, AM, FSS, B7],
        ],
        [
            [
                [81, 72, H, 76, 72, H, 69, 72],
                [77, 69, H, 72, 69, H, 65, R],
                [79, 72, H, 76, 79, H, 76, 72],
                [79, 71, H, 74, 71, H, 67, 71],
                [81, 76, H, 72, 69, 72, 76, H],
                [78, 74, H, 69, 66, 69, 74, H],
                [78, 72, H, 69, 66, H, 69, 72],
                [75, H, 71, 66, 63, H, H, R],
            ],
            [
                [72, 76, 81, H, 79, 76, 72, H],
                [71, 76, 79, H, 76, 71, 67, H],
                [69, 72, 77, H, 76, 72, 69, H],
                [67, 72, 76, H, 79, H, 76, R],
                [69, 74, 78, H, 81, H, 78, 74],
                [72, 76, 81, 76, 72, H, 69, H],
                [78, 74, 72, H, 69, H, 66, 69],
                [78, 75, 71, H, 66, H, 63, R],
            ],
            [
                [76, H, H, H, 79, H, 84, H],
                [83, H, 79, H, 74, H, H, H],
                [81, H, H, H, 77, H, 72, H],
                [76, H, H, 72, 69, H, 67, R],
                [72, H, 76, H, 79, H, 84, H],
                [81, H, 78, 74, 69, H, H, H],
                [79, H, 74, H, 71, H, 69, 67],
                [75, H, 71, H, 66, H, H, R],
            ],
            [
                [81, 84, H, 88, 84, H, 81, 76],
                [81, 77, H, 72, 69, H, 72, 77],
                [84, 79, H, 76, 72, 76, 79, H],
                [83, 79, H, 74, 71, H, 74, R],
                [81, 78, 74, H, 69, 74, 78, H],
                [81, 79, 76, H, 72, H, 69, 72],
                [78, H, 74, 72, 69, 66, 69, H],
                [75, 78, 83, H, 78, 75, 71, R],
            ],
        ],
    )
}

fn space_carrier() -> Cue {
    // Heavy C-minor dotted subject, ascending resistance, E-flat release and
    // a high command-deck reprise; distinct from the reused terrestrial boss.
    space_composed(
        "Shadow of the Dreadnought",
        5,
        SpaceTexture::Dreadnought,
        [
            [CM, CM, AB, BF, FM, CMB, DS, G7],
            [CM, EF, FM, AB, DS, G7, CM, G7],
            [EF, BF, AB, CM, FM, EF, DS, G7],
            [CM, AB, EF, BF, FM, CM, DS, G7],
        ],
        [
            [
                [67, H, H, 72, 75, 72, 67, H],
                [66, 67, H, 72, 79, H, 75, 72],
                [68, H, H, 75, 80, 79, 75, H],
                [70, H, H, 77, 82, H, 77, R],
                [72, H, 68, 65, 77, H, 72, 68],
                [67, H, 72, 75, 79, H, 75, 72],
                [68, H, 71, 74, 77, H, 74, 71],
                [71, H, H, 74, 79, H, 74, R],
            ],
            [
                [72, 75, 79, H, 84, H, 83, 79],
                [75, 79, 82, H, 87, H, 84, 82],
                [77, 80, 84, H, 87, 84, 80, H],
                [80, 84, 87, H, 84, H, 80, 75],
                [77, 74, 71, H, 68, H, 65, 62],
                [74, 79, 83, H, 86, H, 83, 79],
                [84, 79, 75, H, 72, H, 70, 67],
                [83, H, 79, 74, 71, H, 67, R],
            ],
            [
                [82, H, H, H, 79, H, 75, H],
                [81, H, 77, H, 74, H, H, H],
                [80, H, H, H, 75, H, 72, H],
                [79, H, 75, H, 72, H, 70, R],
                [80, H, H, H, 77, H, 72, H],
                [79, H, 82, H, 87, H, 84, 82],
                [80, H, 77, 74, 71, H, 68, H],
                [79, H, H, H, 74, H, 71, R],
            ],
            [
                [79, H, H, 84, 87, 84, 79, H],
                [80, H, H, 84, 87, 84, 80, H],
                [82, 79, 75, H, 79, H, 82, 87],
                [86, 82, 77, H, 74, H, 77, R],
                [84, H, 80, 77, 72, H, 77, 80],
                [87, 84, 79, H, 75, H, 72, 67],
                [80, 77, 74, H, 71, 68, 65, H],
                [83, H, 79, H, 74, 71, 67, R],
            ],
        ],
    )
}

fn jump_gate() -> Cue {
    // E-major awe with a C-sharp-minor response: the gate's promise is audible
    // before the guardian twists the tonal centre back into E minor.
    space_composed(
        "Beyond the Blue Threshold",
        6,
        SpaceTexture::Gate,
        [
            [E, A, CSM, B, E, FSM, A, B],
            [CSM, FSM, A, E, FSM, CSM, A, B],
            [A, E, FSM, CSM, A, E, B, B7],
            [E, CSM, A, B, FSM, A, B, E],
        ],
        [
            [
                [76, H, 80, 83, H, 80, 76, H],
                [81, H, 85, H, 83, 81, 76, R],
                [80, H, 85, 88, H, 85, 83, 80],
                [78, H, 83, H, 81, 78, 75, H],
                [80, 83, H, 88, 83, H, 80, 76],
                [81, H, 78, 73, 69, H, 73, 78],
                [81, H, 85, 88, 85, H, 83, 81],
                [83, H, 78, 75, 71, H, H, R],
            ],
            [
                [88, H, 85, 83, 80, H, 76, 73],
                [85, H, 81, 78, 73, H, 69, R],
                [88, 85, 81, H, 76, H, 73, 76],
                [83, 80, 76, H, 71, H, 68, 71],
                [78, 81, 85, H, 90, H, 85, 81],
                [88, H, 85, 80, 76, H, 73, 76],
                [85, 83, 81, H, 76, H, 73, 69],
                [75, 78, 83, H, 81, 78, 75, R],
            ],
            [
                [81, H, H, H, 85, H, 88, H],
                [83, H, 80, H, 76, H, H, H],
                [78, H, H, H, 81, H, 85, H],
                [83, H, 80, 76, 73, H, H, R],
                [76, H, H, 81, 85, H, 88, H],
                [87, H, 83, H, 80, H, 76, H],
                [78, H, 83, H, 87, H, 85, 83],
                [81, H, 78, 75, 71, H, H, R],
            ],
            [
                [88, H, 83, 80, H, 83, 88, H],
                [85, H, 88, H, 85, 83, 80, R],
                [88, 85, H, 81, 76, H, 81, 85],
                [87, H, 83, 78, 75, H, 78, 83],
                [85, H, 81, 78, 73, H, 78, 81],
                [88, H, 85, 81, 76, 73, 76, H],
                [87, 83, 78, H, 75, H, 78, 83],
                [80, H, 76, H, 71, H, 68, R],
            ],
        ],
    )
}

fn ascent() -> Cue {
    // Five bars at five ticks per sixteenth are exactly 400 audio ticks:
    // lifting away, two cloud layers, the curved horizon, then open stars.
    let harmony = [CM, EF, AB, B7, EM];
    let lead = [
        [60, H, 63, H, 67, 72, 75, H],
        [70, H, 75, 79, 82, H, 79, 75],
        [80, H, 84, 87, 84, H, 80, 75],
        [78, 81, 83, H, 87, H, 83, 78],
        [88, H, H, 83, 79, H, H, R],
    ];
    let bars = (0..5)
        .map(|index| {
            arrange_space(
                harmony[index],
                lead[index],
                SpaceTexture::Ascent,
                index,
                index.min(3),
            )
        })
        .collect();
    Cue {
        name: "Atmosphere to Starlight",
        row_ticks: 5,
        bars,
    }
}

fn gate_guardian() -> Cue {
    // Urgent E-minor asymmetric hook, chromatic dominant answer, a lyrical
    // final-flight recollection and a rewritten ascent into the last attack.
    space_composed(
        "The Last Sentinel",
        5,
        SpaceTexture::Guardian,
        [
            [EM, C, AM, B7, EM, G, FSS, B7],
            [AM, D, G, C, FSS, B7, EM, B7],
            [G, D, C, EM, AM, C, FSS, B7],
            [EM, C, G, D, AM, EM, FSS, B7],
        ],
        [
            [
                [76, 79, H, 78, 76, H, 71, 75],
                [76, 79, H, 84, 83, 79, 76, R],
                [81, 76, H, 72, 76, H, 79, 81],
                [83, 82, 78, H, 75, H, 71, 75],
                [83, 79, 76, H, 71, H, 76, 79],
                [86, 83, 79, H, 74, H, 79, R],
                [84, 81, 78, H, 74, 72, 69, H],
                [83, 78, 75, H, 71, H, 66, R],
            ],
            [
                [81, H, 84, 88, 84, 81, 76, H],
                [81, H, 86, 90, 86, 81, 78, H],
                [83, H, 86, 91, 86, 83, 79, H],
                [84, H, 88, 91, 88, 84, 79, R],
                [84, 81, 78, H, 72, 69, 66, H],
                [87, 83, 78, H, 75, H, 71, 75],
                [88, 86, 83, H, 79, 76, 74, H],
                [87, H, 83, 81, 78, 75, 71, R],
            ],
            [
                [83, H, H, H, 79, H, 74, H],
                [81, H, 78, H, 74, H, H, H],
                [79, H, H, 76, 72, H, 71, 67],
                [76, H, H, H, 71, H, 67, R],
                [81, H, 76, H, 72, H, 69, H],
                [79, H, H, 76, 72, H, 67, H],
                [78, H, 74, 72, 69, H, 66, H],
                [75, H, 78, H, 83, H, H, R],
            ],
            [
                [88, 91, H, 90, 88, H, 83, 79],
                [84, 88, 91, H, 88, H, 84, 79],
                [86, 83, 79, H, 74, 79, 83, H],
                [90, 86, 81, H, 78, H, 74, R],
                [88, 84, 81, H, 76, H, 72, 69],
                [83, 79, 76, H, 71, 67, 71, H],
                [78, 81, 84, H, 81, 78, 74, 72],
                [75, H, 78, 83, 78, H, 75, R],
            ],
        ],
    )
}

/// Stable runtime cue order: original thirteen, six space worlds, ascent, boss.
/// This always returns all ten authored parts, independently of build profile.
pub fn cues() -> Vec<Cue> {
    vec![
        title(),
        lowland(),
        canyon(),
        fortress(),
        carrier(),
        victory(),
        defeat(),
        crystal_current(),
        ashes_and_steel(),
        tempest_drive(),
        gorge_warden(),
        citadel_heart(),
        carrier_overdrive(),
        asteroid_belt(),
        shattered_moon(),
        ship_graveyard(),
        orbital_shipyard(),
        space_carrier(),
        jump_gate(),
        ascent(),
        gate_guardian(),
    ]
}

/// Encode the same composition for the selected two- or four-SID executable.
/// All profile stripping happens here; the four core parts are byte-identical.
pub fn score_bank() -> Vec<u8> {
    use crate::music::{
        self, BANK_HEADER_SIZE, EVENT_OFFSET, INSTRUMENT_OFFSET, MUSIC_VOICES, PAGE_SIZE, SID_COUNT,
    };
    let suite = cues();
    let mut bank = vec![0; BANK_HEADER_SIZE];
    bank[..4].copy_from_slice(b"SMU3");
    bank[4] = SID_COUNT as u8;
    bank[5] = MUSIC_VOICES as u8;
    bank[6] = CUE_COUNT as u8;
    bank[12..14].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
    for (cue_index, cue) in suite.iter().enumerate() {
        let start = bank.len();
        for (bar_index, bar) in cue.bars.iter().enumerate() {
            let mut page = [0u8; PAGE_SIZE];
            page[..4].copy_from_slice(b"SPG2");
            page[4] = cue_index as u8;
            page[5] = bar_index as u8;
            page[6] = ((bar_index + 1) % cue.bars.len()) as u8;
            page[7] = cue.row_ticks;
            page[8] = MUSIC_VOICES as u8;
            for voice in 0..MUSIC_VOICES {
                let at = INSTRUMENT_OFFSET + voice * 5;
                page[at..at + 5].copy_from_slice(&bar.instruments[voice]);
            }
            for row in 0..ROWS_PER_BAR {
                let at = EVENT_OFFSET + row * MUSIC_VOICES;
                page[at..at + MUSIC_VOICES].copy_from_slice(&bar.rows[row][..MUSIC_VOICES]);
            }
            let checksum = music::page_checksum(&page);
            page[10..12].copy_from_slice(&checksum.to_le_bytes());
            bank.extend_from_slice(&page);
        }
        let checksum = crate::assets::checksum(&bank[start..]);
        let entry = 16 + cue_index * 8;
        bank[entry..entry + 4].copy_from_slice(&(start as u32).to_le_bytes());
        bank[entry + 4] = cue.bars.len() as u8;
        bank[entry + 5] = cue.row_ticks;
        bank[entry + 6..entry + 8].copy_from_slice(&checksum.to_le_bytes());
    }
    let length = bank.len() as u32;
    assert!(
        length <= music::BANK_CAPACITY,
        "the suite must fit its 256 KiB REU bank"
    );
    bank[8..12].copy_from_slice(&length.to_le_bytes());
    let checksum = music::bank_header_checksum(&bank[..BANK_HEADER_SIZE]);
    bank[14..16].copy_from_slice(&checksum.to_le_bytes());
    bank
}
