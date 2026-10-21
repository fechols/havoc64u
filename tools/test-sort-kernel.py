#!/usr/bin/env python3
"""Run the production depth sort on the MOS CPU through its Rust wrapper.

Usage: python3 tools/test-sort-kernel.py
Requires LLVM-MOS's rustc target, mos-sim-clang and mos-sim. The kernel must
leave the commands untouched and write a permutation to the order page such
that commands[order[i]] is the i-th record of a stable descending signed-depth
sort (an independent C insertion sort over whole records), for every length
up to the 256-command capacity, including the single-command case that still
records position 0 and the empty case that writes nothing, with and without
scenery segments (the foreground [prefix, pivot) first, then the scenery
prefix, then the rest). Guards surround
the commands, the order page and the $8C00-$8FFF workspace. The JSON report
stays under target/sort-test.
"""
import hashlib
import json
import os
import pathlib
import re
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/sort-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home() / ".local/llvm-mos"))) / "bin"
dependencies = root / "target-c64/target/mos-c64-none/release/deps"
core = max(dependencies.glob("libcore-*.rlib"), key=lambda p: p.stat().st_mtime)
builtins = max(dependencies.glob("libcompiler_builtins-*.rlib"), key=lambda p: p.stat().st_mtime)
source = '''#![no_std]
#[derive(Clone,Copy)] #[repr(C)]
pub struct Triangle { x:[i16;3], y:[i16;3], depth:i16, color:u8, reserved:u8 }
mod projection { pub fn begin_frame() { unsafe { core::ptr::write_bytes(0x8c00 as *mut u8,0,640); } } }
mod sort {
'''
sort_source = (root / "src/render/sort.rs").read_text()
source += "\n".join(line for line in sort_source.splitlines() if not line.startswith("//!"))
source += '''
}
#[no_mangle] pub unsafe extern "C" fn check_sort(data:*mut Triangle,count:u16,prefix:u8,pivot:u8) {
    sort::set_segments(prefix,pivot);
    sort::depth_sort(core::slice::from_raw_parts_mut(data,count as usize));
}
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { loop {} }
'''
(build / "sort.rs").write_text(source)
subprocess.run([
    "rustc", "+mos", build / "sort.rs", "--edition=2021", "--crate-type=staticlib",
    "--target=mos-c64-none", "-L", f"dependency={dependencies}",
    "--extern", f"core={core}", "--extern", f"compiler_builtins={builtins}",
    "-C", "opt-level=z", "-C", "panic=abort", "-C", "lto=fat", "-o", build / "sort.a",
], check=True)
# The harness keeps the order page at $A000 (production: $7600) so the image
# and the kernel workspace cannot meet it.
ORDER = 0xa000
harness = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { int16_t x[3],y[3],depth; uint8_t color,reserved; } Triangle;
extern void check_sort(Triangle*,uint16_t,uint8_t,uint8_t);
#define ORDER ((volatile uint8_t*)%(order)#x)
#define WORK_BELOW (*(volatile uint8_t*)0x8bff)
#define WORK_ABOVE (*(volatile uint8_t*)0x9000)
static struct { uint8_t before[16]; Triangle data[256]; uint8_t after[16]; } actual;
static Triangle expected[256];
static const uint16_t lengths[]={0,1,2,3,15,16,17,57,128,129,180,255,256};
static uint32_t random_state=0x31987685;
static Triangle generate(uint32_t *state, uint16_t i, uint8_t pattern) {
  *state=*state*1664525UL+1013904223UL;
  int16_t depth;
  switch(pattern) {
    case 0: depth=7; break;
    case 1: depth=i; break;
    case 2: depth=-(int16_t)i; break;
    case 3: depth=(i&3)*4096; break;
    case 4: { static const int16_t values[]={-32768,32767,-1,0}; depth=values[i&3]; break; }
    case 5: depth=(int16_t)(40+(*state>>20)%%1200); break;
    case 6: depth=(int16_t)((*state>>16)&0x0f0f); break;
    default: depth=(int16_t)*state;
  }
  Triangle value={{i,99,-43},{-17,i,200},depth,i&15,i};
  return value;
}
int main(void) {
  uint16_t cases=0;
  for(uint8_t length=0;length<sizeof lengths/sizeof lengths[0];length++) for(uint8_t pattern=0;pattern<8;pattern++) for(uint8_t segments=0;segments<4;segments++) {
    uint16_t count=lengths[length];
    /* segments: 0 emission order; 1 scenery prefix then foreground to the end;
       2 a later tail after the pivot; 3 a single-command prefix */
    uint16_t prefix=0,pivot=0;
    if(segments==1&&count>=2){prefix=count/3+1;pivot=count;}
    if(segments==2&&count>=3){prefix=count/4+1;pivot=count-count/5-1;}
    if(segments==3&&count>=2){prefix=1;pivot=count;}
    if(prefix>=pivot){prefix=0;pivot=0;}
    uint32_t seed=random_state;
    memset(&actual,0xa5,sizeof(actual));
    for(uint16_t i=0;i<count;i++) actual.data[i]=generate(&random_state,i,pattern);
    /* the logical sequence, then a stable insertion sort of whole records */
    uint16_t n=0;
    for(uint16_t i=prefix;i<pivot;i++) expected[n++]=actual.data[i];
    for(uint16_t i=0;i<prefix;i++) expected[n++]=actual.data[i];
    for(uint16_t i=pivot;i<count;i++) expected[n++]=actual.data[i];
    if(prefix==0) for(uint16_t i=0;i<count;i++) expected[i]=actual.data[i];
    for(uint16_t i=1;i<count;i++) {
      Triangle value=expected[i]; uint16_t j=i;
      while(j && expected[j-1].depth<value.depth) { expected[j]=expected[j-1]; j--; }
      expected[j]=value;
    }
    for(uint8_t repeat=0;repeat<2;repeat++) {
      for(int16_t i=-1;i<257;i++) ORDER[i]=0xc3;
      WORK_BELOW=0xa5; WORK_ABOVE=0x5a;
      check_sort(actual.data,count,(uint8_t)prefix,(uint8_t)pivot);
      uint32_t replay=seed;
      for(uint16_t i=0;i<count;i++) {
        Triangle value=generate(&replay,i,pattern);
        if(memcmp(&actual.data[i],&value,sizeof value)) { printf("commands moved count=%%u pattern=%%u segments=%%u repeat=%%u\n",count,pattern,segments,repeat);return 1; }
      }
      static uint8_t seen[256];
      memset(seen,0,sizeof seen);
      for(uint16_t i=0;i<count;i++) {
        uint8_t index=ORDER[i];
        if(index>=count || seen[index] || memcmp(&actual.data[index],&expected[i],sizeof(Triangle))) {
          printf("order mismatch count=%%u pattern=%%u segments=%%u (prefix %%u pivot %%u) repeat=%%u position=%%u index=%%u\n",count,pattern,segments,prefix,pivot,repeat,i,index);return 1;
        }
        seen[index]=1;
      }
      for(uint16_t i=count;i<256;i++) if(ORDER[i]!=0xc3) { printf("order written past count=%%u at %%u\n",count,i);return 2; }
      if(ORDER[-1]!=0xc3 || ORDER[256]!=0xc3) { puts("order page guard damaged");return 2; }
      for(uint8_t i=0;i<16;i++) if(actual.before[i]!=0xa5 || actual.after[i]!=0xa5) { puts("triangle guard damaged");return 2; }
      if(WORK_BELOW!=0xa5 || WORK_ABOVE!=0x5a) { puts("sort workspace guard damaged");return 3; }
    }
    cases++;
  }
  printf("PASS %%u cases, each sorted twice\n",cases);return 0;
}
''' % {"order": ORDER}
(build / "check.c").write_text(harness)
subprocess.run([sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", f"-Wa,-defsym,ORDER={ORDER:#x}", build / "check.c", build / "sort.a",
                root / "target-c64/asm/sort.S", "-Wl,-Map=" + str(build / "check.map"), "-o", build / "check"], check=True)
layout = (build / "check.map").read_text(encoding="utf-8", errors="replace")
heap = re.search(r"^\s*([0-9a-f]+)\s+[0-9a-f]+\s+[0-9a-f]+\s+\d+\s+__heap_start", layout, re.M)
if heap is None or int(heap.group(1), 16) > 0x8bff:
    raise SystemExit(f"harness image reaches {heap.group(1) if heap else '?'}: overlaps the sort workspace at $8C00")
result = subprocess.run([sdk / "mos-sim", "--cycles", build / "check"], text=True, capture_output=True, timeout=120)
print(result.stdout.strip(), result.stderr.strip())
report = {
    "passed": result.returncode == 0 and "PASS 416 cases" in result.stdout,
    "source_sha256": hashlib.sha256(sort_source.encode()).hexdigest(),
    "assembly_sha256": hashlib.sha256((root / "target-c64/asm/sort.S").read_bytes()).hexdigest(),
    "cases": 416, "sorts": 832, "capacity": 256, "segment_orders": 4,
    "contract": "commands unchanged; order page holds the permutation of a stable descending signed-depth sort over the logical segment order",
    "reference": "independent stable C insertion sort on identical whole records",
    "cpu": "LLVM-MOS 6502 simulator", "cycles_include_harness": True,
    "output": result.stdout + result.stderr,
}
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
raise SystemExit(0 if report["passed"] else (result.returncode or 1))
