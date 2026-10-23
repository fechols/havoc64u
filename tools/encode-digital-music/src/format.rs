//! DLT3 stereo frames, extending Second Reality's DLT2 quantizer (Unlicense).
pub const DELTA_QUANT_ENTRY_SIZE: usize = 3;

pub fn packing_group(bits: u8, channels: u8) -> u32 {
    let frame_bits = u32::from(bits) * u32::from(channels);
    let mut group = 1;
    while group * frame_bits % 8 != 0 {
        group += 1;
    }
    group
}

pub fn profile_codebook_count(samples: u32, requested: u16, bits: u8, channels: u8) -> u16 {
    (requested.max(1) as u32).min((samples / packing_group(bits, channels)).max(1)) as u16
}

pub fn profile_samples_per_segment(samples: u32, requested: u16, bits: u8, channels: u8) -> u32 {
    let count = profile_codebook_count(samples, requested, bits, channels) as u32;
    if count <= 1 {
        samples
    } else {
        let group = packing_group(bits, channels);
        samples / count / group * group
    }
}

pub fn pack_codes(codes: &[u8], bits: u8) -> Vec<u8> {
    let mut packed = vec![0; (codes.len() * bits as usize).div_ceil(8)];
    for (index, &code) in codes.iter().enumerate() {
        assert!(u16::from(code) < 1u16 << bits);
        let bit = index * bits as usize;
        packed[bit / 8] |= code << (bit % 8);
        if bit % 8 + bits as usize > 8 {
            packed[bit / 8 + 1] |= code >> (8 - bit % 8);
        }
    }
    packed
}

pub fn unpack_code(packed: &[u8], index: usize, bits: u8) -> u8 {
    let bit = index * bits as usize;
    let mut value = u16::from(packed[bit / 8]);
    if bit % 8 + bits as usize > 8 {
        value |= u16::from(packed[bit / 8 + 1]) << 8;
    }
    ((value >> (bit % 8)) & ((1u16 << bits) - 1)) as u8
}

/// Return the signed-8 DAC value expanded to signed-16 for WAVs and metering.
pub fn rounded_output(sample: i16) -> i16 {
    (((i32::from(sample) + 128) >> 8).clamp(-128, 127) * 256) as i16
}

pub fn delta_codebook_count(samples: u32, requested: u16, bits: u8) -> u16 {
    profile_codebook_count(samples, requested, bits, 1)
}

pub fn delta_samples_per_segment(samples: u32, count: u16, bits: u8) -> u32 {
    let count = delta_codebook_count(samples, count, bits) as u32;
    if count <= 1 {
        samples
    } else {
        let group = packing_group(bits, 1);
        samples / count / group * group
    }
}

pub fn encode_delta_entry(value: i32) -> [u8; 3] {
    let magnitude = value.unsigned_abs().min(u16::MAX as u32) as u16;
    [magnitude as u8, (magnitude >> 8) as u8, u8::from(value < 0)]
}

pub fn decode_delta_entry_i16(bytes: &[u8]) -> i16 {
    let magnitude = u16::from_le_bytes([bytes[0], bytes[1]]) as i32;
    let signed = if bytes[2] != 0 { -magnitude } else { magnitude };
    signed.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

pub struct DeltaQuantHeader {
    pub rate_hz: u32,
    pub n_samples: u32,
    pub stream_bytes: u32,
    pub codebook_entries: u16,
    pub num_codebooks: u16,
    pub bits_per_sample: u8,
    pub channels: u8,
}

pub struct DeltaBlob {
    pub header: DeltaQuantHeader,
    pub codebook_bytes: Vec<u8>,
    pub stream: Vec<u8>,
}

impl DeltaBlob {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.codebook_bytes.len() + self.stream.len());
        out.extend_from_slice(if self.header.bits_per_sample == 3 {
            b"DLT4"
        } else if self.header.channels == 2 {
            b"DLT3"
        } else {
            b"DLT2"
        });
        out.extend_from_slice(&self.header.rate_hz.to_le_bytes());
        out.extend_from_slice(&self.header.n_samples.to_le_bytes());
        out.extend_from_slice(&self.header.stream_bytes.to_le_bytes());
        out.extend_from_slice(&self.header.codebook_entries.to_le_bytes());
        out.extend_from_slice(&self.header.num_codebooks.to_le_bytes());
        out.push(self.header.bits_per_sample);
        out.push(if self.header.bits_per_sample == 3 {
            self.header.channels
        } else if self.header.channels == 2 {
            2
        } else {
            0
        });
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(&self.codebook_bytes);
        out.extend_from_slice(&self.stream);
        out
    }

    pub fn decode(&self) -> Vec<i16> {
        let stride = if self.header.bits_per_sample == 3 {
            profile_samples_per_segment(
                self.header.n_samples,
                self.header.num_codebooks,
                3,
                self.header.channels,
            )
        } else {
            delta_samples_per_segment(
                self.header.n_samples,
                self.header.num_codebooks,
                self.header.bits_per_sample,
            )
        };
        let channels = self.header.channels as usize;
        let mut accumulators = [0i16; 2];
        let mut decoded = Vec::with_capacity(self.header.n_samples as usize * channels);
        for frame in 0..self.header.n_samples {
            let segment = (frame / stride).min(self.header.num_codebooks as u32 - 1) as usize;
            for (channel, accumulator) in accumulators.iter_mut().enumerate().take(channels) {
                let sample = frame as usize * channels + channel;
                let index = unpack_code(&self.stream, sample, self.header.bits_per_sample);
                let at = ((segment * channels + channel) * self.header.codebook_entries as usize
                    + index as usize)
                    * DELTA_QUANT_ENTRY_SIZE;
                *accumulator = accumulator
                    .saturating_add(decode_delta_entry_i16(&self.codebook_bytes[at..at + 3]));
                decoded.push(*accumulator);
            }
        }
        decoded
    }
}

/// CRC stays cumulative across chunks, as in the source sampler backend.
pub fn crc_byte(mut crc: u8, byte: u8) -> u8 {
    crc ^= byte;
    for _ in 0..8 {
        crc = (crc << 1) ^ if crc & 128 != 0 { 7 } else { 0 };
    }
    crc
}

pub fn checksum(bytes: &[u8]) -> u16 {
    let (mut a, mut b) = (0u16, 0u16);
    for &byte in bytes {
        a = (a + byte as u16) % 255;
        b = (b + a) % 255;
    }
    a | b << 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_bit_packing_crosses_every_byte_alignment_and_zero_pads_tails() {
        for count in 1..=80 {
            let codes: Vec<_> = (0..count).map(|i| ((i * 3 + 1) & 7) as u8).collect();
            let packed = pack_codes(&codes, 3);
            for (index, &expected) in codes.iter().enumerate() {
                assert_eq!(unpack_code(&packed, index, 3), expected);
                let independently_read = (0..3).fold(0u8, |value, shift| {
                    let bit = index * 3 + shift;
                    value | (((packed[bit / 8] >> (bit % 8)) & 1) << shift)
                });
                assert_eq!(independently_read, expected);
            }
            if count * 3 % 8 != 0 {
                assert_eq!(packed.last().unwrap() >> (count * 3 % 8), 0);
            }
        }
        assert_eq!(pack_codes(&[0, 1, 2, 3, 4, 5, 6, 7], 3), [0x88, 0xc6, 0xfa]);
        assert_eq!(packing_group(3, 1), 8);
        assert_eq!(packing_group(3, 2), 4);
    }

    #[test]
    fn rounded_output_matches_definition_for_all_signed_samples() {
        for sample in i16::MIN..=i16::MAX {
            let expected = ((f64::from(sample) / 256.0 + 0.5)
                .floor()
                .clamp(-128.0, 127.0)
                * 256.0) as i16;
            assert_eq!(rounded_output(sample), expected);
        }
    }

    #[test]
    fn source_dlt2_alignment_and_signed_saturation() {
        assert_eq!(delta_samples_per_segment(31, 3, 4), 10);
        assert_eq!(
            decode_delta_entry_i16(&encode_delta_entry(65_535)),
            i16::MAX
        );
        assert_eq!(
            decode_delta_entry_i16(&encode_delta_entry(-65_535)),
            i16::MIN
        );
        assert_eq!(decode_delta_entry_i16(&encode_delta_entry(-1234)), -1234);
        assert_eq!(
            b"123456789"
                .iter()
                .fold(0, |crc, &byte| crc_byte(crc, byte)),
            0xf4
        );
    }

    #[test]
    fn packing_and_odd_final_sample_decode_with_segment_changes() {
        let blob = DeltaBlob {
            header: DeltaQuantHeader {
                rate_hz: 24_000,
                n_samples: 7,
                stream_bytes: 4,
                codebook_entries: 2,
                num_codebooks: 3,
                bits_per_sample: 4,
                channels: 1,
            },
            codebook_bytes: [1, 2, 10, 20, -1, -2]
                .iter()
                .flat_map(|&x| encode_delta_entry(x))
                .collect(),
            stream: vec![0x10, 0x10, 0x10, 0x00],
        };
        assert_eq!(blob.decode(), [1, 3, 13, 33, 32, 30, 29]);
    }

    #[test]
    fn stereo_nibbles_and_predictors_remain_independent_across_segments() {
        let blob = DeltaBlob {
            header: DeltaQuantHeader {
                rate_hz: 24_000,
                n_samples: 5,
                stream_bytes: 5,
                codebook_entries: 2,
                num_codebooks: 2,
                bits_per_sample: 4,
                channels: 2,
            },
            // Each segment contains a complete left book then a complete right book.
            codebook_bytes: [1, 2, -10, -20, 100, 200, -1000, -2000]
                .iter()
                .flat_map(|&x| encode_delta_entry(x))
                .collect(),
            stream: vec![0x10, 0x01, 0x10, 0x01, 0x10],
        };
        assert_eq!(
            blob.decode(),
            [1, -20, 3, -30, 103, -2030, 303, -3030, 403, -5030]
        );
        let bytes = blob.to_bytes();
        assert_eq!(&bytes[..4], b"DLT3");
        assert_eq!(bytes[21], 2);
        assert_eq!(&bytes[22..32], &[0; 10]);
    }
}
