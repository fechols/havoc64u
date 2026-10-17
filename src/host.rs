//! Original assets and portable package tools. Never linked into the C64 game.
use havoc::{assets::*, music, soundtrack};
use std::{fs, path::Path};

fn put16(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
pub fn common_bank() -> [u8; COMMON_SIZE] {
    let mut b = [0; COMMON_SIZE];
    b[..4].copy_from_slice(b"SMA1");
    b[4] = 8;
    let mut cursor = 80;
    let models: Vec<(Vec<[i16; 3]>, Vec<[u8; 4]>)> = vec![
        // Kestrel: swept delta wings, raised cockpit, split aft fins.
        (
            vec![
                [0, 0, 36],
                [-36, -2, -16],
                [36, -2, -16],
                [0, 12, -3],
                [0, -7, -14],
                [-10, 0, -26],
                [10, 0, -26],
                [0, 8, -27],
            ],
            vec![
                [0, 1, 3, 15],
                [0, 3, 2, 1],
                [0, 4, 1, 12],
                [0, 2, 4, 12],
                [1, 5, 3, 14],
                [2, 3, 6, 14],
                [3, 5, 7, 6],
                [3, 7, 6, 6],
                [4, 5, 1, 11],
                [4, 2, 6, 11],
                [4, 6, 5, 11],
                [5, 6, 7, 8],
            ],
        ),
        (
            vec![
                [0, 0, -30],
                [-27, 0, 14],
                [27, 0, 14],
                [0, 10, 8],
                [0, -7, 8],
                [0, 0, 25],
            ],
            vec![
                [0, 1, 3, 2],
                [0, 3, 2, 10],
                [0, 4, 1, 9],
                [0, 2, 4, 9],
                [1, 5, 3, 8],
                [3, 5, 2, 10],
                [1, 4, 5, 11],
                [2, 5, 4, 11],
            ],
        ),
        (
            vec![
                [0, 0, -32],
                [-18, 0, 20],
                [18, 0, 20],
                [0, 18, 13],
                [0, -10, 13],
            ],
            vec![
                [0, 1, 3, 4],
                [0, 3, 2, 10],
                [0, 4, 1, 6],
                [0, 2, 4, 6],
                [1, 4, 3, 12],
                [2, 3, 4, 12],
            ],
        ),
        (
            vec![
                [-20, -8, -20],
                [20, -8, -20],
                [20, -8, 20],
                [-20, -8, 20],
                [-12, 12, -12],
                [12, 12, -12],
                [12, 12, 12],
                [-12, 12, 12],
                [0, 18, -28],
            ],
            vec![
                [0, 4, 1, 12],
                [1, 4, 5, 15],
                [1, 5, 2, 11],
                [2, 5, 6, 12],
                [2, 6, 3, 11],
                [3, 6, 7, 12],
                [3, 7, 0, 11],
                [0, 7, 4, 12],
                [4, 7, 6, 15],
                [4, 6, 5, 15],
                [4, 8, 5, 2],
            ],
        ),
        (
            vec![
                [-85, -16, -70],
                [85, -16, -70],
                [70, -16, 80],
                [-70, -16, 80],
                [-60, 15, -45],
                [60, 15, -45],
                [45, 15, 65],
                [-45, 15, 65],
                [0, 38, 30],
                [0, 0, -86],
            ],
            vec![
                [0, 4, 1, 12],
                [1, 4, 5, 15],
                [1, 5, 2, 11],
                [2, 5, 6, 12],
                [2, 6, 3, 12],
                [3, 6, 7, 15],
                [3, 7, 0, 11],
                [0, 7, 4, 12],
                [4, 7, 8, 14],
                [7, 6, 8, 6],
                [6, 5, 8, 14],
                [5, 4, 8, 15],
                [0, 1, 9, 2],
                [0, 9, 4, 10],
                [1, 5, 9, 10],
                [0, 3, 2, 11],
                [0, 2, 1, 11],
            ],
        ),
        (
            vec![
                [0, 20, 0],
                [-13, 0, 0],
                [0, 0, -13],
                [13, 0, 0],
                [0, 0, 13],
                [0, -20, 0],
            ],
            vec![
                [0, 1, 2, 3],
                [0, 2, 3, 1],
                [0, 3, 4, 7],
                [0, 4, 1, 13],
                [5, 2, 1, 6],
                [5, 3, 2, 14],
                [5, 4, 3, 8],
                [5, 1, 4, 5],
            ],
        ),
        (
            vec![
                [-22, 0, -22],
                [22, 0, -22],
                [22, 0, 22],
                [-22, 0, 22],
                [-22, 60, -22],
                [22, 60, -22],
                [22, 60, 22],
                [-22, 60, 22],
            ],
            vec![
                [0, 4, 1, 12],
                [1, 4, 5, 15],
                [1, 5, 2, 11],
                [2, 5, 6, 12],
                [2, 6, 3, 11],
                [3, 6, 7, 12],
                [3, 7, 0, 11],
                [0, 7, 4, 12],
                [4, 7, 6, 15],
                [4, 6, 5, 15],
            ],
        ),
        (
            vec![
                [-32, 0, -25],
                [28, 0, -30],
                [35, 0, 28],
                [-26, 0, 30],
                [-8, 42, 0],
                [15, 27, -12],
            ],
            vec![
                [0, 4, 1, 9],
                [1, 4, 5, 8],
                [1, 5, 2, 12],
                [2, 5, 4, 15],
                [2, 4, 3, 12],
                [3, 4, 0, 11],
            ],
        ),
    ];
    for (id, (vertices, faces)) in models.iter().enumerate() {
        let d = 16 + id * 8;
        put16(&mut b, d, cursor as u16);
        b[d + 2] = vertices.len() as u8;
        b[d + 3] = faces.len() as u8;
        for vertex in vertices {
            for v in vertex {
                put16(&mut b, cursor, *v as u16);
                cursor += 2;
            }
        }
        put16(&mut b, d + 4, cursor as u16);
        for face in faces {
            b[cursor..cursor + 4].copy_from_slice(face);
            cursor += 4;
        }
    }
    assert!(cursor <= MODEL_SIZE, "model bank exceeds common arena");
    let score = soundtrack::score_bank();
    b[MODEL_SIZE..MODEL_SIZE + music::BANK_HEADER_SIZE]
        .copy_from_slice(&score[..music::BANK_HEADER_SIZE]);
    AssetBank::new(&b).expect("generated common bank");
    b
}

pub fn segment_bank(index: u8) -> [u8; SEGMENT_SIZE] {
    let mut b = [0; SEGMENT_SIZE];
    b[..4].copy_from_slice(b"SMS1");
    b[4] = index;
    b[5] = index;
    b[6] = [5, 9, 11, 6][index as usize];
    let mut events: Vec<Spawn> = Vec::new();
    if index < 3 {
        for wave in 0..10u16 {
            let base = 1200 + wave * 2700;
            for lane in 0..3i16 {
                events.push(Spawn {
                    distance: base + lane as u16 * 100,
                    kind: if wave % 3 == 2 { DIVER } else { FIGHTER },
                    x: (lane - 1) * 80,
                    y: if wave % 2 == 0 { 35 } else { -25 },
                    flags: if wave % 4 == 0 && lane == 1 { 1 } else { 0 },
                });
            }
            if wave % 2 == 1 {
                events.push(Spawn {
                    distance: base + 1200,
                    kind: TURRET,
                    x: if wave % 4 == 1 { -95 } else { 95 },
                    y: -90,
                    flags: 0,
                });
            }
        }
        for i in 0..6u16 {
            events.push(Spawn {
                distance: 2200 + i * 4300,
                kind: if index == 2 { BUILDING } else { ROCK },
                x: if i % 2 == 0 { -70 } else { 70 },
                y: -110,
                flags: 0,
            });
        }
    }
    assert!(
        events.len() <= MAX_SEGMENT_SPAWNS as usize,
        "encounters exceed the runtime segment cache"
    );
    events.sort_by_key(|e| e.distance);
    b[7] = events.len() as u8;
    for (i, e) in events.iter().enumerate() {
        let p = 16 + i * 8;
        put16(&mut b, p, e.distance);
        b[p + 2] = e.kind;
        b[p + 3] = e.flags;
        put16(&mut b, p + 4, e.x as u16);
        put16(&mut b, p + 6, e.y as u16);
    }
    Segment::new(&b).expect("generated segment");
    b
}

pub fn bake(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(out)?;
    let common = common_bank();
    let mut reu = vec![0; REU_SIZE as usize];
    reu[..4].copy_from_slice(b"SM64");
    put32(&mut reu, 4, PACKAGE_ID);
    put32(&mut reu, 8, COMMON_OFFSET);
    put32(&mut reu, 12, SEGMENTS_OFFSET);
    reu[16] = SEGMENT_COUNT;
    reu[17] = music::SID_COUNT as u8;
    put16(&mut reu, 18, checksum(&common));
    reu[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE].copy_from_slice(&common);
    let score = soundtrack::score_bank();
    reu[MUSIC_OFFSET as usize..MUSIC_OFFSET as usize + score.len()].copy_from_slice(&score);
    for i in 0..4u8 {
        let segment = segment_bank(i);
        let p = SEGMENTS_OFFSET as usize + i as usize * SEGMENT_SIZE;
        reu[p..p + SEGMENT_SIZE].copy_from_slice(&segment);
        put16(
            &mut reu,
            20 + i as usize * 2,
            checksum(&segment[..SEGMENT_CACHE_SIZE]),
        );
    }
    let check = checksum(&reu[..30]);
    put16(&mut reu, 30, check);
    check_package(&reu)?;
    fs::write(out.join("havoc.reu"), &reu)?;
    fs::write(
        out.join("havoc.cfg"),
        if cfg!(feature = "four-sid") {
            include_str!("../config/havoc-4sid.cfg")
        } else {
            include_str!("../config/havoc.cfg")
        },
    )?;
    fs::write(out.join("assets.txt"),format!("H.A.V.O.C. package {PACKAGE_ID:08X}\n16 MiB REU; common {COMMON_OFFSET:06X} (3072 bytes; 2048 resident model bytes); four 2048-byte segments at {SEGMENTS_OFFSET:06X}, each with a checksummed 512-byte runtime prefix (up to 62 encounters)\n{} SIDs: {} music voices + 2 effects voices. Seven original cues; {} bytes of paged score data at {MUSIC_OFFSET:06X}.\nOriginal models and music; no external ROMs or samples.\n", music::SID_COUNT, music::MUSIC_VOICES, score.len()))?;
    println!(
        "Baked {} ({} bytes)",
        out.join("havoc.reu").display(),
        reu.len()
    );
    Ok(())
}
pub fn check_package(reu: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if reu.len() != REU_SIZE as usize {
        return Err("REU must be exactly16MiB".into());
    }
    validate_header(reu).map_err(|e| format!("package header: {e:?}"))?;
    let common = &reu[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE];
    let bank = AssetBank::new(common).map_err(|e| format!("common bank: {e:?}"))?;
    let music_bank = &reu[MUSIC_OFFSET as usize..MUSIC_OFFSET as usize + 0x10000];
    if bank.audio()[..music::BANK_HEADER_SIZE] != music_bank[..music::BANK_HEADER_SIZE] {
        return Err("music directory does not match common assets".into());
    }
    validate_soundtrack(music_bank)?;
    if checksum(common) != u16_at(reu, 18) {
        return Err("common checksum".into());
    }
    for i in 0..4 {
        let p = SEGMENTS_OFFSET as usize + i * SEGMENT_SIZE;
        let data = &reu[p..p + SEGMENT_SIZE];
        let s = Segment::new(data).map_err(|e| format!("segment {i}: {e:?}"))?;
        if s.index() != i as u8 || checksum(&data[..SEGMENT_CACHE_SIZE]) != u16_at(reu, 20 + i * 2)
        {
            return Err("segment checksum/index".into());
        }
    }
    Ok(())
}

pub fn validate_soundtrack(bank: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut end = music::BANK_HEADER_SIZE;
    for cue in 0..music::CUE_COUNT as u8 {
        let info = music::cue_info(bank, cue).map_err(|e| format!("music directory: {e:?}"))?;
        if info.offset as usize != end {
            return Err("music page directory is not contiguous".into());
        }
        end += info.pages as usize * music::PAGE_SIZE;
        if end > bank.len() {
            return Err("truncated music bank".into());
        }
        let pages = &bank[info.offset as usize..end];
        if checksum(pages) != info.checksum {
            return Err("music cue checksum".into());
        }
        for (index, page) in pages.chunks_exact(music::PAGE_SIZE).enumerate() {
            music::validate_page(page, cue, index as u8)
                .map_err(|e| format!("music cue {cue} page {index}: {e:?}"))?;
            if page[6] != ((index + 1) % info.pages as usize) as u8 || page[7] != info.row_ticks {
                return Err("music page transport mismatch".into());
            }
            let previous = &pages
                [((index + info.pages as usize - 1) % info.pages as usize) * music::PAGE_SIZE..];
            for voice in 0..music::MUSIC_VOICES {
                let event = page[music::EVENT_OFFSET + voice];
                let at = music::INSTRUMENT_OFFSET + voice * 5;
                if (event == 1 || event & 128 != 0) && page[at..at + 5] != previous[at..at + 5] {
                    return Err("music changes instrument under a boundary tie".into());
                }
                if index == 0 && (event == 1 || event & 128 != 0) {
                    return Err("music opening needs an attack or rest".into());
                }
            }
        }
    }
    if u32_at(bank, 8) as usize != end {
        return Err("music bank length mismatch".into());
    }
    Ok(())
}
