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
    let sum = checksum(&header[..PACKAGE_CHECKSUM_OFFSET]);
    put16(header, PACKAGE_CHECKSUM_OFFSET, sum);
}
fn header() -> [u8; PACKAGE_HEADER_SIZE] {
    let mut bytes = [0u8; PACKAGE_HEADER_SIZE];
    bytes[..4].copy_from_slice(b"SM64");
    put32(&mut bytes, 4, PACKAGE_ID);
    put32(&mut bytes, 8, COMMON_OFFSET);
    put32(&mut bytes, 12, SEGMENTS_OFFSET);
    bytes[16] = SEGMENT_COUNT;
    bytes[17] = havoc::audio::SID_COUNT as u8;
    seal_header(&mut bytes);
    bytes
}
fn package() -> Vec<u8> {
    let mut bytes = vec![0u8; REU_SIZE as usize];
    bytes[..PACKAGE_HEADER_SIZE].copy_from_slice(&header());
    let common = baked_assets::common_bank();
    let directory = baked_assets::model_directory();
    bytes[MODEL_DIRECTORY_OFFSET as usize..MODEL_DIRECTORY_OFFSET as usize + MODEL_DIRECTORY_SIZE]
        .copy_from_slice(&directory);
    let directory = havoc::digital::catalog_directory();
    let start = DIGITAL_MUSIC_OFFSET as usize;
    bytes[start..start + directory.len()].copy_from_slice(directory);
    for (track, asset) in baked_assets::DIGITAL_TRACKS {
        let start = track.descriptor().asset_offset as usize;
        bytes[start..start + asset.len()].copy_from_slice(asset);
    }
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
            SEGMENT_CHECKSUM_OFFSET + index as usize * 2,
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
    assert_eq!(
        validate_header(&good[..PACKAGE_HEADER_SIZE - 1]),
        Err(AssetError::Size)
    );
    for (offset, value, expected) in [
        (0, b'X', AssetError::Magic),
        (4, 0xff, AssetError::Version),
        (8, 0xff, AssetError::Bounds),
        (12, 0xff, AssetError::Bounds),
        (16, 5, AssetError::Bounds),
        (17, 2, AssetError::Bounds),
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
    assert_eq!(bank.shadow_spec(255), ShadowSpec::default());
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
        (4, SEGMENT_COUNT),
        (5, 4),
        (6, 16),
        (7, 201),
        (8, 4),
        (9, havoc::audio::CUE_COUNT as u8),
        (10, 5),
        (11, 6),
        (12, 16),
        (13, 16),
        (14, 16),
        (15, 1),
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
    assert_eq!(dma_chunk(0x7ffff, 0x8400, 2), 1);
    assert_eq!(dma_chunk(0xfffff, 0x8400, 2), 1);
    assert_eq!(dma_chunk(0x100000, 0x8400, 1), 1);
    assert_eq!(dma_chunk(0x7fffff, 0x8400, 2), 1);
    assert_eq!(dma_chunk(0x800000, 0x8400, 1), 1);
    assert_eq!(dma_chunk(0xfffff0, 0x8400, 64), 16);
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
    bytes.push(0);
    assert!(baked_assets::check_package(&bytes).is_err());
    bytes.pop();
    bytes[COMMON_OFFSET as usize + 2000] ^= 1;
    assert!(baked_assets::check_package(&bytes).is_err());
    bytes[COMMON_OFFSET as usize + 2000] ^= 1;
    bytes[SEGMENTS_OFFSET as usize + SEGMENT_SIZE + 400] ^= 1;
    assert!(baked_assets::check_package(&bytes).is_err());
}

#[test]
fn relocated_package_rejects_old_version_even_with_a_valid_header_checksum() {
    let mut bytes = package();
    put32(&mut bytes, 4, 0x534D0900);
    seal_header(&mut bytes);
    assert_eq!(validate_header(&bytes), Err(AssetError::Version));
    assert!(baked_assets::check_package(&bytes).is_err());
}

#[test]
fn digital_payload_is_checked_but_runtime_pcm_ring_is_mutable() {
    let mut bytes = package();
    for (track, _) in baked_assets::DIGITAL_TRACKS {
        let descriptor = track.descriptor();
        for offset in [
            0,
            21,
            22,
            32,
            80,
            descriptor.metadata.stream_offset as usize,
            descriptor.metadata.crc_offset as usize,
        ] {
            let address = descriptor.asset_offset as usize + offset;
            bytes[address] ^= 1;
            assert!(
                baked_assets::check_package(&bytes).is_err(),
                "{track:?} byte {offset}"
            );
            bytes[address] ^= 1;
        }
    }
    bytes[DIGITAL_RING_OFFSET as usize..].fill(0x81);
    assert!(baked_assets::check_package(&bytes).is_ok());
}

#[test]
fn a_valid_checksum_does_not_make_nonzero_reserved_padding_a_supported_package() {
    let mut bytes = package();
    bytes[COMMON_OFFSET as usize + 2048] = b'X';
    let sum = checksum(&bytes[COMMON_OFFSET as usize..COMMON_OFFSET as usize + COMMON_SIZE]);
    put16(&mut bytes, 18, sum);
    seal_header(&mut bytes);
    assert!(
        baked_assets::check_package(&bytes).is_err(),
        "host validation must reject nonzero retired score padding"
    );
}

#[test]
fn model_only_cache_preserves_meshes_without_exposing_missing_audio() {
    let bytes = baked_assets::common_bank();
    let full = AssetBank::new(&bytes).unwrap();
    let cache = AssetBank::new(&bytes[..MODEL_SIZE]).unwrap();
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
        assert_eq!(cache.stage_info(), full.stage_info());
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
    let last = 16 + (MAX_SEGMENT_SPAWNS as usize - 1) * 8;
    bytes[last + 3] = 1;
    put16(&mut bytes, last + 4, i16::MIN as u16);
    put16(&mut bytes, last + 6, i16::MAX as u16);
    assert!(Segment::new(&bytes).is_ok());
    let cache = Segment::new(&bytes[..SEGMENT_CACHE_SIZE]).unwrap();
    let spawn = cache.spawn(MAX_SEGMENT_SPAWNS - 1);
    assert_eq!(
        (spawn.distance, spawn.kind, spawn.flags),
        (6100, FIGHTER, 1)
    );
    assert_eq!((spawn.x, spawn.y), (i16::MIN, i16::MAX));
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
    let bytes = baked_assets::segment_bank(1);
    let validated = Segment::new(&bytes).unwrap();
    // SAFETY: the full segment was validated above; this is its unchanged cache prefix.
    let cache = unsafe { Segment::from_validated(&bytes[..SEGMENT_CACHE_SIZE]) };
    assert_eq!(cache.spawn_count(), validated.spawn_count());
    assert_eq!(cache.index(), 1);
}

#[test]
fn version_60_checksums_only_the_segment_runtime_prefix() {
    let mut bytes = package();
    for index in 0..SEGMENT_COUNT as usize {
        let at = SEGMENTS_OFFSET as usize + index * SEGMENT_SIZE;
        assert_eq!(
            u16_at(&bytes, SEGMENT_CHECKSUM_OFFSET + index * 2),
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
fn stage_model_banks_keep_twenty_six_distinct_enemy_shapes_and_shared_player_assets() {
    let common = baked_assets::common_bank();
    let first = AssetBank::new(&common).unwrap();
    let mut shapes = Vec::new();
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        let bank = AssetBank::new(&bytes).unwrap();
        let info = preview_stage_info(level);
        let profile = info.enemy_profile;
        assert_eq!(bank.level(), level);
        assert_eq!(bank.enemy_count(), if info.boss_kind != 0 { 2 } else { 3 });
        for kind in 1..=bank.enemy_count() {
            assert!(bank.vertex_count(kind) <= 10);
            assert!(bank.face_count(kind) <= 16);
            let shape: Vec<_> = (0..bank.vertex_count(kind))
                .map(|index| bank.vertex(kind, index))
                .collect();
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
            let spec = bank.enemy(kind);
            assert_eq!(spec.score, if kind == 3 { 250 } else { 100 });
            // Boss stages expose two escorts, so every checked spec is a
            // regular enemy at three halves of its authored size.
            assert_eq!(
                (spec.shot_radius, spec.ram_radius),
                (enemy_scaled(19) as u8, enemy_scaled(18) as u8)
            );
            assert_eq!(
                spec.center_y,
                if kind != TURRET || profile == 1 {
                    0
                } else {
                    (enemy_scaled(18) - 8) as i8
                }
            );
            assert_eq!(
                spec.hp,
                [[3, 2, 6], [2, 3, 4], [4, 2, 6], [3, 2, 6]][profile as usize][kind as usize - 1]
            );
        }
        for kind in [0, bank.enemy_count() + 1, 255] {
            assert_eq!(bank.enemy(kind), EnemySpec::default());
        }
        for mesh in [PLAYER, CRYSTAL, BUILDING, MEDAL] {
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
    // Fourteen terrestrial silhouettes plus two newly authored aircraft per
    // orbital mission. Ground guns retain their original silhouettes.
    assert_eq!(shapes.len(), 26);
}

#[test]
fn completed_enemy_surfaces_keep_ground_bases_and_original_vertex_bounds() {
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
    // Independently recorded target extents: ground models keep their -8 base.
    // Regular enemies scale by three halves about the origin; the boss side
    // unit in slot three (level 11) and the carrier assembly keep doubling.
    for (level, mesh, counts, min, max) in [
        (0, FIGHTER, (6, 8), [-40, -10, -45], [40, 15, 37]),
        (0, DIVER, (5, 6), [-27, -15, -48], [27, 27, 30]),
        (0, TURRET, (9, 14), [-30, -8, -42], [30, 31, 30]),
        (2, FIGHTER, (8, 16), [-57, -10, -42], [57, 7, 39]),
        (2, DIVER, (7, 10), [-51, -25, -52], [51, 51, 43]),
        (2, TURRET, (9, 14), [-22, -42, -51], [22, 42, 21]),
        (6, FIGHTER, (10, 16), [-48, -10, -57], [48, 16, 33]),
        (6, DIVER, (6, 8), [-18, -12, -72], [18, 13, 51]),
        (6, TURRET, (9, 14), [-51, -8, -58], [51, 26, 37]),
        (11, FIGHTER, (8, 12), [-45, -15, -42], [45, 19, 37]),
        (11, DIVER, (6, 8), [-25, -18, -63], [25, 40, 40]),
        (11, TURRET, (9, 14), [-40, -16, -56], [40, 36, 40]),
    ] {
        let bytes = baked_assets::model_bank(level);
        check(&AssetBank::new(&bytes).unwrap(), mesh, counts, min, max);
    }
    for level in 0..12 {
        let bytes = baked_assets::model_bank(level);
        assert_eq!(bytes.len(), 2048);
        let bank = AssetBank::new(&bytes).unwrap();
        let (min, max) = match level {
            3 => ([-190, -48, -164], [190, 60, 130]),
            7 => ([-176, -40, -156], [176, 116, 136]),
            _ => ([-170, -32, -172], [170, 76, 160]),
        };
        check(&bank, CARRIER, (10, 16), min, max);
        // Player, pickups and scenery preserve their original dimensions.
        check(&bank, PLAYER, (8, 12), [-36, -7, -27], [36, 12, 36]);
        check(&bank, CRYSTAL, (6, 8), [-13, -20, -13], [13, 20, 13]);
        check(&bank, BUILDING, (8, 12), [-22, 0, -22], [22, 60, 22]);
        if !matches!(level, 1 | 4) {
            check(&bank, ROCK, (6, 8), [-32, 0, -30], [35, 42, 30]);
        }
    }
}

#[test]
fn every_quantized_model_has_outward_solids_and_explicit_thin_panel_backs() {
    use std::collections::BTreeMap;

    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        let bank = AssetBank::new(&bytes).unwrap();
        let info = preview_stage_info(level);
        // The medal has its own closed-component and attached-relief tests.
        for mesh in 0..MEDAL {
            let faces: Vec<_> = (0..bank.face_count(mesh))
                .map(|face| bank.face(mesh, face))
                .collect();
            let mut panel = vec![false; faces.len()];
            let mut pairs = 0;
            for (index, &(indices, color)) in faces.iter().enumerate() {
                if panel[index] {
                    continue;
                }
                let [a, b, c] = indices.map(|vertex| bank.vertex(mesh, vertex));
                let ab = [b.x - a.x, b.y - a.y, b.z - a.z].map(i64::from);
                let ac = [c.x - a.x, c.y - a.y, c.z - a.z].map(i64::from);
                assert_ne!(
                    [
                        ab[1] * ac[2] - ab[2] * ac[1],
                        ab[2] * ac[0] - ab[0] * ac[2],
                        ab[0] * ac[1] - ab[1] * ac[0],
                    ],
                    [0; 3],
                    "stage {level}, mesh {mesh}, face {index} collapsed when quantized"
                );
                let reverse = [indices[0], indices[2], indices[1]];
                if let Some(back) = (index + 1..faces.len()).find(|&other| {
                    !panel[other]
                        && (0..3).any(|offset| {
                            (0..3).all(|corner| {
                                faces[other].0[corner] == reverse[(corner + offset) % 3]
                            })
                        })
                }) {
                    assert_eq!(faces[back].1, color, "thin panels retain their material");
                    panel[index] = true;
                    panel[back] = true;
                    pairs += 1;
                }
            }
            let expected_pairs = match mesh {
                FIGHTER if info.theme < 10 && info.enemy_profile == 1 => 6,
                FIGHTER if info.theme < 10 && info.enemy_profile == 2 => 2,
                DIVER if info.theme < 10 && info.enemy_profile == 1 => 2,
                TURRET if info.enemy_profile != 1 => 1,
                _ => 0,
            };
            assert_eq!(pairs, expected_pairs, "stage {level}, mesh {mesh}");

            let mut edges = BTreeMap::<(u8, u8), Vec<(usize, i8)>>::new();
            for (face, &(indices, _)) in faces.iter().enumerate() {
                if panel[face] {
                    continue;
                }
                for [a, b] in [
                    [indices[0], indices[1]],
                    [indices[1], indices[2]],
                    [indices[2], indices[0]],
                ] {
                    edges
                        .entry((a.min(b), a.max(b)))
                        .or_default()
                        .push((face, if a < b { 1 } else { -1 }));
                }
            }
            for (edge, uses) in &edges {
                assert!(
                    uses.len() == 2 && uses[0].1 == -uses[1].1,
                    "stage {level}, mesh {mesh}, solid edge {edge:?}: {uses:?}"
                );
            }
            let mut visited = panel;
            for start in 0..faces.len() {
                if visited[start] {
                    continue;
                }
                let mut pending = vec![start];
                let mut volume = 0i64;
                while let Some(face) = pending.pop() {
                    if visited[face] {
                        continue;
                    }
                    visited[face] = true;
                    let [a, b, c] = faces[face].0.map(|index| {
                        let point = bank.vertex(mesh, index);
                        [point.x, point.y, point.z].map(i64::from)
                    });
                    volume += a[0] * (b[1] * c[2] - b[2] * c[1])
                        + a[1] * (b[2] * c[0] - b[0] * c[2])
                        + a[2] * (b[0] * c[1] - b[1] * c[0]);
                    for uses in edges.values() {
                        if uses[0].0 == face {
                            pending.push(uses[1].0);
                        } else if uses[1].0 == face {
                            pending.push(uses[0].0);
                        }
                    }
                }
                assert!(volume > 0, "stage {level}, mesh {mesh} has an inward solid");
            }
        }
    }
}

#[test]
fn sma6_banks_reserve_shadow_tail_and_reject_old_culling_contracts() {
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        assert_eq!(&bytes[..4], b"SMA6");
        for mesh in 0..MODEL_COUNT {
            let descriptor = 16 + mesh as usize * 8;
            assert_eq!(bytes[descriptor + 7], 0);
            let end = u16_at(&bytes, descriptor + 4) as usize + bytes[descriptor + 3] as usize * 4;
            assert!(
                end <= SHADOW_SPECS_OFFSET,
                "stage {level}, mesh {mesh} exceeds its bank"
            );
            for old_flags in [1, 255] {
                let mut bad = bytes;
                bad[descriptor + 7] = old_flags;
                assert!(matches!(AssetBank::new(&bad), Err(AssetError::Bounds)));
            }
        }
        let mut old = bytes;
        old[..4].copy_from_slice(b"SMA5");
        assert!(matches!(AssetBank::new(&old), Err(AssetError::Magic)));
    }
    let mut previous_package = header();
    put32(&mut previous_package, 4, 0x534D0B00);
    seal_header(&mut previous_package);
    assert_eq!(validate_header(&previous_package), Err(AssetError::Version));
}

#[test]
fn shadow_descriptors_cover_scaled_quantized_mesh_bounds_in_every_bank() {
    assert_eq!(SHADOW_SPECS_OFFSET, 1920);
    assert_eq!(SHADOW_SPEC_SIZE, 12);
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        let bank = AssetBank::new(&bytes).unwrap();
        for id in 0..MODEL_COUNT {
            let shape = bank.shadow_spec(id);
            let expected = match id {
                PLAYER => (1, 0),
                FIGHTER => (1, 2),
                DIVER => (3, 2),
                CARRIER => (2, 0),
                ROCK if level >= 12 => (3, 0),
                _ => (0, 0),
            };
            assert_eq!(
                (shape.family, shape.heading),
                expected,
                "level {level}, mesh {id}"
            );
            if shape.family == 0 {
                assert_eq!(shape, ShadowSpec::default());
                continue;
            }
            let points: Vec<_> = (0..bank.vertex_count(id))
                .map(|v| bank.vertex(id, v))
                .collect();
            for (center, half, values) in [
                (
                    shape.offset_x,
                    shape.half_x,
                    points.iter().map(|p| p.x).collect::<Vec<_>>(),
                ),
                (
                    shape.offset_z,
                    shape.half_z,
                    points.iter().map(|p| p.z).collect::<Vec<_>>(),
                ),
            ] {
                let min = *values.iter().min().unwrap();
                let max = *values.iter().max().unwrap();
                assert!(half > 0);
                assert!(center - half as i16 <= min && center + half as i16 >= max);
                assert!((half * 2) as i16 - (max - min) <= 1, "tight baked extent");
            }
        }
        assert!(
            bytes[SHADOW_SPECS_OFFSET + MODEL_COUNT as usize * SHADOW_SPEC_SIZE..]
                .iter()
                .all(|&v| v == 0)
        );
    }
}

#[test]
fn malformed_shadow_records_padding_and_mesh_overlap_are_rejected() {
    let good = baked_assets::model_bank(0);
    for (offset, value) in [
        (SHADOW_SPECS_OFFSET, 5),
        (SHADOW_SPECS_OFFSET + 1, 4),
        (SHADOW_SPECS_OFFSET + 10, 1),
        (
            SHADOW_SPECS_OFFSET + TURRET as usize * SHADOW_SPEC_SIZE + 1,
            1,
        ),
        (
            SHADOW_SPECS_OFFSET + MODEL_COUNT as usize * SHADOW_SPEC_SIZE,
            1,
        ),
        (MODEL_SIZE - 1, 1),
    ] {
        let mut bad = good;
        bad[offset] = value;
        assert!(
            matches!(AssetBank::new(&bad), Err(AssetError::Bounds)),
            "offset {offset}"
        );
    }
    for (offset, value) in [
        (2, 0x8000),
        (4, 1025),
        (6, 0),
        (8, 0),
        (6, 1025),
        (8, 0xffff),
    ] {
        let mut bad = good;
        put16(&mut bad, SHADOW_SPECS_OFFSET + offset, value);
        assert!(matches!(AssetBank::new(&bad), Err(AssetError::Bounds)));
    }
    let mut bad = good;
    put16(&mut bad, 20, SHADOW_SPECS_OFFSET as u16 - 3);
    assert!(matches!(AssetBank::new(&bad), Err(AssetError::Bounds)));
}

#[test]
fn every_actor_mesh_is_fully_behind_the_near_plane_at_the_retire_depth() {
    // Actors keep rendering down to the retire depth, so no mesh may still
    // reach the near plane there; otherwise it would vanish while visible.
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        let bank = AssetBank::new(&bytes).unwrap();
        for mesh in [FIGHTER, DIVER, TURRET, BUILDING, ROCK] {
            let front = (0..bank.vertex_count(mesh))
                .map(|index| bank.vertex(mesh, index).z)
                .max()
                .unwrap();
            assert!(
                havoc::game::ACTOR_RETIRE_Z + front < havoc::render::NEAR,
                "level {level} mesh {mesh} reaches z {front}"
            );
        }
    }
}

#[test]
fn model_directory_rejects_unsupported_layout_and_corrupt_checksums() {
    let good = baked_assets::model_directory();
    assert_eq!(validate_model_directory(&good), Ok(()));
    assert_eq!(
        validate_model_directory(&good[..MODEL_DIRECTORY_SIZE - 1]),
        Err(AssetError::Size)
    );
    for (offset, value, expected) in [
        (0, b'X', AssetError::Magic),
        (4, 3, AssetError::Bounds),
        (5, 1, AssetError::Version),
        (6, 1, AssetError::Bounds),
        (8, 1, AssetError::Bounds),
        (12 + SEGMENT_COUNT as usize * 2, 1, AssetError::Bounds),
    ] {
        let mut bad = good;
        bad[offset] = value;
        seal_header(&mut bad);
        assert_eq!(validate_model_directory(&bad), Err(expected));
    }
    let mut bad = good;
    bad[12] ^= 1;
    assert_eq!(validate_model_directory(&bad), Err(AssetError::Checksum));
    assert_eq!(model_checksum(&good, SEGMENT_COUNT), 0);
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
        (5, SEGMENT_COUNT),
        (6, 2),
        (7, 6),
        (ENEMY_SPECS_OFFSET, 0),
        (ENEMY_SPECS_OFFSET, 255),
        (ENEMY_SPECS_OFFSET + 2, 0),
        (ENEMY_SPECS_OFFSET + 3, 0),
        (ENEMY_SPECS_OFFSET + 6, 0),
        (ENEMY_SPECS_OFFSET + 6, 255),
        (ENEMY_SPECS_OFFSET + 7, 2),
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
        let directory_sum = checksum(&bad[directory..directory + MODEL_DIRECTORY_CHECKSUM_OFFSET]);
        put16(
            &mut bad,
            directory + MODEL_DIRECTORY_CHECKSUM_OFFSET,
            directory_sum,
        );
        assert!(
            baked_assets::check_package(&bad).is_err(),
            "wrong model identity {level}"
        );
    }
}

#[test]
fn enemy_timing_leaves_room_for_repeats_without_moving_hazards() {
    let counts = [
        35, 28, 30, 12, 28, 31, 30, 12, 33, 33, 33, 14, 23, 23, 23, 23, 17, 17,
    ];
    let space_hazards: [&[u16]; 6] = [
        &[2400, 5200, 8200, 11200, 14300, 17400, 20500, 23600, 26700],
        &[2700, 5600, 8600, 11600, 14700, 17800, 20800, 23900, 27000],
        &[2500, 5500, 8500, 11500, 14500, 17500, 20600, 23700, 26800],
        &[2600, 5600, 8600, 11600, 14600, 17600, 20600, 23600, 26600],
        &[2400, 5200, 8000, 10800, 13600, 16400],
        &[2700, 5500, 8300, 11100, 13900, 16700],
    ];
    let mut enemies = 0;
    let mut hazards = 0;
    for stage in 0..SEGMENT_COUNT {
        let bytes = baked_assets::segment_bank(stage);
        let segment = Segment::new(&bytes).unwrap();
        let records: Vec<_> = (0..segment.spawn_count())
            .map(|i| segment.spawn(i))
            .collect();
        let combat: Vec<_> = records
            .iter()
            .filter(|s| matches!(s.kind, FIGHTER | DIVER | TURRET))
            .collect();
        assert_eq!(combat.len(), counts[stage as usize], "stage {stage}");
        for spawn in &combat {
            assert!(
                u32::from(spawn.distance) + 1350 < SEGMENT_DISTANCE,
                "stage {stage} repeat crosses its boundary"
            );
            if stage >= 16 {
                assert!(
                    spawn.distance + 1350 < 20_000,
                    "stage {stage} repeat reaches the final boss entrance"
                );
            }
        }
        if stage == 0 {
            // Preserve the opening timing and reward. Lateral lanes are
            // derived from the hull envelopes and audited separately.
            let opening: Vec<_> = combat[..3]
                .iter()
                .map(|s| (s.distance, s.kind, s.y, s.flags))
                .collect();
            assert_eq!(
                opening,
                [
                    (1140, FIGHTER, 35, 0),
                    (1235, FIGHTER, 35, 1),
                    (1330, FIGHTER, 35, 0)
                ]
            );
            assert_eq!(
                combat.iter().find(|s| s.kind == TURRET).unwrap().distance,
                4845
            );
        }
        let actual: Vec<_> = records
            .iter()
            .filter(|s| matches!(s.kind, ROCK | BUILDING))
            .map(|s| s.distance)
            .collect();
        let expected = if stage >= 12 {
            space_hazards[stage as usize - 12].to_vec()
        } else if segment.stage_info().boss_kind == 0 {
            (0..6)
                .map(|i| 2200 + i * 4300 + u16::from(stage) * 40)
                .collect()
        } else {
            Vec::new()
        };
        assert_eq!(actual, expected, "stage {stage} hazard timing changed");
        enemies += combat.len();
        hazards += actual.len();
    }
    assert_eq!((enemies, hazards), (445, 102));
}

#[test]
fn authored_rosters_cover_their_local_types_and_keep_boss_turrets_out_of_actor_slots() {
    let mut scripts = Vec::new();
    for level in 0..SEGMENT_COUNT {
        let bytes = baked_assets::segment_bank(level);
        let segment = Segment::new(&bytes).unwrap();
        let info = segment.stage_info();
        assert_eq!(info, preview_stage_info(level));
        assert_eq!(core::mem::size_of::<StageInfo>(), 8);
        assert!(segment.spawn_count() <= MAX_SEGMENT_SPAWNS);
        if level == 0 {
            assert_eq!(segment.spawn_count(), 41);
        }
        let script = bytes[16..16 + segment.spawn_count() as usize * 8].to_vec();
        assert!(
            !scripts.contains(&script),
            "duplicated stage {level} script"
        );
        scripts.push(script);
        let mut counts = [0; 8];
        for record in 0..segment.spawn_count() {
            let spawn = segment.spawn(record);
            counts[spawn.kind as usize] += 1;
            assert_eq!(spawn.flags & !3, 0);
            if spawn.flags & SMALL_DEBRIS != 0 {
                assert_eq!(spawn.kind, ROCK);
                assert!(level >= 12);
            }
        }
        assert!(counts[1] > 0 && counts[2] > 0);
        if info.boss_kind == 0 && level < 12 {
            assert!(counts[3] > 0);
        } else {
            assert_eq!(counts[3], 0);
        }
        assert_eq!(
            counts[6] + counts[7],
            if level >= 12 {
                if info.boss_kind != 0 {
                    6
                } else {
                    9
                }
            } else if info.boss_kind != 0 {
                0
            } else {
                6
            }
        );
    }
    let mut bad = baked_assets::segment_bank(3);
    bad[18] = TURRET;
    assert!(matches!(Segment::new(&bad), Err(AssetError::Bounds)));
}

#[test]
fn eighteen_stage_profiles_cover_terrestrial_and_orbital_campaign_with_contiguous_crossings() {
    assert_eq!(SEGMENT_COUNT, 18);
    assert_eq!(SEGMENT_DISTANCE * SEGMENT_COUNT as u32, 540_000);
    assert_eq!(StageInfo::FIRST, preview_stage_info(0));
    assert_eq!(PACKAGE_HEADER_SIZE as u32, MODEL_DIRECTORY_OFFSET);
    assert!(MODEL_DIRECTORY_OFFSET + MODEL_DIRECTORY_SIZE as u32 <= COMMON_OFFSET);
    assert!(COMMON_OFFSET + COMMON_SIZE as u32 <= SEGMENTS_OFFSET);
    assert_eq!(
        SEGMENTS_OFFSET + SEGMENT_SIZE as u32 * SEGMENT_COUNT as u32,
        0xb000
    );
    assert_eq!(
        MODEL_BANKS_OFFSET + MODEL_SIZE as u32 * SEGMENT_COUNT as u32,
        0xb9000
    );
    assert_eq!(REU_SIZE, 16 * 1024 * 1024);
    assert_eq!(DIGITAL_MUSIC_OFFSET, havoc::digital::MUSIC_OFFSET);
    assert_eq!(DIGITAL_VICTORY_OFFSET, havoc::digital::HAMMOND_OFFSET);
    assert_eq!(DIGITAL_RING_OFFSET, havoc::digital::RING_OFFSET);
    assert_eq!(DIGITAL_RING_OFFSET + DIGITAL_RING_SIZE, REU_SIZE);
    assert!(havoc::digital::ASSET_BYTES <= DIGITAL_MUSIC_CAPACITY);
    assert!(
        havoc::digital::Track::HammondRocker
            .descriptor()
            .asset_bytes
            <= DIGITAL_VICTORY_CAPACITY
    );
    let mut bosses = Vec::new();
    for stage in 0..SEGMENT_COUNT {
        let bytes = baked_assets::segment_bank(stage);
        let info = Segment::new(&bytes).unwrap().stage_info();
        assert_eq!(info, baked_assets::stage_info(stage));
        assert!(!baked_assets::stage_name(stage).is_empty());
        assert_eq!(
            info.simulation_profile(),
            match info.boss_kind {
                0 => info.enemy_profile,
                4 => 6,
                5 => 5,
                kind => 3 + kind,
            }
        );
        if info.boss_kind != 0 {
            bosses.push((stage, info.boss_kind));
        }
        if stage != 0 {
            assert_eq!(info.previous_theme, preview_stage_info(stage - 1).theme);
        }
        if stage + 1 < SEGMENT_COUNT {
            assert_eq!(info.next_theme, preview_stage_info(stage + 1).theme);
        }
    }
    assert_eq!(bosses, [(3, 1), (7, 2), (11, 3), (16, 4), (17, 5)]);
}

#[test]
fn all_eighteen_encounters_and_their_metadata_are_authenticated() {
    let good = package();
    for stage in 0..SEGMENT_COUNT as usize {
        let start = SEGMENTS_OFFSET as usize + stage * SEGMENT_SIZE;
        for offset in [8, 9, 10, 11, 12, 13, 14, 15, 16, SEGMENT_CACHE_SIZE - 1] {
            let mut corrupt = good.clone();
            corrupt[start + offset] ^= 1;
            assert!(
                baked_assets::check_package(&corrupt).is_err(),
                "unauthenticated stage {stage} byte {offset}"
            );
        }
    }
}

#[test]
fn model_boss_identity_must_agree_with_the_checked_encounter_header() {
    let mut bytes = package();
    let start = MODEL_BANKS_OFFSET as usize + 3 * MODEL_SIZE;
    bytes[start + 7] = 2; // Still a valid two-enemy bank, but the wrong boss.
    let sum = checksum(&bytes[start..start + MODEL_SIZE]);
    let directory = MODEL_DIRECTORY_OFFSET as usize;
    put16(&mut bytes, directory + 12 + 3 * 2, sum);
    let sum = checksum(&bytes[directory..directory + MODEL_DIRECTORY_CHECKSUM_OFFSET]);
    put16(&mut bytes, directory + MODEL_DIRECTORY_CHECKSUM_OFFSET, sum);
    assert!(baked_assets::check_package(&bytes).is_err());
}

#[test]
fn orbital_hazards_are_mixed_in_three_dimensions_and_stop_before_the_final_bosses() {
    for level in 12..SEGMENT_COUNT {
        let bytes = baked_assets::segment_bank(level);
        let segment = Segment::new(&bytes).unwrap();
        let hazards: Vec<_> = (0..segment.spawn_count())
            .map(|i| segment.spawn(i))
            .filter(|spawn| spawn.kind == ROCK)
            .collect();
        assert!(hazards.iter().any(|s| s.flags == SMALL_DEBRIS));
        assert!(hazards.iter().any(|s| s.flags == 0));
        assert!(hazards.iter().any(|s| s.y < 0));
        assert!(hazards.iter().any(|s| s.y > 60));
        assert!(hazards.iter().any(|s| s.x < 0));
        assert!(hazards.iter().any(|s| s.x > 0));
        assert!((0..segment.spawn_count()).all(|i| segment.spawn(i).kind != TURRET));
        if level >= 16 {
            assert!((0..segment.spawn_count()).all(|i| segment.spawn(i).distance < 20_000));
        }
    }
    for (level, kind, flags) in [
        (0, ROCK, SMALL_DEBRIS),
        (12, FIGHTER, SMALL_DEBRIS),
        (12, ROCK, 4),
    ] {
        let mut bytes = baked_assets::segment_bank(level);
        bytes[18] = kind;
        bytes[19] = flags;
        assert!(matches!(Segment::new(&bytes), Err(AssetError::Bounds)));
    }
}

#[test]
fn orbital_meshes_fit_the_unchanged_cache_and_keep_tumbling_obstacles_centered() {
    for level in 12..SEGMENT_COUNT {
        let bytes = baked_assets::model_bank(level);
        let bank = AssetBank::new(&bytes).unwrap();
        let extent = |kind, axis: fn(havoc::Vec3) -> i16| {
            let values: Vec<_> = (0..bank.vertex_count(kind))
                .map(|i| axis(bank.vertex(kind, i)))
                .collect();
            (*values.iter().min().unwrap(), *values.iter().max().unwrap())
        };
        let (low, high) = extent(ROCK, |v| v.y);
        assert_eq!(low, -high);
        let used =
            u16_at(&bytes, 20 + MEDAL as usize * 8) as usize + bank.face_count(MEDAL) as usize * 4;
        assert!(used <= SHADOW_SPECS_OFFSET);
        if level == 16 {
            let (front, rear) = extent(CARRIER, |v| v.z);
            assert!(
                rear - front >= 800,
                "the orbital carrier must read as a capital ship"
            );
        }
        if level == 17 {
            let (bottom, top) = extent(CARRIER, |v| v.y);
            assert!(
                top - bottom >= 400,
                "the gate must span the flight corridor"
            );
            assert_eq!(bank.vertex_count(CARRIER), 32);
        }
    }
}
