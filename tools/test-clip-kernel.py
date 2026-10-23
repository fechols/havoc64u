#!/usr/bin/env python3
"""Differential check of the native near-plane clipping kernel.

Usage: python3 tools/test-clip-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness builds the
quarter-square and projection tables, provides a RENDERER image and compares
fast_clip_triangle (the native body of the renderer's GeometrySink::triangle)
with a C transcription of the reference: corners all at or behind the near
plane emitted directly, all in front dropped, mixed triangles clipped with
near_intersection in exact 32-bit arithmetic (Q15 interpolation, arithmetic
shift) into a three- or four-corner polygon emitted as (p0, p1, p2) and
(p0, p2, p3). Random triangles straddle the plane at every depth scale,
including coordinate differences beyond sixteen bits; the emitted command
bytes, counts and drop counters must match. The JSON report stays under
target/clip-test.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/clip-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/multiply.S", root / "target-c64/asm/project.S",
              root / "target-c64/asm/emit.S", root / "target-c64/asm/division.S"]
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
extern void fast_clip_triangle(const struct Vec3 points[3], uint8_t color, uint8_t flags);
#define QLO ((uint8_t*)0xc000)
#define QHI ((uint8_t*)0xc200)
#define CACHE ((uint8_t*)0x8e00)
uint16_t PROJECT_X[128], PROJECT_Y[128];
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
static void emit(uint8_t *r, const struct Vec3 p[3], uint8_t color, uint8_t flags){
  int16_t sx[3], sy[3];
  for(uint8_t i=0;i<3;i++){ int16_t out[2]; project(&p[i],out); sx[i]=out[0]; sy[i]=out[1]; }
  int32_t area=((int32_t)sx[1]-sx[0])*((int32_t)sy[2]-sy[0])-((int32_t)sy[1]-sy[0])*((int32_t)sx[2]-sx[0]);
  if(area<=0)return;
  if((sx[0]<=0&&sx[1]<=0&&sx[2]<=0)||(sx[0]>=160&&sx[1]>=160&&sx[2]>=160)||
     (sy[0]<=0&&sy[1]<=0&&sy[2]<=0)||(sy[0]>=200&&sy[1]>=200&&sy[2]>=200))return;
  uint16_t count=(uint16_t)(r[0]|r[1]<<8);
  if(count==256){ uint16_t dropped=(uint16_t)(r[2]|r[3]<<8); if(dropped!=65535)dropped++; r[2]=(uint8_t)dropped; r[3]=(uint8_t)(dropped>>8); return; }
  uint32_t sum=(uint32_t)(uint16_t)p[0].z+(uint16_t)p[1].z+(uint16_t)p[2].z;
  uint16_t depth=(uint16_t)(sum/3);
  uint8_t *c=r+6+count*16;
  for(uint8_t i=0;i<3;i++){ c[i*2]=(uint8_t)sx[i]; c[i*2+1]=(uint8_t)(sx[i]>>8); c[6+i*2]=(uint8_t)sy[i]; c[7+i*2]=(uint8_t)(sy[i]>>8); }
  c[12]=(uint8_t)depth; c[13]=(uint8_t)(depth>>8); c[14]=color&15; c[15]=flags&0x62;
  count++; r[0]=(uint8_t)count; r[1]=(uint8_t)(count>>8);
}
/* Rust: div_i32 truncates toward zero; here numerator and denominator are positive. */
static struct Vec3 near_intersection(struct Vec3 a, struct Vec3 b){
  if(!(a.z<b.z)){ struct Vec3 t=a; a=b; b=t; }
  int32_t t=(((int32_t)8-a.z)<<15)/((int32_t)b.z-a.z);
  struct Vec3 r;
  r.x=(int16_t)((int32_t)a.x+((((int32_t)b.x-a.x)*t)>>15));
  r.y=(int16_t)((int32_t)a.y+((((int32_t)b.y-a.y)*t)>>15));
  r.z=8;
  return r;
}
static void reference(const struct Vec3 p[3], uint8_t color, uint8_t flags){
  uint8_t in[3]={p[0].z>=8,p[1].z>=8,p[2].z>=8};
  if(in[0]&&in[1]&&in[2]){ emit(reference_renderer,p,color,flags); return; }
  if(!in[0]&&!in[1]&&!in[2]) return;
  struct Vec3 poly[4]; uint8_t len=0; struct Vec3 previous=p[2]; uint8_t previous_in=in[2];
  for(uint8_t i=0;i<3;i++){
    if(previous_in!=in[i]) poly[len++]=near_intersection(previous,p[i]);
    if(in[i]) poly[len++]=p[i];
    previous=p[i]; previous_in=in[i];
  }
  if(len<3) return;
  emit(reference_renderer,poly,color,flags);
  if(len==4){ struct Vec3 second[3]={poly[0],poly[2],poly[3]}; emit(reference_renderer,second,color,flags); }
}
static uint32_t checks, mixed, commands;
static uint32_t state=0x2545F491u;
static uint32_t next(void){ state=state*1664525u+1013904223u; return state; }
static int16_t pick(uint32_t bits, int16_t span){ return (int16_t)((int32_t)(bits%(2*span+1))-span); }
static int check(const struct Vec3 p[3], uint8_t color, uint8_t flags){
  checks++;
  uint8_t in=(p[0].z>=8)+(p[1].z>=8)+(p[2].z>=8);
  if(in&&in<3)mixed++;
  reference(p,color,flags);
  fast_clip_triangle(p,color,flags);
  if(memcmp(RENDERER,reference_renderer,sizeof RENDERER)){
    uint16_t at=0; while(at<sizeof RENDERER&&RENDERER[at]==reference_renderer[at])at++;
    printf("FAIL check %lu at byte %u: got %02x want %02x; count %u/%u; points (%d,%d,%d) (%d,%d,%d) (%d,%d,%d) color %u flags %02x\n",
      (unsigned long)checks,at,RENDERER[at],reference_renderer[at],RENDERER[0]|RENDERER[1]<<8,reference_renderer[0]|reference_renderer[1]<<8,
      p[0].x,p[0].y,p[0].z,p[1].x,p[1].y,p[1].z,p[2].x,p[2].y,p[2].z,color,flags);
    uint16_t index=(uint16_t)((at>=6?at-6:0)/16); const int16_t *c=(const int16_t*)(RENDERER+6+index*16); const int16_t *w=(const int16_t*)(reference_renderer+6+index*16);
    printf(" command %u kernel: x %d %d %d y %d %d %d depth %d\n command %u ref:    x %d %d %d y %d %d %d depth %d\n",index,c[0],c[1],c[2],c[3],c[4],c[5],c[6],index,w[0],w[1],w[2],w[3],w[4],w[5],w[6]);
    return 1;
  }
  return 0;
}
int main(void){
  uint16_t square=0;
  for(uint16_t n=0;n<512;n++){QLO[n]=(uint8_t)square;QHI[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));}
  for(uint16_t i=0;i<128;i++){uint32_t d=i+128; PROJECT_X[i]=(uint16_t)((80ul*32768ul+d/2)/d); PROJECT_Y[i]=(uint16_t)((133ul*32768ul+d/2)/d);}
  memset(CACHE,0,640); memset(RENDERER,0,6); memset(reference_renderer,0,6);
  /* Clipping preserves positive winding and never revives a reversed face,
     even when every formerly meaningful sidedness bit is set. */
  {
    struct Vec3 front[3]={{-16,-16,4},{0,16,64},{16,-16,64}};
    struct Vec3 back[3]={front[0],front[2],front[1]};
    if(check(front,2,0xff)||check(back,2,0xff))return 1;
    if(RENDERER[0]!=2||RENDERER[1]){puts("FAIL clipped winding rejection");return 1;}
  }
  /* authored: plane-touching depths, extreme coordinate differences, identical corners */
  static const struct Vec3 authored[][3]={
    {{0,0,7},{100,0,100},{0,100,100}}, {{0,0,8},{100,0,100},{0,100,100}}, {{0,0,8},{100,0,7},{0,100,7}},
    {{-32768,-32768,-32768},{32767,32767,32767},{0,0,8}}, {{32767,-32768,0},{-32768,32767,16},{100,100,-1}},
    {{-32768,0,1},{32767,0,9},{0,32767,9}}, {{5,5,5},{5,5,5},{5,5,50}}, {{10,10,-100},{10,10,-100},{300,300,300}},
    {{0,0,0},{200,0,100},{0,200,100}}, {{-8191,-8191,7},{8191,-8191,9},{0,8191,8}}, {{1,2,3},{4,5,6},{7,8,9}},
    {{-1000,-1000,-1000},{1000,1000,1000},{1000,-1000,8}}, {{0,0,8},{0,0,8},{0,0,8}}, {{0,0,7},{0,0,7},{0,0,7}},
  };
  for(uint8_t t=0;t<sizeof authored/sizeof authored[0];t++){
    for(uint8_t order=0;order<6;order++){
      uint8_t first=order%3, direction=order<3?1:2;
      struct Vec3 p[3]={authored[t][first],authored[t][(first+direction)%3],authored[t][(first+2*direction)%3]};
      for(uint8_t flags=0;flags<2;flags++) if(check(p,(uint8_t)(t&15),(uint8_t)(flags|0x62)))return 1;
    }
  }
  for(uint16_t t=0;t<1500;t++){
    struct Vec3 p[3];
    uint8_t scale=(uint8_t)((next()>>8)%4);
    for(uint8_t i=0;i<3;i++){
      if(scale==0){ p[i].x=pick(next()>>4,400); p[i].y=pick(next()>>4,300); p[i].z=(int16_t)(-120+(next()>>8)%300); }
      else if(scale==1){ p[i].x=pick(next()>>4,8000); p[i].y=pick(next()>>4,8000); p[i].z=(int16_t)(-3000+(next()>>8)%6000); }
      else if(scale==2){ p[i].x=(int16_t)(next()>>16); p[i].y=(int16_t)(next()>>16); p[i].z=(int16_t)(next()>>16); }
      else { p[i].x=pick(next()>>4,60); p[i].y=pick(next()>>4,40); p[i].z=(int16_t)(4+(next()>>8)%9); }
    }
    if((next()>>9)%6==0){ uint16_t c=(uint16_t)((RENDERER[0]|RENDERER[1]<<8)); if(c>240){ memset(RENDERER,0,6); memset(reference_renderer,0,6); } }
    if(check(p,(uint8_t)(next()>>8),(uint8_t)((next()>>8)&0x63)))return 1;
    if((RENDERER[0]|RENDERER[1]<<8)>=250){ commands+=(RENDERER[0]|RENDERER[1]<<8); memset(RENDERER,0,6); memset(reference_renderer,0,6); }
  }
  commands+=(RENDERER[0]|RENDERER[1]<<8);
  printf("PASS %lu triangles, %lu mixed, %lu commands\n",(unsigned long)checks,(unsigned long)mixed,(unsigned long)commands);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native clip check did not finish")
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "Renderer::triangle (near-plane classification, near_intersection, polygon emission) transcribed to C with exact 32-bit arithmetic",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
