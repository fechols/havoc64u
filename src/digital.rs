//! DLT4 descriptors shared by the host packer and bounded foreground decoder.
//! The checked manifest generates descriptors and the DMC1 directory together.

pub const REU_SIZE: u32 = 0x1000000;
pub const MUSIC_OFFSET: u32 = 0x100000;
pub const RING_OFFSET: u32 = 0xfe0000;
pub const RING_BYTES: u32 = 0x20000;
pub const MUSIC_CAPACITY: u32 = RING_OFFSET - MUSIC_OFFSET;
pub const HEADER_SIZE: usize = 32;
pub const BITS_PER_SAMPLE: u8 = 3;
pub const CODEBOOK_ENTRIES: usize = 8;
pub const CODEBOOK_BYTES: usize = CODEBOOK_ENTRIES * 3 * CHANNELS as usize;
pub const CRC_CHUNK_BYTES: usize = 1024;
/// Compatibility alias for the Galactic Dash asset, not the directory base.
pub const ASSET_OFFSET: u32 = GALACTIC_OFFSET;
const _: () = assert!(RING_OFFSET + RING_BYTES == REU_SIZE);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Metadata {
    /// Frames: one mono sample or one simultaneous stereo pair.
    pub total_samples: u32,
    pub stream_bytes: u32,
    pub samples_per_segment: u32,
    pub stream_offset: u32,
    pub crc_offset: u32,
    pub codebook_count: u16,
    pub sample_rate: u16,
    pub channels: u8,
}
impl Metadata {
    pub const EMPTY: Self = Self {
        total_samples: 0,
        stream_bytes: 0,
        samples_per_segment: 0,
        stream_offset: 0,
        crc_offset: 0,
        codebook_count: 0,
        sample_rate: 20_000,
        channels: 2,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Descriptor {
    pub asset_offset: u32,
    pub asset_bytes: u32,
    pub header_checksum: u16,
    pub codebook_checksum: u16,
    pub crc_table_checksum: u16,
    pub asset_checksum: u16,
    pub metadata: Metadata,
}
include!("digital_catalog.rs");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigitalError {
    Truncated,
    Signature,
    Profile,
    Bounds,
    Checksum,
}

/// Target validation uses manifest-generated, bounded extents rather than
/// performing address arithmetic using untrusted header fields.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.boot")]
pub fn parse_header(bytes: &[u8], track: Track) -> Result<Metadata, DigitalError> {
    use crate::assets::{checksum, u16_at, u32_at};
    let descriptor = descriptor_startup(track);
    let metadata = &descriptor.metadata;
    if bytes.len() < HEADER_SIZE {
        return Err(DigitalError::Truncated);
    }
    if &bytes[..4] != b"DLT4" {
        return Err(DigitalError::Signature);
    }
    if u32_at(bytes, 4) != metadata.sample_rate as u32
        || u16_at(bytes, 16) != CODEBOOK_ENTRIES as u16
        || u16_at(bytes, 18) != metadata.codebook_count
        || bytes[20] != BITS_PER_SAMPLE
        || bytes[21] != metadata.channels
    {
        return Err(DigitalError::Profile);
    }
    if u32_at(bytes, 8) != metadata.total_samples
        || u32_at(bytes, 12) != metadata.stream_bytes
        || bytes[22..HEADER_SIZE].iter().any(|&byte| byte != 0)
    {
        return Err(DigitalError::Bounds);
    }
    if checksum(&bytes[..HEADER_SIZE]) != descriptor.header_checksum {
        return Err(DigitalError::Checksum);
    }
    Ok(*metadata)
}

/// Round only the published sample; predictors retain their full precision.
#[inline(always)]
pub fn rounded_sample(sample: i16) -> i8 {
    (((sample as i32 + 128) >> 8).min(127)) as i8
}

const fn crc_table() -> [u8; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u8;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc << 1) ^ if crc & 128 != 0 { 7 } else { 0 };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}
#[cfg_attr(target_arch = "mos", link_section = ".phase.audio.crc_table")]
static CRC8: [u8; 256] = crc_table();

#[inline(always)]
pub fn crc_byte(crc: u8, byte: u8) -> u8 {
    CRC8[(crc ^ byte) as usize]
}

/// The source stores a full u16 magnitude, then clips to the target's i16.
#[inline(always)]
pub fn decode_delta_entry(bytes: [u8; 3]) -> i16 {
    let magnitude = u16::from_le_bytes([bytes[0], bytes[1]]);
    if bytes[2] != 0 {
        if magnitude >= 0x8000 {
            i16::MIN
        } else {
            -(magnitude as i16)
        }
    } else {
        magnitude.min(i16::MAX as u16) as i16
    }
}

/// Host validates the entire stored asset before baking or accepting a package.
#[cfg(feature = "host-tools")]
pub fn validate_asset(bytes: &[u8], track: Track) -> Result<Metadata, DigitalError> {
    use crate::assets::checksum;
    let descriptor = descriptor(track);
    let metadata = parse_header(bytes, track)?;
    if bytes.len() != descriptor.asset_bytes as usize {
        return Err(DigitalError::Bounds);
    }
    let used_bits = (metadata.total_samples * metadata.channels as u32 * 3) & 7;
    if used_bits != 0 && bytes[metadata.crc_offset as usize - 1] >> used_bits != 0 {
        return Err(DigitalError::Bounds);
    }
    let codebooks = &bytes[HEADER_SIZE..metadata.stream_offset as usize];
    let crc_table = &bytes[metadata.crc_offset as usize..];
    if checksum(codebooks) != descriptor.codebook_checksum
        || checksum(crc_table) != descriptor.crc_table_checksum
        || checksum(bytes) != descriptor.asset_checksum
    {
        return Err(DigitalError::Checksum);
    }
    let mut crc = 0;
    for (chunk, &expected) in bytes[metadata.stream_offset as usize..metadata.crc_offset as usize]
        .chunks(CRC_CHUNK_BYTES)
        .zip(crc_table)
    {
        for &byte in chunk {
            crc = crc_byte(crc, byte);
        }
        if crc != expected {
            return Err(DigitalError::Checksum);
        }
    }
    Ok(metadata)
}

/// The host binds the packed directory, all assets and their zero padding to
/// the manifest used to compile the target. Mutable playback RAM is excluded.
#[cfg(feature = "host-tools")]
pub fn validate_catalog(reu: &[u8]) -> Result<(), DigitalError> {
    if reu.len() != REU_SIZE as usize {
        return Err(DigitalError::Bounds);
    }
    let mut end = MUSIC_OFFSET as usize + DIRECTORY_BYTES;
    if &reu[MUSIC_OFFSET as usize..end] != catalog_directory() {
        return Err(DigitalError::Checksum);
    }
    for track in Track::ALL {
        let descriptor = track.descriptor();
        let start = descriptor.asset_offset as usize;
        let next = start + descriptor.asset_bytes as usize;
        if start < end || next > RING_OFFSET as usize {
            return Err(DigitalError::Bounds);
        }
        if reu[end..start].iter().any(|&byte| byte != 0) {
            return Err(DigitalError::Bounds);
        }
        validate_asset(&reu[start..next], track)?;
        end = next;
    }
    if reu[end..RING_OFFSET as usize].iter().any(|&byte| byte != 0) {
        return Err(DigitalError::Bounds);
    }
    Ok(())
}
