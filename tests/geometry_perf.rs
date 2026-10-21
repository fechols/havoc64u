use havoc::{
    assets::{AssetBank, MAX_MESH_SHIFT, MESH_ONE_SIDED, MODEL_COUNT, MODEL_DATA_OFFSET},
    emit_mesh, math, GeometrySink, Vec3, TWO_SIDED,
};

#[allow(dead_code)]
#[path = "../src/host.rs"]
mod baked_assets;

#[derive(Default)]
struct Capture(Vec<([Vec3; 3], u8, u8)>);

impl GeometrySink for Capture {
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: u8, flags: u8) {
        self.0.push(([a, b, c], color, flags));
    }
}

/// A bank whose nine meshes all share the given stored vertices, shift and
/// one-sided flag, with two faces: (0, 1, 2) and (2, last, 0).
fn bank_bytes(points: &[[i8; 3]], shift: u8, one_sided: bool) -> Vec<u8> {
    let mut bytes = baked_assets::model_bank(0).to_vec();
    let faces = MODEL_DATA_OFFSET + points.len() * 3;
    for id in 0..MODEL_COUNT as usize {
        let at = 16 + id * 8;
        bytes[at..at + 2].copy_from_slice(&(MODEL_DATA_OFFSET as u16).to_le_bytes());
        bytes[at + 2] = points.len() as u8;
        bytes[at + 3] = 2;
        bytes[at + 4..at + 6].copy_from_slice(&(faces as u16).to_le_bytes());
        bytes[at + 6] = shift;
        bytes[at + 7] = if one_sided { MESH_ONE_SIDED } else { 0 };
    }
    for (index, point) in points.iter().enumerate() {
        for (axis, value) in point.iter().enumerate() {
            bytes[MODEL_DATA_OFFSET + index * 3 + axis] = *value as u8;
        }
    }
    bytes[faces..faces + 8].copy_from_slice(&[0, 1, 2, 5, 2, points.len() as u8 - 1, 0, 7]);
    bytes
}

fn check_mesh(bank: &AssetBank<'_>, angles: [u8; 3], half: bool, position: Vec3) {
    let [yaw, pitch, roll] = angles;
    // The reference applies the composed matrix to every stored vertex
    // directly; emit_mesh must match it through its vertex array and fallback.
    let matrix = math::RotationMatrix::new(yaw, pitch, roll);
    let shift = 8 - bank.shift(0) + half as u8;
    let direct = |index| {
        let rotated = matrix.apply(bank.vertex_bytes(0, index), shift);
        Vec3 {
            x: rotated.x.saturating_add(position.x),
            y: rotated.y.saturating_add(position.y),
            z: rotated.z.saturating_add(position.z),
        }
    };
    let flags = if bank.one_sided(0) { 0 } else { TWO_SIDED };
    let expected: Vec<_> = (0..2)
        .map(|face| {
            let (indices, color) = bank.face(0, face);
            (indices.map(direct), color, flags)
        })
        .collect();
    let mut actual = Capture::default();
    emit_mesh(&mut actual, bank, 0, position, yaw, pitch, roll, half);
    assert_eq!(actual.0, expected, "angles={angles:?}, half={half}");
}

fn test_points(count: usize) -> Vec<[i8; 3]> {
    (0..count)
        .map(|index| {
            let i = index as i32;
            [
                (i * 17 - 113).rem_euclid(255) - 127,
                (127 - i * 11).rem_euclid(255) - 127,
                (i * 23 - 19).rem_euclid(255) - 127,
            ]
            .map(|c| c as i8)
        })
        .collect()
}

#[test]
fn matrix_mesh_rotation_matches_the_direct_reference_for_every_vertex_count() {
    for vertices in [4, 32, 40, 128] {
        let points = test_points(vertices);
        for (shift, one_sided) in [(0, false), (1, true), (MAX_MESH_SHIFT, false)] {
            let bytes = bank_bytes(&points, shift, one_sided);
            let bank = AssetBank::new(&bytes).unwrap();
            let position = Vec3 {
                x: 80,
                y: -70,
                z: 900,
            };
            for angle in (0..=255).step_by(5) {
                for axis in 0..3 {
                    let mut angles = [0; 3];
                    angles[axis] = angle;
                    for half in [false, true] {
                        check_mesh(&bank, angles, half, position);
                    }
                }
            }
            let mut random = 0x1a6b_77f3_u32;
            for index in 0..512 {
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let angles = [random as u8, (random >> 8) as u8, (random >> 16) as u8];
                check_mesh(&bank, angles, index % 3 == 0, position);
            }
        }
    }
}

#[test]
fn identity_rotation_reproduces_stored_coordinates_and_saturates_translation() {
    let points = [[-128i8, 127, -1], [127, -128, 0], [-1, 1, 1], [0, 0, -1]];
    for shift in 0..=MAX_MESH_SHIFT {
        let bytes = bank_bytes(&points, shift, false);
        let bank = AssetBank::new(&bytes).unwrap();
        // With no rotation the matrix is the Q8 identity, so every stored
        // coordinate comes back exactly scaled by the mesh shift.
        let identity = math::RotationMatrix::new(0, 0, 0);
        assert_eq!(identity.m, [[256, 0, 0], [0, 256, 0], [0, 0, 256]]);
        for (index, point) in points.iter().enumerate() {
            assert_eq!(
                identity.apply(*point, 8 - shift),
                bank.vertex(0, index as u8),
                "shift {shift}"
            );
        }
        for offset in [i16::MIN, -1, 0, 1, i16::MAX] {
            check_mesh(
                &bank,
                [0; 3],
                false,
                Vec3 {
                    x: offset,
                    y: offset,
                    z: offset,
                },
            );
        }
    }
}

#[test]
fn composed_matrix_stays_within_q8_unit_bounds_and_matches_single_axis_pairs() {
    // Single-axis rotations must reproduce the classic pair matrices exactly.
    for angle in 0..=255u8 {
        let s = math::sin8(angle);
        let c = math::sin8(angle.wrapping_add(64));
        let yaw = math::RotationMatrix::new(angle, 0, 0);
        assert_eq!(yaw.m, [[c, 0, s], [0, 256, 0], [-s, 0, c]], "yaw {angle}");
        let pitch = math::RotationMatrix::new(0, angle, 0);
        assert_eq!(pitch.m, [[256, 0, 0], [0, c, -s], [0, s, c]], "pitch {angle}");
        let roll = math::RotationMatrix::new(0, 0, angle);
        assert_eq!(roll.m, [[c, -s, 0], [s, c, 0], [0, 0, 256]], "roll {angle}");
    }
    let mut random = 0x5bd1_e995_u32;
    for _ in 0..4096 {
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let m = math::RotationMatrix::new(random as u8, (random >> 8) as u8, (random >> 16) as u8);
        for row in m.m {
            for entry in row {
                assert!((-256..=256).contains(&entry), "{m:?}");
            }
        }
        // Rotating a 100-unit vertex keeps its length within Q8 rounding.
        let v = m.apply([100, 0, 0], 8);
        let length = (v.x as i32).pow(2) + (v.y as i32).pow(2) + (v.z as i32).pow(2);
        assert!((9_400..=10_600).contains(&length), "{m:?} -> {v:?}");
    }
}
