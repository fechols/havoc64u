"""Reject allocated sections outside the fixed C64 ownership map."""
import os, pathlib, re, runpy, struct, subprocess, sys
elf, prg = map(pathlib.Path, sys.argv[1:3])
nm = pathlib.Path(os.environ.get('LLVM_MOS', str(pathlib.Path.home()/'.local/llvm-mos')))/'bin/llvm-nm'
readelf = nm.with_name('llvm-readelf')
listing=subprocess.check_output([str(readelf),'-SW',str(elf)],text=True)
symbols={}
for line in subprocess.check_output([str(nm),'-n',str(elf)],text=True).splitlines():
    fields=line.split()
    if len(fields)==3:
        try: symbols[fields[2]]=int(fields[0],16)
        except ValueError: pass
for helper in ('__divsi3', '__udivsi3', '__mulsi3'):
    assert helper not in symbols, f'generic 32-bit helper linked: {helper}'
span_enabled = 'span_compose' in symbols
if span_enabled:
    assert (symbols.get('__span_buffer_start'), symbols.get('__span_buffer_end'),
            symbols.get('__span_buffer_size')) == (0xd000, 0xda00, 0xa00), \
        'span buffer must reuse exactly $D000-$D9FF of retired commands'
overlay_audit = runpy.run_path(str(pathlib.Path(__file__).with_name('check-overlays.py')))
metadata = overlay_audit['Elf'](elf)
directories = [symbol for symbol in metadata.symbols
               if overlay_audit['readable_name'](symbol.name) == 'havoc_c64::MUSIC_DIRECTORY']
assert len(directories) == 1, 'missing or ambiguous resident music directory'
music_directory = directories[0]
assert music_directory.size == 21 * 8, 'music directory must hold all 21 validated descriptors'
assert metadata.sections[music_directory.section].name == '.bss', 'music directory must not enter IRQ-saved .noinit'
compiler_frames = next(section for section in metadata.sections if section.name == '.noinit')
assert music_directory.address + music_directory.size <= compiler_frames.address, 'music directory overlaps compiler static frames'
assert symbols['__irq_static_save_size'] == compiler_frames.size, 'IRQ save extent disagrees with compiler static frames'
data=prg.read_bytes()
assert int.from_bytes(data[:2],'little')==0x0801,'PRG must load at0801'
assert len(data)-2+0x0801==0xa000,'PRG must initialize through9FFF'
assert symbols.get('__stack')==0xc800,'incorrect main stack'
assert symbols.get('__irq_stack')==0x7600,'incorrect IRQ stack'
assert symbols['__resident_end']<=0x3000
assert symbols['__startup_checks_start']==0x8c00
assert 0x8c00<symbols['__startup_checks_end']<=0x9100
assert symbols['__phase_sim_aux_start']==0x8c00
assert 0x8c00<symbols['__phase_sim_aux_end']<=0x9100
assert symbols['__phase_sim_aux_size']==symbols['__phase_sim_aux_end']-symbols['__phase_sim_aux_start']
assert symbols['__music_page_start']==0x720
assert symbols['__music_page_end'] in (0x720,0x800)
four_sid=symbols['__music_page_end']==0x800
low_end=0x720 if four_sid else 0x800
assert symbols['__renderer_start']==0xcffa and symbols['__renderer_end']==0xe000
assert symbols['__irq_static_save_size']<=1088
assert symbols['__pixel_band_start']==0x8c00 and symbols['__pixel_band_end']==0x9100
assert symbols['__projection_cache_start']==0x8c00 and symbols['__projection_cache_end']==0x8e80
assert symbols['__projection_cache_end']<=symbols['__pixel_band_end']
assert symbols['__projection_cache_start']+624<=symbols['__projection_cache_end']
assert symbols['__projection_cache_start']+0x400<=symbols['__pixel_band_end']
assert symbols['__sort_order']==0x7600 and symbols['__irq_stack']==0x7600
assert (symbols['__staged_bitmap_start'],symbols['__staged_bitmap_end'])==(0x9340,0x9480), 'staged bitmap must reuse cell coverage only'
assert (symbols['__staged_screen_start'],symbols['__staged_screen_end'])==(0x7700,0x7728), 'staged screen must reuse HUD slot flags'
assert (symbols['__staged_color_start'],symbols['__staged_color_end'])==(0x7728,0x7750), 'staged colors must reuse overflow flags only'
# The PIXELS kernels own zero page $D0-$E0 (row walker) and $F0-$FF (cell
# writer, $FF the band's clear colour): the compiler's zero-page region must
# stay empty.
assert symbols['__zp_bss_size']==0 and symbols['__zp_data_size']==0, 'compiler zero-page statics would collide with the PIXELS kernels'
assert (symbols['__raster_heads_start'],symbols['__raster_heads_end'])==(0xef20,0xff20)
assert (symbols['__scenery_records_start'],symbols['__scenery_records_end'])==(0xd880,0xe000)
assert 0 < symbols['__phase_scenery_base_size'] < symbols['__phase_scenery_size'] <= 0x4200
assert symbols['__phase_scenery_base_end'] == 0x3000 + symbols['__phase_scenery_base_size']
assert 0 < symbols['__phase_scenery_terrain_size'] < symbols['__phase_scenery_base_size']
assert symbols['__phase_scenery_terrain_end'] == 0x3000 + symbols['__phase_scenery_terrain_size']
assert 0 < symbols['__phase_geometry_base_size'] < symbols['__phase_geometry_size'] <= 0x4200
assert symbols['__phase_geometry_base_end'] == 0x3000 + symbols['__phase_geometry_base_size']
assert 0 < symbols['__phase_audio_runtime_size'] < symbols['__phase_audio_size'] <= 0x4200
assert symbols['__phase_audio_runtime_end'] == 0x3000 + symbols['__phase_audio_runtime_size']
assert symbols['encounter_step'] == symbols['__phase_sim_base_end'] == 0x3000 + symbols['__phase_sim_base_size']
assert 0 < symbols['__phase_sim_base_size'] < symbols['__phase_sim_size'] <= 0x4200
assert symbols['__phase_sim_size'] == symbols['__phase_sim_base_size'] + symbols['__phase_enemy_size']
assert symbols['terminal_step'] == symbols['game_terminal'] == symbols['encounter_step']
assert symbols['victory_geometry'] == symbols['medal_geometry'] == symbols['__phase_geometry_base_end']
assert symbols['__phase_geometry_base_size'] < symbols['__phase_victory_size'] <= 0x4200
enemy_profiles = ('lowlands', 'canyon', 'fortress', 'carrier', 'warden', 'citadel', 'flagship')
enemy_sections = {f'.phase.enemy.{level}' for level in enemy_profiles}
for level in enemy_profiles:
    assert symbols[f'encounter_{level}'] == symbols['encounter_step'], f'{level} ABI entry is not first'
assert symbols['scenic_decoration'] == symbols['__phase_scenery_base_end']
scenery_profiles = ('wildlife', 'transport', 'meteors', 'ascent', 'asteroid', 'moon',
                    'graveyard', 'shipyard', 'carrier', 'gate')
scenery_sections = {f'.phase.scenery.{profile}' for profile in scenery_profiles}
for profile in scenery_profiles:
    assert symbols[f'scenic_{profile}'] == symbols['scenic_decoration'], f'{profile} ABI entry is not first'
assert (symbols['__raster_tails_start'],symbols['__raster_tails_end'])==(0xe000,0xec00)
assert (symbols['__raster_masks_start'],symbols['__raster_masks_end'])==(0xec00,0xef20)
assert symbols['__raster_heads_start']>=symbols['__raster_masks_end'] and symbols['__raster_heads_end']<=0xfffa
assert symbols['__phase_pixels_start'] == 0x3000
assert 0 < symbols['__phase_pixels_prefix_size'] <= 0x3f20, 'PIXELS prefix overlaps its helpers'
assert symbols['__phase_pixels_end'] == 0x3000 + symbols['__phase_pixels_prefix_size']
assert symbols['__phase_pixels_aux_start'] == 0x6f20, 'PIXELS helpers must start at $6F20'
assert 0 < symbols['__phase_pixels_aux_size'] <= 0x2e0, 'PIXELS helpers exceed workspace'
assert symbols['__phase_pixels_aux_end'] == 0x6f20 + symbols['__phase_pixels_aux_size']
assert symbols['__phase_pixels_aux_end'] <= symbols['__irq_stack_bottom'], 'PIXELS helpers overlap IRQ stack'
assert symbols['__phase_pixels_aux_load'] == 0x43f20, 'incorrect PIXELS helper REU address'
assert symbols['__phase_pixels_size'] == 0x3f20 + symbols['__phase_pixels_aux_size'], 'incorrect composed PIXELS size'
assert symbols['__raster_player_seen']==0x77ab
assert (symbols['__raster_prepare_scratch_start'],symbols['__raster_prepare_scratch_end'])==(0x77a0,0x77bf)
assert symbols['__irq_stack']<=symbols['__raster_prepare_scratch_start']<symbols['__raster_prepare_scratch_end']<=0x7800
ranges={'.basic_header':(0x801,0x3000),'.text':(0x801,0x3000),'.data':(0x200,low_end),'.bss':(0x200,low_end),'.noinit':(0x200,low_end),'.music_page':(0x720,0x800),'.startup_checks':(0x8c00,0x9100),'.rodata':(0x7800,0x8000),'.tables':(0x9900,0xa000),'.renderer':(0xcffa,0xe000),'.game_state':(0xc800,0xca80),'.audio_state':(0xce00,0xcfb0),'.zp.data':(0x22,0xf0),'.zp.bss':(0x22,0xf0),'.zp.noinit':(0x22,0xf0)}
ranges['.benchmark_resident'] = (0x7800, 0x8000)
used=[]
phase_sizes={}
kernel_sizes={}
assert symbols['__raster_heads_end'] <= 0xff20, 'raster heads must leave the CPU vectors free'
assert symbols['__irq_stack_bottom'] == 0x7500 and symbols['__irq_stack'] == 0x7600, 'IRQ stack must be the measured 256-byte reservation'
assert 0x7200 <= symbols['__music_kernel_start'] and symbols['__music_kernel_start'] + symbols['__music_kernel_size'] <= symbols['__irq_stack_bottom'], 'music kernel slot must sit between the overlay window and the IRQ stack'
for line in listing.splitlines():
    m=re.search(r'\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-fA-F]+)\s+[0-9a-fA-F]+\s+([0-9a-fA-F]+)\s+\S+\s+(\S+)',line)
    if not m:continue
    name,address,size,flags=m.groups();address=int(address,16);size=int(size,16)
    if 'A' not in flags or not size:continue
    if name.startswith('.phase.'):
        phase_sizes[name]=size
    if name == '.phase.sim_aux':
        assert address == 0x8c00 and 0 < size <= 0x500, name
        assert size == symbols['__phase_sim_aux_size'], name
    elif name == '.phase.pixels_aux':
        assert address == 0x6f20 and 0 < size <= 0x2e0, 'PIXELS helpers outside workspace'
        assert size == symbols['__phase_pixels_aux_size'], 'PIXELS helper size mismatch'
    elif name == '.phase.victory':
        assert address == symbols['victory_geometry'] and address + size <= 0x7200, name
    elif name in enemy_sections or name == '.phase.terminal':
        assert address == symbols['encounter_step'] and 0 < size <= symbols['__phase_enemy_size'], name
        assert address + size <= 0x7200, name
    elif name in scenery_sections:
        assert address == symbols['scenic_decoration'], name
        assert 0 < size <= symbols['__phase_scenery_size'] - symbols['__phase_scenery_base_size'], name
        assert address + size <= 0x7200, name
    elif name.startswith('.phase.'):
        assert name in {'.phase.boot','.phase.sim','.phase.geometry','.phase.pixels','.phase.audio','.phase.scenery'},name
        limit=0x3f20 if name=='.phase.pixels' else 0x4200
        assert address==0x3000 and size<=limit,name
    elif name.startswith('.music.kernel.'):
        assert address == symbols['music_kernel_tick'] == symbols['__music_kernel_start'], f'{name} must link at the kernel slot'
        assert 0 < size <= symbols['__music_kernel_size'], f'{name} exceeds the kernel slot'
        kernel_sizes[name[len('.music.kernel.'):]] = size
    else:
        assert name in ranges,f'unclassified allocated section {name}'
        lo,hi=ranges[name];assert lo<=address and address+size<=hi,f'{name} outside allocation'
        for other,start,end in used:assert address+size<=start or address>=end,f'{name} overlaps {other}'
        used.append((name,address,address+size))
    print(f'{name:18} ${address:04X}-${address+size-1:04X} {size:5} bytes')
# Linker bases are the source for both packaging and the runtime address table.
# Validate the fixed v8 map independently, including each full reserved slot.
reu_layout = (
    (1, 'boot', 0x10000),
    (2, 'sim_lowlands', 0x20000),
    (3, 'geometry', 0x30000),
    (4, 'pixels', 0x40000),
    (5, 'audio', 0x50000),
    (7, 'scenery_wildlife', 0x70000),
    (8, 'sim_canyon', 0x80000),
    (9, 'sim_fortress', 0x90000),
    (10, 'sim_carrier', 0xa0000),
    (12, 'victory', 0x15000),
    (13, 'sim_terminal', 0x1a000),
    (18, 'sim_warden', 0x25000),
    (19, 'sim_citadel', 0x2a000),
    (20, 'sim_flagship', 0x35000),
    (21, 'scenery_transport', 0x3a000),
    (22, 'scenery_meteors', 0x55000),
    (23, 'sim_helpers', 0x5a000),
    (24, 'scenery_ascent', 0x60000),
    (25, 'scenery_asteroid', 0x65000),
    (26, 'scenery_moon', 0x6a000),
    (27, 'scenery_graveyard', 0x75000),
    (28, 'scenery_shipyard', 0x7a000),
    (29, 'scenery_carrier', 0x85000),
    (30, 'scenery_gate', 0x8a000),
)
assert symbols['__reu_size'] == 0x100000, 'incorrect 1 MiB REU image size'
assert symbols['__reu_code_slot_size'] == 0x4200, 'incorrect REU code slot size'
assert symbols['__reu_models'] == 0xb0000, 'incorrect REU model bank address'
assert symbols['__reu_music'] == 0xc0000, 'incorrect REU music bank address'
reu_allocations = [(0, 0x180, 'directories'), (0x1000, 0x1c00, 'common assets'),
                   (0x2000, 0xb000, 'encounters'), (0xb0000, 0xb9000, 'models'),
                   (symbols['__reu_music_kernels'], symbols['__reu_music_kernels'] + 0x100 + 21 * symbols['__music_kernel_size'], 'music kernels'),
                   (0xc0000, 0x100000, 'music')]
assert symbols['__reu_music_kernels'] == 0xb9000, 'incorrect REU music kernel address'
for phase_id, name, expected in reu_layout:
    address = symbols[f'__reu_{name}']
    end = address + symbols['__reu_code_slot_size']
    assert 0 <= address < end <= symbols['__reu_size'], f'{name} REU slot exceeds image'
    assert address & 0xfff == 0, f'{name} REU slot must be 4 KiB aligned'
    assert (address & 0xffff) + 0x4200 <= 0x10000, f'{name} REU slot crosses a 64 KiB bank'
    for start, stop, owner in reu_allocations:
        assert end <= start or address >= stop, f'{name} REU slot overlaps {owner}'
    reu_allocations.append((address, end, name))
for phase_id, name, expected in reu_layout:
    assert symbols[f'__reu_{name}'] == expected, f'incorrect {name} REU slot address'

reu_table = symbols['__code_reu_pages']
assert symbols['__code_reu_pages_end'] - reu_table == 31, 'incorrect REU address table size'
assert any(name == '.tables' and start <= reu_table and reu_table + 31 <= end
           for name,start,end in used), 'REU address table must remain in resident tables'
reu_table_offset = reu_table - 0x0801 + 2
reu_pages = data[reu_table_offset:reu_table_offset+31]
expected_reu_pages = bytearray(31)
for phase_id, name, expected in reu_layout:
    expected_reu_pages[phase_id] = symbols[f'__reu_{name}'] >> 12
assert reu_pages == expected_reu_pages, 'REU address table disagrees with linked slots'

# ELF physical addresses must match the bases used by the packer. Virtual
# addresses deliberately overlap because all these code images share RAM.
program_offset = struct.unpack_from('<I', metadata.data, 28)[0]
program_stride, program_count = struct.unpack_from('<HH', metadata.data, 42)
assert program_stride == 32 and program_count > 0, 'missing ELF load addresses'
assert program_offset + program_stride * program_count <= len(metadata.data), 'truncated ELF load addresses'
programs = [struct.unpack_from('<8I', metadata.data, program_offset + i * program_stride)
            for i in range(program_count)]
phase_loads = {
    '.phase.boot': symbols['__reu_boot'],
    '.phase.sim': symbols['__reu_sim_lowlands'],
    '.phase.geometry': symbols['__reu_geometry'],
    '.phase.pixels': symbols['__reu_pixels'],
    '.phase.pixels_aux': symbols['__reu_pixels'] + 0x3f20,
    '.phase.audio': symbols['__reu_audio'],
    '.phase.scenery': symbols['__reu_scenery_wildlife'],
    '.phase.sim_aux': symbols['__reu_sim_helpers'],
    '.phase.terminal': symbols['__reu_sim_terminal'] + symbols['__phase_sim_base_size'],
    '.phase.victory': symbols['__reu_victory'] + symbols['__phase_geometry_base_size'],
}
phase_loads.update({f'.phase.enemy.{name}': symbols[f'__reu_sim_{name}'] + symbols['__phase_sim_base_size']
                    for name in enemy_profiles})
phase_loads.update({f'.phase.scenery.{name}': symbols[f'__reu_scenery_{name}'] + symbols['__phase_scenery_base_size']
                    for name in scenery_profiles})
for section in metadata.sections:
    if section.name not in phase_loads:
        continue
    loads = [physical + section.offset - offset
             for kind, offset, virtual, physical, file_size, memory_size, flags, alignment in programs
             if kind == 1 and offset <= section.offset
             and section.offset + section.size <= offset + file_size
             and virtual + section.offset - offset == section.address]
    assert loads == [phase_loads[section.name]], f'{section.name} ELF load address disagrees with REU slot'

assert phase_sizes['.phase.sim'] == symbols['__phase_sim_base_size']
assert phase_sizes['.phase.pixels'] == symbols['__phase_pixels_prefix_size']
assert symbols['__phase_enemy_size'] == max(phase_sizes[name] for name in enemy_sections | {'.phase.terminal'})
load_table = symbols['__sim_load_sizes']
assert any(name == '.tables' and start <= load_table and load_table + 16 <= end
           for name,start,end in used), 'SIM load table must remain in resident tables'
table_offset = load_table - 0x0801 + 2
sim_load_sizes = tuple(int.from_bytes(data[table_offset+i:table_offset+i+2], 'little')
                       for i in range(0,16,2))
expected_load_sizes = tuple(phase_sizes['.phase.sim'] + phase_sizes[name]
                            for name in [*(f'.phase.enemy.{level}' for level in enemy_profiles), '.phase.terminal'])
assert sim_load_sizes == expected_load_sizes, f'SIM load table disagrees with profile images: {sim_load_sizes} != {expected_load_sizes}'
code_table = symbols['__code_sizes']
assert any(name == '.startup_checks' and start <= code_table and code_table + 48 <= end
           for name,start,end in used), 'Code size table must remain in startup checks'
code_offset = code_table - 0x0801 + 2
code_sizes = tuple(int.from_bytes(data[code_offset+i:code_offset+i+2], 'little')
                   for i in range(0,48,2))
# This order is the startup OVL7 walk: five original phases, wildlife,
# three ordinary SIM profiles, victory, terminal, three bosses, two scenic
# tails, then the separately addressed SIM helpers, ascent and six space tails.
code_profiles = ('boot', 'sim', 'geometry', 'pixels', 'audio', 'scenery',
                 'sim', 'sim', 'sim', 'victory', 'sim', 'sim', 'sim', 'sim',
                 'scenery', 'scenery', 'sim_aux', *(['scenery'] * 7))
expected_code_sizes = tuple(symbols[f'__phase_{profile}_size'] for profile in code_profiles)
assert code_sizes == expected_code_sizes, f'Code size table disagrees with overlay images: {code_sizes} != {expected_code_sizes}'
assert phase_sizes['.phase.scenery'] == symbols['__phase_scenery_base_size']
assert symbols['__phase_scenery_size'] == symbols['__phase_scenery_base_size'] + max(phase_sizes[name] for name in scenery_sections)
print(f'PRG {len(data):,} bytes; resident headroom {0x3000-symbols["__resident_end"]} bytes')
print(f'Music profile: {4 if four_sid else 2} SIDs; extra score page {224 if four_sid else 0} bytes')
if kernel_sizes:
    largest = max(kernel_sizes, key=kernel_sizes.get)
    print(f'Music kernels: {len(kernel_sizes)} per-song images in the {symbols["__music_kernel_size"]}-byte slot at ${symbols["__music_kernel_start"]:04X}; largest {largest} {kernel_sizes[largest]} bytes; REU ${symbols["__reu_music_kernels"]:06X}')
    for name in sorted(kernel_sizes):
        print(f'  kernel {name:18} {kernel_sizes[name]:4} bytes')
else:
    print(f'Music kernels: none linked; {symbols["__music_kernel_size"]}-byte slot at ${symbols["__music_kernel_start"]:04X} reserved')
print(f'Validated music directory: ${music_directory.address:04X}-${music_directory.address+music_directory.size-1:04X}, {music_directory.size} BSS bytes outside IRQ-saved compiler frames')
print('Startup checks8C00-90FF: retired before BOOT asset loading and rendering')
print('VIC bank 2: screen8000 bitmapA000-BF3F, guarded staged band commits; C000-C3FF quarter squares; E000-EF1F raster tails/masks; EF20-FF1F raster heads; FF20-FFF9 free RAM')
print('Models8400-8BFF; overflow-cell pixel band8C00-90FF; GEOMETRY kernel scratch9100-91FF (PIXELS cell scratch9100-9127); cell codes9200-933F, cover9340-947F, palettes9480-94F7; segments9500-98FF')
print('Overlay3000-71FF; music kernel slot7200-74FF; main stackC400-C7FF; IRQ stack7500-75FF')
print('PIXELS code3000-6F1F helpers6F20-71FF; sorted order7600-76FF; edge headsEF20-FF1F tailsE000-EBFF masksEC00-EF1F; commands stay in emission orderD000-DFFF')
print(f'PIXELS prefix {symbols["__phase_pixels_prefix_size"]} bytes; helpers {symbols["__phase_pixels_aux_size"]} bytes; checksummed composition {symbols["__phase_pixels_size"]} bytes; runtime omits the raster-data gap')
print('GEOMETRY code3000-71FF; projection cache8C00-8E7F and depth-sort keys/buckets8C00-8FFF share PIXELS band; sorted order7600-76FF')
print('SCENERY code3000-71FF; raw recordsD880-DFFF share GEOMETRY commands')
print(f'SCENERY terrestrial prefix {symbols["__phase_scenery_terrain_size"]} bytes; complete prefix {symbols["__phase_scenery_base_size"]} bytes; largest decoration tail {symbols["__phase_scenery_size"]-symbols["__phase_scenery_base_size"]} bytes; ten padded images')
print(f'GEOMETRY transition prefix {symbols["__phase_geometry_base_size"]} bytes; combat tail {symbols["__phase_geometry_size"]-symbols["__phase_geometry_base_size"]} bytes')
print(f'AUDIO runtime prefix {symbols["__phase_audio_runtime_size"]} bytes; startup-only validation tail {symbols["__phase_audio_size"]-symbols["__phase_audio_runtime_size"]} bytes; startup verifies the full image')
print(f'SIM common prefix {symbols["__phase_sim_base_size"]} bytes; largest simulation tail {symbols["__phase_enemy_size"]} bytes; eight padded images {symbols["__phase_sim_size"]} bytes each')
print('SIM runtime transfer sizes by profile0..7: ' + ', '.join(map(str, sim_load_sizes)))
print('OVL7 startup code sizes by directory order: ' + ', '.join(map(str, code_sizes)))
print('REU 1 MiB: 744,832 reserved bytes; 303,744 unreserved bytes; music $0C0000-$0FFFFF')
print('REU logical phase slots: ' + ', '.join(f'{phase_id:02X}=${symbols[f"__reu_{name}"]:06X}' for phase_id, name, expected in reu_layout))
print(f'SIM helper code8C00-90FF: {symbols["__phase_sim_aux_size"]} bytes; reloaded before every SIM profile, shares retired startup/cache/band workspace')
print(f'Victory GEOMETRY composed image {symbols["__phase_victory_size"]} bytes')

print('PIXELS preparation scratch77A0-77BE: reused later by raster/packer; models remain intact')
if span_enabled:
    print('Span buffer: nextD000-D4FF inkD500-D9FF; retired commands after all edge preparation; no additional persistent RAM')
print('PIXELS player coverage flag77AB: reset per band, persistent until the band finish')
print('PIXELS cell tables: HUD slots $7700-$7727, overflow flags $7728-$774F, cover-clean marker $7750; cell writer zero page $F0-$FF; row walker zero page $D0-$E0')
print('PIXELS staging consumes cover9340-947F, HUD7700-7727, overflow7728-774F; next band clears coverage; raster-safe copy publishes 400 bytes')
