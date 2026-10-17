#!/usr/bin/env python3
"""Optional Rust/6502 division benchmark, after the normal C64 target build.

Run in the configured WSL SDK: python3 tools/benchmark-division.py
Compiles actual production math helpers and a frozen restoring-loop baseline.
Every quotient is checked against a host-generated reference. Simulator cycles
include the identical validation harness and must not be interpreted as FPS.
Artifacts and the JSON report are written to target/division-test.
"""
import hashlib
import json
import os
import pathlib
import random
import subprocess


root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/division-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home() / ".local/llvm-mos"))) / "bin"
rust_bin = pathlib.Path(os.environ.get("CARGO_HOME", str(pathlib.Path.home() / ".cargo"))) / "bin"
os.environ["PATH"] = str(rust_bin) + os.pathsep + str(sdk) + os.pathsep + os.environ.get("PATH", "")
os.environ.setdefault("RUST_TARGET_PATH", str(pathlib.Path.home() / ".local/rust-mos-targets"))
dependencies = root / "target-c64/target/mos-c64-none/release/deps"
cores = list(dependencies.glob("libcore-*.rlib"))
builtins = list(dependencies.glob("libcompiler_builtins-*.rlib"))
if not cores or not builtins:
    raise SystemExit("Build the target with scripts/build.sh first; its MOS core libraries are required.")

# This baseline deliberately remains the original 32-step implementation, so
# later production changes cannot silently change both sides of the comparison.
probe = r'''#![no_std]
#![allow(dead_code)]
#[derive(Clone, Copy)] pub struct Vec3 { pub x:i16, pub y:i16, pub z:i16 }
#[path="../../src/math.rs"] mod math;
#[inline(never)]
fn restoring(numerator:u32, denominator:u32)->u32 {
    if denominator == 0 { return u32::MAX; }
    if numerator < denominator { return 0; }
    let mut quotient = numerator;
    let mut remainder = 0u32;
    let mut bits = 32u8;
    while bits != 0 {
        let overflow = remainder & 0x8000_0000 != 0;
        remainder = (remainder << 1) | (quotient >> 31);
        quotient <<= 1;
        if overflow || remainder >= denominator {
            remainder = remainder.wrapping_sub(denominator);
            quotient |= 1;
        }
        bits -= 1;
    }
    quotient
}
#[inline(never)]
fn restoring_signed(n:i32,d:i32)->i32 {
    let q = restoring(n.unsigned_abs(), d.unsigned_abs()) as i32;
    if (n ^ d) < 0 { q.wrapping_neg() } else { q }
}
#[no_mangle]
pub extern "C" fn baseline_bounded_u16(numerator:u32,denominator:u16)->u16 {
    let mut remainder = (numerator >> 16) as u16;
    let mut quotient = numerator as u16;
    let mut bits = 16u8;
    while bits != 0 {
        let overflow = remainder & 0x8000 != 0;
        remainder = (remainder << 1) | (quotient >> 15);
        quotient <<= 1;
        if overflow || remainder >= denominator {
            remainder = remainder.wrapping_sub(denominator);
            quotient |= 1;
        }
        bits -= 1;
    }
    quotient
}
#[no_mangle] pub extern "C" fn baseline_u32(n:u32,d:u32)->u32 { restoring(n,d) }
#[no_mangle] pub extern "C" fn baseline_i32(n:i32,d:i32)->i32 { restoring_signed(n,d) }
#[no_mangle] pub extern "C" fn production_u32(n:u32,d:u32)->u32 { math::div_u32(n,d) }
#[no_mangle] pub extern "C" fn production_i32(n:i32,d:i32)->i32 { math::div_i32(n,d) }
#[no_mangle] pub extern "C" fn bounded_u16(n:u32,d:u16)->u16 { math::div_u32_u16(n,d) }
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { loop {} }
'''
(build / "probe.rs").write_text(probe)
subprocess.run([
    "rustc", "+mos", build / "probe.rs", "--edition=2021", "--crate-type=lib",
    "--emit=obj", "--target=mos-c64-none", "-L", f"dependency={dependencies}",
    "--extern", f"core={max(cores, key=lambda path: path.stat().st_mtime)}",
    "--extern", f"compiler_builtins={max(builtins, key=lambda path: path.stat().st_mtime)}",
    "-C", "opt-level=z", "-C", "panic=abort", "-C", "overflow-checks=no",
    "-o", build / "probe.o",
], check=True)
symbols = subprocess.run([sdk / "llvm-nm", "-S", "--size-sort", build / "probe.o"],
                         text=True, capture_output=True, check=True).stdout
(build / "symbols.txt").write_text(symbols)
subprocess.run([sdk / "mos-c64-clang", "-c", root / "target-c64/asm/division.S",
                "-o", build / "division.o"], check=True)

rng = random.Random(517)
positive = []
for _ in range(128):
    denominator = rng.randint(1, 16_382)
    quotient = rng.randint(0, 32_767)
    numerator = denominator * quotient + rng.randrange(denominator)
    positive.append((numerator, denominator, quotient))
signed = []
for _ in range(128):
    numerator = rng.randint(-2_147_483_648, 2_147_483_647)
    denominator = rng.randint(-32_767, 32_767) or 1
    quotient = abs(numerator) // abs(denominator)
    if (numerator < 0) != (denominator < 0):
        quotient = -quotient
    signed.append((numerator, denominator, quotient))
signed[:3] = [(-2_147_483_648, -1, -2_147_483_648),
              (-2_147_483_648, 1, -2_147_483_648),
              (2_147_483_647, -1, -2_147_483_647)]


def harness(rows, is_signed):
    kind, suffix = ("int32_t", "L") if is_signed else ("uint32_t", "UL")
    source = "#include <stdint.h>\n#include <stdio.h>\n"
    source += f"static const {kind} cases[][3]={{"
    source += ",".join("{" + ",".join(str(value) + suffix for value in row) + "}" for row in rows)
    source += "};\n"
    source += "#ifdef BOUNDED\nextern uint16_t FUNCTION(uint32_t,uint16_t);\n#else\n"
    source += f"extern {kind} FUNCTION({kind},{kind});\n#endif\n"
    source += f'''int main(void) {{
  uint32_t checksum = 0;
  for (uint8_t repeat = 0; repeat < 4; repeat++) {{
    for (uint8_t i = 0; i < 128; i++) {{
      {kind} actual = FUNCTION(cases[i][0], cases[i][1]);
      if (actual != cases[i][2]) {{
        printf("wrong quotient at case %u\\n", i);
        return 1;
      }}
      checksum += (uint32_t)actual;
    }}
  }}
  printf("PASS checksum %lu\\n", (unsigned long)checksum);
  return 0;
}}
'''
    return source


report = {
    "math_sha256": hashlib.sha256((root / "src/math.rs").read_bytes()).hexdigest(),
    "assembly_sha256": hashlib.sha256((root / "target-c64/asm/division.S").read_bytes()).hexdigest(),
    "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
    "divisions_per_case": 512,
    "cycles_include_validation_harness": True,
    "object_symbols": symbols,
    "measurements": {},
}
for function in ["baseline_u32", "production_u32", "baseline_bounded_u16", "bounded_u16", "baseline_i32", "production_i32"]:
    is_signed = function.endswith("i32")
    (build / "check.c").write_text(harness(signed if is_signed else positive, is_signed))
    command = [sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", f"-DFUNCTION={function}"]
    if function.endswith("bounded_u16"):
        command.append("-DBOUNDED")
    command += [build / "check.c", build / "probe.o", build / "division.o", "-o", build / function,
                f"-Wl,-Map,{build / function}.map"]
    subprocess.run(command, check=True)
    result = subprocess.run([sdk / "mos-sim", "--cycles", build / function],
                            text=True, capture_output=True)
    output = result.stdout.strip() + " " + result.stderr.strip()
    print(f"{function}: {output}", flush=True)
    report["measurements"][function] = {"status": result.returncode, "output": output}
    if result.returncode:
        raise SystemExit(result.returncode)
# Exercise the seventeenth remainder bit as well as quotient and divisor
# boundaries. The rendering workload uses smaller divisors, but the helper's
# documented contract permits the full nonzero u16 range.
bounded_cases = []
for denominator in [1, 2, 3, 7, 255, 256, 32_767, 32_768, 49_152, 65_534, 65_535]:
    for quotient in [0, 1, 2, 32_767, 32_768, 65_534, 65_535]:
        for remainder in [0, denominator - 1]:
            bounded_cases.append((denominator * quotient + remainder, denominator, quotient))
for _ in range(4096):
    denominator = rng.randrange(1, 65_536)
    quotient = rng.randrange(65_536)
    remainder = rng.randrange(denominator)
    bounded_cases.append((denominator * quotient + remainder, denominator, quotient))
for first in range(0, len(bounded_cases), 128):
    rows = bounded_cases[first:first + 128]
    rows += [(0, 1, 0)] * (128 - len(rows))
    (build / "check.c").write_text(harness(rows, False))
    subprocess.run([sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", "-DFUNCTION=bounded_u16", "-DBOUNDED",
                    build / "check.c", build / "probe.o", build / "division.o", "-o", build / "bounded-check"], check=True)
    result = subprocess.run([sdk / "mos-sim", build / "bounded-check"], text=True, capture_output=True)
    if result.returncode:
        raise SystemExit(f"Full-range case batch {first} failed: {result.stdout} {result.stderr}")
report.update(passed=True, bounded_full_range_cases=len(bounded_cases), repetitions_per_case=4)
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
print(f"PASS: benchmarks plus {len(bounded_cases)} full-range bounded quotients match host references; report in target/division-test/report.json")
