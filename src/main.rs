mod host;
use havoc::{
    assets::*,
    game::{Game, InputSample, FIRE, TRANSITION_TICKS},
    hud,
    render::{self, Renderer},
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
    let mut output = vec![0; 10_000];
    for band in 0..25u8 {
        let mut pixels = [0; 1280];
        let mut bitmap = [0; 320];
        let mut screen = [0; 40];
        let mut color = [0; 40];
        r.render_band(band, &mut pixels);
        hud::draw(band, &mut pixels, &game.hud(), number);
        render::pack_band(
            &pixels,
            &[0; 40],
            &[0; 40],
            &mut bitmap,
            &mut screen,
            &mut color,
        );
        let n = band as usize;
        output[n * 320..n * 320 + 320].copy_from_slice(&bitmap);
        output[8000 + n * 40..8000 + n * 40 + 40].copy_from_slice(&screen);
        output[9000 + n * 40..9000 + n * 40 + 40].copy_from_slice(&color);
    }
    (output, r.triangle_count, r.dropped)
}
fn ppm(frame: &[u8]) -> Vec<u8> {
    let mut out = b"P6\n640 480\n255\n".to_vec();
    for py in 0..480 {
        let y = py * 200 / 480;
        for px in 0..640 {
            let x = px / 4;
            let cell = (y / 8) * 40 + x / 4;
            let byte = frame[cell * 8 + y % 8];
            let slot = (byte >> (6 - (x % 4) * 2)) & 3;
            let c = match slot {
                0 => 0,
                1 => frame[8000 + cell] >> 4,
                2 => frame[8000 + cell] & 15,
                _ => frame[9000 + cell] & 15,
            };
            out.extend_from_slice(&render::PALETTE_RGB[c as usize]);
        }
    }
    out
}
enum Snapshot {
    Title,
    Section(u8, u16),
    Attract(u32),
    Transition(u8, u16),
}

fn snapshot(source: Snapshot, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let models: [_; 4] = core::array::from_fn(|i| host::model_bank(i as u8));
    let game = match source {
        Snapshot::Title => Game::new(),
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
            let banks: [_; 4] = core::array::from_fn(|i| host::segment_bank(i as u8));
            let mut game = Game::new();
            // Replay actual 20-ms steps, including title idle and repeat cycles.
            // Fractional steps have not occurred yet at the requested timestamp.
            for tick in 1..=elapsed_ms / 20 {
                game.sample_input(InputSample {
                    bits: 0,
                    pressed: 0,
                    ms: tick * 20,
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
    let (bytes, triangles, dropped) = frame(&game, &bank, &segment, 0);
    if let Some(p) = out.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(out.with_extension("ppm"), ppm(&bytes))?;
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
                    .ok_or("transition needs outgoing section 0..2")?
                    .parse::<u8>()?;
                let elapsed_ms = args
                    .get(4)
                    .ok_or("transition needs elapsed milliseconds 0..8000")?
                    .parse::<u16>()?;
                if section >= 3 || elapsed_ms > TRANSITION_TICKS * 20 {
                    return Err(
                        "transition section must be 0..2 and elapsed milliseconds 0..8000".into(),
                    );
                }
                Snapshot::Transition(section, elapsed_ms / 20)
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
                if section.is_some_and(|s| s >= 4) {
                    return Err("section must be 0..3, title, or attract".into());
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
        _ => {
            println!("H.A.V.O.C. tools\n  bake [directory]\n  check game.reu\n  snapshot <title|0|1|2|3> [distance] [output-prefix]\n  snapshot attract <elapsed-ms> [output-prefix]\n  snapshot transition <0|1|2> <elapsed-ms:0..8000> [output-prefix]\n  sizes");
            Ok(())
        }
    }
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
        assert_eq!(dma_chunk(0x1000000, 0x8400, 1024), 0);
        assert_eq!(dma_chunk(0x10, 0x8400, 0), 0);
    }
}
