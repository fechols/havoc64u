#!/usr/bin/env python3
"""Test the assembled front-to-back renderer against independent pixel rules.

Exercises ordered multi-triangle bands, noncommitting terrain prepasses, holes,
clipping, dust, HUD overwrites, register guards and decoded palette output.
Generated files stay in target/front-spans-test.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parents[1]
build = root / 'target/front-spans-test'
build.mkdir(parents=True, exist_ok=True)
sdk = Path(os.environ.get('LLVM_MOS', Path.home()/'.local/llvm-mos'))/'bin'
assemblies = [root/'target-c64/asm'/name for name in ('front_spans.S','cells.S','pack.S','raster.S')]
subprocess.run(['cargo','test','--lib','render::tests::export_native_raster_fixtures','--','--ignored','--exact'],
               cwd=root, env=dict(os.environ, HAVOC_RASTER_FIXTURES=str(build/'fixtures.h')), check=True)
cell_test = (root/'tools/test-cells-kernel.py').read_text()
quantizer = 'static void pack_cell(' + cell_test.split('static void pack_cell(',1)[1].split('static void m_finish(',1)[0]
rgb = re.search(r'static const uint8_t PALETTE_RGB\[16\]\[3\]=.*?;',cell_test).group()
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Head { int32_t long_x,long_step,short_x,short_step; };
struct Tail { int32_t lower_x,lower_step; uint8_t top,middle,bottom,flags; };
struct Fixture { struct Head head; struct Tail tail; };
struct Job { struct Head *head; const struct Tail *tail; uint8_t *pixels; uint8_t band,paint; };
#include "fixtures.h"
extern void fast_raster_rows(const struct Job*);
extern void fast_raster_band(const uint8_t*,uint8_t*,uint8_t,uint8_t);
extern void fast_span_clear(void);
extern void fast_span_plot(uint8_t,uint8_t,uint8_t);
extern void test_front(uint8_t,uint8_t,uint8_t,uint8_t,uint8_t);
extern void test_cover(uint8_t,uint8_t,uint8_t);
extern void fast_cell_clear(uint8_t);
extern void fast_cell_plot(uint8_t,uint8_t,uint8_t);
extern void fast_cell_stage(const uint8_t*,const uint8_t*);
#define NEXT ((uint8_t*)0xd000)
#define END ((uint8_t*)0xd500)
#define HEADS ((uint8_t*)0x9f6c)
#define ORACLE ((uint8_t*)0xe000)
#define WRITES ((uint8_t*)0xe500)
#define OWNED ((uint8_t*)0xb600)
#define K_CODES ((uint8_t*)0xa800)
#define K_COVER ((uint8_t*)0xa940)
#define K_PAL ((uint8_t*)0xaa80)
#define K_HUD ((uint8_t*)0x9f00)
#define K_OVER ((uint8_t*)0x9f28)
#define K_PIXELS ((uint8_t*)0xb000)
#define MASK ((uint8_t*)0xec00)
/* Simulator C stack grows down from $FFF0, overlapping production head255.
   Relocate the heads while retaining the production base's low-byte carry. */
#define NATIVE_HEADS ((struct Head*)0x8020)
#define NATIVE_TAILS ((struct Tail*)0xc000)
#define RECORD (*(volatile uint8_t*)0xab40)
#define BAD_ABI (*(volatile uint8_t*)0xab41)
#define COLOR_DISTANCE (*(uint16_t (*)[16][16])0xa400)
#define PRESENT ((uint8_t*)0xa600)
static uint8_t background;
static uint32_t state=0x62512783u,checks,submitted,prepasses;
static uint32_t random_word(void){state=state*1664525u+1013904223u;return state;}
static void clear(uint8_t bg){
  background=bg;RECORD=0;BAD_ABI=0;
  memset((void*)0xcfe0,0xa5,32);memset((void*)0xda00,0x5a,32);
  fast_cell_clear(bg);fast_span_clear();
  memset(ORACLE,bg,1280);memset(OWNED,0,1280);memset(WRITES,0,1280);
}
static void oracle_span(uint8_t row,uint8_t first,uint8_t end,uint8_t color,uint8_t mode){
  for(uint8_t x=first;x<end;x++){
    uint16_t at=row*160+x;uint8_t *p=ORACLE+at;OWNED[at]=1;
    if(mode==1)*p=(color&15)|0x40;
    else if(mode!=2||!(*p&0x40))*p=(color&15)|(*p&0x40);
  }
}
static int check_pixels(void){
  checks++;
  if(BAD_ABI){puts("FAIL front caller registers");return 1;}
  for(uint8_t i=0;i<32;i++)if(((uint8_t*)0xcfe0)[i]!=0xa5||((uint8_t*)0xda00)[i]!=0x5a){puts("FAIL front storage guard");return 1;}
  for(uint8_t row=0;row<8;row++){
    uint16_t base=row*160;uint8_t hole=HEADS[row],previous=0;
    while(hole<160){
      uint8_t end=END[base+hole],next=NEXT[base+hole];
      if(end<=hole||end>160||next<end||next>160||hole<previous){puts("FAIL malformed holes");return 1;}
      for(uint8_t x=hole;x<end;x++)if(OWNED[base+x]){puts("FAIL owned pixel remains uncovered");return 1;}
      previous=end;hole=next;
    }
    hole=HEADS[row];
    for(uint8_t x=0;x<160;x++){
      uint16_t at=base+x;uint8_t cell=x/4,shift=6-2*(x&3);
      while(hole<160&&END[base+hole]<=x)hole=NEXT[base+hole];
      uint8_t uncovered=hole<160&&hole<=x;
      if(uncovered==OWNED[at]){puts("FAIL hole ownership mismatch");return 1;}
      if(WRITES[at]>1){puts("FAIL scene overdraw");return 1;}
      if((ORACLE[at]&15)!=background&&WRITES[at]!=1){puts("FAIL visible pixel not painted once");return 1;}
      uint8_t actual;
      if(K_OVER[cell])actual=K_PIXELS[at]&0x4f;
      else{uint8_t slot=(K_CODES[row*40+cell]>>shift)&3;actual=slot?(K_PAL[(slot-1)*40+cell]^background):background;if(K_COVER[row*40+cell]&(8>>(x&3)))actual|=0x40;}
      if(actual!=ORACLE[at]){printf("FAIL pixels check %lu row %u x %u got %02x want %02x\n",(unsigned long)checks,row,x,actual,ORACLE[at]);return 1;}
    }
  }
  return 0;
}
static uint8_t decode(uint8_t code,uint8_t screen,uint8_t color){return code==0?background:code==1?screen>>4:code==2?screen&15:color&15;}
'''+rgb+'\n'+quantizer+r'''
static int check_quantized(void){
  uint8_t old_screen[40],old_color[40],bitmap[8],screen,color;
  for(uint8_t row=0;row<8;row++){
    uint8_t x=row*17;fast_cell_plot(row,x,0x8e);ORACLE[row*160+x]=0x8e;
    fast_cell_plot(row,x,11);ORACLE[row*160+x]=11;
    fast_cell_plot(row,x+1,0x87);ORACLE[row*160+x+1]=0x87;
  }
  for(uint8_t c=0;c<40;c++){old_screen[c]=random_word()>>8;old_color[c]=(random_word()>>8)&15;}
  fast_cell_stage(old_screen,old_color);
  for(uint8_t cell=0;cell<40;cell++){
    uint8_t old[3]={old_screen[cell]>>4,old_screen[cell]&15,old_color[cell]};
    pack_cell(ORACLE,cell,old,background,bitmap,&screen,&color);
    for(uint8_t row=0;row<8;row++)for(uint8_t x=0;x<4;x++){
      uint8_t shift=6-2*x,got=decode((K_COVER[cell*8+row]>>shift)&3,K_HUD[cell],K_OVER[cell]);
      uint8_t want=decode((bitmap[row]>>shift)&3,screen,color);
      if(got!=want){printf("FAIL decoded cell %u row %u x %u got %u want %u\n",cell,row,x,got,want);return 1;}
    }
  }
  return 0;
}
struct Op{uint8_t row,first,end,color,mode;};
static struct Op ops[160];
static int exercise(uint8_t count,uint8_t bg){
  clear(bg);
  /* Dust is behind all geometry in the independent painter oracle. */
  for(uint8_t r=0;r<8;r++)oracle_span(r,r*19,r*19+1,12,0);
  for(uint8_t i=0;i<count;i++){
    struct Op *o=ops+i;if(o->first>=o->end)continue;
    oracle_span(o->row,o->first,o->end,o->color,o->mode);
    if(o->mode==1){test_cover(o->row,o->first,o->end);prepasses++;}
  }
  RECORD=1;
  for(uint8_t i=count;i;i--){struct Op *o=ops+i-1;if(o->first<o->end){test_front(o->row,o->first,o->end,o->color,o->mode);submitted++;}}
  for(uint8_t r=0;r<8;r++)fast_span_plot(r,r*19,12);
  RECORD=0;return check_pixels()||check_quantized();
}
static uint8_t clamp(int32_t x){x=(x+32767L)>>16;return x<0?0:x>160?160:(uint8_t)x;}
static void reference_rows(struct Head *h,const struct Tail *t,uint8_t band,uint8_t paint){
  uint8_t first=t->top>band?t->top:band,end=t->bottom<band+8?t->bottom:band+8;
  for(uint8_t y=first;y<end;y++){
    if(y==t->middle){h->short_x=t->lower_x;h->short_step=t->lower_step;}
    uint8_t left=clamp((t->flags&16)?h->long_x:h->short_x),right=clamp((t->flags&16)?h->short_x:h->long_x);
    if(paint&&left<right)oracle_span(y-band,left,right,t->flags&15,(t->flags&32)?1:(t->flags&64)?2:0);
    h->long_x+=h->long_step;h->short_x+=h->short_step;
  }
}
int main(void){
  for(uint16_t b=0;b<256;b++)PRESENT[b]=(1<<(b>>6))|(1<<((b>>4)&3))|(1<<((b>>2)&3))|(1<<(b&3));
  for(uint8_t a=0;a<16;a++)for(uint8_t b=0;b<16;b++){int32_t r=PALETTE_RGB[a][0]-PALETTE_RGB[b][0],g=PALETTE_RGB[a][1]-PALETTE_RGB[b][1],bl=PALETTE_RGB[a][2]-PALETTE_RGB[b][2];COLOR_DISTANCE[a][b]=(r*r+2*g*g+bl*bl)/4;}
  *(uint8_t*)0x9f50=0;
  for(uint8_t bg=0;bg<16;bg++){
    ops[0]=(struct Op){0,0,160,1,0};ops[1]=(struct Op){0,3,159,bg,1};
    ops[2]=(struct Op){0,9,143,8,0};ops[3]=(struct Op){0,0,160,5,2};
    ops[4]=(struct Op){7,159,160,bg,1};ops[5]=(struct Op){7,0,160,2,2};
    if(exercise(6,bg))return 1;
  }
  /* Alternating one-pixel claims produce the maximum 80 disjoint holes. */
  for(uint8_t row=0;row<8;row++){
    for(uint8_t x=0;x<80;x++)ops[x]=(struct Op){row,(uint8_t)(x*2),(uint8_t)(x*2+1),(uint8_t)(x&15),0};
    if(exercise(80,6))return 2;
    ops[80]=(struct Op){row,0,160,7,1};if(exercise(81,6))return 2;
  }
  for(uint8_t band=0;band<64;band++){
    for(uint8_t i=0;i<120;i++){
      struct Op *o=ops+i;o->row=(random_word()>>8)&7;o->first=(random_word()>>8)%161;o->end=(random_word()>>8)%161;
      if(o->first>o->end){uint8_t s=o->first;o->first=o->end;o->end=s;}
      o->color=(random_word()>>8)&15;o->mode=(random_word()>>8)%3;
    }
    if(exercise(120,(random_word()>>8)&15))return 3;
  }
  /* Eight live indices span every mask-byte boundary, including index 255. */
  static const uint8_t indices[8]={0,1,7,8,31,63,127,255};
  struct Head expected[8];
  for(uint8_t group=0;group<32;group++){
    for(uint8_t i=0;i<8;i++){
      uint8_t index=indices[i];struct Fixture f=fixtures[group*8+i];
      expected[i]=f.head;f.head.long_x+=32767L;f.head.short_x+=32767L;f.tail.lower_x+=32767L;
      NATIVE_HEADS[index]=f.head;NATIVE_TAILS[index]=f.tail;
    }
    for(uint8_t band=0;band<25;band++){
      clear(group&15);memset(MASK,0,32);uint8_t paint=band%3!=1;
      for(uint8_t i=0;i<8;i++){
        uint8_t index=indices[i];const struct Tail *t=&fixtures[group*8+i].tail;
        if(t->top>=band*8+8||t->bottom<=band*8)continue;
        MASK[index/8]|=1<<(index&7);
        if(paint&&(t->flags&32)){
          struct Head before=NATIVE_HEADS[index];struct Tail tail_before=NATIVE_TAILS[index];
          struct Job job={NATIVE_HEADS+index,NATIVE_TAILS+index,K_PIXELS,(uint8_t)(band*8),2};
          fast_raster_rows(&job);prepasses++;
          if(memcmp(&before,NATIVE_HEADS+index,sizeof before)||memcmp(&tail_before,NATIVE_TAILS+index,sizeof tail_before)){puts("FAIL prepass committed edge state");return 4;}
        }
        reference_rows(expected+i,t,band*8,paint);
      }
      RECORD=1;fast_raster_band(MASK,K_PIXELS,band*8,paint);RECORD=0;
      for(uint8_t i=0;i<8;i++){
        uint8_t index=indices[i];const struct Tail *t=&fixtures[group*8+i].tail;
        if(t->top<band*8+8&&t->bottom>band*8+8){struct Head *a=NATIVE_HEADS+index,*e=expected+i;
          if(a->long_x-32767L!=e->long_x||a->short_x-32767L!=e->short_x||a->short_step!=e->short_step){printf("FAIL main edge group %u band %u i %u\n",group,band,i);return 4;}}
      }
      if(check_pixels()||check_quantized()){printf(" group %u band %u\n",group,band);return 4;}
    }
  }
  printf("PASS front spans: %lu checks, %lu input spans, %lu noncommitting coverage walks; 256 fixtures in multi-triangle bands\n",(unsigned long)checks,(unsigned long)submitted,(unsigned long)prepasses);
  return 0;
}
'''
(build/'check.c').write_text(source)
cells=assemblies[1].read_text()
(build/'cells.S').write_text(re.sub(r'\bcell_rows\b','real_cell_rows',cells))
# The existing instrumentation only counts writer submissions; it does not
# implement visibility, coverage, the Q16 walker or cell encoding.
existing=(root/'tools/test-spans-kernel.py').read_text()
wrapper=existing.split("wrapper = r'''",1)[1].split("'''",1)[0]
wrapper=wrapper.replace('test_compose','test_front').replace('jsr span_compose','jsr span_front_compose')
wrapper=wrapper.replace('    sta $f3\n','    sta $f3\n    eor $ff\n    sta $fe\n',1)
wrapper=wrapper.replace('    sta $f5\n','    sta $f5\n    lda #0\n    sta $f4\n',1)
# rc8..11 are the cell-row pointers, intentionally selected by the front writer.
wrapper=wrapper.replace('3:  lda $00,y\n', '3:  cpy #8\n    bcc 11f\n    cpy #12\n    bcc 12f\n11: lda $00,y\n')
wrapper=wrapper.replace('    cmp $b580,y\n    bne 5f\n    iny\n', '    cmp $b580,y\n    bne 5f\n12: iny\n')
wrapper += r'''
.globl test_cover
test_cover:
    sta $f0
    stx $f1
    lda __rc2
    sta $f2
    jmp span_front_cover
'''
(build/'wrapper.S').write_text(wrapper)
defines={'SPAN_BUFFER':1,'SPAN_FRONT':1,'HUDS':0x9f00,'OVER':0x9f28,
         'COVER_CLEAN':0x9f50,'PIXELS':0xb000,'RR_SCRATCH':0x9f60,'PK_SCRATCH':0x9fa0,
         'CODES':0xa800,'COVER':0xa940,'PAL0':0xaa80,'CS_SCRATCH':0xab00,
         'RASTER_HEADS':0x8020,'RASTER_TAILS':0xc000}
subprocess.run([sdk/'mos-sim-clang','-Oz','-mlto-zp=0',*(f'-Wa,-defsym,{k}={v}' for k,v in defines.items()),
                '-Wl,--defsym=COLOR_DISTANCE=0xa400',build/'check.c',build/'wrapper.S',assemblies[0],
                build/'cells.S',*assemblies[2:],'-Wl,-Map='+str(build/'check.map'),'-o',build/'check'],check=True)
layout=(build/'check.map').read_text()
heap=re.search(r'^\s*([0-9a-f]+)\s+[0-9a-f]+\s+[0-9a-f]+\s+\d+\s+__heap_start',layout,re.M)
if heap is None or int(heap.group(1),16)>0x8020:raise SystemExit('front harness overlaps relocated raster heads')
result=subprocess.run([sdk/'mos-sim','--cycles',build/'check'],text=True,capture_output=True,timeout=300)
print(result.stdout.strip());print(result.stderr.strip())
if result.returncode or 'PASS front spans:' not in result.stdout:raise SystemExit(result.returncode or 'front harness incomplete')
(build/'report.json').write_text(json.dumps({'passed':True,'reference':'independent far-to-near pixel and Q16 oracle',
    'fixtures':256,'multi_triangle_bands':800,'random_bands':64,'nonoverlap':True,'prepass_immutable':True,
    'assembly_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in assemblies},
    'fixtures_sha256':hashlib.sha256((build/'fixtures.h').read_bytes()).hexdigest(),
    'simulator_output':result.stdout.strip()+' '+result.stderr.strip()},indent=2)+'\n')
