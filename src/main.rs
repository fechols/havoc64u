mod host;
use havoc::{
    assets::*,
    game::{Game, InputSample, FIRE, TRANSITION_TICKS},
    hud, hud_sprites,
    render::{self, Renderer},
    space_dust::SpaceDustFrame,
};
use std::{env, fs, path::Path};

fn frame(
    game: &Game,
    bank: &AssetBank<'_>,
    segment: &Segment<'_>,
    number: u16,
) -> (Vec<u8>, u16, u16) {
    let mut r = Renderer::new();
    r.begin(game.sky_color());
    game.render(&mut r, bank, segment);
    r.finish();
    let status = game.hud();
    let mut dust = SpaceDustFrame::EMPTY;
    let ambient = dust.prepare(status.section, status.mode, game.animation_ticks());
    let mut output = vec![0; 10_001];
    for band in 0..25u8 {
        let mut pixels = [0; 1280];
        let mut bitmap = [0; 320];
        let mut screen = [0; 40];
        let mut color = [0; 40];
        r.render_band_with_ambient(band, &mut pixels, ambient);
        hud::draw(band, &mut pixels, &status, number);
        render::pack_band(
            &pixels,
            &[0; 40],
            &[0; 40],
            r.clear_color(),
            &mut bitmap,
            &mut screen,
            &mut color,
        );
        let n = band as usize;
        output[n * 320..n * 320 + 320].copy_from_slice(&bitmap);
        output[8000 + n * 40..8000 + n * 40 + 40].copy_from_slice(&screen);
        output[9000 + n * 40..9000 + n * 40 + 40].copy_from_slice(&color);
    }
    // Byte 10,000 is the VIC background ($D021): the frame's clear colour.
    output[10_000] = r.clear_color();
    (output, r.triangle_count, r.dropped)
}
fn ppm(frame: &[u8]) -> Vec<u8> {
    let background = frame.get(10_000).copied().unwrap_or(0) & 15;
    let mut out = b"P6\n640 480\n255\n".to_vec();
    for py in 0..480 {
        let y = py * 200 / 480;
        for px in 0..640 {
            let x = px / 4;
            let cell = (y / 8) * 40 + x / 4;
            let byte = frame[cell * 8 + y % 8];
            let slot = (byte >> (6 - (x % 4) * 2)) & 3;
            let c = match slot {
                0 => background,
                1 => frame[8000 + cell] >> 4,
                2 => frame[8000 + cell] & 15,
                _ => frame[9000 + cell] & 15,
            };
            out.extend_from_slice(&render::PALETTE_RGB[c as usize]);
        }
    }
    out
}

/// Sidecar state uses actual VIC coordinates and bytes, just like native
/// captures. Keep it separate from the longstanding bitmap-only .bin format.
struct SpriteCapture {
    registers: [u8; 47],
    images: [[u8; 64]; 8],
}

impl SpriteCapture {
    fn new(game: &Game, frame: u16) -> Self {
        let display = hud_sprites::DisplayState::from_hud(&game.hud(), frame, None);
        let mut capture = Self {
            registers: [0; 47],
            images: [[0; 64]; 8],
        };
        capture.registers[0x11] = 0x3b;
        capture.registers[0x16] = 0x18;
        capture.registers[0x18] = 0x08;
        capture.registers[0x15] = display.enabled_mask();
        capture.registers[0x1c] = 0xff;
        capture.registers[0x25] = 12;
        capture.registers[0x26] = 1;
        for corner in 0..4u8 {
            let mut images = [0; 128];
            hud_sprites::render_corner(corner, &display, &mut images);
            let (x, y) = hud_sprites::CORNER_POSITIONS[corner as usize];
            for half in 0..2usize {
                let sprite = corner as usize * 2 + half;
                let vic_x = 24 + x as u16 * 2 + half as u16 * 24;
                capture.registers[sprite * 2] = vic_x as u8;
                capture.registers[sprite * 2 + 1] = 50 + y as u8;
                capture.registers[0x10] |= ((vic_x >> 8) as u8) << sprite;
                capture.registers[0x27 + sprite] = display.accent(corner);
                capture.images[sprite].copy_from_slice(&images[half * 64..half * 64 + 64]);
            }
        }
        capture
    }

    fn json(&self) -> String {
        let images = self
            .images
            .iter()
            .map(|image| {
                format!(
                    "\"{}\"",
                    image.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"format\":\"vic-sprites-v1\",\"bank_base\":32768,\"screen_base\":32768,\"origin\":[24,50],\"registers\":{:?},\"pointers\":[48,49,50,51,52,53,54,55],\"images\":[{}]}}\n", self.registers, images)
    }

    fn ppm(&self, frame: &[u8]) -> Vec<u8> {
        let mut output = ppm(frame);
        let header = b"P6\n640 480\n255\n".len();
        for py in 0..480usize {
            let y = py * 200 / 480;
            for px in 0..640usize {
                let x = px / 2;
                // Lower numbered sprites win when nontransparent pixels overlap.
                for sprite in 0..8usize {
                    if self.registers[0x15] & (1 << sprite) == 0 {
                        continue;
                    }
                    let sx = self.registers[sprite * 2] as i16
                        + (((self.registers[0x10] >> sprite) & 1) as i16) * 256
                        - 24;
                    let sy = self.registers[sprite * 2 + 1] as i16 - 50;
                    let dx = x as i16 - sx;
                    let dy = y as i16 - sy;
                    if !(0..24).contains(&dx) || !(0..21).contains(&dy) {
                        continue;
                    }
                    let byte = self.images[sprite][dy as usize * 3 + dx as usize / 8];
                    let code = (byte >> (6 - (dx as usize / 2 % 4) * 2)) & 3;
                    let color = match code {
                        0 => continue,
                        1 => self.registers[0x25],
                        2 => self.registers[0x27 + sprite],
                        _ => self.registers[0x26],
                    } & 15;
                    let at = header + (py * 640 + px) * 3;
                    output[at..at + 3].copy_from_slice(&render::PALETTE_RGB[color as usize]);
                    break;
                }
            }
        }
        output
    }
}
enum Snapshot {
    Title,
    Section(u8, u16),
    Attract(u32),
    Transition(u8, u16),
    Victory(u16),
}

fn snapshot(source: Snapshot, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let models: [_; SEGMENT_COUNT as usize] = core::array::from_fn(|i| host::model_bank(i as u8));
    let hud_frame = match &source {
        Snapshot::Victory(elapsed_ticks) => *elapsed_ticks,
        _ => 0,
    };
    let game = match source {
        Snapshot::Title => Game::new(),
        Snapshot::Victory(elapsed_ticks) => Game::preview_victory(elapsed_ticks),
        Snapshot::Transition(section, elapsed_ticks) => {
            Game::preview_transition(section, elapsed_ticks)
        }
        Snapshot::Section(section, distance) => {
            let mut game = Game::preview_at(section, distance);
            let data = host::segment_bank(game.segment_index());
            let segment = Segment::new(&data).unwrap();
            game.sample_input(InputSample {
                bits: FIRE,
                pressed: 0,
                ms: 6000,
            });
            let bank = AssetBank::new(&models[game.segment_index() as usize]).unwrap();
            game.tick(&segment, &bank);
            game
        }
        Snapshot::Attract(elapsed_ms) => {
            let banks: [_; SEGMENT_COUNT as usize] =
                core::array::from_fn(|i| host::segment_bank(i as u8));
            let mut game = Game::new();
            // Replay actual 20-ms steps, including title idle and repeat cycles.
            // Fractional steps have not occurred yet at the requested timestamp.
            for tick in 1..=elapsed_ms / 20 {
                game.sample_input(InputSample {
                    bits: 0,
                    pressed: 0,
                    ms: (tick * 20) as u16,
                });
                let segment = Segment::new(&banks[game.segment_index() as usize]).unwrap();
                let bank = AssetBank::new(&models[game.segment_index() as usize]).unwrap();
                game.tick(&segment, &bank);
            }
            game
        }
    };
    // A simulation step may transition sections; render its resulting section.
    let data = host::segment_bank(game.segment_index());
    let segment = Segment::new(&data).unwrap();
    let bank = AssetBank::new(&models[game.segment_index() as usize]).unwrap();
    let (bytes, triangles, dropped) = frame(&game, &bank, &segment, hud_frame);
    if let Some(p) = out.parent() {
        fs::create_dir_all(p)?;
    }
    let sprites = SpriteCapture::new(&game, hud_frame);
    fs::write(out.with_extension("ppm"), sprites.ppm(&bytes))?;
    fs::write(out.with_extension("bitmap.ppm"), ppm(&bytes))?;
    fs::write(out.with_extension("sprites.json"), sprites.json())?;
    fs::write(out.with_extension("bin"), bytes)?;
    println!(
        "{}: {triangles} triangles, {dropped} dropped; {:?}",
        out.display(),
        game.hud()
    );
    if dropped > 0 {
        return Err("scene exceeds triangle budget".into());
    }
    Ok(())
}

fn victory_ticks(elapsed_ms: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let elapsed_ms = elapsed_ms.parse::<u32>()?;
    u16::try_from(elapsed_ms / 20)
        .map_err(|_| "victory elapsed milliseconds exceed the 65535-tick preview range".into())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("bake") => host::bake(Path::new(args.get(2).map(String::as_str).unwrap_or("dist"))),
        Some("check") => {
            host::check_package(&fs::read(args.get(2).ok_or("check needs a .reu file")?)?)?;
            println!("Asset data validated; executable overlays are checked at startup");
            Ok(())
        }
        Some("snapshot") => {
            let transition = args.get(2).map(String::as_str) == Some("transition");
            let source = if transition {
                let section = args
                    .get(3)
                    .ok_or("transition needs outgoing section 0..16")?
                    .parse::<u8>()?;
                let elapsed_ms = args
                    .get(4)
                    .ok_or("transition needs elapsed milliseconds 0..8000")?
                    .parse::<u16>()?;
                if section >= SEGMENT_COUNT - 1 || elapsed_ms > TRANSITION_TICKS * 20 {
                    return Err(
                        "transition section must be 0..16 and elapsed milliseconds 0..8000".into(),
                    );
                }
                Snapshot::Transition(section, elapsed_ms / 20)
            } else if args.get(2).map(String::as_str) == Some("victory") {
                Snapshot::Victory(victory_ticks(
                    args.get(3).ok_or("victory needs elapsed milliseconds")?,
                )?)
            } else if args.get(2).map(String::as_str) == Some("attract") {
                Snapshot::Attract(
                    args.get(3)
                        .ok_or("attract needs elapsed milliseconds")?
                        .parse()?,
                )
            } else {
                let section = args
                    .get(2)
                    .filter(|s| s.as_str() != "title")
                    .map(|s| s.parse::<u8>())
                    .transpose()?;
                if section.is_some_and(|s| s >= SEGMENT_COUNT) {
                    return Err(
                        "section must be 0..17, title, attract, transition, or victory".into(),
                    );
                }
                let distance = args
                    .get(3)
                    .map(|s| s.parse::<u16>())
                    .transpose()?
                    .unwrap_or(5000);
                section.map_or(Snapshot::Title, |s| Snapshot::Section(s, distance))
            };
            snapshot(
                source,
                Path::new(
                    args.get(if transition { 5 } else { 4 })
                        .map(String::as_str)
                        .unwrap_or("target/preview"),
                ),
            )
        }
        Some("sizes") => {
            println!(
                "Game={} Audio={} NextPage={} SID={} Renderer={} bytes",
                std::mem::size_of::<Game>(),
                std::mem::size_of::<havoc::audio::AudioEngine>(),
                havoc::music::PAGE_SIZE,
                havoc::music::SID_COUNT,
                std::mem::size_of::<Renderer>()
            );
            Ok(())
        }
        Some("kernel-fixtures") => kernel_fixtures(Path::new(
            args.get(2)
                .map(String::as_str)
                .unwrap_or("target/kernel-fixtures"),
        )),
        _ => {
            println!("H.A.V.O.C. tools\n  bake [directory]\n  check game.reu\n  snapshot <title|0..17> [distance] [output-prefix]\n  snapshot attract <elapsed-ms> [output-prefix]\n  snapshot transition <0..16> <elapsed-ms:0..8000> [output-prefix]\n  snapshot victory <elapsed-ms> [output-prefix]\n  sizes\n  kernel-fixtures [directory] (requires --features music-kernels)");
            Ok(())
        }
    }
}

/// Record per-song kernel fixtures for `tools/test-music-kernel.py`: engine
/// images before and after one extra kernel call at legitimately reached
/// transport positions, plus the SID registers that call writes. The
/// simulator replays the same call on the linked 6502 image and must match.
#[cfg(feature = "music-kernels")]
fn kernel_fixtures(out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use havoc::audio::{kernels::has_kernel, AudioEngine, MusicState, SidBus, VideoStandard};
    use havoc::music::{self, BANK_HEADER_SIZE, PAGE_SIZE, SID_COUNT};
    struct Recorder([u8; 128]);
    impl Default for Recorder {
        fn default() -> Self {
            Self([0; 128])
        }
    }
    impl SidBus for Recorder {
        fn write(&mut self, sid: u8, reg: u8, value: u8) {
            self.0[sid as usize * 32 + reg as usize] = value;
        }
    }
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let bank = havoc::soundtrack::score_bank();
    let mut cases = Vec::new();
    for cue in 0..music::CUE_COUNT as u8 {
        let state = MusicState::from_cue(cue).ok_or("cue")?;
        if !has_kernel(state) {
            continue;
        }
        let info = music::cue_info(&bank[..BANK_HEADER_SIZE], cue).map_err(|e| format!("{e:?}"))?;
        let mut engine = AudioEngine::new(VideoStandard::Pal);
        let mut bus = Recorder::default();
        engine.reset(&mut bus);
        engine.set_music(state);
        engine.accept_kernel(state, true);
        let mut next_page = [0u8; PAGE_SIZE];
        let page_ticks = 16 * info.row_ticks as usize;
        let ticks = (info.pages as usize * page_ticks).min(1500);
        for tick in 0..ticks {
            if tick % 10 == 0 {
                if let Some(request) = engine.page_request() {
                    let at = info.offset as usize + request.page as usize * PAGE_SIZE;
                    next_page.copy_from_slice(&bank[at..at + PAGE_SIZE]);
                    engine
                        .publish_page(request, &next_page)
                        .map_err(|e| format!("{e:?}"))?;
                }
            }
            engine.tick(&mut bus, &next_page);
            // Sample across row phases and bar boundaries, including row zero.
            if tick % 53 == 7 || tick % page_ticks == 1 || tick % page_ticks == page_ticks - 1 {
                let before = engine.image().to_vec();
                let snapshot = engine.snapshot();
                let mut probe = Recorder::default();
                if engine.run_kernel_once(&mut probe) {
                    let after = engine.image().to_vec();
                    cases.push(format!(
                        "{{\"cue\":{cue},\"tick\":{tick},\"before\":\"{}\",\"after\":\"{}\",\"sid\":\"{}\"}}",
                        hex(&before),
                        hex(&after),
                        hex(&probe.0[..SID_COUNT * 32])
                    ));
                    engine.restore(&snapshot);
                }
            }
        }
    }
    fs::create_dir_all(out)?;
    let path = out.join(format!("kernel-fixtures-{}sid.json", SID_COUNT));
    fs::write(
        &path,
        format!(
            "{{\"sid_count\":{},\"engine_size\":{},\"cases\":[\n{}\n]}}\n",
            SID_COUNT,
            std::mem::size_of::<AudioEngine>(),
            cases.join(",\n")
        ),
    )?;
    println!(
        "Wrote {} kernel fixtures to {}",
        cases.len(),
        path.display()
    );
    Ok(())
}

#[cfg(not(feature = "music-kernels"))]
fn kernel_fixtures(_out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Err("kernel-fixtures needs `cargo run --features music-kernels`".into())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn victory_preview_time_quantizes_and_accepts_settled_animation() {
        assert_eq!(victory_ticks("0").unwrap(), 0);
        assert_eq!(victory_ticks("7999").unwrap(), 399);
        assert_eq!(victory_ticks("8000").unwrap(), 400);
        assert_eq!(victory_ticks("12019").unwrap(), 600);
        assert_eq!(victory_ticks("1310719").unwrap(), u16::MAX);
        assert!(victory_ticks("1310720").is_err());
        assert!(victory_ticks("-1").is_err());
        assert!(victory_ticks("not-a-time").is_err());
    }
    #[test]
    fn original_assets_fit_and_validate() {
        let b = host::common_bank();
        assert!(AssetBank::new(&b).is_ok());
        for i in 0..4 {
            assert!(Segment::new(&host::segment_bank(i)).is_ok());
        }
    }
    #[test]
    fn dma_boundaries() {
        assert_eq!(dma_chunk(0x7fff0, 0x8400, 1024), 16);
        assert_eq!(dma_chunk(0x80000, 0xfff0, 1024), 16);
        assert_eq!(dma_chunk(0x100000, 0x8400, 1024), 0);
        assert_eq!(dma_chunk(0x10, 0x8400, 0), 0);
    }
}
