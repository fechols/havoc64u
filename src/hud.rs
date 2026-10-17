//! Screen-space cockpit and mission screens. All ink participates in the same
//! VIC-II palette packing as geometry; no sprites, textures, or allocations.

use crate::assets::SEGMENT_DISTANCE;
use crate::game::{Hud, Mode};
use crate::render::{draw_bar, draw_text, draw_text_scaled, fill_color, HUD_INK};

/// Paint a HUD into one rendered eight-row band. `frame` controls soft blinking.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw(band: u8, pixels: &mut [u8; 1280], hud: &Hud, frame: u16) {
    if band >= 25 {
        return;
    }
    // Cockpit strips occupy only the first band and the last two bands. None
    // of the mission panels overlaps them, so route them before mode work.
    if hud.mode != Mode::Title && (band == 0 || band >= 23) {
        cockpit(band, pixels, hud, frame);
        return;
    }
    match hud.mode {
        Mode::Title => title(band, pixels, frame),
        Mode::Playing => {
            if (1..=2).contains(&band) && hud.boss_shield != 0 {
                panel(band, pixels, 35, 10, 90, 11, 12);
                draw_text(band, pixels, 39, 13, b"CARRIER", 15);
                draw_bar(
                    band,
                    pixels,
                    70,
                    14,
                    50,
                    3,
                    hud.boss_shield as u16,
                    100,
                    if hud.boss_shield <= 25 { 10 } else { 8 },
                );
            }
            if (3..=6).contains(&band)
                && hud
                    .distance
                    .saturating_sub(hud.section as u32 * SEGMENT_DISTANCE)
                    < 800
            {
                panel(band, pixels, 24, 29, 112, 22, 12);
                let mut sector = *b"SECTOR 1";
                sector[7] = b'1' + hud.section.min(3);
                centered(band, pixels, 33, &sector, 3);
                centered(band, pixels, 43, section_name(hud.section), 1);
            }
        }
        Mode::Respawning => {
            if !(9..=14).contains(&band) {
                return;
            }
            panel(band, pixels, 26, 78, 108, 37, 12);
            centered(band, pixels, 84, b"SHIP LOST", 10);
            centered(band, pixels, 96, b"RETURNING TO CHECKPOINT", 15);
            let mut lives = *b"SHIPS REMAINING: 0";
            lives[17] = b'0' + hud.lives.min(9);
            centered(band, pixels, 106, &lives, 3);
        }
        Mode::Victory | Mode::GameOver => {
            if !(8..=16).contains(&band) {
                return;
            }
            panel(band, pixels, 8, 69, 144, 65, 12);
            if hud.mode == Mode::Victory {
                centered_scaled(band, pixels, 77, b"MISSION COMPLETE", 7, 2);
                centered(band, pixels, 96, b"KESTREL RETURNS", 3);
            } else {
                centered_scaled(band, pixels, 77, b"MISSION LOST", 10, 2);
                centered(band, pixels, 96, b"NO SHIPS REMAIN", 15);
            }
            if band == 13 || band == 14 {
                let mut score = *b"SCORE 000000";
                digits(hud.score, &mut score[6..]);
                centered(band, pixels, 109, &score, 1);
            }
            centered(
                band,
                pixels,
                124,
                if hud.mode == Mode::Victory {
                    b"FIRE: FLY AGAIN"
                } else {
                    b"FIRE: RETRY MISSION"
                },
                blink(frame),
            );
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn title(band: u8, pixels: &mut [u8; 1280], frame: u16) {
    // Keep the center open so the original Kestrel model remains visible.
    if band == 0 || (9..16).contains(&band) {
        return;
    }
    if band <= 8 {
        rect(band, pixels, 0, 10, 160, 58, 0);
        centered(band, pixels, 17, b"C64 ULTIMATE", 12);
        centered_scaled(band, pixels, 31, b"SUEME64", 1, 3);
        centered(band, pixels, 55, b"KESTREL STRIKE", 3);
        rect(band, pixels, 34, 65, 92, 1, HUD_INK | 14);
        return;
    }
    rect(band, pixels, 10, 131, 140, 67, 0);
    centered(band, pixels, 134, b"PORT 2 / ONE BUTTON", 12);
    centered(band, pixels, 146, b"HOLD FIRE: SHOOT", 15);
    centered(band, pixels, 155, b"RELEASE: CHARGE", 15);
    centered(band, pixels, 164, b"DOUBLE FIRE: BOMB", 15);
    centered(band, pixels, 173, b"DOUBLE LEFT/RIGHT: ROLL", 15);
    centered(band, pixels, 189, b"PUSH FIRE TO LAUNCH", blink(frame));
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn cockpit(band: u8, pixels: &mut [u8; 1280], hud: &Hud, frame: u16) {
    if band == 0 {
        rect(band, pixels, 0, 0, 160, 8, 0);
        draw_text(band, pixels, 3, 2, b"SCORE", 12);
        let mut score = [b'0'; 6];
        digits(hud.score, &mut score);
        draw_text(band, pixels, 27, 2, &score, 1);
        draw_text(band, pixels, 115, 2, b"LIVES", 12);
        draw_text(band, pixels, 143, 2, &[b'0' + hud.lives.min(9)], 1);
    }
    if band < 23 {
        return;
    }
    rect(band, pixels, 0, 185, 160, 15, 0);
    draw_text(band, pixels, 3, 187, b"SHIELD", 12);
    draw_text(
        band,
        pixels,
        68,
        187,
        if hud.charge == 255 {
            b"HOMING MISSILE"
        } else {
            b"LASER CHARGE"
        },
        if hud.charge == 255 { blink(frame) } else { 12 },
    );
    draw_text(band, pixels, 127, 187, b"BOMBS", 12);
    draw_bar(
        band,
        pixels,
        3,
        194,
        60,
        3,
        hud.shield as u16,
        100,
        if hud.shield <= 25 { 10 } else { 13 },
    );
    draw_bar(
        band,
        pixels,
        68,
        194,
        55,
        3,
        hud.charge as u16,
        255,
        if hud.charge == 255 { 7 } else { 3 },
    );
    for i in 0..3 {
        rect(
            band,
            pixels,
            129 + i * 7,
            194,
            4,
            3,
            if i < hud.bombs as i16 {
                HUD_INK | 7
            } else {
                11
            },
        );
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn section_name(section: u8) -> &'static [u8] {
    match section {
        0 => b"LOWLAND APPROACH",
        1 => b"IRON CANYON",
        2 => b"FORTRESS RUN",
        _ => b"CARRIER ASSAULT",
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn blink(frame: u16) -> u8 {
    if frame & 16 == 0 {
        1
    } else {
        3
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn centered(band: u8, pixels: &mut [u8; 1280], y: i16, text: &[u8], color: u8) {
    draw_text(
        band,
        pixels,
        (161 - text.len() as i16 * 4) / 2,
        y,
        text,
        color,
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn centered_scaled(band: u8, pixels: &mut [u8; 1280], y: i16, text: &[u8], color: u8, scale: u8) {
    let width = (text.len() as i16 * 4 - 1) * scale as i16;
    draw_text_scaled(band, pixels, (160 - width) / 2, y, text, color, scale);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn panel(band: u8, pixels: &mut [u8; 1280], x: i16, y: i16, width: i16, height: i16, border: u8) {
    rect(band, pixels, x, y, width, height, 0);
    rect(band, pixels, x, y, width, 1, border | HUD_INK);
    rect(band, pixels, x, y + height - 1, width, 1, border | HUD_INK);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn rect(band: u8, pixels: &mut [u8; 1280], x: i16, y: i16, width: i16, height: i16, color: u8) {
    let band_y = band as i16 * 8;
    let left = x.clamp(0, 160) as usize;
    let right = (x + width).clamp(0, 160) as usize;
    if left >= right {
        return;
    }
    for row in y.max(band_y)..(y + height).min(band_y + 8) {
        let offset = (row - band_y) as usize * 160;
        fill_color(&mut pixels[offset + left..offset + right], color);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn digits(mut value: u32, output: &mut [u8]) {
    // Saturation preserves the display width instead of wrapping high scores.
    value = value.min(999_999);
    const POWERS: [u32; 6] = [100_000, 10_000, 1000, 100, 10, 1];
    for i in 0..output.len().min(6) {
        let power = POWERS[i];
        let mut digit = b'0';
        while value >= power {
            value -= power;
            digit += 1;
        }
        output[i] = digit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_score_is_fixed_width_and_saturates() {
        let mut output = [0; 6];
        digits(42, &mut output);
        assert_eq!(&output, b"000042");
        digits(u32::MAX, &mut output);
        assert_eq!(&output, b"999999");
    }
}
