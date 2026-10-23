# Digital music encoder

This standalone tool extends the DLT2 quantizer, Lloyd training, and band-limited
resampler from `secondreality-opt`, under the accompanying Unlicense. It needs
FFmpeg and FFprobe on PATH. Normal game builds use checked-in encoded assets.

## Profiles

| Invocation | Format | Source processing | Stored codes | DAC output |
|---|---|---|---|---|
| `--profile stereo20k3` | DLT4 | Preserve stereo, resample to 20 kHz | 3 bits/channel/frame | Rounded signed 8-bit stereo |
| `--profile mono16k3` | DLT4 | Floating-point `(L+R)/2`, resample to 16 kHz | 3 bits/frame | Rounded signed 8-bit mono |
| No profile option | Legacy DLT3 | Preserve stereo, resample to 24 kHz | 4 bits/channel/frame | Signed 16-bit stereo |

Hammond Rocker and Galactic Dash use `stereo20k3`. The temporary Galactic Dash
mono asset exercises gameplay playback while new recordings are authored. It is
a test fixture, not an additional song in the final soundtrack capacity budget.

## Reproduction

Preserve the original MP3s under `assets/music/source/`; convert them to signed
16-bit FLAC at their original sample rate and channel count. Do not downmix or
resample in FFmpeg: the encoder owns those operations.

```powershell
New-Item -ItemType Directory -Force -Path target/digital-music | Out-Null
ffmpeg -i 'assets/music/source/Galactic Dash.mp3' -vn -sample_fmt s16 -c:a flac target/digital-music/galactic-dash-source.flac
ffmpeg -i 'assets/music/source/Hammond Rocker.mp3' -vn -sample_fmt s16 -c:a flac target/digital-music/hammond-rocker-source.flac
cargo run --release --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build -- --profile stereo20k3 target/digital-music/galactic-dash-source.flac assets/music/galactic-dash.dlt target/digital-music/galactic-dash-dlt4.wav
cargo run --release --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build -- --profile stereo20k3 target/digital-music/hammond-rocker-source.flac assets/music/hammond-rocker.dlt target/digital-music/hammond-rocker-dlt4.wav
cargo run --release --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build -- --profile mono16k3 target/digital-music/galactic-dash-source.flac assets/music/galactic-dash-mono.dlt target/digital-music/galactic-dash-mono-dlt4.wav
```

Regenerating an asset also requires updating its provenance and regenerating the
soundtrack catalog. The positional legacy interface remains
`input.flac output.dlt preview.wav`; it reproduces the previous DLT3 bytes.

## Reconstruction and encoder search

The sequential 512-phase Kaiser-windowed sinc resampler retains floating-point
precision and filter overshoot until normalization. Mono downmix retains half
sample values. One constant track-wide gain preserves dynamics and, for stereo,
left/right balance.

DLT4 starts with eight-entry books trained from source deltas using twenty Lloyd
iterations. Up to three closed-loop retraining passes are retained only when
they improve squared error after final DAC rounding. A 32-trajectory beam search
then selects sample codes across each segment; it is retained only when it
improves on the greedy encoding. This search adds no target CPU work or bits.

Each channel keeps its own signed-16 saturating predictor across codebook changes.
Only publication is rounded:

```text
signed8 = clamp(floor(sample / 256 + 0.5), -128, 127)
```

WAV files expand these values by 256 into signed-16 containers, so they describe
the actual 8-bit DAC waveform. The predictor itself is never rounded to 8 bits.

The decoded sample peak and FFmpeg EBU R128 true-peak upper bounds must remain at
or below -1 dB. Metering includes a loop seam and the transition to silence. If
necessary, reduce gain by the excess plus 0.1 dB and retrain; fail after five
attempts without replacing deliverables. The true-peak bound includes 0.0005
linear amplitude for FFmpeg's rounded metadata.

## DLT4 bytes

The 32-byte little-endian header retains the existing field offsets: `DLT4` magic,
rate at 4, exact frame count at 8, packed byte count at 12, eight entries at 16,
book count at 18, three bits at 20, and one or two channels at 21. Bytes 22–31 are
zero. A frame means one mono sample or one simultaneous stereo pair.

Each segment stores eight three-byte signed-magnitude deltas per channel, left
before right for stereo. Effective deltas clamp to signed-16 before predictor
addition. The regular segment stride is rounded down to a multiple of eight
mono frames or four stereo frames; the final segment takes the remainder.
Predictors do not reset at segment boundaries.

Codes form a continuous least-significant-bit-first stream. Stereo alternates
left and right codes. Eight codes occupy three bytes; the final unused bits are
zero and never decoded. The stream contains `ceil(frames * channels * 3 / 8)`
bytes. Both predictors and the stream CRC reset at the exact song loop.

The entire stream is followed by one cumulative CRC-8 byte per 1,024 packed bytes,
including a final short chunk. Polynomial is `0x07`, initial value zero, without
resetting between chunks.

## Evidence and listening

The JSON records extents, checksums, hashes, profile, normalization, decoded peaks,
and SNR. **SNR is relative to the normalized reference at the selected profile's
rate and channel count.** It does not measure the loss of bandwidth or stereo
information caused by profile selection; do not directly equate it with the
legacy profile's SNR.

For each DLT4 preview, a `.reference.wav` contains the normalized uncompressed
profile reference. Mono additionally writes `.delayed.wav`: left is the direct
waveform, right begins 160 samples later, with a silent prefix and retained final
tail. Compare these with the original stereo recording and legacy decode using
matched listening levels. Check attacks, cymbals, sustained tones, quiet parts,
loop seams, and mono fold-down. Numerical checks do not establish comparable
perceived quality.

```text
cargo test --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build
```

Tests include independent bit-level decoding, partial tails, segment continuity,
all signed-16 rounding inputs, stereo predictor separation, fractional downmix,
beam-search correctness, legacy decoding, normalization and filter response.
