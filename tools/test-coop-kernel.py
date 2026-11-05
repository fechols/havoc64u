#!/usr/bin/env python3
"""Differential check of the native two-player glue (target-c64/asm/coop.S).

Usage: python3 tools/test-coop-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness places a byte
image of the native Game (the offsets game.rs pins with const assertions) at
`__game_state_start` and runs every coop.S entry point against a C
transcription of the Rust host versions in src/game.rs: ship swapping, the
two-ship driver, proximity masks, ship reset, flight and respawn countdown,
the cached camera view and enemy aim, the afterburner, pickup credit, a lost
ship, enemy targeting, life awards, boss refills and hits with roll dodges.
Calls into Rust (controls, sounds, damage, deflection) are recording stubs.
Random states run every entry point with both seats, swapped fronts,
downed and retired ships. The JSON report stays under target/coop-test.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/coop-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assembly = root / "target-c64/asm/coop.S"
source = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
/* Offsets: game.rs (mos const assertions) and coop.S. */
enum { SHIP=49, RESERVE=49, DEVICES=98, SWAPPED=99, VIEW=100, HIT_AMOUNT=104, HIT_AT=106,
       SHIP_BANK=112, AIM=114, AIM_TURN=118, MODE=128, GAME_SIZE=129,
       C_NOW=0, C_RELEASED=2, C_FLAGS=11, C_CHARGE=12, C_ROLL=14, PX=15, PY=17, PZ=19, FLIGHT=21,
       SCORE=31, BOOST=35, SHIELD=37, BOMBS=38, LIVES=40, SHOT_COOL=41, INVULN=42, FLASH=43,
       ROLL_TICKS=44, ROLL_COOL=45, RESPAWN=47 };
uint8_t __game_state_start[GAME_SIZE];
#define G __game_state_start
struct Vec3 { int16_t x, y, z; };
extern void coop_swap(void *g);
extern void coop_each(void *g, void (*action)(void *), uint8_t ships);
extern uint8_t coop_near(const struct Vec3 *pos, uint8_t radius);
extern void coop_update_ships(void *g, const void *bank, uint8_t fly);
extern void coop_reset_ship(void *g);
extern void coop_boost_ship(void *g);
extern void coop_collect_ship(void *g);
extern uint8_t coop_ship_down(void *g);
extern uint16_t coop_target(uint8_t select);
extern void coop_award_lives(void *g, uint8_t cap);
extern void coop_refill_ships(void *g);
extern void coop_hit_ships(void *g, const struct Vec3 *pos, uint8_t ships, uint8_t amount);
extern uint16_t coop_held(const void *g, const uint8_t bits[3]);
static uint8_t R[GAME_SIZE];      /* reference image */
static uint8_t *ref=R;
static uint32_t checks;
static uint32_t state=0x13579bdfu;
static uint8_t rnd(void){ state=state*1664525u+1013904223u; return (uint8_t)(state>>24); }
static int16_t get16(const uint8_t *g,int o){ return (int16_t)(g[o]|g[o+1]<<8); }
static void put16(uint8_t *g,int o,int16_t v){ g[o]=(uint8_t)v; g[o+1]=(uint8_t)((uint16_t)v>>8); }
/* ---- Recording Rust stubs: each logs (id, front ship's first byte). */
static uint8_t log_native[64], log_ref[64], n_native, n_ref;
static void note(uint8_t *g, uint8_t *log, uint8_t *n, uint8_t id, uint8_t arg){
  if(*n<62){ log[(*n)++]=id; log[(*n)++]=(uint8_t)(arg^g[PX]^g[SHIELD]); } }
static uint8_t *native_target;
void game_update_player_controls(void *g, const void *bank){ note(G,log_native,&n_native,1,(uint8_t)(uintptr_t)bank); G[PX]+=3; }
void game_sound_player(void *g, uint8_t effect){ note(G,log_native,&n_native,2,effect); }
void game_damage_player(void *g, uint8_t amount){ note(G,log_native,&n_native,3,amount); G[SHIELD]-=amount; }
void game_explode_hit(void *g){ note(G,log_native,&n_native,4,G[HIT_AT]); }
void game_sample_ptr(void *g, const uint8_t *in){ (void)g; (void)in; }
void game_sample_paused_ptr(void *g, const uint8_t *in){ (void)g; (void)in; }
/* ---- Reference: game.rs host bodies over the byte image. */
static void r_swap(uint8_t *g){ for(int i=0;i<SHIP;i++){ uint8_t t=g[i]; g[i]=g[RESERVE+i]; g[RESERVE+i]=t; } g[SWAPPED]^=1; }
static int present(uint8_t *g){ return g[SWAPPED] || (g[DEVICES]>>4)!=0; }
static int reserve_alive(uint8_t *g){ return present(g) && g[RESERVE+LIVES] && !g[RESERVE+RESPAWN]; }
static int flying(uint8_t *g,int base){ return g[base+LIVES] && !g[base+RESPAWN]; }
static void r_each(uint8_t *g, void (*a)(uint8_t *), uint8_t ships){ if(ships&1) a(g); if(ships&2){ r_swap(g); a(g); r_swap(g); } }
static int close_axis(int16_t a,int16_t b,int16_t r){ int16_t d=(int16_t)(a-b); if(d<0) d=(int16_t)-d; return d<=r; }
static uint8_t r_near(uint8_t *g,struct Vec3 p,uint8_t r){
  uint8_t m=0;
  if(flying(g,0)&&close_axis(p.x,get16(g,PX),r)&&close_axis(p.y,get16(g,PY),r)) m=1;
  if((g[DEVICES]>>4)&&flying(g,RESERVE)&&close_axis(p.x,get16(g,RESERVE+PX),r)&&close_axis(p.y,get16(g,RESERVE+PY),r)) m|=2;
  return m; }
static void r_reset(uint8_t *g){
  put16(g,PX,g[SWAPPED]?48:0); put16(g,PY,0); put16(g,PZ,180); memset(g+FLIGHT,0,10);
  put16(g,BOOST,0); g[C_FLAGS]&=0x7f; g[SHIELD]=100; g[INVULN]=100; g[FLASH]=0; g[ROLL_TICKS]=0;
  g[ROLL_COOL]=0; g[SHOT_COOL]=0; g[RESPAWN]=0; if(!g[BOMBS]) g[BOMBS]=1; g[C_FLAGS]&=~7;
  g[C_RELEASED]=g[C_NOW]; g[C_RELEASED+1]=g[C_NOW+1]; g[C_CHARGE]=0; }
static void r_settle(uint8_t *g){ g[C_FLAGS]&=0x87; g[C_ROLL]=0; }
static void r_fly(uint8_t *g){
  if(g[RESPAWN]){ if(--g[RESPAWN]==0) r_reset(g); }
  else if(g[LIVES]){ note(g,log_ref,&n_ref,1,(uint8_t)(g[SHIP_BANK])); g[PX]+=3; }
  r_settle(g); }
static void r_update(uint8_t *g,uint16_t bank,uint8_t fly){
  put16(g,SHIP_BANK,(int16_t)bank);
  r_each(g, fly?r_fly:r_settle, 1|((g[DEVICES]>>4)?2:0));
  g[AIM_TURN]++;
  int base=0;
  if(reserve_alive(g) && ((g[AIM_TURN]&1) || !flying(g,0))) base=RESERVE;
  put16(g,AIM,get16(g,base+PX)); put16(g,AIM+2,get16(g,base+PY));
  int16_t vx=get16(g,PX), vy=get16(g,PY);
  if((g[DEVICES]>>4) && flying(g,RESERVE)){
    int16_t sx=get16(g,RESERVE+PX), sy=get16(g,RESERVE+PY);
    if(flying(g,0)){ vx=(int16_t)((int16_t)(vx+sx)>>1); vy=(int16_t)((int16_t)(vy+sy)>>1); } else { vx=sx; vy=sy; } }
  put16(g,VIEW,vx); put16(g,VIEW+2,vy); }
static void r_boost(uint8_t *g){
  if(!flying(g,0) || !(g[MODE]==1||g[MODE]==6)){ put16(g,BOOST,0); g[C_FLAGS]&=0x3f; return; }
  uint16_t c=(uint16_t)get16(g,BOOST); if(c) c--;
  if((g[C_FLAGS]&0x40) && c==0){ c=300; put16(g,BOOST,(int16_t)c); note(g,log_ref,&n_ref,2,5); }
  put16(g,BOOST,(int16_t)c); g[C_FLAGS]&=0x3f; if(c>200) g[C_FLAGS]|=0x80; }
static void r_collect(uint8_t *g){
  if(g[HIT_AMOUNT]==1){ int s=g[SHIELD]+60; g[SHIELD]=(uint8_t)(s>100?100:s); } else if(g[BOMBS]<3) g[BOMBS]++;
  uint32_t s=(uint32_t)(uint16_t)get16(g,SCORE)+2u; put16(g,SCORE,(int16_t)(s>65535?65535:s)); }
static uint8_t r_down(uint8_t *g){
  if(!g[LIVES] && g[SWAPPED]) g[DEVICES]&=15;
  if(!reserve_alive(g)){ if(present(g)) g[RESERVE+RESPAWN]=75; return (!g[LIVES] && !(present(g)&&g[RESERVE+LIVES]))?2:0; }
  if(g[LIVES]) g[RESPAWN]=75; return 1; }
static uint8_t r_target(uint8_t *g,uint8_t sel){ return reserve_alive(g)&&((sel&1)||!g[LIVES]||g[RESPAWN]); }
static void r_award(uint8_t *g,uint8_t cap){
  if(g[LIVES] && g[LIVES]<cap) g[LIVES]++;
  if(present(g) && g[RESERVE+LIVES] && g[RESERVE+LIVES]<cap) g[RESERVE+LIVES]++; }
static void r_refill(uint8_t *g){ g[SHIELD]=100; g[BOMBS]=3; if(present(g)&&g[RESERVE+LIVES]){ g[RESERVE+SHIELD]=100; g[RESERVE+BOMBS]=3; } }
static void r_hit(uint8_t *g){
  if((g[HIT_AMOUNT]&0x80) && g[ROLL_TICKS]) note(g,log_ref,&n_ref,4,g[HIT_AT]);
  else { note(g,log_ref,&n_ref,3,g[HIT_AMOUNT]&0x7f); g[SHIELD]-=g[HIT_AMOUNT]&0x7f; } }
/* ---- Native action stubs for coop_each. */
static uint8_t trace_native[8], trace_ref[8], tn, tr;
static void native_action(void *g){ (void)g; if(tn<8) trace_native[tn++]=G[PX]^G[SWAPPED]; G[SHIELD]++; }
static void ref_action(uint8_t *g){ if(tr<8) trace_ref[tr++]=g[PX]^g[SWAPPED]; g[SHIELD]++; }
/* ---- Random states. */
static void ship(uint8_t *g,int base){
  for(int i=0;i<SHIP;i++) g[base+i]=rnd();
  uint8_t k=rnd()%8;
  g[base+LIVES]=k==0?0:(k==1?99:1+rnd()%3);
  g[base+RESPAWN]=k==2?(uint8_t)(1+rnd()%3):(k==3?75:0);
  g[base+SHIELD]=rnd()%101; g[base+BOMBS]=rnd()%4;
  if(rnd()&1){ put16(g,base+BOOST,(int16_t)(rnd()%3)); }
  if(rnd()&1){ put16(g,base+SCORE,(int16_t)(0xfffe + rnd()%2)); }
  put16(g,base+PX,(int16_t)((int16_t)(rnd()<<1)-256)); put16(g,base+PY,(int16_t)((int16_t)rnd()-128));
}
static void scene(void){
  for(int i=0;i<GAME_SIZE;i++) G[i]=rnd();
  ship(G,0); ship(G,RESERVE);
  uint8_t seat=rnd()%4; G[DEVICES]=(uint8_t)(1+rnd()%3) | (seat?((1+rnd()%3)<<4):0);
  G[SWAPPED]=(rnd()%4)==0; G[MODE]=(uint8_t)(rnd()%7);
  memcpy(R,G,GAME_SIZE); n_native=n_ref=0; tn=tr=0; }
static int same(const char *what,int k){
  checks++;
  if(memcmp(G,R,GAME_SIZE)||n_native!=n_ref||memcmp(log_native,log_ref,n_native)){
    printf("FAIL %s scene %d\n",what,k);
    for(int i=0;i<GAME_SIZE;i++) if(G[i]!=R[i]) printf(" byte %d native %02x ref %02x\n",i,G[i],R[i]);
    printf(" calls native %u ref %u\n",n_native,n_ref); return 1; }
  return 0; }
int main(void){
  for(int k=0;k<4000;k++){
    scene(); coop_swap(G); r_swap(R); if(same("swap",k)) return 1;
    scene(); { uint8_t m=rnd()%4; coop_each(G,native_action,m); r_each(R,ref_action,m);
      if(tn!=tr||memcmp(trace_native,trace_ref,tn)) { printf("FAIL each order %d\n",k); return 1; } if(same("each",k)) return 1; }
    scene(); { struct Vec3 p={(int16_t)(get16(G,PX)+(int16_t)(rnd()%80)-40),(int16_t)(get16(G,RESERVE+PY)+(int16_t)(rnd()%80)-40),0};
      if(rnd()&1) p.x=(int16_t)(get16(G,RESERVE+PX)+(int16_t)(rnd()%60)-30);
      uint8_t r=(uint8_t)(rnd()%70); G[SWAPPED]=0; R[SWAPPED]=0;
      uint8_t a=coop_near(&p,r), b=r_near(R,p,r); if(a!=b){ printf("FAIL near %d: %u %u\n",k,a,b); return 1; } if(same("near",k)) return 1; }
    scene(); coop_reset_ship(G); r_reset(R); if(same("reset",k)) return 1;
    scene(); { G[SWAPPED]=R[SWAPPED]=0; uint8_t fly=rnd()&1; uint16_t bank=(uint16_t)(0x4000+rnd());
      coop_update_ships(G,(const void*)(uintptr_t)bank,fly); r_update(R,bank,fly); if(same("update",k)) return 1; }
    scene(); coop_boost_ship(G); r_boost(R); if(same("boost",k)) return 1;
    scene(); { G[HIT_AMOUNT]=R[HIT_AMOUNT]=(uint8_t)(1+rnd()%2); coop_collect_ship(G); r_collect(R); if(same("collect",k)) return 1; }
    scene(); { uint8_t a=coop_ship_down(G), b=r_down(R); if(a!=b){ printf("FAIL down result %d\n",k); return 1; } if(same("down",k)) return 1; }
    scene(); { uint8_t s=rnd(); uint8_t a=coop_target(s)==(uint16_t)(uintptr_t)(G+RESERVE+PX), b=r_target(R,s); if(a!=b){ printf("FAIL target %d\n",k); return 1; } if(same("target",k)) return 1; }
    scene(); { uint8_t cap=(rnd()&1)?99:3; coop_award_lives(G,cap); r_award(R,cap); if(same("award",k)) return 1; }
    scene(); coop_refill_ships(G); r_refill(R); if(same("refill",k)) return 1;
    scene(); { G[SWAPPED]=R[SWAPPED]=0; struct Vec3 p={(int16_t)(rnd()*3),(int16_t)rnd(),(int16_t)(rnd()*5)};
      uint8_t m=rnd()%4, amount=(uint8_t)((rnd()%30)|((rnd()&1)<<7));
      coop_hit_ships(G,&p,m,amount);
      R[HIT_AMOUNT]=amount; memcpy(R+HIT_AT,&p,6); r_each(R,r_hit,m); if(same("hit",k)) return 1; }
    scene(); { uint8_t bits[3]={rnd(),rnd(),rnd()}; uint8_t first=rnd()%4, second=rnd()%4;
      G[DEVICES]=R[DEVICES]=(uint8_t)(first|(second<<4));
      uint16_t a=coop_held(G,bits);
      uint8_t lo=first?bits[first-1]:(uint8_t)(bits[0]|bits[1]|bits[2]), hi=second?bits[second-1]:0;
      if(a!=(uint16_t)(lo|(hi<<8))){ printf("FAIL held %d: %04x %02x%02x\n",k,a,hi,lo); return 1; }
      if(same("held",k)) return 1; }
  }
  printf("PASS %lu coop kernel checks\n",(unsigned long)checks);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", assembly, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native coop check did not finish")
report = {"passed": True, "assembly_sha256": hashlib.sha256(assembly.read_bytes()).hexdigest(),
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "simulator_output": result.stdout.strip() + " " + result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
