//! Transient mission screens and announcements painted into the bitmap.
//! Persistent cockpit readouts are independent sprites in `hud_sprites`.

use crate::assets::SEGMENT_COUNT;
use crate::game::{Hud, Mode};
use crate::render::{draw_text, draw_text_scaled, Canvas, HUD_INK};
use crate::victory::REVEAL_TICKS;

/// The sector panel occupies bands 3-6 for the first 800 distance units.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub(crate) fn sector_banner_visible(hud: &Hud) -> bool {
    hud.distance < 800
}

/// The pause caption: double-height text across the middle of the picture.
/// Returns whether the band received ink.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw_paused(band: u8, canvas: &mut impl Canvas) -> bool {
    // Rows 92..108 span bands 11 to 13.
    if !(11..=13).contains(&band) {
        return false;
    }
    centered_scaled(band, canvas, 92, b"PAUSED", 1, 2);
    true
}

/// Paint a HUD into one rendered eight-row band. `frame` controls soft blinking.
/// Returns whether any ink reached the band: the packer's row-uniform fast
/// path is only valid for cells no HUD primitive touched.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn draw(band: u8, canvas: &mut impl Canvas, hud: &Hud, frame: u16) -> bool {
    if band >= 25 {
        return false;
    }
    if hud.mode == Mode::Victory {
        return victory(band, canvas, hud, frame);
    }
    // The map tour keeps the entire world visible around two short labels.
    if hud.mode == Mode::Attract {
        if band == 0 {
            centered(band, canvas, 2, section_name(hud.section), 1);
            return true;
        }
        if band >= 23 {
            centered(band, canvas, 189, b"MOVE OR FIRE FOR MENU", blink(frame));
            return true;
        }
        return false;
    }
    match hud.mode {
        Mode::Attract | Mode::Transition | Mode::Victory => false,
        Mode::Title => title(band, canvas, frame, hud.story_mode),
        Mode::Playing => {
            let banner = (3..=6).contains(&band) && sector_banner_visible(hud);
            if banner {
                panel(band, canvas, 24, 29, 112, 22, 12);
                let mut sector = *b"SECTOR 01";
                let number = hud.section.min(SEGMENT_COUNT - 1) + 1;
                if number >= 10 {
                    sector[7] = b'1';
                    sector[8] = b'0' + number - 10;
                } else {
                    sector[8] = b'0' + number;
                }
                centered(band, canvas, 33, &sector, 3);
                centered(band, canvas, 43, section_name(hud.section), 1);
            }
            banner
        }
        Mode::Respawning => {
            if !(9..=14).contains(&band) {
                return false;
            }
            panel(band, canvas, 26, 78, 108, 37, 12);
            centered(band, canvas, 84, b"SHIP LOST", 10);
            centered(band, canvas, 96, b"RETURNING TO CHECKPOINT", 15);
            let mut lives = *b"SHIPS REMAINING: 00";
            let count = life_digits(hud.lives);
            let length = if count[0] == b' ' {
                lives[17] = count[1];
                18
            } else {
                lives[17] = count[0];
                lives[18] = count[1];
                19
            };
            centered(band, canvas, 106, &lives[..length], 3);
            true
        }
        Mode::GameOver => {
            if !(8..=16).contains(&band) {
                return false;
            }
            panel(band, canvas, 8, 69, 144, 65, 12);
            centered_scaled(band, canvas, 77, b"MISSION LOST", 10, 2);
            centered(band, canvas, 96, b"NO SHIPS REMAIN", 15);
            if band == 13 || band == 14 {
                let mut score = *b"SCORE 000000";
                digits(hud.score, &mut score[6..]);
                centered(band, canvas, 109, &score, 1);
            }
            centered(band, canvas, 124, b"FIRE: RETRY MISSION", blink(frame));
            true
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn victory(band: u8, canvas: &mut impl Canvas, hud: &Hud, frame: u16) -> bool {
    if hud.victory_ticks < REVEAL_TICKS {
        return false;
    }
    // Narrow opaque strips keep the raised star and most of the medal clear.
    if (1..=3).contains(&band) {
        rect(band, canvas, 8, 9, 144, 18, 0);
        centered_scaled(band, canvas, 13, b"MISSION COMPLETE", 7, 2);
    }
    if band >= 22 {
        rect(band, canvas, 36, 177, 88, 22, 0);
        let mut score = *b"SCORE 000000";
        digits(hud.score, &mut score[6..]);
        centered(band, canvas, 180, &score, 1);
        centered(
            band,
            canvas,
            190,
            b"FIRE: TITLE SCREEN",
            if frame & 16 == 0 { 1 } else { 7 },
        );
    }
    (1..=3).contains(&band) || band >= 22
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn title(band: u8, canvas: &mut impl Canvas, frame: u16, story_mode: bool) -> bool {
    // Keep the center open so the original Kestrel model remains visible.
    if band == 0 || (9..16).contains(&band) {
        return false;
    }
    if band <= 8 {
        rect(band, canvas, 0, 10, 160, 58, 0);
        centered(band, canvas, 17, b"C64 ULTIMATE", 12);
        centered_scaled(band, canvas, 31, b"H.A.V.O.C.", 1, 3);
        centered(band, canvas, 51, b"HEAVY ASSAULT VESSEL:", 3);
        centered(band, canvas, 59, b"ORBITAL COMBAT", 3);
        rect(band, canvas, 34, 65, 92, 1, HUD_INK | 14);
        return true;
    }
    rect(band, canvas, 10, 131, 140, 67, 0);
    centered(band, canvas, 134, b"PORT 2 / ONE BUTTON", 12);
    centered(band, canvas, 146, b"HOLD FIRE: SHOOT", 15);
    centered(band, canvas, 155, b"RELEASE: CHARGE", 15);
    centered(band, canvas, 164, b"DOUBLE FIRE: BOMB", 15);
    centered(band, canvas, 173, b"DOUBLE LEFT/RIGHT: ROLL", 15);
    centered(band, canvas, 181, b"DIGITAL MUSIC BY WILLIAM ECHOLS", 12);
    centered(
        band,
        canvas,
        189,
        if story_mode {
            b"S: STORY 99 / FIRE: START"
        } else {
            b"S: NORMAL 3 / FIRE: START"
        },
        blink(frame),
    );
    true
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels_aux")]
fn section_name(section: u8) -> &'static [u8] {
    let index = section.min(SEGMENT_COUNT - 1) as usize;
    // The bounded index selects two monotonically increasing authored offsets.
    // The final offset is exactly the packed label array's length.
    unsafe {
        let start = *SECTOR_OFFSETS.get_unchecked(index) as usize;
        let end = *SECTOR_OFFSETS.get_unchecked(index + 1) as usize;
        core::slice::from_raw_parts(SECTOR_NAMES.as_ptr().add(start), end - start)
    }
}

// Byte offsets avoid eighteen native four-byte slice descriptors. PIXELS owns
// both tables; keep the strings in its prefix so the helper tail stays bounded.
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels.data")]
static SECTOR_NAMES: [u8; 228] = *b"LOWLAND APPROACHCRYSTAL CAVEIRON CANYONCANYON GATE\
ASHEN WASTESFOUNDRY BELTFORTRESS RUNCITADEL CORESTORM FRONTMETEOR REACH\
CARRIER COASTCARRIER ASSAULTASTEROID BELTSHATTERED MOONSHIP GRAVEYARD\
ORBITAL SHIPYARDSPACE CARRIERJUMP GATE";
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels_aux.data")]
static SECTOR_OFFSETS: [u8; SEGMENT_COUNT as usize + 1] = [
    0, 16, 28, 39, 50, 62, 74, 86, 98, 109, 121, 134, 149, 162, 176, 190, 206, 219, 228,
];
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
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels_aux")]
fn centered(band: u8, canvas: &mut impl Canvas, y: i16, text: &[u8], color: u8) {
    draw_text(
        band,
        canvas,
        (161 - text.len() as i16 * 4) / 2,
        y,
        text,
        color,
    );
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn centered_scaled(band: u8, canvas: &mut impl Canvas, y: i16, text: &[u8], color: u8, scale: u8) {
    let width = (text.len() as i16 * 4 - 1) * scale as i16;
    draw_text_scaled(band, canvas, (160 - width) / 2, y, text, color, scale);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn panel(band: u8, canvas: &mut impl Canvas, x: i16, y: i16, width: i16, height: i16, border: u8) {
    rect(band, canvas, x, y, width, height, 0);
    rect(band, canvas, x, y, width, 1, border | HUD_INK);
    rect(band, canvas, x, y + height - 1, width, 1, border | HUD_INK);
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn rect(band: u8, canvas: &mut impl Canvas, x: i16, y: i16, width: i16, height: i16, color: u8) {
    let band_y = band as i16 * 8;
    let left = x.clamp(0, 160) as usize;
    let right = (x + width).clamp(0, 160) as usize;
    if left >= right {
        return;
    }
    for row in y.max(band_y)..(y + height).min(band_y + 8) {
        canvas.span((row - band_y) as usize, left, right, color);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub(crate) fn digits(mut value: u32, output: &mut [u8]) {
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

/// Two decimal places for the life counter, with a blank leading zero.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub(crate) fn life_digits(value: u8) -> [u8; 2] {
    let mut units = value.min(99);
    let mut tens = b'0';
    // A fixed ladder avoids LLVM replacing a subtraction loop with a native
    // division helper in scarce resident memory.
    if units >= 80 {
        units -= 80;
        tens += 8;
    }
    if units >= 40 {
        units -= 40;
        tens += 4;
    }
    if units >= 20 {
        units -= 20;
        tens += 2;
    }
    if units >= 10 {
        units -= 10;
        tens += 1;
    }
    [if tens == b'0' { b' ' } else { tens }, b'0' + units]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_mode_cues_use_visible_glyphs() {
        for cue in [b"S: NORMAL 3 / FIRE: START", b"S: STORY 99 / FIRE: START"] {
            for &character in cue {
                if character != b' ' {
                    assert!(
                        crate::render::glyph(character).iter().any(|&row| row != 0),
                        "missing title glyph {}",
                        character as char
                    );
                }
            }
        }
    }

    #[test]
    fn decimal_score_is_fixed_width_and_saturates() {
        let mut output = [0; 6];
        digits(42, &mut output);
        assert_eq!(&output, b"000042");
        digits(u32::MAX, &mut output);
        assert_eq!(&output, b"999999");
    }
}
