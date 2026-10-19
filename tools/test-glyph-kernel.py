#!/usr/bin/env python3
"""Check the production 3x5 glyph kernel on the LLVM-MOS 6502 simulator.

All 256 byte patterns are exercised in each of the five source rows, with four
ink bytes, every destination page alignment, and source-pointer page crossings.
An independent scalar oracle checks whole buffers, including untouched pixels
and guards. An assembly shim checks the documented C ABI register preservation.
Run under scripts/env.sh; artifacts stay in target/glyph-kernel-test.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess


root = Path(__file__).resolve().parents[1]
build = root / "target/glyph-kernel-test"
build.mkdir(parents=True, exist_ok=True)
sdk = Path(os.environ.get("LLVM_MOS", str(Path.home() / ".local/llvm-mos"))) / "bin"
assembly = root / "target-c64/asm/raster.S"
source_hash = hashlib.sha256(assembly.read_bytes()).hexdigest()

# Keep the C argument convention intact while replacing every register the
# kernel promises not to touch with a recognizable value. Restore the caller's
# original registers even if a preservation check fails.
registers = range(8, 32)
shim = [
    *[f".zeropage __rc{register}" for register in registers],
    '.section .text.checked_glyph,"ax",@progbits',
    ".globl checked_glyph",
    ".type checked_glyph,@function",
    "checked_glyph:",
    "    sta abi_ink",
]
for register in registers:
    shim += [f"    lda __rc{register}", "    pha"]
for register in registers:
    shim += [f"    lda #{(0x35 ^ (register * 11)) & 255}", f"    sta __rc{register}"]
shim += ["    lda abi_ink", "    jsr fast_glyph"]
for register in registers:
    shim += [
        f"    lda __rc{register}",
        f"    cmp #{(0x35 ^ (register * 11)) & 255}",
        f"    beq .Lpreserved_{register}",
        f"    lda #{register}",
        "    sta abi_bad_register",
        f".Lpreserved_{register}:",
    ]
for register in reversed(registers):
    shim += ["    pla", f"    sta __rc{register}"]
shim += ["    rts", ".size checked_glyph, .-checked_glyph"]
(build / "abi.S").write_text("\n".join(shim) + "\n")

harness = r'''#include <stdint.h>
#include <stdio.h>

extern void checked_glyph(uint8_t *, const uint8_t *, uint8_t);
volatile uint8_t abi_ink;
volatile uint8_t abi_bad_register;
static uint8_t pixels[1536] __attribute__((aligned(256)));
static uint8_t expected[1536];
static uint8_t font[512] __attribute__((aligned(256)));
static const uint8_t inks[4] = {0, 0x81, 0x8c, 0xff};
static const uint8_t font_offsets[6] = {0, 1, 251, 253, 254, 255};

static uint8_t initial_pixel(uint16_t index, uint16_t serial) {
  return (uint8_t)((index * 29 + serial) ^ (index >> 8));
}
static uint8_t initial_font(uint16_t index) {
  return (uint8_t)(index * 37 + (index >> 8) + 0xa5);
}

int main(void) {
  uint16_t serial = 0;
  uint32_t checksum = 0;
  for (uint8_t varied_row = 0; varied_row < 5; ++varied_row) {
    for (uint16_t pattern = 0; pattern < 256; ++pattern) {
      for (uint8_t ink_index = 0; ink_index < 4; ++ink_index) {
        uint8_t ink = inks[ink_index];
        uint16_t offset = 256 + (uint8_t)(pattern + varied_row * 61 + ink_index * 29);
        uint16_t font_offset = font_offsets[serial % 6];
        uint8_t rows[5];
        for (uint16_t i = 0; i < sizeof(pixels); ++i)
          pixels[i] = expected[i] = initial_pixel(i, serial);
        for (uint16_t i = 0; i < sizeof(font); ++i)
          font[i] = initial_font(i);
        for (uint8_t row = 0; row < 5; ++row) {
          rows[row] = row == varied_row ? (uint8_t)pattern
                     : (uint8_t)(pattern * (row * 2 + 3) + row * 53 + ink_index);
          font[font_offset + row] = rows[row];
          for (uint8_t column = 0; column < 3; ++column)
            if ((rows[row] >> (2 - column)) & 1)
              expected[offset + row * 160 + column] = ink;
        }
        abi_bad_register = 0;
        checked_glyph(pixels + offset, font + font_offset, ink);
        if (abi_bad_register) {
          printf("FAIL case %u preserved register rc%u\n", serial, abi_bad_register);
          return 1;
        }
        for (uint16_t i = 0; i < sizeof(pixels); ++i) {
          if (pixels[i] != expected[i]) {
            printf("FAIL case %u row %u pattern %u ink %u destination %u byte %u got %u expected %u\n",
                   serial, varied_row, pattern, ink, offset, i, pixels[i], expected[i]);
            return 1;
          }
        }
        for (uint16_t i = 0; i < sizeof(font); ++i) {
          uint8_t wanted = i >= font_offset && i < font_offset + 5
                           ? rows[i - font_offset] : initial_font(i);
          if (font[i] != wanted) {
            printf("FAIL case %u source/guard byte %u got %u expected %u\n",
                   serial, i, font[i], wanted);
            return 1;
          }
        }
        checksum += pixels[offset] + pixels[offset + 642];
        ++serial;
      }
    }
  }
  printf("PASS %u cases checksum %lu\n", serial, (unsigned long)checksum);
  return 0;
}
'''
(build / "check.c").write_text(harness)
subprocess.run([sdk / "mos-sim-clang", "-c", assembly, "-o", build / "raster.o"], check=True)
subprocess.run([sdk / "mos-sim-clang", "-c", build / "abi.S", "-o", build / "abi.o"], check=True)
subprocess.run([
    sdk / "mos-sim-clang", "-Os", "-mlto-zp=0", build / "check.c",
    build / "abi.o", build / "raster.o", "-o", build / "glyph-check",
], check=True)
symbols = subprocess.run(
    [sdk / "llvm-nm", "-S", "--size-sort", build / "raster.o", build / "abi.o"],
    text=True, capture_output=True, check=True,
).stdout
(build / "symbols.txt").write_text(symbols)
result = subprocess.run(
    [sdk / "mos-sim", "--cycles", build / "glyph-check"], text=True, capture_output=True,
)
output = result.stdout.strip() + "\n" + result.stderr.strip()
(build / "simulator.log").write_text(output + "\n")
if result.returncode or "PASS 5120 cases" not in output:
    raise SystemExit(output)
assert hashlib.sha256(assembly.read_bytes()).hexdigest() == source_hash, "assembly changed during test"
report = {
    "passed": True,
    "cases": 5120,
    "patterns_per_source_row": 256,
    "source_rows": 5,
    "ink_bytes": [0, 0x81, 0x8c, 0xff],
    "destination_page_offsets": "all 256 offsets for each source row and ink",
    "source_page_offsets": [0, 1, 251, 253, 254, 255],
    "checked_destination_bytes_per_call": 1536,
    "checked_source_bytes_per_call": 512,
    "preserved_registers_checked": "rc8..31, including C ABI callee-saved rc16..31",
    "oracle": "independent scalar 3-bit mask rendering; exact whole-buffer and guard comparisons",
    "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
    "assembly_sha256": source_hash,
    "executable_sha256": hashlib.sha256((build / "glyph-check").read_bytes()).hexdigest(),
    "cycles_include_validation_harness": True,
    "simulator_output": output,
}
(build / "report.json").write_text(json.dumps(report, indent=2) + "\n")
print(output, flush=True)
print(f"PASS: {build / 'report.json'}", flush=True)
