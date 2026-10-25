/* C64 Ultimate PRG with selected REU code overlays and eight SIM tails. The PRG
 * initializes resident code, startup checks and high constants in one LOAD.
 * Overlay load addresses are REU package offsets, not C64 RAM addresses.
 */
ENTRY(_start)
EXTERN(main)

__rc0 = 0x02;
INCLUDE imag-regs.ld

MEMORY {
    /* $F0-$FD is the cell writer's zero page (target-c64/asm/cells.S). */
    zp (rw) : ORIGIN = 0x0022, LENGTH = 0x00CE
    /* $0600-$07FF is the first mission buffer; resident code may reach $2FFF. */
    low (rw) : ORIGIN = 0x0200, LENGTH = 0x0400
    ram (rwx) : ORIGIN = 0x0801, LENGTH = 0x27FF
    phase (rwx) : ORIGIN = 0x3000, LENGTH = 0x4200
    startup_checks (rx) : ORIGIN = 0x8E00, LENGTH = 0x0500
    sim_aux (rx) : ORIGIN = 0x8E00, LENGTH = 0x0500
    sim_extra (rx) : ORIGIN = 0xE000, LENGTH = 0x1800
    scenery_extra (rx) : ORIGIN = 0xE000, LENGTH = 0x0800
    constants (r) : ORIGIN = 0x7800, LENGTH = 0x0800
    tables (r) : ORIGIN = 0x9900, LENGTH = 0x0700
    /* Empty regions materialize zero-filled holes in the contiguous PRG. */
    image_gap (r) : ORIGIN = 0x3000, LENGTH = 0x4200
    sfx_patches (r) : ORIGIN = 0x7200, LENGTH = 0x0300
    image_sfx_gap (r) : ORIGIN = 0x7500, LENGTH = 0x0300
    image_midgap (r) : ORIGIN = 0x8000, LENGTH = 0x0E00
    image_postgap (r) : ORIGIN = 0x9300, LENGTH = 0x0600
    renderer (rw) : ORIGIN = 0xCFFA, LENGTH = 0x1006
    game (rw) : ORIGIN = 0xC800, LENGTH = 0x0300
    audio (rw) : ORIGIN = 0xCE00, LENGTH = 0x01B0
    /* The effects state sits in the page below the DAC state. */
    sfx_state (rw) : ORIGIN = 0xCD00, LENGTH = 0x0100
    benchmark_ram (rx) : ORIGIN = 0xFF20, LENGTH = 0x00DA
}

INPUT(basic-header.o)
REGION_ALIAS("c_readonly", ram)
REGION_ALIAS("c_writeable", low)

/* Package v8 keeps the original logical phase IDs while packing code into
 * 4 KiB-aligned slots. No slot crosses a 64 KiB bank. Bank 4 remains PIXELS-only
 * so its split-transfer audit can distinguish prefix/helper DMA by bank. */
__reu_size = 0x1000000;
/* The DMC1 manifest directory and its contiguous DLT4 assets share this region. */
__reu_digital_music = 0x100000;
/* Legacy symbol now aliases the manifest region; assets have generated offsets. */
__reu_digital_victory = __reu_digital_music;
__reu_digital_ring = 0xFE0000;
__reu_digital_ring_size = 0x20000;
__reu_code_slot_size = 0x4200;
__reu_models = 0x0B0000;
__reu_boot = 0x010000;
__reu_sim_lowlands = 0x020000;
__reu_geometry = 0x030000;
__reu_pixels = 0x040000;
__reu_audio = 0x050000;
__reu_scenery_wildlife = 0x070000;
__reu_sim_canyon = 0x080000;
__reu_sim_fortress = 0x090000;
__reu_sim_carrier = 0x0A0000;
__reu_victory = 0x015000;
__reu_sim_terminal = 0x01A000;
__reu_sim_warden = 0x025000;
__reu_sim_citadel = 0x02A000;
__reu_sim_flagship = 0x035000;
__reu_scenery_transport = 0x03A000;
__reu_scenery_meteors = 0x055000;
__reu_sim_helpers = 0x05A000;
__reu_scenery_ascent = 0x060000;
__reu_scenery_asteroid = 0x065000;
__reu_scenery_moon = 0x06A000;
__reu_scenery_graveyard = 0x075000;
__reu_scenery_shipyard = 0x07A000;
__reu_scenery_carrier = 0x085000;
__reu_scenery_gate = 0x08A000;
__reu_shadow = 0x095000;
__reu_scenery_helpers = 0x0AA000;

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

    .phase.boot 0x3000 : AT(__reu_boot) {
        __phase_boot_start = .;
        *(.phase.boot .phase.boot.*)
        __phase_boot_end = .;
    } >phase
    .phase.sim 0x3000 : AT(__reu_sim_lowlands) {
        __phase_sim_start = .;
        *(.phase.sim .phase.sim.*)
        __phase_sim_base_end = .;
    } >phase
    /* Each composed SIM image has the same prefix and one opaque ABI entry.
     * Keep the bodies invisible to the caller's LTO optimization. */
    encounter_step = __phase_sim_base_end;
    terminal_step = __phase_sim_base_end;
    .phase.terminal terminal_step : AT(__reu_sim_terminal + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.terminal.entry))
        *(.phase.terminal .phase.terminal.*)
    } >phase
    .phase.enemy.lowlands encounter_step : AT(__reu_sim_lowlands + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.lowlands.entry))
        *(.phase.enemy.lowlands .phase.enemy.lowlands.*)
    } >phase
    .phase.enemy.canyon encounter_step : AT(__reu_sim_canyon + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.canyon.entry))
        *(.phase.enemy.canyon .phase.enemy.canyon.*)
    } >phase
    .phase.enemy.fortress encounter_step : AT(__reu_sim_fortress + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.fortress.entry))
        *(.phase.enemy.fortress .phase.enemy.fortress.*)
    } >phase
    .phase.enemy.carrier encounter_step : AT(__reu_sim_carrier + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.carrier.entry))
        *(.phase.enemy.carrier .phase.enemy.carrier.*)
    } >phase
    .phase.enemy.warden encounter_step : AT(__reu_sim_warden + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.warden.entry))
        *(.phase.enemy.warden .phase.enemy.warden.*)
    } >phase
    .phase.enemy.citadel encounter_step : AT(__reu_sim_citadel + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.citadel.entry))
        *(.phase.enemy.citadel .phase.enemy.citadel.*)
    } >phase
    .phase.enemy.flagship encounter_step : AT(__reu_sim_flagship + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.flagship.entry))
        *(.phase.enemy.flagship .phase.enemy.flagship.*)
    } >phase
    .phase.geometry 0x3000 : AT(__reu_geometry) {
        __phase_geometry_start = .;
        *(.phase.geometry .phase.geometry.data)
        __phase_geometry_base_end = .;
        /* Bridges omit enemy meshes but retain the player and projectiles. */
        *(.phase.geometry.combat .phase.geometry.combat.*)
        __phase_geometry_end = .;
    } >phase
    victory_geometry = __phase_geometry_base_end;
    .phase.victory victory_geometry : AT(__reu_victory + __phase_geometry_base_end - 0x3000) {
        KEEP(*(.phase.victory.entry))
        *(.phase.victory .phase.victory.*)
    } >phase
    /* Projection survives the GEOMETRY -> SHADOW prefix replacement. PIXELS
     * reloads its own helper into this same tail only after preparation. */
    .phase.render_aux 0x6F20 : AT(__reu_geometry + 0x3F20) {
        __phase_render_aux_start = .;
        *(.phase.render_aux .phase.render_aux.*)
        __phase_render_aux_end = .;
    } >phase
    .phase.shadow 0x3000 : AT(__reu_shadow) {
        __phase_shadow_start = .;
        KEEP(*(.phase.shadow.entry))
        *(.phase.shadow.final .phase.shadow.final.*)
        __phase_shadow_finalize_end = .;
        *(.phase.shadow .phase.shadow.*)
        /* The enabled-only camera dispatch belongs to the full shadow tail. */
        *(.rodata.*setup_shadows*)
        __phase_shadow_end = .;
    } >phase
    .phase.scenery_aux 0x8E00 : AT(__reu_scenery_helpers) {
        __phase_scenery_aux_start = .;
        *(.phase.scenery_aux .phase.scenery_aux.*)
        __phase_scenery_aux_end = .;
    } >sim_aux
    .phase.scenery_extra 0xE000 : AT(__reu_scenery_helpers + 0x500) {
        __phase_scenery_extra_start = .;
        *(.phase.scenery_extra .phase.scenery_extra.*)
        __phase_scenery_extra_end = .;
    } >scenery_extra
    .phase.pixels 0x3000 : AT(__reu_pixels) {
        __phase_pixels_start = .;
        *(.phase.pixels .phase.pixels.*)
        __phase_pixels_end = .;
    } >phase
    /* Prepared raster tails and masks live in the free RAM at $E000, so PIXELS
     * code may grow to $6F1F. The helper tail is always reloaded before
     * rasterization, after GEOMETRY relinquishes this window. */
    .phase.pixels_aux 0x6F20 : AT(__reu_pixels + 0x3F20) {
        __phase_pixels_aux_start = .;
        *(.phase.pixels_aux .phase.pixels_aux.*)
        __phase_pixels_aux_end = .;
    } >phase
    .phase.audio 0x3000 : AT(__reu_audio) {
        __phase_audio_start = .;
        *(.phase.audio .phase.audio.data .phase.audio.crc_table .phase.audio.kernel)
        /* Startup validation may be in BOOT, leaving the whole image live. */
        __phase_audio_runtime_end = .;
        *(.phase.audio.startup .phase.audio.startup.*)
        __phase_audio_end = .;
    } >phase
    /* Scenery images share their prefix; the score bank begins at $0C0000. */
    .phase.scenery 0x3000 : AT(__reu_scenery_wildlife) {
        __phase_scenery_start = .;
        *(.phase.scenery .phase.scenery.data)
        /* Ordinary terrestrial frames omit space-only helpers and data. The
         * opaque decoration ABI remains after the complete shared prefix. */
        __phase_scenery_terrain_end = .;
        *(.phase.scenery.space_shared .phase.scenery.space_shared.*)
        __phase_scenery_base_end = .;
    } >phase
    scenic_decoration = __phase_scenery_base_end;
    .phase.scenery.wildlife scenic_decoration : AT(__reu_scenery_wildlife + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.wildlife.entry))
        *(.phase.scenery.wildlife .phase.scenery.wildlife.*)
    } >phase
    .phase.scenery.transport scenic_decoration : AT(__reu_scenery_transport + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.transport.entry))
        *(.phase.scenery.transport .phase.scenery.transport.*)
    } >phase
    .phase.scenery.meteors scenic_decoration : AT(__reu_scenery_meteors + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.meteors.entry))
        *(.phase.scenery.meteors .phase.scenery.meteors.*)
    } >phase
    .phase.scenery.ascent scenic_decoration : AT(__reu_scenery_ascent + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.ascent.entry))
        *(.phase.scenery.ascent .phase.scenery.ascent.*)
    } >phase
    .phase.scenery.asteroid scenic_decoration : AT(__reu_scenery_asteroid + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.asteroid.entry))
        *(.phase.scenery.asteroid .phase.scenery.asteroid.*)
    } >phase
    .phase.scenery.moon scenic_decoration : AT(__reu_scenery_moon + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.moon.entry))
        *(.phase.scenery.moon .phase.scenery.moon.*)
    } >phase
    .phase.scenery.graveyard scenic_decoration : AT(__reu_scenery_graveyard + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.graveyard.entry))
        *(.phase.scenery.graveyard .phase.scenery.graveyard.*)
    } >phase
    .phase.scenery.shipyard scenic_decoration : AT(__reu_scenery_shipyard + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.shipyard.entry))
        *(.phase.scenery.shipyard .phase.scenery.shipyard.*)
    } >phase
    .phase.scenery.carrier scenic_decoration : AT(__reu_scenery_carrier + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.carrier.entry))
        *(.phase.scenery.carrier .phase.scenery.carrier.*)
    } >phase
    .phase.scenery.gate scenic_decoration : AT(__reu_scenery_gate + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.gate.entry))
        *(.phase.scenery.gate .phase.scenery.gate.*)
    } >phase
    /* Shared SIM helpers reuse the retired startup / renderer workspace.
     * load_sim restores them before every SIM entry. No IRQ references. */
    .phase.sim_aux 0x8E00 : AT(__reu_sim_helpers) {
        __phase_sim_aux_start = .;
        *(.phase.sim_aux .phase.sim_aux.*)
        __phase_sim_aux_end = .;
    } >sim_aux
    /* SIM runs before SCENERY resets its shadow frame. Previous raster tails,
     * masks and shadow records are retired here; every SIM entry reloads this
     * code before use, and it becomes scratch again when SIM returns. */
    .phase.sim_extra 0xE000 : AT(__reu_sim_helpers + 0x500) {
        __phase_sim_extra_start = .;
        *(.phase.sim_extra .phase.sim_extra.*)
        __phase_sim_extra_end = .;
    } >sim_extra
    /* Called exactly once from main, before BOOT loads common assets over
     * this range. Never reachable from an interrupt or a later overlay. */
    .startup_checks : {
        __startup_checks_start = .;
        *(.startup_checks .startup_checks.*)
        __code_sizes = .;
        SHORT(__phase_boot_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_geometry_size)
        SHORT(__phase_pixels_size)
        SHORT(__phase_audio_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_victory_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_sim_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_sim_aux_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_scenery_size)
        SHORT(__phase_shadow_size)
        SHORT(__phase_scenery_aux_size)
        __startup_checks_end = .;
    } >startup_checks

    INCLUDE text.ld
    .tables : {
        /* Runtime loads only the active SIM tail. Startup validates the full
         * padded package image, including every byte omitted at runtime. */
        __sim_load_sizes = .;
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.lowlands))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.canyon))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.fortress))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.carrier))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.warden))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.citadel))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.enemy.flagship))
        SHORT(SIZEOF(.phase.sim) + SIZEOF(.phase.terminal))
        /* Logical phase IDs retain their original values. Each byte stores
         * the 4 KiB page of its physical REU slot; unused IDs remain zero. */
        __code_reu_pages = .;
        BYTE(0)
        BYTE(__reu_boot >> 12)
        BYTE(__reu_sim_lowlands >> 12)
        BYTE(__reu_geometry >> 12)
        BYTE(__reu_pixels >> 12)
        BYTE(__reu_audio >> 12)
        BYTE(0)
        BYTE(__reu_scenery_wildlife >> 12)
        BYTE(__reu_sim_canyon >> 12)
        BYTE(__reu_sim_fortress >> 12)
        BYTE(__reu_sim_carrier >> 12)
        BYTE(0)
        BYTE(__reu_victory >> 12)
        BYTE(__reu_sim_terminal >> 12)
        BYTE(0) BYTE(0) BYTE(0) BYTE(0)
        BYTE(__reu_sim_warden >> 12)
        BYTE(__reu_sim_citadel >> 12)
        BYTE(__reu_sim_flagship >> 12)
        BYTE(__reu_scenery_transport >> 12)
        BYTE(__reu_scenery_meteors >> 12)
        BYTE(__reu_sim_helpers >> 12)
        BYTE(__reu_scenery_ascent >> 12)
        BYTE(__reu_scenery_asteroid >> 12)
        BYTE(__reu_scenery_moon >> 12)
        BYTE(__reu_scenery_graveyard >> 12)
        BYTE(__reu_scenery_shipyard >> 12)
        BYTE(__reu_scenery_carrier >> 12)
        BYTE(__reu_scenery_gate >> 12)
        BYTE(__reu_shadow >> 12)
        BYTE(0) /* retired shadow-draw logical ID 32 */
        BYTE(__reu_scenery_helpers >> 12)
        __code_reu_pages_end = .;
        *(.tables .tables.*)
        *(.rodata.*PROJECT_X* .rodata.*PROJECT_Y* .rodata.*COLOR_DISTANCE* .rodata.*SIN*)
    } >tables
    .sfx_patches : {
        __sfx_patches_start = .;
        KEEP(*(.sfx_patches .sfx_patches.*))
        __sfx_patches_end = .;
    } >sfx_patches
    .rodata : { INCLUDE rodata-sections.ld } >constants
    .sfx_helpers : { *(.sfx_helpers .sfx_helpers.*) } >constants
    /* Benchmark-only resident helpers load in the unused PRG stack gap. BOOT
     * copies them before initializing the IRQ stack at the source address. */
    .benchmark_resident 0xFF20 : AT(0x7500) {
        __benchmark_start = .;
        *(.benchmark_resident .benchmark_resident.*)
        __benchmark_end = .;
    } >benchmark_ram
    __benchmark_load = LOADADDR(.benchmark_resident);
    __benchmark_size = SIZEOF(.benchmark_resident);
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
    .sfx_state (NOLOAD) : {
        __sfx_state_start = .;
        KEEP(*(.sfx_state .sfx_state.*))
        __sfx_state_end = .;
    } >sfx_state
}

/* Independent software stacks grow downward. The bounded effects engine uses
 * resident static frames; keep the full 256-byte IRQ stack and the immutable
 * synthesis bank at $7200-$74FF in separate lifetimes/allocations. */
__stack = 0xC800;
__stack_bottom = 0xC400;
__irq_stack = 0x7600;
__irq_stack_bottom = 0x7500;
/* LLVM-MOS gives non-reentrant functions static local frames in .noinit.
 * Nothing reachable from irq_body may own one (tools/check-irq-frames.py
 * audits the linked closure), so the interrupt needs no frame backup and
 * $BF40-$BFBF plus $CB00-$CCFF are free; Game uses the first 128 bytes of
 * the former $CA80-$CCFF backup for the four-boss state and byte locks. */
/* Immutable reticle image in the unused bitmap tail. */
__reticle_sprite_start = 0xBFC0;
__reticle_sprite_end = 0xC000;
__resident_end = MAX(LOADADDR(.data) + SIZEOF(.data), ADDR(.text) + SIZEOF(.text));
/* The first mission buffer uses the retired default screen page below the
 * PRG. The second stays above the cell workspace; neither is used by an
 * interrupt. Resident code may therefore fill $0801-$2FFF. */
__segment0_start = 0x0600;
__segment0_end = 0x0800;
__segment1_start = 0x9700;
__segment1_end = 0x9900;
/* Eight 64-byte images are permanently owned by the VIC after BOOT's common
 * asset validation retires. Models stop at $8BFF; shared scratch starts $8E00. */
__hud_sprites_start = 0x8C00;
__hud_sprites_end = 0x8E00;
__phase_run = 0x3000;
__phase_boot_size = SIZEOF(.phase.boot);
__phase_sim_base_size = SIZEOF(.phase.sim);
__phase_sim_aux_prefix_size = SIZEOF(.phase.sim_aux);
__phase_sim_extra_size = SIZEOF(.phase.sim_extra);
__phase_sim_aux_size = 0x500 + __phase_sim_extra_size;
__phase_enemy_size = MAX(MAX(MAX(MAX(MAX(MAX(MAX(SIZEOF(.phase.terminal), SIZEOF(.phase.enemy.lowlands)), SIZEOF(.phase.enemy.canyon)), SIZEOF(.phase.enemy.fortress)), SIZEOF(.phase.enemy.carrier)), SIZEOF(.phase.enemy.warden)), SIZEOF(.phase.enemy.citadel)), SIZEOF(.phase.enemy.flagship));
__phase_sim_size = __phase_sim_base_size + __phase_enemy_size;
__phase_sim_end = __phase_sim_start + __phase_sim_size;
__phase_geometry_prefix_size = SIZEOF(.phase.geometry);
__phase_render_aux_size = SIZEOF(.phase.render_aux);
__phase_render_aux_load = LOADADDR(.phase.render_aux);
__phase_geometry_size = __phase_render_aux_end - __phase_geometry_start;
__phase_geometry_base_size = __phase_geometry_base_end - __phase_geometry_start;
__phase_victory_size = __phase_geometry_base_size + SIZEOF(.phase.victory);
__phase_shadow_size = SIZEOF(.phase.shadow);
__phase_shadow_finalize_size = __phase_shadow_finalize_end - __phase_shadow_start;
__phase_scenery_aux_prefix_size = SIZEOF(.phase.scenery_aux);
__phase_scenery_extra_size = SIZEOF(.phase.scenery_extra);
__phase_scenery_aux_size = 0x500 + __phase_scenery_extra_size;
__phase_pixels_prefix_size = SIZEOF(.phase.pixels);
__phase_pixels_aux_size = SIZEOF(.phase.pixels_aux);
__phase_pixels_aux_load = LOADADDR(.phase.pixels_aux);
/* Startup checks the complete image, including its zero-filled raster gap.
 * Runtime DMA transfers only the prefix and auxiliary tail. */
__phase_pixels_size = __phase_pixels_aux_end - __phase_pixels_start;
__phase_audio_size = SIZEOF(.phase.audio);
__phase_audio_runtime_size = __phase_audio_runtime_end - __phase_audio_start;
__phase_scenery_earth_tail_size = MAX(SIZEOF(.phase.scenery.wildlife), MAX(SIZEOF(.phase.scenery.transport), SIZEOF(.phase.scenery.meteors)));
__phase_scenery_space_tail_size = MAX(MAX(MAX(SIZEOF(.phase.scenery.ascent), SIZEOF(.phase.scenery.asteroid)), MAX(SIZEOF(.phase.scenery.moon), SIZEOF(.phase.scenery.graveyard))), MAX(MAX(SIZEOF(.phase.scenery.shipyard), SIZEOF(.phase.scenery.carrier)), SIZEOF(.phase.scenery.gate)));
__phase_scenery_size = SIZEOF(.phase.scenery) + MAX(__phase_scenery_earth_tail_size, __phase_scenery_space_tail_size);
__phase_scenery_end = __phase_scenery_start + __phase_scenery_size;
__phase_scenery_base_size = __phase_scenery_base_end - __phase_scenery_start;
__phase_scenery_terrain_size = __phase_scenery_terrain_end - __phase_scenery_start;
/* SCENERY stages 96 camera-space records in the command array's tail.
 * GEOMETRY copies each record out before projection reuses consumed bytes. */
__scenery_records_start = 0xD880;
__scenery_records_end = 0xE000;
/* GEOMETRY resets its cache before use. PIXELS then fills this same workspace
 * before rasterization; neither the IRQ nor resident model data uses it. */
__pixel_band_start = 0x8E00;
__pixel_band_end = 0x9300;
/* Once a band's raster/HUD work ends, its coverage and cell flags are dead.
 * Staging consumes them; the next clear must invalidate COVER_CLEAN. */
__staged_bitmap_start = 0x9540;
__staged_bitmap_end = 0x9680;
__staged_screen_start = 0x7700;
__staged_screen_end = 0x7728;
__staged_color_start = 0x7728;
__staged_color_end = 0x7750;
__projection_cache_start = __pixel_band_start;
__projection_cache_end = __projection_cache_start + 0x0280;
/* The depth sort leaves the commands in emission order and writes the sorted
 * order (command index per sorted position) here; PIXELS prepares the edge
 * heads through it into the free tail below the CPU vectors, so nothing is
 * permuted in place. BOOT's directory reads of this page precede any frame. */
__sort_order = 0x7600;
__raster_heads_start = 0xEF20;
__raster_heads_end = 0xFF20;
__raster_tails_start = 0xE000;
__raster_tails_end = 0xEC00;
__raster_masks_start = 0xEC00;
__raster_masks_end = 0xEF20;
/* Commands retire after every edge is prepared; PIXELS then owns these bytes. */
__span_buffer_start = 0xD000;
__span_buffer_end = 0xDA00;
__span_buffer_size = __span_buffer_end - __span_buffer_start;
/* Flat shadow descriptors retire before raster preparation reuses this RAM. */
__shadow_frame_start = 0xE800;
__shadow_frame_end = 0xFF20;
ASSERT(__span_buffer_size == 8 * 160 * 2, "incorrect span compositor allocation")
ASSERT(__span_buffer_start == __renderer_start + 6 && __span_buffer_end <= __renderer_end, "span compositor must reuse retired commands only")
/* Front-to-back row heads borrow preparation/packer scratch only while painting. */
__span_front_heads_start = 0x77AC;
__span_front_heads_end = 0x77B4;
ASSERT(__span_front_heads_end - __span_front_heads_start == 8, "incorrect front span row-head allocation")
/* GEOMETRY retires this scratch before cell clear; preserve it until staging. */
__cell_hud_seen = 0x9328;
/* Preparation is complete before the row walker or packer uses this scratch.
 * The IRQ never borrows it; persistent models at $8400 are untouched. */
__raster_prepare_scratch_start = 0x77A0;
__raster_prepare_scratch_end = 0x77BF;
/* PIXELS resets this byte per band; preparation and packing own it before/after. */
__raster_player_seen = 0x77AB;
ASSERT(__span_front_heads_start > __raster_player_seen && __span_front_heads_end <= __raster_prepare_scratch_end, "front span heads must reuse retired raster scratch")
/* BOOT builds the quarter-square table for the resident Q8 multiply here. */
__quarter_squares_start = 0xC000;
__quarter_squares_end = 0xC400;

/* The byte address table and bounded loader rely on these physical slots. */
ASSERT(__reu_size == 0x1000000 && __reu_code_slot_size == 0x4200, "incorrect 16 MiB REU allocation")
ASSERT(__reu_digital_music == 0x100000 && __reu_digital_victory == __reu_digital_music && __reu_digital_ring == 0xFE0000 &&
       __reu_digital_ring_size == 0x20000 && __reu_digital_ring + __reu_digital_ring_size == __reu_size,
       "incorrect digital music or sampler ring allocation")
ASSERT(__code_reu_pages_end - __code_reu_pages == 34, "incorrect logical REU page table size")
ASSERT((__reu_boot & 0xFFF) == 0 && (__reu_boot & 0xFFFF) + __reu_code_slot_size <= 0x10000, "boot REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_lowlands & 0xFFF) == 0 && (__reu_sim_lowlands & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_lowlands REU slot must be aligned and stay in one bank")
ASSERT((__reu_geometry & 0xFFF) == 0 && (__reu_geometry & 0xFFFF) + __reu_code_slot_size <= 0x10000, "geometry REU slot must be aligned and stay in one bank")
ASSERT((__reu_pixels & 0xFFF) == 0 && (__reu_pixels & 0xFFFF) + __reu_code_slot_size <= 0x10000, "pixels REU slot must be aligned and stay in one bank")
ASSERT((__reu_audio & 0xFFF) == 0 && (__reu_audio & 0xFFFF) + __reu_code_slot_size <= 0x10000, "audio REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_wildlife & 0xFFF) == 0 && (__reu_scenery_wildlife & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_wildlife REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_canyon & 0xFFF) == 0 && (__reu_sim_canyon & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_canyon REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_fortress & 0xFFF) == 0 && (__reu_sim_fortress & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_fortress REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_carrier & 0xFFF) == 0 && (__reu_sim_carrier & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_carrier REU slot must be aligned and stay in one bank")
ASSERT((__reu_victory & 0xFFF) == 0 && (__reu_victory & 0xFFFF) + __reu_code_slot_size <= 0x10000, "victory REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_terminal & 0xFFF) == 0 && (__reu_sim_terminal & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_terminal REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_warden & 0xFFF) == 0 && (__reu_sim_warden & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_warden REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_citadel & 0xFFF) == 0 && (__reu_sim_citadel & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_citadel REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_flagship & 0xFFF) == 0 && (__reu_sim_flagship & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_flagship REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_transport & 0xFFF) == 0 && (__reu_scenery_transport & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_transport REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_meteors & 0xFFF) == 0 && (__reu_scenery_meteors & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_meteors REU slot must be aligned and stay in one bank")
ASSERT((__reu_sim_helpers & 0xFFF) == 0 && (__reu_sim_helpers & 0xFFFF) + __reu_code_slot_size <= 0x10000, "sim_helpers REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_ascent & 0xFFF) == 0 && (__reu_scenery_ascent & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_ascent REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_asteroid & 0xFFF) == 0 && (__reu_scenery_asteroid & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_asteroid REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_moon & 0xFFF) == 0 && (__reu_scenery_moon & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_moon REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_graveyard & 0xFFF) == 0 && (__reu_scenery_graveyard & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_graveyard REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_shipyard & 0xFFF) == 0 && (__reu_scenery_shipyard & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_shipyard REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_carrier & 0xFFF) == 0 && (__reu_scenery_carrier & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_carrier REU slot must be aligned and stay in one bank")
ASSERT((__reu_scenery_gate & 0xFFF) == 0 && (__reu_scenery_gate & 0xFFFF) + __reu_code_slot_size <= 0x10000, "scenery_gate REU slot must be aligned and stay in one bank")
ASSERT((__reu_shadow & 0xFFF) == 0 && (__reu_shadow & 0xFFFF) + __reu_code_slot_size <= 0x10000, "shadow REU slot must be aligned and stay in one bank")
ASSERT(0xB000 <= __reu_boot &&
       __reu_boot + __reu_code_slot_size <= __reu_victory &&
       __reu_victory + __reu_code_slot_size <= __reu_sim_terminal &&
       __reu_sim_terminal + __reu_code_slot_size <= __reu_sim_lowlands &&
       __reu_sim_lowlands + __reu_code_slot_size <= __reu_sim_warden &&
       __reu_sim_warden + __reu_code_slot_size <= __reu_sim_citadel &&
       __reu_sim_citadel + __reu_code_slot_size <= __reu_geometry &&
       __reu_geometry + __reu_code_slot_size <= __reu_sim_flagship &&
       __reu_sim_flagship + __reu_code_slot_size <= __reu_scenery_transport &&
       __reu_scenery_transport + __reu_code_slot_size <= __reu_pixels &&
       __reu_pixels + __reu_code_slot_size <= __reu_audio &&
       __reu_audio + __reu_code_slot_size <= __reu_scenery_meteors &&
       __reu_scenery_meteors + __reu_code_slot_size <= __reu_sim_helpers &&
       __reu_sim_helpers + __reu_code_slot_size <= __reu_scenery_ascent &&
       __reu_scenery_ascent + __reu_code_slot_size <= __reu_scenery_asteroid &&
       __reu_scenery_asteroid + __reu_code_slot_size <= __reu_scenery_moon &&
       __reu_scenery_moon + __reu_code_slot_size <= __reu_scenery_wildlife &&
       __reu_scenery_wildlife + __reu_code_slot_size <= __reu_scenery_graveyard &&
       __reu_scenery_graveyard + __reu_code_slot_size <= __reu_scenery_shipyard &&
       __reu_scenery_shipyard + __reu_code_slot_size <= __reu_sim_canyon &&
       __reu_sim_canyon + __reu_code_slot_size <= __reu_scenery_carrier &&
       __reu_scenery_carrier + __reu_code_slot_size <= __reu_scenery_gate &&
       __reu_scenery_gate + __reu_code_slot_size <= __reu_sim_fortress &&
       __reu_sim_fortress + __reu_code_slot_size <= __reu_shadow &&
       __reu_shadow + __reu_code_slot_size <= __reu_sim_carrier &&
       __reu_sim_carrier + __reu_code_slot_size <= __reu_models &&
       __reu_models + 0x9000 <= __reu_digital_music,
       "REU code, asset and music reservations overlap or exceed the image")
ASSERT(__reu_pixels == 0x40000 && __reu_audio >= 0x50000, "REU bank 4 must remain PIXELS-only")

ASSERT(__rc31 == 0x21, "IRQ save range must match compiler imaginary registers")
ASSERT(__resident_end <= __phase_run, "resident code overlaps the overlay window at $3000")
ASSERT(__segment0_start == 0x0600 && __segment0_end == 0x0800 && __segment0_end - __segment0_start == 512, "incorrect first mission buffer allocation")
ASSERT(ADDR(.noinit) + SIZEOF(.noinit) <= __segment0_start, "low globals overlap the first mission buffer at $0600")
ASSERT(__segment1_start == 0x9700 && __segment1_end == ORIGIN(tables) && __segment1_end - __segment1_start == 512, "incorrect second mission buffer allocation")
ASSERT(__hud_sprites_start == 0x8C00 && __hud_sprites_end == 0x8E00 && __hud_sprites_end - __hud_sprites_start == 8 * 64, "incorrect HUD sprite allocation")
ASSERT((__hud_sprites_start & 63) == 0 && __hud_sprites_end <= 0x9000, "HUD sprites must be aligned in VIC-visible RAM")
ASSERT(__reticle_sprite_start == 0xBFC0 && __reticle_sprite_end == 0xC000 && __reticle_sprite_end - __reticle_sprite_start == 64 && (__reticle_sprite_start & 63) == 0, "incorrect resident reticle allocation")
ASSERT(__hud_sprites_end == __pixel_band_start && ORIGIN(startup_checks) == __pixel_band_start && ORIGIN(sim_aux) == __pixel_band_start, "shared workspace overlaps HUD sprites")
ASSERT(__pixel_band_start == 0x8E00 && __pixel_band_end == 0x9300, "incorrect shared pixel-band workspace")
ASSERT(__cell_hud_seen == 0x9328, "HUD-seen flag must reuse cell scratch at $9328")
ASSERT(ADDR(.sfx_state) == 0xCD00 && SIZEOF(.sfx_state) <= 0x0100, "SID effects state must occupy $CD00-$CDFF")
ASSERT(SIZEOF(.renderer) <= 0x1006, "renderer exceeds header plus hidden-I/O triangle storage")
ASSERT(SIZEOF(.game_state) <= 0x0300, "game state exceeds $C800-$CAFF")
ASSERT(SIZEOF(.unexpected_static_stack) == 0, "compiler static locals outside .noinit are not IRQ-safe")
ASSERT(SIZEOF(.audio_state) <= 0x01B0, "audio state exceeds $CE00-$CFAF")
ASSERT(SIZEOF(.rodata) <= 0x0800, "resident constants exceed $7800-$7FFF")
ASSERT(SIZEOF(.rodata) + SIZEOF(.sfx_helpers) <= 0x0800, "resident constants and SFX helpers exceed constant RAM")
ASSERT(SIZEOF(.tables) <= 0x0700, "resident tables exceed $9900-$9FFF")
ASSERT(SIZEOF(.phase.boot) <= 0x4200, "BOOT overlay exceeds $3000-$71FF")
ASSERT(__phase_sim_size <= 0x4200, "composed SIM overlay exceeds $3000-$71FF")
ASSERT(__phase_sim_aux_prefix_size > 0 && __phase_sim_aux_prefix_size <= 0x0500, "SIM helpers exceed retired startup / renderer workspace")
ASSERT(__phase_sim_extra_size > 0 && __phase_sim_extra_size <= 0x1800, "SIM extra helpers exceed retired raster / shadow workspace")
ASSERT(encounter_lowlands == encounter_step && encounter_canyon == encounter_step && encounter_fortress == encounter_step && encounter_carrier == encounter_step && encounter_warden == encounter_step && encounter_citadel == encounter_step && encounter_flagship == encounter_step, "encounter ABI entries must start every SIM tail")
ASSERT(SIZEOF(.phase.enemy.lowlands) > 0 && SIZEOF(.phase.enemy.canyon) > 0 && SIZEOF(.phase.enemy.fortress) > 0 && SIZEOF(.phase.enemy.carrier) > 0, "all four enemy tails are required")
ASSERT(__phase_geometry_prefix_size <= 0x3F20, "GEOMETRY overlaps shared projection helpers at $6F20")
ASSERT(__phase_geometry_size <= 0x4200, "composed GEOMETRY image exceeds phase slot")
ASSERT(__phase_victory_size <= 0x3F20, "victory GEOMETRY overlaps shared projection helpers")
ASSERT(__phase_geometry_base_size > 0 && __phase_geometry_base_size < __phase_geometry_prefix_size, "GEOMETRY needs a common prefix and a combat tail")
ASSERT(__phase_render_aux_start == 0x6F20 && __phase_render_aux_size > 0 && __phase_render_aux_size <= 0x2E0, "shared projection helpers exceed $6F20-$71FF")
ASSERT(__phase_shadow_size > 0 && __phase_shadow_size <= 0x3F20, "SHADOW overlaps shared projection helpers at $6F20")
ASSERT(__phase_scenery_aux_prefix_size > 0 && __phase_scenery_aux_prefix_size <= 0x500, "SCENERY helpers exceed retired pixel workspace")
ASSERT(__phase_scenery_extra_size > 0 && __phase_scenery_extra_size <= 0x800, "SCENERY extra helpers overlap persistent shadow frame")
ASSERT((__reu_scenery_helpers & 0xFFF) == 0 && (__reu_scenery_helpers & 0xFFFF) + __reu_code_slot_size <= 0x10000, "SCENERY helper REU slot crosses bank")
ASSERT(__reu_sim_carrier + __reu_code_slot_size <= __reu_scenery_helpers && __reu_scenery_helpers + __reu_code_slot_size <= __reu_models, "SCENERY helper REU slot overlaps carrier or models")
ASSERT(SIZEOF(.phase.pixels) <= 0x3F20, "PIXELS overlay overlaps its helpers at $6F20")
ASSERT(__phase_pixels_aux_start == 0x6F20, "PIXELS helpers must start at $6F20")
ASSERT(__phase_pixels_aux_size > 0 && __phase_pixels_aux_size <= 0x02E0, "PIXELS helpers exceed $6F20-$71FF")
ASSERT(__phase_pixels_aux_end <= __irq_stack_bottom, "PIXELS helpers overlap IRQ stack")
ASSERT(__phase_pixels_size <= 0x4200, "composed PIXELS image exceeds phase slot")
ASSERT(SIZEOF(.phase.audio) <= 0x4200, "AUDIO overlay exceeds $3000-$71FF")
ASSERT(__phase_audio_runtime_size > 0 && __phase_audio_runtime_size <= __phase_audio_size, "AUDIO runtime extent must fit its image")
ASSERT(__phase_scenery_size <= 0x4200, "composed SCENERY overlay exceeds $3000-$71FF")
ASSERT(__phase_scenery_base_size > 0 && __phase_scenery_base_size < __phase_scenery_size, "SCENERY needs a common prefix and a wildlife tail")
ASSERT(__phase_scenery_terrain_size > 0 && __phase_scenery_terrain_size < __phase_scenery_base_size, "SCENERY needs a terrestrial prefix before space helpers")
ASSERT(__scenery_records_end - __scenery_records_start == 96 * 20, "incorrect scenery staging allocation")
ASSERT(__scenery_records_start >= __renderer_start && __scenery_records_end <= __renderer_end, "scenery staging exceeds command array")
ASSERT(SIZEOF(.startup_checks) > 0 && SIZEOF(.startup_checks) <= 0x0500, "startup checks exceed retired pixel-band workspace")
ASSERT(__projection_cache_start >= __pixel_band_start && __projection_cache_end <= __pixel_band_end, "projection cache exceeds shared pixel-band workspace")
ASSERT(__projection_cache_start + 0x0400 <= __pixel_band_end, "depth-sort scratch exceeds the pixel-band workspace")
ASSERT(__sort_order == 0x7600 && __sort_order + 256 <= 0x7700, "sorted order must fill the page below the cell tables")
ASSERT(__staged_bitmap_start == 0x9540 && __staged_bitmap_end == 0x9680 && __staged_bitmap_end - __staged_bitmap_start == 320, "staged bitmap must reuse cell coverage only")
ASSERT(__staged_bitmap_end + 3 * 40 <= __segment1_start, "cell palettes overlap the second mission buffer")
ASSERT(__staged_screen_start == __sort_order + 256 && __staged_screen_end == __staged_color_start && __staged_screen_end - __staged_screen_start == 40, "staged screen must reuse HUD slot flags")
ASSERT(__staged_color_end == 0x7750 && __staged_color_end - __staged_color_start == 40 && __staged_color_end < __raster_prepare_scratch_start, "staged colors must reuse overflow flags without the clean marker or packer scratch")
ASSERT(__raster_heads_end - __raster_heads_start == 256 * 16, "incorrect raster head allocation")
ASSERT(__raster_tails_end - __raster_tails_start == 256 * 12, "incorrect raster tail allocation")
ASSERT(__raster_masks_start >= __raster_tails_end && __raster_masks_end - __raster_masks_start == 25 * 32, "incorrect ordered band mask allocation")
ASSERT(__raster_heads_start >= __raster_masks_end && __raster_heads_end <= 0xFFFA, "raster heads must follow the masks in the free RAM below the CPU vectors")
ASSERT(__raster_tails_start >= 0xE000 && __raster_masks_end <= 0xFFFA, "prepared raster data must stay in the free RAM below the CPU vectors")
ASSERT(__raster_prepare_scratch_start >= __irq_stack && __raster_prepare_scratch_end <= ORIGIN(constants), "preparation scratch overlaps IRQ stack or constants")
ASSERT(__raster_prepare_scratch_end - __raster_prepare_scratch_start == 31, "incorrect native preparation scratch allocation")
ASSERT(__raster_player_seen == 0x77AB && __raster_player_seen >= __raster_prepare_scratch_start && __raster_player_seen < __raster_prepare_scratch_end, "player coverage state must stay in sequential raster scratch")
ASSERT(__quarter_squares_end - __quarter_squares_start == 1024 && __quarter_squares_end <= __stack_bottom, "quarter-square table must fill the free KiB below the main stack")

OUTPUT_FORMAT {
    SHORT(ORIGIN(ram))
    FULL(ram)
    FULL(image_gap)
    FULL(sfx_patches)
    FULL(image_sfx_gap)
    FULL(constants)
    FULL(image_midgap)
    FULL(startup_checks)
    FULL(image_postgap)
    FULL(tables)
}
ASSERT(__phase_scenery_extra_end <= __shadow_frame_start && __shadow_frame_end <= __raster_heads_end, "SCENERY helper or shadow scratch overlaps live frame");

ASSERT(SIZEOF(.sfx_patches) > 0 && SIZEOF(.sfx_patches) <= 0x300, "SFX patches exceed permanent bank")
ASSERT(__sfx_patches_end <= __irq_stack_bottom, "SFX patches overlap IRQ stack")

ASSERT(SIZEOF(.benchmark_resident) <= 0xDA, "benchmark helpers overlap CPU vectors")
ASSERT(LOADADDR(.benchmark_resident) == 0x7500 && LOADADDR(.benchmark_resident) + SIZEOF(.benchmark_resident) <= 0x7600, "benchmark helper source exceeds retired boot stack gap")
