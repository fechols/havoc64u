#!/usr/bin/env python3
"""Differential check of the native mesh kernel.

Usage: python3 tools/test-mesh-kernel.py
Requires LLVM-MOS's mos-sim-clang and mos-sim. The harness builds the
quarter-square and projection tables, provides a RENDERER image and compares
fast_emit_mesh with a C transcription of the reference mesh path
(emit_mesh_reference feeding SceneBudget::triangle and Renderer::triangle):
every vertex rotated by a 3x3 Q8 matrix with the arithmetic shift, translated
with saturating adds and lowered by the chase camera; all faces visited in
order; faces entirely at or behind the near plane projected
and emitted (project_triangle); faces entirely in front dropped; faces that
straddle the plane handed back for clipping in sequence. Random meshes cover
every shift, colour override, metadata flag bits, i16 saturation,
the 256-command capacity and 128-vertex meshes. The vertex records at $E000
are checked as well. The JSON report stays under target/mesh-test.
"""
import hashlib
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
build = root / "target/mesh-test"
build.mkdir(parents=True, exist_ok=True)
sdk = pathlib.Path(os.environ.get("LLVM_MOS", str(pathlib.Path.home()/".local/llvm-mos"))) / "bin"
assemblies = [root / "target-c64/asm/multiply.S", root / "target-c64/asm/project.S",
              root / "target-c64/asm/emit.S", root / "target-c64/asm/division.S"]
source = r'''
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
struct Vec3 { int16_t x, y, z; };
struct __attribute__((packed)) MeshArgs {
  const uint8_t *vertices; const uint8_t *faces; const int16_t *matrix;
  struct Vec3 position; int16_t camera_y;
  uint8_t vertex_count, face_count, shift, flags, color, face_color, indices[3];
};
_Static_assert(sizeof(struct MeshArgs)==23,"mesh argument ABI");
_Static_assert(offsetof(struct MeshArgs,face_color)==19,"mesh return ABI");
extern uint8_t fast_emit_mesh(struct MeshArgs *args, uint8_t start);
#define QLO ((uint8_t*)0xc000)
#define QHI ((uint8_t*)0xc200)
#define RECORDS ((uint8_t*)0xe000)
uint16_t PROJECT_X[128], PROJECT_Y[128];
uint8_t RENDERER[6 + 256 * 16];
static uint8_t reference_renderer[6 + 256 * 16];
static uint8_t vertices[128 * 3], faces[256 * 4];
static int16_t matrix[9];
static struct Vec3 cam[128];
static uint8_t clipped_faces[256], clipped_count, ref_clipped[256], ref_clipped_count, ref_clip_color[256], kernel_clip_color[256];
static int16_t clamp16(int32_t v, int32_t lo, int32_t hi){ return (int16_t)(v<lo?lo:(v>hi?hi:v)); }
static void project(const struct Vec3 *p, int16_t out[2]){
  uint16_t d=(uint16_t)p->z;
  if(d==0){out[0]=80;out[1]=100;return;}
  int8_t e=0;
  while(d>255){d>>=1;e++;}
  while(d<128){d<<=1;e--;}
  uint8_t shift=(uint8_t)(15+e);
  int32_t x=((int32_t)p->x*(int32_t)PROJECT_X[d-128])>>shift;
  int32_t y=(-(int32_t)p->y*(int32_t)PROJECT_Y[d-128])>>shift;
  out[0]=clamp16(80+x,-8191,8191); out[1]=clamp16(100+y,-8191,8191);
}
static void emit(uint8_t *r, const struct Vec3 p[3], uint8_t color, uint8_t flags){
  int16_t sx[3], sy[3];
  for(uint8_t i=0;i<3;i++){ int16_t out[2]; project(&p[i],out); sx[i]=out[0]; sy[i]=out[1]; }
  int32_t area=((int32_t)sx[1]-sx[0])*((int32_t)sy[2]-sy[0])-((int32_t)sy[1]-sy[0])*((int32_t)sx[2]-sx[0]);
  if(area<=0)return;
  if((sx[0]<=0&&sx[1]<=0&&sx[2]<=0)||(sx[0]>=160&&sx[1]>=160&&sx[2]>=160)||
     (sy[0]<=0&&sy[1]<=0&&sy[2]<=0)||(sy[0]>=200&&sy[1]>=200&&sy[2]>=200))return;
  uint16_t count=(uint16_t)(r[0]|r[1]<<8);
  if(count==256){ uint16_t dropped=(uint16_t)(r[2]|r[3]<<8); if(dropped!=65535)dropped++; r[2]=(uint8_t)dropped; r[3]=(uint8_t)(dropped>>8); return; }
  uint32_t sum=(uint32_t)(uint16_t)p[0].z+(uint16_t)p[1].z+(uint16_t)p[2].z;
  uint16_t depth=(uint16_t)(sum/3);
  uint8_t *c=r+6+count*16;
  for(uint8_t i=0;i<3;i++){ c[i*2]=(uint8_t)sx[i]; c[i*2+1]=(uint8_t)(sx[i]>>8); c[6+i*2]=(uint8_t)sy[i]; c[7+i*2]=(uint8_t)(sy[i]>>8); }
  c[12]=(uint8_t)depth; c[13]=(uint8_t)(depth>>8); c[14]=color&15; c[15]=flags&0xe2;
  count++; r[0]=(uint8_t)count; r[1]=(uint8_t)(count>>8);
}
static int16_t sat_add(int16_t a, int16_t b){ int32_t s=(int32_t)a+b; return clamp16(s,-32768,32767); }
/* The reference: emit_mesh_reference -> SceneBudget::triangle -> Renderer::triangle. */
static void reference(const struct MeshArgs *a){
  for(uint8_t i=0;i<a->vertex_count;i++){
    const int8_t *v=(const int8_t*)a->vertices+i*3;
    int32_t s[3];
    for(uint8_t row=0;row<3;row++){
      s[row]=(int32_t)a->matrix[row*3]*v[0]+(int32_t)a->matrix[row*3+1]*v[1]+(int32_t)a->matrix[row*3+2]*v[2];
      s[row]>>=a->shift;
    }
    cam[i].x=sat_add((int16_t)s[0],a->position.x);
    cam[i].y=(int16_t)(sat_add((int16_t)s[1],a->position.y)-a->camera_y);
    cam[i].z=sat_add((int16_t)s[2],a->position.z);
  }
  ref_clipped_count=0;
  for(uint8_t f=0;f<a->face_count;f++){
    const uint8_t *face=a->faces+f*4;
    struct Vec3 p[3]={cam[face[0]],cam[face[1]],cam[face[2]]};
    uint8_t color=a->color?a->color:face[3];
    uint8_t behind=(p[0].z>=8)+(p[1].z>=8)+(p[2].z>=8);
    if(behind==3) emit(reference_renderer,p,color,a->flags);
    else if(behind){ ref_clipped[ref_clipped_count]=f; ref_clip_color[ref_clipped_count++]=color; }
  }
}
static uint32_t checks, emitted, clipped_total, vertices_total;
static int fail(const char *what, uint32_t detail){ printf("FAIL check %lu: %s (%lu)\n",(unsigned long)checks,what,(unsigned long)detail); return 1; }
static int check(struct MeshArgs *a){
  checks++;
  struct MeshArgs kernel=*a;
  reference(a);
  clipped_count=0;
  uint8_t face=0;
  while(1){
    face=fast_emit_mesh(&kernel,face);
    if(face>=a->face_count)break;
    clipped_faces[clipped_count]=face; kernel_clip_color[clipped_count++]=kernel.face_color;
    const uint8_t *want=a->faces+face*4;
    if(kernel.indices[0]!=want[0]||kernel.indices[1]!=want[1]||kernel.indices[2]!=want[2])return fail("clipped face indices",face);
    if(clipped_count>250)return fail("clip loop",face);
    face++;
  }
  if(clipped_count!=ref_clipped_count)return fail("clipped face count",clipped_count*256u+ref_clipped_count);
  if(memcmp(&kernel,a,offsetof(struct MeshArgs,face_color)))return fail("mesh input arguments changed",0);
  for(uint8_t i=0;i<clipped_count;i++){
    if(clipped_faces[i]!=ref_clipped[i])return fail("clipped face order",i);
    if(kernel_clip_color[i]!=ref_clip_color[i])return fail("clipped face colour",i);
  }
  for(uint8_t i=0;i<a->vertex_count;i++){
    const uint8_t *rec=RECORDS+i*16;
    struct Vec3 got; memcpy(&got,rec,6);
    if(got.x!=cam[i].x||got.y!=cam[i].y||got.z!=cam[i].z){
      printf(" vertex %u got (%d,%d,%d) want (%d,%d,%d)\n",i,got.x,got.y,got.z,cam[i].x,cam[i].y,cam[i].z);
      return fail("vertex record",i);
    }
    if(rec[10]){ int16_t out[2]; project(&cam[i],out); int16_t sx=(int16_t)(rec[6]|rec[7]<<8), sy=(int16_t)(rec[8]|rec[9]<<8);
      if(sx!=out[0]||sy!=out[1]){ printf(" vertex %u screen (%d,%d) want (%d,%d)\n",i,sx,sy,out[0],out[1]); return fail("projected record",i);} }
  }
  if(memcmp(RENDERER,reference_renderer,sizeof RENDERER)){
    uint16_t at=0; while(at<sizeof RENDERER&&RENDERER[at]==reference_renderer[at])at++;
    printf("FAIL check %lu at byte %u: got %02x want %02x; count %u/%u\n",(unsigned long)checks,at,RENDERER[at],reference_renderer[at],
           RENDERER[0]|RENDERER[1]<<8,reference_renderer[0]|reference_renderer[1]<<8);
    { uint16_t index=(uint16_t)((at>=6?at-6:0)/16); const int16_t *c=(const int16_t*)(RENDERER+6+index*16); const int16_t *w=(const int16_t*)(reference_renderer+6+index*16);
      printf(" command %u kernel: x %d %d %d y %d %d %d depth %d color %u reserved %u\n",index,c[0],c[1],c[2],c[3],c[4],c[5],c[6],RENDERER[6+index*16+14],RENDERER[6+index*16+15]);
      printf(" command %u ref:    x %d %d %d y %d %d %d depth %d color %u reserved %u\n",index,w[0],w[1],w[2],w[3],w[4],w[5],w[6],reference_renderer[6+index*16+14],reference_renderer[6+index*16+15]); }
    return 1;
  }
  clipped_total+=clipped_count; vertices_total+=a->vertex_count;
  return 0;
}
static void begin_frame(uint16_t count){
  memset(RENDERER,0,sizeof RENDERER); memset(reference_renderer,0,sizeof reference_renderer);
  RENDERER[0]=reference_renderer[0]=(uint8_t)count; RENDERER[1]=reference_renderer[1]=(uint8_t)(count>>8);
  memset(RECORDS,0xa5,128*16);
}
static uint32_t state=0x9e3779b9u;
static uint32_t next(void){ state=state*1664525u+1013904223u; return state; }
static int16_t pick(uint32_t bits, int16_t span){ return (int16_t)((int32_t)(bits%(2*span+1))-span); }
int main(void){
  uint16_t square=0;
  for(uint16_t n=0;n<512;n++){QLO[n]=(uint8_t)square;QHI[n]=(uint8_t)(square>>8);square=(uint16_t)(square+((n+1)>>1));}
  for(uint16_t i=0;i<128;i++){uint32_t d=i+128; PROJECT_X[i]=(uint16_t)((80ul*32768ul+d/2)/d); PROJECT_Y[i]=(uint16_t)((133ul*32768ul+d/2)/d);}
  /* A visible last face after 254 culled faces must still be visited. Then
     exercise every physical-capacity rejection in a complete 255-face mesh. */
  {
    static const int8_t points[9]={-40,-30,0,10,50,0,60,-30,0};
    memcpy(vertices,points,sizeof points);
    for(uint8_t i=0;i<9;i++)matrix[i]=(i%4==0)?256:0;
    struct MeshArgs a={vertices,faces,matrix,{0,0,200},0,3,255,8,0x63,0,0,{0,0,0}};
    for(uint16_t f=0;f<255;f++){
      faces[f*4]=0;faces[f*4+1]=2;faces[f*4+2]=1;faces[f*4+3]=7;
    }
    faces[254*4+1]=1;faces[254*4+2]=2;
    begin_frame(0);
    if(check(&a))return 1;
    if(RENDERER[0]!=1||RENDERER[1])return fail("late visible face was skipped",RENDERER[0]);
    for(uint16_t f=0;f<255;f++){faces[f*4+1]=1;faces[f*4+2]=2;}
    for(uint16_t start=254;start<=256;start++){
      begin_frame(start);
      if(check(&a))return 1;
      if(RENDERER[0]||RENDERER[1]!=1||RENDERER[2]!=(uint8_t)(start-1)||RENDERER[3])
        return fail("complete overflow accounting",start);
    }
    begin_frame(256);
    RENDERER[2]=reference_renderer[2]=0xfe;RENDERER[3]=reference_renderer[3]=0xff;
    if(check(&a))return 1;
    if(RENDERER[2]!=0xff||RENDERER[3]!=0xff)return fail("overflow saturation",0);
  }
  for(uint16_t mesh=0;mesh<160;mesh++){
    struct MeshArgs a;
    uint8_t vc=(uint8_t)(mesh%40==39?128:4+(next()>>8)%28);
    uint8_t fc=(uint8_t)(mesh%40==39?200:(next()>>8)%40);
    for(uint16_t i=0;i<vc*3u;i++) vertices[i]=(uint8_t)(next()>>12);
    for(uint16_t f=0;f<fc;f++){ for(uint8_t c=0;c<3;c++) faces[f*4+c]=(uint8_t)((next()>>9)%vc); faces[f*4+3]=(uint8_t)((next()>>7)&15); }
    uint8_t kind=(uint8_t)((next()>>5)%6);
    for(uint8_t i=0;i<9;i++){
      if(kind==0) matrix[i]=(i%4==0)?256:0;                     /* identity */
      else if(kind==1) matrix[i]=pick(next()>>3,256);           /* any entries */
      else matrix[i]=(next()>>7)%3==0?0:pick(next()>>3,255);    /* sparse */
    }
    a.vertices=vertices; a.faces=faces; a.matrix=matrix;
    a.shift=(uint8_t)(5+(next()>>9)%5);
    uint8_t extreme=(uint8_t)((next()>>6)%7==0);
    if(extreme){ a.position.x=(int16_t)(next()>>16); a.position.y=(int16_t)(next()>>16); a.position.z=(int16_t)(next()>>16); }
    else { a.position.x=pick(next()>>4,900); a.position.y=pick(next()>>4,500); a.position.z=(int16_t)(-60+(next()>>8)%1500); }
    if(mesh%7==3) a.position.z=(int16_t)(-20+(next()>>8)%60);  /* straddling the near plane */
    a.camera_y=pick(next()>>5,200);
    a.vertex_count=vc; a.face_count=fc;
    a.flags=(uint8_t)((next()>>8)&((mesh&1)?0xff:0x63));
    a.color=(uint8_t)((next()>>11)%4==0?1+(next()>>7)%15:0);
    uint16_t renderer_count=(uint16_t)((next()>>9)%9==0?250+(next()>>8)%7:(next()>>8)%64);
    begin_frame(renderer_count);
    a.face_color=0; a.indices[0]=a.indices[1]=a.indices[2]=0;
    if(check(&a))return 1;
    emitted+=(uint16_t)((RENDERER[0]|RENDERER[1]<<8)-renderer_count);
  }
  printf("PASS %lu meshes, %lu vertices, %lu commands, %lu faces clipped\n",(unsigned long)checks,(unsigned long)vertices_total,(unsigned long)emitted,(unsigned long)clipped_total);
  return 0;
}
'''
(build/"check.c").write_text(source, encoding="utf-8")
subprocess.run([sdk/"mos-sim-clang", "-Os", "-mlto-zp=0", build/"check.c", *assemblies, "-o", build/"check"], check=True)
result = subprocess.run([sdk/"mos-sim", "--cycles", build/"check"], text=True, capture_output=True)
print(result.stdout.strip(), flush=True)
print(result.stderr.strip(), flush=True)
if result.returncode or "PASS" not in result.stdout:
    raise SystemExit(result.returncode or "native mesh check did not finish")
report = {"passed": True,
          "assembly_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in assemblies},
          "reference": "emit_mesh_reference through SceneBudget::triangle and Renderer::triangle, transcribed to C",
          "cpu": "LLVM-MOS mos-sim 6502 instruction simulator",
          "cycles_include_test_harness": True, "simulator_output": result.stdout.strip()+" "+result.stderr.strip()}
(build/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
