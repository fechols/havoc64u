#![cfg(feature = "host-tools")]

use havoc::{
    assets,
    music::{self, ScoreError, BANK_HEADER_SIZE, EVENT_OFFSET, INSTRUMENT_OFFSET, PAGE_SIZE},
    soundtrack,
};

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn seal_page(page: &mut [u8]) {
    put16(page, 10, music::page_checksum(page));
}

fn seal_directory(bank: &mut [u8]) {
    let sum = music::bank_header_checksum(&bank[..BANK_HEADER_SIZE]);
    put16(bank, 14, sum);
}

#[test]
fn repaired_checksums_do_not_accept_invalid_page_fields() {
    let bank = soundtrack::score_bank();
    let good = &bank[BANK_HEADER_SIZE..BANK_HEADER_SIZE + PAGE_SIZE];
    for (offset, value, expected) in [
        (0, b'X', ScoreError::Signature),
        (4, music::CUE_COUNT as u8, ScoreError::Bounds),
        (5, 1, ScoreError::Bounds),
        (7, 2, ScoreError::Tempo),
        (8, 12, ScoreError::Profile),
        (9, 1, ScoreError::Bounds),
        (INSTRUMENT_OFFSET, 0x11, ScoreError::Instrument),
        (INSTRUMENT_OFFSET + 2, 16, ScoreError::Instrument),
        (EVENT_OFFSET, 2, ScoreError::Note),
        (EVENT_OFFSET, 0x81, ScoreError::Note),
        (EVENT_OFFSET, 96, ScoreError::Note),
    ] {
        let mut page = good.to_vec();
        page[offset] = value;
        seal_page(&mut page);
        assert_eq!(music::validate_page(&page, 0, 0), Err(expected));
    }
    assert_eq!(
        music::validate_page(&good[..PAGE_SIZE - 1], 0, 0),
        Err(ScoreError::Truncated)
    );
    let mut corrupt = good.to_vec();
    corrupt[EVENT_OFFSET] ^= 1;
    assert_eq!(
        music::validate_page(&corrupt, 0, 0),
        Err(ScoreError::Checksum)
    );
}

#[test]
fn bank_rejects_wrong_profile_truncation_gaps_and_bad_transport() {
    let good = soundtrack::score_bank();
    assert!(baked_assets::validate_soundtrack(&good).is_ok());
    assert!(baked_assets::validate_soundtrack(&good[..good.len() - 1]).is_err());
    let mut bad = good.clone();
    bad[4] = 6 - music::SID_COUNT as u8;
    seal_directory(&mut bad);
    assert_eq!(music::cue_info(&bad, 0).unwrap_err(), ScoreError::Profile);
    let mut bad = good.clone();
    bad[16] += 1;
    seal_directory(&mut bad);
    assert!(baked_assets::validate_soundtrack(&bad).is_err());

    // Repair every checksum so that only the semantic transport check can fail.
    let mut bad = good.clone();
    let pages = bad[20] as usize;
    let end = BANK_HEADER_SIZE + pages * PAGE_SIZE;
    bad[BANK_HEADER_SIZE + 6] = 0;
    seal_page(&mut bad[BANK_HEADER_SIZE..BANK_HEADER_SIZE + PAGE_SIZE]);
    let sum = assets::checksum(&bad[BANK_HEADER_SIZE..end]);
    put16(&mut bad, 22, sum);
    seal_directory(&mut bad);
    assert!(baked_assets::validate_soundtrack(&bad)
        .unwrap_err()
        .to_string()
        .contains("transport"));
}

#[test]
fn package_validation_authenticates_the_entire_cue_including_its_last_page() {
    let mut bank = soundtrack::score_bank();
    let last_event = bank.len() - PAGE_SIZE + EVENT_OFFSET;
    bank[last_event] ^= 1;
    assert!(baked_assets::validate_soundtrack(&bank)
        .unwrap_err()
        .to_string()
        .contains("checksum"));
}

#[test]
fn expanded_directory_authenticates_cues_beyond_the_old_header_and_rejects_smu2() {
    let good = soundtrack::score_bank();
    assert_eq!(BANK_HEADER_SIZE, 256);
    assert_eq!(&good[..4], b"SMU3");
    for cue in 14..music::CUE_COUNT as u8 {
        assert!(music::cue_info(&good, cue).is_ok());
    }
    let mut bad = good.clone();
    bad[16 + 20 * 8 + 4] ^= 1;
    assert_eq!(music::cue_info(&bad, 20).unwrap_err(), ScoreError::Checksum);
    let mut obsolete = good;
    obsolete[..4].copy_from_slice(b"SMU2");
    seal_directory(&mut obsolete);
    assert_eq!(
        music::cue_info(&obsolete, 0).unwrap_err(),
        ScoreError::Signature
    );
}

#[test]
fn cue_directory_preserves_high_reu_offsets_and_enforces_the_expanded_bank_bound() {
    let mut bank = soundtrack::score_bank();
    let last = (music::CUE_COUNT - 1) as u8;
    let entry = 16 + last as usize * 8;
    let pages = bank[entry + 4] as u32;
    // Exercise the loader's 32-bit directory path in both profiles, even
    // though only the four-SID production score crosses the 128 KiB boundary.
    let high_offset = 0x20000u32;
    let length = high_offset + pages * PAGE_SIZE as u32;
    bank[entry..entry + 4].copy_from_slice(&high_offset.to_le_bytes());
    bank[8..12].copy_from_slice(&length.to_le_bytes());
    seal_directory(&mut bank);
    let info = music::cue_info(&bank, last).unwrap();
    assert_eq!(info.offset, high_offset);
    assert_eq!(info.pages as u32, pages);

    let excessive = music::BANK_CAPACITY + 1;
    bank[8..12].copy_from_slice(&excessive.to_le_bytes());
    seal_directory(&mut bank);
    assert_eq!(
        music::cue_info(&bank, last).unwrap_err(),
        ScoreError::Bounds
    );

    bank[8..12].copy_from_slice(&music::BANK_CAPACITY.to_le_bytes());
    let overflowing = u32::MAX - PAGE_SIZE as u32;
    bank[entry..entry + 4].copy_from_slice(&overflowing.to_le_bytes());
    seal_directory(&mut bank);
    assert_eq!(
        music::cue_info(&bank, last).unwrap_err(),
        ScoreError::Bounds
    );
}
