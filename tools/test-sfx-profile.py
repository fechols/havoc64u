#!/usr/bin/env python3
"""Independent checks for native SFX profiling and sustained-fixture evidence."""
import json
from pathlib import Path
import runpy
import tempfile
from types import SimpleNamespace
import unittest

from native_cpu import KernelError
from sfx_profile import Aggregate, Sample

KERNEL = runpy.run_path(str(Path(__file__).with_name('test-sfx-kernel.py')))


class ProfileTests(unittest.TestCase):
    def test_distribution_uses_median_and_nearest_rank_p95(self):
        aggregate = Aggregate()
        for cycles in range(1, 101):
            aggregate.add(SimpleNamespace(cycles=cycles, sid_writes=bytearray()), None)
        report = aggregate.report(1000000)
        self.assertAlmostEqual(report['mean_tick_ms_model'], 50.5 / 64000)
        self.assertAlmostEqual(report['median_tick_ms_model'], 50.5 / 64000)
        self.assertAlmostEqual(report['p95_tick_ms_model'], 95 / 64000)
        self.assertAlmostEqual(report['max_tick_ms_model'], 100 / 64000)

    def test_jsr_tail_jump_rts_and_sid_stall_accounting(self):
        memory = bytearray(65536)
        # caller: JSR callee; NOP; RTS (14 cycles)
        memory[0x1000:0x1005] = bytes([0x20, 0x00, 0x11, 0xea, 0x60])
        # callee: LDA #1; JMP tail (5 cycles)
        memory[0x1100:0x1105] = bytes([0xa9, 1, 0x4c, 0x00, 0x12])
        # tail: STA SID; RTS (10 cycles, plus modeled 63-cycle stall)
        memory[0x1200:0x1204] = bytes([0x8d, 0x00, 0xd4, 0x60])
        owners = [None] * 65536
        for begin, size, name in ((0x1000, 5, 'caller'), (0x1100, 5, 'callee'), (0x1200, 4, 'tail')):
            owners[begin:begin + size] = [name] * size
        symbols = SimpleNamespace(owner=owners, starts={0x1000:'caller', 0x1100:'callee', 0x1200:'tail'})
        cpu = KERNEL['EffectsCpu'](memory, [(0xd400, 0xd480)])
        cpu.push(0xff); cpu.push(0xfe); cpu.pc = 0x1000
        profile = Sample(symbols, cpu.pc)
        while cpu.pc != 0xffff:
            profile.step(cpu)
        self.assertEqual(cpu.cycles, 29)
        self.assertEqual(profile.frames, [])
        self.assertEqual(dict(profile.exclusive), dict(caller=14, callee=5, tail=10))
        self.assertEqual(dict(profile.inclusive), dict(caller=29, callee=15, tail=10))
        self.assertEqual(profile.stalls['tail'], 63)
        self.assertEqual(sum(profile.paths.values()), 29)
        self.assertEqual(dict(profile.calls), dict(caller=1, callee=1, tail=1))
        aggregate = Aggregate(); aggregate.add(cpu, profile)
        report = aggregate.report(1000000)
        self.assertEqual(report['cpu_cycles'], 29)
        self.assertEqual(report['sid_stall_cycles'], 63)
        self.assertEqual(report['median_tick_ms_model'], 92 / 64000)
        self.assertEqual(report['p95_tick_ms_model'], report['max_tick_ms_model'])

    def test_continuity_allows_queue_injection_but_not_state_or_register_rewrites(self):
        after, regs = bytes(range(20)), bytes(128)
        before = bytearray(after); before[8:12] = b'\xff' * 4
        check = KERNEL['check_continuity']
        check(after, regs, before, regs, 8, 12)
        before[7] ^= 1
        with self.assertRaises(KernelError):
            check(after, regs, before, regs, 8, 12)
        before[7] ^= 1
        with self.assertRaises(KernelError):
            check(after, regs, before, b'\x01' + regs[1:], 8, 12)

    def test_index_rejects_gaps_overlaps_wrong_cadence_and_short_sustained_runs(self):
        valid = dict(format='sfx-fixture-index-v1', tick_hz=50,
                     groups=[dict(name='busy', start=0, count=1500, continuous=True)])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'index.json'
            path.write_text(json.dumps(valid))
            self.assertEqual(KERNEL['fixture_index'](path, 1500), valid)
            for field, value in (('start', 1), ('start', -1), ('count', 1499), ('count', 1501)):
                invalid = json.loads(json.dumps(valid)); invalid['groups'][0][field] = value
                path.write_text(json.dumps(invalid))
                with self.assertRaises(KernelError):
                    KERNEL['fixture_index'](path, 1500)
            invalid = json.loads(json.dumps(valid)); invalid['tick_hz'] = 60
            path.write_text(json.dumps(invalid))
            with self.assertRaises(KernelError):
                KERNEL['fixture_index'](path, 1500)
            invalid = json.loads(json.dumps(valid)); invalid['groups'] *= 2
            path.write_text(json.dumps(invalid))
            with self.assertRaises(KernelError):
                KERNEL['fixture_index'](path, 3000)


if __name__ == '__main__':
    unittest.main()
