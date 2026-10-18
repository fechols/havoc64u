#!/usr/bin/env python3
"""Differential-test production raster preparation on the MOS CPU.

The row-walker test starts from host-prepared records. This separate test
compiles the actual Rust entry point and native preparation kernel for MOS,
then checks all 28 head/tail bytes against independent Python arithmetic.
Both entry points run with guarded buffers, including true in-place input.
No production source or final game artifact is changed.
"""
import hashlib
import itertools
import json
import os
import pathlib
import struct
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILD = ROOT / "target/raster-prep-test"
SDK = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home() / ".local/llvm-mos"))) / "bin"


def item(source, declaration):
    """Copy one complete production item verbatim, including its attributes."""
    start = source.index(declaration)
    line = source.rfind("\n", 0, start) + 1
    while line > 0:
        previous = source.rfind("\n", 0, line - 1) + 1
        if not source[previous:line].strip().startswith("#["):
            break
        line = previous
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[line:end]


def fixtures():
    authored = [
        ([0, 160, 0], [0, 0, 200]), ([160, 160, 0], [0, 200, 200]),
        ([-8191, 8191, 0], [-8191, 8191, 100]),
        ([-8191, 8191, 8191], [-8191, -8191, 8191]),
        ([-8191, 8191, -8191], [8191, -8191, -8191]),
        ([0, 159, 80], [-8191, 8191, 100]), ([0, 1, 0], [0, 200, 200]),
        ([159, 160, 160], [0, 0, 200]), ([20, 80, 130], [0, 8, 200]),
        ([20, 80, 130], [-7, 0, 8]), ([20, 80, 130], [191, 192, 200]),
        ([20, 80, 130], [192, 200, 201]), ([0, 160, 80], [0, 1, 0]),
        ([0, 160, 80], [199, 200, 200]), ([20, 80, 130], [-100, -50, -1]),
        ([20, 80, 130], [200, 201, 300]),
        ([81, 80, 80], [-8191, -8190, 8191]),
        ([-8191, 8191, 8190], [0, 1, 200]),
    ]
    for x, y in authored:
        for order in itertools.permutations(range(3)):
            yield [x[i] for i in order], [y[i] for i in order]
    random = 0xC90324AB
    for case in range(1000):
        x, y = [], []
        for _ in range(3):
            random = (random * 1664525 + 1013904223) & 0xFFFFFFFF
            x.append(random % (16383 if case % 2 == 0 else 400) - (8191 if case % 2 == 0 else 120))
            random = (random * 1664525 + 1013904223) & 0xFFFFFFFF
            y.append(random % (16383 if case % 2 == 0 else 400) - (8191 if case % 2 == 0 else 100))
        # Collinear projected faces are rejected before production preparation.
        if (x[1] - x[0]) * (y[2] - y[0]) != (y[1] - y[0]) * (x[2] - x[0]):
            yield x, y


def expected(x, y, color, flags):
    a, b, c = sorted(zip(x, y), key=lambda point: (point[1], point[0]))

    def edge(p, q):
        ax, ay = p
        bx, by = q
        if ay == by or by <= 0 or ay >= 200:
            return 0, 0
        dx, dy = bx - ax, by - ay
        t = ((2 * (max(ay, 0) - ay) + 1) * 16384) // dy
        # Rust signed division truncates toward zero; Python // floors.
        step = abs(dx * 65536) // dy
        if dx < 0:
            step = -step
        return ax * 65536 + dx * t * 2, step

    orientation = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    return struct.pack(
        "<6i4B", *edge(a, c), *edge(a, b), *edge(b, c),
        max(0, min(200, a[1])), max(0, min(200, b[1])), max(0, min(200, c[1])),
        color | (16 if orientation > 0 else 0) | (flags & 0x60),
    )


HARNESS = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void check_prepare(const void*,void*,void*);
extern void fast_prepare_triangle(const void*,void*,void*);
struct Fixture { uint8_t triangle[16],prepared[28]; };
#include "fixtures.h"
static uint8_t input[304],saved_input[304],head[304],tail[304],want_head[304],want_tail[304];
static const uint8_t offsets[]={0,1,127,255};
int main(void) {
  uint16_t cases=0;
  for(uint16_t f=0;f<sizeof(fixtures)/sizeof(fixtures[0]);f++)
    for(uint8_t alignment=0;alignment<4;alignment++)
      for(uint8_t direct=0;direct<2;direct++)
      for(uint8_t inplace=0;inplace<2;inplace++) {
        uint16_t offset=16+offsets[alignment];
        memset(input,0xa5,sizeof(input));memset(head,0xa5,sizeof(head));memset(tail,0xa5,sizeof(tail));
        memset(want_head,0xa5,sizeof(want_head));memset(want_tail,0xa5,sizeof(want_tail));
        memcpy(input+offset,fixtures[f].triangle,16);memcpy(saved_input,input,sizeof(input));
        memcpy(want_head+offset,fixtures[f].prepared,16);
        memcpy(want_tail+offset,fixtures[f].prepared+16,12);
        if(inplace)memcpy(head+offset,fixtures[f].triangle,16);
        *(volatile uint8_t*)0x779f=0xc3;*(volatile uint8_t*)0x77bf=0x3c;
        if(direct)fast_prepare_triangle(inplace?head+offset:input+offset,head+offset,tail+offset);
        else check_prepare(inplace?head+offset:input+offset,head+offset,tail+offset);
        if(memcmp(input,saved_input,sizeof(input)) || memcmp(head,want_head,sizeof(head)) || memcmp(tail,want_tail,sizeof(tail)) || *(volatile uint8_t*)0x779f!=0xc3 || *(volatile uint8_t*)0x77bf!=0x3c) {
          printf("FAIL fixture=%u alignment=%u inplace=%u direct=%u\n",f,alignment,inplace,direct);
          for(uint16_t i=0;i<304;i++)if(head[i]!=want_head[i] || tail[i]!=want_tail[i]) {
            printf("offset=%u head=%u/%u tail=%u/%u\n",i,head[i],want_head[i],tail[i],want_tail[i]);break;
          }
          return 1;
        }
        cases++;
      }
  printf("PASS %u native preparation cases; all head/tail bytes and guards match\n",cases);
  return 0;
}
'''


def main():
    BUILD.mkdir(parents=True, exist_ok=True)
    dependencies = ROOT / "target-c64/target/mos-c64-none/release/deps"
    core = max(dependencies.glob("libcore-*.rlib"), key=lambda path: path.stat().st_mtime)
    builtins = max(dependencies.glob("libcompiler_builtins-*.rlib"), key=lambda path: path.stat().st_mtime)
    render = (ROOT / "src/render.rs").read_text()
    items = "\n".join(item(render, declaration) for declaration in [
        "struct Triangle {", "struct RasterHead {", "struct RasterTail {",
        "fn prepare_edge(", "fn prepare_triangle_into(", "struct Point2 {", "fn sort_pair(",
    ])
    source = f'''#![no_std]
#![allow(dead_code, private_interfaces)]
#[derive(Clone,Copy)] pub struct Vec3 {{ pub x:i16,pub y:i16,pub z:i16 }}
#[path="{ROOT / 'src/math.rs'}"] mod math;
const POSITIVE_AREA:u8=4;
{items}
#[no_mangle] pub unsafe extern "C" fn check_prepare(input:*const Triangle,head:*mut RasterHead,tail:*mut RasterTail) {{
    // Match RasterPass::raster_pass: preserve the old command before replacing
    // those sixteen bytes with its prepared head.
    let triangle=*input;
    prepare_triangle_into(&triangle,&mut *head,&mut *tail);
}}
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! {{ loop {{}} }}
'''
    (BUILD / "prepare.rs").write_text(source)
    subprocess.run([
        "rustc", "+mos", BUILD / "prepare.rs", "--edition=2021", "--crate-type=staticlib",
        "--target=mos-c64-none", "-L", f"dependency={dependencies}",
        "--extern", f"core={core}", "--extern", f"compiler_builtins={builtins}",
        "-C", "opt-level=z", "-C", "panic=abort", "-C", "overflow-checks=off",
        "-C", "lto=fat", "-o", BUILD / "prepare.a",
    ], check=True)
    cases = []
    for index, (x, y) in enumerate(fixtures()):
        area = (x[1] - x[0]) * (y[2] - y[0]) - (y[1] - y[0]) * (x[2] - x[0])
        color = index & 15
        # STATIC_SKY must not leak into raster color or orientation flags.
        flags = (4 if area > 0 else 0) | (2 if index & 1 else 0) | ((index & 3) << 5)
        cases.append((struct.pack("<7h2B", *x, *y, 100, color, flags), expected(x, y, color, flags)))
    (BUILD / "check.c").write_text(HARNESS)
    outputs = []
    for base in range(0, len(cases), 256):
        batch = cases[base:base + 256]
        entries = ["{{" + ",".join(map(str, triangle)) + "},{" + ",".join(map(str, result)) + "}}" for triangle, result in batch]
        (BUILD / "fixtures.h").write_text("static const struct Fixture fixtures[]={\n" + ",\n".join(entries) + "\n};\n")
        executable = BUILD / f"check-{base // 256}"
        subprocess.run([
            SDK / "mos-sim-clang", "-Os", "-mlto-zp=0", BUILD / "check.c", BUILD / "prepare.a",
            ROOT / "target-c64/asm/division.S", ROOT / "target-c64/asm/preparation.S", "-o", executable,
        ], check=True)
        result = subprocess.run([SDK / "mos-sim", "--cycles", executable], text=True, capture_output=True, timeout=60)
        print(f"Batch {base // 256}: {result.stdout.strip()} {result.stderr.strip()}", flush=True)
        if result.returncode or f"PASS {len(batch) * 16} native preparation cases" not in result.stdout:
            raise SystemExit(result.returncode or "native preparation suite did not finish")
        outputs.append(result.stdout.strip() + " " + result.stderr.strip())
    report = {
        "passed": True, "triangles": len(cases), "preparations": len(cases) * 16,
        "alignments": [0, 1, 127, 255], "in_place_and_separate": True,
        "rust_wrapper_and_direct_kernel": True, "scratch_guards_intact": True,
        "reference": "independent Python unbounded integer Q16 preparation",
        "render_source_sha256": hashlib.sha256(render.encode()).hexdigest(),
        "extracted_helpers_sha256": hashlib.sha256(items.encode()).hexdigest(),
        "math_source_sha256": hashlib.sha256((ROOT / "src/math.rs").read_bytes()).hexdigest(),
        "division_assembly_sha256": hashlib.sha256((ROOT / "target-c64/asm/division.S").read_bytes()).hexdigest(),
        "preparation_assembly_sha256": hashlib.sha256((ROOT / "target-c64/asm/preparation.S").read_bytes()).hexdigest(),
        "fixtures_sha256": hashlib.sha256(b"".join(a + b for a, b in cases)).hexdigest(),
        "cpu": "LLVM-MOS 6502 simulator", "cycles_include_harness": True, "output": outputs,
    }
    (BUILD / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
