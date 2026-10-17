#!/usr/bin/env python3
"""Run the production Rust radix sorter on the MOS CPU, including index 255."""
import hashlib
import json
import os
import pathlib
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
#[no_mangle] pub unsafe extern "C" fn check_sort(data:*mut Triangle,count:u16) {
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
harness = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { int16_t x[3],y[3],depth; uint8_t color,reserved; } Triangle;
extern void check_sort(Triangle*,uint16_t);
static struct { uint8_t before[16]; Triangle data[256]; uint8_t after[16]; } actual;
static Triangle expected[256];
static const uint16_t lengths[]={0,1,2,15,16,57,128,255,256};
static uint32_t random_state=0x31987685;
int main(void) {
  uint16_t cases=0;
  for(uint8_t length=0;length<9;length++) for(uint8_t pattern=0;pattern<6;pattern++) {
    uint16_t count=lengths[length];
    memset(&actual,0xa5,sizeof(actual));
    for(uint16_t i=0;i<count;i++) {
      random_state=random_state*1664525UL+1013904223UL;
      int16_t depth;
      switch(pattern) {
        case 0: depth=7; break;
        case 1: depth=i; break;
        case 2: depth=-(int16_t)i; break;
        case 3: depth=(i&3)*4096; break;
        case 4: { static const int16_t values[]={-32768,32767,-1,0}; depth=values[i&3]; break; }
        default: depth=(int16_t)random_state;
      }
      Triangle value={{i,99,-43},{-17,i,200},depth,i&15,i};
      actual.data[i]=value; expected[i]=value;
    }
    for(uint16_t i=1;i<count;i++) {
      Triangle value=expected[i]; uint16_t j=i;
      while(j && expected[j-1].depth<value.depth) { expected[j]=expected[j-1]; j--; }
      expected[j]=value;
    }
    *(volatile uint8_t*)0x8bff=0xa5;
    *(volatile uint8_t*)0x8e80=0x5a;
    for(uint8_t repeat=0;repeat<2;repeat++) {
      check_sort(actual.data,count);
      if(memcmp(actual.data,expected,count*sizeof(Triangle))) {
        printf("sort mismatch count=%u pattern=%u repeat=%u\n",count,pattern,repeat);return 1;
      }
      for(uint8_t i=0;i<16;i++) if(actual.before[i]!=0xa5 || actual.after[i]!=0xa5) {
        puts("triangle guard damaged");return 2;
      }
      if(*(volatile uint8_t*)0x8bff!=0xa5 || *(volatile uint8_t*)0x8e80!=0x5a) {
        puts("sort workspace guard damaged");return 3;
      }
    }
    cases++;
  }
  printf("PASS %u cases, each sorted twice\n",cases);return 0;
}
'''
(build / "check.c").write_text(harness)
subprocess.run([sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", build / "check.c", build / "sort.a", root / "target-c64/asm/sort.S", "-o", build / "check"], check=True)
result = subprocess.run([sdk / "mos-sim", "--cycles", build / "check"], text=True, capture_output=True, timeout=60)
print(result.stdout.strip(), result.stderr.strip())
report = {
    "passed": result.returncode == 0,
    "source_sha256": hashlib.sha256(sort_source.encode()).hexdigest(),
    "assembly_sha256": hashlib.sha256((root / "target-c64/asm/sort.S").read_bytes()).hexdigest(),
    "cases": 54, "sorts": 108, "capacity": 256,
    "reference": "independent stable C insertion sort on identical whole records",
    "cpu": "LLVM-MOS 6502 simulator", "cycles_include_harness": True,
    "output": result.stdout + result.stderr,
}
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
raise SystemExit(result.returncode)
