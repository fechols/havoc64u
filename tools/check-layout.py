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
assert symbols['__renderer_start']==0xcffa and symbols['__renderer_end']==0xe000
assert symbols['__irq_static_save_size']<=1088
assert symbols['__pixel_band_start']==0x8c00 and symbols['__pixel_band_end']==0x9100
assert symbols['__projection_cache_start']==0x8c00 and symbols['__projection_cache_end']==0x8e80
assert symbols['__projection_cache_end']<=symbols['__pixel_band_end']
assert symbols['__projection_cache_start']+624<=symbols['__projection_cache_end']
assert (symbols['__raster_heads_start'],symbols['__raster_heads_end'])==(0xd000,0xe000)
assert (symbols['__raster_tails_start'],symbols['__raster_tails_end'])==(0x6000,0x6c00)
assert (symbols['__raster_masks_start'],symbols['__raster_masks_end'])==(0x6c00,0x6f20)
assert symbols['__raster_masks_end']<=symbols['__irq_stack_bottom']
ranges={'.basic_header':(0x801,0x3000),'.text':(0x801,0x3000),'.data':(0x200,0x800),'.bss':(0x200,0x800),'.noinit':(0x200,0x800),'.rodata':(0x7800,0x8000),'.tables':(0x9900,0xa000),'.renderer':(0xcffa,0xe000),'.game_state':(0xc800,0xca80),'.audio_state':(0xce00,0xcfb0),'.zp.data':(0x22,0x100),'.zp.bss':(0x22,0x100),'.zp.noinit':(0x22,0x100)}
used=[]
for line in listing.splitlines():
    m=re.search(r'\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-fA-F]+)\s+[0-9a-fA-F]+\s+([0-9a-fA-F]+)\s+\S+\s+(\S+)',line)
    if not m:continue
    name,address,size,flags=m.groups();address=int(address,16);size=int(size,16)
    if 'A' not in flags or not size:continue
    if name.startswith('.phase.'):
        assert name in {'.phase.boot','.phase.sim','.phase.geometry','.phase.pixels'},name
        limit=0x3000 if name=='.phase.pixels' else 0x4200
        assert address==0x3000 and size<=limit,name
    else:
        assert name in ranges,f'unclassified allocated section {name}'
        lo,hi=ranges[name];assert lo<=address and address+size<=hi,f'{name} outside allocation'
        for other,start,end in used:assert address+size<=start or address>=end,f'{name} overlaps {other}'
        used.append((name,address,address+size))
    print(f'{name:18} ${address:04X}-${address+size-1:04X} {size:5} bytes')
print(f'PRG {len(data):,} bytes; resident headroom {0x3000-symbols["__resident_end"]} bytes')
print('VIC A: screen8000 bitmapA000-BF3F; VIC B: screenC000 bitmapE000-FF3F')
print('Models8400-8BFF; band8C00-90FF; color9100-94FF; segments9500-98FF')
print('Overlay3000-71FF; main stackC400-C7FF; IRQ stack7200-75FF')
print('PIXELS code3000-5FFF; edge headsD000-DFFF tails6000-6BFF masks6C00-6F1F')
print('GEOMETRY code3000-71FF; projection/sort cache8C00-8E7F shares PIXELS band')
