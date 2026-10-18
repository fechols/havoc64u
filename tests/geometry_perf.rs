use havoc::{assets::AssetBank, emit_mesh, math, GeometrySink, Vec3, TWO_SIDED};

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

fn bank_bytes(points: &[Vec3]) -> Vec<u8> {
    let mut bytes = baked_assets::model_bank(0).to_vec();
    let data_offset = havoc::assets::MODEL_DATA_OFFSET;
    let faces = data_offset + points.len() * 6;
    for id in 0..havoc::assets::MODEL_COUNT as usize {
        let at = 16 + id * 8;
        bytes[at..at + 2].copy_from_slice(&(data_offset as u16).to_le_bytes());
        bytes[at + 2] = points.len() as u8;
        bytes[at + 3] = 2;
        bytes[at + 4..at + 6].copy_from_slice(&(faces as u16).to_le_bytes());
    }
    for (index, point) in points.iter().enumerate() {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let at = data_offset + index * 6 + axis * 2;
            bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[faces..faces + 8].copy_from_slice(&[0, 1, 2, 5, 2, points.len() as u8 - 1, 0, 7]);
    bytes
}

fn check_mesh(bank: &AssetBank<'_>, angles: [u8; 3], scale: i16, position: Vec3) {
    let [yaw, pitch, roll] = angles;
    // Keep math::rotate's original per-vertex trigonometry as the independent
    // reference; emit_mesh now uses PreparedRotation internally instead.
    let direct = |index| {
        let point = bank.vertex(0, index);
        let rotated = math::rotate(
            Vec3 {
                x: math::mul_q8(point.x, scale),
                y: math::mul_q8(point.y, scale),
                z: math::mul_q8(point.z, scale),
            },
            yaw,
            pitch,
            roll,
        );
        Vec3 {
            x: rotated.x.saturating_add(position.x),
            y: rotated.y.saturating_add(position.y),
            z: rotated.z.saturating_add(position.z),
        }
    };
    let expected: Vec<_> = (0..2)
        .map(|face| {
            let (indices, color) = bank.face(0, face);
            (indices.map(direct), color, TWO_SIDED)
        })
        .collect();
    let mut actual = Capture::default();
    emit_mesh(&mut actual, bank, 0, position, yaw, pitch, roll, scale);
    assert_eq!(actual.0, expected, "angles={angles:?}, scale={scale}");
}

#[test]
fn prepared_mesh_rotation_preserves_q8_rounding_and_cache_fallback() {
    for vertices in [4, 32, 40] {
        let points: Vec<_> = (0..vertices)
            .map(|index| Vec3 {
                x: index * 17 - 313,
                y: 127 - index * 11,
                z: index * 23 - 419,
            })
            .collect();
        let bytes = bank_bytes(&points);
        let bank = AssetBank::new(&bytes).unwrap();
        let position = Vec3 {
            x: 80,
            y: -70,
            z: 900,
        };
        for angle in 0..=255 {
            for axis in 0..3 {
                let mut angles = [0; 3];
                angles[axis] = angle;
                for scale in [192, 256, 257] {
                    check_mesh(&bank, angles, scale, position);
                }
            }
        }
        let mut random = 0x1a6b_77f3_u32;
        for index in 0..2048 {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let angles = [random as u8, (random >> 8) as u8, (random >> 16) as u8];
            let scale = [-512, -257, -256, -1, 0, 1, 128, 255, 256, 257, 384, 512][index % 12];
            check_mesh(&bank, angles, scale, position);
        }
    }
}

#[test]
fn identity_scale_preserves_i16_extremes_and_translation_saturation() {
    let bytes = bank_bytes(&[
        Vec3 {
            x: i16::MIN,
            y: i16::MAX,
            z: -1,
        },
        Vec3 {
            x: i16::MAX,
            y: i16::MIN,
            z: 0,
        },
        Vec3 { x: -1, y: 1, z: 1 },
        Vec3 { x: 0, y: 0, z: -1 },
    ]);
    let bank = AssetBank::new(&bytes).unwrap();
    for offset in [i16::MIN, -1, 0, 1, i16::MAX] {
        check_mesh(
            &bank,
            [0; 3],
            256,
            Vec3 {
                x: offset,
                y: offset,
                z: offset,
            },
        );
    }
}
