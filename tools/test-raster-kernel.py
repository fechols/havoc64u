#!/usr/bin/env python3
"""Compare the production persistent raster assembly with a C Q16 oracle.

Rust exports production-prepared records validated separately against an i64
rational oracle and the legacy boundary tolerance. The simulator exercises all
25 sequential bands, hidden-band state advancement, page crossings, split-buffer
guards, and immutable tails. It does not measure complete game performance.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/raster-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assembly = root / "target-c64/asm/raster.S"
env = dict(os.environ, HAVOC_RASTER_FIXTURES=str(build/"fixtures.h"))
subprocess.run(["cargo", "test", "--lib", "render::tests::export_native_raster_fixtures",
                "--", "--ignored", "--exact"], cwd=root, env=env, check=True)
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Head { int32_t long_x,long_step,short_x,short_step; };
struct Tail { int32_t lower_x,lower_step; uint8_t top,middle,bottom,flags; };
struct Fixture { struct Head head; struct Tail tail; };
struct Job { struct Head *head; const struct Tail *tail; uint8_t *pixels; uint8_t band,paint; };
_Static_assert(sizeof(struct Head)==16,"head");
_Static_assert(sizeof(struct Tail)==12,"tail");
_Static_assert(sizeof(struct Job)==8,"job");
extern void fast_raster_rows(const struct Job *);
struct Command { int16_t x[3],y[3],depth;uint8_t color,flags; };
_Static_assert(sizeof(struct Command)==16,"command");
extern uint32_t fast_static_sky_bands(const struct Command *,uint16_t);
#include "fixtures.h"
static uint8_t actual[1568],expected[1568];
static uint8_t actual_head[304],expected_head[304],actual_tail[304],expected_tail[304];
static uint8_t offsets[]={0,1,127,255};
static struct Command sky_commands[256];
static uint16_t sky_counts[]={0,1,13,14,15,51,255,256};
static uint32_t reference_sky(uint16_t count){
  uint16_t background=0;uint32_t mask=0x1ffffffUL;
  for(uint16_t i=0;i<count;++i){
    const struct Command *t=sky_commands+i;
    if(t->flags&2){++background;continue;}
    int16_t low=200,high=0;
    for(uint8_t c=0;c<3;++c){int16_t y=t->y[c];if(y<0)y=0;if(y>200)y=200;if(y<low)low=y;if(y>high)high=y;}
    if(low<high)for(uint8_t b=low/8;b<(high+7)/8;++b)mask&=~(1UL<<b);
  }
  return background==14?mask:0;
}
static uint8_t clamp(int32_t x){x=(x+32767L)>>16;return x<0?0:x>160?160:(uint8_t)x;}
__attribute__((noinline))
static void reference(struct Head *h,const struct Tail *t,uint8_t *pixels,uint8_t band,uint8_t paint){
  uint8_t first=t->top>band?t->top:band;
  uint8_t end=t->bottom<band+8?t->bottom:band+8;
  for(uint8_t y=first;y<end;++y){
    if(y==t->middle){h->short_x=t->lower_x;h->short_step=t->lower_step;}
    uint8_t left=clamp((t->flags&16)?h->long_x:h->short_x);
    uint8_t right=clamp((t->flags&16)?h->short_x:h->long_x);
    if(paint)for(uint8_t x=left;x<right;++x)pixels[(uint16_t)(y-band)*160+x]=t->flags&15;
    h->long_x+=h->long_step;h->short_x+=h->short_step;
  }
}
int main(void){
  uint16_t cases=0;
  for(uint16_t f=0;f<sizeof(fixtures)/sizeof(fixtures[0]);++f){
    for(uint8_t alignment=0;alignment<4;++alignment){
      memset(actual_head,0xa5,sizeof(actual_head));memset(expected_head,0xa5,sizeof(expected_head));
      memset(actual_tail,0xa5,sizeof(actual_tail));memset(expected_tail,0xa5,sizeof(expected_tail));
      struct Head *ah=(struct Head*)(actual_head+16+offsets[alignment]);
      struct Head *eh=(struct Head*)(expected_head+16+offsets[alignment]);
      struct Tail *at=(struct Tail*)(actual_tail+16+offsets[alignment]);
      struct Tail *et=(struct Tail*)(expected_tail+16+offsets[alignment]);
      *ah=*eh=fixtures[f].head;*at=*et=fixtures[f].tail;
      for(uint8_t band=0;band<25;++band){
        memset(actual,0xa5,sizeof(actual));memset(expected,0xa5,sizeof(expected));
        uint8_t *ap=actual+16+offsets[alignment],*ep=expected+16+offsets[alignment];
        uint8_t paint=(band%3)!=1;
        struct Job job={ah,at,ap,(uint8_t)(band*8),paint};
        *(volatile uint8_t*)0x779f=0x39;*(volatile uint8_t*)0x77ab=0x93;
        fast_raster_rows(&job);
        reference(eh,et,ep,band*8,paint);
        if(memcmp(actual,expected,sizeof(actual)) || memcmp(actual_head,expected_head,sizeof(actual_head)) ||
           memcmp(actual_tail,expected_tail,sizeof(actual_tail)) || *(volatile uint8_t*)0x779f!=0x39 || *(volatile uint8_t*)0x77ab!=0x93){
          printf("FAIL fixture=%u alignment=%u band=%u\n",f,alignment,band);
          for(uint16_t i=0;i<sizeof(actual);++i)if(actual[i]!=expected[i]){printf("pixel%u got%u want%u\n",i,actual[i],expected[i]);break;}
          printf("head %ld/%ld short %ld/%ld\n",ah->long_x,eh->long_x,ah->short_x,eh->short_x);
          return 1;
        }
      }
      ++cases;
    }
  }
  printf("PASS %u native raster cases; 25 bands each, split state and pixel guards intact\n",cases);
  for(uint8_t scenario=0;scenario<32;++scenario){
    for(uint16_t i=0;i<256;++i){
      sky_commands[i].flags=i<(uint16_t)(13+scenario%3)?2:0;
      for(uint8_t c=0;c<3;++c)sky_commands[i].y[c]=scenario<8?(int16_t)(i+scenario+c)%208-4:(int16_t)((i*137u+c*31u+scenario*7u)%16383u)-8191;
    }
    for(uint8_t n=0;n<sizeof(sky_counts)/sizeof(sky_counts[0]);++n){
      uint16_t count=sky_counts[n];uint32_t expected_mask=reference_sky(count),got=fast_static_sky_bands(sky_commands,count);
      if(got!=expected_mask){printf("FAIL sky scenario%u count%u got%lu want%lu\n",scenario,count,got,expected_mask);return 2;}
    }
  }
  puts("PASS 256 native conservative sky-mask cases");
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", assembly,
                "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS 1024 native raster cases" not in result.stdout or "PASS 256 native conservative sky-mask cases" not in result.stdout:
    raise SystemExit(result.returncode or "native raster fixture suite did not finish")
report = {"passed": True, "assembly_sha256": hashlib.sha256(assembly.read_bytes()).hexdigest(),
          "fixtures_sha256": hashlib.sha256((build/"fixtures.h").read_bytes()).hexdigest(),
          "cases": 1024, "band_calls": 25600, "sky_mask_cases": 256, "reference": "independent C Q16 row traversal and conservative sky masks",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator", "cycles_include_test_harness": True,
          "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
