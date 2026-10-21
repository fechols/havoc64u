#!/usr/bin/env python3
"""Differentially exercise native span composition, raster handoff and cell flush.

Run in the configured LLVM-MOS environment. Generated files and the report stay
under target/spans-test. A byte-per-pixel oracle shares no run-list algorithm;
the existing pack-cell reference checks decoded output with identical old
attributes, rather than claiming palette-history parity between renderers.
"""
import hashlib
import json
import os
import pathlib
import re
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/spans-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", pathlib.Path.home()/".local/llvm-mos")) / "bin"
assemblies = [root / "target-c64/asm" / name for name in
              ("spans.S", "cells.S", "pack.S", "raster.S")]
env = dict(os.environ, HAVOC_RASTER_FIXTURES=str(build / "fixtures.h"))
subprocess.run(["cargo", "test", "--lib", "render::tests::export_native_raster_fixtures",
                "--", "--ignored", "--exact"], cwd=root, env=env, check=True)

# Reuse only the independent quantizer oracle, never the CellBand/run-list model.
cell_test = (root / "tools/test-cells-kernel.py").read_text(encoding="utf-8")
quantizer = cell_test.split("static void pack_cell(", 1)[1].split("static void m_finish(", 1)[0]
quantizer = "static void pack_cell(" + quantizer
rgb = re.search(r"static const uint8_t PALETTE_RGB\[16\]\[3\]=.*?;", cell_test).group()
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Head { int32_t long_x,long_step,short_x,short_step; };
struct Tail { int32_t lower_x,lower_step; uint8_t top,middle,bottom,flags; };
struct Fixture { struct Head head; struct Tail tail; };
struct Job { struct Head *head; const struct Tail *tail; uint8_t *pixels; uint8_t band,paint; };
extern void fast_raster_rows(const struct Job *);
extern void fast_span_clear(void);
extern void fast_span_plot(uint8_t row,uint8_t x,uint8_t color);
extern void fast_span_flush(void);
extern void test_compose(uint8_t row,uint8_t first,uint8_t end,uint8_t color,uint8_t mode);
extern void fast_cell_clear(uint8_t color);
extern void fast_cell_plot(uint8_t row,uint8_t x,uint8_t color);
extern void fast_cell_stage(const uint8_t *old_screen,const uint8_t *old_colors);
#include "fixtures.h"
#define K_NEXT ((uint8_t*)0xd000)
#define K_INK ((uint8_t*)0xd500)
#define ORACLE ((uint8_t*)0xe000)
#define WRITES ((uint8_t*)0xe500)
#define K_CODES ((uint8_t*)0xa800)
#define K_COVER ((uint8_t*)0xa940)
#define K_PAL ((uint8_t*)0xaa80)
#define K_HUD ((uint8_t*)0x9f00)
#define K_OVER ((uint8_t*)0x9f28)
#define K_PIXELS ((uint8_t*)0xb000)
#define K_SEEN (*(volatile uint8_t*)0x9f6b)
#define RECORD (*(volatile uint8_t*)0xab40)
#define BAD_ABI (*(volatile uint8_t*)0xab41)
#define COLOR_DISTANCE (*(uint16_t (*)[16][16])0xa400)
#define PRESENT ((uint8_t*)0xa600)
static uint8_t background;
static uint32_t checks, submitted, emitted, raw_pixels, visible_pixels;
static uint32_t state=0x81559a37u;
static uint32_t random_word(void){state=state*1664525u+1013904223u;return state;}
static void oracle_span(uint8_t row,uint8_t first,uint8_t end,uint8_t color,uint8_t mode){
  for(uint8_t x=first;x<end;x++){
    uint8_t *pixel=ORACLE+row*160+x;
    if(mode==1)*pixel=(color&15)|0x40;
    else if(mode!=2||!(*pixel&0x40))*pixel=(color&15)|(*pixel&0x40);
  }
}
static void clear(uint8_t bg){
  background=bg; RECORD=0; BAD_ABI=0;
  memset((void*)0xcfe0,0xa5,32); memset((void*)0xda00,0x5a,32);
  fast_cell_clear(bg); fast_span_clear(); K_SEEN=0;
  memset(ORACLE,bg,1280);
}
static int check_runs(void){
  checks++;
  if(BAD_ABI){puts("FAIL compositor clobbered caller state");return 1;}
  for(uint8_t i=0;i<32;i++)if(((uint8_t*)0xcfe0)[i]!=0xa5||((uint8_t*)0xda00)[i]!=0x5a){puts("FAIL span storage guard");return 1;}
  for(uint8_t row=0;row<8;row++){
    uint8_t x=0, previous=0xff; uint16_t base=row*160;
    while(x<160){
      uint8_t end=K_NEXT[base+x],ink=K_INK[base+x];
      if(end<=x||end>160||(ink&0xb0)||ink==previous){printf("FAIL invalid run row %u x %u end %u ink %02x previous %02x\n",row,x,end,ink,previous);return 1;}
      for(uint8_t p=x;p<end;p++)if(ORACLE[base+p]!=ink){printf("FAIL pixels check %lu row %u x %u got %02x want %02x\n",(unsigned long)checks,row,p,ink,ORACLE[base+p]);return 1;}
      previous=ink;x=end;
    }
  }
  return 0;
}
static void compose(uint8_t row,uint8_t first,uint8_t end,uint8_t color,uint8_t mode){
  /* The internal kernel receives nonempty spans; the raster owns clipping. */
  if(first>=end)return;
  test_compose(row,first,end,color,mode);oracle_span(row,first,end,color,mode);
  submitted++;raw_pixels+=end-first;
}
static uint8_t decode(uint8_t code,uint8_t screen,uint8_t color){
  return code==0?background:code==1?screen>>4:code==2?screen&15:color&15;
}
static int check_flushed(void){
  memset(WRITES,0,1280); RECORD=1; fast_span_flush(); RECORD=0;
  for(uint8_t row=0;row<8;row++)for(uint8_t x=0;x<160;x++){
    uint16_t at=row*160+x; uint8_t cell=x/4,shift=6-2*(x&3);
    if(WRITES[at]>1){printf("FAIL flush overdraw row %u x %u count %u\n",row,x,WRITES[at]);return 1;}
    if(ORACLE[at]!=background&&WRITES[at]!=1){puts("FAIL missing visible/protected flush pixel");return 1;}
    visible_pixels+=WRITES[at];
    uint8_t actual;
    if(K_OVER[cell])actual=K_PIXELS[at]&0x4f;
    else {uint8_t slot=(K_CODES[row*40+cell]>>shift)&3;actual=slot?(K_PAL[(slot-1)*40+cell]^background):background;if(K_COVER[row*40+cell]&(8>>(x&3)))actual|=0x40;}
    if(actual!=ORACLE[at]){printf("FAIL flush color/coverage row %u x %u got %02x want %02x\n",row,x,actual,ORACLE[at]);return 1;}
  }
  emitted++; return check_runs();
}
'''
source += rgb + "\n" + quantizer + r'''
static int check_quantized(uint8_t hud){
  uint8_t old_screen[40],old_color[40],bitmap[8],screen,color;
  /* HUD follows the final flush; marked ink remains visible in this scenario. */
  if(hud)for(uint8_t row=0;row<8;row++){
    uint8_t x=(uint8_t)(row*17);
    fast_cell_plot(row,x,0x87);ORACLE[row*160+x]=0x87;
  }
  for(uint8_t cell=0;cell<40;cell++){old_screen[cell]=(uint8_t)(random_word()>>8);old_color[cell]=(uint8_t)((random_word()>>8)&15);}
  fast_cell_stage(old_screen,old_color);
  for(uint8_t cell=0;cell<40;cell++){
    uint8_t old[3]={old_screen[cell]>>4,old_screen[cell]&15,old_color[cell]};
    pack_cell(ORACLE,cell,old,background,bitmap,&screen,&color);
    for(uint8_t row=0;row<8;row++)for(uint8_t col=0;col<4;col++){
      uint8_t shift=6-2*col;
      uint8_t got=decode((K_COVER[cell*8+row]>>shift)&3,K_HUD[cell],K_OVER[cell]);
      uint8_t want=decode((bitmap[row]>>shift)&3,screen,color);
      if(got!=want){printf("FAIL decoded cell %u row %u col %u got %u want %u\n",cell,row,col,got,want);return 1;}
    }
  }
  return 0;
}
static uint8_t clamp(int32_t x){x=(x+32767L)>>16;return x<0?0:x>160?160:(uint8_t)x;}
static void reference_rows(struct Head *h,const struct Tail *t,uint8_t band,uint8_t paint){
  uint8_t first=t->top>band?t->top:band,end=t->bottom<band+8?t->bottom:band+8;
  for(uint8_t row=first;row<end;row++){
    if(row==t->middle){h->short_x=t->lower_x;h->short_step=t->lower_step;}
    uint8_t left=clamp((t->flags&16)?h->long_x:h->short_x),right=clamp((t->flags&16)?h->short_x:h->long_x);
    if(paint&&left<right)oracle_span(row-band,left,right,t->flags&15,(t->flags&32)?1:(t->flags&64)?2:0);
    h->long_x+=h->long_step;h->short_x+=h->short_step;
  }
}
int main(void){
  for(uint16_t b=0;b<256;b++)PRESENT[b]=(uint8_t)((1<<(b>>6))|(1<<((b>>4)&3))|(1<<((b>>2)&3))|(1<<(b&3)));
  for(uint8_t a=0;a<16;a++)for(uint8_t b=0;b<16;b++){int32_t r=PALETTE_RGB[a][0]-PALETTE_RGB[b][0],g=PALETTE_RGB[a][1]-PALETTE_RGB[b][1],bl=PALETTE_RGB[a][2]-PALETTE_RGB[b][2];COLOR_DISTANCE[a][b]=(uint16_t)((r*r+2*g*g+bl*bl)/4);}
  *(uint8_t*)0x9f50=0;
  /* Empty/end-sentinel spans, protected background, persistent coverage and
     clear winning when a triangle carries both role flags. */
  for(uint8_t bg=0;bg<16;bg++){
    clear(bg);compose(0,0,0,3,0);compose(7,160,160,3,0);
    compose(0,0,160,1,0);compose(0,3,159,bg,1);compose(0,9,143,8,0);
    compose(0,0,160,5,2);compose(7,159,160,15,1);
    if(check_runs()||check_flushed()||check_quantized(1))return 1;
  }
  /* The maximal representation: every pixel starts a separate run, all rows. */
  clear(0);
  for(uint8_t row=0;row<8;row++)for(uint8_t x=0;x<160;x++)compose(row,x,x+1,1+(x&1),x&1);
  if(check_runs()||check_flushed()||check_quantized(1))return 2;
  clear(6);
  for(uint8_t row=0;row<8;row++){
    for(uint8_t x=0;x<160;x++)compose(row,x,x+1,(x&1)?2:1,0);
    compose(row,0,160,6,0);
    if(K_NEXT[row*160]!=160){puts("FAIL full row did not coalesce");return 2;}
  }
  if(check_runs()||check_flushed()||check_quantized(0))return 2;
  for(uint8_t band=0;band<32;band++){
    clear((uint8_t)((random_word()>>8)&15));
    /* Ambient plots enter the same buffer before ordinary geometry. */
    for(uint8_t i=0;i<24;i++){
      uint8_t row=(uint8_t)((random_word()>>8)&7),x=(uint8_t)((random_word()>>8)%160),color=(uint8_t)((random_word()>>8)&15);
      fast_span_plot(row,x,color);oracle_span(row,x,x+1,color,0);
    }
    for(uint8_t op=0;op<120;op++){
      uint8_t row=(uint8_t)((random_word()>>8)&7),first=(uint8_t)((random_word()>>8)%161),end=(uint8_t)((random_word()>>8)%161);
      if(first>end){uint8_t swap=first;first=end;end=swap;}
      compose(row,first,end,(uint8_t)((random_word()>>8)&15),(uint8_t)((random_word()>>8)%3));
      if(check_runs()){printf(" random band %u op %u\n",band,op);return 3;}
    }
    if(check_flushed()||check_quantized(1))return 3;
  }
  printf("PASS random spans, 160-run rows, coalescing, coverage, ambient, guards and decoded flush\n");
  /* Real raster entry, all 256 production Q16 fixtures and every band. */
  for(uint16_t f=0;f<sizeof(fixtures)/sizeof(fixtures[0]);f++){
    struct Head actual=fixtures[f].head,expected=actual;
    struct Tail tail=fixtures[f].tail,reference=tail;
    actual.long_x+=32767L;actual.short_x+=32767L;tail.lower_x+=32767L;
    for(uint8_t band=0;band<25;band++){
      clear((uint8_t)(f&15));uint8_t paint=(band%3)!=1;
      struct Job job={&actual,&tail,K_PIXELS,(uint8_t)(band*8),paint};
      fast_raster_rows(&job);reference_rows(&expected,&reference,band*8,paint);
      if(tail.bottom>band*8+8&&(actual.long_x-32767L!=expected.long_x||actual.short_x-32767L!=expected.short_x||actual.short_step!=expected.short_step)){printf("FAIL persistent head fixture %u band %u\n",f,band);return 4;}
      if(check_runs()||check_flushed()){printf(" fixture %u band %u\n",f,band);return 4;}
    }
  }
  printf("PASS 256 fixtures over 25 bands, %lu checks, %lu submitted spans, %lu flushes, %lu source pixels, %lu flushed pixels\n",(unsigned long)checks,(unsigned long)submitted,(unsigned long)emitted,(unsigned long)raw_pixels,(unsigned long)visible_pixels);
  return 0;
}
'''
(build / "check.c").write_text(source, encoding="utf-8")

# Instrument only the writer boundary; the actual cell writer is unchanged.
cells = assemblies[1].read_text(encoding="utf-8")
(build / "cells.S").write_text(re.sub(r"\bcell_rows\b", "real_cell_rows", cells), encoding="utf-8")
wrapper = r'''
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.text
.globl test_compose
test_compose:
    sta $f0
    stx $f1
    lda __rc2
    sta $f2
    lda __rc3
    sta $f3
    lda __rc4
    sta $f5
    ldy #2
1:  lda $00,y
    sta $b580,y
    iny
    cpy #32
    bne 1b
    ldy #0
2:  lda $d0,y
    sta $b5a0,y
    iny
    cpy #17
    bne 2b
    jsr span_compose
    ldy #2
3:  lda $00,y
    cmp $b580,y
    bne 5f
    iny
    cpy #32
    bne 3b
    ldy #0
4:  lda $d0,y
    cmp $b5a0,y
    bne 5f
    iny
    cpy #17
    bne 4b
    rts
5:  lda #1
    sta $ab41
    rts
.globl cell_rows
cell_rows:
    pha
    txa
    pha
    tya
    pha
    lda $ab40
    beq 8f
    lda $cc
    pha
    lda $cd
    pha
    ldx $f0
    lda write_lo,x
    sta $cc
    lda write_hi,x
    sta $cd
    ldy $f1
6:  cpy $f2
    bcs 7f
    lda ($cc),y
    clc
    adc #1
    sta ($cc),y
    iny
    bne 6b
7:  pla
    sta $cd
    pla
    sta $cc
8:  pla
    tay
    pla
    tax
    pla
    jmp real_cell_rows
write_lo:
    .byte <$e500,<$e5a0,<$e640,<$e6e0,<$e780,<$e820,<$e8c0,<$e960
write_hi:
    .byte >$e500,>$e5a0,>$e640,>$e6e0,>$e780,>$e820,>$e8c0,>$e960
'''
(build / "wrapper.S").write_text(wrapper, encoding="utf-8")
defines = {"SPAN_BUFFER": 1, "HUDS": 0x9f00, "OVER": 0x9f28,
           "COVER_CLEAN": 0x9f50, "PIXELS": 0xb000, "RR_SCRATCH": 0x9f60,
           "PK_SCRATCH": 0x9fa0, "CODES": 0xa800, "COVER": 0xa940,
           "PAL0": 0xaa80, "CS_SCRATCH": 0xab00}
subprocess.run([sdk / "mos-sim-clang", "-Oz", "-mlto-zp=0",
                *(f"-Wa,-defsym,{key}={value}" for key, value in defines.items()),
                "-Wl,--defsym=COLOR_DISTANCE=0xa400", build / "check.c",
                build / "wrapper.S", assemblies[0], build / "cells.S", *assemblies[2:],
                "-Wl,-Map=" + str(build / "check.map"), "-o", build / "check"], check=True)
layout = (build / "check.map").read_text(encoding="utf-8", errors="replace")
heap = re.search(r"^\s*([0-9a-f]+)\s+[0-9a-f]+\s+[0-9a-f]+\s+\d+\s+__heap_start", layout, re.M)
if heap is None or int(heap.group(1), 16) > 0x9f00:
    raise SystemExit(f"harness image reaches {heap.group(1) if heap else '?'}: overlaps state at $9F00")
result = subprocess.run([sdk / "mos-sim", "--cycles", build / "check"],
                        text=True, capture_output=True, timeout=300)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS 256 fixtures over 25 bands" not in result.stdout:
    raise SystemExit(result.returncode or "native span check did not finish")
report = {"passed": True, "fixtures": 256, "bands_per_fixture": 25,
          "random_bands": 32, "max_runs_per_row": 160, "guard_bytes": 64,
          "flush_nonoverlap": True, "compositor_caller_state_preserved": True,
          "reference": "independent byte-per-pixel composition and Q16 traversal; decoded pack-cell oracle with identical previous attributes",
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "fixtures_sha256": hashlib.sha256((build / "fixtures.h").read_bytes()).hexdigest(),
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True,
          "simulator_output": result.stdout.strip() + " " + result.stderr.strip()}
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
