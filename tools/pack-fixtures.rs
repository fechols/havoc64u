#[allow(dead_code)] #[path="../src/host.rs"] mod host;
use std::{fs, path::Path};
use havoc::{assets::{AssetBank, Segment}, game::Game, hud, render::{self, Renderer}};

fn random(state: &mut u16) -> u8 { *state = state.wrapping_mul(25173).wrapping_add(13849); (*state >> 8) as u8 }
// Each fixture: 1,280 pixels, 40 old screen bytes, 40 old colour nibbles, the
// expected 320 bitmap + 40 screen + 40 colour bytes, then the background colour.
fn save(dir: &Path, id: usize, pixels: &[u8;1280], old_screen: &[u8;40], old_color: &[u8;40], background: u8) -> ([u8;40],[u8;40]) {
    let mut bitmap=[0;320];let mut screen=[0;40];let mut color=[0;40];
    render::pack_band(pixels,old_screen,old_color,background,&mut bitmap,&mut screen,&mut color);
    let mut out=Vec::new();out.extend_from_slice(pixels);out.extend_from_slice(old_screen);out.extend_from_slice(old_color);
    out.extend_from_slice(&bitmap);out.extend_from_slice(&screen);out.extend_from_slice(&color);out.push(background);
    fs::write(dir.join(format!("case-{id:03}.bin")),out).unwrap();(screen,color)
}
// Backgrounds for the synthetic cases: black (the classic rules), colours that
// occur in the cells (they become code 0 and free a slot) and ones that do not.
const BACKGROUNDS: [u8; 8] = [0, 0, 6, 11, 4, 8, 15, 1];
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
            if sample&0x40!=0 {*pixel|=0x40;} // Coverage metadata never affects quantization.
        }
        if case%5==0 {for c in 0..40 {let v=old_color[c];old_screen[c]=(v<<4)|v;}}
        if case%7==0 {old_screen.fill(0);old_color.fill(0);}
        // Uniform cells: the background equal to the fill (code 0) every third case.
        let background=if case<16 {if case%3==0 {case as u8} else {BACKGROUNDS[case%8]}} else {BACKGROUNDS[case%8]};
        save(dir,id,&pixels,&old_screen,&old_color,background);id+=1;
    }
    // Exercise the exact-color shortcut at one through five nonblack colors,
    // including the three/four-color boundary, HUD minorities, ranking ties,
    // empty old slots, retained slots and duplicate old colors.
    for nonblack in 1..=5usize {
        for variant in 0..8usize {
            let mut pixels=[0;1280];let mut old_screen=[0;40];let mut old_color=[0;40];
            for cell in 0..40usize {
                let colors: [u8;5]=core::array::from_fn(|i|1+((cell*7+i*3)%15)as u8);
                let old=match variant {
                    0 => [0,0,0],
                    1 => [colors[0],colors[1],colors[2]],
                    2 => [colors[2],colors[0],colors[1]],
                    3 => [colors[0];3],
                    4 => [colors[1],colors[1],colors[2]],
                    5 => [colors[4],0,colors[0]],
                    6 => [0,colors[2],0],
                    _ => [colors[2],colors[1],colors[0]],
                };
                old_screen[cell]=(old[0]<<4)|old[1];old_color[cell]=old[2];
                for row in 0..8usize {for col in 0..4usize {
                    let index=row*4+col;
                    let selected=if variant&4!=0 && index<24 {0}else{index%nonblack};
                    let mut pixel=colors[selected];
                    if variant&1!=0 && index>=30 {pixel=0;}
                    if variant&2!=0 && selected==nonblack-1 {pixel|=render::HUD_INK;}
                    pixels[row*160+cell*4+col]=pixel;
                }}
            }
            // The three/four-colour boundary with and without a palette colour as background.
            save(dir,id,&pixels,&old_screen,&old_color,if variant&1!=0 {1+((7+3*(nonblack-1))%15) as u8} else {BACKGROUNDS[variant]});id+=1;
        }
    }
    // Row-uniform cells (every row one byte) exercise the packer's sampled fast
    // path: one to six colors per cell, HUD ink on some rows, coverage bits,
    // black rows, and every old-slot arrangement from the exact-color cases.
    for colors_per_cell in 1..=6usize {
        for variant in 0..8usize {
            let mut rng=0x4a17u16 ^ (colors_per_cell as u16*131+variant as u16*17);
            let mut pixels=[0;1280];let mut old_screen=[0;40];let mut old_color=[0;40];
            for cell in 0..40usize {
                let palette: [u8;6]=core::array::from_fn(|i|1+((cell*5+i*7+variant)%15)as u8);
                let old=match variant {
                    0 => [0,0,0],
                    1 => [palette[0],palette[1],palette[2]],
                    2 => [palette[2],palette[0],palette[1]],
                    3 => [palette[0];3],
                    4 => [palette[1],palette[1],palette[2]],
                    5 => [palette[5],0,palette[0]],
                    6 => [0,palette[2],0],
                    _ => [palette[2],palette[1],palette[0]],
                };
                old_screen[cell]=(old[0]<<4)|old[1];old_color[cell]=old[2];
                for row in 0..8usize {
                    let sample=random(&mut rng);
                    let mut pixel=palette[(sample as usize)%colors_per_cell];
                    if variant&1!=0 && row==7 {pixel=0;}
                    if variant&2!=0 && sample&0x60==0 {pixel|=render::HUD_INK;}
                    if sample&0x10!=0 {pixel|=0x40;}
                    for col in 0..4 {pixels[row*160+cell*4+col]=pixel;}
                }
            }
            save(dir,id,&pixels,&old_screen,&old_color,BACKGROUNDS[(colors_per_cell+variant)%8]);id+=1;
        }
    }
    for scene in 0..5 {
        let game=if scene==0 {Game::new()}else{Game::preview_at(scene-1,5000)};
        let models=host::model_bank(game.segment_index());let bank=AssetBank::new(&models).unwrap();
        let segment_data=host::segment_bank(game.segment_index());let segment=Segment::new(&segment_data).unwrap();
        let mut renderer=Renderer::new();renderer.begin(game.sky_color());game.render(&mut renderer,&bank,&segment);renderer.finish();
        for band in [0,4,8,12,16,20,23,24] {
            let mut pixels=[0;1280];renderer.render_band(band,&mut pixels);hud::draw(band,&mut pixels,&game.hud(),0);
            // The real sky as background, then the classic black background with retained slots.
            let (screen,color)=save(dir,id,&pixels,&[0;40],&[0;40],renderer.clear_color());id+=1;
            save(dir,id,&pixels,&screen,&color,renderer.clear_color());id+=1;
            save(dir,id,&pixels,&screen,&color,0);id+=1;
        }
    }
    let mut distances=Vec::new();for a in render::PALETTE_RGB {for b in render::PALETTE_RGB {
        let r=a[0]as i32-b[0]as i32;let g=a[1]as i32-b[1]as i32;let blue=a[2]as i32-b[2]as i32;
        distances.extend_from_slice(&(((r*r+2*g*g+blue*blue)/4)as u16).to_le_bytes());
    }}
    fs::write(dir.join("distances.bin"),distances).unwrap();println!("{id} exact Rust pack fixtures");
}
