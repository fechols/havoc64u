"""Prove composed overlay guards reject private, deliberately damaged ELFs."""
from pathlib import Path
import runpy
import struct
import sys
import tempfile


def main():
    source = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/havoc.elf")
    checker = runpy.run_path(str(Path(__file__).with_name("check-overlays.py")))
    elf = checker["Elf"](source)
    checker["audit"](source)
    names = {symbol.name for symbol in elf.symbols}
    front_enabled = "span_front_compose" in names
    span_entry = "span_front_compose" if front_enabled else "span_compose"
    span_enabled = span_entry in names
    entries = []
    for section in elf.sections:
        if section.kind != 4:
            continue
        symbols = elf.symbol_tables[section.link]
        owner = elf.sections[section.info]
        for position in range(section.offset, section.offset + section.size, 12):
            address, info, addend = struct.unpack_from("<IIi", elf.data, position)
            entries.append((position, owner, address, info & 255, symbols[info >> 8], section.link))

    def symbol_index(table, name):
        return next(i for i, symbol in enumerate(elf.symbol_tables[table]) if symbol.name == name)

    def replace_target(data, entry, name):
        position, _owner, _address, kind, _symbol, table = entry
        struct.pack_into("<Ii", data, position + 4, (symbol_index(table, name) << 8) | kind, 0)

    def symbol_address(data, name, value):
        table = next(section for section in elf.sections if section.kind == 2)
        index = symbol_index(table.index, name)
        struct.pack_into("<I", data, table.offset + index * table.entry_size + 4, value)

    opaque = next(entry for entry in entries if entry[4].name == "encounter_step")
    scenic = next(entry for entry in entries if entry[4].name == "scenic_decoration")
    terminal = next(entry for entry in entries if entry[4].name == "terminal_step")
    victory = next(entry for entry in entries if entry[4].name == "victory_geometry")
    geometry = next(entry for entry in entries if entry[1].name == ".phase.geometry"
                    and entry[3] == 3 and entry[4].name == "fast_emit_triangle")
    audio_validation = next(entry for entry in entries
                            if entry[1].name == ".phase.audio" and entry[3] == 3
                            and entry[4].section < len(elf.sections)
                            and (callee := elf.function_at(entry[4].section, entry[4].address
                                 + struct.unpack_from("<i", elf.data, entry[0] + 8)[0]))
                            and callee.section == entry[1].index)
    audio_parser = next(symbol.name for symbol in elf.symbols
                        if checker["readable_name"](symbol.name) == "havoc::digital::parse_header")
    lowlands = next(entry for entry in entries if entry[1].name == ".phase.enemy.lowlands" and entry[3] == 3)
    wildlife = next(entry for entry in entries if entry[1].name == ".phase.scenery.wildlife" and entry[3] == 3)
    terrain_end = next(symbol.address for symbol in elf.symbols if symbol.name == "__phase_scenery_terrain_end")
    terrain_call = next(entry for entry in entries
                        if entry[1].name == ".phase.scenery" and entry[2] < terrain_end
                        and entry[3] == 3 and entry[4].section == entry[1].index
                        and entry[4].address < terrain_end)
    space_helper = next(symbol.name for symbol in elf.symbols
                        if symbol.section == terrain_call[1].index and symbol.kind == 2
                        and symbol.address >= terrain_end)
    pointer = next(entry for entry in entries
                   if not entry[1].executable and entry[1].flags & 2 and entry[3] in (3, 4, 5)
                   and entry[4].section < len(elf.sections)
                   and elf.sections[entry[4].section].name.startswith(".phase."))
    irq = next(entry for entry in entries
               if entry[1].name == ".text" and entry[3] == 3
               and (function := elf.function_at(entry[1].index, entry[2]))
               and function.name == "irq_body")
    auxiliary = next(section for section in elf.sections if section.name == checker["SIM_AUX_SECTION"])
    auxiliary_name = next(symbol.name for symbol in elf.symbols if symbol.section == auxiliary.index and symbol.kind == 2)
    auxiliary_call = next(entry for entry in entries if entry[1].index == auxiliary.index and entry[3] == 3)
    extra = next(section for section in elf.sections if section.name == checker["SIM_EXTRA_SECTION"])
    extra_name = next(symbol.name for symbol in elf.symbols if symbol.section == extra.index and symbol.kind == 2)
    pixels_auxiliary = next(section for section in elf.sections if section.name == checker["PIXELS_AUX_SECTION"])
    pixels_auxiliary_name = next(symbol.name for symbol in elf.symbols
                                if symbol.section == pixels_auxiliary.index and symbol.kind == 2)
    pixels_prefix_size = next(entry for entry in entries if entry[4].name == "__phase_pixels_prefix_size")
    pixels_dma = next(entry for entry in entries if entry[1].name == ".phase.pixels" and entry[3] == 3
                      and (caller := elf.function_at(entry[1].index, entry[2]))
                      and caller.name == "havoc_c64::draw_pixels"
                      and entry[4].section < len(elf.sections)
                      and (callee := elf.function_at(entry[4].section, entry[4].address
                           + struct.unpack_from("<i", elf.data, entry[0] + 8)[0]))
                      and callee.name == "havoc_c64::dma")
    pixels_io_on = next(entry for entry in entries if entry[1].name == ".phase.pixels"
                        and entry[4].name == "io_on")
    startup_name = next(symbol.name for symbol in elf.symbols
                        if checker["readable_name"](symbol.name) == checker["STARTUP_ENTRY"])
    phase_loader_name = next(symbol.name for symbol in elf.symbols
                            if checker["readable_name"](symbol.name) == "havoc_c64::load_phase")
    helper_dma = next(entry for entry in entries if entry[1].executable and entry[3] == 3
                      and (caller := elf.function_at(entry[1].index, entry[2]))
                      and caller.name == "havoc_c64::load_sim"
                      and entry[4].section < len(elf.sections)
                      and (callee := elf.function_at(entry[4].section, entry[4].address + struct.unpack_from("<i", elf.data, entry[0] + 8)[0]))
                      and callee.name == "havoc_c64::dma")
    geometry_prefix_size = next(entry for entry in entries if entry[4].name == '__phase_geometry_prefix_size')
    shadow_finalize_size = next(entry for entry in entries if entry[4].name == '__phase_shadow_finalize_size')
    shadow_tail_name = next(symbol.name for symbol in elf.symbols
                            if checker['readable_name'](symbol.name) == 'havoc_c64::setup_shadows')
    shadow_finalize_call = next(entry for entry in entries if entry[1].name == '.phase.shadow'
                               and entry[3] == 3
                               and (caller := elf.function_at(entry[1].index, entry[2]))
                               and caller.name == 'havoc::render::Renderer::finish_ground_shadows')
    shadow_projection_call = next(entry for entry in entries if entry[1].name == '.phase.shadow'
                            and entry[3] == 3 and entry[4].name == 'fast_project')
    scenery_helper_name = next(symbol.name for symbol in elf.symbols
                               if symbol.kind == 2 and symbol.section < len(elf.sections)
                               and elf.sections[symbol.section].name == '.phase.scenery_aux')

    def auxiliary_header(data, field, value):
        headers = struct.unpack_from("<I", data, 32)[0]
        struct.pack_into("<I", data, headers + auxiliary.index * 40 + field, value)

    def pixels_header(data, field, value):
        headers = struct.unpack_from("<I", data, 32)[0]
        struct.pack_into("<I", data, headers + pixels_auxiliary.index * 40 + field, value)

    def extra_header(data, field, value):
        headers = struct.unpack_from("<I", data, 32)[0]
        struct.pack_into("<I", data, headers + extra.index * 40 + field, value)

    def non_jsr(data, entry):
        _position, owner, address, _kind, _symbol, _table = entry
        data[owner.offset + address - owner.address - 1] = 0x4c

    def nonzero_addend(data, entry):
        addend = struct.unpack_from("<i", data, entry[0] + 8)[0]
        struct.pack_into("<i", data, entry[0] + 8, addend + 1)

    def scenic_load(data, entry=scenic):
        _position, owner, address, _kind, _symbol, _table = entry
        data[owner.offset + address - owner.address - 1] = 0xad

    cases = [
        ("opaque-tail-jump", lambda data: non_jsr(data, opaque), "opaque encounter ABI"),
        ("opaque-interior-entry", lambda data: nonzero_addend(data, opaque), "opaque encounter ABI"),
        ("direct-common-to-tail", lambda data: replace_target(data, opaque, "encounter_lowlands"), "unsafe overlay references"),
        ("geometry-calls-encounter", lambda data: replace_target(data, geometry, "encounter_step"), "opaque encounter ABI"),
        ("irq-calls-encounter", lambda data: replace_target(data, irq, "encounter_step"), "opaque encounter ABI"),
        ("escaped-encounter-pointer", lambda data: replace_target(data, pointer, "encounter_step"), None),
        ("foreign-switch-table", lambda data: replace_target(data, pointer, "encounter_canyon"), "unsafe overlay references"),
        ("scenic-address-load", scenic_load, "opaque scenery ABI"),
        ("scenic-interior-entry", lambda data: nonzero_addend(data, scenic), "opaque scenery ABI"),
        ("direct-common-to-scenic", lambda data: replace_target(data, scenic, "scenic_wildlife"), "unsafe overlay references"),
        ("geometry-calls-scenic", lambda data: replace_target(data, geometry, "scenic_decoration"), "opaque scenery ABI"),
        ("irq-calls-scenic", lambda data: replace_target(data, irq, "scenic_decoration"), "opaque scenery ABI"),
        ("escaped-scenic-pointer", lambda data: replace_target(data, pointer, "scenic_decoration"), None),
        ("encounter-calls-scenic", lambda data: replace_target(data, lowlands, "scenic_wildlife"), "unsafe overlay references"),
        ("scenic-calls-encounter", lambda data: replace_target(data, wildlife, "encounter_lowlands"), "unsafe overlay references"),
        ("terrain-calls-unloaded-space-helper", lambda data: replace_target(data, terrain_call, space_helper), "terrestrial SCENERY prefix references unloaded space helpers"),
        ("earth-tail-calls-unloaded-space-helper", lambda data: replace_target(data, wildlife, space_helper), "terrestrial SCENERY prefix references unloaded space helpers"),
        ("audio-runtime-calls-startup-parser", lambda data: replace_target(data, audio_validation, audio_parser), "unsafe overlay references"),
        ("audio-validation-interior-entry", lambda data: (replace_target(data, audio_validation, audio_parser), nonzero_addend(data, audio_validation)), "unsafe overlay references"),
        ("audio-validation-address-load", lambda data: (replace_target(data, audio_validation, audio_parser), scenic_load(data, audio_validation)), "unsafe overlay references"),
        ("geometry-calls-terminal", lambda data: replace_target(data, geometry, "terminal_step"), "opaque terminal_step ABI"),
        ("sim-calls-victory", lambda data: replace_target(data, opaque, "victory_geometry"), "opaque victory_geometry ABI"),
        ("direct-terminal-tail", lambda data: replace_target(data, terminal, "game_terminal"), "unsafe overlay references"),
        ("direct-victory-tail", lambda data: replace_target(data, victory, "medal_geometry"), "unsafe overlay references"),
        ("irq-calls-medal", lambda data: replace_target(data, irq, "victory_geometry"), "opaque victory_geometry ABI"),
        ("irq-calls-sim-helpers", lambda data: replace_target(data, irq, auxiliary_name), "unsafe overlay references"),
        ("escaped-helper-pointer", lambda data: replace_target(data, pointer, auxiliary_name), "unsafe overlay references"),
        ("helpers-call-retired-startup", lambda data: replace_target(data, auxiliary_call, startup_name), "unsafe overlay references"),
        ("helpers-call-alternative-tail", lambda data: replace_target(data, auxiliary_call, "encounter_lowlands"), "unsafe overlay references"),
        ("helpers-wrong-address", lambda data: auxiliary_header(data, 12, 0x8e01), "SIM helpers must fit"),
        ("helpers-outgrow-workspace", lambda data: auxiliary_header(data, 20, 0x501), "SIM helpers must fit"),
        ("helpers-not-reloaded", lambda data: replace_target(data, helper_dma, phase_loader_name), "SIM helper loader"),
        ("sim-extra-wrong-address", lambda data: extra_header(data, 12, 0xe001), "SIM extra helpers must fit"),
        ("sim-extra-outgrows-workspace", lambda data: extra_header(data, 20, 0x1801), "SIM extra helpers must fit"),
        ("irq-calls-sim-extra", lambda data: replace_target(data, irq, extra_name), "unsafe overlay references"),
        ("geometry-calls-sim-extra", lambda data: replace_target(data, geometry, extra_name), "unsafe overlay references"),
        ("escaped-sim-extra-pointer", lambda data: replace_target(data, pointer, extra_name), "unsafe overlay references"),
        ("pixels-helpers-overlap-masks", lambda data: pixels_header(data, 12, 0x6f1f), "PIXELS helpers must fit"),
        ("pixels-helpers-overlap-irq", lambda data: pixels_header(data, 20, 0x2e1), "PIXELS helpers must fit"),
        ("pixels-helpers-not-reloaded", lambda data: replace_target(data, pixels_dma, phase_loader_name), "PIXELS helper loader"),
        ("pixels-helper-dma-with-io-hidden", lambda data: replace_target(data, pixels_io_on, "io_off"), "PIXELS helper loader"),
        ("pixels-load-overwrites-raster-data", lambda data: replace_target(data, pixels_prefix_size, "__phase_pixels_size"), "runtime PIXELS loader"),
        ("geometry-calls-pixels-helpers", lambda data: replace_target(data, geometry, pixels_auxiliary_name), "unsafe overlay references"),
        ("irq-calls-pixels-helpers", lambda data: replace_target(data, irq, pixels_auxiliary_name), "unsafe overlay references"),
        ("escaped-pixels-helper-pointer", lambda data: replace_target(data, pointer, pixels_auxiliary_name), "unsafe overlay references"),
        ('geometry-full-image-runtime-load', lambda data: replace_target(data, geometry_prefix_size, '__phase_geometry_size'), 'runtime GEOMETRY loader'),
        ('shadow-calls-retired-geometry', lambda data: replace_target(data, shadow_projection_call, 'fast_emit_triangle'), 'unsafe overlay references'),
        ('shadow-finalize-wrong-transfer-size', lambda data: symbol_address(data, '__phase_shadow_finalize_size', 1), 'SHADOW finalization transfer size'),
        ('shadow-finalize-missing-prefix-load', lambda data: replace_target(data, shadow_finalize_size, '__phase_shadow_size'), 'SHADOW loader must choose the full image'),
        ('shadow-finalize-calls-unloaded-tail', lambda data: replace_target(data, shadow_finalize_call, shadow_tail_name), 'unsafe overlay references'),
        ('irq-calls-projection-helper', lambda data: replace_target(data, irq, 'fast_project'), 'unsafe overlay references'),
        ('geometry-calls-retired-scenery-helper', lambda data: replace_target(data, geometry, scenery_helper_name), 'unsafe overlay references'),
        ('projection-helper-wrong-load-address', lambda data: symbol_address(data, '__phase_render_aux_load', 0x33f21), 'shared projection'),
    ]
    # Symbol presence is sufficient: generic arithmetic must stay absent even
    # if an optimizer currently leaves a helper unreachable.
    def rename_symbol(data, name):
        table = next(section for section in elf.sections if section.kind == 2)
        index = next(i for i, symbol in enumerate(elf.symbol_tables[table.index])
                     if symbol.name == '__phase_sim_aux_size')
        offset = struct.unpack_from('<I', data, table.offset + index * table.entry_size)[0]
        strings = elf.sections[table.link]
        data[strings.offset + offset:strings.offset + offset + len(name) + 1] = name.encode() + b'\0'

    for helper in ('__divsi3', '__udivsi3', '__mulsi3'):
        cases.append((f'forbidden-{helper}', lambda data, name=helper: rename_symbol(data, name),
                      'generic 32-bit helper linked'))
    projection_loader = next((symbol.name for symbol in elf.symbols
                              if checker['readable_name'](symbol.name) == 'havoc_c64::benchmark_load_projection'), None)
    if projection_loader is not None:
        projection_call = next(entry for entry in entries
                               if entry[1].executable and entry[4].section < len(elf.sections)
                               and (callee := elf.function_at(entry[4].section, entry[4].address
                                    + struct.unpack_from('<i', elf.data, entry[0] + 8)[0]))
                               and callee.name == 'havoc_c64::benchmark_load_projection')
        expected = 'benchmark projection loader needs one direct GEOMETRY entry call'
        cases.extend((
            ('benchmark-projection-jump', lambda data: non_jsr(data, projection_call), expected),
            ('benchmark-projection-interior', lambda data: nonzero_addend(data, projection_call), expected),
            ('irq-calls-benchmark-projection', lambda data: replace_target(data, irq, projection_loader), expected),
            ('escaped-benchmark-projection-pointer', lambda data: replace_target(data, pointer, projection_loader), expected),
        ))
    for name, value, expected in (
        ('__hud_sprites_start', 0x8bff, 'incorrect HUD sprite allocation'),
        ('__hud_sprites_end', 0x8e01, 'incorrect HUD sprite allocation'),
        ('__segment0_start', 0x5ff, 'incorrect mission buffer allocation'),
        ('__segment0_end', 0x801, 'incorrect mission buffer allocation'),
        ('__segment1_start', 0x96ff, 'incorrect mission buffer allocation'),
        ('__segment1_end', 0x9901, 'incorrect mission buffer allocation'),
        ('__resident_end', 0x3001, 'resident code overlaps the overlay window at $3000'),
    ):
        cases.append((f'ownership-{name}',
                      lambda data, name=name, value=value: symbol_address(data, name, value),
                      expected))
    for value in (0x9327, 0x9329):
        cases.append((f'hud-seen-address-{value:04x}',
                      lambda data, value=value: symbol_address(data, '__cell_hud_seen', value),
                      'HUD-seen flag must reuse cell scratch at $9328'))
    if span_enabled:
        for name, value in (('__span_buffer_start', 0xcfff),
                            ('__span_buffer_end', 0xda01),
                            ('__span_buffer_size', 0xa01)):
            cases.append((f'span-buffer-{name}',
                          lambda data, name=name, value=value: symbol_address(data, name, value),
                          'span buffer must reuse exactly'))
        # Front code belongs to PIXELS, so the general overlay guard may
        # reject these references before the compositor-specific guard.
        span_rejection = ('span compositor reference outside PIXELS', 'unsafe overlay references')
        cases.extend([
            ('geometry-calls-span-compositor', lambda data: replace_target(data, geometry, span_entry),
             span_rejection),
            ('irq-calls-span-compositor', lambda data: replace_target(data, irq, span_entry),
             span_rejection),
            ('escaped-span-compositor', lambda data: replace_target(data, pointer, span_entry),
             span_rejection),
        ])
    if front_enabled:
        for name, value in (('__span_front_heads_start', 0x77ab),
                            ('__span_front_heads_start', 0x77ad),
                            ('__span_front_heads_end', 0x77b3),
                            ('__span_front_heads_end', 0x77b5)):
            cases.append((f'front-heads-{name}-{value:04x}',
                          lambda data, name=name, value=value: symbol_address(data, name, value),
                          'front span heads must reuse exactly'))
    for phase in (".phase.boot", ".phase.geometry", ".phase.pixels", checker["PIXELS_AUX_SECTION"], ".phase.audio", ".phase.scenery", checker["STARTUP_SECTION"]):
        source_call = next(entry for entry in entries if entry[1].name == phase and entry[3] == 3
                           and (caller := elf.function_at(entry[1].index, entry[2]))
                           and caller.name not in ("havoc_c64::draw_pixels", "havoc_c64::prepare_geometry", "havoc_c64::prepare_scenery"))
        cases.append((f"{phase}-calls-sim-helpers",
                      lambda data, call=source_call: replace_target(data, call, auxiliary_name),
                      "unsafe overlay references"))
    # Every composed profile participates in the negative audit, including all
    # three bosses; a newly added tail must not silently bypass classification.
    for family in (checker["ENEMY_ENTRIES"], checker["SCENERY_ENTRIES"]):
        tails = list(family.items())
        for index, (section, _entry) in enumerate(tails):
            source_call = next(entry for entry in entries if entry[1].name == section and entry[3] == 3)
            target_name = tails[(index + 1) % len(tails)][1]
            cases.append((f"cross-tail-{section}",
                          lambda data, call=source_call, target=target_name: replace_target(data, call, target),
                          "unsafe overlay references"))
    with tempfile.TemporaryDirectory(prefix="havoc-overlay-audit-") as temporary:
        target = Path(temporary) / "damaged.elf"
        for name, mutate, expected in cases:
            data = bytearray(elf.data)
            mutate(data)
            target.write_bytes(data)
            try:
                checker["audit"](target)
            except checker["AuditError"] as error:
                messages = (expected,) if isinstance(expected, str) else expected
                if messages is not None and not any(message in str(error) for message in messages):
                    raise AssertionError(f"{name}: unexpected rejection: {error}") from error
                print(f"Rejected {name}")
            else:
                raise AssertionError(f"audit accepted {name}")
    assert source.read_bytes() == elf.data, "input ELF changed"
    print(f"Overlay audit negative fixtures: {len(cases)} passed; original ELF unchanged")


if __name__ == "__main__":
    main()
