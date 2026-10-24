use havoc::digital::*;

const ASSET: &[u8] = include_bytes!("../assets/music/galactic-dash.dlt");
const HAMMOND: &[u8] = include_bytes!("../assets/music/hammond-rocker.dlt");
const TRACKS: [(Track, &[u8]); Track::ALL.len()] = TRACK_ASSETS;

#[test]
fn shipped_tracks_have_exact_profile_bounds_and_all_stream_checksums() {
    let mut previous = MUSIC_OFFSET + DIRECTORY_BYTES as u32;
    for (track, bytes) in TRACKS {
        let descriptor = track.descriptor();
        let metadata = validate_asset(bytes, track).unwrap();
        assert_eq!(metadata, descriptor.metadata);
        assert_eq!(bytes.len() as u32, descriptor.asset_bytes);
        assert_eq!(&bytes[..4], b"DLT4");
        assert!(matches!(
            (metadata.sample_rate, metadata.channels),
            (20_000, 2) | (16_000, 1)
        ));
        assert_eq!(bytes[20], 3);
        assert_eq!(bytes[21], metadata.channels);
        assert_eq!(
            metadata.stream_offset,
            32 + metadata.codebook_count as u32 * 8 * 3 * metadata.channels as u32
        );
        assert_eq!(
            metadata.stream_bytes,
            (metadata.total_samples * metadata.channels as u32 * 3).div_ceil(8)
        );
        let alignment = 8 / metadata.channels as u32;
        assert_eq!(
            metadata.samples_per_segment,
            metadata.total_samples / metadata.codebook_count as u32 / alignment * alignment
        );
        assert_eq!(
            metadata.crc_offset,
            metadata.stream_offset + metadata.stream_bytes
        );
        assert_eq!(
            descriptor.asset_bytes - metadata.crc_offset,
            metadata.stream_bytes.div_ceil(1024)
        );
        assert!(descriptor.asset_offset >= previous);
        assert_eq!(descriptor.asset_offset & 15, 0);
        previous = descriptor.asset_offset + descriptor.asset_bytes;
    }
    assert_eq!(previous, CATALOG_END);
    assert!(CATALOG_END <= RING_OFFSET);
    assert_eq!(MUSIC_CAPACITY, 15_597_568);
    assert_eq!(RING_OFFSET + RING_BYTES, REU_SIZE);
    assert_eq!(
        Track::ALL[..3],
        [
            Track::GalacticDash,
            Track::HammondRocker,
            Track::GalacticDashMono
        ]
    );
    assert_eq!(
        track_for_cue(havoc::audio::MusicState::Title),
        Track::HammondRocker
    );
    assert_eq!(
        track_for_cue(havoc::audio::MusicState::Victory),
        Track::GalacticDash
    );
    assert_eq!(
        track_for_cue(havoc::audio::MusicState::Lowlands),
        Track::GalacticDashMono
    );
    for id in 0..=255 {
        assert_eq!(
            Track::from_id(id).is_some(),
            (id as usize) < Track::ALL.len()
        );
    }
}

#[test]
fn recorded_stages_override_shared_cues_and_special_scenes_keep_their_music() {
    use havoc::audio::MusicState;
    let stages = [
        (MusicState::Lowlands, Track::LowlandApproach),
        (MusicState::CrystalCurrent, Track::CrystalCave),
        (MusicState::Canyon, Track::IronCanyon),
        (MusicState::Canyon, Track::CanyonGate),
        (MusicState::AshesAndSteel, Track::AshenWastes),
        (MusicState::AshesAndSteel, Track::FoundryBelt),
        (MusicState::Fortress, Track::FortressRun),
    ];
    for (stage, (cue, track)) in stages.into_iter().enumerate() {
        assert_eq!(track_for_scene(cue, stage as u8), track);
        assert_eq!(track.descriptor().metadata.channels, 1);
    }
    assert_eq!(
        track_for_scene(MusicState::GorgeWarden, 3),
        Track::CanyonGate
    );
    for stage in 0..=255 {
        for cue in [
            MusicState::Title,
            MusicState::Victory,
            MusicState::Defeat,
            MusicState::Ascent,
        ] {
            assert_eq!(track_for_scene(cue, stage), track_for_cue(cue));
        }
    }
    for stage in 7..=255 {
        for cue in 0..havoc::audio::CUE_COUNT as u8 {
            let cue = MusicState::from_cue(cue).unwrap();
            assert_eq!(track_for_scene(cue, stage), track_for_cue(cue));
        }
    }
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
            // The low bit is used even in a partially filled final stream byte.
            bytes[index] ^= 0x01;
            assert_eq!(
                validate_asset(&bytes, track),
                Err(DigitalError::Checksum),
                "{track:?} byte {index}"
            );
        }
        let used_bits = (metadata.total_samples * metadata.channels as u32 * 3) % 8;
        if used_bits != 0 {
            let mut bytes = asset.to_vec();
            bytes[metadata.crc_offset as usize - 1] |= 0x80;
            assert_eq!(
                validate_asset(&bytes, track),
                Err(DigitalError::Bounds),
                "{track:?} nonzero stream padding"
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

#[test]
fn rounded_output_matches_mathematical_definition_for_every_predictor() {
    for sample in i16::MIN..=i16::MAX {
        let expected = ((sample as f64 / 256.0 + 0.5).floor() as i32).clamp(-128, 127);
        assert_eq!(rounded_sample(sample) as i32, expected, "{sample}");
    }
    assert_eq!(rounded_sample(32767), 127);
    assert_eq!(rounded_sample(-128), 0);
    assert_eq!(rounded_sample(-129), -1);
}

#[test]
fn directory_padding_and_packed_catalog_are_bound_to_compiled_descriptors() {
    let mut reu = vec![0; REU_SIZE as usize];
    let start = MUSIC_OFFSET as usize;
    let directory = catalog_directory();
    let stage_offset = 16 + 16 * Track::ALL.len() + havoc::audio::CUE_COUNT;
    assert_eq!(&directory[..4], b"DMC1");
    assert_eq!(directory[4], 2);
    assert_eq!(directory[10] as usize, STAGE_COUNT);
    assert_eq!(&directory[11..14], &[0; 3]);
    assert_eq!(
        &directory[stage_offset..stage_offset + 7],
        &[3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(
        &directory[stage_offset + 7..stage_offset + STAGE_COUNT],
        &[255; 11]
    );
    assert!(directory[stage_offset + STAGE_COUNT..]
        .iter()
        .all(|&byte| byte == 0));
    reu[start..start + DIRECTORY_BYTES].copy_from_slice(directory);
    for (track, bytes) in TRACKS {
        let start = track.descriptor().asset_offset as usize;
        reu[start..start + bytes.len()].copy_from_slice(bytes);
    }
    validate_catalog(&reu).unwrap();
    for offset in [
        MUSIC_OFFSET,
        MUSIC_OFFSET + 4,
        MUSIC_OFFSET + 10,
        MUSIC_OFFSET + 11,
        MUSIC_OFFSET + 12,
        MUSIC_OFFSET + 13,
        MUSIC_OFFSET + 14,
        MUSIC_OFFSET + stage_offset as u32,
        MUSIC_OFFSET + stage_offset as u32 + 6,
        MUSIC_OFFSET + stage_offset as u32 + 7,
        MUSIC_OFFSET + DIRECTORY_BYTES as u32 - 1,
        CATALOG_END,
        RING_OFFSET - 1,
    ] {
        reu[offset as usize] ^= 1;
        assert!(
            validate_catalog(&reu).is_err(),
            "accepted corrupt catalog byte {offset}"
        );
        reu[offset as usize] ^= 1;
    }
    // This check excludes mutable hardware playback memory by design.
    reu[RING_OFFSET as usize] = 127;
    reu[REU_SIZE as usize - 1] = 127;
    validate_catalog(&reu).unwrap();
}
