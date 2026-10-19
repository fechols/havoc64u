#!/usr/bin/env python3
"""Differential-test signed swept-depth assembly on the LLVM-MOS simulator.

Uses host-generated inclusive-interval oracles, signed extrema, wrapped bounds,
reversed endpoints, exact contacts, and deterministic random i16 cases. Also
compiles a frozen Rust baseline and compares size/cycles with the same harness.
Run under the configured Linux/WSL toolchain; outputs stay in target/game-math-test.
"""
import hashlib
import json
import os
from pathlib import Path
import random
import re
import subprocess

root = Path(__file__).resolve().parents[1]
build = root / "target/game-math-test"
build.mkdir(parents=True, exist_ok=True)
sdk = Path(os.environ.get("LLVM_MOS", str(Path.home() / ".local/llvm-mos"))) / "bin"
rust = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "bin"
os.environ["PATH"] = str(rust) + os.pathsep + str(sdk) + os.pathsep + os.environ.get("PATH", "")
os.environ.setdefault("RUST_TARGET_PATH", str(Path.home() / ".local/rust-mos-targets"))
dependencies = root / "target-c64/target/mos-c64-none/release/deps"
core = max(dependencies.glob("libcore-*.rlib"), key=lambda path: path.stat().st_mtime)
builtins = max(dependencies.glob("libcompiler_builtins-*.rlib"), key=lambda path: path.stat().st_mtime)
assembly = root / "target-c64/asm/game_math.S"
probe = r'''#![no_std]
#[path="../../src/game/math.rs"] mod kernel;
#[inline(never)]
fn baseline(from:i16,to:i16,target:i16,radius:i16)->bool {
    from.min(to) <= target + radius && from.max(to) >= target - radius
}
#[no_mangle]
pub extern "C" fn baseline_swept(from:i16,to:i16,target:i16,radius:i16)->u8 {
    baseline(from,to,target,radius) as u8
}
#[no_mangle]
pub extern "C" fn production_swept(from:i16,to:i16,target:i16,radius:i16)->u8 {
    kernel::swept_z(from,to,target,radius) as u8
}
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { loop {} }
'''
(build / "probe.rs").write_text(probe)
subprocess.run([
    "rustc", "+mos", build / "probe.rs", "--edition=2021", "--crate-type=lib",
    "--emit=obj", "--target=mos-c64-none", "-L", f"dependency={dependencies}",
    "--extern", f"core={core}", "--extern", f"compiler_builtins={builtins}",
    "-C", "opt-level=z", "-C", "panic=abort", "-C", "overflow-checks=no",
    "-o", build / "probe.o",
], check=True)
subprocess.run([sdk / "mos-sim-clang", "-c", assembly, "-o", build / "game_math.o"], check=True)
symbols = subprocess.run([sdk / "llvm-nm", "-S", "--size-sort", build / "probe.o", build / "game_math.o"],
                         text=True, capture_output=True, check=True).stdout
(build / "symbols.txt").write_text(symbols)


def signed(value):
    return (value + 32768) % 65536 - 32768


def oracle(row):
    start, end, target, radius = row
    return int(min(start, end) <= signed(target + radius)
               and max(start, end) >= signed(target - radius))


def harness(rows, repeats):
    values = ",\n".join("{" + ",".join(map(str, (*row, oracle(row)))) + "}" for row in rows)
    return f'''#include <stdint.h>
#include <stdio.h>
extern uint8_t FUNCTION(int16_t,int16_t,int16_t,int16_t);
static const int16_t cases[][5]={{{values}}};
int main(void) {{
  uint16_t checksum=0;
  for(uint8_t repeat=0;repeat<{repeats};++repeat) {{
    for(uint16_t i=0;i<sizeof(cases)/sizeof(cases[0]);++i) {{
      uint8_t got=FUNCTION(cases[i][0],cases[i][1],cases[i][2],cases[i][3]);
      if(got!=cases[i][4]) {{ printf("FAIL case %u got %u expected %d\\n",i,got,cases[i][4]); return 1; }}
      checksum+=got;
    }}
  }}
  printf("PASS %u cases checksum %u\\n",(unsigned)(sizeof(cases)/sizeof(cases[0])*{repeats}),checksum);
  return 0;
}}
'''


def run(rows, function, repeats=1):
    source = build / "check.c"
    source.write_text(harness(rows, repeats))
    executable = build / function
    subprocess.run([sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", f"-DFUNCTION={function}",
                    source, build / "probe.o", build / "game_math.o", "-o", executable], check=True)
    result = subprocess.run([sdk / "mos-sim", "--cycles", executable], text=True, capture_output=True)
    output = result.stdout.strip() + " " + result.stderr.strip()
    if result.returncode or f"PASS {len(rows)*repeats} cases" not in output:
        raise SystemExit(f"{function}: {output}")
    return output


rows = []
edges = [-32768, -32767, -1, 0, 1, 32766, 32767]
for start in edges:
    for end in edges:
        for target in edges:
            for radius in [-32768, -1, 0, 1, 38, 32767]:
                rows.append((start, end, target, radius))
for target in [0, 40, 280, 680, 1800, 32760, -32760]:
    for radius in [0, 1, 10, 35, 38, 76]:
        for delta in [-1, 0, 1]:
            low, high = signed(target-radius), signed(target+radius)
            rows += [(signed(low+delta), signed(low+delta), target, radius),
                     (signed(high+delta), signed(high+delta), target, radius),
                     (signed(low-1), signed(high+1), target, radius),
                     (signed(high+1), signed(low-1), target, radius)]
rng = random.Random(0x6510)
rows += [tuple(rng.randrange(-32768, 32768) for _ in range(4)) for _ in range(4096)]
for start in range(0, len(rows), 512):
    run(rows[start:start+512], "production_swept")
print(f"PASS {len(rows)} native swept-depth cases against host oracles", flush=True)

benchmark = [(rng.randrange(32, 2401), rng.randrange(32, 2401),
              rng.randrange(40, 2001), rng.choice([10, 35, 38, 76])) for _ in range(256)]
measurements = {}
for function in ["baseline_swept", "production_swept"]:
    output = run(benchmark, function, 8)
    measurements[function] = output
    print(f"{function}: {output}", flush=True)
report = dict(passed=True, cases=len(rows), assembly_sha256=hashlib.sha256(assembly.read_bytes()).hexdigest(),
              cpu="LLVM-MOS mos-sim 6502 instruction simulator", oracle="host-generated inclusive interval comparison with i16 wrapping bounds",
              benchmark_calls=2048, cycles_include_same_validation_harness=True, measurements=measurements,
              object_symbols=symbols)
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
print(symbols, flush=True)
