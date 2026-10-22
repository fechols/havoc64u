#!/usr/bin/env python3
"""Differential check of the native per-triangle emission kernel.

Usage: python3 tools/test-emit-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness builds the
quarter-square and projection tables, provides a RENDERER image and the
vertex cache at $8E00, and compares fast_emit_triangle with a C transcription
of Renderer::project_triangle (projection through the cache, area and
orientation culling, screen-edge rejection, the 256-command capacity with
saturating drops, the exact depth average and the 16-byte command) over
pseudo-random triangles at every depth class, degenerate and off-screen cases
and a full-capacity run. The JSON report stays under target/emit-test.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/emit-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/multiply.S", root / "target-c64/asm/project.S",
              root / "target-c64/asm/emit.S", root / "target-c64/asm/division.S"]
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
extern void fast_emit_triangle(const struct Vec3 points[3], uint8_t color, uint8_t flags);
#define QLO ((uint8_t*)0xc000)
#define QHI ((uint8_t*)0xc200)
#define CACHE ((uint8_t*)0x8e00)
uint16_t PROJECT_X[128], PROJECT_Y[128];
/* The resident renderer: header then 256 commands of 16 bytes. */
uint8_t RENDERER[6 + 256 * 16];
static uint8_t reference_renderer[6 + 256 * 16];
static int16_t clamp16(int32_t v, int32_t lo, int32_t hi){ return (int16_t)(v<lo?lo:(v>hi?hi:v)); }
static void project(const struct Vec3 *p, int16_t out[2]){
  uint16_t d=(uint16_t)p->z;
  if(d==0){out[0]=80;out[1]=100;return;}
  int8_t e=0;
  while(d>255){d>>=1;e++;}
  while(d<128){d<<=1;e--;}
  uint8_t shift=(uint8_t)(15+e);
  int32_t x=((int32_t)p->x*(int32_t)PROJECT_X[d-128])>>shift;
  int32_t y=(-(int32_t)p->y*(int32_t)PROJECT_Y[d-128])>>shift;
  out[0]=clamp16(80+x,-8191,8191); out[1]=clamp16(100+y,-8191,8191);
}
static void reference(uint8_t *r, const struct Vec3 p[3], uint8_t color, uint8_t flags){
  int16_t sx[3], sy[3];
  for(uint8_t i=0;i<3;i++){ int16_t out[2]; project(&p[i],out); sx[i]=out[0]; sy[i]=out[1]; }
  int32_t area=((int32_t)sx[1]-sx[0])*((int32_t)sy[2]-sy[0])-((int32_t)sy[1]-sy[0])*((int32_t)sx[2]-sx[0]);
  if(area==0||(area<0&&!(flags&1)))return;
  if((sx[0]<=0&&sx[1]<=0&&sx[2]<=0)||(sx[0]>=160&&sx[1]>=160&&sx[2]>=160)||
     (sy[0]<=0&&sy[1]<=0&&sy[2]<=0)||(sy[0]>=200&&sy[1]>=200&&sy[2]>=200))return;
  uint16_t count=(uint16_t)(r[0]|r[1]<<8);
  if(count==256){ uint16_t dropped=(uint16_t)(r[2]|r[3]<<8); if(dropped!=65535)dropped++; r[2]=(uint8_t)dropped; r[3]=(uint8_t)(dropped>>8); return; }
  uint32_t sum=(uint32_t)(uint16_t)p[0].z+(uint16_t)p[1].z+(uint16_t)p[2].z;
  uint16_t depth=(uint16_t)(sum/3);
  uint8_t *c=r+6+count*16;
  for(uint8_t i=0;i<3;i++){ c[i*2]=(uint8_t)sx[i]; c[i*2+1]=(uint8_t)(sx[i]>>8); c[6+i*2]=(uint8_t)sy[i]; c[7+i*2]=(uint8_t)(sy[i]>>8); }
  c[12]=(uint8_t)depth; c[13]=(uint8_t)(depth>>8); c[14]=color&15; c[15]=(flags&0x62)|(area>0?4:0);
  count++; r[0]=(uint8_t)count; r[1]=(uint8_t)(count>>8);
}
static uint32_t checks, emitted;
static int check(struct Vec3 p[3], uint8_t color, uint8_t flags){
  fast_emit_triangle(p,color,flags);
  reference(reference_renderer,p,color,flags);
  checks++;
  if(memcmp(RENDERER,reference_renderer,sizeof RENDERER)){
    uint16_t at=0; while(at<sizeof RENDERER&&RENDERER[at]==reference_renderer[at])at++;
    printf("FAIL check %lu at byte %u: got %02x want %02x; count %u/%u\n",(unsigned long)checks,at,RENDERER[at],reference_renderer[at],
           RENDERER[0]|RENDERER[1]<<8,reference_renderer[0]|reference_renderer[1]<<8);
    printf(" points (%d,%d,%d) (%d,%d,%d) (%d,%d,%d) color %u flags %u\n",p[0].x,p[0].y,p[0].z,p[1].x,p[1].y,p[1].z,p[2].x,p[2].y,p[2].z,color,flags);
    { uint16_t index=(uint16_t)((at>=6?at-6:0)/16); const int16_t *c=(const int16_t*)(RENDERER+6+index*16); const int16_t *w=(const int16_t*)(reference_renderer+6+index*16);
      printf(" command %u kernel: x %d %d %d y %d %d %d depth %d color %u reserved %u\n",index,c[0],c[1],c[2],c[3],c[4],c[5],c[6],RENDERER[6+index*16+14],RENDERER[6+index*16+15]);
      printf(" command %u ref:    x %d %d %d y %d %d %d depth %d color %u reserved %u\n",index,w[0],w[1],w[2],w[3],w[4],w[5],w[6],reference_renderer[6+index*16+14],reference_renderer[6+index*16+15]);
      int16_t out[2]; for(uint8_t i=0;i<3;i++){project(&p[i],out); printf(" ref corner %u -> (%d,%d)\n",i,out[0],out[1]);} }
    return 1;
  }
  return 0;
}
static void begin_frame(void){ memset(CACHE,0,640); memset(RENDERER,0,sizeof RENDERER); memset(reference_renderer,0,sizeof reference_renderer); }
static int16_t pick(uint32_t bits, int16_t span){ return (int16_t)((int32_t)(bits%(2*span+1))-span); }
int main(void){
  uint16_t square=0;
  for(uint16_t n=0;n<512;n++){QLO[n]=(uint8_t)square;QHI[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));}
  for(uint16_t i=0;i<128;i++){uint32_t d=i+128; PROJECT_X[i]=(uint16_t)((80ul*32768ul+d/2)/d); PROJECT_Y[i]=(uint16_t)((133ul*32768ul+d/2)/d);}
  uint32_t state=0x7f4a7c15u;
  /* Frames of random triangles: game-like spans, shared vertices to exercise
     cache hits, both orientations, every flag bit, colours 0..255. */
  for(uint8_t frame=0;frame<24;frame++){
    begin_frame();
    struct Vec3 shared[8];
    for(uint8_t i=0;i<8;i++){ state=state*1664525u+1013904223u; shared[i].x=pick(state>>4,900); shared[i].y=pick(state>>14,500); shared[i].z=(int16_t)(8+(state>>22)%2500); }
    for(uint16_t t=0;t<120;t++){
      struct Vec3 p[3];
      for(uint8_t i=0;i<3;i++){
        state=state*1664525u+1013904223u;
        if((state>>30)==0) p[i]=shared[(state>>8)&7];
        else { int16_t span=(frame&1)?8191:700; p[i].x=pick(state>>3,span); p[i].y=pick(state>>13,(frame&1)?8191:400); p[i].z=(int16_t)(8+(state>>22)%((frame&2)?300:3000)); }
      }
      state=state*1664525u+1013904223u;
      if(t%17==0){ p[1]=p[0]; }              /* degenerate */
      if(t%23==0){ p[0].x=-9000;p[1].x=-8500;p[2].x=-8200; } /* all left */
      if(t%29==0){ p[0].y=-9000;p[1].y=-8500;p[2].y=-8200; } /* all above (screen y >= 200) */
      if(check(p,(uint8_t)(state>>8),(uint8_t)(state>>16)))return 1;
    }
    emitted+=RENDERER[0]|RENDERER[1]<<8;
  }
  /* Capacity: fill all 256 commands, then drops saturate. */
  begin_frame();
  for(uint16_t t=0;t<300;t++){
    struct Vec3 p[3]={{(int16_t)(-40+(t&7)),-30,200},{60,-30,(int16_t)(200+(t&15))},{10,50,220}};
    if(check(p,7,1))return 1;
  }
  reference_renderer[2]=0xfe; reference_renderer[3]=0xff; RENDERER[2]=0xfe; RENDERER[3]=0xff;
  for(uint16_t t=0;t<4;t++){ struct Vec3 p[3]={{-40,-30,200},{60,-30,200},{10,50,220}}; if(check(p,7,1))return 1; }
  printf("PASS %lu emissions checked, %lu commands written\n",(unsigned long)checks,(unsigned long)emitted);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native emit check did not finish")
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "Renderer::project_triangle with the cached projection, transcribed to C",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
