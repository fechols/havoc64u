#!/usr/bin/env python3
"""Differential check of the native flight step.

Usage: python3 tools/test-flight-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness compares
fast_flight_step (the native body of Game::move_player: the stick's target
velocity, the 3/32 lag with its exact-settling magnitude rule, the soft
arena wall, the Q8 position split over the player coordinate and its
fraction, the coordinated bank from the lateral velocity change and the
pitch from the vertical target) with a C transcription of the Rust
reference (src/game/flight.rs step_reference) in release-mode arithmetic:
wrapping 16-bit differences, arithmetic shifts, truncating halves.
Trajectories hold every stick pattern with every push from rest and
release them; random scenes start from arbitrary states, realistic and
wrapping (positions outside the arena, stale fractions, extreme velocities
and angles), as single steps and as continuing flights. Every byte of the
player coordinates and the flight state must match after every step. The
JSON report stays under target/flight-test.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/flight-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/flight.S"]
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
struct Flight { uint8_t frac[2]; int16_t vel[2]; int16_t bank; int16_t pitch; };
extern void fast_flight_step(uint8_t bits, int8_t push, struct Vec3 *player, struct Flight *flight);
static int16_t w16(int32_t v){ return (int16_t)(uint16_t)(uint32_t)v; }
static int16_t delta(int16_t e, int shift){
  uint16_t m = (uint16_t)(e < 0 ? (uint16_t)(-(int32_t)e) : (uint16_t)e) >> shift;
  if(m == 0 && e != 0) m = 1;
  return e < 0 ? w16(-(int32_t)m) : (int16_t)m;
}
static int16_t wall(int16_t v, int16_t p, int16_t edge){
  if(p >= 0){ int16_t b = (int16_t)(w16((int32_t)edge - p + 7) >> 3); return v < b ? v : b; }
  int16_t b = w16(-(int32_t)(w16((int32_t)edge + p + 7) >> 3)); return v > b ? v : b;
}
static int16_t axis(int16_t *pos, uint8_t *frac, int16_t *vel, int16_t target, int16_t edge){
  int16_t before = *vel;
  int16_t d = delta(w16((int32_t)target - before), 4);
  int16_t lagged = w16((int32_t)before + d + d / 2);
  int16_t q = (int16_t)(uint16_t)(((uint16_t)*pos << 8) | *frac);
  int16_t v = wall(lagged, q, edge);
  q = w16((int32_t)q + v);
  *pos = (int16_t)(q >> 8);
  *frac = (uint8_t)q;
  *vel = v;
  return w16((int32_t)v - before);
}
static int16_t clamp20(int16_t v){ return v < -20 ? -20 : (v > 20 ? 20 : v); }
static void reference(struct Vec3 *p, struct Flight *f, uint8_t bits, int8_t push){
  int16_t dx = (int16_t)(((bits & 8) != 0) - ((bits & 4) != 0));
  int16_t dy = (int16_t)(((bits & 1) != 0) - ((bits & 2) != 0));
  int16_t tx = w16(dx * 768 + push * 640), ty = w16(dy * 704);
  int16_t dvx = axis(&p->x, &f->frac[0], &f->vel[0], tx, 25600);
  axis(&p->y, &f->frac[1], &f->vel[1], ty, 16640);
  int16_t bt = (dvx >= -1 && dvx <= 1) ? 0 : w16((int32_t)clamp20(w16(-(int32_t)dvx)) << 8);
  f->bank = w16((int32_t)f->bank + delta(w16((int32_t)bt - f->bank), 3));
  int16_t pt = w16(-(int32_t)w16((int32_t)ty << 2));
  f->pitch = w16((int32_t)f->pitch + delta(w16((int32_t)pt - f->pitch), 3));
}
static uint32_t state=0x2545F491u;
static uint32_t next(void){ state=state*1664525u+1013904223u; return state; }
static int16_t pick(uint32_t bits, int16_t span){ return (int16_t)((int32_t)(bits%(2*(uint32_t)span+1))-span); }
static uint32_t checks;
static struct Vec3 rp, kp; static struct Flight rf, kf;
static int check(uint8_t bits, int8_t push){
  checks++;
  reference(&rp, &rf, bits, push);
  fast_flight_step(bits, push, &kp, &kf);
  if(memcmp(&rp,&kp,sizeof rp) || memcmp(&rf,&kf,sizeof rf)){
    printf("FAIL check %lu: bits %u push %d\n kernel    (%d,%d,%d) frac %u,%u vel %d,%d bank %d pitch %d\n reference (%d,%d,%d) frac %u,%u vel %d,%d bank %d pitch %d\n",
      (unsigned long)checks, bits, push,
      kp.x, kp.y, kp.z, kf.frac[0], kf.frac[1], kf.vel[0], kf.vel[1], kf.bank, kf.pitch,
      rp.x, rp.y, rp.z, rf.frac[0], rf.frac[1], rf.vel[0], rf.vel[1], rf.bank, rf.pitch);
    return 1;
  }
  return 0;
}
static void start(struct Vec3 p, struct Flight f){ rp=p; kp=p; rf=f; kf=f; }
static struct Vec3 rest(void){ struct Vec3 p={0,0,180}; return p; }
static struct Flight still(void){ struct Flight f; memset(&f,0,sizeof f); return f; }
static struct Vec3 random_position(int wide){
  struct Vec3 p;
  if(wide){ p.x=(int16_t)(next()>>16); p.y=(int16_t)(next()>>16); p.z=(int16_t)(next()>>16); }
  else { p.x=pick(next()>>4,110); p.y=pick(next()>>4,75); p.z=180; }
  return p;
}
static struct Flight random_flight(int wide){
  struct Flight f;
  f.frac[0]=(uint8_t)(next()>>8); f.frac[1]=(uint8_t)(next()>>8);
  if(wide){ for(int i=0;i<2;i++) f.vel[i]=(int16_t)(next()>>16); f.bank=(int16_t)(next()>>16); f.pitch=(int16_t)(next()>>16); }
  else { for(int i=0;i<2;i++) f.vel[i]=pick(next()>>4,1500); f.bank=pick(next()>>4,5200); f.pitch=pick(next()>>4,2900); }
  return f;
}
int main(void){
  /* trajectories: every stick pattern (FIRE set on half of them) with every push, held then released, from rest and from the far corners */
  static const int16_t corners[][2]={{0,0},{100,65},{-100,-65},{99,-64},{-1,1}};
  for(uint8_t corner=0;corner<5;corner++) for(uint8_t bits=0;bits<32;bits++) for(int8_t push=-1;push<=1;push++){
    struct Vec3 p=rest(); p.x=corners[corner][0]; p.y=corners[corner][1];
    start(p, still());
    for(uint8_t t=0;t<70;t++) if(check(bits,push)) return 1;
    for(uint8_t t=0;t<70;t++) if(check(0,0)) return 1;
    for(uint8_t t=0;t<40;t++) if(check((uint8_t)(bits ^ 15),(int8_t)-push)) return 1;
  }
  /* random single steps from arbitrary states, realistic and wrapping */
  for(uint16_t t=0;t<12000;t++){
    int wide=(next()>>8)%4==0;
    start(random_position(wide), random_flight(wide));
    if(check((uint8_t)(next()>>8),(int8_t)((next()>>8)%3-1))) return 1;
  }
  /* random continuing flights: held gestures of random length */
  for(uint8_t flight=0;flight<40;flight++){
    int wide=flight%5==0;
    start(random_position(wide), random_flight(wide));
    for(uint8_t gesture=0;gesture<60;gesture++){
      uint8_t bits=(uint8_t)(next()>>8); int8_t push=(int8_t)((next()>>8)%3-1); uint8_t length=(uint8_t)(1+(next()>>8)%40);
      for(uint8_t t=0;t<length;t++) if(check(bits,push)) return 1;
    }
  }
  printf("PASS %lu steps\n",(unsigned long)checks);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies,
                "-Wl,-Map=" + str(build/"check.map"), "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True, timeout=900)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native flight step check did not finish")
# cycles of the kernel alone: a thousand steps of a held diagonal with the roll's push, then idle coasting
timing = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
struct Flight { uint8_t frac[2]; int16_t vel[2]; int16_t bank; int16_t pitch; };
extern void fast_flight_step(uint8_t bits, int8_t push, struct Vec3 *player, struct Flight *flight);
int main(void){
  struct Vec3 p={0,0,180}; struct Flight f; memset(&f,0,sizeof f);
  for(uint16_t t=0;t<STEPS;t++) fast_flight_step(BITS, PUSH, &p, &f);
  printf("%d %d %d %d\n", p.x, p.y, f.bank, f.pitch);
  return 0;
}
'''
cycles = {}
def cycle_count(run):
    text = (run.stderr.strip() or run.stdout.strip()).split()
    return int(text[0]) if run.stderr.strip() else int(text[-2])
for name, bits, push, steps in [("held diagonal with push", 9, 1, 1000), ("idle at rest", 0, 0, 1000), ("baseline (no steps)", 0, 0, 0)]:
    src = timing.replace("STEPS", str(steps)).replace("BITS", str(bits)).replace("PUSH", str(push))
    (build/"timing.c").write_text(src, encoding="utf-8")
    subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"timing.c", *assemblies, "-o", build/"timing"], check=True)
    run = subprocess.run([sdk/"mos-sim", "--cycles", build/"timing"], text=True, capture_output=True, timeout=300)
    cycles[name] = cycle_count(run)
per_step = {name: (cycles[name] - cycles["baseline (no steps)"]) / 1000 for name in cycles if name != "baseline (no steps)"}
print("cycles per step (call included):", per_step)
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "src/game/flight.rs step_reference transcribed to C with release-mode wrapping arithmetic",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_per_step": per_step,
          "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
