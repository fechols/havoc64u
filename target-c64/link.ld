/* C64 Ultimate PRG with selected REU code overlays and four SIM tails. The PRG
 * initializes resident code, startup checks and high constants in one LOAD.
 * Overlay load addresses are REU package offsets, not C64 RAM addresses.
 */
ENTRY(_start)
EXTERN(main)

__rc0 = 0x02;
INCLUDE imag-regs.ld

MEMORY {
    zp (rw) : ORIGIN = 0x0022, LENGTH = 0x00DE
    low (rw) : ORIGIN = 0x0200, LENGTH = 0x0600
    ram (rwx) : ORIGIN = 0x0801, LENGTH = 0x27FF
    phase (rwx) : ORIGIN = 0x3000, LENGTH = 0x4200
    startup_checks (rx) : ORIGIN = 0x8C00, LENGTH = 0x0500
    constants (r) : ORIGIN = 0x7800, LENGTH = 0x0800
    tables (r) : ORIGIN = 0x9900, LENGTH = 0x0700
    /* Empty regions materialize zero-filled holes in the contiguous PRG. */
    image_gap (r) : ORIGIN = 0x3000, LENGTH = 0x4800
    image_midgap (r) : ORIGIN = 0x8000, LENGTH = 0x0C00
    image_postgap (r) : ORIGIN = 0x9100, LENGTH = 0x0800
    renderer (rw) : ORIGIN = 0xCFFA, LENGTH = 0x1006
    game (rw) : ORIGIN = 0xC800, LENGTH = 0x0280
    audio (rw) : ORIGIN = 0xCE00, LENGTH = 0x01B0
    music_page (rw) : ORIGIN = 0x0720, LENGTH = 0x00E0
}

INPUT(basic-header.o)
REGION_ALIAS("c_readonly", ram)
REGION_ALIAS("c_writeable", low)

SECTIONS {
    /* The stock C64 .init.250 case-mode hook calls KERNAL CHROUT. Our BSS
     * owns the KERNAL vector RAM, so no ROM calls are legal after clearing
     * it. Reserve .250 for discarding this hook; our data copy uses .251. */
    /DISCARD/ : { *(.init.250) }
    .basic_header : { KEEP(*(.basic_header)) } >ram
    .unexpected_static_stack (NOLOAD) : {
        *(.data..Lstatic_stack .bss..Lstatic_stack)
    } >low
    INCLUDE zp.ld

    .phase.boot 0x3000 : AT(0x10000) {
        __phase_boot_start = .;
        *(.phase.boot .phase.boot.*)
        __phase_boot_end = .;
    } >phase
    .phase.sim 0x3000 : AT(0x20000) {
        __phase_sim_start = .;
        *(.phase.sim .phase.sim.*)
        __phase_sim_base_end = .;
    } >phase
    /* Each composed SIM image has the same prefix and one opaque ABI entry.
     * Keep the bodies invisible to the caller's LTO optimization. */
    enemy_step = __phase_sim_base_end;
    .phase.enemy.lowlands enemy_step : AT(0x20000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.lowlands.entry))
        *(.phase.enemy.lowlands .phase.enemy.lowlands.*)
    } >phase
    .phase.enemy.canyon enemy_step : AT(0x80000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.canyon.entry))
        *(.phase.enemy.canyon .phase.enemy.canyon.*)
    } >phase
    .phase.enemy.fortress enemy_step : AT(0x90000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.fortress.entry))
        *(.phase.enemy.fortress .phase.enemy.fortress.*)
    } >phase
    .phase.enemy.carrier enemy_step : AT(0xA0000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.carrier.entry))
        *(.phase.enemy.carrier .phase.enemy.carrier.*)
    } >phase
    .phase.geometry 0x3000 : AT(0x30000) {
        __phase_geometry_start = .;
        *(.phase.geometry .phase.geometry.data)
        __phase_geometry_base_end = .;
        /* Bridges omit enemy meshes but retain the player and projectiles. */
        *(.phase.geometry.combat .phase.geometry.combat.*)
        __phase_geometry_end = .;
    } >phase
    .phase.pixels 0x3000 : AT(0x40000) {
        __phase_pixels_start = .;
        *(.phase.pixels .phase.pixels.*)
        __phase_pixels_end = .;
    } >phase
    .phase.audio 0x3000 : AT(0x50000) {
        __phase_audio_start = .;
        *(.phase.audio .phase.audio.*)
        __phase_audio_end = .;
    } >phase
    /* $060000 remains the soundtrack; scenery has its own REU page. */
    .phase.scenery 0x3000 : AT(0x70000) {
        __phase_scenery_start = .;
        *(.phase.scenery .phase.scenery.data)
        __phase_scenery_base_end = .;
        /* Only Transition reaches wildlife. Ordinary frames omit this tail. */
        *(.phase.scenery.wildlife .phase.scenery.wildlife.*)
        __phase_scenery_end = .;
    } >phase
    /* Called exactly once from main, before BOOT loads common assets over
     * this range. Never reachable from an interrupt or a later overlay. */
    .startup_checks : {
        __startup_checks_start = .;
        *(.startup_checks .startup_checks.*)
        __startup_checks_end = .;
    } >startup_checks

    INCLUDE text.ld
    .tables : {
        *(.tables .tables.*)
        *(.rodata.*RECIPROCAL* .rodata.*COLOR_DISTANCE* .rodata.*SIN*)
    } >tables
    .rodata : { INCLUDE rodata-sections.ld } >constants
    INCLUDE data.ld
    INCLUDE bss.ld
    INCLUDE noinit.ld

    .renderer (NOLOAD) : {
        __renderer_start = .;
        KEEP(*(.renderer .renderer.*))
        __renderer_end = .;
    } >renderer
    .game_state (NOLOAD) : {
        __game_state_start = .;
        KEEP(*(.game_state .game_state.*))
        __game_state_end = .;
    } >game
    .audio_state (NOLOAD) : {
        __audio_state_start = .;
        KEEP(*(.audio_state .audio_state.*))
        __audio_state_end = .;
    } >audio
    .music_page (NOLOAD) : {
        __music_page_start = .;
        KEEP(*(.music_page .music_page.*))
        __music_page_end = .;
    } >music_page
}

/* Independent software stacks; both grow toward lower addresses. */
__stack = 0xC800;
__stack_bottom = 0xC400;
__irq_stack = 0x7600;
__irq_stack_bottom = 0x7200;
/* LLVM-MOS can give arithmetic helpers static local frames, even when called
 * from both main and an assembly IRQ. Preserve these across every IRQ. */
__irq_static_backup = 0xCA80;
__irq_static_backup_tail = 0xBF40;
__irq_static_save_start = ADDR(.noinit);
__irq_static_save_size = SIZEOF(.noinit);
__irq_static_first_size = MIN(SIZEOF(.noinit), 0x380);
__irq_static_tail_size = MAX(SIZEOF(.noinit), 0x380) - 0x380;
__irq_static_tail_start = ADDR(.noinit) + __irq_static_first_size;
__resident_end = MAX(LOADADDR(.data) + SIZEOF(.data), ADDR(.text) + SIZEOF(.text));
__phase_run = 0x3000;
__phase_boot_size = SIZEOF(.phase.boot);
__phase_sim_base_size = SIZEOF(.phase.sim);
__phase_enemy_size = MAX(MAX(SIZEOF(.phase.enemy.lowlands), SIZEOF(.phase.enemy.canyon)), MAX(SIZEOF(.phase.enemy.fortress), SIZEOF(.phase.enemy.carrier)));
__phase_sim_size = __phase_sim_base_size + __phase_enemy_size;
__phase_sim_end = __phase_sim_start + __phase_sim_size;
__phase_geometry_size = SIZEOF(.phase.geometry);
__phase_geometry_base_size = __phase_geometry_base_end - __phase_geometry_start;
__phase_pixels_size = SIZEOF(.phase.pixels);
__phase_audio_size = SIZEOF(.phase.audio);
__phase_scenery_size = SIZEOF(.phase.scenery);
__phase_scenery_base_size = __phase_scenery_base_end - __phase_scenery_start;
/* SCENERY stages 96 camera-space records in the command array's tail.
 * GEOMETRY copies each record out before projection reuses consumed bytes. */
__scenery_records_start = 0xD880;
__scenery_records_end = 0xE000;
/* GEOMETRY resets its cache before use. PIXELS then fills this same workspace
 * before rasterization; neither the IRQ nor resident model data uses it. */
__pixel_band_start = 0x8C00;
__pixel_band_end = 0x9100;
__projection_cache_start = __pixel_band_start;
__projection_cache_end = __projection_cache_start + 0x0280;
/* PIXELS consumes the sorted 16-byte commands into persistent edge heads.
 * Twelve-byte tails and ordered band masks replace the old slope workspace. */
__raster_heads_start = 0xD000;
__raster_heads_end = 0xE000;
__raster_tails_start = 0x6000;
__raster_tails_end = 0x6C00;
__raster_masks_start = 0x6C00;
__raster_masks_end = 0x6F20;

ASSERT(__rc31 == 0x21, "IRQ save range must match compiler imaginary registers")
ASSERT(__resident_end <= 0x3000, "resident code exceeds $0801-$2FFF")
ASSERT(ADDR(.noinit) + SIZEOF(.noinit) <= (SIZEOF(.music_page) > 0 ? 0x0720 : 0x0800), "low globals overlap audio score page")
ASSERT(SIZEOF(.music_page) == 0 || SIZEOF(.music_page) == 0x00E0, "four-SID score page must occupy $0720-$07FF")
ASSERT(SIZEOF(.renderer) <= 0x1006, "renderer exceeds header plus hidden-I/O triangle storage")
ASSERT(SIZEOF(.game_state) <= 0x0280, "game state overlaps IRQ static-frame backup")
ASSERT(SIZEOF(.noinit) <= 0x0440, "IRQ static-frame backup exceeds 1088 bytes")
ASSERT(SIZEOF(.unexpected_static_stack) == 0, "compiler static locals outside .noinit are not IRQ-safe")
ASSERT(SIZEOF(.audio_state) <= 0x01B0, "audio state exceeds $CE00-$CFAF")
ASSERT(SIZEOF(.rodata) <= 0x0800, "resident constants exceed $7800-$7FFF")
ASSERT(SIZEOF(.tables) <= 0x0700, "resident tables exceed $9900-$9FFF")
ASSERT(SIZEOF(.phase.boot) <= 0x4200, "BOOT overlay exceeds $3000-$71FF")
ASSERT(__phase_sim_size <= 0x4200, "composed SIM overlay exceeds $3000-$71FF")
ASSERT(enemy_lowlands == enemy_step && enemy_canyon == enemy_step && enemy_fortress == enemy_step && enemy_carrier == enemy_step, "enemy ABI entries must start every SIM tail")
ASSERT(SIZEOF(.phase.enemy.lowlands) > 0 && SIZEOF(.phase.enemy.canyon) > 0 && SIZEOF(.phase.enemy.fortress) > 0 && SIZEOF(.phase.enemy.carrier) > 0, "all four enemy tails are required")
ASSERT(SIZEOF(.phase.geometry) <= 0x4200, "GEOMETRY overlay exceeds $3000-$71FF")
ASSERT(__phase_geometry_base_size > 0 && __phase_geometry_base_size < __phase_geometry_size, "GEOMETRY needs a common prefix and a combat tail")
ASSERT(SIZEOF(.phase.pixels) <= 0x3000, "PIXELS overlay overlaps prepared edges at $6000")
ASSERT(SIZEOF(.phase.audio) <= 0x4200, "AUDIO overlay exceeds $3000-$71FF")
ASSERT(SIZEOF(.phase.scenery) <= 0x4200, "SCENERY overlay exceeds $3000-$71FF")
ASSERT(__phase_scenery_base_size > 0 && __phase_scenery_base_size < __phase_scenery_size, "SCENERY needs a common prefix and a wildlife tail")
ASSERT(__scenery_records_end - __scenery_records_start == 96 * 20, "incorrect scenery staging allocation")
ASSERT(__scenery_records_start >= __raster_heads_start && __scenery_records_end <= __raster_heads_end, "scenery staging exceeds command array")
ASSERT(SIZEOF(.startup_checks) > 0 && SIZEOF(.startup_checks) <= 0x0500, "startup checks exceed retired pixel-band workspace")
ASSERT(__projection_cache_start >= __pixel_band_start && __projection_cache_end <= __pixel_band_end, "projection cache exceeds shared pixel-band workspace")
ASSERT(__projection_cache_start + 624 <= __projection_cache_end, "depth-sort scratch exceeds retired projection cache")
ASSERT(__raster_heads_end - __raster_heads_start == 256 * 16, "incorrect raster head allocation")
ASSERT(__raster_tails_end - __raster_tails_start == 256 * 12, "incorrect raster tail allocation")
ASSERT(__raster_masks_start >= __raster_tails_end && __raster_masks_end - __raster_masks_start == 25 * 32, "incorrect ordered band mask allocation")
ASSERT(__raster_masks_end <= __irq_stack_bottom, "prepared raster data overlaps IRQ stack")

OUTPUT_FORMAT {
    SHORT(ORIGIN(ram))
    FULL(ram)
    FULL(image_gap)
    FULL(constants)
    FULL(image_midgap)
    FULL(startup_checks)
    FULL(image_postgap)
    FULL(tables)
}
