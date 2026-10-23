"""Compare matched production scenes, including exact ordinary command preservation."""
import argparse
import json
from pathlib import Path
import statistics


def ordinary(commands, sample):
    """Current captures have a suffix; archived shadows carry a parent marker."""
    counts = sample.get('shadow_counts', {})
    if 'base_count' in counts:
        count = counts['base_count']
        assert 0 <= count <= len(commands)
        assert count + counts['command_count'] == len(commands)
        return commands[:count]
    return [command for command in commands if not command['reserved'] & 0x80]


def compare(before, after, shadow_colors_only=False):
    old = json.loads((before / 'report.json').read_text())
    new = json.loads((after / 'report.json').read_text())
    assert old['passed'] and new['passed'] and old['video'] == new['video']
    previous = {(c['fixture'], c['palette']): c for c in old['hud_checks']['cases']}
    cases = []
    for current in new['hud_checks']['cases']:
        fixture, palette = current['fixture'], current['palette']
        baseline = previous[fixture, palette]
        old_samples = {sample['cia_phase']: sample for sample in baseline['samples']}
        for sample in current['samples']:
            relative = Path(fixture) / palette / str(sample['cia_phase']) / 'geometry.json'
            expected = json.loads((before / relative).read_text())
            actual = json.loads((after / relative).read_text())
            assert ordinary(actual, sample) == ordinary(expected, old_samples[sample['cia_phase']]), f'ordinary geometry changed: {relative}'
            if shadow_colors_only:
                previous_sample = old_samples[sample['cia_phase']]
                old_count = previous_sample['shadow_counts']['base_count']
                new_count = sample['shadow_counts']['base_count']
                old_shapes = [{key: value for key, value in command.items() if key != 'color'}
                              for command in expected[old_count:]]
                new_shapes = [{key: value for key, value in command.items() if key != 'color'}
                              for command in actual[new_count:]]
                assert new_shapes == old_shapes, f'shadow geometry changed: {relative}'
            assert sample['counters']['frame_dropped'] == 0
        initial, final = baseline['median_cycles'], current['median_cycles']
        cases.append(dict(fixture=fixture, palette=palette, before_cycles=initial,
                          after_cycles=final, delta_cycles=final-initial,
                          change_percent=100*(final/initial-1),
                          base_commands=current['samples'][0]['base_triangle_count'],
                          shadow_commands=current['samples'][0]['shadow_triangle_count']))
    total_before = sum(case['before_cycles'] for case in cases)
    total_after = sum(case['after_cycles'] for case in cases)
    return dict(passed=True, video=new['video'], case_count=len(cases),
                samples_checked=sum(len(c['samples']) for c in new['hud_checks']['cases']),
                ordinary_geometry_identical=True, shadow_geometry_identical=shadow_colors_only,
                dropped_commands=0,
                summed_median_before_cycles=total_before, summed_median_after_cycles=total_after,
                summed_change_percent=100*(total_after/total_before-1),
                median_case_change_percent=statistics.median(c['change_percent'] for c in cases),
                minimum_case_change_percent=min(c['change_percent'] for c in cases),
                maximum_case_change_percent=max(c['change_percent'] for c in cases), cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--no-shadow-baseline', type=Path,
                        help='Check flat-shadow performance targets; BEFORE must be the complex-shadow build')
    parser.add_argument('--enforce-targets', action='store_true')
    parser.add_argument('--shadow-colors-only', action='store_true',
                        help='Also require identical flat-shadow commands apart from their ink')
    args = parser.parse_args()
    if args.enforce_targets and args.no_shadow_baseline is None:
        parser.error('--enforce-targets requires --no-shadow-baseline')
    report = compare(args.before, args.after, args.shadow_colors_only)
    if args.no_shadow_baseline is not None:
        clear = compare(args.no_shadow_baseline, args.after)
        current = json.loads((args.after / 'report.json').read_text())
        disabled = {(case['fixture'], case['palette']) for case in current['hud_checks']['cases']
                    if all(sample['shadow_counts']['caster_count'] == 0 for sample in case['samples'])}
        old_added = report['summed_median_before_cycles'] - clear['summed_median_before_cycles']
        new_added = report['summed_median_after_cycles'] - clear['summed_median_before_cycles']
        assert old_added > 0, 'complex-shadow baseline must have measurable overhead'
        removed = 100 * (old_added - new_added) / old_added
        disabled_overhead = max((case['change_percent'] for case in clear['cases']
                                if (case['fixture'], case['palette']) in disabled), default=0)
        regression = report['maximum_case_change_percent']
        report['performance_targets'] = dict(
            passed=removed >= 75 and disabled_overhead <= 5 and regression <= 3,
            added_shadow_cost_removed_percent=removed,
            added_frame_cost_percent=clear['summed_change_percent'],
            disabled_case_count=len(disabled),
            maximum_disabled_overhead_percent=disabled_overhead,
            maximum_case_change_percent=regression,
            minimum_cost_removal_percent=75, maximum_disabled_overhead_target_percent=5,
            maximum_regression_target_percent=3)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(f"PASS {report['samples_checked']} matched {report['video'].upper()} samples: "
          f"ordinary commands identical, zero drops; "
          f"summed median frame cycles {report['summed_change_percent']:+.1f}%")
    if 'performance_targets' in report:
        print(json.dumps(report['performance_targets'], indent=2))
    if args.enforce_targets and not report['performance_targets']['passed']:
        raise SystemExit('Flat-shadow performance targets were not met')


if __name__ == '__main__':
    main()
