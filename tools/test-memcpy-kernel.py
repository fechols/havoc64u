#!/usr/bin/env python3
"""Differential-test the production memcpy on LLVM-MOS's native simulator.

Run in the configured Linux/WSL SDK: python3 tools/test-memcpy-kernel.py
The volatile reference loop cannot be folded into memcpy. Lengths include zero,
page boundaries, 1000 bytes, and larger copies at 81 alignment combinations.
Generated files and the JSON report stay under target/memcpy-test. Simulator
cycles include the entire harness and are not a game frame-rate measurement.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/memcpy-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assembly = root / "target-c64/asm/memcpy.S"
source = r'''
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
extern void *memcpy(void *, const void *, size_t);
static uint8_t source[3328];
static uint8_t actual[3328];
static uint8_t expected[3328];
static const uint16_t lengths[]={0,1,2,127,128,254,255,256,257,511,512,513,999,1000,2048,3072};
static const uint8_t offsets[]={0,1,3,63,127,128,253,254,255};

__attribute__((noinline))
static void reference(volatile uint8_t *dst,const volatile uint8_t *src,uint16_t length){
  for(uint16_t i=0;i<length;++i)dst[i]=src[i];
}
int main(void){
  uint16_t cases=0;
  for(uint16_t i=0;i<sizeof(source);++i)source[i]=(uint8_t)(i*73u+(i>>4)+0x39);
  for(uint8_t n=0;n<sizeof(lengths)/sizeof(lengths[0]);++n){
    uint16_t length=lengths[n];
    for(uint8_t s=0;s<sizeof(offsets);++s){
      for(uint8_t d=0;d<sizeof(offsets);++d){
        for(uint16_t i=0;i<sizeof(actual);++i)actual[i]=expected[i]=(uint8_t)(i*19u+0xcd);
        reference(expected+offsets[d],source+offsets[s],length);
        void *returned=memcpy(actual+offsets[d],source+offsets[s],length);
        if(returned!=actual+offsets[d]){puts("wrong return pointer");return 1;}
        for(uint16_t i=0;i<sizeof(actual);++i)if(actual[i]!=expected[i]){
          printf("mismatch len=%u src=%u dst=%u i=%u got=%u expected=%u\n",length,offsets[s],offsets[d],i,actual[i],expected[i]);
          return 2;
        }
        ++cases;
      }
    }
  }
  if(memcpy((void *)0x1234,(const void *)0x5678,0)!=(void *)0x1234)return 3;
  for(uint16_t i=0;i<sizeof(source);++i)if(source[i]!=(uint8_t)(i*73u+(i>>4)+0x39)){
    puts("source changed");return 4;
  }
  printf("PASS %u native memcpy cases; return pointers, source, and destination guards intact\n",cases+1);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", "-fno-builtin-memcpy",
                build/"check.c", assembly, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode:
    raise SystemExit(result.returncode)
if "PASS 1297 native memcpy cases" not in result.stdout:
    raise SystemExit("simulator did not report completion of all memcpy cases")
report = {
    "assembly_sha256": hashlib.sha256(assembly.read_bytes()).hexdigest(),
    "reference": "independent volatile byte-copy loop",
    "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
    "cases": 1297,
    "alignments_per_length": 81,
    "lengths": [0,1,2,127,128,254,255,256,257,511,512,513,999,1000,2048,3072],
    "cycles_include_test_harness": True,
    "simulator_output": result.stdout.strip()+" "+result.stderr.strip(),
    "passed": True,
}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
