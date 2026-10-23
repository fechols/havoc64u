"""Check production triangle preparation preserves the LLVM-MOS C ABI.

Independent register canaries check RC0/1, RC20..31, hardware stack and I/D.
Numeric output is covered by test-raster-prep.py separately.
"""
import hashlib
import os
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target/preparation-abi"
SDK = Path(os.environ.get("LLVM_MOS", Path.home() / ".local/llvm-mos")) / "bin"
ASM = r'''
.zeropage __rc0
.zeropage __rc1
.zeropage __rc20
.section .text.preparation_abi,"ax",@progbits
.globl preparation_abi
preparation_abi:
    sta $8800
    php
    pla
    and #$0c
    sta $8801
    tsx
    stx $8802
    lda __rc0
    sta $8803
    lda __rc1
    sta $8804
    ldx #11
.Lsave:
    lda __rc20,x
    sta $8810,x
    txa
    eor $8800
    sta __rc20,x
    dex
    bpl .Lsave
    jsr fast_prepare_triangle
    lda #0
    sta $8805
    tsx
    cpx $8802
    bne .Lrestore
    php
    pla
    and #$0c
    cmp $8801
    bne .Lrestore
    lda __rc0
    cmp $8803
    bne .Lrestore
    lda __rc1
    cmp $8804
    bne .Lrestore
    ldx #11
.Lcheck:
    txa
    eor $8800
    cmp __rc20,x
    bne .Lrestore
    dex
    bpl .Lcheck
    lda #1
    sta $8805
.Lrestore:
    ldx #11
.Lrestore_loop:
    lda $8810,x
    sta __rc20,x
    dex
    bpl .Lrestore_loop
    lda $8805
    rts
'''
C = r'''
#include <stdint.h>
#include <stdio.h>
extern uint8_t preparation_abi(const uint8_t*,uint8_t*,uint8_t*,uint8_t);
static const int16_t triangles[4][8]={
  {-8191,8191,0,-8191,8191,100,100,0x000e},
  {0,160,0,0,0,200,100,0x0001},
  {81,80,80,-8191,8191,-8190,100,0x000f},
  {20,80,130,-100,-50,-1,100,0x0002}};
int main(void) {
  /* the quarter-square table of the resident multiply, as BOOT builds it */
  { uint16_t square=0; for(uint16_t n=0;n<512;n++){((uint8_t*)0xc000)[n]=(uint8_t)square;((uint8_t*)0xc200)[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));} }
  for(uint16_t seed=0;seed<256;seed++) for(uint8_t f=0;f<4;f++) {
    int16_t input[10];uint8_t head[18],tail[14];
    input[0]=(int16_t)0xc3c3;input[9]=(int16_t)0x3c3c;
    for(uint8_t n=0;n<8;n++)input[n+1]=triangles[f][n];
    head[0]=0xa5;head[17]=0x5a;tail[0]=0x69;tail[13]=0x96;
    if(!preparation_abi((const uint8_t*)(input+1),head+1,tail+1,(uint8_t)seed) ||
       input[0]!=(int16_t)0xc3c3 || input[9]!=(int16_t)0x3c3c ||
       head[0]!=0xa5 || head[17]!=0x5a || tail[0]!=0x69 || tail[13]!=0x96) {
      printf("FAIL seed=%u fixture=%u\n",seed,f);return 1;
    }
    for(uint8_t n=0;n<8;n++)if(input[n+1]!=triangles[f][n])return 2;
  }
  puts("PASS 1024 preparation calls preserve RC0/1, RC20..31, S, I/D, and buffer guards");
  return 0;
}
'''


def main():
    BUILD.mkdir(parents=True, exist_ok=True)
    (BUILD / "check.c").write_text(C)
    (BUILD / "wrapper.S").write_text(ASM)
    executable = BUILD / "check"
    subprocess.run([SDK / "mos-sim-clang", "-Os", "-mlto-zp=0", BUILD / "check.c",
                    BUILD / "wrapper.S", ROOT / "target-c64/asm/preparation.S",
                    ROOT / "target-c64/asm/division.S", ROOT / "target-c64/asm/multiply.S", "-o", executable], check=True)
    result = subprocess.run([SDK / "mos-sim", "--cycles", executable], text=True, capture_output=True, timeout=60)
    if result.returncode or "PASS 1024" not in result.stdout:
        raise RuntimeError(result.stdout + result.stderr)
    (BUILD / "report.json").write_text(json.dumps({"passed": True, "calls": 1024,
        "kernel_sha256": hashlib.sha256((ROOT / "target-c64/asm/preparation.S").read_bytes()).hexdigest(),
        "output": result.stdout.strip(), "simulator": result.stderr.strip()}, indent=2) + "\n")
    print(result.stdout.strip())


if __name__ == "__main__":
    main()
