#![no_std]
#![no_main]
#![allow(static_mut_refs)]

use core::{
    mem::MaybeUninit,
    ptr::{read_volatile as rd, write_volatile as wr},
};
use havoc::{
    assets::{self, AssetBank, Segment},
    audio::{AudioEngine, SidBus, VideoStandard},
    game::{Game, Hud, InputSample},
    hud,
    render::{self, Renderer},
    sky_cache::SkyCache,
};

#[link_section = ".renderer"]
static mut RENDERER: MaybeUninit<Renderer> = MaybeUninit::uninit();
#[link_section = ".game_state"]
static mut GAME: MaybeUninit<Game> = MaybeUninit::uninit();
#[link_section = ".audio_state"]
static mut AUDIO: MaybeUninit<AudioEngine> = MaybeUninit::uninit();
#[no_mangle]
pub static mut FRAME_SERIAL: u16 = 0;
#[no_mangle]
pub static mut VIDEO_FRAMES: u16 = 0;
#[no_mangle]
pub static mut AUDIO_TICKS: u16 = 0;
#[no_mangle]
pub static mut FRONT_BANK: u8 = 3;
#[no_mangle]
pub static mut FRAME_TRIANGLES: u16 = 0;
#[no_mangle]
pub static mut FRAME_DROPPED: u16 = 0;
#[no_mangle]
pub static mut GAME_MODE: u8 = 0;
#[no_mangle]
pub static mut GAME_DISTANCE: u32 = 0;
#[no_mangle]
pub static mut STREAM_BYTES: u16 = 0;
#[no_mangle]
pub static mut RENDER_SEGMENT: u8 = 0;
#[no_mangle]
pub static mut INPUT_OVERFLOWS: u16 = 0;
#[no_mangle]
pub static mut TEST_INPUT: u8 = 0xff;
#[no_mangle]
pub static mut PROF_PHASE: u8 = 0;
#[no_mangle]
pub static mut PROF_DETAIL: u8 = 0;
#[no_mangle]
pub static mut SKY_CACHE: SkyCache = SkyCache::new();
#[no_mangle]
pub static mut SKY_BANDS: u32 = 0;
// Host-monitored benchmark controls are absent from production builds.
#[cfg(feature = "benchmarks")]
#[no_mangle]
pub static mut BENCH_TICKS: u8 = 255;
#[cfg(feature = "benchmarks")]
#[no_mangle]
pub static mut BENCH_RUNTIME: u16 = 0;
static mut CLOCK_MS: u32 = 0;
static mut AUDIO_PERIOD: u16 = 19705;
static mut CLOCK_KHZ: u16 = 985;
static mut INPUTS: [InputSample; 64] = [InputSample {
    bits: 0,
    pressed: 0,
    ms: 0,
}; 64];
static mut INPUT_HEAD: u8 = 0;
static mut INPUT_TAIL: u8 = 0;
static mut LAST_BITS: u8 = 0;
static mut SEGMENT_CHECKSUMS: [u16; 4] = [0; 4];
extern "C" {
    fn irq_entry();
    fn nmi_entry();
    fn disable_irq();
    fn enable_irq();
    fn fast_copy_color();
    fn io_on();
    fn io_off();
    static __phase_boot_size: u8;
    static __phase_sim_size: u8;
    static __phase_geometry_size: u8;
    static __phase_pixels_size: u8;
    static __resident_end: u8;
}
struct HardwareSid;
impl SidBus for HardwareSid {
    fn write(&mut self, sid: u8, reg: u8, value: u8) {
        unsafe { poke(0xd400 + sid as u16 * 32 + reg as u16, value) }
    }
}
unsafe fn poke(p: u16, v: u8) {
    wr(p as *mut u8, v)
}
unsafe fn peek(p: u16) -> u8 {
    rd(p as *const u8)
}
unsafe fn word(p: u16, v: u16) {
    poke(p, v as u8);
    poke(p + 1, (v >> 8) as u8)
}

#[no_mangle]
pub unsafe extern "C" fn irq_body() {
    let cia = peek(0xdc0d);
    if cia & 1 != 0 {
        audio_tick();
    }
    if peek(0xd019) & 1 != 0 {
        poke(0xd019, 1);
        VIDEO_FRAMES = VIDEO_FRAMES.wrapping_add(1);
        let bits = if TEST_INPUT == 0xff {
            (!peek(0xdc00)) & 31
        } else {
            TEST_INPUT & 31
        };
        let timer = loop {
            let hi = peek(0xdc05);
            let lo = peek(0xdc04);
            if hi != peek(0xdc05) {
                continue;
            }
            // A reload between the first ICR read and timer sampling belongs to
            // the new 20-ms interval. Account for it before stamping the input.
            if peek(0xdc0d) & 1 != 0 {
                audio_tick();
                continue;
            }
            break lo as u16 | ((hi as u16) << 8);
        };
        let mut elapsed = AUDIO_PERIOD.saturating_sub(timer);
        let mut fraction = 0u8;
        while elapsed >= CLOCK_KHZ {
            elapsed -= CLOCK_KHZ;
            fraction += 1;
        }
        let ms = CLOCK_MS.wrapping_add(fraction as u32);
        let next = (INPUT_HEAD + 1) & 63;
        if next != INPUT_TAIL {
            INPUTS[INPUT_HEAD as usize] = InputSample {
                bits,
                pressed: bits & !LAST_BITS,
                ms,
            };
            INPUT_HEAD = next;
            LAST_BITS = bits;
        } else {
            INPUT_OVERFLOWS = INPUT_OVERFLOWS.wrapping_add(1);
        }
    }
}
#[inline(never)]
unsafe fn audio_tick() {
    CLOCK_MS = CLOCK_MS.wrapping_add(20);
    AUDIO_TICKS = AUDIO_TICKS.wrapping_add(1);
    AUDIO.assume_init_mut().tick(&mut HardwareSid);
}

unsafe fn fail(message: &[u8]) -> ! {
    disable_irq();
    poke(1, 0x35);
    // Restore normal VIC fetches for the character-mode error console.
    poke(0xd031, 0x0f);
    poke(0xd01a, 0);
    poke(0xdc0d, 0x7f);
    poke(0xdd0d, 0x7f);
    poke(0xd418, 0);
    poke(0xd438, 0);
    // Bank 2 exposes the VIC character ROM at $9000. Reuse screen A, keeping
    // the compiler's low-memory scratch intact even while displaying an error.
    poke(0xdd02, peek(0xdd02) | 3);
    poke(0xdd00, (peek(0xdd00) & 0xfc) | 1);
    poke(0xd011, 0x1b);
    poke(0xd016, 8);
    poke(0xd018, 0x04);
    poke(0xd020, 2);
    poke(0xd021, 0);
    for i in 0..1000u16 {
        poke(0x8000 + i, 32);
        poke(0xd800 + i, 1);
    }
    for (i, &b) in message.iter().enumerate() {
        poke(0x8051 + i as u16, b & 0x3f);
    }
    loop {}
}
unsafe fn dma(mut reu: u32, mut cpu: u16, mut count: u16) {
    if cpu as u32 + count as u32 > 0x10000 {
        fail(b"REU DESTINATION WOULD WRAP")
    }
    while count != 0 {
        let n = assets::dma_chunk(reu, cpu, count);
        if n == 0 {
            fail(b"INVALID REU TRANSFER")
        }
        word(0xdf02, cpu);
        poke(0xdf04, reu as u8);
        poke(0xdf05, (reu >> 8) as u8);
        poke(0xdf06, (reu >> 16) as u8);
        word(0xdf07, n);
        poke(0xdf09, 0);
        poke(0xdf0a, 0);
        poke(0xdf01, 0x91);
        reu += n as u32;
        cpu = cpu.wrapping_add(n);
        count -= n;
    }
}
#[inline(never)]
unsafe fn load_phase(index: u8, size: u16) {
    let mut offset = 0u16;
    while offset < size {
        let n = (size - offset).min(1024);
        dma(
            ((index as u32 + 1) << 16) + offset as u32,
            0x3000 + offset,
            n,
        );
        offset += n;
    }
}
#[inline(never)]
unsafe fn check_code_package() {
    dma(0x100, 0x7600, 32);
    let directory = core::slice::from_raw_parts(0x7600 as *const u8, 32);
    if &directory[..4] != b"OVL1"
        || assets::checksum(&directory[..30]) != assets::u16_at(directory, 30)
    {
        fail(b"REU CODE DIRECTORY MISSING OR INVALID")
    }
    let sizes = [
        core::ptr::addr_of!(__phase_boot_size) as u16,
        core::ptr::addr_of!(__phase_sim_size) as u16,
        core::ptr::addr_of!(__phase_geometry_size) as u16,
        core::ptr::addr_of!(__phase_pixels_size) as u16,
    ];
    let resident = core::slice::from_raw_parts(
        0x0801 as *const u8,
        core::ptr::addr_of!(__resident_end) as usize - 0x0801,
    );
    if assets::checksum(resident) != assets::u16_at(directory, 20)
        || assets::checksum(core::slice::from_raw_parts(0x7800 as *const u8, 2048))
            != assets::u16_at(directory, 22)
        || assets::checksum(core::slice::from_raw_parts(0x9900 as *const u8, 1792))
            != assets::u16_at(directory, 24)
    {
        fail(b"PRG AND REU DO NOT MATCH")
    }
    for index in 0..4 {
        let size = sizes[index];
        if size != assets::u16_at(directory, 4 + index * 2) || size == 0 || size > 0x4200 {
            fail(b"INVALID REU CODE SIZE")
        }
        load_phase(index as u8, size);
        if assets::checksum(core::slice::from_raw_parts(
            0x3000 as *const u8,
            size as usize,
        )) != assets::u16_at(directory, 12 + index * 2)
        {
            fail(b"REU CODE CHECKSUM FAILED")
        }
    }
    load_phase(0, sizes[0]);
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn load_initial_assets() {
    dma(0, 0x7600, 32);
    let header = core::slice::from_raw_parts(0x7600 as *const u8, 32);
    if assets::validate_header(header).is_err() {
        fail(b"LOAD MATCHING H.A.V.O.C. REU AT ZERO")
    }
    for i in 0..4 {
        SEGMENT_CHECKSUMS[i] = assets::u16_at(header, 20 + i * 2);
    }
    let common_check = assets::u16_at(header, 18);
    dma(assets::COMMON_OFFSET, 0x8400, 3072);
    let common = core::slice::from_raw_parts(0x8400 as *const u8, 3072);
    if assets::checksum(common) != common_check || AssetBank::new(common).is_err() {
        fail(b"INVALID COMMON ASSET BANK")
    }
    if AUDIO
        .assume_init_mut()
        .load_score(&common[2048..2560])
        .is_err()
    {
        fail(b"INVALID STEREO SCORE")
    }
}
unsafe fn segment_data(slot: u8) -> &'static [u8] {
    core::slice::from_raw_parts((0x9500 + slot as u16 * 512) as *const u8, 512)
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn validate_segment(slot: u8, index: u8) {
    let b = segment_data(slot);
    if assets::checksum(b) != SEGMENT_CHECKSUMS[index as usize]
        || Segment::new(b).map(|s| s.index() != index).unwrap_or(true)
    {
        fail(b"INVALID MISSION SEGMENT")
    }
}
unsafe fn raster() -> u16 {
    ((peek(0xd011) as u16 & 0x80) << 1) | peek(0xd012) as u16
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn detect_standard() -> VideoStandard {
    while raster() < 256 {}
    let mut max = 256;
    loop {
        let r = raster();
        if r < 256 {
            break;
        }
        if r > max {
            max = r;
        }
    }
    if max > 280 {
        VideoStandard::Pal
    } else {
        VideoStandard::Ntsc
    }
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_video() {
    poke(0xd011, 0x2b);
    poke(0xd016, 0x18);
    poke(0xd018, 0x08);
    poke(0xd015, 0);
    poke(0xd020, 0);
    poke(0xd021, 0);
    poke(0xdd02, peek(0xdd02) | 3);
    poke(0xdd00, peek(0xdd00) & 0xfc);
    for i in 0..8000u16 {
        poke(0xa000 + i, 0);
        poke(0xe000 + i, 0);
    }
    for i in 0..1000u16 {
        poke(0x8000 + i, 0);
        poke(0xc000 + i, 0);
        poke(0xd800 + i, 0);
        poke(0x9100 + i, 0);
    }
    for i in 0..32u16 {
        poke(0xc400 + i, 0xa5);
    }
    for i in 0..0x400u16 {
        poke(0x7200 + i, 0xa5);
    }
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_irq(standard: VideoStandard) {
    AUDIO_PERIOD = if standard == VideoStandard::Pal {
        19705
    } else {
        20455
    };
    CLOCK_KHZ = if standard == VideoStandard::Pal {
        985
    } else {
        1023
    };
    word(0xfffe, irq_entry as usize as u16);
    word(0xfffa, nmi_entry as usize as u16);
    poke(0xdc0d, 0x7f);
    poke(0xdd0d, 0x7f);
    let _ = peek(0xdc0d);
    let _ = peek(0xdd0d);
    poke(0xdc02, 0);
    poke(0xdc03, 0);
    poke(0xdc0e, 0);
    word(0xdc04, AUDIO_PERIOD - 1);
    poke(0xdc0d, 0x81);
    poke(0xdc0e, 0x11);
    poke(0xd012, 251);
    poke(0xd019, 0x0f);
    poke(0xd01a, 1);
    enable_irq();
}
unsafe fn clock_ms() -> u32 {
    disable_irq();
    let value = rd(core::ptr::addr_of!(CLOCK_MS));
    enable_irq();
    value
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn take_input(until: u32) -> Option<InputSample> {
    disable_irq();
    let result = if INPUT_TAIL != INPUT_HEAD
        && INPUTS[INPUT_TAIL as usize].ms.wrapping_sub(until) as i32 <= 0
    {
        let value = INPUTS[INPUT_TAIL as usize];
        INPUT_TAIL = (INPUT_TAIL + 1) & 63;
        Some(value)
    } else {
        None
    };
    enable_irq();
    result
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn send_audio(game: &mut Game) {
    disable_irq();
    let audio = AUDIO.assume_init_mut();
    audio.set_music(game.music_state());
    while let Some(e) = game.pop_audio() {
        audio.queue(e);
    }
    enable_irq();
}
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn frame_complete() {
    FRAME_SERIAL = FRAME_SERIAL.wrapping_add(1);
}
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn geometry_complete() {
    // This resident checkpoint precedes any PIXELS reuse of command storage.
    wr(core::ptr::addr_of_mut!(PROF_PHASE), 3);
}
#[cfg(feature = "benchmarks")]
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn benchmark_frame_begin() {
    if rd(core::ptr::addr_of!(BENCH_TICKS)) != 255 {
        while raster() != 250 {}
    }
    benchmark_frame_ready();
}
#[cfg(feature = "benchmarks")]
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn benchmark_frame_ready() {
    wr(core::ptr::addr_of_mut!(PROF_PHASE), 1);
}
#[inline(never)]
unsafe fn publish(bank: u8, last: &mut u16) {
    loop {
        disable_irq();
        let frames = rd(core::ptr::addr_of!(VIDEO_FRAMES));
        enable_irq();
        let line = raster();
        if frames.wrapping_sub(*last) >= 2 && (line >= 251 || line < 30) {
            break;
        }
    }
    disable_irq();
    fast_copy_color();
    poke(0xdd00, (peek(0xdd00) & 0xfc) | (3 - bank));
    FRONT_BANK = bank;
    poke(0xd011, 0x3b);
    *last = VIDEO_FRAMES;
    enable_irq();
    frame_complete();
}

#[link_section = ".phase.geometry"]
#[inline(never)]
unsafe fn prepare_geometry(current: u8, bank: &AssetBank<'_>) {
    havoc::projection::begin_frame();
    let game = GAME.assume_init_ref();
    let renderer = RENDERER.assume_init_mut();
    let data = segment_data(current);
    RENDER_SEGMENT = data[4];
    renderer.begin(6);
    game.render(renderer, bank, &Segment::from_validated(data));
    renderer.finish();
    SKY_BANDS = renderer.static_sky_bands();
    FRAME_TRIANGLES = renderer.triangle_count;
    FRAME_DROPPED = renderer.dropped;
}
#[link_section = ".phase.pixels"]
#[inline(never)]
unsafe fn draw_pixels(status: &Hud) -> u8 {
    let renderer = RENDERER.assume_init_mut();
    let front = rd(core::ptr::addr_of!(FRONT_BANK));
    let back = if front == 3 { 2 } else { 3 };
    let front_screen = if front == 3 { 0xc000 } else { 0x8000 };
    let back_screen = if back == 3 { 0xc000 } else { 0x8000 };
    let back_bitmap = if back == 3 { 0xe000 } else { 0xa000 };
    let eligible = SkyCache::eligible_bands(SKY_BANDS, status);
    let mut reusable = SKY_CACHE.reusable_bands() & eligible;
    // Preparation belongs to raster time, including before the opaque HUD band.
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 1);
    let Some(mut pass) = renderer.raster_pass() else {
        fail(b"RASTER COMMANDS ALREADY CONSUMED")
    };
    for band in 0..25u8 {
        if band != 0 {
            wr(core::ptr::addr_of_mut!(PROF_DETAIL), 1);
        }
        let pixels = &mut *(0x8c00 as *mut [u8; 1280]);
        let offset = band as u16 * 40;
        let previous_screen = &*((front_screen + offset) as *const [u8; 40]);
        let screen = &mut *((back_screen + offset) as *mut [u8; 40]);
        let reuse_sky = reusable & 1 != 0 && SkyCache::screens_match(previous_screen, screen);
        reusable >>= 1;
        // The cockpit completely replaces these two bands. Edge state still
        // advances so triangles crossing their boundaries remain continuous.
        let paint_scene = !reuse_sky
            && (status.mode == havoc::game::Mode::Title || (band != 0 && band != 24));
        if pass.next_band(pixels, paint_scene) != Some(band) {
            fail(b"INVALID RASTER BAND ORDER")
        }
        if reuse_sky {
            // Both banks contain the same static pixels and exact palette.
            // Keep the back bitmap/screen and the current front color shadow.
            continue;
        }
        wr(core::ptr::addr_of_mut!(PROF_DETAIL), 2);
        hud::draw(band, pixels, status, FRAME_SERIAL);
        let previous_color = &*((0x9100 + offset) as *const [u8; 40]);
        let bitmap = &mut *((back_bitmap + band as u16 * 320) as *mut [u8; 320]);
        // Stage only color: previous_color and mutable colors must not alias.
        let color = &mut *(0x7768 as *mut [u8; 40]);
        wr(core::ptr::addr_of_mut!(PROF_DETAIL), 3);
        render::pack_band(
            pixels,
            previous_screen,
            previous_color,
            bitmap,
            screen,
            color,
        );
        wr(core::ptr::addr_of_mut!(PROF_DETAIL), 4);
        core::ptr::copy_nonoverlapping(color.as_ptr(), (0x9100 + offset) as *mut u8, 40);
    }
    SKY_CACHE.record_frame(back, eligible);
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 0);
    back
}

#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn boot_init() {
    let standard = detect_standard();
    initialize_video();
    // Renderer consists solely of integer fields; its zero representation is
    // Renderer::new(). Initialize in place, without a 4102-byte stack temporary.
    io_off();
    core::ptr::write_bytes(
        RENDERER.as_mut_ptr().cast::<u8>(),
        0,
        core::mem::size_of::<Renderer>(),
    );
    io_on();
    initialize_game();
    initialize_audio(standard);
    load_initial_assets();
    AUDIO.assume_init_mut().reset(&mut HardwareSid);
    initialize_irq(standard);
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_game() {
    GAME.write(Game::new());
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_audio(standard: VideoStandard) {
    AUDIO.write(AudioEngine::new(standard));
}
struct Runtime {
    sim_ms: u32,
    loaded: [u8; 2],
    current: u8,
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn load_segment(slot: u8, index: u8) {
    dma(
        assets::SEGMENTS_OFFSET + index as u32 * 2048,
        0x9500 + slot as u16 * 512,
        512,
    );
    validate_segment(slot, index);
    STREAM_BYTES += 512;
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn simulate(runtime: &mut Runtime) -> Hud {
    let game = GAME.assume_init_mut();
    let need = game.segment_index();
    STREAM_BYTES = 0;
    if runtime.loaded[0] == need {
        runtime.current = 0
    } else if runtime.loaded[1] == need {
        runtime.current = 1
    } else {
        load_segment(runtime.current, need);
        runtime.loaded[runtime.current as usize] = need;
    }
    let desired = if matches!(
        game.hud().mode,
        havoc::game::Mode::GameOver | havoc::game::Mode::Victory
    ) {
        0
    } else {
        (need + 1) & 3
    };
    if !runtime.loaded.contains(&desired) {
        let slot = 1 - runtime.current;
        load_segment(slot, desired);
        runtime.loaded[slot as usize] = desired;
    }
    #[cfg(feature = "benchmarks")]
    let bench_ticks = rd(core::ptr::addr_of!(BENCH_TICKS));
    #[cfg(not(feature = "benchmarks"))]
    let bench_ticks = 255u8;
    let now = if bench_ticks == 255 {
        clock_ms()
    } else {
        runtime.sim_ms.wrapping_add(bench_ticks as u32 * 20)
    };
    while now.wrapping_sub(runtime.sim_ms) >= 20 {
        let need = game.segment_index();
        if runtime.loaded[0] == need {
            runtime.current = 0
        } else if runtime.loaded[1] == need {
            runtime.current = 1
        } else {
            break;
        }
        runtime.sim_ms = runtime.sim_ms.wrapping_add(20);
        if bench_ticks == 255 {
            while let Some(input) = take_input(runtime.sim_ms) {
                game.sample_input(input)
            }
        } else {
            game.sample_input(InputSample {
                bits: TEST_INPUT & 31,
                pressed: 0,
                ms: runtime.sim_ms,
            });
        }
        game.tick(&Segment::from_validated(segment_data(runtime.current)));
    }
    // The last catch-up tick can cross a checkpoint or restart the mission.
    // Select that tick's resulting section before rendering this same frame.
    let need = game.segment_index();
    if runtime.loaded[0] == need {
        runtime.current = 0;
    } else if runtime.loaded[1] == need {
        runtime.current = 1;
    } else {
        load_segment(runtime.current, need);
        runtime.loaded[runtime.current as usize] = need;
    }
    send_audio(game);
    let status = game.hud();
    GAME_MODE = status.mode as u8;
    GAME_DISTANCE = status.distance;
    status
}
#[no_mangle]
pub unsafe extern "C" fn main() -> i32 {
    disable_irq();
    io_on();
    poke(0xd031, 0x8f);
    if peek(0xd031) != 0x8f {
        fail(b"LOAD H.A.V.O.C. CFG: C64U TURBO REQUIRED")
    }
    check_code_package();
    boot_init();
    let bank = AssetBank::from_validated(core::slice::from_raw_parts(0x8400 as *const u8, 2048));
    let mut runtime = Runtime {
        sim_ms: 0,
        loaded: [0xff; 2],
        current: 0,
    };
    let mut last_present = 0;
    #[cfg(feature = "benchmarks")]
    {
        BENCH_RUNTIME = core::ptr::addr_of_mut!(runtime) as u16;
    }
    loop {
        #[cfg(feature = "benchmarks")]
        benchmark_frame_begin();
        #[cfg(not(feature = "benchmarks"))]
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 1);
        load_phase(1, core::ptr::addr_of!(__phase_sim_size) as u16);
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 5);
        let status = simulate(&mut runtime);
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 2);
        load_phase(2, core::ptr::addr_of!(__phase_geometry_size) as u16);
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 6);
        io_off();
        prepare_geometry(runtime.current, &bank);
        io_on();
        geometry_complete();
        load_phase(3, core::ptr::addr_of!(__phase_pixels_size) as u16);
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 7);
        io_off();
        let back = draw_pixels(&status);
        io_on();
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 4);
        publish(back, &mut last_present);
        if peek(0xc400) != 0xa5 || peek(0x7200) != 0xa5 {
            fail(b"STACK GUARD FAILED")
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { fail(b"H.A.V.O.C. PANIC - RESET TO EXIT") }
}
