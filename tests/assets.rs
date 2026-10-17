use sueme64::assets::*;

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

fn put16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn seal_header(header: &mut [u8]) {
    let sum = checksum(&header[..30]);
    put16(header, 30, sum);
}
fn header() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(b"SM64");
    put32(&mut bytes, 4, PACKAGE_ID);
    put32(&mut bytes, 8, COMMON_OFFSET);
    put32(&mut bytes, 12, SEGMENTS_OFFSET);
    bytes[16] = SEGMENT_COUNT;
    seal_header(&mut bytes);
    bytes
}
fn package() -> Vec<u8> {
    let mut bytes = vec![0u8; REU_SIZE as usize];
    bytes[..32].copy_from_slice(&header());
    let common = baked_assets::common_bank();
    let offset = COMMON_OFFSET as usize;
    bytes[offset..offset + COMMON_SIZE].copy_from_slice(&common);
    put16(&mut bytes, 18, checksum(&common));
    for index in 0..SEGMENT_COUNT {
        let segment = baked_assets::segment_bank(index);
        let offset = SEGMENTS_OFFSET as usize + index as usize * SEGMENT_SIZE;
        bytes[offset..offset + SEGMENT_SIZE].copy_from_slice(&segment);
        put16(
            &mut bytes,
            20 + index as usize * 2,
            checksum(&segment[..SEGMENT_CACHE_SIZE]),
        );
    }
    seal_header(&mut bytes);
    bytes
}

#[test]
fn header_rejects_wrong_size_signature_version_and_directory() {
    let good = header();
    assert_eq!(validate_header(&good), Ok(()));
    assert_eq!(validate_header(&good[..31]), Err(AssetError::Size));
    for (offset, value, expected) in [
        (0, b'X', AssetError::Magic),
        (4, 0, AssetError::Version),
        (8, 0xff, AssetError::Bounds),
        (12, 0xff, AssetError::Bounds),
        (16, 5, AssetError::Bounds),
    ] {
        let mut bytes = good;
        bytes[offset] = value;
        seal_header(&mut bytes);
        assert_eq!(validate_header(&bytes), Err(expected));
    }
    let mut corrupt = good;
    corrupt[18] ^= 1;
    assert_eq!(validate_header(&corrupt), Err(AssetError::Checksum));
}

#[test]
fn malformed_mesh_ranges_indices_and_colors_are_rejected() {
    let good = baked_assets::common_bank();
    assert!(AssetBank::new(&good).is_ok());
    assert!(matches!(
        AssetBank::new(&good[..COMMON_SIZE - 1]),
        Err(AssetError::Size)
    ));
    for (offset, value) in [(16, 0u16), (16, 0xfffe), (20, 0xffff), (20, 2048)] {
        let mut bad = good;
        put16(&mut bad, offset, value);
        assert!(AssetBank::new(&bad).is_err(), "bad offset {offset}={value}");
    }
    let mut bad = good;
    bad[18] = 129;
    assert!(matches!(AssetBank::new(&bad), Err(AssetError::Bounds)));
    let mut bad = good;
    bad[18] = 0;
    assert!(matches!(AssetBank::new(&bad), Err(AssetError::Bounds)));
    let face = u16_at(&good, 20) as usize;
    let mut bad = good;
    bad[face] = good[18];
    assert!(matches!(AssetBank::new(&bad), Err(AssetError::Index)));
    let mut bad = good;
    bad[face + 3] = 16;
    assert!(matches!(AssetBank::new(&bad), Err(AssetError::Index)));
}

#[test]
fn views_return_safe_empty_values_for_unknown_meshes_and_records() {
    let bytes = baked_assets::common_bank();
    let bank = AssetBank::new(&bytes).unwrap();
    assert_eq!(bank.vertex_count(255), 0);
    assert_eq!(bank.face_count(255), 0);
    assert_eq!(bank.vertex(255, 255), sueme64::Vec3::default());
    assert_eq!(bank.face(255, 255), ([0; 3], 0));
    let bytes = baked_assets::segment_bank(3);
    let segment = Segment::new(&bytes).unwrap();
    assert_eq!(segment.spawn(255).kind, 0);
}

#[test]
fn malformed_segment_metadata_and_encounter_order_are_rejected() {
    let good = baked_assets::segment_bank(0);
    assert!(matches!(
        Segment::new(&good[..SEGMENT_SIZE - 1]),
        Err(AssetError::Size)
    ));
    for (offset, value) in [
        (0, b'X'),
        (4, 4),
        (5, 4),
        (6, 16),
        (7, 201),
        (18, 0),
        (18, 4),
        (18, 8),
    ] {
        let mut bytes = good;
        bytes[offset] = value;
        assert!(Segment::new(&bytes).is_err(), "byte {offset}={value}");
    }
    let mut bad = good;
    put16(&mut bad, 16, 30_000);
    assert!(matches!(Segment::new(&bad), Err(AssetError::Bounds)));
    let mut bad = good;
    put16(&mut bad, 24, 0);
    assert!(matches!(Segment::new(&bad), Err(AssetError::Bounds)));
}

#[test]
fn dma_chunks_stop_at_reu_pages_cpu_wrap_and_end_of_installed_ram() {
    assert_eq!(dma_chunk(0x7fff0, 0x8400, 64), 16);
    assert_eq!(dma_chunk(0x80000, 0x8410, 48), 48);
    assert_eq!(dma_chunk(0x2000, 0xfff0, 64), 16);
    assert_eq!(dma_chunk(0x2000, 0xffff, 2), 1);
    assert_eq!(dma_chunk(0xffffff, 0x8400, 2), 1);
    assert_eq!(dma_chunk(0x1000000, 0x8400, 1), 0);
    assert_eq!(dma_chunk(u32::MAX, 0x8400, 1), 0);
    assert_eq!(dma_chunk(0, 0, 0), 0);
    assert_eq!(dma_chunk(0, 0, u16::MAX), u16::MAX);
}

#[test]
fn package_validation_detects_payload_corruption_and_wrong_image_size() {
    let mut bytes = package();
    assert!(baked_assets::check_package(&bytes).is_ok());
    assert!(baked_assets::check_package(&bytes[..REU_SIZE as usize - 1]).is_err());
    bytes[COMMON_OFFSET as usize + 2000] ^= 1;
    assert!(baked_assets::check_package(&bytes).is_err());
    bytes[COMMON_OFFSET as usize + 2000] ^= 1;
    bytes[SEGMENTS_OFFSET as usize + SEGMENT_SIZE + 400] ^= 1;
    assert!(baked_assets::check_package(&bytes).is_err());
}

#[test]
fn a_valid_checksum_does_not_make_invalid_music_a_supported_package() {
    let mut bytes = package();
    bytes[COMMON_OFFSET as usize + 2048] = b'X';
    let sum = checksum(&bytes[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE]);
    put16(&mut bytes, 18, sum);
    seal_header(&mut bytes);
    assert!(
        baked_assets::check_package(&bytes).is_err(),
        "host validation must reject a score the target cannot play"
    );
}

#[test]
fn model_only_cache_preserves_meshes_without_exposing_missing_audio() {
    let bytes = baked_assets::common_bank();
    let full = AssetBank::new(&bytes).unwrap();
    let cache = AssetBank::new(&bytes[..MODEL_SIZE]).unwrap();
    assert_eq!(full.audio().len(), 512);
    assert!(cache.audio().is_empty());
    assert!(
        sueme64::audio::AudioEngine::new(sueme64::audio::VideoStandard::Pal)
            .load_score(cache.audio())
            .is_err()
    );
    for mesh in 0..full.mesh_count() {
        assert_eq!(full.vertex_count(mesh), cache.vertex_count(mesh));
        assert_eq!(full.face_count(mesh), cache.face_count(mesh));
        for index in 0..full.vertex_count(mesh) {
            assert_eq!(full.vertex(mesh, index), cache.vertex(mesh, index));
        }
        for index in 0..full.face_count(mesh) {
            assert_eq!(full.face(mesh, index), cache.face(mesh, index));
        }
    }
    assert!(matches!(
        AssetBank::new(&bytes[..MODEL_SIZE - 1]),
        Err(AssetError::Size)
    ));
    assert!(matches!(
        AssetBank::new(&bytes[..MODEL_SIZE + 1]),
        Err(AssetError::Size)
    ));
}

#[test]
fn segment_prefix_contains_every_authored_encounter() {
    for index in 0..SEGMENT_COUNT {
        let bytes = baked_assets::segment_bank(index);
        let full = Segment::new(&bytes).unwrap();
        let cache = Segment::new(&bytes[..SEGMENT_CACHE_SIZE]).unwrap();
        assert_eq!(cache.index(), full.index());
        assert_eq!(cache.terrain(), full.terrain());
        assert_eq!(cache.ground_color(), full.ground_color());
        assert_eq!(cache.spawn_count(), full.spawn_count());
        for record in 0..full.spawn_count() {
            let a = full.spawn(record);
            let b = cache.spawn(record);
            assert_eq!(
                (a.distance, a.kind, a.x, a.y, a.flags),
                (b.distance, b.kind, b.x, b.y, b.flags)
            );
        }
    }
}

#[test]
fn both_segment_forms_enforce_the_62_record_cache_limit() {
    let mut bytes = baked_assets::segment_bank(3);
    bytes[7] = MAX_SEGMENT_SPAWNS;
    for index in 0..MAX_SEGMENT_SPAWNS as usize {
        let at = 16 + index * 8;
        put16(&mut bytes, at, index as u16 * 100);
        bytes[at + 2] = FIGHTER;
    }
    assert!(Segment::new(&bytes).is_ok());
    let cache = Segment::new(&bytes[..SEGMENT_CACHE_SIZE]).unwrap();
    assert_eq!(cache.spawn(MAX_SEGMENT_SPAWNS - 1).distance, 6100);
    assert_eq!(cache.spawn(MAX_SEGMENT_SPAWNS).kind, 0);
    for count in [MAX_SEGMENT_SPAWNS + 1, 200, 255] {
        bytes[7] = count;
        assert!(matches!(Segment::new(&bytes), Err(AssetError::Bounds)));
        assert!(matches!(
            Segment::new(&bytes[..SEGMENT_CACHE_SIZE]),
            Err(AssetError::Bounds)
        ));
    }
    assert!(matches!(
        Segment::new(&bytes[..SEGMENT_CACHE_SIZE - 1]),
        Err(AssetError::Size)
    ));
    assert!(matches!(
        Segment::new(&bytes[..SEGMENT_CACHE_SIZE + 1]),
        Err(AssetError::Size)
    ));
}

#[test]
fn trusted_views_match_prior_validation_of_the_same_immutable_bytes() {
    let models = baked_assets::common_bank();
    let _validated = AssetBank::new(&models).unwrap();
    // SAFETY: the full bank was validated above; this is its unchanged model prefix.
    let model_cache = unsafe { AssetBank::from_validated(&models[..MODEL_SIZE]) };
    assert_eq!(model_cache.vertex_count(PLAYER), 8);
    assert!(model_cache.audio().is_empty());
    let bytes = baked_assets::segment_bank(1);
    let validated = Segment::new(&bytes).unwrap();
    // SAFETY: the full segment was validated above; this is its unchanged cache prefix.
    let cache = unsafe { Segment::from_validated(&bytes[..SEGMENT_CACHE_SIZE]) };
    assert_eq!(cache.spawn_count(), validated.spawn_count());
    assert_eq!(cache.index(), 1);
}

#[test]
fn version_12_checksums_only_the_segment_runtime_prefix() {
    let mut bytes = package();
    for index in 0..SEGMENT_COUNT as usize {
        let at = SEGMENTS_OFFSET as usize + index * SEGMENT_SIZE;
        assert_eq!(
            u16_at(&bytes, 20 + index * 2),
            checksum(&bytes[at..at + SEGMENT_CACHE_SIZE])
        );
    }
    // Packaging padding is not loaded into a runtime cache and is not authenticated.
    bytes[SEGMENTS_OFFSET as usize + SEGMENT_CACHE_SIZE + 10] ^= 1;
    assert!(baked_assets::check_package(&bytes).is_ok());
    put32(&mut bytes, 4, 0x534D0101);
    seal_header(&mut bytes);
    assert_eq!(validate_header(&bytes), Err(AssetError::Version));
}
