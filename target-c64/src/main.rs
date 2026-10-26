#![no_std]
#![no_main]
#![allow(static_mut_refs)]

use core::{
    mem::MaybeUninit,
    ptr::{read_volatile as rd, write_volatile as wr},
};
use havoc::{
    assets::{self, AssetBank, Segment},
    audio::{EffectsEngine, MusicState, SidBus, VideoStandard},
    game::{Game, Hud, InputSample, PresentationEvent, INPUT_MASK},
    hud, hud_sprites,
    presentation::BombEffect,
    render::Renderer,
    space_dust::SpaceDustFrame,
    transition::TransitionAudio,
};

// Fatal diagnostics are consumed before their owning phase can be replaced.
// Keep shared/runtime errors resident; these messages have only local callers.
#[link_section = ".startup_checks.data"]
static ERROR_CODE_CHECKSUM: [u8; 24] = *b"REU CODE CHECKSUM FAILED";
#[link_section = ".startup_checks.data"]
static ERROR_CODE_SIZE: [u8; 21] = *b"INVALID REU CODE SIZE";
#[link_section = ".startup_checks.data"]
static ERROR_RESIDENT_PAIR: [u8; 24] = *b"PRG AND REU DO NOT MATCH";
#[link_section = ".startup_checks.data"]
static ERROR_CODE_DIRECTORY: [u8; 37] = *b"REU CODE DIRECTORY MISSING OR INVALID";
#[link_section = ".phase.boot.data"]
static ERROR_COMMON_BANK: [u8; 25] = *b"INVALID COMMON ASSET BANK";
#[link_section = ".phase.boot.data"]
static ERROR_MODEL_DIRECTORY: [u8; 23] = *b"INVALID MODEL DIRECTORY";
#[link_section = ".phase.boot.data"]
static ERROR_PACKAGE_HEADER: [u8; 36] = *b"LOAD MATCHING H.A.V.O.C. REU AT ZERO";
mod digital;

#[link_section = ".renderer"]
#[no_mangle]
static mut RENDERER: MaybeUninit<Renderer> = MaybeUninit::uninit();
#[link_section = ".game_state"]
static mut GAME: MaybeUninit<Game> = MaybeUninit::uninit();
// PIXELS reloads this writable tail together with its helper code every frame.
// It is never read by the IRQ and consumes no persistent resident storage.
#[link_section = ".phase.pixels_aux.data"]
static mut SPACE_DUST: SpaceDustFrame = SpaceDustFrame::EMPTY;
#[link_section = ".audio_state"]
#[no_mangle]
static mut AUDIO: MaybeUninit<digital::State> = MaybeUninit::uninit();
#[link_section = ".sfx_state"]
#[no_mangle]
static mut EFFECTS: MaybeUninit<EffectsEngine> = MaybeUninit::uninit();
const _: () = assert!(core::mem::size_of::<digital::State>() <= 432);
const _: () = assert!(core::mem::size_of::<EffectsEngine>() <= 224);
const BACKEND_DAC: u8 = 1;
const BACKEND_STARTING: u8 = 2;
const BACKEND_SFX_ONLY: u8 = 3;
#[no_mangle]
pub static mut MUSIC_BACKEND: u8 = BACKEND_STARTING;
// Set once at main by emulator tooling. Never a production fallback.
#[no_mangle]
pub static mut EMULATOR_SFX_ONLY: u8 = 0;
// Debug hook: nonzero re-enables the per-stream-byte CRC-8 and its 1 KiB
// checkpoints during digital playback (Fault::StreamCrc). Production leaves
// it off; startup still authenticates headers, codebooks and CRC tables.
#[no_mangle]
pub static mut DIGITAL_STREAM_CRC: u8 = 0;
#[no_mangle]
pub static mut DIGITAL_FAULT: u8 = 0;
#[no_mangle]
pub static mut DIGITAL_LOOPS: u16 = 0;
#[no_mangle]
pub static mut DIGITAL_MINIMUM_LEAD: u16 = 0;
#[no_mangle]
static mut REQUESTED_CUE: MusicState = MusicState::Title;
#[no_mangle]
static mut REQUESTED_STAGE: u8 = 255;
#[no_mangle]
static mut REQUESTED_MENU_TRACK: u8 = 255;
#[no_mangle]
static mut REQUESTED_MENU_SOUNDTRACK: bool = false;
static mut DIGITAL_CALIBRATION: digital::Calibration = digital::Calibration::new();
#[no_mangle]
static mut AUDIO_READY: bool = false;
#[no_mangle]
pub static mut MUSIC_CUE: u8 = 0;
#[no_mangle]
pub static mut MUSIC_UNDERRUNS: u16 = 0;
#[no_mangle]
pub static mut FRAME_SERIAL: u16 = 0;
#[no_mangle]
pub static mut VIDEO_FRAMES: u16 = 0;
#[no_mangle]
pub static mut AUDIO_TICKS: u16 = 0;
#[no_mangle]
pub static mut BOMB_EFFECT: BombEffect = BombEffect::new();
// Image storage lives in the VIC bank; retain only the compact display key.
static mut HUD_STATE: MaybeUninit<hud_sprites::DisplayState> = MaybeUninit::uninit();
static mut HUD_VALID: bool = false;
// The first frame replaces the boot bitmap band by band. Keep HUD sprites
// hidden until that publication finishes; this latch never repeats on wrap.
static mut FIRST_FRAME_PENDING: bool = true;
static mut HUD_ENABLE_STAGED: u8 = 0;
#[no_mangle]
pub static mut HUD_SCROLL: u8 = 0xff;
#[no_mangle]
pub static mut HUD_IMAGE_WRITES: u16 = 0;
#[no_mangle]
pub static mut HUD_COMMITS: u16 = 0;
// The raster IRQ owns sprite placement/enables; PIXELS can atomically cancel
// an armed reticle before publication. SHADOW stages one aim sample, which
// present publishes after all bitmap bands.
#[no_mangle]
pub static mut HUD_ENABLE_PENDING: u8 = 0;
#[no_mangle]
pub static mut HUD_ENABLE_ACTIVE: u8 = 0;
#[no_mangle]
pub static mut HUD_SPRITE0_COLOR: u8 = 3;
#[no_mangle]
pub static mut RETICLE_PENDING_X: u16 = 0;
#[no_mangle]
pub static mut RETICLE_PENDING_Y: u8 = 0;
#[no_mangle]
pub static mut RETICLE_PENDING_VISIBLE: u8 = 0;
#[no_mangle]
pub static mut RETICLE_ACTIVE_X: u16 = 0;
#[no_mangle]
pub static mut RETICLE_ACTIVE_Y: u8 = 0;
#[no_mangle]
pub static mut RETICLE_ACTIVE_VISIBLE: u8 = 0;
#[no_mangle]
pub static mut RETICLE_MISSES: u16 = 0;
/// Zero: HUD registers/next raster 92. One: reticle registers/next raster 251.
#[no_mangle]
pub static mut SPRITE_PHASE: u8 = 1;
#[no_mangle]
pub static mut IRQ_FRAME_EVENT: u8 = 0;
static mut RETICLE_FRAME_X: u16 = 0;
static mut RETICLE_FRAME_Y: u8 = 0;
static mut RETICLE_FRAME_VISIBLE: u8 = 0;
static mut RETICLE_PROBE: havoc::render::PlayerPixelProbe = havoc::render::PlayerPixelProbe::EMPTY;
static mut RETICLE_RETIRE_OLD: bool = false;
// The game publishes staged bands into VIC bank 2; tooling reads this constant to
// locate the visible screen and bitmap. initialize_video stores it so the
// linker keeps the symbol.
#[no_mangle]
pub static mut FRONT_BANK: u8 = 2;
#[no_mangle]
pub static mut FRAME_TRIANGLES: u16 = 0;
#[no_mangle]
pub static mut FRAME_DROPPED: u16 = 0;
#[no_mangle]
pub static mut GAME_MODE: u8 = 0;
#[no_mangle]
pub static mut GAME_DISTANCE: u32 = 0;
#[no_mangle]
pub static mut GAME_STAGE: u8 = 0;
#[no_mangle]
pub static mut STREAM_BYTES: u16 = 0;
#[no_mangle]
pub static mut MODEL_STREAM_BYTES: u16 = 0;
#[no_mangle]
pub static mut ENEMY_CODE_PROFILE: u8 = 0;
#[no_mangle]
pub static mut ENEMY_MODEL_PROFILE: u8 = 0xff;
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
// Host-monitored benchmark controls are absent from production builds.
#[cfg(feature = "benchmarks")]
#[no_mangle]
pub static mut BENCH_TICKS: u8 = 255;
#[cfg(feature = "benchmarks")]
#[no_mangle]
pub static mut BENCH_RUNTIME: u16 = 0;
// Sixteen-bit milliseconds (wrapping every 65.5 s): the simulation keeps every
// remembered instant within two seconds of the present, and every frame
// samples the controls at least once, also while paused.
static mut CLOCK_MS: u16 = 0;
static mut AUDIO_PERIOD: u16 = 19705;
#[no_mangle]
static mut CLOCK_KHZ: u16 = 985;
// Frame-rate overlay toggled by F. Each window covers at least one
// second of 50 Hz audio ticks on either video standard; the readout is in
// tenths of frames per second.
static mut FPS_SHOWN: bool = false;
/// Keys seen down last frame: bit 0 space, bit 1 F, bit 2 music chord, bit 3 S.
static mut KEYS_HELD: u8 = 0;
/// Set by the PIXELS key scan, consumed by SIM: simulation time stands still.
static mut PAUSED: bool = false;
static mut FPS_WINDOW_START: u16 = 0;
static mut FPS_WINDOW_FRAMES: u8 = 0;
static mut FPS_TENTHS: u16 = 0;
static mut INPUTS: [InputSample; 64] = [InputSample {
    bits: 0,
    pressed: 0,
    ms: 0,
}; 64];
static mut INPUT_HEAD: u8 = 0;
static mut INPUT_TAIL: u8 = 0;
static mut LAST_BITS: u8 = 0;
static mut SEGMENT_CHECKSUMS: [u16; assets::SEGMENT_COUNT as usize] =
    [0; assets::SEGMENT_COUNT as usize];
static mut MODEL_CHECKSUMS: [u16; assets::SEGMENT_COUNT as usize] =
    [0; assets::SEGMENT_COUNT as usize];
extern "C" {
    fn irq_entry();
    fn nmi_entry();
    fn disable_irq();
    fn enable_irq();
    fn read_controls() -> u8;
    fn restore_controls();
    fn io_on();
    fn io_off();
    fn publish_staged_band(band: u8);
    fn publish_hud_corner(corner: u8);
    fn hud_apply_scroll(scroll: u8);
    fn reticle_restore();
    fn reticle_hide();
    static __reticle_sprite_start: u8;
    static __phase_boot_size: u8;
    static __phase_sim_aux_prefix_size: u8;
    static __phase_sim_extra_size: u8;
    static __phase_geometry_prefix_size: u8;
    static __phase_geometry_base_size: u8;
    static __phase_render_aux_size: u8;
    static __phase_shadow_size: u8;
    static __phase_shadow_finalize_size: u8;
    static __phase_victory_size: u8;
    fn victory_geometry(game: *const (), renderer: *mut Renderer, bank: *const ());
    static __phase_pixels_prefix_size: u8;
    static __phase_pixels_aux_size: u8;
    static __phase_pixels_aux_reu_bank: u8;
    static __phase_pixels_aux_reu_offset: u8;
    static __phase_audio_size: u8;
    static __phase_audio_runtime_size: u8;
    static __phase_scenery_size: u8;
    static __phase_scenery_aux_prefix_size: u8;
    static __phase_scenery_extra_size: u8;
    static __phase_scenery_terrain_size: u8;
    static __phase_scenery_base_size: u8;
    static __phase_scenery_base_end: u8;
    static __phase_scenery_earth_tail_size: u8;
    static __resident_end: u8;
    static __startup_checks_start: u8;
    static __startup_checks_end: u8;
    static __code_sizes: [u16; 26];
    static __code_reu_pages: [u8; 34];
    static __sim_load_sizes: [u16; 8];
}
pub(crate) struct HardwareSid;
impl SidBus for HardwareSid {
    fn write(&mut self, sid: u8, reg: u8, value: u8) {
        // The engine bounds sid < 4 and reg < 25, so the register address
        // shares the same single-byte offset as its changed-write cache.
        unsafe { poke(0xd400 | u16::from((sid << 5) | reg), value) }
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
pub unsafe extern "C" fn irq_body(frame_event: u8) {
    let cia = peek(0xdc0d);
    if cia & 1 != 0 {
        audio_tick();
    }
    // The wrapper classifies/acknowledges raster events before calling Rust.
    // A middle-screen interrupt arriving during audio remains pending until
    // RTI; it must never become an extra frame/input sample here.
    if frame_event != 0 {
        VIDEO_FRAMES = VIDEO_FRAMES.wrapping_add(1);
        let [scroll, border] = BOMB_EFFECT.sample();
        poke(0xd016, 0x18 | (scroll & 7));
        // Preserve the splash/frame display enable. Bit seven reads raster
        // position but writes the IRQ compare high bit.
        poke(0xd011, (peek(0xd011) & 0x78) | (scroll >> 3));
        poke(0xd020, border);
        // Reassert positions every video frame: sprite zero has been borrowed
        // since line 92 even if the bitmap's fine scroll has not changed.
        hud_apply_scroll(scroll);
        reticle_restore();
        // Initial score validation can outlast the input queue. Begin input
        // capture and the simulation clock only when that boot work is done.
        if !AUDIO_READY {
            return;
        }
        let bits = if TEST_INPUT == 0xff {
            read_controls()
        } else {
            TEST_INPUT & INPUT_MASK
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
        let ms = CLOCK_MS.wrapping_add(fraction as u16);
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
#[cfg_attr(feature = "benchmarks", link_section = ".benchmark_resident")]
unsafe fn audio_tick() {
    if AUDIO_READY {
        CLOCK_MS = CLOCK_MS.wrapping_add(20);
    }
    AUDIO_TICKS = AUDIO_TICKS.wrapping_add(1);
    BOMB_EFFECT.tick();
    if AUDIO_READY {
        // The native body of EffectsEngine::tick (target-c64/asm/sfx.S)
        // advances EFFECTS and writes the four chips; tools/test-sfx-kernel.py
        // holds it to the host reference's exact state and write stream.
        extern "C" {
            fn fast_effects_tick();
        }
        fast_effects_tick();
    }

}

unsafe fn fail(message: &[u8]) -> ! {
    disable_irq();
    poke(1, 0x35);
    // Restore normal VIC fetches for the character-mode error console.
    poke(0xd031, 0x0f);
    poke(0xd01a, 0);
    poke(0xd015, 0);
    poke(0xdc0d, 0x7f);
    poke(0xdd0d, 0x7f);
    // The native sampler otherwise keeps running after a game/asset failure.
    poke(0xdf21, 0);
    poke(0xdf20, 0);
    poke(0xdf41, 0);
    poke(0xdf40, 0);
    poke(0xd418, 0);
    poke(0xd438, 0);
    poke(0xd458, 0);
    poke(0xd478, 0);
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
        // Keep foreground transfers short enough for the independent 50 Hz
        // audio IRQ, including the SIM helper image and model/cache loads.
        let n = assets::dma_chunk(reu, cpu, count).min(1024);
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
unsafe fn load_phase(page: u8, size: u16) {
    // Callers select fixed logical phase IDs; the checked linker table maps
    // them to 2 KiB-aligned slots without widening the hot loader interface.
    let reu_page = *__code_reu_pages.get_unchecked(page as usize);
    let reu_middle = reu_page << 3;
    let reu_bank = reu_page >> 5;
    let mut offset = 0u16;
    while offset < size {
        let n = (size - offset).min(1024);
        // Every caller uses a linker size (or a checked prefix),
        // bounded by $4200; startup also validates each full package size.
        // CPU $3000-$71FF never reaches I/O or wraps. Compact slots may cross
        // a 64 KiB source bank; carry the middle byte into the bank register.
        // Aligned starts and 1 KiB steps keep each individual DMA in one bank.
        // Thus general dma() would emit exactly one identical transfer here.
        // Keep its register order and 1024-byte IRQ-latency limit, avoiding its
        // repeated 32-bit bounds arithmetic for these statically bounded loads.
        word(0xdf02, 0x3000 + offset);
        poke(0xdf04, offset as u8);
        let (middle, carry) = reu_middle.overflowing_add((offset >> 8) as u8);
        poke(0xdf05, middle);
        poke(0xdf06, reu_bank + u8::from(carry));
        word(0xdf07, n);
        poke(0xdf09, 0);
        poke(0xdf0a, 0);
        poke(0xdf01, 0x91);
        offset += n;
    }
}
#[inline(never)]
unsafe fn load_sim(profile: u8) {
    // GEOMETRY, PIXELS and AUDIO reuse this workspace after SIM returns.
    // Reload on every SIM entry; the IRQ never references these helpers.
    dma(
        (__code_reu_pages[23] as u32) << 11,
        0x8e00,
        core::ptr::addr_of!(__phase_sim_aux_prefix_size) as u16,
    );
    // Previous raster/shadow records are dead. SCENERY resets the shadow frame
    // later in this frame, after SIM and its extra helper code have returned.
    dma(
        ((__code_reu_pages[23] as u32) << 11) + 0x500,
        0xe000,
        core::ptr::addr_of!(__phase_sim_extra_size) as u16,
    );
    load_phase(
        if profile == 7 {
            13
        } else if profile == 0 {
            2
        } else if profile < 4 {
            profile + 7
        } else {
            profile + 14
        },
        // All callers use Game::simulation_profile(), which is bounded 0..7.
        // The startup check validated the complete padded image already.
        *__sim_load_sizes.get_unchecked(profile as usize),
    );
    ENEMY_CODE_PROFILE = profile;
}
#[link_section = ".startup_checks"]
#[inline(never)]
unsafe fn check_code_package() {
    dma(0x100, 0x7600, 128);
    let directory = core::slice::from_raw_parts(0x7600 as *const u8, 128);
    if &directory[..4] != b"OVL7"
        || assets::checksum(&directory[..126]) != assets::u16_at(directory, 126)
    {
        fail(&ERROR_CODE_DIRECTORY)
    }
    let sizes = &__code_sizes;
    let resident = core::slice::from_raw_parts(
        0x0801 as *const u8,
        core::ptr::addr_of!(__resident_end) as usize - 0x0801,
    );
    if directory[32] != havoc::audio::SID_COUNT as u8
        || assets::u16_at(directory, 120) != 768
        || assets::checksum(core::slice::from_raw_parts(0x7200 as *const u8, 768))
            != assets::u16_at(directory, 122)
        || assets::u16_at(directory, 124) != 0
        || assets::checksum(resident) != assets::u16_at(directory, 24)
        || assets::checksum(core::slice::from_raw_parts(0x7800 as *const u8, 2048))
            != assets::u16_at(directory, 26)
        || assets::checksum(core::slice::from_raw_parts(0x9900 as *const u8, 1792))
            != assets::u16_at(directory, 28)
        || assets::checksum(core::slice::from_raw_parts(
            core::ptr::addr_of!(__startup_checks_start),
            core::ptr::addr_of!(__startup_checks_end) as usize
                - core::ptr::addr_of!(__startup_checks_start) as usize,
        )) != assets::u16_at(directory, 30)
    {
        fail(&ERROR_RESIDENT_PAIR)
    }
    for index in 0..26 {
        let size = sizes[index];
        let size_offset = if index == 25 {
            58
        } else if index >= 11 {
            64 + (index - 11) * 4
        } else if index >= 6 {
            38 + (index - 6) * 4
        } else if index == 5 {
            34
        } else {
            4 + index * 2
        };
        let hash_offset = if index >= 6 {
            size_offset + 2
        } else if index == 5 {
            36
        } else {
            14 + index * 2
        };
        if size != assets::u16_at(directory, size_offset) || size == 0 || size > 0x4200 {
            fail(&ERROR_CODE_SIZE)
        }
        load_phase(
            if index == 25 {
                33
            } else if index >= 11 {
                index as u8 + 7
            } else if index >= 9 {
                index as u8 + 3
            } else if index >= 5 {
                index as u8 + 2
            } else {
                index as u8 + 1
            },
            size,
        );
        if assets::checksum(core::slice::from_raw_parts(
            0x3000 as *const u8,
            size as usize,
        )) != assets::u16_at(directory, hash_offset)
        {
            fail(&ERROR_CODE_CHECKSUM)
        }
    }
    load_phase(1, sizes[0]);
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn load_initial_assets() {
    dma(0, 0x7600, assets::PACKAGE_HEADER_SIZE as u16);
    let header = core::slice::from_raw_parts(0x7600 as *const u8, assets::PACKAGE_HEADER_SIZE);
    if assets::validate_header(header).is_err() {
        fail(&ERROR_PACKAGE_HEADER)
    }
    for i in 0..assets::SEGMENT_COUNT as usize {
        SEGMENT_CHECKSUMS[i] = assets::u16_at(header, 20 + i * 2);
    }
    let common_check = assets::u16_at(header, 18);
    // Validate every segment's structure before play. Later transfers check
    // their complete runtime prefix against these validated package hashes.
    for level in 0..assets::SEGMENT_COUNT {
        dma(
            assets::SEGMENTS_OFFSET + level as u32 * 2048,
            segment_address(0),
            512,
        );
        let bytes = segment_data(0);
        if assets::checksum(bytes) != SEGMENT_CHECKSUMS[level as usize]
            || bytes[4] != level
            || Segment::new(bytes).is_err()
        {
            fail(b"INVALID MISSION SEGMENT")
        }
    }
    dma(assets::COMMON_OFFSET, 0x8400, 3072);
    let common = core::slice::from_raw_parts(0x8400 as *const u8, 3072);
    if assets::checksum(common) != common_check || common[5] != 0 || AssetBank::new(common).is_err()
    {
        fail(&ERROR_COMMON_BANK)
    }
    let common_models_check = assets::checksum(&common[..assets::MODEL_SIZE]);
    dma(
        assets::MODEL_DIRECTORY_OFFSET,
        0x7600,
        assets::MODEL_DIRECTORY_SIZE as u16,
    );
    let directory = core::slice::from_raw_parts(0x7600 as *const u8, assets::MODEL_DIRECTORY_SIZE);
    if assets::validate_model_directory(directory).is_err() {
        fail(&ERROR_MODEL_DIRECTORY)
    }
    for level in 0..assets::SEGMENT_COUNT as usize {
        MODEL_CHECKSUMS[level] = assets::model_checksum(directory, level as u8);
    }
    if common_models_check != MODEL_CHECKSUMS[0] {
        fail(&ERROR_COMMON_BANK)
    }
    for level in (0..assets::SEGMENT_COUNT).rev() {
        copy_model_bank(level);
        let bytes = core::slice::from_raw_parts(0x8400 as *const u8, assets::MODEL_SIZE);
        if assets::checksum(bytes) != MODEL_CHECKSUMS[level as usize]
            || AssetBank::new(bytes)
                .map(|bank| bank.level() != level)
                .unwrap_or(true)
        {
            fail(b"INVALID LEVEL MODEL BANK")
        }
    }
    ENEMY_MODEL_PROFILE = 0;
}
#[inline(always)]
unsafe fn copy_model_bank(level: u8) {
    let source = assets::MODEL_BANKS_OFFSET + level as u32 * assets::MODEL_SIZE as u32;
    dma(source, 0x8400, 1024);
    dma(source + 1024, 0x8800, 1024);
}
#[inline(always)]
fn segment_address(slot: u8) -> u16 {
    if slot == 0 {
        0x0600
    } else {
        0x9700
    }
}
unsafe fn segment_data(slot: u8) -> &'static [u8] {
    core::slice::from_raw_parts(segment_address(slot) as *const u8, 512)
}
// Keep validation with SIM in both production and fixed-work benchmarks.
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn validate_segment(slot: u8, index: u8) {
    let b = segment_data(slot);
    if assets::checksum(b) != SEGMENT_CHECKSUMS[index as usize] {
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
    // Early assembly already displays the splash in VIC bank 2. Preserve
    // its bitmap, attributes and mode through all remaining initialization.
    wr(core::ptr::addr_of_mut!(FRONT_BANK), 2);
    for i in 0..32u16 {
        poke(0xc400 + i, 0xa5);
    }
    for i in 0..0x100u16 {
        poke(0x7500 + i, 0xa5);
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
    poke(0xdc03, 0);
    // Timer B supplies the unaccelerated Phi2 settling clock for POTX. It has
    // no interrupt enabled and is independent of audio's Timer A cadence.
    poke(0xdc0f, 0);
    word(0xdc06, 65535);
    poke(0xdc0f, 0x11);
    restore_controls();
    poke(0xdc0e, 0);
    word(0xdc04, AUDIO_PERIOD - 1);
    poke(0xdc0d, 0x81);
    poke(0xdc0e, 0x11);
    poke(0xd012, 251);
    poke(0xd019, 0x0f);
    poke(0xd01a, 1);
    enable_irq();
}
unsafe fn clock_ms() -> u16 {
    disable_irq();
    let value = rd(core::ptr::addr_of!(CLOCK_MS));
    enable_irq();
    value
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn take_input(until: u16) -> Option<InputSample> {
    disable_irq();
    let result = if INPUT_TAIL != INPUT_HEAD
        && INPUTS[INPUT_TAIL as usize].ms.wrapping_sub(until) as i16 <= 0
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
// The publisher runs only from SIM, whose extra helpers are loaded with every
// profile. Benchmark instrumentation uses spare resident space in that build.
#[cfg_attr(not(feature = "benchmarks"), link_section = ".phase.sim_extra")]
#[inline(never)]
unsafe fn send_audio(game: &mut Game) {
    disable_irq();
    REQUESTED_CUE = game.music_state();
    REQUESTED_STAGE = game.segment_index();
    REQUESTED_MENU_TRACK = game.menu_music_override();
    REQUESTED_MENU_SOUNDTRACK = game.menu_soundtrack_enabled();
    if EMULATOR_SFX_ONLY != 0 { MUSIC_CUE = REQUESTED_CUE as u8; }
    while let Some(e) = game.pop_audio() {
        if AUDIO_READY {
            EFFECTS.assume_init_mut().queue(e);
        }
    }
    enable_irq();
}

/// DAC state is permanently foreground-owned; the IRQ borrows only EFFECTS.
#[link_section = ".phase.audio"]
#[inline(never)]
unsafe fn service_audio() {
    if !AUDIO_READY {
        let standard = if CLOCK_KHZ == 985 { VideoStandard::Pal } else { VideoStandard::Ntsc };
        EFFECTS.write(EffectsEngine::new(standard));
        EFFECTS.assume_init_mut().reset(&mut HardwareSid);
    }
    // Benchmark fixtures restore only SID state; eliminate unreachable DAC service.
    if cfg!(feature = "benchmarks") || rd(core::ptr::addr_of!(EMULATOR_SFX_ONLY)) != 0 {
        MUSIC_BACKEND = BACKEND_SFX_ONLY;
        MUSIC_CUE = REQUESTED_CUE as u8;
        AUDIO_READY = true;
        return;
    }
    let mut track = havoc::digital::Track::from_id(REQUESTED_MENU_TRACK)
        .unwrap_or_else(|| havoc::digital::track_for_scene(REQUESTED_CUE, REQUESTED_STAGE));
    let state = AUDIO.as_mut_ptr();
    if MUSIC_BACKEND != BACKEND_DAC {
        digital::clear(state);
        if let Err(fault) = digital::init(&mut *state, &mut DIGITAL_CALIBRATION, CLOCK_KHZ == 985, track) {
            digital_failure(fault);
        }
        // Keep the boot splash visible while the initial ring is primed.
        MUSIC_BACKEND = BACKEND_DAC;
    }
    digital::request_menu(&mut *state, track, REQUESTED_MENU_SOUNDTRACK);
    loop {
        if let Err(fault) = digital::service(&mut *state) { digital_failure(fault); }
        // Only startup waits for a complete ring. Once ready, each foreground
        // visit keeps the existing bounded refill/track-transition budget.
        if AUDIO_READY || (*state).is_running() { break; }
    }
    if let Some(next) = digital::take_auto_advance(&mut *state) {
        GAME.assume_init_mut().sync_menu_music(next);
        REQUESTED_MENU_TRACK = next as u8;
        REQUESTED_CUE = if next == havoc::digital::Track::GalacticDash {
            MusicState::Victory
        } else {
            MusicState::Title
        };
        track = next;
    }
    DIGITAL_LOOPS = (*state).loops;
    DIGITAL_MINIMUM_LEAD = (*state).minimum_lead;
    MUSIC_UNDERRUNS = (*state).underruns;
    if (*state).is_running() && (*state).track == track { MUSIC_CUE = REQUESTED_CUE as u8; }
    AUDIO_READY = true;
}

#[link_section = ".phase.audio"]
#[inline(never)]
unsafe fn digital_failure(fault: digital::Fault) -> ! {
    DIGITAL_FAULT = fault as u8;
    // One AUDIO-owned message avoids resident strings and retains the exact
    // fault in telemetry and on the error screen.
    ERROR_DIGITAL[20] = b'0' + fault as u8;
    fail(&ERROR_DIGITAL)
}
#[link_section = ".phase.audio.data"]
static mut ERROR_DIGITAL: [u8; 36] = *b"DIGITAL AUDIO ERROR 0 - CHECK CONFIG";

#[link_section = ".phase.boot.data"]
static ERROR_BOOT_MUSIC: [u8; 24] = *b"INVALID MUSIC PACKAGE   ";

/// Runs while BOOT owns the overlay. Effects and track decoding start later.
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_music_boot() {
    if rd(core::ptr::addr_of!(EMULATOR_SFX_ONLY)) == 0 {
        if let Err(fault) = digital::bootstrap(&mut DIGITAL_CALIBRATION, CLOCK_KHZ == 985) {
            DIGITAL_FAULT = fault as u8;
            fail(&ERROR_BOOT_MUSIC);
        }
    }
}

#[no_mangle]
#[inline(never)]
#[cfg_attr(feature = "benchmarks", link_section = ".benchmark_resident")]
pub unsafe extern "C" fn frame_complete() {
    FRAME_SERIAL = FRAME_SERIAL.wrapping_add(1);
}
#[no_mangle]
#[inline(never)]
#[cfg_attr(feature = "benchmarks", link_section = ".benchmark_resident")]
pub unsafe extern "C" fn geometry_complete() {
    // This resident checkpoint precedes any PIXELS reuse of command storage.
    // Keep it in ordinary resident code: instrumented four-SID builds exhaust
    // constant RAM, while the ordinary resident region has room for this call.
    wr(core::ptr::addr_of_mut!(PROF_PHASE), 3);
}
#[cfg(feature = "benchmarks")]
#[no_mangle]
#[inline(never)]
#[cfg_attr(feature = "benchmarks", link_section = ".benchmark_resident")]
pub unsafe extern "C" fn benchmark_frame_begin() {
    if rd(core::ptr::addr_of!(BENCH_TICKS)) != 255 {
        while raster() != 250 {}
    }
    benchmark_frame_ready();
}
#[cfg(feature = "benchmarks")]
#[no_mangle]
#[inline(never)]
#[link_section = ".benchmark_resident"]
pub unsafe extern "C" fn benchmark_frame_ready() {
    wr(core::ptr::addr_of_mut!(PROF_PHASE), 1);
}
// Capture the benchmark deadline in resident main before SIM reentry.
#[cfg(feature = "benchmarks")]
#[inline(never)]
#[link_section = ".benchmark_resident"]
unsafe fn benchmark_now(sim_ms: u16, ticks: u8) -> u16 {
    sim_ms.wrapping_add(ticks as u16 * 20)
}
// The front renderer keeps the completed-frame handoff in its roomier helper
// tail; draw_pixels has restored that tail before raster work begins.
#[cfg_attr(feature = "span-front-to-back", link_section = ".phase.pixels_aux")]
#[cfg_attr(not(feature = "span-front-to-back"), link_section = ".phase.pixels")]
#[inline(never)]
unsafe fn present() {
    io_on();
    // Every band has already been published into the visible bank and color RAM.
    // The boot splash was already visible. Finish its first-frame handoff
    // without blanking, preserving the raster-driven bomb shake's Y scroll.
    disable_irq();
    RETICLE_PENDING_X = RETICLE_FRAME_X;
    RETICLE_PENDING_Y = RETICLE_FRAME_Y;
    RETICLE_PENDING_VISIBLE = RETICLE_FRAME_VISIBLE & !PAUSED as u8;
    if FIRST_FRAME_PENDING {
        HUD_ENABLE_PENDING = HUD_ENABLE_STAGED;
        FIRST_FRAME_PENDING = false;
    }
    poke(0xd011, 0x38 | (peek(0xd011) & 7));
    enable_irq();
    frame_complete();
}

/// Frame-rate bookkeeping and the keyboard scan run inside PIXELS, which has
/// the overlay room; only their few state bytes stay resident. The caller
/// must have I/O mapped in: the CIA registers are read directly.
#[link_section = ".phase.pixels"]
#[inline(never)]
unsafe fn update_fps_overlay(mode: havoc::game::Mode) {
    disable_irq();
    let ticks = AUDIO_TICKS;
    enable_irq();
    FPS_WINDOW_FRAMES = FPS_WINDOW_FRAMES.saturating_add(1);
    let elapsed = ticks.wrapping_sub(FPS_WINDOW_START);
    if elapsed >= 50 {
        // frames * 50 Hz / ticks, in tenths; frames stay far below 130.
        FPS_TENTHS =
            havoc::math::div_u32_u16((FPS_WINDOW_FRAMES.min(130) as u16 * 500) as u32, elapsed);
        FPS_WINDOW_START = ticks;
        FPS_WINDOW_FRAMES = 0;
    }
    // Row 7: Space column 4, CTRL column 2, Commodore column 5.
    // F is row 2 column 5; S is row 1 column 5; DEL is row 0 column 0.
    // Comma/< and period/> are row 5, columns 7 and 4. CIA 1 port A
    // doubles as joystick port 2, so drive it as key rows only briefly with
    // the sampling IRQ masked, then restore joystick inputs and paddle port 2.
    // A column counts as a key only if it also reads high with no row selected:
    // a joystick in port 1 or a test harness driving port B cannot fake a key.
    disable_irq();
    poke(0xdc02, 0xff);
    poke(0xdc00, 0xff);
    let idle = peek(0xdc01);
    poke(0xdc00, 0x7f);
    let row7 = !peek(0xdc01);
    let space = row7 & idle & 0x10 != 0;
    poke(0xdc00, 0xfb);
    let f_key = peek(0xdc01) & 0x20 == 0 && idle & 0x20 != 0;
    poke(0xdc00, 0xfd);
    let story_key = peek(0xdc01) & 0x20 == 0;
    poke(0xdc00, 0xfe);
    let delete = peek(0xdc01) & 1 == 0;
    poke(0xdc00, 0xdf);
    let row5 = !peek(0xdc01);
    let previous = row5 & 0x80 != 0;
    let next = row5 & 0x10 != 0;
    restore_controls();
    enable_irq();
    let music_chord = row7 & 0x24 == 0x24 && delete;
    let keys = space as u8
        | (f_key as u8) << 1
        | (music_chord as u8) << 2
        | (story_key as u8) << 3
        | (previous as u8) << 4
        | (next as u8) << 5;
    let pressed = keys & !KEYS_HELD;
    // Pause only has meaning while flying; elsewhere the key is ignored and
    // a stale pause cannot survive, because no tick runs while it is set.
    if pressed & 1 != 0
        && matches!(
            mode,
            havoc::game::Mode::Playing | havoc::game::Mode::Transition
        )
    {
        PAUSED = !PAUSED;
    }
    if pressed & 2 != 0 {
        FPS_SHOWN = !FPS_SHOWN;
    }
    if pressed & 4 != 0 && idle & 0x25 == 0x25 {
        // Inlines into PIXELS; the next SIM/AUDIO pass publishes the selection.
        GAME.assume_init_mut().toggle_menu_music();
    } else if !(previous && next) {
        if pressed & 0x10 != 0 && idle & 0x80 != 0 {
            GAME.assume_init_mut().step_menu_music(true);
        } else if pressed & 0x20 != 0 && idle & 0x10 != 0 {
            GAME.assume_init_mut().step_menu_music(false);
        }
    }
    if pressed & 8 != 0 && idle & 0x20 != 0 {
        // Title-only selection; the ordinary Fire edge still launches in SIM.
        GAME.assume_init_mut().toggle_story_mode();
    }
    // Latch raw music/S keys even when idle filtering or mode guards suppress them. Releasing
    // joystick interference or changing modes must not rearm held keys.
    KEYS_HELD = keys;
}

#[link_section = ".phase.scenery"]
#[inline(never)]
unsafe fn prepare_scenery(current: u8, sounds: &mut TransitionAudio) -> u8 {
    io_on();
    dma(
        (__code_reu_pages[33] as u32) << 11,
        0x8e00,
        core::ptr::addr_of!(__phase_scenery_aux_prefix_size) as u16,
    );
    dma(
        ((__code_reu_pages[33] as u32) << 11) + 0x500,
        0xe000,
        core::ptr::addr_of!(__phase_scenery_extra_size) as u16,
    );
    io_off();
    let game = GAME.assume_init_ref();
    let data = segment_data(current);
    // Telemetry only: tools read the section whose scenery is being drawn.
    RENDER_SEGMENT = data[4];
    let bridge = data[10];
    if bridge < 3 && game.transition_progress().is_some() {
        // Terrestrial crossings omit the space helpers between this prefix and
        // the opaque tail. Restore only the selected tail with bounded DMA.
        let page = match bridge {
            1 => 0x15,
            2 => 0x16,
            _ => 7,
        };
        io_on();
        dma(
            ((__code_reu_pages[page] as u32) << 11)
                + core::ptr::addr_of!(__phase_scenery_base_size) as u32,
            core::ptr::addr_of!(__phase_scenery_base_end) as u16,
            core::ptr::addr_of!(__phase_scenery_earth_tail_size) as u16,
        );
        io_off();
    }
    let cues = sounds.update(game.transition_progress().map(|(_, tick)| (bridge, tick)));
    // I/O is banked out here. Only update RAM; the IRQ restores I/O before
    // writing SID registers. No effects reference survives this mask.
    disable_irq();
    if AUDIO_READY {
        let audio = EFFECTS.assume_init_mut();
        if cues.cancel {
            audio.cancel_scene_effects();
        }
        for lane in 0..2 {
            if let Some(event) = cues.events[lane] {
                audio.queue(event);
            }
        }
    }
    enable_irq();
    let renderer = RENDERER.assume_init_mut();
    renderer.begin(game.sky_color());
    let shadows = havoc::shadow::native_frame();
    shadows.reset();
    let mut stage = if game.shadow_ground_y().is_some() {
        renderer.scenery_stage_with_shadows(shadows)
    } else {
        renderer.scenery_stage()
    };
    game.stage_scenery(&mut stage, &Segment::from_validated(data));
    stage.count()
}

// Instrumented builds use spare resident space for this hardware-only helper.
// Keep production's reload in GEOMETRY; the additional benchmark code otherwise
// pushes that tightly bounded overlay into its projection-helper window.
#[cfg(feature = "benchmarks")]
#[inline(never)]
unsafe fn benchmark_load_projection() {
    io_on();
    dma(
        ((__code_reu_pages[3] as u32) << 11) + 0x3f20,
        0x6f20,
        core::ptr::addr_of!(__phase_render_aux_size) as u16,
    );
    io_off();
}

#[link_section = ".phase.geometry"]
#[inline(never)]
unsafe fn prepare_geometry(scenery_count: u8, bank: &AssetBank<'_>) {
    #[cfg(feature = "benchmarks")]
    benchmark_load_projection();
    #[cfg(not(feature = "benchmarks"))]
    {
        io_on();
        dma(
            ((__code_reu_pages[3] as u32) << 11) + 0x3f20,
            0x6f20,
            core::ptr::addr_of!(__phase_render_aux_size) as u16,
        );
        io_off();
    }
    havoc::projection::begin_frame();
    let game = GAME.assume_init_mut();
    game.prepare_damage_feedback_frame();
    let renderer = RENDERER.assume_init_mut();
    let [parallax_x, parallax_y] = game.sky_parallax();
    // Detail markers 5-8 split GEOMETRY work for the profilers; 0 ends it.
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 5);
    let prefix = renderer.project_scenery(scenery_count, parallax_x, parallax_y);
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 6);
    game.render_foreground(renderer, bank, 174.min(renderer.remaining_commands()));
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 7);
    renderer.scenery_last(prefix);
    if game.mode() == havoc::game::Mode::Victory {
        victory_geometry(
            (game as *const Game).cast(),
            renderer,
            (bank as *const AssetBank<'_>).cast(),
        );
    }
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 8);
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 0);
}

#[link_section = ".phase.shadow.final"]
#[inline(never)]
unsafe fn prepare_reticle_probe() {
    let aim = GAME
        .assume_init_ref()
        .reticle_position()
        .filter(|&(x, y)| (0..160).contains(&x) && (0..200).contains(&y));
    if let Some(center) = aim {
        let (x, y) = havoc::reticle::vic_position(center);
        RETICLE_FRAME_X = x;
        RETICLE_FRAME_Y = y;
        RETICLE_FRAME_VISIBLE = 1;
    } else {
        RETICLE_FRAME_VISIBLE = 0;
    }

    // Pending is immutable until present; active can latch it in the IRQ.
    // Snapshot both atomically before testing the next rendered silhouette.
    disable_irq();
    let active = if core::ptr::read_volatile(core::ptr::addr_of!(RETICLE_ACTIVE_VISIBLE)) != 0 {
        Some(havoc::reticle::screen_position((
            core::ptr::read_volatile(core::ptr::addr_of!(RETICLE_ACTIVE_X)),
            core::ptr::read_volatile(core::ptr::addr_of!(RETICLE_ACTIVE_Y)),
        )))
    } else {
        None
    };
    let pending = if core::ptr::read_volatile(core::ptr::addr_of!(RETICLE_PENDING_VISIBLE)) != 0 {
        Some(havoc::reticle::screen_position((
            core::ptr::read_volatile(core::ptr::addr_of!(RETICLE_PENDING_X)),
            core::ptr::read_volatile(core::ptr::addr_of!(RETICLE_PENDING_Y)),
        )))
    } else {
        None
    };
    enable_irq();
    RETICLE_PROBE = havoc::render::PlayerPixelProbe::new([aim, active, pending]);
    RETICLE_RETIRE_OLD = false;
}

#[link_section = ".phase.shadow.entry"]
#[inline(never)]
unsafe fn prepare_shadows(enabled: bool) {
    if enabled {
        setup_shadows();
    }
    let renderer = RENDERER.assume_init_mut();
    let shadows = havoc::shadow::native_frame();
    if enabled {
        shadows.finish(renderer);
    } else {
        shadows.finish_without_shadows(renderer);
    }
    FRAME_TRIANGLES = renderer.triangle_count;
    FRAME_DROPPED = renderer.dropped;
    geometry_complete();
    prepare_reticle_probe();
    let probe = &mut *core::ptr::addr_of_mut!(RETICLE_PROBE);
    renderer.prepare_raster_with_probe(probe);
    if probe.covered(0) {
        RETICLE_FRAME_VISIBLE = 0;
    }
    RETICLE_RETIRE_OLD = probe.covered(1) || probe.covered(2);
}

// Only the full image may enter this tail. Disabled levels load just the
// guarded entry plus ordinary sorting/preparation, and always pass false.
#[link_section = ".phase.shadow"]
#[inline(never)]
unsafe fn setup_shadows() {
    let shadows = havoc::shadow::native_frame();
    let bank = AssetBank::from_validated(core::slice::from_raw_parts(
        0x8400 as *const u8,
        assets::MODEL_SIZE,
    ));
    let game = GAME.assume_init_ref();
    if let Some(ground_y) = game.shadow_ground_y() {
        game.collect_shadow_casters(shadows, &bank);
        shadows.render_unsorted(
            RENDERER.assume_init_mut(),
            ground_y,
            game.shadow_color(),
        );
    }
}

// This entry replaces the combat tail at the same execution address. The
// common prefix calls only its opaque linker alias after the matching DMA.
#[no_mangle]
#[inline(never)]
#[link_section = ".phase.victory.entry"]
pub unsafe extern "C" fn medal_geometry(game: *const (), renderer: *mut Renderer, bank: *const ()) {
    let game = &*game.cast::<Game>();
    let renderer = &mut *renderer;
    renderer.fade_scene(game.victory_ticks());
    game.render_medal(renderer, &*bank.cast::<AssetBank<'_>>());
}
/// Sprite images persist independently of the world. Reuse retired geometry
/// workspace to stage one corner, then publish it outside its VIC fetch window.
#[link_section = ".phase.pixels"]
#[inline(never)]
unsafe fn update_sprite_hud(status: &Hud) {
    let next = hud_sprites::DisplayState::from_hud(
        status,
        FRAME_SERIAL,
        if FPS_SHOWN { Some(FPS_TENTHS) } else { None },
    );
    let dirty = next.changed_corners(if HUD_VALID {
        Some(HUD_STATE.assume_init_ref())
    } else {
        None
    });
    for corner in 0..4u8 {
        if dirty & (1 << corner) != 0 {
            let images = &mut *(0x8e00 as *mut [u8; 128]);
            hud_sprites::render_corner(corner, &next, images);
            poke(0x8e80, next.accent(corner));
            publish_hud_corner(corner);
        }
    }
    // Byte publication is atomic; the line-251 IRQ latches the desired mask.
    // The first mask waits until present, after the splash has been covered.
    let enable = next.enabled_mask();
    HUD_ENABLE_STAGED = enable;
    if !FIRST_FRAME_PENDING {
        wr(core::ptr::addr_of_mut!(HUD_ENABLE_PENDING), enable);
    }
    HUD_STATE.write(next);
    HUD_VALID = true;
}

#[link_section = ".phase.pixels"]
#[inline(never)]
unsafe fn draw_pixels() {
    // GEOMETRY may have overwritten the tail. The caller has hidden I/O for
    // raster access, so expose the REU only while loading the auxiliary image;
    // the frame-rate overlay scans the keyboard in the same exposure, and the
    // VIC background takes the frame's clear colour (the cell writer's code 0).
    io_on();
    dma(
        ((core::ptr::addr_of!(__phase_pixels_aux_reu_bank) as u8 as u32) << 16)
            | (core::ptr::addr_of!(__phase_pixels_aux_reu_offset) as u32),
        0x6f20,
        core::ptr::addr_of!(__phase_pixels_aux_size) as u16,
    );
    // Simulation is complete and no intervening render phase changes its
    // visible status. Build the readout only after its PIXELS code is loaded.
    update_fps_overlay(GAME.assume_init_ref().mode());
    let status = GAME.assume_init_ref().hud();
    if RETICLE_FRAME_VISIBLE == 0 || RETICLE_RETIRE_OLD || PAUSED {
        // Retire old aiming before new hull pixels or captions are published. Position
        // changes still wait for present; only hiding can take effect early.
        reticle_hide();
    }
    update_sprite_hud(&status);
    let renderer = RENDERER.assume_init_mut();
    poke(0xd021, renderer.clear_color());
    io_off();
    let ambient = SPACE_DUST.prepare(
        status.section,
        status.mode,
        GAME.assume_init_ref().travel_ticks(),
    );
    // Band zero belongs to the GUI. Consume its edges without painting the
    // world so triangles crossing row eight continue at the correct positions.
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 1);
    let Some(mut pass) = renderer.raster_pass() else {
        fail(b"RASTER COMMANDS ALREADY CONSUMED")
    };
    for band in 0..25u8 {
        if band != 0 {
            wr(core::ptr::addr_of_mut!(PROF_DETAIL), 1);
        }
        let pixels = &mut *(0x8e00 as *mut [u8; 1280]);
        let offset = band as u16 * 40;
        if pass.next_band_with_ambient(pixels, band != 0, ambient) != Some(band) {
            fail(b"INVALID RASTER BAND ORDER")
        }
        wr(core::ptr::addr_of_mut!(PROF_DETAIL), 2);
        let mut cells = havoc::cells::NativeCells;
        hud::draw(band, &mut cells, &status, FRAME_SERIAL);
        if PAUSED {
            hud::draw_paused(band, &mut cells);
        }
        wr(core::ptr::addr_of_mut!(PROF_DETAIL), 3);
        // Quantize completely into consumed band workspace before touching
        // the display. Only overflow cells need the preceding live attributes.
        // Keep I/O mapped for color RAM and the publisher's raster reads.
        io_on();
        cells.stage(
            &*((0x8000 + offset) as *const [u8; 40]),
            &*((0xd800 + offset) as *const [u8; 40]),
        );
        wr(core::ptr::addr_of_mut!(PROF_DETAIL), 4);
        publish_staged_band(band);
        io_off();
    }
    wr(core::ptr::addr_of_mut!(PROF_DETAIL), 0);
    wr(core::ptr::addr_of_mut!(PROF_PHASE), 4);
    present();
}

// The source is the PRG's future IRQ-stack gap; copy before initialize_irq
// fills it with guards. The destination remains resident until shutdown.
#[cfg(feature = "benchmarks")]
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_benchmark_helpers() {
    extern "C" {
        static __benchmark_load: u8;
        static __benchmark_start: u8;
        static __benchmark_size: u8;
    }
    core::ptr::copy_nonoverlapping(
        core::ptr::addr_of!(__benchmark_load),
        core::ptr::addr_of!(__benchmark_start) as *mut u8,
        core::ptr::addr_of!(__benchmark_size) as usize,
    );
}

#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn boot_init() {
    #[cfg(feature = "benchmarks")]
    initialize_benchmark_helpers();
    let standard = detect_standard();
    initialize_video();
    initialize_quarter_squares();
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
    initialize_audio();
    load_initial_assets();
    initialize_hud();
    CLOCK_KHZ = if standard == VideoStandard::Pal {
        985
    } else {
        1023
    };
    initialize_music_boot();
    initialize_irq(standard);
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_hud() {
    // The common asset bank temporarily owns $8400-$8FFF during validation.
    // Initialize these bytes only after load_initial_assets has consumed it.
    core::ptr::write_bytes(0x8c00 as *mut u8, 0, 512);
    core::ptr::copy_nonoverlapping(
        havoc::reticle::IMAGE.as_ptr(),
        core::ptr::addr_of!(__reticle_sprite_start) as *mut u8,
        64,
    );
    for sprite in 0..8u16 {
        poke(0x83f8 + sprite, 0x30 + sprite as u8);
    }
    poke(0xd015, 0);
    poke(0xd01b, 0);
    poke(0xd01c, 0xff);
    poke(0xd017, 0);
    poke(0xd01d, 0);
    poke(0xd025, 12);
    poke(0xd026, 1);
    hud_apply_scroll(3 << 3);
}
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_game() {
    GAME.write(Game::new());
}
/// Quarter squares Q[n] = floor(n^2 / 4) for n in 0..512, low bytes at $C000
/// and high bytes at $C200, for the exact Q8 multiply kernel. Q[n+1] - Q[n] =
/// floor((n + 1) / 2), so the table accumulates without a multiply. This RAM
/// is otherwise unused, and the IRQ never touches it.
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_quarter_squares() {
    let low = QUARTER_SQUARES as *mut u8;
    let high = (QUARTER_SQUARES + 512) as *mut u8;
    let mut square: u16 = 0;
    for n in 0..512u16 {
        low.add(n as usize).write_volatile(square as u8);
        high.add(n as usize).write_volatile((square >> 8) as u8);
        square = square.wrapping_add((n + 1) >> 1);
    }
}
const QUARTER_SQUARES: usize = 0xc000;
#[link_section = ".phase.boot"]
#[inline(never)]
unsafe fn initialize_audio() {
    // The shared slot stays uninitialized while BACKEND_SWITCHING gates IRQ
    // access. AUDIO initializes the selected backend after validating assets.
    for sid in 0..havoc::audio::SID_COUNT as u16 {
        poke(0xd418 + sid * 32, 0);
    }
}
struct Runtime {
    sim_ms: u16,
    // Foreground-only history; benchmark fixture restoration includes it.
    transition_audio: TransitionAudio,
    loaded: [u8; 2],
    current: u8,
    profile: u8,
    was_paused: bool,
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn load_segment(slot: u8, index: u8) {
    dma(
        assets::SEGMENTS_OFFSET + index as u32 * 2048,
        segment_address(slot),
        512,
    );
    validate_segment(slot, index);
    STREAM_BYTES += 512;
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn select_stage(runtime: &mut Runtime, game: &mut Game) {
    let need = game.segment_index();
    if runtime.loaded[0] == need {
        runtime.current = 0;
    } else if runtime.loaded[1] == need {
        runtime.current = 1;
    } else {
        load_segment(runtime.current, need);
        runtime.loaded[runtime.current as usize] = need;
    }
    game.bind_stage(&Segment::from_validated(segment_data(runtime.current)));
    runtime.profile = game.simulation_profile();
}
#[link_section = ".phase.sim"]
#[inline(never)]
unsafe fn simulate(runtime: &mut Runtime, now: u16) -> Option<()> {
    let game = GAME.assume_init_mut();
    select_stage(runtime, game);
    if runtime.profile != ENEMY_CODE_PROFILE {
        return None;
    }
    let need = game.segment_index();
    if ENEMY_MODEL_PROFILE != need {
        copy_model_bank(need);
        let bytes = core::slice::from_raw_parts(0x8400 as *const u8, assets::MODEL_SIZE);
        if assets::checksum(bytes) != MODEL_CHECKSUMS[need as usize] {
            fail(b"INVALID LEVEL MODEL BANK")
        }
        ENEMY_MODEL_PROFILE = need;
        MODEL_STREAM_BYTES += assets::MODEL_SIZE as u16;
    }
    let bank = AssetBank::from_validated(core::slice::from_raw_parts(
        0x8400 as *const u8,
        assets::MODEL_SIZE,
    ));
    let desired = if matches!(
        game.mode(),
        havoc::game::Mode::GameOver | havoc::game::Mode::Victory
    ) {
        0
    } else {
        if need + 1 < assets::SEGMENT_COUNT {
            need + 1
        } else {
            0
        }
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
    // Synchronize held buttons and their clock without queuing gestures.
    // The first resumed frame also discards samples from the final paused
    // render and rebases the deadline, so neither inputs nor missed ticks
    // replay when play resumes.
    if PAUSED || runtime.was_paused {
        disable_irq();
        INPUT_TAIL = INPUT_HEAD;
        let held = LAST_BITS;
        enable_irq();
        runtime.sim_ms = now;
        game.sample_paused_input(held, now);
        runtime.was_paused = PAUSED;
    }
    while now.wrapping_sub(runtime.sim_ms) >= 20 {
        let step_ms = runtime.sim_ms.wrapping_add(20);
        if bench_ticks == 255 {
            while let Some(input) = take_input(step_ms) {
                game.sample_input(input)
            }
        } else {
            game.sample_input(InputSample {
                bits: TEST_INPUT & INPUT_MASK,
                pressed: 0,
                ms: step_ms,
            });
        }
        // Waking attract mode changes the required tail before this tick.
        // Preserve the sampled gesture and deadline across the overlay reload.
        runtime.profile = game.simulation_profile();
        if runtime.profile != ENEMY_CODE_PROFILE {
            return None;
        }
        runtime.sim_ms = step_ms;
        game.tick(
            &Segment::from_validated(segment_data(runtime.current)),
            &bank,
        );
        if game.segment_index() != need || game.simulation_profile() != ENEMY_CODE_PROFILE {
            // All page borrows above end before selecting the incoming metadata.
            select_stage(runtime, game);
            return None;
        }
    }
    // Present the latest event after catch-up so a bomb cannot expire before
    // the display sees it. The IRQ owns only the resident effect countdown.
    let presentation = game.take_presentation_event();
    disable_irq();
    match presentation {
        PresentationEvent::None => {}
        PresentationEvent::Bomb => BOMB_EFFECT.trigger(),
        PresentationEvent::Hit => BOMB_EFFECT.trigger_hit(),
        PresentationEvent::Reset => BOMB_EFFECT.reset(),
    }
    enable_irq();
    send_audio(game);
    GAME_MODE = game.mode() as u8;
    // The campaign distance for the monitor tools: section × 30,000 as a
    // 16-bit product of 1,875 shifted by four, plus the section-local part.
    let section = game.segment_index();
    GAME_DISTANCE = (((section as u16 * 1_875) as u32) << 4) + game.local_distance() as u32;
    GAME_STAGE = section;
    Some(())
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
    let mut runtime = Runtime {
        sim_ms: 0,
        transition_audio: TransitionAudio::new(),
        loaded: [0xff; 2],
        current: 0,
        profile: 0,
        was_paused: false,
    };
    #[cfg(feature = "benchmarks")]
    {
        BENCH_RUNTIME = core::ptr::addr_of_mut!(runtime) as u16;
    }
    loop {
        #[cfg(feature = "benchmarks")]
        benchmark_frame_begin();
        #[cfg(not(feature = "benchmarks"))]
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 1);
        STREAM_BYTES = 0;
        MODEL_STREAM_BYTES = 0;
        load_sim(runtime.profile);
        // Freeze one deadline for every same-frame reentry. A section-changing
        // tick returns before using the incoming AI; reload and finish catch-up
        // without adding simulation time or delaying the new level a frame.
        let now = clock_ms();
        #[cfg(feature = "benchmarks")]
        let now = {
            let ticks = rd(core::ptr::addr_of!(BENCH_TICKS));
            if ticks != 255 {
                benchmark_now(runtime.sim_ms, ticks)
            } else {
                now
            }
        };
        // Paused frames keep rendering (the caption and the readout live in
        // PIXELS, which only toggles the flag while flying); `simulate`
        // discards the queued joystick samples and moves the simulation
        // deadline along so resuming does not replay the missed ticks.
        loop {
            wr(core::ptr::addr_of_mut!(PROF_PHASE), 5);
            if simulate(&mut runtime, now).is_some() {
                break;
            }
            wr(core::ptr::addr_of_mut!(PROF_PHASE), 1);
            load_sim(runtime.profile);
        };
        let mode = GAME.assume_init_ref().mode();
        disable_irq();
        let music_pending = !AUDIO_READY || rd(core::ptr::addr_of!(EMULATOR_SFX_ONLY)) == 0;
        enable_irq();
        if music_pending {
            wr(core::ptr::addr_of_mut!(PROF_PHASE), 8);
            // The fully verified boot directory makes the validation tail
            // unnecessary for later page refills.
            load_phase(
                5,
                if AUDIO_READY {
                    core::ptr::addr_of!(__phase_audio_runtime_size) as u16
                } else {
                    core::ptr::addr_of!(__phase_audio_size) as u16
                },
            );
            wr(core::ptr::addr_of_mut!(PROF_PHASE), 9);
            service_audio();
        }
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 10);
        // Simulation's mode is unchanged through AUDIO and both render phases.
        // Space environments use the opaque scenery tail on ordinary frames.
        // Crossings clear to shared stars before the midpoint tail change.
        let info = GAME.assume_init_ref().stage_info();
        let crossing = GAME.assume_init_ref().transition_progress();
        let scenery_page = if info.theme >= 10 {
            let theme = if crossing.map_or(false, |(_, tick)| tick >= 200) {
                info.next_theme
            } else {
                info.theme
            };
            0x19 + theme - 10
        } else {
            match info.bridge_profile {
                1 => 0x15,
                2 => 0x16,
                3 if crossing.is_some() => 0x18,
                _ => 7,
            }
        };
        let scenery_size = if scenery_page >= 0x18 {
            core::ptr::addr_of!(__phase_scenery_size) as u16
        } else {
            core::ptr::addr_of!(__phase_scenery_terrain_size) as u16
        };
        load_phase(scenery_page, scenery_size);
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 11);
        io_off();
        let scenery_count = prepare_scenery(runtime.current, &mut runtime.transition_audio);
        io_on();
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 2);
        // Bridges keep the common player/projectile renderer and replace enemy
        // presentation with SCENERY wildlife. SIM remains a separate overlay.
        let geometry_size = if mode == havoc::game::Mode::Victory {
            core::ptr::addr_of!(__phase_victory_size) as u16
        } else if mode == havoc::game::Mode::Transition {
            core::ptr::addr_of!(__phase_geometry_base_size) as u16
        } else {
            core::ptr::addr_of!(__phase_geometry_prefix_size) as u16
        };
        load_phase(
            if mode == havoc::game::Mode::Victory {
                12
            } else {
                3
            },
            geometry_size,
        );
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 6);
        io_off();
        {
            let bank = AssetBank::from_validated(core::slice::from_raw_parts(
                0x8400 as *const u8,
                assets::MODEL_SIZE,
            ));
            prepare_geometry(scenery_count, &bank);
        }
        io_on();
        let shadows_enabled = GAME.assume_init_ref().shadow_ground_y().is_some();
        load_phase(
            31,
            if shadows_enabled {
                core::ptr::addr_of!(__phase_shadow_size) as u16
            } else {
                core::ptr::addr_of!(__phase_shadow_finalize_size) as u16
            },
        );
        io_off();
        prepare_shadows(shadows_enabled);
        io_on();
        load_phase(4, core::ptr::addr_of!(__phase_pixels_prefix_size) as u16);
        wr(core::ptr::addr_of_mut!(PROF_PHASE), 7);
        io_off();
        draw_pixels();
        if peek(0xc400) != 0xa5 || peek(0x7500) != 0xa5 {
            fail(b"STACK GUARD FAILED")
        }
    }
}

#[panic_handler]
#[cfg_attr(feature = "benchmarks", link_section = ".benchmark_resident")]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { fail(b"H.A.V.O.C. PANIC - RESET TO EXIT") }
}
