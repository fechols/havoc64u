#!/usr/bin/env python3
"""Differential check of the native missile guidance kernel.

Usage: python3 tools/test-missile-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness builds the
quarter-square table and compares fast_steer_missile (the native body of
Game::steer_missile: the missile_target search over the twelve actor slots and
the boss's turrets and core, then missile_velocity per axis) with a C
transcription of the Rust reference in release-mode arithmetic: wrapping i16
differences and products, truncating division, retained target IDs and the
same rejection-sampled LFSR ranks. Random scenes mix live,
dead and empty slots, kinds outside the bank's descriptors, small debris,
every closing speed, boss states on both sides of the 680 depth and shots at
every position, including coordinates whose differences wrap. The steered
dx/dy, selected target and RNG state must match on every scene. The JSON report
stays under target/missile-test.
"""
import hashlib
import json
import os
import pathlib
import re
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/missile-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/multiply.S", root / "target-c64/asm/missile.S"]
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
struct Actor { struct Vec3 pos; int16_t origin_x, origin_y; uint16_t age; uint8_t kind, hp, flags, approach_speed; };
struct Args { const struct Actor *actors; const uint8_t *specs; struct Vec3 shot; int16_t boss_z;
              uint8_t enemy_count, boss_active, boss_left, boss_right, boss_core; int8_t dx, dy;
              uint8_t target, rng; };
_Static_assert(sizeof(struct Args)==21, "native steering ABI");
extern void fast_steer_missile(struct Args *args);
#define QLO ((uint8_t*)0xc000)
#define QHI ((uint8_t*)0xc200)
/* the kernel's selection, read back from its scratch for a sharper comparison */
#define MS_BEST ((volatile int16_t*)0x915b)
#define MS_BESTC (*(volatile int16_t*)0x9161)
#define MS_FOUND (*(volatile uint8_t*)0x9167)
static struct Actor actors[12];
static uint8_t specs[3 * 8];
static int16_t w16(int32_t v){ return (int16_t)(uint16_t)(uint32_t)v; }
static int16_t clamp16(int32_t v, int32_t lo, int32_t hi){ return (int16_t)(v<lo?lo:(v>hi?hi:v)); }
static int8_t velocity(int16_t error, int16_t closing, int16_t dz){
  int16_t e=clamp16(error,-512,512);
  int16_t p=w16((int32_t)e*(int32_t)closing);
  int16_t q=(int16_t)((int32_t)p/(int32_t)dz);
  return (int8_t)clamp16(q,-12,12);
}
static int candidate(const struct Args *a, uint8_t id, struct Vec3 *t, int16_t *closing){
    if(id==0 || id>15) return 0;
    if(id<=12){
      const struct Actor *actor=&a->actors[id-1];
      uint8_t spec_hp=0; int8_t center=0;
      if(actor->kind!=0 && actor->kind<=a->enemy_count){ const uint8_t *s=a->specs+(actor->kind-1)*8; spec_hp=s[0]; center=(int8_t)s[1]; }
      int small=(actor->flags&2)!=0;
      if(actor->kind==0 || actor->hp==0 || (spec_hp==0 && !small)) return 0;
      *t=actor->pos; t->y=w16((int32_t)t->y+(small?10:center));
      *closing=(int16_t)(32+actor->approach_speed);
    } else {
      if(!a->boss_active) return 0;
      int16_t x, y, z;
      if(id==13){ if(a->boss_left==0) return 0; x=-130; y=30; z=-70; }
      else if(id==14){ if(a->boss_right==0) return 0; x=130; y=30; z=-70; }
      else { if(!(a->boss_left==0 && a->boss_right==0 && a->boss_core>0)) return 0; x=0; y=0; z=-200; }
      t->x=x; t->y=y; t->z=w16((int32_t)a->boss_z+z);
      *closing=a->boss_z>680?40:32;
    }
    return w16((int32_t)t->z-a->shot.z)>0;
}
static int target(struct Args *a, struct Vec3 *best, int16_t *best_closing){
  if(candidate(a,a->target,best,best_closing)) return 1;
  uint8_t candidates[15], count=0, rank=0;
  for(uint8_t id=1;id<=15;id++) if(candidate(a,id,best,best_closing)) candidates[count++]=id;
  a->target=0;
  if(count==0) return 0;
  if(count>1){
    uint8_t sample;
    if(a->rng==0) a->rng=0xa5;
    do { a->rng=(uint8_t)((a->rng>>1)^((a->rng&1)?0xb8:0)); sample=a->rng>>4; }
    while(sample==0 || sample>count);
    rank=sample-1;
  }
  a->target=candidates[rank];
  return candidate(a,a->target,best,best_closing);
}
static void reference(struct Args *a, int *found, struct Vec3 *best, int16_t *closing){
  int8_t dx=0, dy=0;
  *found=target(a,best,closing);
  if(*found){
    int16_t dz=w16((int32_t)best->z-a->shot.z); if(dz<*closing) dz=*closing;
    dx=velocity(w16((int32_t)best->x-a->shot.x),*closing,dz);
    dy=velocity(w16((int32_t)best->y-a->shot.y),*closing,dz);
  }
  int8_t sx=(int8_t)(dx-a->dx), sy=(int8_t)(dy-a->dy);
  sx=(int8_t)clamp16(sx,-1,1); sy=(int8_t)clamp16(sy,-1,1);
  a->dx=(int8_t)(a->dx+sx); a->dy=(int8_t)(a->dy+sy);
}
static uint32_t state=0x9E3779B9u;
static uint32_t next(void){ state=state*1664525u+1013904223u; return state; }
static int16_t pick(uint32_t bits, int16_t span){ return (int16_t)((int32_t)(bits%(2*(uint32_t)span+1))-span); }
static uint32_t checks, targeted, boss_targets;
static int check(struct Args *a){
  struct Args mine=*a, theirs=*a;
  int found; struct Vec3 best={0,0,0}; int16_t closing=0;
  checks++;
  reference(&theirs,&found,&best,&closing);
  fast_steer_missile(&mine);
  int kernel_found=MS_FOUND!=0;
  int same=mine.dx==theirs.dx && mine.dy==theirs.dy && mine.target==theirs.target &&
    mine.rng==theirs.rng && kernel_found==found &&
    (!found || (MS_BEST[0]==best.x && MS_BEST[1]==best.y && MS_BEST[2]==best.z && MS_BESTC==closing));
  if(found){ targeted++; if(best.z!=0 && (best.x==130||best.x==-130||(best.x==0&&best.y==0)) && a->boss_active) boss_targets++; }
  if(!same){
    printf("FAIL check %lu: shot (%d,%d,%d) dx %d dy %d -> kernel dx %d dy %d found %d target (%d,%d,%d) closing %d; reference dx %d dy %d found %d target (%d,%d,%d) closing %d\n",
      (unsigned long)checks,a->shot.x,a->shot.y,a->shot.z,a->dx,a->dy,mine.dx,mine.dy,kernel_found,MS_BEST[0],MS_BEST[1],MS_BEST[2],MS_BESTC,
      theirs.dx,theirs.dy,found,best.x,best.y,best.z,closing);
    printf(" lock/rng %u/%02x -> kernel %u/%02x reference %u/%02x\n",a->target,a->rng,mine.target,mine.rng,theirs.target,theirs.rng);
    printf(" kernel scratch: d %d e %d p %u neg %02x v %d %d\n",*(volatile int16_t*)0x9168,*(volatile int16_t*)0x9171,*(volatile uint16_t*)0x9173,*(volatile uint8_t*)0x9177,*(volatile int8_t*)0x9178,*(volatile int8_t*)0x9179);
    printf(" boss active %u z %d left %u right %u core %u; bank count %u\n",a->boss_active,a->boss_z,a->boss_left,a->boss_right,a->boss_core,a->enemy_count);
    for(uint8_t i=0;i<12;i++) printf(" actor %u: kind %u hp %u flags %02x speed %u at (%d,%d,%d)\n",i,actors[i].kind,actors[i].hp,actors[i].flags,actors[i].approach_speed,actors[i].pos.x,actors[i].pos.y,actors[i].pos.z);
    return 1;
  }
  *a=mine;
  return 0;
}
static void random_actor(struct Actor *a, uint8_t scale){
  memset(a,0,sizeof *a);
  uint32_t r=next()>>8;
  if(r%10<3){ a->kind=0; a->hp=0; return; }
  a->kind=(uint8_t)((next()>>8)%8);
  a->hp=(uint8_t)(((next()>>8)%6==0)?0:1+(next()>>8)%255);
  a->flags=(uint8_t)(((next()>>8)%4==0)?2:0)|(uint8_t)((next()>>8)&0xfd);
  a->approach_speed=(uint8_t)(((next()>>8)%8==0)?(next()>>8):(next()>>8)%32);
  if(scale==0){ a->pos.x=pick(next()>>4,1000); a->pos.y=pick(next()>>4,600); a->pos.z=(int16_t)(-100+(next()>>8)%2600); }
  else if(scale==1){ a->pos.x=pick(next()>>4,200); a->pos.y=pick(next()>>4,120); a->pos.z=(int16_t)((next()>>8)%700); }
  else { a->pos.x=(int16_t)(next()>>16); a->pos.y=(int16_t)(next()>>16); a->pos.z=(int16_t)(next()>>16); }
}
int main(void){
  uint16_t square=0;
  for(uint16_t n=0;n<512;n++){QLO[n]=(uint8_t)square;QHI[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));}
  struct Args a; memset(&a,0,sizeof a); a.actors=actors; a.specs=specs; a.rng=0xa5;
  /* Authored eligibility, retained locks, vanished/passed targets and boss parts. */
  memset(actors,0,sizeof actors); memset(specs,0,sizeof specs);
  specs[0]=3; specs[1]=0; specs[8]=3; specs[9]=0; specs[16]=6; specs[17]=(uint8_t)-19;
  a.enemy_count=3; a.shot.z=200;
  struct Actor rock={{0,0,300},0,0,0,7,3,2,8}, fighter={{0,0,190},0,0,0,1,3,0,8}, diver={{0,0,400},0,0,0,2,0,0,12};
  struct Actor far_fighter={{0,0,800},0,0,0,1,3,0,8}, left={{-40,0,700},0,0,0,2,3,0,12}, right={{40,0,700},0,0,0,2,3,0,12};
  actors[0]=rock; actors[1]=fighter; actors[2]=diver; actors[3]=far_fighter; actors[4]=left; actors[5]=right;
  if(check(&a)) return 1;
  actors[5].pos.x=39; if(check(&a)) return 1;
  struct Actor turret={{0,-19,600},0,0,0,3,6,0,8}; actors[5]=turret; if(check(&a)) return 1;
  memset(actors,0,sizeof actors);
  struct Actor chased={{100,0,700},0,0,0,1,3,0,8}; actors[0]=chased;
  for(uint8_t step=0;step<30;step++) if(check(&a)) return 1;
  actors[0].kind=0; if(check(&a)) return 1;
  actors[0]=chased; actors[0].pos.x=-100; if(check(&a)) return 1;
  actors[0].pos.z=199; if(check(&a)) return 1;
  actors[0].kind=0; actors[0].flags=2; actors[0].hp=2; actors[0].pos.z=700;
  a.target=1; if(check(&a) || a.target!=0) return 1; /* empty debris slot cannot retain a lock */
  for(uint16_t z=0;z<3000;z+=97){ a.boss_active=1; a.boss_z=(int16_t)z; a.boss_left=40; a.boss_right=40; a.boss_core=120;
    if(check(&a)) return 1; a.boss_left=0; if(check(&a)) return 1; a.boss_right=0; if(check(&a)) return 1; a.boss_core=0; if(check(&a)) return 1; }
  a.boss_z=680; a.boss_left=1; if(check(&a)) return 1; a.boss_z=681; if(check(&a)) return 1;
  a.boss_active=0;
  /* Every candidate rank and initial RNG state, including zero repair. Counts
     13/14 include the boss's turrets; a live core excludes those two turrets. */
  for(uint8_t count=0;count<=14;count++){
    memset(actors,0,sizeof actors); a.shot.x=0; a.shot.y=0; a.shot.z=200;
    a.boss_active=count>12; a.boss_z=900; a.boss_left=count>12; a.boss_right=count>13; a.boss_core=1;
    for(uint8_t i=0;i<count && i<12;i++) actors[i]=chased;
    for(uint16_t seed=0;seed<256;seed++){
      a.target=0; a.rng=(uint8_t)seed;
      if(check(&a)) return 1;
      if(count<=1 && a.rng!=(uint8_t)seed) return 1;
      uint8_t locked=a.target, saved_rng=a.rng;
      if(check(&a) || a.target!=locked || a.rng!=saved_rng) return 1;
    }
    if(count>1){
      /* Seed the cycle at an accepted draw, then exactly 16*count subsequent
         acquisitions traverse all accepted high nibbles once. */
      a.target=0; a.rng=0xa5; if(check(&a)) return 1;
      uint8_t cycle_rng=a.rng; uint16_t hits[15]={0};
      for(uint16_t draw=0;draw<(uint16_t)count*16;draw++){
        a.target=0; if(check(&a)) return 1; hits[a.target-1]++;
      }
      if(a.rng!=cycle_rng) return 1;
      for(uint8_t i=0;i<count;i++) if(hits[i]!=16) return 1;
    }
  }
  a.boss_active=0; a.target=0;
  /* extreme guidance: error clamps, wrapping products, tiny and huge depths */
  memset(actors,0,sizeof actors);
  static const int16_t errors[]={-32768,-30000,-513,-512,-511,-1,0,1,511,512,513,30000,32767};
  static const int16_t depths[]={1,2,3,7,31,32,33,40,41,100,287,288,500,2399,2400,32767};
  for(uint8_t e=0;e<sizeof errors/sizeof errors[0];e++) for(uint8_t d=0;d<sizeof depths/sizeof depths[0];d++) for(uint8_t s=0;s<4;s++){
    a.shot.x=0; a.shot.y=0; a.shot.z=(int16_t)(s<2?8:-3000); a.dx=(int8_t)(s&1?12:-12); a.dy=(int8_t)(s&1?-12:12);
    struct Actor t={{errors[e],(int16_t)(-errors[e]),w16((int32_t)a.shot.z+depths[d])},0,0,0,1,3,0,(uint8_t)(s==3?255:s*10)};
    actors[0]=t; if(check(&a)) return 1;
  }
  /* random scenes */
  for(uint16_t t=0;t<2500;t++){
    uint8_t scale=(uint8_t)((next()>>8)%3);
    for(uint8_t i=0;i<12;i++) random_actor(&actors[i],scale);
    for(uint8_t k=0;k<3;k++){ specs[k*8]=(uint8_t)(((next()>>8)%12==0)?0:1+(next()>>8)%254); specs[k*8+1]=(uint8_t)(next()>>8); }
    a.enemy_count=(uint8_t)(2+(next()>>8)%2);
    if(scale==2){ a.shot.x=(int16_t)(next()>>16); a.shot.y=(int16_t)(next()>>16); a.shot.z=(int16_t)(next()>>16); }
    else { a.shot.x=pick(next()>>4,1000); a.shot.y=pick(next()>>4,600); a.shot.z=(int16_t)(8+(next()>>8)%2400); }
    a.boss_active=(uint8_t)((next()>>8)%3==0); a.boss_z=(int16_t)(((next()>>8)%4==0)?680+(int16_t)((next()>>8)%3)-1:pick(next()>>4,2500));
    a.boss_left=(uint8_t)(((next()>>8)&1)?0:(next()>>8)); a.boss_right=(uint8_t)(((next()>>8)&1)?0:(next()>>8)); a.boss_core=(uint8_t)(((next()>>8)%3)?(next()>>8):0);
    a.target=(uint8_t)((next()>>8)%20); a.rng=(uint8_t)(next()>>8);
    if((next()>>8)%5==0){ a.dx=(int8_t)(next()>>8); a.dy=(int8_t)(next()>>8); } else { a.dx=(int8_t)pick(next()>>4,12); a.dy=(int8_t)pick(next()>>4,12); }
    if(check(&a)) return 1;
  }
  printf("PASS %lu scenes, %lu with a target, %lu on the boss\n",(unsigned long)checks,(unsigned long)targeted,(unsigned long)boss_targets);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies,
                "-Wl,-Map=" + str(build/"check.map"), "-o", build/"check"], check=True)
layout = (build/"check.map").read_text(encoding="utf-8", errors="replace")
heap = re.search(r"^\s*([0-9a-f]+)\s+[0-9a-f]+\s+[0-9a-f]+\s+\d+\s+__heap_start", layout, re.M)
if heap is None or int(heap.group(1), 16) > 0x913f:
    raise SystemExit(f"harness image reaches {heap.group(1) if heap else '?'}: overlaps the kernel scratch at $9140")
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True, timeout=300)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native missile check did not finish")
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "Game::missile_target, missile_velocity and steer_missile transcribed to C with release-mode wrapping arithmetic",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
