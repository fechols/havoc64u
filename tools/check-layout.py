"""Reject allocated sections outside the fixed C64 ownership map."""
import os, pathlib, re, subprocess, sys
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
assert (symbols['__raster_heads_start'],symbols['__raster_heads_end'])==(0xd000,0xe000)
assert (symbols['__scenery_records_start'],symbols['__scenery_records_end'])==(0xd880,0xe000)
assert 0 < symbols['__phase_scenery_base_size'] < symbols['__phase_scenery_size'] <= 0x4200
assert symbols['__phase_scenery_base_end'] == 0x3000 + symbols['__phase_scenery_base_size']
assert 0 < symbols['__phase_geometry_base_size'] < symbols['__phase_geometry_size'] <= 0x4200
assert symbols['__phase_geometry_base_end'] == 0x3000 + symbols['__phase_geometry_base_size']
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
scenery_profiles = ('wildlife', 'transport', 'meteors')
scenery_sections = {f'.phase.scenery.{profile}' for profile in scenery_profiles}
for profile in scenery_profiles:
    assert symbols[f'scenic_{profile}'] == symbols['scenic_decoration'], f'{profile} ABI entry is not first'
assert (symbols['__raster_tails_start'],symbols['__raster_tails_end'])==(0x6000,0x6c00)
assert (symbols['__raster_masks_start'],symbols['__raster_masks_end'])==(0x6c00,0x6f20)
assert symbols['__raster_masks_end']<=symbols['__irq_stack_bottom']
assert symbols['__raster_player_seen']==0x77ab
assert (symbols['__raster_prepare_scratch_start'],symbols['__raster_prepare_scratch_end'])==(0x77a0,0x77bf)
assert symbols['__irq_stack']<=symbols['__raster_prepare_scratch_start']<symbols['__raster_prepare_scratch_end']<=0x7800
ranges={'.basic_header':(0x801,0x3000),'.text':(0x801,0x3000),'.data':(0x200,low_end),'.bss':(0x200,low_end),'.noinit':(0x200,low_end),'.music_page':(0x720,0x800),'.startup_checks':(0x8c00,0x9100),'.rodata':(0x7800,0x8000),'.tables':(0x9900,0xa000),'.renderer':(0xcffa,0xe000),'.game_state':(0xc800,0xca80),'.audio_state':(0xce00,0xcfb0),'.zp.data':(0x22,0x100),'.zp.bss':(0x22,0x100),'.zp.noinit':(0x22,0x100)}
ranges['.benchmark_resident'] = (0x7800, 0x8000)
used=[]
phase_sizes={}
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
        limit=0x3000 if name=='.phase.pixels' else 0x4200
        assert address==0x3000 and size<=limit,name
    else:
        assert name in ranges,f'unclassified allocated section {name}'
        lo,hi=ranges[name];assert lo<=address and address+size<=hi,f'{name} outside allocation'
        for other,start,end in used:assert address+size<=start or address>=end,f'{name} overlaps {other}'
        used.append((name,address,address+size))
    print(f'{name:18} ${address:04X}-${address+size-1:04X} {size:5} bytes')
assert phase_sizes['.phase.sim'] == symbols['__phase_sim_base_size']
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
assert any(name == '.startup_checks' and start <= code_table and code_table + 34 <= end
           for name,start,end in used), 'Code size table must remain in startup checks'
code_offset = code_table - 0x0801 + 2
code_sizes = tuple(int.from_bytes(data[code_offset+i:code_offset+i+2], 'little')
                   for i in range(0,34,2))
# This order is the startup OVL6 walk: five original phases, wildlife,
# three ordinary SIM profiles, victory, terminal, three bosses, two scenic
# tails, then the separately addressed SIM helpers.
code_profiles = ('boot', 'sim', 'geometry', 'pixels', 'audio', 'scenery',
                 'sim', 'sim', 'sim', 'victory', 'sim', 'sim', 'sim', 'sim',
                 'scenery', 'scenery', 'sim_aux')
expected_code_sizes = tuple(symbols[f'__phase_{profile}_size'] for profile in code_profiles)
assert code_sizes == expected_code_sizes, f'Code size table disagrees with overlay images: {code_sizes} != {expected_code_sizes}'
assert phase_sizes['.phase.scenery'] == symbols['__phase_scenery_base_size']
assert symbols['__phase_scenery_size'] == symbols['__phase_scenery_base_size'] + max(phase_sizes[name] for name in scenery_sections)
print(f'PRG {len(data):,} bytes; resident headroom {0x3000-symbols["__resident_end"]} bytes')
print(f'Music profile: {4 if four_sid else 2} SIDs; extra score page {224 if four_sid else 0} bytes')
print('Startup checks8C00-90FF: retired before BOOT asset loading and rendering')
print('VIC A: screen8000 bitmapA000-BF3F; VIC B: screenC000 bitmapE000-FF3F')
print('Models8400-8BFF; band8C00-90FF; color9100-94FF; segments9500-98FF')
print('Overlay3000-71FF; main stackC400-C7FF; IRQ stack7200-75FF')
print('PIXELS code3000-5FFF; edge headsD000-DFFF tails6000-6BFF masks6C00-6F1F')
print('GEOMETRY code3000-71FF; projection/sort cache8C00-8E7F shares PIXELS band')
print('SCENERY code3000-71FF; raw recordsD880-DFFF share GEOMETRY commands')
print(f'SCENERY ordinary prefix {symbols["__phase_scenery_base_size"]} bytes; largest decoration tail {symbols["__phase_scenery_size"]-symbols["__phase_scenery_base_size"]} bytes; three padded images')
print(f'GEOMETRY transition prefix {symbols["__phase_geometry_base_size"]} bytes; combat tail {symbols["__phase_geometry_size"]-symbols["__phase_geometry_base_size"]} bytes')
print(f'SIM common prefix {symbols["__phase_sim_base_size"]} bytes; largest simulation tail {symbols["__phase_enemy_size"]} bytes; eight padded images {symbols["__phase_sim_size"]} bytes each')
print('SIM runtime transfer sizes by profile0..7: ' + ', '.join(map(str, sim_load_sizes)))
print('OVL6 startup code sizes by directory order: ' + ', '.join(map(str, code_sizes)))
print(f'SIM helper code8C00-90FF: {symbols["__phase_sim_aux_size"]} bytes; reloaded before every SIM profile, shares retired startup/cache/band workspace')
print(f'Victory GEOMETRY composed image {symbols["__phase_victory_size"]} bytes')

print('PIXELS preparation scratch77A0-77BE: reused later by raster/packer; models remain intact')
print('PIXELS player coverage flag77AB: reset per band, persistent until packing')
