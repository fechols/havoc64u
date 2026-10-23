# Digital music encoder

This standalone tool extends the DLT2 quantizer, Lloyd training, and band-limited
resampler from `secondreality-opt` into normalized DLT3 stereo, under the
accompanying Unlicense. It is independent of that checkout. FFmpeg produces
signed 16-bit source PCM, preserving the original left/right channels. Each
channel passes through the same sequential 512-phase Kaiser-windowed sinc
resampler to 24 kHz. Floating-point output retains filter overshoot and fractional
precision until normalization; there is no fixed input attenuation or mono mix.
Mono input is duplicated into both output channels.

The louder resampled channel sets one constant gain for the whole track, targeting
−1 dBFS before quantization. That shared gain preserves stereo balance and musical
dynamics; silence retains unity gain. Each channel then gets its own codebooks
and saturating predictor, using twenty Lloyd iterations and closed-loop encoding.
The decoded result is measured with FFmpeg's EBU R128 true-peak meter. If its
sample peak or either channel's true-peak upper bound exceeds −1 dBTP, the tool
reduces the shared gain by the excess plus 0.1 dB and re-encodes. It allows five
attempts and opens no output files until validation succeeds. True-peak bounds
include a 0.0005 linear margin for FFmpeg's rounded metadata readings.

The original MP3s are preserved in `assets/music/source/`. Their SHA-256 hashes
match the `source.sha256` values in the existing `assets/music/*.provenance.json`
records. The encoder input is a 16-bit FLAC preserving the MP3's original sample
rate and channels. FFmpeg and FFprobe must be on PATH. From the repository root:

```powershell
New-Item -ItemType Directory -Force -Path target/digital-music | Out-Null
ffmpeg -i 'assets/music/source/Galactic Dash.mp3' -vn -sample_fmt s16 -c:a flac target/digital-music/galactic-dash-source.flac
cargo run --release --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build -- target/digital-music/galactic-dash-source.flac assets/music/galactic-dash.dlt target/digital-music/galactic-dash-decoded.wav
ffmpeg -i 'assets/music/source/Hammond Rocker.mp3' -vn -sample_fmt s16 -c:a flac target/digital-music/hammond-rocker-source.flac
cargo run --release --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build -- target/digital-music/hammond-rocker-source.flac assets/music/hammond-rocker.dlt target/digital-music/hammond-rocker-decoded.wav
```

The positional interface remains `input.flac output.dlt preview.wav`. DLT3 has a
32-byte header with stereo frame counts, four bits per channel, and channel count
two at byte 21; bytes 22–31 are reserved zero. Each segment stores its 48-byte left
codebook followed by its 48-byte right codebook. Segment lengths retain the
original even-frame alignment. Each stream byte holds one complete stereo frame:
left code in the low nibble, right code in the high nibble.

One cumulative CRC-8 byte follows per 1024 packed stream bytes, including the
final short chunk. CRC uses polynomial `0x07`, initial value zero, and does not
reset between chunks. The adjacent JSON records byte extents, Fletcher checksums,
SHA-256 hashes, shared normalization gain and attempt count, integrated decoded
loudness, and per-channel input, normalized and decoded peaks, true-peak upper
bounds, and matched-level SNR. JSON `sample_count` and `samples_per_segment` count
stereo frames. The WAV is the decoded 24 kHz stereo preview for listening and
target playback comparisons.

Both checked-in tracks use 16-entry codebooks per channel, 521 paired segments,
four bits per channel per frame, and a 24 kHz frame rate. Regenerating a song also
requires updating its provenance record and the
shipping descriptor in `src/digital.rs`; normal game builds consume the checked-in
assets and do not need the MP3, FFmpeg, or this tool.

Run encoder tests with:

```text
cargo test --manifest-path tools/encode-digital-music/Cargo.toml --target-dir target/digital-music/tool-build
```
