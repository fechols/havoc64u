#!/usr/bin/env python3
"""Optional native 6502/Rust differential check; run in the configured WSL SDK.

Usage: python3 tools/test-pack-kernel.py
Requires stable Rust plus LLVM-MOS's mos-c64-clang, mos-sim-clang, and mos-sim.
All generated fixtures, binaries, and the JSON report stay under target/pack-test.
Simulator cycle counts include the comparison harness; they are not game FPS.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/pack-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
rust_bin = pathlib.Path(os.environ.get("CARGO_HOME", str(pathlib.Path.home()/".cargo"))) / "bin"
os.environ["PATH"] = str(rust_bin) + os.pathsep + str(sdk) + os.pathsep + os.environ.get("PATH", "")

subprocess.run(["cargo", "+stable", "build", "--lib"], cwd=root, check=True)
libraries = list((root/"target/debug/deps").glob("libhavoc-*.rlib"))
library = max(libraries, key=lambda path: path.stat().st_mtime)
subprocess.run(["rustc", "+stable", "--edition=2021", root/"tools/pack-fixtures.rs",
                "--extern", f"havoc={library}", "-L", f"dependency={root/'target/debug/deps'}",
                "-o", build/"generate"], cwd=root, check=True)
subprocess.run([build/"generate", build/"fixtures"], cwd=root, check=True)
subprocess.run([sdk/"mos-c64-clang", "-c", root/"target-c64/asm/pack.S", "-o", build/"pack.o"], check=True)
distance_bytes = (build/"fixtures/distances.bin").read_bytes()
distance = [int.from_bytes(distance_bytes[n:n+2], "little") for n in range(0, 512, 2)]
cases = sorted((build/"fixtures").glob("case-*.bin"))
report = {
    "assembly_sha256": hashlib.sha256((root/"target-c64/asm/pack.S").read_bytes()).hexdigest(),
    "reference": "actual host Rust render::pack_band outputs (pack_cell per cell with the band's background colour)",
    "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
    "cycles_include_test_harness": True,
    "batches": [],
}
for batch in range(0, len(cases), 8):
    selected = cases[batch:batch+8]
    rows = [path.read_bytes() for path in selected]
    assert all(len(row) == 1761 for row in rows)
    source = '#include <stdint.h>\n#include <stdio.h>\n'
    source += 'const uint16_t COLOR_DISTANCE[256]={' + ','.join(map(str, distance)) + '};\n'
    source += 'static const uint8_t cases[][1761]={\n' + ',\n'.join('{'+','.join(map(str, row))+'}' for row in rows) + '};\n'
    source += r'''
extern void fast_pack_cell(uint8_t cell, uint8_t *bitmap_cell, uint8_t *screen_band, uint8_t *color_band);
static uint8_t actual[402];
/* The kernel reads the cell's pixels at the game's fixed band address and its
   preceding attributes in place from the screen and colour arrays, which it
   then overwrites; the background colour is the cell writer's zero-page byte. */
#define BAND ((uint8_t*)0x8c00)
#define BACKGROUND (*(volatile uint8_t*)0xff)
int main(void){
  if((uint16_t)(uintptr_t)&actual[401] >= 0x7700u){puts("harness overlaps kernel scratch");return 4;}
  for(uint8_t n=0;n<sizeof(cases)/sizeof(cases[0]);n++){
    actual[0]=0xa5;actual[401]=0x5a;
    for(uint16_t i=1;i<=320;i++)actual[i]=0xcd;
    for(uint16_t i=0;i<1280;i++)BAND[i]=cases[n][i];
    for(uint8_t cell=0;cell<40;cell++){ actual[321+cell]=cases[n][1280+cell]; actual[361+cell]=cases[n][1320+cell]; }
    BACKGROUND=cases[n][1760];
    for(uint8_t cell=0;cell<40;cell++) fast_pack_cell(cell,actual+1+cell*8,actual+321,actual+361);
    if(actual[0]!=0xa5||actual[401]!=0x5a){puts("output guard corrupted");return 2;}
    for(uint16_t i=0;i<400;i++)if(actual[i+1]!=cases[n][1360+i]){
      printf("case %u byte %u got %u expected %u (background %u)\n",n,i,actual[i+1],cases[n][1360+i],cases[n][1760]);return 1;
    }
  }
  puts("native pack matches Rust reference");return 0;
}
'''
    (build/"check.c").write_text(source)
    subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", build/"pack.o", "-o", build/"check"], check=True)
    result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
    print(f"cases {batch}..{batch+len(rows)-1}: {result.stdout.strip()} {result.stderr.strip()}", flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
    report["batches"].append({"first_case": batch, "count": len(rows), "simulator_output": result.stdout.strip()+" "+result.stderr.strip()})
report.update(passed=True, bands=len(cases), compared_output_bytes=len(cases)*400, output_guards_intact=True)
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n")
print(f"PASS: {len(cases)} full bands, {len(cases)*400} native output bytes versus Rust; guard bytes intact")
