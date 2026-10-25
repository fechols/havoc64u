//! Floating HUD text and gauges, encoded as eight VIC-II multicolour sprites.
//!
//! A corner uses two adjacent 12x21 multicolour images. Pixel codes are
//! transparent, shared grey, individual status colour, and shared white.
//! Only text and functional gauge pixels are opaque. The caller owns the image
//! storage; only the small displayed-value cache persists.

use crate::game::{Hud, Mode};

/// Origins in the game's 160x200 logical canvas; each corner is 24x21 pixels.
pub const CORNER_POSITIONS: [(u8, u8); 4] = [(3, 12), (133, 12), (3, 175), (133, 175)];

const COCKPIT: u8 = 1;
const SHIELD_LOW: u8 = 2;
const BOOST_FLASH: u8 = 4;
const READY: u8 = 8;
const BLINK: u8 = 16;
const NO_FPS: u16 = u16::MAX;
const GREY: u8 = 1;
const ACCENT: u8 = 2;
const WHITE: u8 = 3;

/// Only values which can change visible sprite pixels or their accent colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayState {
    score: u32,
    fps: u16,
    shield: u8,
    charge: u8,
    boost: u8,
    lives: u8,
    bombs: u8,
    flags: u8,
}

impl DisplayState {
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn from_hud(hud: &Hud, frame: u16, fps: Option<u16>) -> Self {
        let mut state = Self {
            score: 0,
            fps: fps.map_or(NO_FPS, |value| value.min(999)),
            shield: 0,
            charge: 0,
            boost: 0,
            lives: 0,
            bombs: 0,
            flags: 0,
        };
        if matches!(hud.mode, Mode::Title | Mode::Attract | Mode::Victory) {
            return state;
        }
        state.flags = COCKPIT;
        state.score = hud.score.min(999_999);
        state.lives = hud.lives.min(99);
        state.bombs = hud.bombs.min(3);
        state.shield = meter(hud.shield.min(100), 22, 100);
        state.charge = meter(hud.charge, 22, 255);
        state.boost = meter(hud.boost, 15, 255);
        if hud.boost_active && frame & 8 != 0 {
            state.flags |= BOOST_FLASH;
        }
        if hud.shield <= 25 {
            state.flags |= SHIELD_LOW;
        }
        if hud.charge == 255 {
            state.flags |= READY;
            if frame & 16 != 0 {
                state.flags |= BLINK;
            }
        }
        state
    }

    /// VIC sprite-enable bits. Menus can show the score pair as an FPS chip.
    /// Register helpers execute with the PIXELS overlay loaded.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn enabled_mask(&self) -> u8 {
        if self.flags & COCKPIT != 0 {
            0xff
        } else if self.fps != NO_FPS {
            0x03
        } else {
            0
        }
    }

    /// Both sprites in a corner use this individual VIC palette colour.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn accent(&self, corner: u8) -> u8 {
        match corner {
            0 => 3,
            // Bomb inventory uses this pair's purple accent.
            1 => 4,
            2 if self.flags & SHIELD_LOW != 0 => 10,
            2 => 13,
            3 if self.flags & READY != 0 => 7,
            3 => 3,
            _ => 0,
        }
    }

    /// Dirty corner bits (0..3), including newly visible corners. Hiding a
    /// corner needs only an enable-register change, never an image rewrite.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
    pub fn changed_corners(&self, previous: Option<&Self>) -> u8 {
        let visible = if self.flags & COCKPIT != 0 {
            15
        } else if self.fps != NO_FPS {
            1
        } else {
            0
        };
        let Some(old) = previous else {
            return visible;
        };
        let cockpit_changed = (self.flags ^ old.flags) & COCKPIT != 0;
        if cockpit_changed && self.flags & COCKPIT != 0 {
            return visible;
        }
        let mut changed = 0;
        if cockpit_changed || self.score != old.score || self.fps != old.fps {
            changed |= 1;
        }
        if self.lives != old.lives || self.bombs != old.bombs {
            changed |= 2;
        }
        if self.shield != old.shield
            || self.boost != old.boost
            || (self.flags ^ old.flags) & (SHIELD_LOW | BOOST_FLASH) != 0
        {
            changed |= 4;
        }
        if self.charge != old.charge || (self.flags ^ old.flags) & (READY | BLINK) != 0 {
            changed |= 8;
        }
        changed & visible
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels_aux")]
fn meter(value: u8, width: u8, maximum: u16) -> u8 {
    let mut remaining = value as u16 * width as u16;
    let mut length = 0;
    while remaining >= maximum {
        remaining -= maximum;
        length += 1;
    }
    length
}

// Keep labels with the swapped pixel code, not resident RAM.
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels.data")]
static LABELS: [u8; 27] = *b"SCORESHIELDCHARGEHOMINGLIFE";

/// Encode the left and right sprite into two consecutive 64-byte blocks.
/// Their final padding bytes remain zero. Invalid or hidden corners are clear.
#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
pub fn render_corner(corner: u8, state: &DisplayState, output: &mut [u8; 128]) {
    output.fill(0);
    if corner >= 4 || state.enabled_mask() & (3 << (corner * 2)) == 0 {
        return;
    }
    if state.flags & COCKPIT != 0 {
        match corner {
            0 => {
                text(output, 2, 2, &LABELS[..5], WHITE);
                let mut score = [b'0'; 6];
                crate::hud::digits(state.score, &mut score);
                text(output, 0, 9, &score, WHITE);
            }
            1 => {
                text(output, 0, 2, &LABELS[23..27], WHITE);
                text(output, 16, 2, &crate::hud::life_digits(state.lives), WHITE);
                // Three 6x5 rectangles, with transparent gaps. White outlines
                // keep purple bombs visible on matching skies; grey marks spent bombs.
                for bomb in 0..3 {
                    gauge(output, 1 + bomb * 8, 9, 4, if bomb < state.bombs { 4 } else { 0 });
                }
            }
            2 => {
                text(output, 0, 2, &LABELS[5..11], WHITE);
                gauge(output, 0, 9, 22, state.shield);
                text(
                    output,
                    1,
                    15,
                    b"B",
                    if state.flags & BOOST_FLASH != 0 { ACCENT } else { WHITE },
                );
                gauge(output, 6, 15, 15, state.boost);
            }
            _ => {
                let ready = state.flags & READY != 0;
                text(
                    output,
                    0,
                    4,
                    if ready {
                        &LABELS[17..23]
                    } else {
                        &LABELS[11..17]
                    },
                    if state.flags & BLINK != 0 {
                        ACCENT
                    } else {
                        WHITE
                    },
                );
                gauge(output, 0, 11, 22, state.charge);
            }
        }
    }
    if corner == 0 && state.fps != NO_FPS {
        let whole = crate::math::div_u32_u16(state.fps as u32, 10);
        let tens = crate::math::div_u32_u16(whole as u32, 10);
        let value = [
            b'F',
            if tens == 0 { b' ' } else { b'0' + tens as u8 },
            b'0' + (whole - tens * 10) as u8,
            b'.',
            b'0' + (state.fps - whole * 10) as u8,
        ];
        text(output, 2, 15, &value, WHITE);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn gauge(output: &mut [u8; 128], x: u8, y: u8, width: u8, fill: u8) {
    span(output, x, x + width + 2, y, WHITE);
    span(output, x, x + width + 2, y + 4, WHITE);
    for row in y + 1..y + 4 {
        pixel(output, x, row, WHITE);
        span(output, x + 1, x + 1 + fill, row, ACCENT);
        span(output, x + 1 + fill, x + width + 1, row, GREY);
        pixel(output, x + width + 1, row, WHITE);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn text(output: &mut [u8; 128], x: u8, y: u8, value: &[u8], color: u8) {
    let mut cursor = x;
    for &character in value {
        let glyph = crate::render::glyph(character);
        for row in 0..5u8 {
            let bits = unsafe { *glyph.get_unchecked(row as usize) };
            for column in 0..3 {
                if bits & (4 >> column) != 0 {
                    pixel(output, cursor + column, y + row, color);
                }
            }
        }
        cursor += 4;
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn span(output: &mut [u8; 128], left: u8, right: u8, y: u8, color: u8) {
    for x in left..right {
        pixel(output, x, y, color);
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mos", link_section = ".phase.pixels")]
fn pixel(output: &mut [u8; 128], x: u8, y: u8, color: u8) {
    let (base, column) = if x < 12 { (0u8, x) } else { (64u8, x - 12) };
    let index = base + y * 3 + column / 4;
    let shift = 6 - (column & 3) * 2;
    // All private drawing primitives stay within 24x21. The largest byte
    // index is 126, leaving both sprite padding bytes untouched.
    let byte = unsafe { output.get_unchecked_mut(index as usize) };
    *byte = (*byte & !(3 << shift)) | (color << shift);
}
