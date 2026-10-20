/* C64 Ultimate PRG with selected REU code overlays and eight SIM tails. The PRG
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
    sim_aux (rx) : ORIGIN = 0x8C00, LENGTH = 0x0500
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
    encounter_step = __phase_sim_base_end;
    terminal_step = __phase_sim_base_end;
    .phase.terminal terminal_step : AT(0xD0000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.terminal.entry))
        *(.phase.terminal .phase.terminal.*)
    } >phase
    .phase.enemy.lowlands encounter_step : AT(0x20000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.lowlands.entry))
        *(.phase.enemy.lowlands .phase.enemy.lowlands.*)
    } >phase
    .phase.enemy.canyon encounter_step : AT(0x80000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.canyon.entry))
        *(.phase.enemy.canyon .phase.enemy.canyon.*)
    } >phase
    .phase.enemy.fortress encounter_step : AT(0x90000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.fortress.entry))
        *(.phase.enemy.fortress .phase.enemy.fortress.*)
    } >phase
    .phase.enemy.carrier encounter_step : AT(0xA0000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.carrier.entry))
        *(.phase.enemy.carrier .phase.enemy.carrier.*)
    } >phase
    .phase.enemy.warden encounter_step : AT(0x120000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.warden.entry))
        *(.phase.enemy.warden .phase.enemy.warden.*)
    } >phase
    .phase.enemy.citadel encounter_step : AT(0x130000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.citadel.entry))
        *(.phase.enemy.citadel .phase.enemy.citadel.*)
    } >phase
    .phase.enemy.flagship encounter_step : AT(0x140000 + SIZEOF(.phase.sim)) {
        KEEP(*(.phase.enemy.flagship.entry))
        *(.phase.enemy.flagship .phase.enemy.flagship.*)
    } >phase
    .phase.geometry 0x3000 : AT(0x30000) {
        __phase_geometry_start = .;
        *(.phase.geometry .phase.geometry.data)
        __phase_geometry_base_end = .;
        /* Bridges omit enemy meshes but retain the player and projectiles. */
        *(.phase.geometry.combat .phase.geometry.combat.*)
        __phase_geometry_end = .;
    } >phase
    victory_geometry = __phase_geometry_base_end;
    .phase.victory victory_geometry : AT(0xC0000 + __phase_geometry_base_end - 0x3000) {
        KEEP(*(.phase.victory.entry))
        *(.phase.victory .phase.victory.*)
    } >phase
    .phase.pixels 0x3000 : AT(0x40000) {
        __phase_pixels_start = .;
        *(.phase.pixels .phase.pixels.*)
        __phase_pixels_end = .;
    } >phase
    .phase.audio 0x3000 : AT(0x50000) {
        __phase_audio_start = .;
        *(.phase.audio .phase.audio.data)
        /* After the full bank scan, refills use the immutable resident cue
         * directory and no longer need the startup validator or parser. */
        __phase_audio_runtime_end = .;
        *(.phase.audio.startup .phase.audio.startup.*)
        __phase_audio_end = .;
    } >phase
    /* Scenery has its own REU pages; the score bank begins at $200000. */
    .phase.scenery 0x3000 : AT(0x70000) {
        __phase_scenery_start = .;
        *(.phase.scenery .phase.scenery.data)
        /* Ordinary terrestrial frames omit space-only helpers and data. The
         * opaque decoration ABI remains after the complete shared prefix. */
        __phase_scenery_terrain_end = .;
        *(.phase.scenery.space_shared .phase.scenery.space_shared.*)
        __phase_scenery_base_end = .;
    } >phase
    scenic_decoration = __phase_scenery_base_end;
    .phase.scenery.wildlife scenic_decoration : AT(0x070000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.wildlife.entry))
        *(.phase.scenery.wildlife .phase.scenery.wildlife.*)
    } >phase
    .phase.scenery.transport scenic_decoration : AT(0x150000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.transport.entry))
        *(.phase.scenery.transport .phase.scenery.transport.*)
    } >phase
    .phase.scenery.meteors scenic_decoration : AT(0x160000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.meteors.entry))
        *(.phase.scenery.meteors .phase.scenery.meteors.*)
    } >phase
    .phase.scenery.ascent scenic_decoration : AT(0x180000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.ascent.entry))
        *(.phase.scenery.ascent .phase.scenery.ascent.*)
    } >phase
    .phase.scenery.asteroid scenic_decoration : AT(0x190000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.asteroid.entry))
        *(.phase.scenery.asteroid .phase.scenery.asteroid.*)
    } >phase
    .phase.scenery.moon scenic_decoration : AT(0x1A0000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.moon.entry))
        *(.phase.scenery.moon .phase.scenery.moon.*)
    } >phase
    .phase.scenery.graveyard scenic_decoration : AT(0x1B0000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.graveyard.entry))
        *(.phase.scenery.graveyard .phase.scenery.graveyard.*)
    } >phase
    .phase.scenery.shipyard scenic_decoration : AT(0x1C0000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.shipyard.entry))
        *(.phase.scenery.shipyard .phase.scenery.shipyard.*)
    } >phase
    .phase.scenery.carrier scenic_decoration : AT(0x1D0000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.carrier.entry))
        *(.phase.scenery.carrier .phase.scenery.carrier.*)
    } >phase
    .phase.scenery.gate scenic_decoration : AT(0x1E0000 + SIZEOF(.phase.scenery)) {
        KEEP(*(.phase.scenery.gate.entry))
        *(.phase.scenery.gate .phase.scenery.gate.*)
    } >phase
    /* Shared SIM helpers reuse the retired startup / renderer workspace.
     * load_sim restores them before every SIM entry. No IRQ references. */
    .phase.sim_aux 0x8C00 : AT(0x170000) {
        __phase_sim_aux_start = .;
        *(.phase.sim_aux .phase.sim_aux.*)
        __phase_sim_aux_end = .;
    } >sim_aux
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
        *(.tables .tables.*)
        *(.rodata.*RECIPROCAL* .rodata.*COLOR_DISTANCE* .rodata.*SIN*)
    } >tables
    .rodata : { INCLUDE rodata-sections.ld } >constants
    /* Benchmark-only checkpoints use free, loaded/checksummed constant RAM.
     * Keep executable code separate so the audit still owns data jump tables. */
    .benchmark_resident : { *(.benchmark_resident .benchmark_resident.*) } >constants
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
__phase_sim_aux_size = SIZEOF(.phase.sim_aux);
__phase_enemy_size = MAX(MAX(MAX(MAX(MAX(MAX(MAX(SIZEOF(.phase.terminal), SIZEOF(.phase.enemy.lowlands)), SIZEOF(.phase.enemy.canyon)), SIZEOF(.phase.enemy.fortress)), SIZEOF(.phase.enemy.carrier)), SIZEOF(.phase.enemy.warden)), SIZEOF(.phase.enemy.citadel)), SIZEOF(.phase.enemy.flagship));
__phase_sim_size = __phase_sim_base_size + __phase_enemy_size;
__phase_sim_end = __phase_sim_start + __phase_sim_size;
__phase_geometry_size = SIZEOF(.phase.geometry);
__phase_geometry_base_size = __phase_geometry_base_end - __phase_geometry_start;
__phase_victory_size = __phase_geometry_base_size + SIZEOF(.phase.victory);
__phase_pixels_size = SIZEOF(.phase.pixels);
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
/* Preparation is complete before the row walker or packer uses this scratch.
 * The IRQ never borrows it; persistent models at $8400 are untouched. */
__raster_prepare_scratch_start = 0x77A0;
__raster_prepare_scratch_end = 0x77BF;
/* PIXELS resets this byte per band; preparation and packing own it before/after. */
__raster_player_seen = 0x77AB;

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
ASSERT(SIZEOF(.rodata) + SIZEOF(.benchmark_resident) <= 0x0800, "benchmark checkpoints exceed constant RAM")
ASSERT(SIZEOF(.tables) <= 0x0700, "resident tables exceed $9900-$9FFF")
ASSERT(SIZEOF(.phase.boot) <= 0x4200, "BOOT overlay exceeds $3000-$71FF")
ASSERT(__phase_sim_size <= 0x4200, "composed SIM overlay exceeds $3000-$71FF")
ASSERT(__phase_sim_aux_size > 0 && __phase_sim_aux_size <= 0x0500, "SIM helpers exceed retired startup / renderer workspace")
ASSERT(encounter_lowlands == encounter_step && encounter_canyon == encounter_step && encounter_fortress == encounter_step && encounter_carrier == encounter_step && encounter_warden == encounter_step && encounter_citadel == encounter_step && encounter_flagship == encounter_step, "encounter ABI entries must start every SIM tail")
ASSERT(SIZEOF(.phase.enemy.lowlands) > 0 && SIZEOF(.phase.enemy.canyon) > 0 && SIZEOF(.phase.enemy.fortress) > 0 && SIZEOF(.phase.enemy.carrier) > 0, "all four enemy tails are required")
ASSERT(SIZEOF(.phase.geometry) <= 0x4200, "GEOMETRY overlay exceeds $3000-$71FF")
ASSERT(__phase_victory_size <= 0x4200, "victory GEOMETRY overlay exceeds $3000-$71FF")
ASSERT(__phase_geometry_base_size > 0 && __phase_geometry_base_size < __phase_geometry_size, "GEOMETRY needs a common prefix and a combat tail")
ASSERT(SIZEOF(.phase.pixels) <= 0x3000, "PIXELS overlay overlaps prepared edges at $6000")
ASSERT(SIZEOF(.phase.audio) <= 0x4200, "AUDIO overlay exceeds $3000-$71FF")
ASSERT(__phase_audio_runtime_size > 0 && __phase_audio_runtime_size < __phase_audio_size, "AUDIO needs a runtime prefix before startup validation")
ASSERT(__phase_scenery_size <= 0x4200, "composed SCENERY overlay exceeds $3000-$71FF")
ASSERT(__phase_scenery_base_size > 0 && __phase_scenery_base_size < __phase_scenery_size, "SCENERY needs a common prefix and a wildlife tail")
ASSERT(__phase_scenery_terrain_size > 0 && __phase_scenery_terrain_size < __phase_scenery_base_size, "SCENERY needs a terrestrial prefix before space helpers")
ASSERT(__scenery_records_end - __scenery_records_start == 96 * 20, "incorrect scenery staging allocation")
ASSERT(__scenery_records_start >= __raster_heads_start && __scenery_records_end <= __raster_heads_end, "scenery staging exceeds command array")
ASSERT(SIZEOF(.startup_checks) > 0 && SIZEOF(.startup_checks) <= 0x0500, "startup checks exceed retired pixel-band workspace")
ASSERT(__projection_cache_start >= __pixel_band_start && __projection_cache_end <= __pixel_band_end, "projection cache exceeds shared pixel-band workspace")
ASSERT(__projection_cache_start + 624 <= __projection_cache_end, "depth-sort scratch exceeds retired projection cache")
ASSERT(__raster_heads_end - __raster_heads_start == 256 * 16, "incorrect raster head allocation")
ASSERT(__raster_tails_end - __raster_tails_start == 256 * 12, "incorrect raster tail allocation")
ASSERT(__raster_masks_start >= __raster_tails_end && __raster_masks_end - __raster_masks_start == 25 * 32, "incorrect ordered band mask allocation")
ASSERT(__raster_masks_end <= __irq_stack_bottom, "prepared raster data overlaps IRQ stack")
ASSERT(__raster_prepare_scratch_start >= __irq_stack && __raster_prepare_scratch_end <= ORIGIN(constants), "preparation scratch overlaps IRQ stack or constants")
ASSERT(__raster_prepare_scratch_end - __raster_prepare_scratch_start == 31, "incorrect native preparation scratch allocation")
ASSERT(__raster_player_seen == 0x77AB && __raster_player_seen >= __raster_prepare_scratch_start && __raster_player_seen < __raster_prepare_scratch_end, "player coverage state must stay in sequential raster scratch")

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
