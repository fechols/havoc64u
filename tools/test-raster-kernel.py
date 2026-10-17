#!/usr/bin/env python3
"""Optional exact native row-walker test: python3 tools/test-raster-kernel.py.

The actual Rust renderer generates 256 randomized triangles across all 25 bands,
including full-range clipped coordinates and thin edges. Expected spans encode
all 1,280 reference pixels losslessly. The MOS simulator compares every byte
after calling the actual assembly kernel, and checks guard bytes on both sides.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/raster-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home() / ".local/llvm-mos"))) / "bin"
rust_bin = pathlib.Path(os.environ.get("CARGO_HOME", str(pathlib.Path.home() / ".cargo"))) / "bin"
os.environ["PATH"] = str(rust_bin) + os.pathsep + str(sdk) + os.pathsep + os.environ.get("PATH", "")
subprocess.run(["cargo", "+stable", "build", "--lib"], cwd=root, check=True)
libraries = list((root / "target/debug/deps").glob("libhavoc-*.rlib"))
library = max(libraries, key=lambda path: path.stat().st_mtime)
source = "#![allow(dead_code)]\nuse havoc::{Vec3,GeometrySink,TWO_SIDED,math,projection};\nmod render {\n"
source += (root / "src/render.rs").read_text()
source += (root / "tools/raster-fixtures.inc.rs").read_text()
source += '\n}\nfn main(){let path=std::env::args().nth(1).unwrap();render::write_native_fixtures(std::path::Path::new(&path));}\n'
(build / "generate.rs").write_text(source)
subprocess.run(["rustc", "+stable", "--edition=2021", build / "generate.rs", "--extern", f"havoc={library}",
                "-L", f"dependency={root / 'target/debug/deps'}", "-o", build / "generate"], check=True)
subprocess.run([build / "generate", build / "fixtures.bin"], check=True)
subprocess.run([sdk / "mos-c64-clang", "-c", root / "target-c64/asm/raster.S", "-o", build / "raster.o"], check=True)
fixtures = (build / "fixtures.bin").read_bytes()
assert len(fixtures) == 256 * 25 * 56
report = {
    "assembly_sha256": hashlib.sha256((root / "target-c64/asm/raster.S").read_bytes()).hexdigest(),
    "reference": "actual Rust renderer, 256 triangles at all 25 bands",
    "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
    "cycles_include_validation_harness": True,
    "batches": [],
}
for first in range(0, 6400, 128):
    rows = [fixtures[n * 56:(n + 1) * 56] for n in range(first, min(first + 128, 6400))]
    source = "#include <stdint.h>\n#include <stdio.h>\n"
    source += 'static const uint8_t cases[][56]={' + ','.join('{' + ','.join(map(str, row)) + '}' for row in rows) + '};\n'
    source += r'''
extern void fast_raster_rows(uint8_t*,uint8_t*);
static uint8_t job[40];
static uint8_t output[1282];
int main(void) {
  for (uint8_t n = 0; n < sizeof(cases)/sizeof(cases[0]); n++) {
    for (uint8_t i = 0; i < 40; i++) job[i] = cases[n][i];
    for (uint16_t i = 0; i < 1282; i++) output[i] = 0;
    output[0] = 0xa5; output[1281] = 0x5a;
    fast_raster_rows(job, output + 1);
    if (output[0] != 0xa5 || output[1281] != 0x5a) { puts("guard corrupted"); return 1; }
    const uint8_t *pixel = output + 1;
    for (uint8_t row = 0; row < 8; row++) {
      uint8_t left = cases[n][40 + row*2], right = cases[n][41 + row*2];
      for (uint8_t x = 0; x < 160; x++) {
        uint8_t expected = x >= left && x < right ? cases[n][39] : 0;
        if (*pixel++ != expected) { printf("mismatch case %u row %u x %u\n", n, row, x); return 2; }
      }
    }
  }
  puts("native raster matches Rust reference"); return 0;
}
'''
    (build / "check.c").write_text(source)
    subprocess.run([sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", build / "check.c", build / "raster.o", "-o", build / "check"], check=True)
    result = subprocess.run([sdk / "mos-sim", "--cycles", build / "check"], text=True, capture_output=True)
    output = result.stdout.strip() + " " + result.stderr.strip()
    print(f"bands {first}..{first+len(rows)-1}: {output}", flush=True)
    report["batches"].append({"first_band": first, "count": len(rows), "output": output})
    if result.returncode:
        raise SystemExit(result.returncode)
report.update(passed=True, bands=6400, compared_pixel_bytes=6400*1280, guards_intact=True)
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
print("PASS: 6,400 bands / 8,192,000 native pixel bytes match the Rust reference; guards intact")
