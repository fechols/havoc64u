#!/usr/bin/env python3
"""Differential check of the native shot collision scan.

Usage: python3 tools/test-shots-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness compares
fast_shot_scan (the native body of Game::shot_target: the per-actor radius
and centre selection, swept_z over the shot's move and close_xy, in slot
order) with a C transcription of the Rust reference in release-mode
arithmetic: wrapping differences and bounds, i16::abs wrapping at -32768,
signed comparisons. Authored scenes put a slot exactly on and one unit past
every edge of the hit volume on both sides, in each kind class (small debris,
ROCK, BUILDING, descriptor kinds, kinds past the bank), with the sweep in
both directions, and check that the earliest of several hit slots wins.
Random scenes mix empty and live slots, kinds 0..7, random descriptors
(radii 0..255, signed centres), both bank sizes, every shot speed and
coordinates whose differences wrap. The returned slot must match on every
scene. The JSON report stays under target/shots-test.
"""
import hashlib
import json
import os
import pathlib
import re
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/shots-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/shots.S"]
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
struct Actor { struct Vec3 pos; int16_t origin_x, origin_y; uint16_t age; uint8_t kind, hp, flags, approach_speed; };
struct Args { const struct Actor *actors; const uint8_t *specs; int16_t old_z; struct Vec3 pos; uint8_t enemy_count; };
extern uint8_t fast_shot_scan(int16_t old_z, const struct Vec3 *pos, const struct Actor *actors, const uint8_t *specs, uint8_t enemy_count);
static struct Actor actors[12];
static uint8_t specs[3 * 8];
static int16_t w16(int32_t v){ return (int16_t)(uint16_t)(uint32_t)v; }
static int16_t abs16(int16_t v){ return v<0?w16(-(int32_t)v):v; }
static int swept(int16_t from, int16_t to, int16_t target, int16_t radius){
  int16_t lo=from<to?from:to, hi=from<to?to:from;
  return lo<=w16((int32_t)target+radius) && hi>=w16((int32_t)target-radius);
}
static int close_xy(struct Vec3 a, struct Vec3 b, int16_t radius){
  return abs16(w16((int32_t)a.x-b.x))<=radius && abs16(w16((int32_t)a.y-b.y))<=radius;
}
static uint8_t reference(const struct Args *a){
  for(uint8_t i=0;i<12;i++){
    const struct Actor *t=&a->actors[i];
    if(t->kind==0) continue;
    uint8_t spec_radius=0; int8_t spec_center=0;
    if(t->kind<=a->enemy_count){ const uint8_t *s=a->specs+(t->kind-1)*8; spec_center=(int8_t)s[1]; spec_radius=s[2]; }
    int small=(t->flags&2)!=0;
    int16_t radius=small?18:((t->kind==6||t->kind==7)?35:spec_radius);
    int16_t center=t->kind==7?(small?10:20):(t->kind==6?30:spec_center);
    struct Vec3 c={t->pos.x, w16((int32_t)t->pos.y+center), t->pos.z};
    if(swept(a->old_z,a->pos.z,t->pos.z,radius) && close_xy(a->pos,c,radius)) return i;
  }
  return 255;
}
static uint32_t state=0x2545F491u;
static uint32_t next(void){ state=state*1664525u+1013904223u; return state; }
static int16_t pick(uint32_t bits, int16_t span){ return (int16_t)((int32_t)(bits%(2*(uint32_t)span+1))-span); }
static uint32_t checks, hits;
static int check(const struct Args *a){
  checks++;
  uint8_t theirs=reference(a), mine=fast_shot_scan(a->old_z,&a->pos,a->actors,a->specs,a->enemy_count);
  if(theirs!=255) hits++;
  if(mine!=theirs){
    printf("FAIL check %lu: shot (%d,%d,%d) from z %d -> kernel %u, reference %u; bank count %u\n",
      (unsigned long)checks,a->pos.x,a->pos.y,a->pos.z,a->old_z,mine,theirs,a->enemy_count);
    for(uint8_t i=0;i<12;i++) printf(" actor %u: kind %u flags %02x at (%d,%d,%d)\n",i,actors[i].kind,actors[i].flags,actors[i].pos.x,actors[i].pos.y,actors[i].pos.z);
    printf(" specs:"); for(uint8_t i=0;i<24;i++) printf(" %u",specs[i]); printf("\n");
    return 1;
  }
  return 0;
}
/* the reference's radius and centre of one slot, for authored edge placement */
static void shape(const struct Actor *t, uint8_t count, int16_t *radius, int16_t *center){
  uint8_t spec_radius=0; int8_t spec_center=0;
  if(t->kind!=0 && t->kind<=count){ const uint8_t *s=specs+(t->kind-1)*8; spec_center=(int8_t)s[1]; spec_radius=s[2]; }
  int small=(t->flags&2)!=0;
  *radius=small?18:((t->kind==6||t->kind==7)?35:spec_radius);
  *center=t->kind==7?(small?10:20):(t->kind==6?30:spec_center);
}
int main(void){
  struct Args a; memset(&a,0,sizeof a); a.actors=actors; a.specs=specs;
  memset(specs,0,sizeof specs);
  specs[0]=3; specs[1]=0; specs[2]=20; specs[8]=3; specs[9]=(uint8_t)-6; specs[10]=24; specs[16]=6; specs[17]=(uint8_t)-19; specs[18]=30;
  /* authored: every edge of every kind class's hit volume, both sweep directions, both bank sizes */
  static const uint8_t kinds[]={1,2,3,4,5,6,7,7,1,3};
  static const uint8_t flags[]={0,0,0,0,0,0,0,2,2,1};
  static const int16_t speeds[]={48,32,-18,0,1,-1,300};
  for(uint8_t count=2;count<=3;count++){
    a.enemy_count=count;
    for(uint8_t k=0;k<sizeof kinds/sizeof kinds[0];k++) for(uint8_t sp=0;sp<sizeof speeds/sizeof speeds[0];sp++) for(uint8_t slot=0;slot<12;slot+=5){
      memset(actors,0,sizeof actors);
      struct Actor t; memset(&t,0,sizeof t); t.kind=kinds[k]; t.flags=flags[k]; t.hp=3;
      t.pos.x=(int16_t)(slot*37-120); t.pos.y=(int16_t)(60-slot*20); t.pos.z=(int16_t)(500+slot*90);
      actors[slot]=t;
      int16_t radius, center; shape(&t,count,&radius,&center);
      int16_t cy=w16((int32_t)t.pos.y+center);
      for(int8_t ex=-1;ex<=1;ex++) for(int8_t ey=-1;ey<=1;ey++) for(int8_t ez=-1;ez<=1;ez++) for(uint8_t past=0;past<2;past++){
        /* the shot ends on an edge (past 0) or one unit outside it (past 1); the sweep covers the move */
        int16_t px=w16((int32_t)t.pos.x+ex*(radius+past)), py=w16((int32_t)cy+ey*(radius+past));
        int16_t pz=w16((int32_t)t.pos.z+ez*(radius+past));
        a.pos.x=px; a.pos.y=py; a.pos.z=pz; a.old_z=w16((int32_t)pz-speeds[sp]);
        if(check(&a)) return 1;
        /* the sweep starts on the edge instead */
        a.old_z=pz; a.pos.z=w16((int32_t)pz+speeds[sp]);
        if(check(&a)) return 1;
      }
    }
  }
  /* slot order: several hits, the earliest wins; dead-looking slots still count */
  memset(actors,0,sizeof actors); a.enemy_count=3;
  for(uint8_t i=0;i<12;i++){ actors[i].kind=(uint8_t)(1+i%3); actors[i].hp=(uint8_t)(i&1); actors[i].pos.z=700; }
  a.pos.x=0; a.pos.y=0; a.pos.z=700; a.old_z=652;
  for(uint8_t first=0;first<12;first++){
    for(uint8_t i=0;i<12;i++) actors[i].pos.x=(int16_t)(i<first?400:0);
    if(check(&a)) return 1;
  }
  /* wrapping corners: differences of -32768, bounds that wrap around the i16 range */
  memset(actors,0,sizeof actors);
  static const int16_t corners[]={-32768,-32767,-32700,-1,0,1,32700,32767};
  for(uint8_t cx=0;cx<8;cx++) for(uint8_t cy=0;cy<8;cy++) for(uint8_t cz=0;cz<8;cz++){
    actors[0].kind=(uint8_t)(1+cz%7); actors[0].flags=(uint8_t)((cx&1)?2:0);
    actors[0].pos.x=corners[cx]; actors[0].pos.y=corners[cy]; actors[0].pos.z=corners[cz];
    for(uint8_t s=0;s<8;s++){
      a.pos.x=corners[(cx+s)%8]; a.pos.y=corners[(cy+s*3)%8]; a.pos.z=corners[(cz+s*5)%8]; a.old_z=w16((int32_t)a.pos.z-speeds[s%7]);
      if(check(&a)) return 1;
      a.pos.x=w16((int32_t)actors[0].pos.x+32768); a.pos.y=w16((int32_t)actors[0].pos.y+32768);
      if(check(&a)) return 1;
    }
  }
  /* random scenes */
  for(uint16_t t=0;t<3000;t++){
    uint8_t scale=(uint8_t)((next()>>8)%3);
    for(uint8_t i=0;i<12;i++){
      struct Actor *x=&actors[i]; memset(x,0,sizeof *x);
      if((next()>>8)%10<3) continue;
      x->kind=(uint8_t)((next()>>8)%8); x->hp=(uint8_t)(next()>>8);
      x->flags=(uint8_t)(((next()>>8)%4==0)?2:0)|(uint8_t)((next()>>8)&0xfd);
      if(scale==0){ x->pos.x=pick(next()>>4,1000); x->pos.y=pick(next()>>4,600); x->pos.z=(int16_t)(-100+(next()>>8)%2600); }
      else if(scale==1){ x->pos.x=pick(next()>>4,60); x->pos.y=pick(next()>>4,60); x->pos.z=(int16_t)(600+pick(next()>>4,80)); }
      else { x->pos.x=(int16_t)(next()>>16); x->pos.y=(int16_t)(next()>>16); x->pos.z=(int16_t)(next()>>16); }
    }
    for(uint8_t k=0;k<3;k++){ specs[k*8+1]=(uint8_t)(next()>>8); specs[k*8+2]=(uint8_t)(((next()>>8)%8==0)?((next()>>8)&1?0:255):1+(next()>>8)%64); }
    a.enemy_count=(uint8_t)(2+(next()>>8)%2);
    if(scale==2){ a.pos.x=(int16_t)(next()>>16); a.pos.y=(int16_t)(next()>>16); a.pos.z=(int16_t)(next()>>16); a.old_z=(int16_t)(next()>>16); }
    else { a.pos.x=pick(next()>>4,scale?70:1000); a.pos.y=pick(next()>>4,scale?70:600); a.pos.z=(int16_t)(scale?600+pick(next()>>4,100):8+(next()>>8)%2400); a.old_z=w16((int32_t)a.pos.z-speeds[(next()>>8)%7]); }
    if(check(&a)) return 1;
  }
  printf("PASS %lu scenes, %lu with a hit\n",(unsigned long)checks,(unsigned long)hits);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies,
                "-Wl,-Map=" + str(build/"check.map"), "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True, timeout=600)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native shot scan check did not finish")
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "Game::shot_target (swept_z, close_xy, actor radii and centres) transcribed to C with release-mode wrapping arithmetic",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
