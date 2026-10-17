/* C64 Ultimate PRG with four explicitly selected REU code overlays. The PRG
 * initializes resident code and both high constant pools in one ordinary LOAD.
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
    constants (r) : ORIGIN = 0x7800, LENGTH = 0x0800
    tables (r) : ORIGIN = 0x9900, LENGTH = 0x0700
    /* Empty regions materialize zero-filled holes in the contiguous PRG. */
    image_gap (r) : ORIGIN = 0x3000, LENGTH = 0x4800
    image_midgap (r) : ORIGIN = 0x8000, LENGTH = 0x1900
    renderer (rw) : ORIGIN = 0xCFFA, LENGTH = 0x1006
    game (rw) : ORIGIN = 0xC800, LENGTH = 0x0280
    audio (rw) : ORIGIN = 0xCE00, LENGTH = 0x01B0
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
        __phase_sim_end = .;
    } >phase
    .phase.geometry 0x3000 : AT(0x30000) {
        __phase_geometry_start = .;
        *(.phase.geometry .phase.geometry.*)
        __phase_geometry_end = .;
    } >phase
    .phase.pixels 0x3000 : AT(0x40000) {
        __phase_pixels_start = .;
        *(.phase.pixels .phase.pixels.*)
        __phase_pixels_end = .;
    } >phase

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
__phase_sim_size = SIZEOF(.phase.sim);
__phase_geometry_size = SIZEOF(.phase.geometry);
__phase_pixels_size = SIZEOF(.phase.pixels);
/* GEOMETRY resets its cache before use. PIXELS then fills this same workspace
 * before rasterization; neither the IRQ nor resident model data uses it. */
__pixel_band_start = 0x8C00;
__pixel_band_end = 0x9100;
__projection_cache_start = __pixel_band_start;
__projection_cache_end = __projection_cache_start + 0x0280;

ASSERT(__rc31 == 0x21, "IRQ save range must match compiler imaginary registers")
ASSERT(__resident_end <= 0x3000, "resident code exceeds $0801-$2FFF")
ASSERT(ADDR(.noinit) + SIZEOF(.noinit) <= 0x0800, "low globals exceed $0200-$07FF")
ASSERT(SIZEOF(.renderer) <= 0x1006, "renderer exceeds header plus hidden-I/O triangle storage")
ASSERT(SIZEOF(.game_state) <= 0x0280, "game state overlaps IRQ static-frame backup")
ASSERT(SIZEOF(.noinit) <= 0x0440, "IRQ static-frame backup exceeds 1088 bytes")
ASSERT(SIZEOF(.unexpected_static_stack) == 0, "compiler static locals outside .noinit are not IRQ-safe")
ASSERT(SIZEOF(.audio_state) <= 0x01B0, "audio state exceeds $CE00-$CFAF")
ASSERT(SIZEOF(.rodata) <= 0x0800, "resident constants exceed $7800-$7FFF")
ASSERT(SIZEOF(.tables) <= 0x0700, "resident tables exceed $9900-$9FFF")
ASSERT(SIZEOF(.phase.boot) <= 0x4200, "BOOT overlay exceeds $3000-$71FF")
ASSERT(SIZEOF(.phase.sim) <= 0x4200, "SIM overlay exceeds $3000-$71FF")
ASSERT(SIZEOF(.phase.geometry) <= 0x4200, "GEOMETRY overlay exceeds $3000-$71FF")
ASSERT(SIZEOF(.phase.pixels) <= 0x3000, "PIXELS overlay overlaps $6000-$6BFF slope cache")
ASSERT(__projection_cache_start >= __pixel_band_start && __projection_cache_end <= __pixel_band_end, "projection cache exceeds shared pixel-band workspace")

OUTPUT_FORMAT {
    SHORT(ORIGIN(ram))
    FULL(ram)
    FULL(image_gap)
    FULL(constants)
    FULL(image_midgap)
    FULL(tables)
}
