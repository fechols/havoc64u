"""Measure production shadow frames and compare ordinary geometry separately."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('bench', ROOT/'tools/bench-native.py')
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)
vice = bench.vice

# All enabled levels and transitions use C64 dark gray.
SHADOW_COLOR = 11

parser = argparse.ArgumentParser()
parser.add_argument('--package-dir', type=Path, required=True)
parser.add_argument('--out-dir', type=Path, required=True)
parser.add_argument('--video', choices=['pal','ntsc'], required=True)
parser.add_argument('--fixtures', default='all')
parser.add_argument('--phase-breakdown', action='store_true')
args = parser.parse_args()

all_fixtures = bench.fixtures(campaign=True, space=True, include_victory=True)
selected = {
    'lowlands', 'full-pools', 'missiles', 'enemies-lowlands', 'enemies-crystal',
    'enemies-canyon', 'enemies-foundry', 'enemies-orbital-shipyard',
    'enemies-space-carrier', 'boss-gorge-warden', 'boss-citadel-heart',
    'boss-dreadnought', 'boss-gate-guardian', 'hazards-ship-graveyard',
    'bridge-0-200', 'bridge-10-306', 'bridge-11-200', 'bridge-14-200', 'bridge-16-200',
}
FIXTURES = [f for f in all_fixtures if f['name'] in selected]
base = next(f for f in FIXTURES if f['name'] == 'lowlands')
for name, values in [
    ('player-bank-left', {'flight.bank': -8192}),
    ('player-bank-right', {'flight.bank': 8192}),
    ('player-inverted', {'roll_ticks': 12, 'roll_direction': 1}),
    ('player-climb', {'flight.pitch': 8192}),
    ('player-dive', {'flight.pitch': -8192}),
    ('near-plane-fighter', {'actors[0].kind': 1, 'actors[0].hp': 6,
        'actors[0].pos.z': 20, 'actors[0].pos.x': 0, 'actors[0].pos.y': 0}),
]:
    FIXTURES.append(dict(base, name=name, patch=dict(base['patch'], invulnerability=0, **values)))
if args.fixtures != 'all':
    available = {f['name']: f for f in all_fixtures + FIXTURES}
    FIXTURES = [available[name] for name in args.fixtures.split(',')]
assert FIXTURES

# Preserve the live command/sidecar state when a native guard fails before
# the first published frame; it makes overlay-lifetime failures reviewable.
_run_until = vice.Monitor.run_until
def diagnostic_run_until(mon, target, failures):
    try:
        return _run_until(mon, target, failures)
    except Exception:
        args.out_dir.mkdir(parents=True, exist_ok=True)
        mon.command('bank ram')
        for name, start, size in [('commands',0xcffa,4102),('shadow-frame',0xe800,1024),
                                  ('order',0x7600,256)]:
            (args.out_dir/f'failure-{name}.bin').write_bytes(mon.read(start,size))
        mon.command('bank cpu')
        raise
vice.Monitor.run_until = diagnostic_run_until


def checks(mon, syms, failures, elf, dwarf_tool, output, result):
    layouts = bench.Layouts(elf, dwarf_tool, output)
    game_address = vice.resident_symbol(syms,'GAME')[1]
    renderer_address = vice.resident_symbol(syms,'RENDERER')[1]
    audio_address = vice.resident_symbol(syms,'EFFECTS')[1]
    pause = vice.resident_symbol(syms,'PAUSED')[1]
    fps = vice.resident_symbol(syms,'FPS_SHOWN')[1]
    package = (args.package_dir/'havoc.reu').read_bytes()
    period = 19705 if args.video == 'pal' else 20455
    phase_stops = []
    if args.phase_breakdown:
        spec = importlib.util.spec_from_file_location('shadow_overlay_audit', ROOT/'tools/check-overlays.py')
        audit = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = audit
        spec.loader.exec_module(audit)
        native = audit.Elf(elf)
        for source, address, kind, symbol, addend in native.relocations():
            callee = native.function_at(symbol.section, symbol.address + addend)
            name = callee.name if callee else audit.readable_name(symbol.name)
            if source.name == '.text' and kind == 3 and name in (
                    'havoc_c64::prepare_geometry', 'havoc_c64::prepare_shadows'):
                phase_stops.append((address - 1, name.rsplit('::', 1)[1]))
        phase_stops.sort()
        assert [name for _, name in phase_stops] == ['prepare_geometry', 'prepare_shadows'], phase_stops

    def write(address, raw):
        for offset in range(0,len(raw),16):
            mon.command(f'> {address+offset:04x} ' + ' '.join(f'{v:02x}' for v in raw[offset:offset+16]))
        assert mon.read(address,len(raw)) == raw

    def word(address,value,size=2):
        write(address,int(value).to_bytes(size,'little'))

    mon.command('bank ram')
    initial = bytearray(mon.read(game_address,layouts.size('Game')))
    initial_audio = mon.read(audio_address,layouts.size('EffectsEngine'))
    for field in ['actors','shots','pickups','explosions','flight','controls']:
        offset,kind = layouts.member('Game',field)
        initial[offset:offset+layouts.size(kind)] = bytes(layouts.size(kind))
    word(pause,1,1)
    word(fps,0,1)
    mon.command('bank cpu')
    mon.command('jpdb 1 $0f')

    def geometry():
        mon.command('bank ram')
        count_offset,_ = layouts.member('Renderer','triangle_count')
        count = mon.integer(renderer_address+count_offset)
        offset,_ = layouts.member('Renderer','triangles')
        size = layouts.size('Triangle')
        order = list(mon.read(syms['__sort_order'],count))
        assert sorted(order)==list(range(count))
        commands = [layouts.decode('Triangle',mon.read(renderer_address+offset+n*size,size)) for n in order]
        clear_offset,_ = layouts.member('Renderer','clear_color')
        clear = mon.integer(renderer_address+clear_offset,1)
        shadow_counts = {}
        for field in ('base_count', 'command_count', 'caster_count', 'rendered', 'omitted'):
            member, kind = layouts.member('ShadowFrame', field)
            shadow_counts[field] = mon.integer(0xe800 + member, layouts.size(kind))
        mon.command('bank cpu')
        return commands,clear,shadow_counts

    def seed(fixture):
        stage = fixture['section']
        values = dict(fixture['patch'])
        values.update((f'stage_info.{field}',value) for field,value in bench.stage_metadata(package,stage).items())
        values = vice.clock_fixture(values, layouts.load('Game')['fields'],18)
        game = bytearray(initial)
        for field,value in values.items(): layouts.patch(game,'Game',field,value)
        mon.command('bank ram')
        write(game_address,game)
        write(audio_address,initial_audio)
        word(syms['FRAME_SERIAL'],32)
        word(syms['TEST_INPUT'],16,1)
        for symbol,size in [('INPUT_OVERFLOWS',2),('AUDIO_TICKS',2)]: word(syms[symbol],0,size)
        for symbol in ['INPUT_HEAD','INPUT_TAIL']:
            word(vice.resident_symbol(syms,symbol)[1],0,1)
        word(vice.resident_symbol(syms,'LAST_BITS')[1],16,1)
        mon.command('bank cpu')

    result.update(passed=False,scope='Untouched production package; paused seeded scenes, full render frames including live audio and publication; simulation timing excluded.', fixtures=FIXTURES,cases=[])
    for fixture in FIXTURES:
        name,stage = fixture['name'],fixture['section']
        for palette in ['cold','retained']:
            case={'fixture':name,'palette':palette,'samples':[]}
            result['cases'].append(case)
            for phase in range(3):
                folder=output/name/palette/str(phase)
                folder.mkdir(parents=True,exist_ok=True)
                seed(fixture)
                mon.run_until(syms['frame_complete'],failures)
                seed(fixture)
                if palette=='cold':
                    mon.command('bank ram')
                    write(0x8000,bytes(1000))
                    mon.command('bank io')
                    mon.command('fill d800 dbe7 00')
                    mon.command('bank cpu')
                mon.command('bank ram')
                old_screen=mon.read(0x8000,1000)
                mon.command('bank io')
                old_color=bytes(v&15 for v in mon.read(0xd800,1000))
                mon.command('bank cpu')
                (folder/'old-attributes.bin').write_bytes(old_screen+old_color)
                first_period=(period,period*2//3,period//3)[phase]
                mon.command('> dc0e 00')
                mon.command(f'> dc04 {(first_period-1)&255:02x} {(first_period-1)>>8:02x}')
                mon.command('> dc0e 11')
                mon.command('sidefx on');mon.read(0xdc0d,1);mon.command('sidefx off')
                start=vice.counters(mon,syms)
                mon.command('step $40')
                mon.command(f'> dc04 {(period-1)&255:02x} {(period-1)>>8:02x}')
                phase_cycles = {}
                for index, (address, phase_name) in enumerate(phase_stops):
                    mon.run_until(address, failures)
                    phase_cycles[f'{index}_{phase_name}'] = vice.counters(mon, syms)['bus_cycles'] - start['bus_cycles']
                mon.run_until(syms['geometry_complete'],failures)
                commands,clear,shadow_counts=geometry()
                geometry_end=vice.counters(mon,syms)
                mon.run_until(syms['frame_complete'],failures)
                end=vice.counters(mon,syms)
                mon.command('bank ram')
                game=layouts.decode('Game',mon.read(game_address,layouts.size('Game')))
                guards={name:mon.integer(syms[name],1) for name in ['__stack_bottom','__irq_stack_bottom']}
                mon.command('bank cpu')
                assert game['ticks']==fixture['patch']['ticks'] and all(v==165 for v in guards.values())
                violations = {key: end.get(key, 0) for key in ['frame_dropped','input_overflows','music_underruns'] if end.get(key, 0)}
                assert not violations, (name, palette, phase, violations, shadow_counts, phase_cycles, end)
                shot=vice.capture(mon,syms,folder,'frame')
                (folder/'geometry.json').write_text(json.dumps(commands,indent=2)+'\n')
                (folder/'game.json').write_text(json.dumps(game,indent=2)+'\n')
                base_count = shadow_counts['base_count']
                assert 0 <= base_count <= len(commands)
                base, shadows = commands[:base_count], commands[base_count:]
                assert len(shadows) == shadow_counts['command_count']
                for command in commands:
                    assert not command['reserved'] & ~0x62
                shadow_color = SHADOW_COLOR
                assert all(c['color'] == shadow_color and c['reserved'] == 0x40 for c in shadows)
                sample={'cia_phase':phase,'cycles':end['bus_cycles']-start['bus_cycles'],'geometry_cycles':geometry_end['bus_cycles']-start['bus_cycles'],'triangle_count':len(commands),'clear_color':clear,
                        'phase_cycles':phase_cycles,
                        'shadow_counts':shadow_counts,'shadow_color':shadow_color,
                        'base_triangle_count':len(base),'shadow_triangle_count':len(shadows),
                        'base_geometry_sha256':bench.semantic_hash(base),
                        'geometry_sha256':bench.semantic_hash(commands),'raw_geometry_sha256':bench.semantic_hash(commands),
                        'capture':shot,'game':vice.logical_game_state(game),'counters':end,'guards':guards}
                case['samples'].append(sample)
            case['median_cycles']=statistics.median(s['cycles'] for s in case['samples'])
            case['median_geometry_cycles']=statistics.median(s['geometry_cycles'] for s in case['samples'])
            case['triangle_count']=case['samples'][0]['triangle_count']
            print(f'{args.video} {name} {palette}: {case["median_cycles"]} cycles, {case["triangle_count"]} triangles',flush=True)
    result['passed']=True

vice.hud_checks=checks
sys.argv=[str(ROOT/'tools/vice-smoke.py'),'--package-dir',str(args.package_dir),'--out-dir',str(args.out_dir),
          '--video',args.video,'--frames','8','--hud-checks']
raise SystemExit(vice.main())
