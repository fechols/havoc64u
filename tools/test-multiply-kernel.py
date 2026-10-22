#!/usr/bin/env python3
"""Differential check of the native Q8 multiply, mesh-matrix and vertex kernels.

Usage: python3 tools/test-multiply-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness builds the same
quarter-square table BOOT builds at $C000 and compares fast_mul_q8 with
(int16_t)(((int32_t)a * b) >> 8) over every multiplier in -512..512 for a
sweep of multiplicands, all sign/magnitude corners, and pseudo-random pairs;
fast_mesh_matrix against RotationMatrix::compose (clamped to +-256); and
fast_rotate_vertex against (sum m[i][j] * v[j]) >> shift for shifts 4..9 over
the full i8 vertex range. The JSON report stays under target/multiply-test.
Cycle totals include the harness.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/multiply-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assembly = root / "target-c64/asm/multiply.S"
source = r'''
#include <stdint.h>
#include <stdio.h>
extern int16_t fast_mul_q8(int16_t a, int16_t b);
struct Vec3 { int16_t x, y, z; };
extern void fast_mesh_matrix(const int16_t sincos[6], int16_t out[9]);
extern void fast_rotate_vertex(const int8_t v[3], const int16_t m[9], struct Vec3 *out, uint8_t shift);
#define QLO ((uint8_t*)0xc000)
#define QHI ((uint8_t*)0xc200)
static int16_t reference(int16_t a, int16_t b){ return (int16_t)(((int32_t)a*(int32_t)b)>>8); }
static int16_t clamp256(int16_t v){ return v>256?256:(v<-256?-256:v); }
/* RotationMatrix::compose: M = R_roll * R_pitch * R_yaw, each product rounded
   like mul_q8 in the kernel's table order, entries clamped to +-256. */
static void reference_matrix(const int16_t s[6], int16_t m[9]){
  int16_t sy=s[0],cy=s[1],sp=s[2],cp=s[3],sr=s[4],cr=s[5];
  int16_t t1=reference(sp,sy), t2=reference(sp,cy);
  m[0]=clamp256((int16_t)(reference(cr,cy)-reference(sr,t1)));
  m[1]=clamp256((int16_t)(-reference(sr,cp)));
  m[2]=clamp256((int16_t)(reference(cr,sy)+reference(sr,t2)));
  m[3]=clamp256((int16_t)(reference(sr,cy)+reference(cr,t1)));
  m[4]=clamp256(reference(cr,cp));
  m[5]=clamp256((int16_t)(reference(sr,sy)-reference(cr,t2)));
  m[6]=clamp256((int16_t)(-reference(cp,sy)));
  m[7]=clamp256(reference(sp,256));
  m[8]=clamp256(reference(cp,cy));
}
static void reference_rotate(const int8_t v[3], const int16_t m[9], struct Vec3 *out, uint8_t shift){
  int16_t *o=&out->x;
  for(uint8_t i=0;i<3;i++){
    int32_t sum=(int32_t)m[i*3]*v[0]+(int32_t)m[i*3+1]*v[1]+(int32_t)m[i*3+2]*v[2];
    o[i]=(int16_t)(sum>>shift);
  }
}
static uint32_t checks, matrices, rotations;
static int check(int16_t a,int16_t b){
  int16_t got=fast_mul_q8(a,b), want=reference(a,b);
  checks++;
  if(got!=want){printf("FAIL a=%d b=%d got %d want %d\n",a,b,got,want);return 1;}
  return 0;
}
static int16_t q8_angle(uint32_t bits){
  /* Sine-like values across the full Q8 range including +-256. */
  uint8_t kind=bits%9;
  if(kind==0)return 256; if(kind==1)return -256; if(kind==2)return 0;
  return (int16_t)((bits>>4)&0x1ff)-256;
}
int main(void){
  uint16_t square=0;
  for(uint16_t n=0;n<512;n++){QLO[n]=(uint8_t)square;QHI[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));}
  static const int16_t corners[]={-32768,-32767,-32513,-32512,-257,-256,-255,-129,-128,-127,-2,-1,0,1,2,127,128,129,255,256,257,32512,32513,32767};
  for(uint8_t i=0;i<sizeof(corners)/sizeof(corners[0]);i++)
    for(uint8_t j=0;j<sizeof(corners)/sizeof(corners[0]);j++)
      if(check(corners[i],corners[j]))return 1;
  for(int32_t a=-32768;a<=32767;a+=997)
    for(int16_t b=-512;b<=512;b++)
      if(check((int16_t)a,b))return 1;
  uint32_t state=0x2545F491u;
  for(uint16_t i=0;i<20000;i++){
    state=state*1664525u+1013904223u;
    int16_t a=(int16_t)(state>>16), b=(int16_t)(state>>3);
    if(check(a,b))return 1;
    if(check(a,(int16_t)((state>>7)&0x1ff)-256))return 1;
  }
  /* Matrix composition over random sine/cosine sextets (not necessarily
     consistent pairs, which exercises the clamp), then vertex rotations with
     every shift 4..9 over the full i8 range including -128 and the +-256
     matrix entries. */
  for(uint16_t i=0;i<1500;i++){
    int16_t sincos[6], want_m[9], got_m[9];
    for(uint8_t k=0;k<6;k++){state=state*1664525u+1013904223u; sincos[k]=q8_angle(state);}
    if(i<256){ /* genuine single-axis pairs: s, c from a crude table shape */
      int16_t s=(int16_t)((i*2)&0x1ff)-256; int16_t c=(int16_t)(256-(s<0?-s:s));
      for(uint8_t k=0;k<6;k+=2){sincos[k]=(i%3==k/2)?s:0; sincos[k+1]=(i%3==k/2)?c:256;}
    }
    fast_mesh_matrix(sincos,got_m);
    reference_matrix(sincos,want_m);
    matrices++;
    for(uint8_t k=0;k<9;k++) if(got_m[k]!=want_m[k]){
      printf("FAIL matrix %u entry %u got %d want %d\n",i,k,got_m[k],want_m[k]);
      printf(" sincos %d %d %d %d %d %d\n",sincos[0],sincos[1],sincos[2],sincos[3],sincos[4],sincos[5]);
      printf(" got  %d %d %d / %d %d %d / %d %d %d\n",got_m[0],got_m[1],got_m[2],got_m[3],got_m[4],got_m[5],got_m[6],got_m[7],got_m[8]);
      printf(" want %d %d %d / %d %d %d / %d %d %d\n",want_m[0],want_m[1],want_m[2],want_m[3],want_m[4],want_m[5],want_m[6],want_m[7],want_m[8]);
      { const int16_t *slots=(const int16_t*)0x9300; printf(" slots in %d %d %d %d %d %d one %d t %d %d\n",slots[0],slots[1],slots[2],slots[3],slots[4],slots[5],slots[6],slots[7],slots[8]);
        printf(" slots m %d %d %d / %d %d %d / %d %d %d index %u\n",slots[9],slots[10],slots[11],slots[12],slots[13],slots[14],slots[15],slots[16],slots[17],*(uint8_t*)0x9326);
        printf(" out captured %04x actual %04x sincos actual %04x\n",*(uint16_t*)0x9324,(unsigned)(uintptr_t)got_m,(unsigned)(uintptr_t)sincos); }
      return 2;}
    for(uint8_t r=0;r<4;r++){
      state=state*1664525u+1013904223u;
      int8_t v[3]={(int8_t)(state>>8),(int8_t)(state>>16),(int8_t)(state>>24)};
      if(r==0){v[0]=-128;v[1]=127;v[2]=(i&1)?-128:127;}
      uint8_t shift=(uint8_t)(4+(state+r)%6);
      struct Vec3 got={1,2,3}, want={4,5,6};
      fast_rotate_vertex(v,want_m,&got,shift);
      reference_rotate(v,want_m,&want,shift);
      rotations++;
      if(got.x!=want.x||got.y!=want.y||got.z!=want.z){
        printf("FAIL rotate %u shift %u v=(%d,%d,%d) got (%d,%d,%d) want (%d,%d,%d)\n",i,shift,v[0],v[1],v[2],got.x,got.y,got.z,want.x,want.y,want.z);return 3;}
    }
  }
  printf("PASS %lu multiply checks, %lu matrices, %lu vertex rotations\n",(unsigned long)checks,(unsigned long)matrices,(unsigned long)rotations);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", assembly, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native multiply check did not finish")
report = {"passed": True, "assembly_sha256": hashlib.sha256(assembly.read_bytes()).hexdigest(),
          "reference": "(int16_t)(((int32_t)a * b) >> 8)", "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
