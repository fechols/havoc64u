//! H.A.V.O.C. hardware timing probe for the C64 Ultimate.
//!
//! Measures, with CIA 1 as a 1 MHz clock, how long the operations that bound
//! the game's frame take on the running machine: pure CPU loops before and
//! after enabling the turbo register, REU DMA in both directions, I/O
//! register traffic, and RAM copies with I/O mapped in and banked out.
//! Results are printed to the text screen in microseconds. Import the game's
//! `havoc.cfg` first so the turbo register selects the same CPU speed.
#![no_std]
#![no_main]

use core::ptr::{read_volatile as rd, write_volatile as wr};

extern "C" {
    fn probe_disable_interrupts();
    fn probe_cpu_loop();
    fn probe_write_color_ram();
    fn probe_write_border();
    fn probe_read_raster();
    fn probe_write_ram();
    fn probe_copy_8k(destination_page: u8);
}

/// Nominal 6502 cycles executed by `probe_cpu_loop`, including its call.
const CPU_LOOP_CYCLES: u32 = 329_229;
const SCREEN: u16 = 0x0400;
const COLOR_RAM: u16 = 0xd800;
/// Stash destination in the 1 MiB image's unreserved padding before music.
const STASH_REU_ADDRESS: u32 = 0x0bfc00;

#[inline(always)]
unsafe fn poke(address: u16, value: u8) {
    wr(address as *mut u8, value)
}
#[inline(always)]
unsafe fn peek(address: u16) -> u8 {
    rd(address as *const u8)
}

/// CIA 1 timer B counts timer A underflows: together a 32-bit 1 MHz
/// down-counter that runs for more than an hour without wrapping.
unsafe fn start_clock() {
    poke(0xdc0d, 0x7f);
    let _ = peek(0xdc0d);
    poke(0xdc0e, 0);
    poke(0xdc0f, 0);
    poke(0xdc04, 0xff);
    poke(0xdc05, 0xff);
    poke(0xdc06, 0xff);
    poke(0xdc07, 0xff);
    // Timer B first: input mode counts timer A underflows; force load; start.
    poke(0xdc0f, 0x51);
    poke(0xdc0e, 0x11);
}

unsafe fn clock() -> u32 {
    loop {
        let high = ((peek(0xdc07) as u32) << 24) | ((peek(0xdc06) as u32) << 16);
        let low = ((peek(0xdc05) as u32) << 8) | peek(0xdc04) as u32;
        let again = ((peek(0xdc07) as u32) << 24) | ((peek(0xdc06) as u32) << 16);
        if high == again {
            return high | low;
        }
    }
}

/// Microseconds spent in `work`; the counter runs down.
unsafe fn measure(work: impl FnOnce()) -> u32 {
    let start = clock();
    work();
    start.wrapping_sub(clock())
}

unsafe fn reu_transfer(command: u8, reu: u32, cpu: u16, count: u16) {
    poke(0xdf02, cpu as u8);
    poke(0xdf03, (cpu >> 8) as u8);
    poke(0xdf04, reu as u8);
    poke(0xdf05, (reu >> 8) as u8);
    poke(0xdf06, (reu >> 16) as u8);
    poke(0xdf07, count as u8);
    poke(0xdf08, (count >> 8) as u8);
    poke(0xdf09, 0);
    poke(0xdf0a, 0);
    poke(0xdf01, command);
}

/// Sixty-four 1 KiB transfers, the game's bounded DMA chunk size.
unsafe fn reu_64k(command: u8, fixed_reu: Option<u32>) {
    let mut reu = 0u32;
    for _ in 0..64 {
        reu_transfer(command, fixed_reu.unwrap_or(reu), 0x4000, 1024);
        reu += 1024;
    }
}

struct Screen {
    row: u8,
}
impl Screen {
    unsafe fn clear() -> Self {
        for offset in 0..1000u16 {
            poke(SCREEN + offset, 0x20);
            poke(COLOR_RAM + offset, 1);
        }
        poke(0xd020, 0);
        poke(0xd021, 0);
        Self { row: 0 }
    }
    unsafe fn text(&mut self, column: u8, text: &[u8]) {
        let base = SCREEN + self.row as u16 * 40 + column as u16;
        for (index, &byte) in text.iter().enumerate() {
            let code = match byte {
                b'a'..=b'z' => byte - b'a' + 1,
                b'A'..=b'Z' => byte - b'A' + 1,
                _ => byte,
            };
            poke(base + index as u16, code);
        }
    }
    unsafe fn number(&mut self, column: u8, mut value: u32) {
        let mut digits = [b' '; 10];
        let mut index = digits.len();
        loop {
            index -= 1;
            digits[index] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.text(column, &digits);
    }
    unsafe fn line(&mut self, label: &[u8], value: u32) {
        self.text(0, label);
        self.number(28, value);
        self.row += 1;
    }
    unsafe fn newline(&mut self) {
        self.row += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn main() -> i32 {
    probe_disable_interrupts();
    let mut screen = Screen::clear();
    screen.text(0, b"HAVOC HARDWARE PROBE  (ALL TIMES IN US)");
    screen.newline();
    start_clock();

    let stock = measure(|| probe_cpu_loop());
    screen.line(b"CPU LOOP, STOCK SPEED", stock);

    // The game's exact turbo request: C64U turbo registers, maximum speed.
    poke(0xd031, 0x8f);
    let turbo_register = peek(0xd031);
    screen.text(0, b"TURBO REGISTER D031 READS");
    let status: &[u8] = if turbo_register == 0x8f {
        b"8F OK"
    } else {
        b"NO TURBO"
    };
    screen.text(28, status);
    screen.newline();
    let turbo = measure(|| probe_cpu_loop());
    screen.line(b"CPU LOOP, TURBO", turbo);
    if turbo != 0 {
        // Tenths of the nominal 1 MHz speed, e.g. 640 for 64x.
        let tenths = (CPU_LOOP_CYCLES * 10 + turbo / 2) / turbo;
        screen.line(b"TURBO MULTIPLIER X10", tenths);
    }
    screen.newline();

    let fetch = measure(|| reu_64k(0x91, None));
    screen.line(b"REU FETCH 64X1KB TO RAM", fetch);
    screen.line(b"  PER KB", fetch / 64);
    let stash = measure(|| reu_64k(0x90, Some(STASH_REU_ADDRESS)));
    screen.line(b"REU STASH 64X1KB (PADDING)", stash);
    screen.line(b"  PER KB", stash / 64);
    screen.newline();

    let color = measure(|| probe_write_color_ram());
    screen.line(b"1024 STA D800,X", color);
    let border = measure(|| probe_write_border());
    screen.line(b"1024 STA D020", border);
    let raster = measure(|| probe_read_raster());
    screen.line(b"1024 LDA D012", raster);
    let ram = measure(|| probe_write_ram());
    screen.line(b"1024 STA 6000,X (RAM)", ram);
    screen.newline();

    let copy = measure(|| probe_copy_8k(0x60));
    screen.line(b"COPY 8KB A000 TO 6000", copy);
    // RAM under I/O, as the game's raster heads at $D000-$DFFF. Timing is
    // read before I/O returns; the ROMs stay banked out until then.
    poke(1, 0x34);
    let start = clock_with_io(0x34);
    probe_copy_8k(0xd0);
    poke(1, 0x37);
    let under_io = start.wrapping_sub(clock());
    screen.line(b"COPY 8KB TO D000 UNDER IO", under_io);
    screen.newline();
    screen.text(0, b"DONE. RESET TO EXIT.");
    loop {}
}

/// Read the clock, briefly exposing I/O when it is banked out.
unsafe fn clock_with_io(restore: u8) -> u32 {
    poke(1, 0x37);
    let value = clock();
    poke(1, restore);
    value
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
