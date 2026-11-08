#!/usr/bin/env python3
"""Differential check of the compact 8/16-bit division libcalls (asm/libdiv.S).

Usage: python3 tools/test-libdiv-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. Every routine is compared with
libcrt's 32-bit division (which libdiv.S does not replace): all 8-bit pairs,
16-bit edge pairs (0, 1, powers of two and their neighbours, 0x7fff, 0x8000,
0xffff in both operands and signs) and pseudo-random pairs, divisor nonzero,
excluding i16::MIN / -1 (Rust panics before dividing). The JSON report stays
under target/libdiv-test.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/libdiv-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assembly = root / "target-c64/asm/libdiv.S"
source = r'''
#include <stdint.h>
#include <stdio.h>
extern uint16_t __wrap___udivhi3(uint16_t a, uint16_t b);
extern uint16_t __wrap___umodhi3(uint16_t a, uint16_t b);
extern int16_t __wrap___divhi3(int16_t a, int16_t b);
extern uint8_t __wrap___udivqi3(uint8_t a, uint8_t b);
extern uint8_t __wrap___umodqi3(uint8_t a, uint8_t b);
static uint32_t checks;
static int pair(uint16_t a, uint16_t b){
  if(!b) return 0;
  uint16_t q=(uint16_t)((uint32_t)a/(uint32_t)b), r=(uint16_t)((uint32_t)a%(uint32_t)b);
  if(__wrap___udivhi3(a,b)!=q||__wrap___umodhi3(a,b)!=r){ printf("FAIL u16 %u %u\n",a,b); return 1; }
  int16_t sa=(int16_t)a, sb=(int16_t)b;
  if(!(sa==-32768&&sb==-1)){
    int16_t sq=(int16_t)((int32_t)sa/(int32_t)sb);
    if(__wrap___divhi3(sa,sb)!=sq){ printf("FAIL i16 %d %d\n",sa,sb); return 1; } }
  checks+=3; return 0;
}
int main(void){
  for(uint16_t a=0;a<256;a++) for(uint16_t b=1;b<256;b++){
    if(__wrap___udivqi3((uint8_t)a,(uint8_t)b)!=(uint8_t)((uint32_t)a/b)||__wrap___umodqi3((uint8_t)a,(uint8_t)b)!=(uint8_t)((uint32_t)a%b)){
      printf("FAIL u8 %u %u\n",a,b); return 1; }
    checks+=2; }
  static uint16_t edge[64]; uint8_t n=0;
  edge[n++]=0; edge[n++]=0xffff;
  for(uint8_t s=0;s<16;s++){ uint16_t p=(uint16_t)(1u<<s); edge[n++]=p; edge[n++]=(uint16_t)(p-1); edge[n++]=(uint16_t)(p+1); }
  edge[n++]=0x7fff; edge[n++]=0x8001; edge[n++]=12345; edge[n++]=(uint16_t)-12345;
  for(uint8_t i=0;i<n;i++) for(uint8_t j=0;j<n;j++) if(pair(edge[i],edge[j])) return 1;
  uint32_t state=0x2545F491u;
  for(uint16_t i=0;i<30000;i++){
    state=state*1664525u+1013904223u;
    uint16_t a=(uint16_t)(state>>16), b=(uint16_t)(state>>(state&7));
    if(i&1) b&=0xff;
    if(pair(a,b)) return 1; }
  printf("PASS %lu libdiv checks\n",(unsigned long)checks);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", assembly, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "libdiv check did not finish")
report = {"passed": True, "assembly_sha256": hashlib.sha256(assembly.read_bytes()).hexdigest(),
          "reference": "libcrt 32-bit division", "simulator_output": result.stdout.strip() + " " + result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
