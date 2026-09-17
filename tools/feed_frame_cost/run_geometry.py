"""Run the separately named untimed renderer proof; never invoke sample tests."""
from pathlib import Path
import argparse
import json
from lifecycle import Stage, decision_url, environment, linux_only, run_owned, secondary, sha
from source import verify_binary_identity
from verify import verify_exports
from run_pairs import geometry


def records(text):
    if 'F2_FRAME_SAMPLE ' in text:
        raise ValueError('Untimed stage unexpectedly emitted a timing sample')
    return [json.loads(line[len('F2_FRAME_GEOMETRY '):]) for line in text.splitlines()
            if line.startswith('F2_FRAME_GEOMETRY ')]


def validate(rows, side):
    expected = {(mode, width, height)
                for mode in (['whole'] if side == 'control' else ['whole', 'panel_actual', 'panel_reference'])
                for width, height in [(1440.0, 900.0), (1000.0, 700.0)]}
    actual = {(row['mode'], *row['size']) for row in rows}
    if actual != expected or len(rows) != len(expected):
        raise ValueError('Incomplete or duplicate untimed geometry cases')
    for row in rows:
        if row['scale'] != 1.5:
            raise ValueError('Unexpected geometry scale')
        geometry(row)
        if (row['panel_geometry'] is None) != (side == 'control'):
            raise ValueError('Unexpected panel presence')
    if side == 'candidate':
        for size in [[1440.0, 900.0], [1000.0, 700.0]]:
            pair = [row for row in rows if row['size'] == size and row['mode'] != 'whole']
            if any(pair[0][key] != pair[1][key] for key in ['panel_geometry', 'delivery_text']):
                raise ValueError('Untimed focused paint differs')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exports', type=Path, required=True)
    parser.add_argument('--build', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--decision', required=True)
    args = parser.parse_args()
    root, out = args.exports.resolve(), args.output.resolve()
    if out.exists():
        raise ValueError('Refuse existing proof output')
    with Stage(out, 'geometry', decision=args.decision, performance_samples=0) as stage:
        decision_url(args.decision)
        linux_only()
        verify_exports(root)
        digest = sha(root / 'source-manifest.json')
        build = json.loads((args.build / 'build-identity.json').read_text(encoding='utf-8'))
        bins = {side: Path(build[side]['binary']).resolve() for side in ['control', 'candidate']}
        verify_binary_identity(build, bins, sha)
        if any(build[side]['manifest_sha256'] != digest for side in bins):
            raise ValueError('Compile/export manifest mismatch')
        stage.record.update(manifest_sha256=digest, build_identity_sha256=sha(args.build / 'build-identity.json'))
        try:
            for side in ['control', 'candidate']:
                with Stage(out, side + '-geometry-protocol', binary_sha256=sha(bins[side])) as protocol:
                    env = environment(out / (side + '-environment'), mode='whole', size='normal')
                    command = [str(bins[side]), 'app::tests::f2_frame_protocol_tests::f2_frame_geometry',
                               '--exact', '--nocapture', '--test-threads=1']
                    run_owned(command, out, env, out, side + '-geometry', timeout=360)
                    text = (out / (side + '-geometry.stdout')).read_text(encoding='utf-8')
                    rows = records(text)
                    protocol.record['observations'] = rows
                    validate(rows, side)
        finally:
            secondary(stage, lambda: verify_exports(root), 'source-after-geometry')
            secondary(stage, lambda: verify_binary_identity(build, bins, sha), 'binary-after-geometry')


if __name__ == '__main__':
    main()
