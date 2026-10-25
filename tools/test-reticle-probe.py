#!/usr/bin/env python3
"""Check and time the linked C64 silhouette probe using independent raster spans.

Run after scripts/build-target.sh. Cycles cover the actual compiled probe call,
including its query loop and early exits, but exclude ordinary edge preparation.
"""
import argparse
import hashlib
import json
import random
import runpy
import statistics
import struct
from pathlib import Path

from native_cpu import Cpu, load_elf, RESIDENT_SECTIONS, SENTINEL

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / 'target/reticle-probe-test'
PROBE, HEAD, TAIL = 0x8000, 0x8100, 0x8120


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--elf', type=Path, default=ROOT / 'dist/havoc.elf')
    parser.add_argument('--scene-fixtures', type=Path,
                        help='reticle-clip-fixtures.json from the matching VICE smoke run')
    args = parser.parse_args()
    elf, symbols = load_elf(args.elf)
    memory = bytearray(65536)
    for section in elf.sections:
        if section.name in (*RESIDENT_SECTIONS, '.phase.shadow', '.phase.render_aux'):
            assert section.address + section.size <= 65536
            memory[section.address:section.address + section.size] = elf.data[section.offset:section.offset + section.size]
    routine = symbols['havoc::render::probe::PlayerPixelProbe::triangle']
    cpu = Cpu(memory, [(0, 0x800), (0xc400, 0xc800), (PROBE, PROBE + 11)])
    # The repr(C) probe is six coordinate bytes, three alias masks, count, hits.
    def sample(prepared, points, covered=0):
        pairs = []
        aliases = []
        for slot, point in enumerate(points):
            if point is None:
                continue
            if point not in pairs:
                pairs.append(point)
                aliases.append(0)
            aliases[pairs.index(point)] |= 1 << slot
        data = bytes(v for point in pairs for v in point).ljust(6, b'\0')
        data += bytes(aliases).ljust(3, b'\0') + bytes([len(pairs), covered])
        memory[PROBE:PROBE + 11] = data
        memory[HEAD:HEAD + 16] = prepared[:16]
        memory[TAIL:TAIL + 12] = prepared[16:]
        memory[2:4] = (0xc800).to_bytes(2, 'little')
        for address, value in ((4, PROBE), (6, HEAD), (8, TAIL)):
            memory[address:address + 2] = value.to_bytes(2, 'little')
        cpu.pc, cpu.s, cpu.cycles, cpu.steps = routine.address, 0xfd, 0, 0
        cpu.push(0xff)
        cpu.push(0xfe)
        while cpu.pc != SENTINEL:
            cpu.step()
            cpu.steps += 1
            assert cpu.steps < 100000, 'probe did not return'
        assert memory[HEAD:HEAD + 16] + memory[TAIL:TAIL + 12] == prepared
        assert memory[PROBE:PROBE + 10] == data[:10]
        assert cpu.s == 0xfd and memory[2:4] == (0xc800).to_bytes(2, 'little')
        return memory[PROBE + 10], cpu.cycles

    preparation = runpy.run_path(str(ROOT / 'tools/test-raster-prep.py'))
    rng = random.Random(0x51A0)
    fixtures = [([20,100,35],[30,60,90]), ([0,160,0],[0,0,200]),
                ([-15,180,50],[-12,11,210]), ([79,80,81],[100,199,100])]
    for _ in range(100):
        fixtures.append(([rng.randrange(-40,200) for _ in range(3)],
                         [rng.randrange(-30,230) for _ in range(3)]))
    cycles = {'outside_row': [], 'inside': [], 'outside_x': [], 'three_points': [], 'already_covered': []}
    cases = 0
    for xs, ys in fixtures:
        area = (xs[1]-xs[0])*(ys[2]-ys[0]) - (ys[1]-ys[0])*(xs[2]-xs[0])
        if not area:
            continue
        if area < 0:
            xs[1],xs[2] = xs[2],xs[1]
            ys[1],ys[2] = ys[2],ys[1]
        prepared = preparation['expected'](xs, ys, rng.randrange(16), 0xa0)
        lx,ls,sx,ss,bx,bs,top,middle,bottom,flags = struct.unpack('<6i4B',prepared)
        spans = {}
        # Independently walk rows, just as the rasterizer does, before probing.
        for y in range(top,bottom):
            if y == middle:
                sx,ss = bx,bs
            left,right = (lx,sx) if flags & 16 else (sx,lx)
            spans[y] = (max(0,min(160,left >> 16)), max(0,min(160,right >> 16)))
            lx += ls
            sx += ss
        def hit(point):
            first,end = spans.get(point[1],(0,0))
            return first <= point[0] < end
        rows = {0,199,max(0,top-1),top,min(199,bottom),max(0,bottom-1),min(199,middle),min(199,(top+bottom)//2)}
        for y in sorted(rows):
            if y >= 200:
                continue
            first,end = spans.get(y,(0,0))
            columns = {0,159,max(0,min(159,first-1)),min(159,first),max(0,min(159,end-1)),min(159,end)}
            for x in sorted(columns):
                point = (x,y)
                result,elapsed = sample(prepared,[point])
                expected = int(hit(point))
                assert result == expected,(xs,ys,point,result,expected)
                label = 'outside_row' if y not in spans else 'inside' if expected else 'outside_x'
                cycles[label].append(elapsed)
                cases += 1
            points = [(0,y),(80,y),(159,y)]
            result,elapsed = sample(prepared,points)
            assert result == sum(int(hit(point)) << slot for slot,point in enumerate(points))
            cycles['three_points'].append(elapsed)
            result,elapsed = sample(prepared,points,7)
            assert result == 7
            cycles['already_covered'].append(elapsed)
            cases += 2
        result,_ = sample(prepared,[(0,0),(0,0),(0,0)])
        assert result == (7 if hit((0,0)) else 0)
        cases += 1
    report = dict(passed=True,cases=cases,elf_sha256=hashlib.sha256(args.elf.read_bytes()).hexdigest(),
                  routine_bytes=routine.size,cycles={key:dict(min=min(v),median=statistics.median(v),max=max(v)) for key,v in cycles.items()},
                  nominal_clock_mhz=64,scope='Linked production probe; CPU cycles exclude preparation, IRQ and bus stalls.')
    if args.scene_fixtures:
        scenes = []
        for scene in json.loads(args.scene_fixtures.read_text()):
            timings = {}
            for label, points in (('one_point', scene['points'][:1]), ('all_positions', scene['points'])):
                covered = elapsed = 0
                for encoded in scene['triangles']:
                    values = struct.unpack('<7h2B', bytes.fromhex(encoded))
                    prepared = preparation['expected'](values[:3], values[3:6], values[7], values[8])
                    covered, cost = sample(prepared, [tuple(p) for p in points], covered)
                    elapsed += cost
                assert bool(covered & 1) == scene['hidden']
                timings[label] = dict(cycles=elapsed, microseconds_at_64mhz=elapsed/64)
            scenes.append(dict(pose=scene['pose'], boosted=scene.get('boosted', False),
                               exhaust_phase=scene.get('exhaust_phase'),
                               triangles=len(scene['triangles']), **timings))
        report['ship_samples'] = scenes
    BUILD.mkdir(parents=True,exist_ok=True)
    (BUILD/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    main()
