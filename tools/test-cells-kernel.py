#!/usr/bin/env python3
"""Differential check of the cell-direct rasterization kernels.

Usage: python3 tools/test-cells-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness compares the
kernels of target-c64/asm/cells.S (fast_cell_clear, fast_cell_span,
fast_cell_plot, fast_cell_glyph, fast_cell_stage), the quantizer fallback
fast_pack_cell (pack.S) and the raster's hand-off (fast_raster_rows) with a
C transcription of havoc::cells::CellBand and render::pack_cell: per-cell
palettes assigned on first use (slot keys = colour XOR the band's clear
colour, which is code 0 — the VIC background — and needs no slot), dead-slot
recycling, overflow into the pixel form with the classic per-pixel rules
(terrain-clear coverage, exterior terrain, HUD ink), and staged band output.
Random operation sequences check the
complete state after every operation; Rust-exported triangle fixtures walk
all 25 bands through the real raster. The JSON report stays under
target/cells-test.
"""
import hashlib
import json
import os
import pathlib
import re
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/cells-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/cells.S", root / "target-c64/asm/pack.S",
              root / "target-c64/asm/raster.S"]
env = dict(os.environ, HAVOC_RASTER_FIXTURES=str(build/"fixtures.h"))
subprocess.run(["cargo", "test", "--lib", "render::tests::export_native_raster_fixtures",
                "--", "--ignored", "--exact"], cwd=root, env=env, check=True)
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Head { int32_t long_x,long_step,short_x,short_step; };
struct Tail { int32_t lower_x,lower_step; uint8_t top,middle,bottom,flags; };
struct Fixture { struct Head head; struct Tail tail; };
struct Job { struct Head *head; const struct Tail *tail; uint8_t *pixels; uint8_t band,paint; };
extern void fast_raster_rows(const struct Job *);
extern void fast_cell_clear(uint8_t color);
extern void fast_cell_span(uint8_t row, uint8_t first, uint8_t end, uint8_t color);
extern void fast_cell_plot(uint8_t row, uint8_t x, uint8_t color);
extern void fast_cell_glyph(uint8_t row, uint8_t x, const uint8_t rows[5], uint8_t color);
extern void fast_cell_stage(const uint8_t *old_screen, const uint8_t *old_colors);
extern void test_cell_span_mode(uint8_t row, uint8_t first, uint8_t end, uint8_t color, uint8_t mode);
#include "fixtures.h"
/* Kernel state (this build relocates the band state above the harness image). */
#define K_CODES ((uint8_t*)0xa800)
#define K_COVER ((uint8_t*)0xa940)
#define K_PAL ((uint8_t*)0xaa80)
#define K_HUD ((uint8_t*)0xb600)
#define K_OVER ((uint8_t*)0xb628)
#define K_CLEAN ((volatile uint8_t*)0xb650)
#define K_HUD_SEEN ((volatile uint8_t*)0xab28)
#define K_PIXELS ((uint8_t*)0xb000)
#define K_SEEN ((volatile uint8_t*)0xb66b)   /* RR_PLAYER_SEEN = RR_SCRATCH + 11 */
#define COLOR_DISTANCE (*(uint16_t (*)[16][16])0xa400)
static const uint8_t PALETTE_RGB[16][3]={{0,0,0},{255,255,255},{136,0,0},{170,255,238},{204,68,204},{0,204,85},{0,0,170},{238,238,119},{221,136,85},{102,68,0},{255,119,119},{51,51,51},{119,119,119},{170,255,102},{0,136,255},{187,187,187}};
#define PRESENT ((uint8_t*)0xa600)
static const uint8_t EXPAND[16]={0x00,0x03,0x0c,0x0f,0x30,0x33,0x3c,0x3f,0xc0,0xc3,0xcc,0xcf,0xf0,0xf3,0xfc,0xff};
enum { PLAIN, CLEAR, EXTERIOR };
struct Model { uint8_t codes[8][40], pal[3][40], cover[8][40], over[40]; uint8_t pixels[1280]; uint8_t seen,hud_seen,bg; };
/* Large buffers live above the kernel state so the harness image stays clear of it. */
#define m (*(struct Model*)0xc000)
static uint32_t checks, overflows, recycled;
/* Slots hold keys (colour XOR the clear colour), as the kernel does: the clear
   colour is key 0 like a free slot. */
static void m_clear(uint8_t color){
  m.bg=color&15;
  memset(m.codes,0,sizeof m.codes); memset(m.pal,0,sizeof m.pal);
  memset(m.cover,0,sizeof m.cover); memset(m.over,0,40); m.seen=m.hud_seen=0;
}
static void m_pixel_row(uint8_t row,uint8_t cell,uint8_t mask,uint8_t color,uint8_t hud,uint8_t paint){
  uint8_t ink=color|(hud?0x80:0);
  for(uint8_t c=0;c<4;c++){
    if(!(mask&(8>>c)))continue;
    uint8_t *p=&m.pixels[row*160+cell*4+c];
    if(paint==CLEAR)*p=ink|0x40;
    else if(paint==EXTERIOR){ if(!(*p&0x40))*p=ink; }
    else *p=hud?ink:((*p&0x40)|ink);
  }
}
static void m_spill(uint8_t cell){
  overflows++;
  for(uint8_t row=0;row<8;row++){ uint8_t codes=m.codes[row][cell],cover=m.cover[row][cell];
    for(uint8_t c=0;c<4;c++){ uint8_t slot=(codes>>(6-2*c))&3; uint8_t pixel=slot?(uint8_t)(m.pal[slot-1][cell]^m.bg):m.bg;
      if(cover&(8>>c))pixel|=0x40; if(cover&(0x80>>c))pixel|=0x80; m.pixels[row*160+cell*4+c]=pixel; } }
  m.over[cell]=1;
  /* the kernel poisons a spilled cell's palette so its span fast paths never match */
  m.pal[0][cell]=m.pal[1][cell]=m.pal[2][cell]=255;
}
static int m_slot(uint8_t cell,uint8_t key){
  for(uint8_t s=0;s<3;s++)if(m.pal[s][cell]==key)return s+1;
  for(uint8_t s=0;s<3;s++)if(m.pal[s][cell]==0){m.pal[s][cell]=key;return s+1;}
  uint8_t live=0; for(uint8_t row=0;row<8;row++)live|=PRESENT[m.codes[row][cell]];
  for(uint8_t s=0;s<3;s++)if(!(live&(2<<s))){recycled++;m.pal[s][cell]=key;return s+1;}
  return 0;
}
static void m_cell_row(uint8_t row,uint8_t cell,uint8_t mask,uint8_t color,uint8_t hud,uint8_t paint){
  if(hud)m.hud_seen=0x80;
  if(m.over[cell]){m_pixel_row(row,cell,mask,color,hud,paint);return;}
  if(paint==EXTERIOR){mask&=~m.cover[row][cell];if(!mask)return;}
  uint8_t slot=0, key=color^m.bg;
  if(key){ int s=m_slot(cell,key); if(!s){m_spill(cell);m_pixel_row(row,cell,mask,color,hud,paint);return;} slot=(uint8_t)s; }
  uint8_t bits=EXPAND[mask]; m.codes[row][cell]=(m.codes[row][cell]&~bits)|((uint8_t)(0x55*slot)&bits);
  m.cover[row][cell]=(m.cover[row][cell]&~(mask<<4))|(hud?(mask<<4):0);
  if(paint==CLEAR)m.cover[row][cell]|=mask;
  else if(paint==PLAIN&&hud)m.cover[row][cell]&=~mask;
}
static void m_span(uint8_t row,uint8_t first,uint8_t end,uint8_t color,uint8_t hud,uint8_t paint){
  if(first>=end)return; color&=15;
  for(uint8_t cell=first/4;cell<=(end-1)/4;cell++){ uint8_t lo=first>cell*4?first-cell*4:0, hi=end<cell*4+4?end-cell*4:4; uint8_t mask=0; for(uint8_t c=lo;c<hi;c++)mask|=8>>c; m_cell_row(row,cell,mask,color,hud,paint); }
}
static void m_geometry_span(uint8_t row,uint8_t first,uint8_t end,uint8_t flags){
  uint8_t paint=PLAIN; if(flags&0x20){m.seen=1;paint=CLEAR;} else if(m.seen&&(flags&0x40))paint=EXTERIOR;
  m_span(row,first,end,flags&15,0,paint);
}
static void m_plot(uint8_t row,uint8_t x,uint8_t color){ m_cell_row(row,x/4,8>>(x&3),color&15,(color&0x80)!=0,PLAIN); }
static void m_glyph(uint8_t row,uint8_t x,const uint8_t rows[5],uint8_t color){
  if(color&0x80)m.hud_seen=0x80;
  for(uint8_t dy=0;dy<5;dy++){ uint8_t bits=rows[dy]; if(bits&4)m_plot(row+dy,x,color); if(bits&2)m_plot(row+dy,x+1,color); if(bits&1)m_plot(row+dy,x+2,color); }
}
/* render::pack_cell with the band's background colour (code 0, never a slot;
   it also marks "no colour" in the working tables) */
static void pack_cell(const uint8_t *pixels,uint8_t cell,const uint8_t old[3],uint8_t bg,uint8_t bitmap[8],uint8_t *screen,uint8_t *color_out){
  uint8_t counts[16]={0}; uint16_t hud=0,present=0;
  for(uint8_t row=0;row<8;row++)for(uint8_t c=0;c<4;c++){uint8_t p=pixels[row*160+cell*4+c];counts[p&15]++;present|=1<<(p&15);if(p&0x80)hud|=1<<(p&15);}
  if((present&(present-1))==0){ uint8_t color=pixels[cell*4]&15; uint8_t slot=0; if(color!=bg){slot=1;for(uint8_t i=0;i<3;i++)if(old[i]==color){slot=i+1;break;}}
    memset(bitmap,(uint8_t)(slot*0x55),8); uint8_t palette[4]={bg,bg,bg,bg}; palette[slot]=color;
    *screen=(uint8_t)((palette[1]<<4)|palette[2]); *color_out=palette[3]; return; }
  uint8_t selected[3]={bg,bg,bg}; uint16_t selected_bits=0;
  for(uint8_t k=0;k<3;k++){ uint8_t best=bg; uint16_t best_score=0;
    for(uint8_t color=0;color<16;color++){ if(color==bg||!counts[color]||(selected_bits&(1<<color)))continue;
      uint16_t score=counts[color]*4+((old[0]==color||old[1]==color||old[2]==color)?1:0)+((hud&(1<<color))?256:0);
      if(score>best_score){best=color;best_score=score;} }
    selected[k]=best; if(best!=bg)selected_bits|=1<<best; }
  uint8_t palette[4]={bg,bg,bg,bg}; uint16_t assigned=0;
  for(uint8_t slot=0;slot<3;slot++) if((selected_bits&(1<<old[slot]))&&!(assigned&(1<<old[slot]))){palette[slot+1]=old[slot];assigned|=1<<old[slot];}
  for(uint8_t k=0;k<3;k++){ uint8_t chosen=selected[k]; if(chosen==bg||(assigned&(1<<chosen)))continue; for(uint8_t slot=1;slot<4;slot++)if(palette[slot]==bg){palette[slot]=chosen;assigned|=1<<chosen;break;} }
  uint8_t map[16]={0};
  for(uint8_t color=0;color<16;color++){ if(color==bg||!counts[color])continue; uint16_t best=COLOR_DISTANCE[color][bg]; for(uint8_t slot=1;slot<4;slot++){uint16_t d=COLOR_DISTANCE[color][palette[slot]]; if(d<best){map[color]=slot;best=d;}} }
  for(uint8_t row=0;row<8;row++){ uint8_t packed=0; for(uint8_t c=0;c<4;c++)packed=(uint8_t)((packed<<2)|map[pixels[row*160+cell*4+c]&15]); bitmap[row]=packed; }
  *screen=(uint8_t)((palette[1]<<4)|palette[2]); *color_out=palette[3];
}
static void m_finish(const uint8_t *old_screen,const uint8_t *old_color,uint8_t *bitmap,uint8_t *screen,uint8_t *colors){
  uint8_t bg2=(uint8_t)((m.bg<<4)|m.bg);
  for(uint8_t cell=0;cell<40;cell++){
    if(m.over[cell]){ uint8_t old[3]={(uint8_t)(old_screen[cell]>>4),(uint8_t)(old_screen[cell]&15),(uint8_t)(old_color[cell]&15)}; pack_cell(m.pixels,cell,old,m.bg,bitmap+cell*8,&screen[cell],&colors[cell]); }
    else { for(uint8_t row=0;row<8;row++)bitmap[cell*8+row]=m.codes[row][cell]; screen[cell]=(uint8_t)(((m.pal[0][cell]<<4)|m.pal[1][cell])^bg2); colors[cell]=(uint8_t)(m.pal[2][cell]^m.bg); }
  }
}
static const char *compare_state(void){
  checks++;
  if(memcmp(K_CODES,m.codes,320))return "codes";
  if(memcmp(K_COVER,m.cover,320))return "cover";
  if(memcmp(K_PAL,m.pal,120))return "palette";
  if(*K_HUD_SEEN!=m.hud_seen)return "HUD band flag";
  if(memcmp(K_OVER,m.over,40))return "overflow flags";
  for(uint8_t cell=0;cell<40;cell++) if(m.over[cell]) for(uint8_t row=0;row<8;row++) if(memcmp(K_PIXELS+row*160+cell*4,m.pixels+row*160+cell*4,4))return "overflow pixels";
  return 0;
}
static void dump(const char *what){
  printf("FAIL check %lu: %s\n",(unsigned long)checks,what);
  for(uint16_t i=0;i<320;i++)if(K_CODES[i]!=((uint8_t*)m.codes)[i]){printf(" codes[%u][%u] got %02x want %02x\n",i/40,i%40,K_CODES[i],((uint8_t*)m.codes)[i]);break;}
  for(uint16_t i=0;i<320;i++)if(K_COVER[i]!=((uint8_t*)m.cover)[i]){printf(" cover[%u][%u] got %02x want %02x\n",i/40,i%40,K_COVER[i],((uint8_t*)m.cover)[i]);break;}
  for(uint16_t i=0;i<120;i++)if(K_PAL[i]!=((uint8_t*)m.pal)[i]){printf(" pal[%u][%u] got %u want %u\n",i/40,i%40,K_PAL[i],((uint8_t*)m.pal)[i]);break;}
  printf(" HUD band flag %u/%u\n",*K_HUD_SEEN,m.hud_seen);
  for(uint16_t i=0;i<40;i++)if(K_OVER[i]!=m.over[i]){printf(" cell %u over %u/%u\n",i,K_OVER[i],m.over[i]);break;}
  for(uint8_t cell=0;cell<40;cell++) if(m.over[cell]) for(uint8_t row=0;row<8;row++) for(uint8_t c=0;c<4;c++){ uint16_t i=row*160+cell*4+c; if(K_PIXELS[i]!=m.pixels[i]){printf(" pixel row %u cell %u col %u got %02x want %02x\n",row,cell,c,K_PIXELS[i],m.pixels[i]);return;} }
}
static uint32_t state=0x12345678u;
static uint8_t last_first, last_end, last_flags, last_role;
static uint32_t next(void){state=state*1664525u+1013904223u;return state;}
#define actual_bitmap ((uint8_t*)0xb700)
#define expected_bitmap ((uint8_t*)0xb840)
#define actual_screen ((uint8_t*)0xb980)
#define expected_screen ((uint8_t*)0xb9a8)
#define actual_color ((uint8_t*)0xb9d0)
#define expected_color ((uint8_t*)0xb9f8)
static int stage_and_compare_attributes(const uint8_t *old_screen,const uint8_t *old_color){
  memcpy(actual_screen,old_screen,40); memcpy(actual_color,old_color,40);
  memset(actual_bitmap,0xa5,320); memset(expected_bitmap,0xa5,320);
  m_finish(old_screen,old_color,expected_bitmap,expected_screen,expected_color);
  fast_cell_stage(actual_screen,actual_color);
  checks++;
  if(memcmp(K_COVER,expected_bitmap,320)||memcmp(K_HUD,expected_screen,40)||memcmp(K_OVER,expected_color,40)){
    printf("FAIL stage check %lu\n",(unsigned long)checks);
    for(uint16_t i=0;i<320;i++)if(K_COVER[i]!=expected_bitmap[i]){printf(" bitmap[%u] got %02x want %02x over %u\n",i,K_COVER[i],expected_bitmap[i],m.over[i/8]);break;}
    for(uint8_t i=0;i<40;i++)if(K_HUD[i]!=expected_screen[i]||K_OVER[i]!=expected_color[i]){printf(" cell %u screen %02x/%02x color %u/%u over %u\n",i,K_HUD[i],expected_screen[i],K_OVER[i],expected_color[i],m.over[i]);break;}
    return 1;
  }
  if(memcmp(actual_screen,old_screen,40)||memcmp(actual_color,old_color,40)){puts("FAIL staging changed preceding attributes");return 1;}
  for(uint16_t i=0;i<320;i++)if(actual_bitmap[i]!=0xa5){puts("FAIL staging wrote live bitmap");return 1;}
  if(*K_CLEAN!=0){puts("FAIL staging left coverage marked clean");return 1;}
  if(*K_HUD_SEEN!=m.hud_seen){puts("FAIL staging changed HUD band flag");return 1;}
  if(memcmp(K_CODES,m.codes,320)||memcmp(K_PAL,m.pal,120)){puts("FAIL staging corrupted adjacent code/palette state");return 1;}
  return 0;
}
static int stage_and_compare(void){
  uint8_t old_screen[40],old_color[40];
  for(uint8_t i=0;i<40;i++){ old_screen[i]=(uint8_t)(next()>>8); old_color[i]=(uint8_t)(next()>>8); if((next()>>9)%3==0){old_screen[i]=0;old_color[i]=0;} }
  return stage_and_compare_attributes(old_screen,old_color);
}
static uint8_t clamp(int32_t x){x=(x+32767L)>>16;return x<0?0:x>160?160:(uint8_t)x;}
/* Native heads and tails carry the walker's +32767 position bias (steps do not). */
#define BIAS 32767L
static void bias(struct Head *h,struct Tail *t){h->long_x+=BIAS;h->short_x+=BIAS;if(t)t->lower_x+=BIAS;}
static int heads_differ(const struct Head *native,const struct Head *host){
  struct Head u=*native; u.long_x-=BIAS; u.short_x-=BIAS; return memcmp(&u,host,sizeof u);
}
static int tails_differ(const struct Tail *native,const struct Tail *host){
  struct Tail u=*native; u.lower_x-=BIAS; return memcmp(&u,host,sizeof u);
}
static void reference_rows(struct Head *h,const struct Tail *t,uint8_t band,uint8_t paint){
  uint8_t first=t->top>band?t->top:band; uint8_t end=t->bottom<band+8?t->bottom:band+8;
  for(uint8_t y=first;y<end;++y){
    if(y==t->middle){h->short_x=t->lower_x;h->short_step=t->lower_step;}
    uint8_t left=clamp((t->flags&16)?h->long_x:h->short_x), right=clamp((t->flags&16)?h->short_x:h->long_x);
    if(paint&&left<right)m_geometry_span(y-band,left,right,t->flags);
    h->long_x+=h->long_step;h->short_x+=h->short_step;
  }
}
int main(void){
  for(uint16_t b=0;b<256;b++)PRESENT[b]=(uint8_t)((1<<(b>>6))|(1<<((b>>4)&3))|(1<<((b>>2)&3))|(1<<(b&3)));
  for(uint8_t a=0;a<16;a++)for(uint8_t b=0;b<16;b++){int32_t r=PALETTE_RGB[a][0]-PALETTE_RGB[b][0],g=PALETTE_RGB[a][1]-PALETTE_RGB[b][1],bl=PALETTE_RGB[a][2]-PALETTE_RGB[b][2];COLOR_DISTANCE[a][b]=(uint16_t)((r*r+2*g*g+bl*bl)/4);}
  *K_CLEAN=0;  /* COVER unknown */
  /* Directed case: fill one cell with three colours, then a fourth spills it. */
  {
    fast_cell_clear(0); m_clear(0); *K_SEEN=0;
    for(uint8_t step=0;step<6;step++){
      static const uint8_t colors[6]={1,2,3,4,5,1};
      fast_cell_span(step&7,8,12,colors[step]); m_span(step&7,8,12,colors[step],0,PLAIN);
      const char *why=compare_state();
      if(why){dump(why);printf(" directed spill step %u\n",step);
        printf(" K_OVER %u %u %u %u | $7728.. %u %u %u %u | K_PAL0 %u %u %u %u | K_CODES[3] %02x %02x %02x %02x | pixels row3 cell2 %02x %02x %02x %02x\n",
          K_OVER[0],K_OVER[1],K_OVER[2],K_OVER[3],*(uint8_t*)0x7728,*(uint8_t*)0x7729,*(uint8_t*)0x772a,*(uint8_t*)0x772b,
          K_PAL[0],K_PAL[1],K_PAL[2],K_PAL[3],K_CODES[120],K_CODES[121],K_CODES[122],K_CODES[123],
          K_PIXELS[480+8],K_PIXELS[481+8],K_PIXELS[482+8],K_PIXELS[483+8]);
        for(uint8_t r=0;r<8;r++)printf(" row %u codes %02x pixels %02x %02x %02x %02x\n",r,K_CODES[r*40+2],K_PIXELS[r*160+8],K_PIXELS[r*160+9],K_PIXELS[r*160+10],K_PIXELS[r*160+11]);
        printf(" scratch:"); for(uint8_t i=0;i<0x28;i++)printf(" %02x",((uint8_t*)0xab00)[i]); printf("\n PAL1 %u %u %u %u PAL2 %u %u %u %u\n",K_PAL[40],K_PAL[41],K_PAL[42],K_PAL[43],K_PAL[80],K_PAL[81],K_PAL[82],K_PAL[83]);
        return 9;}
    }
    fast_cell_plot(6,9,0x8e); m_plot(6,9,0x8e);
    const char *why=compare_state(); if(why){dump(why);puts(" directed plot into spilled cell");return 9;}
    if(stage_and_compare())return 9;
  }
  /* HUD metadata must survive spill timing, exact masks and all paint modes.
     The broad span exercises both edges and full interior cells after HUD. */
  for(uint8_t bg_index=0;bg_index<3;bg_index++)for(uint8_t early=0;early<2;early++)for(uint8_t paint=0;paint<3;paint++)for(uint8_t hud=0;hud<2;hud++){
    static const uint8_t backgrounds[3]={0,6,15}; uint8_t bg=backgrounds[bg_index];
    fast_cell_clear(bg); m_clear(bg); *K_SEEN=0;
    if(early)for(uint8_t row=4;row<8;row++){
      fast_cell_span(row,0,12,row-3); m_span(row,0,12,row-3,0,PLAIN);
    }
    test_cell_span_mode(0,0,12,2,CLEAR); m_span(0,0,12,2,0,CLEAR);
    test_cell_span_mode(1,0,12,2,CLEAR); m_span(1,0,12,2,0,CLEAR);
    fast_cell_span(0,1,11,0x82); m_span(0,1,11,2,1,PLAIN);
    const char *why=compare_state(); if(why){dump(why);puts(" HUD clears terrain coverage");return 11;}
    uint8_t ink=(uint8_t)(bg|(hud?0x80:0));
    test_cell_span_mode(0,2,10,ink,paint); m_span(0,2,10,bg,hud,paint);
    test_cell_span_mode(1,1,11,ink,paint); m_span(1,1,11,bg,hud,paint);
    why=compare_state(); if(why){dump(why);printf(" HUD modes bg %u early %u paint %u hud %u\n",bg,early,paint,hud);return 11;}
    /* Cross-cell glyph followed by unmarked spans and a clear-color plot. */
    const uint8_t glyph[5]={7,5,2,5,7};
    fast_cell_glyph(2,3,glyph,0x8e); m_glyph(2,3,glyph,0x8e);
    fast_cell_span(3,1,11,2); m_span(3,1,11,2,0,PLAIN);
    fast_cell_plot(2,4,bg); m_plot(2,4,bg);
    why=compare_state(); if(why){dump(why);puts(" glyph and unmarked overwrite");return 11;}
    /* Force every affected cell to spill; all metadata must reconstruct. */
    for(uint8_t row=4;row<8;row++){
      fast_cell_span(row,0,12,row-3); m_span(row,0,12,row-3,0,PLAIN);
      why=compare_state(); if(why){dump(why);puts(" spill after HUD overwrite");return 11;}
    }
    if(stage_and_compare())return 11;
    fast_cell_clear(bg); m_clear(bg);
    why=compare_state(); if(why){dump(why);puts(" clear after HUD staging");return 11;}
  }
  puts("PASS 36 HUD metadata layouts: all paint modes, background ink, spill timing, glyphs and clear lifecycle");
  /* Staging consumes flags only for the current cell. Exercise every cell,
     alternating spill/direct paths, and an overflow in the final cell. */
  for(uint8_t background=0;background<3;background++)for(uint8_t layout=0;layout<4;layout++){
    static const uint8_t backgrounds[3]={0,6,15};
    uint8_t bg=backgrounds[background],old_screen[40],old_color[40];
    fast_cell_clear(bg); m_clear(bg);
    for(uint8_t cell=0;cell<40;cell++){
      uint8_t spill=layout==3||(layout==1&&(cell&1))||(layout==2&&cell==39);
      for(uint8_t row=0;row<(spill?4:2);row++){
        uint8_t ink=(uint8_t)((bg^(row+1))|(row==0?0x80:0));
        fast_cell_span(row,cell*4,cell*4+4,ink); m_span(row,cell*4,cell*4+4,ink&15,(ink&0x80)!=0,PLAIN);
      }
      old_screen[cell]=(uint8_t)(((bg^2)<<4)|(bg^4));
      old_color[cell]=(uint8_t)(0xf0|(bg^1)); /* undefined upper colour nibble */
    }
    const char *why=compare_state(); if(why){dump(why);puts(" directed staging setup");return 10;}
    if(*K_CLEAN!=0){puts("FAIL HUD painting left metadata marked clean");return 10;}
    if(stage_and_compare_attributes(old_screen,old_color))return 10;
    /* The staged bitmap occupied clean COVER. A fresh clear must remove it
       before either plain painting or exterior terrain reads coverage. */
    fast_cell_clear(bg); m_clear(bg); *K_SEEN=0;
    why=compare_state(); if(why){dump(why);puts(" after staged band clear");return 10;}
    fast_cell_span(0,0,160,bg^1); m_span(0,0,160,bg^1,0,PLAIN);
    struct Head h={0,0,160L*65536L,0},eh=h;
    struct Tail t={0,0,0,8,1,(uint8_t)(16|64|(bg^2))},et=t;
    bias(&h,&t);
    struct Job job={&h,&t,K_PIXELS,0,1};
    fast_raster_rows(&job); reference_rows(&eh,&et,0,1);
    why=compare_state(); if(why){dump(why);puts(" exterior painting after staged clear");return 10;}
    if(stage_and_compare_attributes(old_screen,old_color))return 10;
  }
  puts("PASS 12 directed staging layouts, read-only inputs, coverage reuse");
  /* Random operation sequences. */
  for(uint16_t band=0;band<120;band++){
    uint8_t clear_color=(uint8_t)((next()>>9)%5==0?0:(next()>>8)&15);
    fast_cell_clear(clear_color); m_clear(clear_color); *K_SEEN=0;
    const char *why=compare_state(); if(why){dump(why);return 1;}
    uint8_t palette_size=(uint8_t)(2+(next()>>7)%6);   /* few colours: exercises recycling and overflow */
    uint8_t colors[8]; for(uint8_t i=0;i<8;i++)colors[i]=(uint8_t)(1+(next()>>6)%15);
    /* ink in the clear colour (code 0 without a slot) is common in the game */
    if(clear_color)colors[(next()>>8)%palette_size]=clear_color;
    uint16_t ops=(uint16_t)(20+(next()>>8)%120);
    for(uint16_t op=0;op<ops;op++){
      uint8_t kind=(uint8_t)((next()>>10)%10);
      uint8_t row=(uint8_t)((next()>>12)&7);
      uint8_t color=(uint8_t)((next()>>5)%7==0?0:colors[(next()>>8)%palette_size]);
      static uint8_t before_codes[8], before_pal[3], before_hud, before_over, before_mcodes[8], before_mpal[3];
      for(uint8_t r=0;r<8;r++){before_codes[r]=K_CODES[r*40+29];before_mcodes[r]=m.codes[r][29];}
      for(uint8_t s=0;s<3;s++){before_pal[s]=K_PAL[s*40+29];before_mpal[s]=m.pal[s][29];}
      before_hud=K_HUD[29]; before_over=K_OVER[29];
      if(kind<5){ /* geometry span through the raster: a flat rectangle row */
        uint8_t first=(uint8_t)((next()>>8)%160), width=(uint8_t)(1+(next()>>7)%((band&1)?12:80)); uint8_t end=(uint8_t)(first+width>160?160:first+width);
        uint8_t role=(uint8_t)((next()>>9)%6); uint8_t flags=(uint8_t)(16|color|(role==0?32:role==1?64:0));
        last_first=first; last_end=end; last_flags=flags; last_role=role;
        struct Head h={(int32_t)first*65536L,0,(int32_t)end*65536L,0}, eh=h;
        struct Tail t={0,0,row,8,(uint8_t)(row+1),flags}, et=t;
        bias(&h,&t);
        struct Job job={&h,&t,K_PIXELS,0,1};
        fast_raster_rows(&job); reference_rows(&eh,&et,0,1);
        if(*K_SEEN!=m.seen){printf("FAIL coverage flag %u/%u\n",*K_SEEN,m.seen);return 1;}
      } else if(kind<7){ /* HUD span */
        uint8_t first=(uint8_t)((next()>>8)%160), width=(uint8_t)(1+(next()>>7)%60); uint8_t end=(uint8_t)(first+width>160?160:first+width);
        uint8_t ink=(uint8_t)(color|((next()>>11)&1?0x80:0));
        fast_cell_span(row,first,end,ink); m_span(row,first,end,ink&15,(ink&0x80)!=0,PLAIN);
      } else if(kind<9){ /* plot */
        uint8_t x=(uint8_t)((next()>>8)%160); uint8_t ink=(uint8_t)(color|((next()>>11)&1?0x80:0));
        fast_cell_plot(row,x,ink); m_plot(row,x,ink);
      } else { /* glyph */
        uint8_t rows[5]; for(uint8_t i=0;i<5;i++)rows[i]=(uint8_t)((next()>>9)&7);
        uint8_t grow=(uint8_t)((next()>>12)&3), x=(uint8_t)((next()>>8)%158); uint8_t ink=(uint8_t)(color|0x80);
        last_first=x; last_end=grow; last_flags=rows[0]|rows[1]<<3; last_role=rows[2]|rows[3]<<3|rows[4]<<6;
        fast_cell_glyph(grow,x,rows,ink); m_glyph(grow,x,rows,ink);
      }
      why=compare_state(); if(why){dump(why);printf(" after op %u kind %u row %u color %u span %u..%u flags %02x role %u seen %u\n",op,kind,row,color,last_first,last_end,last_flags,last_role,m.seen);
        printf(" cell 29 before: kernel codes");for(uint8_t r=0;r<8;r++)printf(" %02x",before_codes[r]);printf(" pal %u %u %u hud %u over %u | model codes",before_pal[0],before_pal[1],before_pal[2],before_hud,before_over);for(uint8_t r=0;r<8;r++)printf(" %02x",before_mcodes[r]);printf(" pal %u %u %u\n",before_mpal[0],before_mpal[1],before_mpal[2]);
        return 1;}
    }
    if(stage_and_compare())return 1;
  }
  printf("PASS %lu state checks over 120 random bands, %lu overflows, %lu recycled slots\n",(unsigned long)checks,(unsigned long)overflows,(unsigned long)recycled);
  /* Rust-exported triangle fixtures through all 25 bands. */
  uint16_t cases=0;
  for(uint16_t f=0;f<sizeof(fixtures)/sizeof(fixtures[0]);++f){
    struct Head ah=fixtures[f].head, eh=ah; struct Tail at=fixtures[f].tail, et=at;
    bias(&ah,&at);
    for(uint8_t band=0;band<25;++band){
      uint8_t clear_color=(uint8_t)(f&1?6:0);
      fast_cell_clear(clear_color); m_clear(clear_color); *K_SEEN=0;
      uint8_t paint=(band%3)!=1;
      struct Job job={&ah,&at,K_PIXELS,(uint8_t)(band*8),paint};
      fast_raster_rows(&job); reference_rows(&eh,&et,band*8,paint);
      /* the kernel skips the head write-back in a triangle's last band */
      uint8_t last=at.bottom<=band*8+8;
      if((!last&&heads_differ(&ah,&eh))||tails_differ(&at,&et)){printf("FAIL fixture %u band %u: edge state long %ld/%ld short %ld/%ld step %ld/%ld tail %d\n",f,band,(long)(ah.long_x-BIAS),(long)eh.long_x,(long)(ah.short_x-BIAS),(long)eh.short_x,(long)ah.short_step,(long)eh.short_step,tails_differ(&at,&et));
        printf(" tail top %u middle %u bottom %u flags %02x; rc8..11 %02x %02x %02x %02x; zp f0..f5 %02x %02x %02x %02x %02x %02x\n",at.top,at.middle,at.bottom,at.flags,*(uint8_t*)0x08,*(uint8_t*)0x09,*(uint8_t*)0x0a,*(uint8_t*)0x0b,*(uint8_t*)0xf0,*(uint8_t*)0xf1,*(uint8_t*)0xf2,*(uint8_t*)0xf3,*(uint8_t*)0xf4,*(uint8_t*)0xf5);
        return 2;}
      const char *why=compare_state(); if(why){dump(why);printf(" fixture %u band %u\n",f,band);return 2;}
    }
    ++cases;
  }
  printf("PASS %u fixture triangles over 25 bands\n",cases);
  /* Layered scenes: several triangles per band, with the three paint modes. */
  for(uint16_t scene=0;scene<40;scene++){
    uint8_t clear_color=(uint8_t)((next()>>8)%3==0?0:(next()>>8)&15);
    fast_cell_clear(clear_color); m_clear(clear_color); *K_SEEN=0;
    uint8_t layers=(uint8_t)(3+(next()>>8)%6);
    for(uint8_t layer=0;layer<layers;layer++){
      uint16_t index=(uint16_t)((next()>>8)%(sizeof(fixtures)/sizeof(fixtures[0])));
      struct Head ah=fixtures[index].head, eh=ah; struct Tail at=fixtures[index].tail, et=at;
      uint8_t role=(uint8_t)((next()>>9)%4);
      at.flags=et.flags=(uint8_t)((at.flags&31)|(role==0?32:role==1?64:0));
      uint8_t band=(uint8_t)((at.top>>3)+(next()>>12)%3); if(band>24)band=24;
      /* seek the head to the band like the host does */
      uint8_t row=band*8; if(row<at.top)row=at.top; if(row>at.bottom)row=at.bottom;
      ah.long_x+=ah.long_step*(int32_t)(row-at.top);
      if(row>=at.middle){ah.short_x=at.lower_x+at.lower_step*(int32_t)(row-at.middle);ah.short_step=at.lower_step;}else ah.short_x+=ah.short_step*(int32_t)(row-at.top);
      eh=ah;
      bias(&ah,&at);
      struct Job job={&ah,&at,K_PIXELS,(uint8_t)(band*8),1};
      fast_raster_rows(&job); reference_rows(&eh,&et,band*8,1);
      const char *why=compare_state(); if(why){dump(why);printf(" scene %u layer %u\n",scene,layer);return 3;}
    }
    if(stage_and_compare())return 3;
  }
  printf("PASS 40 layered scenes; %lu checks total\n",(unsigned long)checks);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
(build/"wrapper.S").write_text('''
.text
.globl test_cell_span_mode
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
test_cell_span_mode:
    sta $f0
    stx $f1
    lda __rc2
    sta $f2
    lda __rc3
    and #15
    sta $f3
    eor $ff
    sta $fe
    lda __rc3
    and #$80
    sta $f4
    lda __rc4
    sta $f5
    jsr cell_row_pointers
    jmp cell_rows
''', encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Oz", "-mlto-zp=0", "-Wa,-defsym,HUDS=0xb600", "-Wa,-defsym,OVER=0xb628",
                "-Wa,-defsym,COVER_CLEAN=0xb650", "-Wa,-defsym,PIXELS=0xb000", "-Wa,-defsym,RR_SCRATCH=0xb660", "-Wa,-defsym,PK_SCRATCH=0xb6a0",
                "-Wa,-defsym,CODES=0xa800", "-Wa,-defsym,COVER=0xa940", "-Wa,-defsym,PAL0=0xaa80", "-Wa,-defsym,CS_SCRATCH=0xab00",
                "-Wl,--defsym=COLOR_DISTANCE=0xa400", build/"check.c", build/"wrapper.S", *assemblies,
                "-Wl,-Map=" + str(build/"check.map"), "-o", build/"check"], check=True)
# Kernel state, output arrays and tables occupy $A400–$BA1F;
# the independent model starts at $C000. The harness image must end
# below all of them or the comparison is void.
layout = (build/"check.map").read_text(encoding="utf-8", errors="replace")
heap = re.search(r"^\s*([0-9a-f]+)\s+[0-9a-f]+\s+[0-9a-f]+\s+\d+\s+__heap_start", layout, re.M)
if heap is None or int(heap.group(1), 16) > 0xa400:
    raise SystemExit(f"harness image reaches {heap.group(1) if heap else '?'}: overlaps kernel tables at $A400")
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS 40 layered scenes" not in result.stdout:
    raise SystemExit(result.returncode or "native cells check did not finish")
report = {"passed": True,
          "directed_hud_layouts": 36, "per_pixel_hud_priority": True,
          "directed_staging_layouts": 12, "live_output_untouched": True,
          "coverage_reuse_cleared": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "fixtures_sha256": hashlib.sha256((build/"fixtures.h").read_bytes()).hexdigest(),
          "reference": "havoc::cells::CellBand and render::pack_cell transcribed to C; independent C Q16 row traversal",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
