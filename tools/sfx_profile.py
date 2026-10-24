"""Instruction/call attribution for the modeled native SID regression player."""
from collections import Counter
import runpy
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parents[1]


class Symbols:
    def __init__(self, elf, resident_sections):
        readable = runpy.run_path(str(ROOT / 'tools/check-overlays.py'))['readable_name']
        sections = {section.index: section for section in elf.sections}
        self.owner = [None] * 65536
        self.starts = {}
        self.functions = {}
        for symbol in elf.symbols:
            if symbol.kind != 2 or symbol.size == 0:
                continue
            section = sections.get(symbol.section)
            if section is None or section.name not in resident_sections:
                continue
            name = readable(symbol.name)
            self.functions[name] = dict(address=symbol.address, size=symbol.size)
            self.starts[symbol.address] = name
            self.owner[symbol.address:symbol.address + symbol.size] = [name] * symbol.size


class Sample:
    def __init__(self, symbols, entry):
        self.symbols = symbols
        self.frames = [[symbols.starts.get(entry, '<entry>')]]
        self.pc_cycles = Counter()
        self.exclusive = Counter()
        self.inclusive = Counter()
        self.stalls = Counter()
        self.calls = Counter()
        self.paths = Counter()
        self.calls[self.frames[0][0]] = 1
        self.context_changed()

    def context_changed(self):
        chain = [name for frame in self.frames for name in frame]
        self.parents = tuple(dict.fromkeys(chain))
        self.context = ' > '.join(chain)

    def step(self, cpu):
        pc, cycles, writes = cpu.pc, cpu.cycles, len(cpu.sid_writes)
        opcode = cpu.mem[pc]
        owner = self.symbols.owner[pc] or self.frames[-1][-1]
        # A tail jump keeps the caller in the inclusive logical call chain but
        # shares its hardware return frame. Ordinary JSR/RTS nesting is exact.
        if owner not in self.frames[-1]:
            self.frames[-1].append(owner)
            self.calls[owner] += 1
            self.context_changed()
        cpu.step()
        elapsed = cpu.cycles - cycles
        stall = (len(cpu.sid_writes) - writes) // 3 * 63
        self.pc_cycles[pc] += elapsed
        self.exclusive[owner] += elapsed
        self.stalls[owner] += stall
        self.paths[self.context] += elapsed
        for name in self.parents:
            self.inclusive[name] += elapsed
        if opcode == 0x20:  # JSR cycles belong to the caller, body to callee.
            callee = self.symbols.starts.get(cpu.pc, self.symbols.owner[cpu.pc] or f'${cpu.pc:04x}')
            self.calls[callee] += 1
            self.frames.append([callee])
            self.context_changed()
        elif opcode == 0x60:
            self.frames.pop()
            self.context_changed()


class Aggregate:
    def __init__(self):
        self.count = self.cpu_cycles = self.sid_stall_cycles = self.sid_writes = 0
        self.max_cycles = self.max_writes = 0
        self.exclusive = Counter()
        self.inclusive = Counter()
        self.stalls = Counter()
        self.calls = Counter()
        self.pc_cycles = Counter()
        self.paths = Counter()
        self.samples = []

    def add(self, cpu, profile):
        writes = len(cpu.sid_writes) // 3
        self.count += 1
        self.cpu_cycles += cpu.cycles
        self.sid_stall_cycles += writes * 63
        self.sid_writes += writes
        self.max_cycles = max(self.max_cycles, cpu.cycles + writes * 63)
        self.samples.append(cpu.cycles + writes * 63)
        self.max_writes = max(self.max_writes, writes)
        if profile is not None:
            self.exclusive.update(profile.exclusive)
            self.inclusive.update(profile.inclusive)
            self.stalls.update(profile.stalls)
            self.calls.update(profile.calls)
            self.pc_cycles.update(profile.pc_cycles)
            self.paths.update(profile.paths)

    def report(self, bus, irq_cycles=0, *, detailed=False, symbols=None):
        cycles = self.cpu_cycles + self.sid_stall_cycles
        factor = 1000 / (64 * bus)
        mean = cycles / self.count if self.count else 0
        ordered = sorted(self.samples)
        p95 = ordered[(len(ordered) * 95 + 99) // 100 - 1] if ordered else 0
        result = dict(ticks=self.count, cpu_cycles=self.cpu_cycles,
                      sid_stall_cycles=self.sid_stall_cycles, sid_writes=self.sid_writes,
                      mean_tick_ms_model=mean * factor, max_tick_ms_model=self.max_cycles * factor,
                      median_tick_ms_model=(statistics.median(ordered) if ordered else 0) * factor,
                      p95_tick_ms_model=p95 * factor,
                      max_sid_writes=self.max_writes,
                      duty_percent_at_50_hz_model=mean * factor * 5,
                      duty_with_audio_irq_at_50_hz_model=(mean + irq_cycles) * factor * 5)
        if detailed:
            result['functions'] = [{
                'function': name, 'exclusive_cpu_cycles': self.exclusive[name],
                'inclusive_cpu_cycles': self.inclusive[name],
                'sid_stall_cycles': self.stalls[name], 'calls': self.calls[name],
            } for name in sorted(self.inclusive, key=self.exclusive.get, reverse=True)]
            result['pc_cpu_cycles'] = [dict(pc=f'${pc:04x}', cpu_cycles=cycles,
                function=symbols.owner[pc] if symbols else None)
                for pc, cycles in self.pc_cycles.most_common()]
            result['call_path_cpu_cycles'] = [dict(call_path=path, cpu_cycles=cycles)
                                               for path, cycles in self.paths.most_common()]
        return result
