"""Reject incorrect DMA layouts, transfer lengths and unsafe effects state."""
from pathlib import Path
import runpy
import struct
import subprocess
import sys
import tempfile
from reu_layout import CODE_IMAGES, OFFSETS


def main():
    source = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/havoc.elf")
    prg = Path(sys.argv[2]) if len(sys.argv) > 2 else source.with_suffix(".prg")
    checker = Path(__file__).with_name("check-layout.py")
    overlay = runpy.run_path(str(checker.with_name("check-overlays.py")))
    elf = overlay["Elf"](source)
    names = {symbol.name for symbol in elf.symbols}
    front_enabled = "span_front_compose" in names
    span_enabled = "span_compose" in names or front_enabled
    original = prg.read_bytes()
    tables = (("__sim_load_sizes", 8, "SIM load table disagrees", "SIM profile"),
              ("__code_sizes", 26, "Code size table disagrees", "OVL7 image"))

    def check(path, image=source):
        return subprocess.run([sys.executable, str(checker), str(image), str(path)],
                              capture_output=True, text=True)

    baseline = check(prg)
    assert baseline.returncode == 0, baseline.stdout + baseline.stderr
    cases = 0
    with tempfile.TemporaryDirectory(prefix="havoc-layout-audit-") as directory:
        damaged = Path(directory) / "damaged.prg"
        for table, count, expected, label in tables:
            symbol = next(symbol for symbol in elf.symbols if symbol.name == table)
            offset = symbol.address - int.from_bytes(original[:2], "little") + 2
            for profile in range(count):
                at = offset + profile * 2
                length = int.from_bytes(original[at:at+2], "little")
                for delta in (-1, 1):
                    data = bytearray(original)
                    data[at:at+2] = (length + delta).to_bytes(2, "little")
                    damaged.write_bytes(data)
                    result = check(damaged)
                    assert result.returncode != 0, f"accepted {label} {profile} length {length + delta}"
                    assert expected in result.stderr, result.stdout + result.stderr
                    cases += 1
                    print(f"Rejected {label} {profile} length {length + delta}")
        reu_table = next(symbol for symbol in elf.symbols if symbol.name == "__code_reu_pages")
        reu_offset = reu_table.address - int.from_bytes(original[:2], "little") + 2
        for phase_id in range(34):
            data = bytearray(original)
            data[reu_offset + phase_id] ^= 1
            damaged.write_bytes(data)
            result = check(damaged)
            assert result.returncode != 0, f"accepted REU address table phase {phase_id}"
            assert "REU address table disagrees" in result.stderr, result.stdout + result.stderr
            cases += 1
            print(f"Rejected REU address table phase {phase_id}")
        damaged_elf = Path(directory) / "damaged.elf"
        for name, value, expected in (
            ("__segment0_start", 0x5ff, "incorrect first mission buffer allocation"),
            ("__segment0_end", 0x801, "incorrect first mission buffer allocation"),
            ("__segment1_start", 0x96ff, "incorrect second mission buffer allocation"),
            ("__segment1_end", 0x9901, "incorrect second mission buffer allocation"),
            ("__resident_end", 0x3001, "resident code overlaps the overlay window at $3000"),
            ("__hud_sprites_start", 0x8bff, "incorrect HUD sprite allocation"),
            ("__hud_sprites_end", 0x8e01, "incorrect HUD sprite allocation"),
            ("__reticle_sprite_start", 0xbfbf, "incorrect resident reticle allocation"),
            ("__reticle_sprite_start", 0xbf80, "incorrect resident reticle allocation"),
            ("__reticle_sprite_end", 0xc001, "incorrect resident reticle allocation"),
            ("__startup_checks_start", 0x8dff, "startup checks must start after HUD sprites"),
            ("__startup_checks_end", 0x9301, "startup checks exceed shared workspace"),
            ("__phase_sim_aux_start", 0x8dff, "SIM helpers must start after HUD sprites"),
            ("__phase_sim_aux_end", 0x9301, "SIM helpers exceed shared workspace"),
            ("__phase_sim_extra_start", 0xdfff, "SIM extra helpers must start at $E000"),
            ("__phase_sim_extra_end", 0xf801, "SIM extra helpers exceed retired raster workspace"),
            ("__phase_sim_aux_size", 0x500, "incorrect composed SIM helper size"),
            ("__pixel_band_start", 0x8dff, "pixel band must occupy $8E00-$92FF after HUD sprites"),
            ("__pixel_band_end", 0x9301, "pixel band must occupy $8E00-$92FF after HUD sprites"),
            ("__sfx_state_start", 0xcd01, "SFX state bounds disagree"),
            ("__sfx_state_end", 0xcd00, "SFX state bounds disagree"),
            ("__sfx_state_end", 0xcdff, "SFX state bounds disagree"),
            ("__sfx_state_end", 0xce01, "SFX state bounds disagree"),
            ("__cell_hud_seen", 0x9327, "HUD-seen flag must reuse cell scratch at $9328"),
            ("__cell_hud_seen", 0x9329, "HUD-seen flag must reuse cell scratch at $9328"),
            ("__phase_pixels_prefix_size", 0x3f21, "PIXELS prefix overlaps its helpers"),
            ("__phase_pixels_aux_start", 0x6f1f, "PIXELS helpers must start at $6F20"),
            ("__phase_pixels_aux_size", 0x2e1, "PIXELS helpers exceed workspace"),
            ("__phase_pixels_aux_load", OFFSETS['pixels'] + 0x3f21, "incorrect PIXELS helper REU address"),
            ("__phase_pixels_aux_reu_bank", (OFFSETS['pixels'] >> 16) + 1, "incorrect PIXELS helper REU bank"),
            ("__phase_pixels_aux_reu_offset", 0x3f21, "incorrect PIXELS helper REU bank offset"),
            ("__phase_pixels_size", 0x3000, "incorrect composed PIXELS size"),
            ("__staged_bitmap_start", 0x953f, "staged bitmap must reuse cell coverage only"),
            ("__staged_bitmap_end", 0x9681, "staged bitmap must reuse cell coverage only"),
            ("__staged_screen_start", 0x76ff, "staged screen must reuse HUD slot flags"),
            ("__staged_screen_end", 0x7729, "staged screen must reuse HUD slot flags"),
            ("__staged_color_start", 0x7727, "staged colors must reuse overflow flags only"),
            ("__staged_color_end", 0x7751, "staged colors must reuse overflow flags only"),
            ("__reu_size", 0x800000, "incorrect 16 MiB REU image size"),
            ("__reu_digital_music", 0xf0000, "incorrect digital music address"),
            ("__reu_digital_victory", 0x200000, "incorrect digital victory alias"),
            ("__reu_digital_ring", 0x3e0000, "incorrect digital ring address"),
            ("__reu_digital_ring_size", 0x10000, "incorrect digital ring size"),
            ("__reu_code_slot_size", 0x4000, "incorrect REU code slot size"),
            ("__reu_models", 0xb1000, "incorrect REU model bank address"),
            ("__reu_models_end", 0x14001, "incorrect REU model bank end"),
            ("__reu_probe_start", 0xb00, "incorrect REU probe reservation"),
            ("__reu_probe_end", 0x1001, "incorrect REU probe reservation"),
            ("__reu_scenery_gate", 0x80000, "scenery-gate REU slot exceeds image"),
            ("__reu_scenery_gate", OFFSETS['scenery-gate'] + 1, "scenery-gate REU slot must be 2 KiB aligned"),
            ("__reu_scenery_gate_end", OFFSETS['scenery-gate'] + 0x4000, "incorrect scenery-gate REU reservation size"),
            ("__phase_boot_size", 0x3001, "boot exceeds REU reservation"),
            ("__phase_audio_size", 0x3801, "audio exceeds REU reservation"),
            *((f'__reu_{symbol}_end', OFFSETS[name] + 0x800,
               f'incorrect {name} REU reservation size') for name, symbol, _ in CODE_IMAGES),
            ("__shadow_frame_start", 0xe7ff, "incorrect persistent shadow frame allocation"),
            ("__shadow_frame_end", 0xff21, "incorrect persistent shadow frame allocation"),
            ("__phase_shadow_finalize_size", 0, "SHADOW finalization prefix must be shorter"),
            ("__phase_shadow_finalize_end", 0x3001, "SHADOW finalization prefix boundary mismatch"),
            ("__code_reu_pages_end", reu_table.address + 30, "incorrect REU address table size"),
            *((name, value, "span buffer must reuse exactly") for name, value in (
                ("__span_buffer_start", 0xcfff),
                ("__span_buffer_end", 0xda01),
                ("__span_buffer_size", 0xa01),
            ) if span_enabled),
            *((name, value, "front span heads must reuse exactly") for name, value in (
                ("__span_front_heads_start", 0x77ab),
                ("__span_front_heads_start", 0x77ad),
                ("__span_front_heads_end", 0x77b3),
                ("__span_front_heads_end", 0x77b5),
            ) if front_enabled),
        ):
            entry = next(table.offset + index * table.entry_size
                         for table in elf.sections if table.kind == 2
                         for index, symbol in enumerate(elf.symbol_tables[table.index])
                         if symbol.name == name)
            data = bytearray(elf.data)
            struct.pack_into("<I", data, entry + 4, value)
            damaged_elf.write_bytes(data)
            result = check(prg, damaged_elf)
            assert result.returncode != 0, f"accepted {name}={value}"
            assert expected in result.stderr, result.stdout + result.stderr
            cases += 1
            print(f"Rejected {name}={value}")
        section_offset = struct.unpack_from("<I", elf.data, 32)[0]
        section_stride = struct.unpack_from("<H", elf.data, 46)[0]
        sfx_state = next(section for section in elf.sections if section.name == ".sfx_state")
        page_header = section_offset + sfx_state.index * section_stride
        for field, value in ((12, 0x721), (20, 0), (20, 223), (20, 225)):
            data = bytearray(elf.data)
            struct.pack_into("<I", data, page_header + field, value)
            damaged_elf.write_bytes(data)
            result = check(prg, damaged_elf)
            assert result.returncode != 0, f"accepted SFX state field {field}={value}"
            assert "SFX state" in result.stderr or ".sfx_state outside allocation" in result.stderr, result.stdout + result.stderr
            cases += 1
            print(f"Rejected SFX state field {field}={value}")
        game_state = next(section for section in elf.sections if section.name == ".game_state")
        game_header = section_offset + game_state.index * section_stride
        # Four boss records and byte-sized missile locks may occupy the first
        # 128 bytes of the former IRQ backup, but must stop before $CB00.
        data = bytearray(elf.data)
        struct.pack_into("<I", data, game_header + 20, 0x300)
        damaged_elf.write_bytes(data)
        result = check(prg, damaged_elf)
        assert result.returncode == 0, result.stdout + result.stderr
        for field, value in ((12, 0xc7ff), (20, 0x301)):
            data = bytearray(elf.data)
            struct.pack_into("<I", data, game_header + field, value)
            damaged_elf.write_bytes(data)
            result = check(prg, damaged_elf)
            assert result.returncode != 0, f"accepted Game state field {field}={value}"
            assert ".game_state outside allocation" in result.stderr, result.stdout + result.stderr
            cases += 1
            print(f"Rejected Game state field {field}={value}")
        program_offset = struct.unpack_from("<I", elf.data, 28)[0]
        program_stride, program_count = struct.unpack_from("<HH", elf.data, 42)
        boot = next(section for section in elf.sections if section.name == ".phase.boot")
        boot_program = next(program_offset + index * program_stride
                            for index in range(program_count)
                            if struct.unpack_from("<II", elf.data, program_offset + index * program_stride)
                            == (1, boot.offset))
        data = bytearray(elf.data)
        physical = struct.unpack_from("<I", data, boot_program + 12)[0]
        struct.pack_into("<I", data, boot_program + 12, physical + 0x1000)
        damaged_elf.write_bytes(data)
        result = check(prg, damaged_elf)
        assert result.returncode != 0, "accepted incorrect BOOT ELF load address"
        assert ".phase.boot ELF load address disagrees with REU slot" in result.stderr, result.stdout + result.stderr
        cases += 1
        print("Rejected BOOT ELF load address")
    assert prg.read_bytes() == original, "input PRG changed"
    assert source.read_bytes() == elf.data, "input ELF changed"
    print(f"Layout audit negative fixtures: {cases} passed; original package unchanged")


if __name__ == "__main__":
    main()
