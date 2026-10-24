#!/usr/bin/env python3
"""Compare the linked quad-SID IRQ player with exact host state/write fixtures."""
import argparse
from collections import defaultdict
import hashlib
import json
import os
from pathlib import Path
import runpy
from native_cpu import Cpu, KernelError, load_elf, RESIDENT_SECTIONS
from sfx_profile import Aggregate, Sample, Symbols

ROOT = Path(__file__).resolve().parents[1]


def fixture_index(path, count):
    index = json.loads(path.read_text()) if path.exists() else dict(
        format='sfx-fixture-index-v1', tick_hz=50,
        groups=[dict(name='directed', start=0, count=count, continuous=False)])
    if index.get('format') != 'sfx-fixture-index-v1' or index.get('tick_hz') != 50:
        raise KernelError('unsupported fixture index or effects cadence')
    cursor, names = 0, set()
    for group in index['groups']:
        if (group['start'] != cursor or group['count'] <= 0 or group['name'] in names
                or group['continuous'] and group['count'] < 1500):
            raise KernelError('fixture groups must partition all cases and sustain at least thirty seconds')
        names.add(group['name'])
        cursor += group['count']
    if cursor != count:
        raise KernelError('fixture index case count differs')
    return index


def check_continuity(previous_after, previous_regs, before, regs, queue_begin, queue_end):
    if (before[:queue_begin] != previous_after[:queue_begin]
            or before[queue_end:] != previous_after[queue_end:] or regs != previous_regs):
        raise KernelError('sustained state continuity broken')

class EffectsCpu(Cpu):
    def __init__(self, memory, writable):
        super().__init__(memory, writable)
        self.sid_writes = bytearray()
        self.min_stack = 0x7600

    def write(self, address, value):
        super().write(address, value)
        if 0xd400 <= address < 0xd480:
            sid, reg = divmod(address-0xd400, 32)
            if reg >= 25:
                raise KernelError(f"invalid SID register {reg}")
            self.sid_writes.extend((sid,reg,value & 255))

def irq_adapter(base, writable, symbols, idle, state):
    """Measure the linked register save/restore and an audio-only CIA dispatch.

    Hardware IRQ acceptance adds seven processor cycles. Raster/input work and
    non-SID I/O stalls are deliberately outside this model. The wrapper backs
    up no compiler static frames: a write outside the IRQ's own stack and
    state here is a fault in the CPU model.
    """
    before, regs, after, expected_regs, expected_writes, worker_cycles = idle
    writable = list(writable)
    memory = base.copy()
    memory[state.address:state.address + state.size] = before
    memory[0xd400:0xd480] = regs
    memory[symbols['AUDIO_READY'].address] = 1
    memory[0xdc0d], memory[0xd019], memory[1] = 1, 0, 0x34
    memory[2:4] = (0xc800).to_bytes(2, 'little')

    def begin():
        cpu = EffectsCpu(memory.copy(), writable)
        cpu.a, cpu.x, cpu.y = 0x12, 0x34, 0x56
        cpu.push(0xff); cpu.push(0xff); cpu.push(cpu.p)
        cpu.pc = symbols['irq_entry'].address
        return cpu

    cpu = begin()
    saved = bytes(cpu.mem[:34])
    for _ in range(100000):
        if cpu.pc == symbols['irq_body'].address:
            break
        cpu.step()
    else:
        raise KernelError('IRQ entry did not reach irq_body')
    entry_cycles = cpu.cycles + 7
    low, high = cpu.pop(), cpu.pop()
    cpu.pc = ((low | high << 8) + 1) & 0xffff
    begin_exit = cpu.cycles
    for _ in range(100000):
        if cpu.pc == 0xffff:
            break
        cpu.step()
    else:
        raise KernelError('IRQ exit did not return')
    exit_cycles = cpu.cycles - begin_exit
    if cpu.mem[:34] != saved or (cpu.a, cpu.x, cpu.y, cpu.s) != (0x12, 0x34, 0x56, 0xfd):
        raise KernelError('IRQ adapter did not preserve foreground context')
    cpu = begin()
    for _ in range(300000):
        if cpu.pc == 0xffff:
            break
        cpu.step()
    else:
        raise KernelError('audio-only IRQ did not return')
    if (cpu.mem[state.address:state.address + state.size] != after
            or cpu.mem[0xd400:0xd480] != expected_regs or cpu.sid_writes != expected_writes):
        raise KernelError('audio-only IRQ differs from the direct worker fixture')
    dispatch = cpu.cycles + 7 - worker_cycles - entry_cycles - exit_cycles
    if dispatch < 0:
        raise KernelError('invalid IRQ dispatch accounting')
    return dict(entry_cpu_cycles=entry_cycles, exit_cpu_cycles=exit_cycles,
                cia_audio_dispatch_cpu_cycles=dispatch,
                total_additional_cpu_cycles=entry_cycles + exit_cycles + dispatch,
                hardware_acceptance_cycles=7,
                scope='Linked save/restore plus CIA audio-only dispatch; excludes raster/input work and non-SID I/O waits; model, not hardware timing')


def run(elf, symbols, fixture, standard, layout, *, profile=True):
    sections = {s.name:s for s in elf.sections}
    base = bytearray(65536)
    for name in RESIDENT_SECTIONS:
        s=sections[name]; base[s.address:s.address+s.size]=elf.contents(s)
    writable=[(0,0x200),(0xd400,0xd480),(0x7500,0x7600)]
    for name in ('.data','.bss','.noinit','.sfx_state'):
        s=sections[name]; writable.append((s.address,s.address+s.size))
    state=symbols['EFFECTS']; ready=symbols['AUDIO_READY'].address
    entry=symbols['havoc_c64::audio_tick'].address
    data=fixture.read_bytes()
    assert data[:4]==b'SFX1'
    size=int.from_bytes(data[4:6],'little'); count=int.from_bytes(data[6:10],'little')
    assert size==state.size<=224
    index_path = fixture.with_suffix('.json')
    index = fixture_index(index_path, count)
    groups = index['groups']
    group_for_case = {}
    for group in groups:
        for case in range(group['start'], group['start'] + group['count']):
            group_for_case[case] = group
    structure = layout['EffectsEngine']['fields']
    instance = layout['Instance']
    instances_at = structure['instances']['offset']
    queue_begin = structure['events']['offset']
    queue_end = structure['event_count']['offset'] + 1
    def instance_value(image, sid, field):
        return image[instances_at + sid * instance['size'] + instance['fields'][field]['offset']]
    def category(before, after):
        active = [instance_value(before, sid, 'mask') != 0 for sid in range(4)]
        queued = before[structure['event_count']['offset']] != 0
        if not queued:
            return 'active-no-events' if any(active) else 'idle'
        started = [sid for sid in range(4) if instance_value(after, sid, 'mask') != 0
                   and instance_value(after, sid, 'age') == 0]
        if not started:
            return 'events-rejected'
        if any(active[sid] and instance_value(before, sid, 'remaining') > 1 for sid in started):
            return 'preemption-or-retrigger'
        return 'new-start'
    attribution = Symbols(elf, RESIDENT_SECTIONS)
    total = Aggregate()
    categories, workloads = defaultdict(Aggregate), defaultdict(Aggregate)
    links = defaultdict(int)
    previous_group = previous_after = previous_regs = idle = None
    at=10; maximum=maximum_writes=total_cycles=0; min_stack=0x7600
    for case in range(count):
        before=data[at:at+size]; at+=size
        regs=data[at:at+128];at+=128
        after=data[at:at+size];at+=size
        expected_regs=data[at:at+128];at+=128
        length=int.from_bytes(data[at:at+2],'little');at+=2
        expected_writes=data[at:at+length];at+=length
        group = group_for_case[case]
        if group['continuous'] and previous_group == group['name']:
            check_continuity(previous_after, previous_regs, before, regs, queue_begin, queue_end)
            links[group['name']] += 1
        mem=base.copy();mem[state.address:state.address+size]=before
        mem[0xd400:0xd480]=regs;mem[ready]=1
        mem[2:4]=(0x7600).to_bytes(2,'little')
        cpu=EffectsCpu(mem,writable);cpu.pc=entry;cpu.push(0xff);cpu.push(0xfe)
        sample = Sample(attribution, entry) if profile else None
        for _ in range(300000):
            if cpu.pc==0xffff:break
            sample.step(cpu) if sample is not None else cpu.step()
            stack=int.from_bytes(mem[2:4],'little')
            min_stack=min(min_stack,stack)
            if not 0x7500<=stack<=0x7600:raise KernelError('SFX software stack overflow')
        else:raise KernelError(f'{standard} case{case}: tick did not return')
        actual=bytes(mem[state.address:state.address+size])
        if actual!=after:
            diff=next(i for i,(a,b) in enumerate(zip(actual,after)) if a!=b)
            raise KernelError(f'{standard} case{case}: state byte{diff}: {actual[diff]} != {after[diff]}')
        if mem[0xd400:0xd480]!=expected_regs or cpu.sid_writes!=expected_writes:
            raise KernelError(f'{standard} case{case}: SID register trace differs')
        if sample is not None and (sample.frames or sum(sample.exclusive.values()) != cpu.cycles):
            raise KernelError('instruction/call attribution did not balance')
        kind = category(before, after)
        if kind == 'idle' and idle is None:
            idle = (before, regs, after, expected_regs, expected_writes, cpu.cycles)
        for aggregate in (total, categories[kind], workloads[group['name']]):
            aggregate.add(cpu, sample)
        previous_group, previous_after, previous_regs = group['name'], actual, bytes(mem[0xd400:0xd480])
        modeled_cycles = cpu.cycles + 63 * (length // 3)
        maximum=max(maximum,modeled_cycles)
        total_cycles += modeled_cycles
        maximum_writes=max(maximum_writes,length//3)
    assert at==len(data)
    bus=985248 if standard=='pal' else 1022727
    millis=maximum/(64*bus)*1000
    assert millis<20, 'effects missed their 50 Hz deadline'
    assert idle is not None
    irq = irq_adapter(base, writable, symbols, idle, state)
    overhead = irq['total_additional_cpu_cycles']
    workload_reports = {}
    for group in groups:
        result = workloads[group['name']].report(bus, overhead, detailed=profile, symbols=attribution)
        result.update(continuous=group['continuous'], continuous_state_links=links[group['name']])
        if group['continuous']:
            assert links[group['name']] == group['count'] - 1
            result['simulated_seconds_at_50_hz'] = group['count'] / 50
        workload_reports[group['name']] = result
    return dict(cases=count,state_bytes=size,max_tick_ms_model=millis,
                mean_fixture_tick_ms_model=total_cycles/count/(64*bus)*1000,
                max_sid_writes=maximum_writes,software_stack_bytes=0x7600-min_stack,
                fixture_sha256=hashlib.sha256(data).hexdigest(), bus_hz=bus, tick_hz=50,
                irq_adapter=irq,
                categories={name: value.report(bus, overhead, detailed=profile, symbols=attribution)
                            for name, value in categories.items()},
                workloads=workload_reports,
                attribution=total.report(bus, overhead, detailed=profile, symbols=attribution),
                attribution_note='Exclusive cycles partition executed instructions. Inclusive cycles include JSR descendants and logical tail-call chains, so must not be summed. SID stalls are separate 63-cycle additions per write.',
                mean_fixture_note='Directed-case averages are coverage statistics, not sustained CPU load; sustained workload duty uses every tick at 50 Hz.',
                passed=True)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--elf',type=Path,default=ROOT/'dist/havoc.elf')
    parser.add_argument('--fixtures',type=Path,default=ROOT/'target/sfx-fixtures')
    parser.add_argument('--report',type=Path,default=ROOT/'target/sfx-kernel/report.json')
    parser.add_argument('--standard',choices=('pal','ntsc','both'),default='both')
    parser.add_argument('--no-profile',action='store_true',help='check exact fixtures without per-instruction attribution')
    parser.add_argument('--llvm-dwarfdump',type=Path,
                        default=Path(os.environ.get('LLVM_MOS', Path.home()/'.local/llvm-mos'))/'bin/llvm-dwarfdump')
    args=parser.parse_args();elf,symbols=load_elf(args.elf)
    report={'elf_sha256':hashlib.sha256(args.elf.read_bytes()).hexdigest(),
            'model':'NMOS6502 at64x bus; SID stores charged one bus cycle; audio worker and linked IRQ entry/exit reported separately; not physical hardware timing'}
    preview=runpy.run_path(str(ROOT/'tools/sfx-previews.py'))
    layout=preview['read_effects_layout'](args.elf,args.llvm_dwarfdump)
    report['effects_layout']=layout
    for standard in ('pal','ntsc') if args.standard=='both' else (args.standard,):
        report[standard]=run(elf,symbols,args.fixtures/f'{standard}.bin',standard,layout,profile=not args.no_profile)
        print(standard, {key: report[standard][key] for key in ('cases','max_tick_ms_model','max_sid_writes','passed')},flush=True)
    args.report.parent.mkdir(parents=True,exist_ok=True)
    args.report.write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__':main()
