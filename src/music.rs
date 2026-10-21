//! Versioned, bounded score pages shared by the host composer and C64 loader.
//! Validation runs in the foreground AUDIO overlay, never in the interrupt.
//!
//! Page header byte 9 is the bar's filter cutoff target (SID register 22);
//! zero leaves the filter untouched, so older pages keep their exact bytes.
//! Instrument control bytes carry one waveform plus optional sync/ring bits.
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
pub const FILTER_OFFSET: usize = 9;
pub const SYNC: u8 = 0x02;
pub const RING: u8 = 0x04;
/// Per-song kernel images follow an `SMK1` directory: a 16-byte header with
/// its own checksum, one four-byte `size, checksum` entry per cue, and a
/// whole-directory checksum at the end. Boot verifies everything; a cue change
/// fetches only the header and its entry, which fit inside the kernel slot.
pub const KERNEL_DIRECTORY_SIZE: usize = 256;
pub const KERNEL_HEADER_SIZE: usize = 16;
pub const KERNEL_ENTRY_SIZE: usize = 4;

/// One tonal or noise waveform; sync and ring modulation only colour tonal
/// waveforms, and ring modulation only has an audible meaning on triangle.
/// The gate and test bits always belong to the player, never to the score.
pub const fn valid_control(control: u8) -> bool {
    let wave = control & 0xf0;
    let flags = control & 0x0f;
    matches!(wave, 0x10 | 0x20 | 0x40 | 0x80)
        && flags & !(SYNC | RING) == 0
        && (flags == 0 || wave != 0x80)
        && (flags & RING == 0 || wave == 0x10)
}

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
    if bytes[4] != cue || bytes[5] != index || cue >= CUE_COUNT as u8 {
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
        if !valid_control(bytes[at]) || bytes[at + 2] > 15 {
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

/// Validate the sixteen-byte kernel directory header against the resident
/// slot capacity. The header checksum treats its own field as zero.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
pub fn kernel_header(header: &[u8], slot: u16) -> Result<(), ScoreError> {
    if header.len() < KERNEL_HEADER_SIZE {
        return Err(ScoreError::Truncated);
    }
    if &header[..4] != b"SMK1" {
        return Err(ScoreError::Signature);
    }
    if header[4] != CUE_COUNT as u8 || header[5] != 0 {
        return Err(ScoreError::Bounds);
    }
    if u16_at(header, 6) != slot {
        return Err(ScoreError::Profile);
    }
    if checksum_excluding(&header[..KERNEL_HEADER_SIZE], 14) != u16_at(header, 14) {
        return Err(ScoreError::Checksum);
    }
    Ok(())
}

/// Decode one cue's `size, checksum` entry; a zero size means no kernel.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio")]
pub fn kernel_entry(entry: &[u8], slot: u16) -> Result<(u16, u16), ScoreError> {
    if entry.len() < KERNEL_ENTRY_SIZE {
        return Err(ScoreError::Truncated);
    }
    let size = u16_at(entry, 0);
    if size > slot {
        return Err(ScoreError::Bounds);
    }
    Ok((size, u16_at(entry, 2)))
}

/// Boot-time check of the complete directory, including its final checksum.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.startup")]
pub fn kernel_directory(directory: &[u8], slot: u16) -> Result<(), ScoreError> {
    if directory.len() < KERNEL_DIRECTORY_SIZE {
        return Err(ScoreError::Truncated);
    }
    kernel_header(directory, slot)?;
    if crate::assets::checksum(&directory[..KERNEL_DIRECTORY_SIZE - 2])
        != u16_at(directory, KERNEL_DIRECTORY_SIZE - 2)
    {
        return Err(ScoreError::Checksum);
    }
    for cue in 0..CUE_COUNT {
        let at = KERNEL_HEADER_SIZE + cue * KERNEL_ENTRY_SIZE;
        kernel_entry(&directory[at..at + KERNEL_ENTRY_SIZE], slot)?;
    }
    if directory[KERNEL_HEADER_SIZE + CUE_COUNT * KERNEL_ENTRY_SIZE..KERNEL_DIRECTORY_SIZE - 2]
        .iter()
        .any(|&x| x != 0)
    {
        return Err(ScoreError::Bounds);
    }
    Ok(())
}

/// The REU offset of one cue's kernel image behind the directory. The cue
/// index times the slot size stays in 16 bits (at most 20 × 768).
pub const fn kernel_image_offset(base: u32, cue: u8, slot: u16) -> u32 {
    base + KERNEL_DIRECTORY_SIZE as u32 + (cue as u16 * slot) as u32
}
