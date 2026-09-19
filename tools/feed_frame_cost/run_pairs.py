"""Fixed Linux protocol; separate finite sample decision required after compile review."""
from pathlib import Path
import argparse
import json
import math
import statistics
import host
from lifecycle import Stage, decision_url, environment, linux_only, run_owned, secondary, sha, write
from source import verify_binary_identity
from verify import verify_exports


def sample_from(path):
    values = [json.loads(line[len('F2_FRAME_SAMPLE '):]) for line in path.read_text(encoding='utf-8', errors='replace').splitlines()
              if line.startswith('F2_FRAME_SAMPLE ')]
    if len(values) != 1:
        raise ValueError('Expected exactly one sample record')
    return values[0]


def geometry(sample):
    value = sample['panel_geometry']
    if value is None:
        return
    def rect(bounds):
        if len(bounds) != 4 or not all(math.isfinite(v) for v in bounds) or not (bounds[0] < bounds[2] and bounds[1] < bounds[3]):
            raise ValueError('Invalid geometry rectangle')
    def contains(outer, inner):
        return outer[0] <= inner[0] and outer[1] <= inner[1] and outer[2] >= inner[2] and outer[3] >= inner[3]
    panel = value['panel']
    rect(panel)
    if not contains([0, 0, *sample['size']], panel):
        raise ValueError('Panel outside viewport')
    if abs(panel[3] - panel[1] - value['expected_height']) > 1 / sample['scale']:
        raise ValueError('Panel height mismatch')
    rows = value['galleys']
    if len(rows) != 3 or [row['text'] for row in rows] != sample['delivery_text']:
        raise ValueError('Actual galley text/order mismatch')
    for row in rows:
        rect(row['rect'])
        rect(row['clip'])
        if not contains(panel, row['rect']) or not contains(row['clip'], row['rect']):
            raise ValueError('Galley outside panel/clip')


def paired_facts(control, candidate, kind):
    if control['facts'] != candidate['facts']:
        raise ValueError('Actual tape/bar/partial/source facts mismatch')
    if kind == 'panel' and any(control[key] != candidate[key] for key in ['delivery_text', 'panel_geometry']):
        raise ValueError('Focused actual/reference panel text/geometry differs')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exports', required=True, type=Path)
    parser.add_argument('--build', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--execution-decision', required=True)
    args = parser.parse_args()
    root, out = args.exports.resolve(), args.output.resolve()
    if out.exists():
        raise ValueError('Refuse existing sample output')
    records = []
    with Stage(out, 'samples', decision=args.execution_decision) as stage:
        decision_url(args.execution_decision)
        linux_only()
        manifest = verify_exports(root)
        digest = sha(root / 'source-manifest.json')
        build = json.loads((args.build / 'build-identity.json').read_text(encoding='utf-8'))
        bins = {side: Path(build[side]['binary']).resolve() for side in ['control', 'candidate']}
        verify_binary_identity(build, bins, sha)
        if any(build[side]['manifest_sha256'] != digest for side in bins):
            raise ValueError('Compile/export manifest mismatch')
        known = {tuple(value) for side in bins for value in build[side]['observed_build_descendants']}
        stage.record.update(manifest_sha256=digest, build_identity_sha256=sha(args.build / 'build-identity.json'),
                            system=host.system_identity(), environment_policy='strict constructed child environment; names only',
                            protocol=manifest['protocol'])
        def inventory(name):
            result = host.observe(known)
            write(out / name, result)
            if result['blockers']:
                raise RuntimeError('Host refused; no retry')
        try:
            write(out / 'settling.json', host.settle_once())
            inventory('process-before.json')
            for kind in ['whole', 'panel']:
                for size in ['normal', 'narrow']:
                    for phase, count in [('warmup', 3), ('measured', 15)]:
                        for pair in range(1, count + 1):
                            observed = {}
                            for side in (['control', 'candidate'] if pair % 2 else ['candidate', 'control']):
                                mode = 'whole' if kind == 'whole' else 'panel_reference' if side == 'control' else 'panel_actual'
                                binary = bins[side] if kind == 'whole' else bins['candidate']
                                tag = f'{kind}-{size}-{phase}-{pair:02}-{side}'
                                with Stage(out, tag + '-observation', binary_sha256=sha(binary), cell=f'{kind}-{size}',
                                           phase=phase, pair=pair, side=side, mode=mode) as observation:
                                    try:
                                        verify_exports(root)
                                        verify_binary_identity(build, bins, sha)
                                        env = environment(out / (tag + '-environment'), mode=mode, size=size)
                                        command = [str(binary), 'app::tests::f2_frame_protocol_tests::f2_frame_sample',
                                                   '--ignored', '--exact', '--nocapture', '--test-threads=1']
                                        run_owned(command, out, env, out, tag, timeout=360)
                                        sample = sample_from(out / (tag + '.stdout'))
                                        observation.record['sample'] = sample
                                        if not (sample['mode'] == mode and sample['size'] == manifest['protocol']['sizes'][size]
                                                and sample['scale'] == 1.5 and sample['frames'] == 600
                                                and sample['warmup_frames'] == 30 and sample['prints_per_frame'] == 64
                                                and len(sample['samples_ns']) == 600 and all(v > 0 for v in sample['samples_ns'])
                                                and sum(sample['samples_ns']) == sample['elapsed_ns'] and sample['shape_count'] > 0):
                                            raise ValueError('Fixed protocol mismatch')
                                        geometry(sample)
                                        if (kind == 'panel' or side == 'candidate') and sample['panel_geometry'] is None:
                                            raise ValueError('Candidate panel geometry absent')
                                        records.append(dict(observation.record, sample=sample))
                                        observed[side] = sample
                                    finally:
                                        secondary(observation, lambda: verify_exports(root), 'full-source-after-observation')
                                        secondary(observation, lambda: verify_binary_identity(build, bins, sha), 'binary-after-observation')
                            paired_facts(observed['control'], observed['candidate'], kind)
            summary = {}
            for cell in ['whole-normal', 'whole-narrow', 'panel-normal', 'panel-narrow']:
                stats = {}
                for side in bins:
                    values = [r['sample']['elapsed_ns'] for r in records if (r['cell'], r['side'], r['phase']) == (cell, side, 'measured')]
                    if len(values) != 15:
                        raise ValueError('Incomplete fixed sample set')
                    stats[side] = {'median_ns': statistics.median(values), 'p95_ns': sorted(values)[math.ceil(.95 * len(values)) - 1],
                                   'cv': statistics.stdev(values) / statistics.mean(values)}
                med = stats['candidate']['median_ns'] / stats['control']['median_ns']
                p95 = stats['candidate']['p95_ns'] / stats['control']['p95_ns']
                summary[cell] = {'stats': stats, 'median_ratio': med, 'p95_ratio': p95,
                                 'verdict': 'INVALID_NOISE' if max(v['cv'] for v in stats.values()) > .05 else 'PASS' if med <= 1.05 and p95 <= 1.10 else 'FAIL'}
            write(out / 'summary.json', summary)
            if any(value['verdict'] != 'PASS' for value in summary.values()):
                raise RuntimeError('Budget/noise gate failed; no retry')
        finally:
            stage.record['records_retained'] = len(records)
            secondary(stage, lambda: inventory('process-after.json'), 'post-inventory')
            secondary(stage, lambda: verify_exports(root), 'full-source-after-samples')


if __name__ == '__main__':
    main()
