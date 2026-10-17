mod host;
use havoc::{
    assets::*,
    game::{Game, InputSample, FIRE},
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
    r.begin(6);
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
fn snapshot(
    section: Option<u8>,
    distance: u16,
    out: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let common = host::common_bank();
    let bank = AssetBank::new(&common).unwrap();
    let mut game = if let Some(s) = section {
        Game::preview_at(s, distance)
    } else {
        Game::new()
    };
    let data = host::segment_bank(game.segment_index());
    let segment = Segment::new(&data).unwrap();
    if section.is_some() {
        game.sample_input(InputSample {
            bits: FIRE,
            pressed: 0,
            ms: 6000,
        });
        game.tick(&segment);
    }
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
            let section = args
                .get(2)
                .filter(|s| s.as_str() != "title")
                .map(|s| s.parse::<u8>())
                .transpose()?;
            if section.is_some_and(|s| s >= 4) {
                return Err("section must be0..3 or title".into());
            }
            let distance = args
                .get(3)
                .map(|s| s.parse::<u16>())
                .transpose()?
                .unwrap_or(5000);
            snapshot(
                section,
                distance,
                Path::new(args.get(4).map(String::as_str).unwrap_or("target/preview")),
            )
        }
        Some("sizes") => {
            println!(
                "Game={} Audio={} Renderer={} bytes",
                std::mem::size_of::<Game>(),
                std::mem::size_of::<havoc::audio::AudioEngine>(),
                std::mem::size_of::<Renderer>()
            );
            Ok(())
        }
        _ => {
            println!("H.A.V.O.C. tools\n  bake [directory]\n  check game.reu\n  snapshot <title|0|1|2|3> [distance] [output-prefix]\n  sizes");
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
