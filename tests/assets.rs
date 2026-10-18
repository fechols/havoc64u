use havoc::assets::*;

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
    bytes[17] = havoc::music::SID_COUNT as u8;
    seal_header(&mut bytes);
    bytes
}
fn package() -> Vec<u8> {
    let mut bytes = vec![0u8; REU_SIZE as usize];
    bytes[..32].copy_from_slice(&header());
    let common = baked_assets::common_bank();
    let directory = baked_assets::model_directory();
    bytes[MODEL_DIRECTORY_OFFSET as usize..MODEL_DIRECTORY_OFFSET as usize + MODEL_DIRECTORY_SIZE]
        .copy_from_slice(&directory);
    let music = havoc::soundtrack::score_bank();
    bytes[MUSIC_OFFSET as usize..MUSIC_OFFSET as usize + music.len()].copy_from_slice(&music);
    let offset = COMMON_OFFSET as usize;
    bytes[offset..offset + COMMON_SIZE].copy_from_slice(&common);
    put16(&mut bytes, 18, checksum(&common));
    for index in 0..SEGMENT_COUNT {
        let model = baked_assets::model_bank(index);
        let model_offset = MODEL_BANKS_OFFSET as usize + index as usize * MODEL_SIZE;
        bytes[model_offset..model_offset + MODEL_SIZE].copy_from_slice(&model);
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
        (4, 0xff, AssetError::Version),
        (8, 0xff, AssetError::Bounds),
        (12, 0xff, AssetError::Bounds),
        (16, 5, AssetError::Bounds),
        (17, 6 - havoc::music::SID_COUNT as u8, AssetError::Bounds),
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
    assert_eq!(bank.vertex(255, 255), havoc::Vec3::default());
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
    assert!(havoc::music::cue_info(cache.audio(), 0).is_err());
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
fn version_30_checksums_only_the_segment_runtime_prefix() {
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

#[test]
fn level_model_banks_share_common_meshes_and_have_eleven_distinct_enemy_shapes() {
    let common = baked_assets::common_bank();
    let first = AssetBank::new(&common).unwrap();
    let mut shapes = Vec::new();
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        let bank = AssetBank::new(&bytes).unwrap();
        assert_eq!(bank.level(), level);
        assert_eq!(bank.enemy_count(), if level == 3 { 2 } else { 3 });
        for kind in 1..=bank.enemy_count() {
            assert!(bank.vertex_count(kind) <= 10);
            assert!(bank.face_count(kind) <= 12);
            let shape: Vec<_> = (0..bank.vertex_count(kind))
                .map(|index| bank.vertex(kind, index))
                .collect();
            assert!(
                !shapes.contains(&shape),
                "reused enemy shape {level}/{kind}"
            );
            shapes.push(shape);
            let spec = bank.enemy(kind);
            assert_eq!(spec.score, if kind == 3 { 250 } else { 100 });
            assert_eq!((spec.shot_radius, spec.ram_radius), (38, 36));
            assert_eq!(
                spec.center_y,
                if kind == TURRET && matches!(level, 0 | 2) {
                    28
                } else {
                    0
                }
            );
            assert_eq!(
                spec.hp,
                [[3, 2, 6], [2, 3, 4], [4, 2, 6], [3, 2, 0]][level as usize][kind as usize - 1]
            );
        }
        for kind in [0, bank.enemy_count() + 1, 255] {
            assert_eq!(bank.enemy(kind), EnemySpec::default());
        }
        for mesh in [PLAYER, CARRIER, CRYSTAL, BUILDING, ROCK] {
            assert_eq!(bank.vertex_count(mesh), first.vertex_count(mesh));
            assert_eq!(bank.face_count(mesh), first.face_count(mesh));
            for index in 0..bank.vertex_count(mesh) {
                assert_eq!(bank.vertex(mesh, index), first.vertex(mesh, index));
            }
            for index in 0..bank.face_count(mesh) {
                assert_eq!(bank.face(mesh, index), first.face(mesh, index));
            }
        }
        if level == 0 {
            assert_eq!(&bytes, &common[..MODEL_SIZE]);
        }
    }
    assert_eq!(shapes.len(), 11);
}

#[test]
fn enlarged_enemy_meshes_keep_ground_bases_and_original_model_budgets() {
    let check = |bank: &AssetBank<'_>, mesh, counts, expected_min, expected_max| {
        assert_eq!((bank.vertex_count(mesh), bank.face_count(mesh)), counts);
        let mut min = [i16::MAX; 3];
        let mut max = [i16::MIN; 3];
        for index in 0..bank.vertex_count(mesh) {
            let vertex = bank.vertex(mesh, index);
            for (axis, value) in [vertex.x, vertex.y, vertex.z].into_iter().enumerate() {
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        assert_eq!(
            (min, max),
            (expected_min, expected_max),
            "level {}/mesh {mesh}",
            bank.level()
        );
    };
    // Independently recorded target extents: ground models keep their -8 base,
    // while airborne enemies and the carrier assembly double about the origin.
    for (level, mesh, counts, min, max) in [
        (0, FIGHTER, (6, 8), [-54, -14, -60], [54, 20, 50]),
        (0, DIVER, (5, 6), [-36, -20, -64], [36, 36, 40]),
        (0, TURRET, (9, 11), [-40, -8, -56], [40, 44, 40]),
        (1, FIGHTER, (8, 10), [-76, -14, -56], [76, 10, 52]),
        (1, DIVER, (7, 8), [-68, -34, -70], [68, 68, 58]),
        (1, TURRET, (9, 12), [-30, -56, -68], [30, 56, 28]),
        (2, FIGHTER, (10, 12), [-64, -14, -76], [64, 22, 44]),
        (2, DIVER, (6, 8), [-24, -16, -96], [24, 18, 68]),
        (2, TURRET, (9, 11), [-68, -8, -78], [68, 38, 50]),
        (3, FIGHTER, (8, 10), [-60, -20, -56], [60, 26, 50]),
        (3, DIVER, (6, 8), [-34, -24, -84], [34, 54, 54]),
        (3, TURRET, (9, 11), [-40, -16, -56], [40, 36, 40]),
    ] {
        let bytes = baked_assets::model_bank(level);
        check(&AssetBank::new(&bytes).unwrap(), mesh, counts, min, max);
    }
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        assert_eq!(bytes.len(), 2048);
        let bank = AssetBank::new(&bytes).unwrap();
        check(&bank, CARRIER, (10, 17), [-170, -32, -172], [170, 76, 160]);
        // Player, pickups and scenery preserve their original dimensions.
        check(&bank, PLAYER, (8, 12), [-36, -7, -27], [36, 12, 36]);
        check(&bank, CRYSTAL, (6, 8), [-13, -20, -13], [13, 20, 13]);
        check(&bank, BUILDING, (8, 10), [-22, 0, -22], [22, 60, 22]);
        check(&bank, ROCK, (6, 6), [-32, 0, -30], [35, 42, 30]);
    }
}

#[test]
fn model_directory_rejects_unsupported_layout_and_corrupt_checksums() {
    let good = baked_assets::model_directory();
    assert_eq!(validate_model_directory(&good), Ok(()));
    assert_eq!(validate_model_directory(&good[..31]), Err(AssetError::Size));
    for (offset, value, expected) in [
        (0, b'X', AssetError::Magic),
        (4, 3, AssetError::Bounds),
        (5, 2, AssetError::Version),
        (6, 1, AssetError::Bounds),
        (8, 1, AssetError::Bounds),
        (20, 1, AssetError::Bounds),
    ] {
        let mut bad = good;
        bad[offset] = value;
        seal_header(&mut bad);
        assert_eq!(validate_model_directory(&bad), Err(expected));
    }
    let mut bad = good;
    bad[12] ^= 1;
    assert_eq!(validate_model_directory(&bad), Err(AssetError::Checksum));
    assert_eq!(model_checksum(&good, 4), 0);
    assert_eq!(model_checksum(&good[..31], 0), 0);
    for level in 0..SEGMENT_COUNT {
        assert_eq!(
            model_checksum(&good, level),
            checksum(&baked_assets::model_bank(level))
        );
    }
}

#[test]
fn model_identity_descriptors_and_vertex_ranges_are_checked() {
    let good = baked_assets::model_bank(0);
    for (offset, value) in [
        (5, 4),
        (6, 2),
        (80, 0),
        (80, 255),
        (82, 0),
        (83, 0),
        (86, 0),
        (86, 255),
        (87, 2),
    ] {
        let mut bad = good;
        bad[offset] = value;
        assert!(
            AssetBank::new(&bad).is_err(),
            "accepted malformed byte {offset}"
        );
    }
    let mut bad = good;
    put16(&mut bad, ENEMY_SPECS_OFFSET + 4, 0);
    assert!(AssetBank::new(&bad).is_err());
    put16(&mut bad, ENEMY_SPECS_OFFSET + 4, 100);
    put16(&mut bad, 16, 80);
    assert!(
        AssetBank::new(&bad).is_err(),
        "mesh vertices overlap enemy descriptors"
    );
    let mut bad = baked_assets::model_bank(3);
    bad[6] = 3;
    assert!(AssetBank::new(&bad).is_err());
}

#[test]
fn every_level_model_is_authenticated_and_paired_with_its_identity() {
    let good = package();
    assert!(baked_assets::check_package(&good).is_ok());
    for level in 0..SEGMENT_COUNT as usize {
        let start = MODEL_BANKS_OFFSET as usize + level * MODEL_SIZE;
        let mut bad = good.clone();
        bad[start + MODEL_SIZE - 1] ^= 1;
        assert!(
            baked_assets::check_package(&bad).is_err(),
            "unchecked model bank {level}"
        );
        let mut bad = good.clone();
        bad[start + 5] = (level as u8 + 1) % SEGMENT_COUNT;
        let directory = MODEL_DIRECTORY_OFFSET as usize;
        let model_sum = checksum(&bad[start..start + MODEL_SIZE]);
        put16(&mut bad, directory + 12 + level * 2, model_sum);
        let directory_sum = checksum(&bad[directory..directory + 30]);
        put16(&mut bad, directory + 30, directory_sum);
        assert!(
            baked_assets::check_package(&bad).is_err(),
            "wrong model identity {level}"
        );
    }
}

#[test]
fn authored_rosters_cover_their_local_types_and_keep_carrier_turrets_out_of_actor_slots() {
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::segment_bank(level);
        let segment = Segment::new(&bytes).unwrap();
        assert_eq!(segment.spawn_count(), [41, 36, 36, 10][level as usize]);
        let mut counts = [0; 8];
        for record in 0..segment.spawn_count() {
            let spawn = segment.spawn(record);
            counts[spawn.kind as usize] += 1;
            assert_eq!(spawn.flags & !1, 0);
        }
        assert!(counts[1] > 0 && counts[2] > 0);
        assert_eq!(counts[3], [5, 10, 10, 0][level as usize]);
        assert_eq!(counts[6] + counts[7], if level == 3 { 0 } else { 6 });
    }
    let mut bad = baked_assets::segment_bank(3);
    bad[18] = TURRET;
    assert!(matches!(Segment::new(&bad), Err(AssetError::Bounds)));
}
