#!/usr/bin/env python3
"""Differential check of the native projection kernel.

Usage: python3 tools/test-projection-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness builds the
quarter-square table BOOT builds at $C000 and the same PROJECT_X/PROJECT_Y
tables as projection.rs, then compares fast_project with
projection::project_reference (normalised depth, one 16x16 product per axis,
arithmetic shift, i16 clamps) for every depth 1..32767 at sampled x/y, every
x and y at sampled depths including z <= 0, the clamp corners and
pseudo-random points. The JSON report stays under target/projection-test.
Cycle totals include the harness.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/projection-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/multiply.S", root / "target-c64/asm/project.S"]
source = r'''
#include <stdint.h>
#include <stdio.h>
struct Vec3 { int16_t x, y, z; };
extern void fast_project(const struct Vec3 *point, int16_t out[2]);
#define QLO ((uint8_t*)0xc000)
#define QHI ((uint8_t*)0xc200)
uint16_t PROJECT_X[128], PROJECT_Y[128];
static int16_t clamp16(int32_t v, int32_t lo, int32_t hi){ return (int16_t)(v<lo?lo:(v>hi?hi:v)); }
/* projection::project_reference */
static void reference(const struct Vec3 *p, int16_t out[2]){
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
static uint32_t checks;
static int check(int16_t x,int16_t y,int16_t z){
  struct Vec3 p={x,y,z}; int16_t got[2]={1,2}, want[2]={3,4};
  fast_project(&p,got); reference(&p,want); checks++;
  if(got[0]!=want[0]||got[1]!=want[1]){
    printf("FAIL (%d,%d,%d) got (%d,%d) want (%d,%d)\n",x,y,z,got[0],got[1],want[0],want[1]);return 1;}
  return 0;
}
int main(void){
  uint16_t square=0;
  for(uint16_t n=0;n<512;n++){QLO[n]=(uint8_t)square;QHI[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));}
  for(uint16_t i=0;i<128;i++){uint32_t d=i+128; PROJECT_X[i]=(uint16_t)((80ul*32768ul+d/2)/d); PROJECT_Y[i]=(uint16_t)((133ul*32768ul+d/2)/d);}
  static const int16_t xs[]={-32768,-30001,-30000,-29999,-8192,-8191,-2047,-640,-161,-160,-159,-81,-80,-79,-1,0,1,79,80,81,159,160,161,640,2047,8191,8192,29999,30000,30001,32767};
  static const int16_t zs[]={1,2,7,8,9,15,16,17,31,32,33,63,64,127,128,129,180,255,256,257,300,511,512,513,900,1023,1024,1025,2350,4095,4096,8191,8192,16383,16384,32767,-1,-32768};
  const uint8_t nx=sizeof(xs)/sizeof(xs[0]), nz=sizeof(zs)/sizeof(zs[0]);
  if(check(0,0,0)||check(-32768,32767,0))return 1;
  /* every depth with the clamp corners */
  for(int32_t z=1;z<=32767;z++){
    uint8_t i=(uint8_t)(z%nx), j=(uint8_t)((z*7)%nx);
    if(check(xs[i],xs[j],(int16_t)z))return 1;
    if(check((int16_t)(z*3-20000),(int16_t)(10000-z),(int16_t)z))return 1;
  }
  /* every x and y at the sampled depths */
  for(int32_t v=-32768;v<=32767;v+=3){
    uint8_t k=(uint8_t)(((uint32_t)v>>2)%nz);
    if(check((int16_t)v,(int16_t)(-v),zs[k]))return 1;
  }
  for(uint8_t i=0;i<nx;i++)for(uint8_t j=0;j<nx;j++)for(uint8_t k=0;k<nz;k++)
    if(check(xs[i],xs[j],zs[k]))return 1;
  uint32_t state=0x9e3779b9u;
  for(uint32_t i=0;i<60000;i++){
    state=state*1664525u+1013904223u;
    int16_t x=(int16_t)(state>>16); int16_t y=(int16_t)(state>>5);
    state=state*1664525u+1013904223u;
    int16_t z=(int16_t)(8+(state>>17));
    if(check(x,y,z))return 1;
  }
  printf("PASS %lu projections\n",(unsigned long)checks);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native projection check did not finish")
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "projection::project_reference (normalised depth, PROJECT_X/Y tables)",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
