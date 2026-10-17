#[allow(dead_code)] #[path="../src/host.rs"] mod host;
use std::{fs, path::Path};
use sueme64::{assets::{AssetBank, Segment}, game::Game, hud, render::{self, Renderer}};

fn random(state: &mut u16) -> u8 { *state = state.wrapping_mul(25173).wrapping_add(13849); (*state >> 8) as u8 }
fn save(dir: &Path, id: usize, pixels: &[u8;1280], old_screen: &[u8;40], old_color: &[u8;40]) -> ([u8;40],[u8;40]) {
    let mut bitmap=[0;320];let mut screen=[0;40];let mut color=[0;40];
    render::pack_band(pixels,old_screen,old_color,&mut bitmap,&mut screen,&mut color);
    let mut out=Vec::new();out.extend_from_slice(pixels);out.extend_from_slice(old_screen);out.extend_from_slice(old_color);
    out.extend_from_slice(&bitmap);out.extend_from_slice(&screen);out.extend_from_slice(&color);
    fs::write(dir.join(format!("case-{id:03}.bin")),out).unwrap();(screen,color)
}
fn main(){
    let dir=std::env::args().nth(1).unwrap();let dir=Path::new(&dir);fs::create_dir_all(dir).unwrap();
    let mut id=0;
    for case in 0..48 {
        let mut rng=0x1951u16 ^ (case as u16).wrapping_mul(97);
        let mut pixels=[0;1280];let mut old_screen=[0;40];let mut old_color=[0;40];
        for c in 0..40 {old_screen[c]=random(&mut rng);old_color[c]=random(&mut rng)&15;}
        for pixel in &mut pixels {
            let sample=random(&mut rng);
            *pixel=if case<16 {case as u8} else if case<24 {[0,1+(case%15)as u8,1+((case*3)%15)as u8,1+((case*7)%15)as u8][(sample&3)as usize]} else {sample&15};
            if sample&0x30==0 {*pixel|=0x80;}
        }
        if case%5==0 {for c in 0..40 {let v=old_color[c];old_screen[c]=(v<<4)|v;}}
        if case%7==0 {old_screen.fill(0);old_color.fill(0);}
        save(dir,id,&pixels,&old_screen,&old_color);id+=1;
    }
    let common=host::common_bank();let bank=AssetBank::new(&common).unwrap();
    for scene in 0..5 {
        let game=if scene==0 {Game::new()}else{Game::preview_at(scene-1,5000)};
        let segment_data=host::segment_bank(game.segment_index());let segment=Segment::new(&segment_data).unwrap();
        let mut renderer=Renderer::new();renderer.begin(6);game.render(&mut renderer,&bank,&segment);renderer.finish();
        for band in [0,4,8,12,16,20,23,24] {
            let mut pixels=[0;1280];renderer.render_band(band,&mut pixels);hud::draw(band,&mut pixels,&game.hud(),0);
            let (screen,color)=save(dir,id,&pixels,&[0;40],&[0;40]);id+=1;
            save(dir,id,&pixels,&screen,&color);id+=1;
        }
    }
    let mut distances=Vec::new();for a in render::PALETTE_RGB {for b in render::PALETTE_RGB {
        let r=a[0]as i32-b[0]as i32;let g=a[1]as i32-b[1]as i32;let blue=a[2]as i32-b[2]as i32;
        distances.extend_from_slice(&(((r*r+2*g*g+blue*blue)/4)as u16).to_le_bytes());
    }}
    fs::write(dir.join("distances.bin"),distances).unwrap();println!("{id} exact Rust pack fixtures");
}
