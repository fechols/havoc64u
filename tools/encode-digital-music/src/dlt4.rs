//! Fixed-cost three-bit profiles. All adaptation happens in the host encoder.
use crate::{format::*, lloyd::train_1d_codebook_signed, normalize, resample};
use anyhow::{bail, Result};
use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Stereo20k3,
    Mono16k3,
}

impl Profile {
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "stereo20k3" => Ok(Self::Stereo20k3),
            "mono16k3" => Ok(Self::Mono16k3),
            _ => bail!("unknown profile {name}; expected stereo20k3 or mono16k3"),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Stereo20k3 => "stereo20k3",
            Self::Mono16k3 => "mono16k3",
        }
    }
    fn rate(self) -> u32 {
        match self {
            Self::Stereo20k3 => 20_000,
            Self::Mono16k3 => 16_000,
        }
    }
    fn channels(self) -> usize {
        match self {
            Self::Stereo20k3 => 2,
            Self::Mono16k3 => 1,
        }
    }
}

fn prepare(pcm: &[Vec<i16>; 2], source_rate: u32, profile: Profile) -> Vec<Vec<f64>> {
    if profile == Profile::Mono16k3 {
        let mono: Vec<_> = pcm[0]
            .iter()
            .zip(&pcm[1])
            .map(|(&left, &right)| (f64::from(left) + f64::from(right)) * 0.5)
            .collect();
        vec![resample::resample_f64(&mono, source_rate, profile.rate())]
    } else {
        pcm.iter()
            .map(|channel| resample::resample_pcm(channel, source_rate, profile.rate()))
            .collect()
    }
}

struct Segment {
    codes: Vec<u8>,
    residuals: Vec<i32>,
    end: i16,
    error: u64,
}

fn encode_segment(pcm: &[i16], book: &[i16], mut previous: i16) -> Segment {
    let mut codes = Vec::with_capacity(pcm.len());
    let mut residuals = Vec::with_capacity(pcm.len());
    let mut error = 0;
    for &target in pcm {
        residuals.push(i32::from(target) - i32::from(previous));
        let mut best = 0;
        let mut best_error = i32::MAX;
        for (index, &delta) in book.iter().enumerate() {
            let difference = (i32::from(target) - i32::from(previous.saturating_add(delta))).abs();
            if difference < best_error {
                best = index;
                best_error = difference;
            }
        }
        previous = previous.saturating_add(book[best]);
        let difference = i64::from(target) - i64::from(rounded_output(previous));
        error += (difference * difference) as u64;
        codes.push(best as u8);
    }
    Segment {
        codes,
        residuals,
        end: previous,
        error,
    }
}

fn train(residuals: &[i32]) -> Vec<i16> {
    train_1d_codebook_signed(residuals, 8, 20)
        .into_iter()
        .map(|delta| decode_delta_entry_i16(&encode_delta_entry(delta)))
        .collect()
}

/// Retain 32 distinct predictor trajectories across a whole segment. This
/// searches ahead without adding any target decoder state or stream bits.
fn encode_segment_beam(pcm: &[i16], book: &[i16], previous: i16) -> Segment {
    const WIDTH: usize = 32;
    #[derive(Clone, Copy, Default)]
    struct Node {
        sample: i16,
        error: u64,
        parent: u8,
        code: u8,
    }
    let mut beam = vec![Node {
        sample: previous,
        ..Node::default()
    }];
    let mut paths = Vec::<[(u8, u8); WIDTH]>::with_capacity(pcm.len());
    let mut candidates = Vec::with_capacity(WIDTH * 8);
    for &target in pcm {
        candidates.clear();
        for (parent, node) in beam.iter().enumerate() {
            for (code, &delta) in book.iter().enumerate() {
                let sample = node.sample.saturating_add(delta);
                let difference = i64::from(target) - i64::from(rounded_output(sample));
                candidates.push(Node {
                    sample,
                    error: node.error + (difference * difference) as u64,
                    parent: parent as u8,
                    code: code as u8,
                });
            }
        }
        candidates.sort_unstable_by_key(|n| (n.error, n.sample, n.parent, n.code));
        beam.clear();
        for &node in &candidates {
            if beam.iter().all(|other| other.sample != node.sample) {
                beam.push(node);
                if beam.len() == WIDTH {
                    break;
                }
            }
        }
        let mut path = [(0, 0); WIDTH];
        for (index, node) in beam.iter().enumerate() {
            path[index] = (node.parent, node.code);
        }
        paths.push(path);
    }
    let mut codes = vec![0; pcm.len()];
    let mut index = 0;
    for frame in (0..pcm.len()).rev() {
        let (parent, code) = paths[frame][index];
        codes[frame] = code;
        index = parent as usize;
    }
    let mut residuals = Vec::with_capacity(pcm.len());
    let mut predictor = previous;
    for (&target, &code) in pcm.iter().zip(&codes) {
        residuals.push(i32::from(target) - i32::from(predictor));
        predictor = predictor.saturating_add(book[code as usize]);
    }
    Segment {
        codes,
        residuals,
        end: predictor,
        error: beam[0].error,
    }
}

pub fn encode(pcm: &[Vec<i16>], profile: Profile, requested_books: u16) -> Result<DeltaBlob> {
    let channels = profile.channels();
    if pcm.len() != channels || pcm[0].is_empty() || pcm.iter().any(|c| c.len() != pcm[0].len()) {
        bail!("profile requires nonempty, equal-length channels");
    }
    if requested_books == 0 {
        bail!("codebook count must be 1..=65535");
    }
    let frames: u32 = pcm[0].len().try_into()?;
    let segments = BalancedSegments::new(frames, requested_books, channels as u8);
    let books = segments.books;
    let mut predictors = [0i16; 2];
    let mut codes = vec![0u8; pcm[0].len() * channels];
    let mut codebook_bytes = Vec::with_capacity(books as usize * channels * 8 * COMPACT_ENTRY_SIZE);
    for segment in 0..books as usize {
        let range = segments.range(segment as u16);
        let (start, end) = (range.start, range.end);
        for channel in 0..channels {
            let source = &pcm[channel][start..end];
            let mut source_previous = if start == 0 {
                0
            } else {
                pcm[channel][start - 1]
            };
            let deltas: Vec<_> = source
                .iter()
                .map(|&sample| {
                    let delta = i32::from(sample) - i32::from(source_previous);
                    source_previous = sample;
                    delta
                })
                .collect();
            let mut book = train(&deltas);
            let mut best = encode_segment(source, &book, predictors[channel]);
            // Retrain on the actual closed-loop residuals, retaining an update
            // only when the final rounded DAC output has lower squared error.
            for _ in 0..3 {
                let candidate_book = train(&best.residuals);
                let candidate = encode_segment(source, &candidate_book, predictors[channel]);
                if candidate.error >= best.error {
                    break;
                }
                book = candidate_book;
                best = candidate;
            }
            let searched = encode_segment_beam(source, &book, predictors[channel]);
            if searched.error < best.error {
                best = searched;
            }
            for &delta in &book {
                codebook_bytes.extend_from_slice(&delta.to_le_bytes());
            }
            for (index, &code) in best.codes.iter().enumerate() {
                codes[(start + index) * channels + channel] = code;
            }
            predictors[channel] = best.end;
        }
    }
    let stream = pack_codes(&codes, 3);
    Ok(DeltaBlob {
        compact_entries: true,
        header: DeltaQuantHeader {
            rate_hz: profile.rate(),
            n_samples: frames,
            stream_bytes: stream.len().try_into()?,
            codebook_entries: 8,
            num_codebooks: books,
            bits_per_sample: 3,
            channels: channels as u8,
        },
        codebook_bytes,
        stream,
    })
}

fn interleave(pcm: &[Vec<i16>]) -> Vec<i16> {
    (0..pcm[0].len())
        .flat_map(|frame| pcm.iter().map(move |channel| channel[frame]))
        .collect()
}

pub fn run(
    profile: Profile,
    requested_books: u16,
    input: &Path,
    output: &Path,
    preview: &Path,
) -> Result<()> {
    let (source, source_rate) = crate::read_source(input)?;
    let pcm = prepare(&source, source_rate, profile);
    let channels = profile.channels();
    let peak = pcm
        .iter()
        .map(|c| normalize::unclipped_peak(c))
        .fold(0.0, f64::max);
    let initial_gain = if peak == 0.0 {
        1.0
    } else {
        (32768.0 * 10f64.powf(-1.0 / 20.0)).floor() / peak
    };
    let mut gain = initial_gain;
    let mut accepted = None;
    for attempt in 1..=normalize::MAX_ATTEMPTS {
        let reference: Vec<Vec<i16>> = pcm
            .iter()
            .map(|channel| {
                channel
                    .iter()
                    .map(|&sample| {
                        (sample * gain)
                            .round()
                            .clamp(i16::MIN as f64, i16::MAX as f64) as i16
                    })
                    .collect()
            })
            .collect();
        let encoded = encode(&reference, profile, requested_books)?;
        let decoded: Vec<_> = encoded.decode().into_iter().map(rounded_output).collect();
        // Include a real loop boundary, not only the final transition to silence.
        let mut meter_pcm = decoded.clone();
        meter_pcm.extend_from_slice(
            &decoded[..decoded.len().min(profile.rate() as usize / 10 * channels)],
        );
        let meter = crate::measure_profile(&meter_pcm, profile.rate(), channels as u16)?;
        let maximum = meter
            .true_peak_dbtp
            .iter()
            .copied()
            .fold(normalize::db(normalize::peak(&decoded) / 32768.0), f64::max);
        if maximum <= normalize::TARGET_DB {
            accepted = Some((encoded, reference, decoded, meter, attempt));
            break;
        }
        let reduction = maximum - normalize::TARGET_DB + 0.1;
        eprintln!("Normalization attempt {attempt}: decoded peak {maximum:.4} dB; reducing gain by {reduction:.4} dB");
        gain *= 10f64.powf(-reduction / 20.0);
    }
    let Some((encoded, reference, decoded, meter, attempts)) = accepted else {
        bail!(
            "decoded peak exceeds -1 dBTP after {} attempts; outputs were not replaced",
            normalize::MAX_ATTEMPTS
        );
    };
    let mut bytes = encoded.to_bytes();
    let stream_offset = 32 + encoded.codebook_bytes.len();
    let crc_offset = bytes.len();
    let mut crc = 0;
    for chunk in encoded.stream.chunks(1024) {
        for &byte in chunk {
            crc = crc_byte(crc, byte);
        }
        bytes.push(crc);
    }
    let channel_metrics: Vec<_> =
        (0..channels)
            .map(|channel| {
                let samples: Vec<_> = decoded
                    .iter()
                    .skip(channel)
                    .step_by(channels)
                    .copied()
                    .collect();
                format!(
                    concat!("    {{\"channel\": \"{}\", \"input_sample_peak_dbfs\": {}, ",
            "\"normalized_sample_peak_dbfs\": {}, \"decoded_sample_peak_dbfs\": {}, ",
            "\"decoded_true_peak_upper_bound_dbtp\": {}, \"matched_level_snr_db\": {}}}"),
                    if channels == 1 {
                        "mono"
                    } else if channel == 0 {
                        "left"
                    } else {
                        "right"
                    },
                    crate::json_number(normalize::db(
                        normalize::unclipped_peak(&pcm[channel]) / 32768.0
                    )),
                    crate::json_number(normalize::db(
                        normalize::peak(&reference[channel]) / 32768.0
                    )),
                    crate::json_number(normalize::db(normalize::peak(&samples) / 32768.0)),
                    crate::json_number(meter.true_peak_dbtp[channel]),
                    crate::json_number(crate::snr(&reference[channel], &samples))
                )
            })
            .collect();
    let reference = interleave(&reference);
    let metadata = format!(concat!(
        "{{\n  \"format\": \"DLT5 with cumulative CRC-8 per 1024 stream bytes\",\n",
        "  \"profile\": \"{}\",\n  \"sample_rate\": {},\n  \"sample_count\": {},\n",
        "  \"channels\": {},\n  \"bits_per_sample\": 3,\n  \"output_bits_per_sample\": 8,\n",
        "  \"codebook_entries\": 8,\n  \"codebook_count\": {},\n  \"samples_per_segment\": {},\n  \"long_segment_count\": {},\n  \"codebook_entry_bytes\": 2,\n",
        "  \"header_bytes\": 32,\n  \"codebook_bytes\": {},\n  \"stream_offset\": {},\n",
        "  \"stream_bytes\": {},\n  \"crc_offset\": {},\n  \"crc_count\": {},\n  \"asset_bytes\": {},\n",
        "  \"header_checksum\": {},\n  \"codebook_checksum\": {},\n  \"crc_table_checksum\": {},\n",
        "  \"asset_checksum\": {},\n  \"asset_sha256\": \"{}\",\n  \"flac_sha256\": \"{}\",\n",
        "  \"reference_pcm_sha256\": \"{}\",\n  \"decoded_pcm_sha256\": \"{}\",\n",
        "  \"matched_level_snr_db\": {},\n  \"normalization_target_dbfs\": -1.0,\n",
        "  \"decoded_true_peak_limit_dbtp\": -1.0,\n  \"normalization_initial_gain_db\": {},\n",
        "  \"normalization_gain_db\": {},\n  \"normalization_attempts\": {},\n",
        "  \"decoded_integrated_lufs\": {},\n  \"closed_loop_training_passes\": 3,\n  \"encoder_beam_width\": 32,\n",
        "  \"true_peak_meter\": \"FFmpeg EBU R128; loop seam included; upper bounds include 0.0005 linear metadata rounding margin\",\n",
        "  \"channel_metrics\": [\n{}\n  ]\n}}\n"),
        profile.name(), profile.rate(), encoded.header.n_samples, channels, encoded.header.num_codebooks,
        BalancedSegments::new(encoded.header.n_samples, encoded.header.num_codebooks, channels as u8).samples_per_segment,
        BalancedSegments::new(encoded.header.n_samples, encoded.header.num_codebooks, channels as u8).long_segment_count,
        encoded.codebook_bytes.len(), stream_offset, encoded.stream.len(), crc_offset, bytes.len() - crc_offset, bytes.len(),
        checksum(&bytes[..32]), checksum(&encoded.codebook_bytes), checksum(&bytes[crc_offset..]), checksum(&bytes),
        crate::hash(&bytes), crate::hash(&fs::read(input)?), crate::hash(&crate::pcm_bytes(&reference)), crate::hash(&crate::pcm_bytes(&decoded)),
        crate::json_number(crate::snr(&reference, &decoded)), crate::json_number(normalize::db(initial_gain)), crate::json_number(normalize::db(gain)),
        attempts, crate::json_number(meter.integrated_lufs), channel_metrics.join(",\n"));
    fs::write(output, &bytes)?;
    crate::write_profile_wav(preview, &decoded, profile.rate(), channels as u16)?;
    fs::write(output.with_extension("json"), &metadata)?;
    // Reference WAV distinguishes coding error from downmix/resampling changes.
    crate::write_profile_wav(
        &preview.with_extension("reference.wav"),
        &reference,
        profile.rate(),
        channels as u16,
    )?;
    if channels == 1 {
        let delay = profile.rate() as usize / 100;
        let delayed: Vec<_> = (0..decoded.len() + delay)
            .flat_map(|frame| {
                [
                    decoded.get(frame).copied().unwrap_or(0),
                    frame
                        .checked_sub(delay)
                        .and_then(|at| decoded.get(at))
                        .copied()
                        .unwrap_or(0),
                ]
            })
            .collect();
        crate::write_profile_wav(
            &preview.with_extension("delayed.wav"),
            &delayed,
            profile.rate(),
            2,
        )?;
    }
    print!("{metadata}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beam_search_finds_the_best_two_sample_path_without_changing_decoder() {
        let book = [-2000, -1000, -500, -100, 100, 500, 1000, 2000];
        let pcm = [780, -1400];
        let result = encode_segment_beam(&pcm, &book, 0);
        let mut exhaustive = u64::MAX;
        for &first in &book {
            for &second in &book {
                let a = i64::from(pcm[0]) - i64::from(rounded_output(first));
                let b = i64::from(pcm[1]) - i64::from(rounded_output(first.saturating_add(second)));
                exhaustive = exhaustive.min((a * a + b * b) as u64);
            }
        }
        assert_eq!(result.error, exhaustive);
    }

    #[test]
    fn downmix_keeps_fractional_samples_before_filtering() {
        let pcm = [vec![0, 32767, -32768], vec![1, -32768, 32767]];
        assert_eq!(
            prepare(&pcm, 16000, Profile::Mono16k3)[0],
            [0.5, -0.5, -0.5]
        );
    }

    #[test]
    fn independent_bit_oracle_matches_both_profiles_and_partial_tails() {
        for profile in [Profile::Mono16k3, Profile::Stereo20k3] {
            for frames in [1, 3, 7, 8, 9, 31, 113] {
                let pcm: Vec<Vec<_>> = (0..profile.channels())
                    .map(|c| {
                        (0..frames)
                            .map(|n| (((n * (137 + c * 193)) % 16000) as i16) - 8000)
                            .collect()
                    })
                    .collect();
                let blob = encode(&pcm, profile, 3).unwrap();
                let mut accum = [0i16; 2];
                let mut expected = Vec::new();
                // Independent partition oracle: distribute complete packing groups,
                // then clip only the final group to the exact source length.
                let alignment = 8 / profile.channels();
                let groups = frames.div_ceil(alignment);
                let books = blob.header.num_codebooks as usize;
                let mut boundaries = Vec::with_capacity(books);
                let mut boundary = 0;
                for book in 0..books {
                    boundary += (groups / books + usize::from(book < groups % books)) * alignment;
                    boundaries.push(boundary.min(frames));
                }
                for frame in 0..frames {
                    let segment = boundaries.iter().position(|&end| frame < end).unwrap();
                    for channel in 0..profile.channels() {
                        let bit = (frame * profile.channels() + channel) * 3;
                        let code = (0..3).fold(0usize, |code, shift| {
                            code | (((blob.stream[(bit + shift) / 8] >> ((bit + shift) % 8)) & 1)
                                as usize)
                                << shift
                        });
                        let at = ((segment * profile.channels() + channel) * 8 + code) * 2;
                        let delta = i16::from_le_bytes([
                            blob.codebook_bytes[at],
                            blob.codebook_bytes[at + 1],
                        ]) as i32;
                        accum[channel] =
                            (i32::from(accum[channel]) + delta).clamp(-32768, 32767) as i16;
                        expected.push(accum[channel]);
                    }
                }
                assert_eq!(blob.decode(), expected);
                assert_eq!(&blob.to_bytes()[..4], b"DLT5");
                assert_eq!(blob.codebook_bytes.len(), books * profile.channels() * 16);
                assert_eq!(
                    blob.stream.len(),
                    (frames * profile.channels() * 3).div_ceil(8)
                );
                let used = frames * profile.channels() * 3 % 8;
                if used != 0 {
                    assert_eq!(blob.stream.last().unwrap() >> used, 0);
                }
            }
        }
    }

    #[test]
    fn accepts_more_than_1024_books_and_caps_at_partial_packing_groups() {
        let pcm = vec![vec![0; 8193]];
        let blob = encode(&pcm, Profile::Mono16k3, u16::MAX).unwrap();
        assert_eq!(blob.header.num_codebooks, 1025);
        assert_eq!(blob.codebook_bytes.len(), 1025 * 16);
        assert_eq!(blob.decode(), vec![0; 8193]);
        assert!(encode(&pcm, Profile::Mono16k3, 0).is_err());
        assert_eq!(
            blob.to_bytes(),
            encode(&pcm, Profile::Mono16k3, u16::MAX)
                .unwrap()
                .to_bytes()
        );
    }
}
