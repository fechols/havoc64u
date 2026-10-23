use havoc::digital::*;

const ASSET: &[u8] = include_bytes!("../assets/music/galactic-dash.dlt");
const HAMMOND: &[u8] = include_bytes!("../assets/music/hammond-rocker.dlt");
const TRACKS: [(Track, &[u8]); 2] = [
    (Track::GalacticDash, ASSET),
    (Track::HammondRocker, HAMMOND),
];

#[test]
fn shipped_tracks_have_exact_profile_bounds_and_all_stream_checksums() {
    let metadata = validate_asset(ASSET, Track::GalacticDash).unwrap();
    assert_eq!(metadata.total_samples, 3_685_440);
    assert_eq!(metadata.samples_per_segment, 7_072);
    assert_eq!(metadata.stream_offset, 50_048);
    assert_eq!(metadata.crc_offset, 3_735_488);
    assert_eq!(metadata.codebook_count, 521);
    assert_eq!(ASSET.len(), 3_739_088);
    assert_eq!(ASSET_OFFSET + ASSET.len() as u32, 4_787_664);
    assert!(ASSET_OFFSET + (ASSET.len() as u32) < HAMMOND_OFFSET);
    let hammond = descriptor(Track::HammondRocker);
    let metadata = validate_asset(HAMMOND, Track::HammondRocker).unwrap();
    assert_eq!(metadata, hammond.metadata);
    assert_eq!(metadata.total_samples, 5_038_080);
    assert_eq!(metadata.samples_per_segment, 9_670);
    assert_eq!(metadata.stream_offset, 50_048);
    assert_eq!(metadata.stream_bytes, 5_038_080);
    assert_eq!(metadata.crc_offset, 5_088_128);
    assert_eq!(metadata.codebook_count, 521);
    assert_eq!(HAMMOND.len(), 5_093_048);
    assert_eq!(hammond.asset_offset, HAMMOND_OFFSET);
    assert_eq!(hammond.asset_offset + hammond.asset_bytes, 10_335_928);
    assert!(hammond.asset_offset + hammond.asset_bytes < RING_OFFSET);
    for (track, bytes) in TRACKS {
        let d = track.descriptor();
        assert_eq!(bytes.len() as u32, d.asset_bytes);
        assert_eq!(
            d.metadata.stream_offset,
            HEADER_SIZE as u32 + CODEBOOK_COUNT as u32 * CODEBOOK_BYTES as u32
        );
        assert_eq!(d.metadata.stream_bytes, d.metadata.total_samples);
        assert_eq!(&bytes[..4], b"DLT3");
        assert_eq!(bytes[21], CHANNELS);
        assert_eq!(CHANNELS, 2);
        assert_eq!(
            d.metadata.samples_per_segment,
            (d.metadata.total_samples / CODEBOOK_COUNT as u32 / 2) * 2
        );
        assert_eq!(
            d.metadata.crc_offset,
            d.metadata.stream_offset + d.metadata.stream_bytes
        );
        assert_eq!(
            d.asset_bytes - d.metadata.crc_offset,
            d.metadata.stream_bytes.div_ceil(CRC_CHUNK_BYTES as u32)
        );
    }
    assert_eq!(REU_SIZE, 16 * 1024 * 1024);
    assert_eq!(RING_OFFSET + RING_BYTES, REU_SIZE);
}

#[test]
fn malformed_or_different_track_headers_are_rejected_before_dma() {
    for (track, bytes) in TRACKS {
        for length in 0..HEADER_SIZE {
            assert!(parse_header(&bytes[..length], track).is_err());
        }
        for index in 0..HEADER_SIZE {
            let mut header = bytes[..HEADER_SIZE].to_vec();
            header[index] ^= 1;
            assert!(
                parse_header(&header, track).is_err(),
                "accepted changed {track:?} header byte {index}"
            );
        }
        let mut legacy = bytes[..HEADER_SIZE].to_vec();
        legacy[3] = b'2';
        assert_eq!(parse_header(&legacy, track), Err(DigitalError::Signature));
    }
    assert_eq!(
        parse_header(ASSET, Track::HammondRocker),
        Err(DigitalError::Bounds)
    );
    assert_eq!(
        parse_header(HAMMOND, Track::GalacticDash),
        Err(DigitalError::Bounds)
    );
    assert_eq!(
        validate_asset(ASSET, Track::HammondRocker),
        Err(DigitalError::Bounds)
    );
    assert_eq!(
        validate_asset(HAMMOND, Track::GalacticDash),
        Err(DigitalError::Bounds)
    );
}

#[test]
fn corrupt_codebook_stream_crc_and_extent_are_rejected() {
    for (track, asset) in TRACKS {
        let descriptor = track.descriptor();
        let metadata = descriptor.metadata;
        for index in [
            HEADER_SIZE,
            metadata.stream_offset as usize - 1,
            metadata.stream_offset as usize,
            metadata.stream_offset as usize + 1023,
            metadata.stream_offset as usize + 1024,
            metadata.crc_offset as usize - 1,
            metadata.crc_offset as usize,
            descriptor.asset_bytes as usize - 1,
        ] {
            let mut bytes = asset.to_vec();
            bytes[index] ^= 0x80;
            assert_eq!(
                validate_asset(&bytes, track),
                Err(DigitalError::Checksum),
                "{track:?} byte {index}"
            );
        }
        assert_eq!(
            validate_asset(&asset[..asset.len() - 1], track),
            Err(DigitalError::Bounds)
        );
        let mut long = asset.to_vec();
        long.push(0);
        assert_eq!(validate_asset(&long, track), Err(DigitalError::Bounds));
    }
}

#[test]
fn nibble_crc_matches_bit_reference_and_signed_deltas_saturate() {
    for previous in 0..=255u8 {
        for byte in 0..=255u8 {
            let mut reference = previous ^ byte;
            for _ in 0..8 {
                reference = (reference << 1) ^ if reference & 128 != 0 { 7 } else { 0 };
            }
            assert_eq!(crc_byte(previous, byte), reference);
        }
    }
    for magnitude in 0..=u16::MAX {
        for sign in [0u8, 1] {
            let signed = if sign == 0 {
                magnitude as i32
            } else {
                -(magnitude as i32)
            };
            assert_eq!(
                decode_delta_entry([magnitude as u8, (magnitude >> 8) as u8, sign]),
                signed.clamp(i16::MIN as i32, i16::MAX as i32) as i16
            );
        }
    }
}
