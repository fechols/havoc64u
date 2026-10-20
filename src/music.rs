//! Versioned, bounded score pages shared by the host composer and C64 loader.
//! Validation runs in the foreground AUDIO overlay, never in the interrupt.
use crate::assets::{u16_at, u32_at};

pub const SID_COUNT: usize = if cfg!(feature = "four-sid") { 4 } else { 2 };
pub const MUSIC_VOICES: usize = if cfg!(feature = "four-sid") { 10 } else { 4 };
pub const PAGE_ROWS: usize = 16;
pub const PAGE_SIZE: usize = if cfg!(feature = "four-sid") { 224 } else { 96 };
pub const INSTRUMENT_OFFSET: usize = 12;
pub const EVENT_OFFSET: usize = INSTRUMENT_OFFSET + MUSIC_VOICES * 5;
pub const BANK_HEADER_SIZE: usize = 256;
pub const BANK_CAPACITY: u32 = 0x40000;
pub const CUE_COUNT: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreError {
    Truncated,
    Signature,
    Profile,
    Bounds,
    Checksum,
    Tempo,
    Instrument,
    Note,
    Request,
    Transition,
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
fn checksum_excluding(bytes: &[u8], excluded: usize) -> u16 {
    let mut a = 0u16;
    let mut b = 0u16;
    for (index, &byte) in bytes.iter().enumerate() {
        if index != excluded && index != excluded + 1 {
            a += byte as u16;
        }
        if a >= 255 {
            a -= 255;
        }
        b += a;
        if b >= 255 {
            b -= 255;
        }
    }
    (b << 8) | a
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
pub fn page_checksum(bytes: &[u8]) -> u16 {
    checksum_excluding(bytes, 10)
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.startup")]
pub fn bank_header_checksum(bytes: &[u8]) -> u16 {
    checksum_excluding(bytes, 14)
}

#[derive(Clone, Copy, Debug)]
pub struct CueInfo {
    pub offset: u32,
    pub pages: u8,
    pub row_ticks: u8,
    pub checksum: u16,
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.startup")]
pub fn cue_info(header: &[u8], cue: u8) -> Result<CueInfo, ScoreError> {
    if header.len() < BANK_HEADER_SIZE {
        return Err(ScoreError::Truncated);
    }
    if &header[..4] != b"SMU3" {
        return Err(ScoreError::Signature);
    }
    if header[4] != SID_COUNT as u8
        || header[5] != MUSIC_VOICES as u8
        || u16_at(header, 12) != PAGE_SIZE as u16
    {
        return Err(ScoreError::Profile);
    }
    if header[6] != CUE_COUNT as u8 || header[7] != 0 || cue >= CUE_COUNT as u8 {
        return Err(ScoreError::Bounds);
    }
    if bank_header_checksum(&header[..BANK_HEADER_SIZE]) != u16_at(header, 14) {
        return Err(ScoreError::Checksum);
    }
    let at = 16 + cue as usize * 8;
    let info = CueInfo {
        offset: u32_at(header, at),
        pages: header[at + 4],
        row_ticks: header[at + 5],
        checksum: u16_at(header, at + 6),
    };
    let end = info
        .offset
        .checked_add(info.pages as u32 * PAGE_SIZE as u32)
        .ok_or(ScoreError::Bounds)?;
    let length = u32_at(header, 8);
    if info.pages == 0
        || info.offset < BANK_HEADER_SIZE as u32
        || end > length
        || length > BANK_CAPACITY
    {
        return Err(ScoreError::Bounds);
    }
    if !(3..=50).contains(&info.row_ticks) {
        return Err(ScoreError::Tempo);
    }
    Ok(info)
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
pub fn validate_page(bytes: &[u8], cue: u8, index: u8) -> Result<(), ScoreError> {
    if bytes.len() != PAGE_SIZE {
        return Err(ScoreError::Truncated);
    }
    if &bytes[..4] != b"SPG2" {
        return Err(ScoreError::Signature);
    }
    if bytes[4] != cue || bytes[5] != index || cue >= CUE_COUNT as u8 || bytes[9] != 0 {
        return Err(ScoreError::Bounds);
    }
    if bytes[8] != MUSIC_VOICES as u8 {
        return Err(ScoreError::Profile);
    }
    if page_checksum(bytes) != u16_at(bytes, 10) {
        return Err(ScoreError::Checksum);
    }
    if !(3..=50).contains(&bytes[7]) {
        return Err(ScoreError::Tempo);
    }
    for voice in 0..MUSIC_VOICES {
        let at = INSTRUMENT_OFFSET + voice * 5;
        if !matches!(bytes[at], 0x10 | 0x20 | 0x40 | 0x80) || bytes[at + 2] > 15 {
            return Err(ScoreError::Instrument);
        }
    }
    for &event in &bytes[EVENT_OFFSET..EVENT_OFFSET + PAGE_ROWS * MUSIC_VOICES] {
        if event > 1 && !(24..=95).contains(&(event & 0x7f)) {
            return Err(ScoreError::Note);
        }
    }
    if bytes[EVENT_OFFSET + PAGE_ROWS * MUSIC_VOICES..]
        .iter()
        .any(|&x| x != 0)
    {
        return Err(ScoreError::Bounds);
    }
    Ok(())
}
