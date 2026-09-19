"""Build both immutable instrumented exports sequentially; never sample."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import subprocess
import time

import host


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def run_build(record, source, environment, log, stderr, receipt, inspect=host.inventory):
    """Persist command/source identity before launch and every terminal result."""
    receipt.write_text(json.dumps(record, indent=2) + '\n')
    observed = set()
    try:
        with log.open('wb') as stdout, stderr.open('wb') as errors:
            process = subprocess.Popen(record['command'], cwd=source, env=environment,
                                       stdout=stdout, stderr=errors)
            while process.poll() is None:
                items = inspect()
                roots = {(p['pid'], p['start']) for p in items if p['pid'] == process.pid}
                observed |= host.descendants(items, roots | observed)
                time.sleep(0.25)
            record['exit'] = process.returncode
    except Exception as error:
        record.update(exit=None, error=str(error), status='failed before terminal build receipt')
        raise
    finally:
        record.update(finish_utc=now(), observed_build_descendants=sorted(observed),
                      descendant_observation='Creation-bound /proc ancestry sampled every250ms during build; brief unobserved children remain a disclosed limitation.')
        record.update(stdout_sha256=hashlib.sha256(log.read_bytes()).hexdigest() if log.exists() else None,
                      stderr_sha256=hashlib.sha256(stderr.read_bytes()).hexdigest() if stderr.exists() else None)
        receipt.write_text(json.dumps(record, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exports', required=True, type=Path)
    args = parser.parse_args()
    root = args.exports.resolve()
    manifest = json.loads((root / 'source-manifest.json').read_text())
    identity = {}
    for side in ['control', 'candidate']:
        source = root / side
        assert (source / 'crates/feed/src/f2_protocol.rs').read_bytes() == (
            root / ('candidate' if side == 'control' else 'control') / 'crates/feed/src/f2_protocol.rs').read_bytes()
        environment = dict(os.environ, CARGO_BUILD_JOBS='1', CARGO_TARGET_DIR=str(root / (side + '-target')))
        command = ['cargo', 'test', '-p', 'quantick-feed', '--lib', '--release', '--locked',
                   '--no-run', '--message-format=json']
        record = {'source_commit': manifest['sides'][side]['commit'], 'profile': 'release',
                  'jobs': 1, 'command': command, 'start_utc': now(),
                  'rustc': subprocess.check_output(['rustc', '-Vv'], cwd=source).decode(),
                  'cargo': subprocess.check_output(['cargo', '-V'], cwd=source).decode()}
        log = root / (side + '-build.jsonl')
        stderr = root / (side + '-build.stderr.log')
        run_build(record, source, environment, log, stderr, root/(side+'-build-receipt.json'))
        identity[side] = record
        (root / 'build-identity.json').write_text(json.dumps(identity, indent=2) + '\n')
        if record['exit']:
            raise SystemExit(record['exit'])
        artifacts = [json.loads(line) for line in log.read_text().splitlines() if line.startswith('{')]
        binaries = [Path(a['executable']) for a in artifacts if a.get('reason') == 'compiler-artifact'
                    and a.get('target', {}).get('name') == 'quantick_feed'
                    and a.get('profile', {}).get('test') and a.get('executable')]
        assert len(binaries) == 1, 'Expected exactly one feed unit-test binary'
        record.update(binary=str(binaries[0]), binary_sha256=hashlib.sha256(binaries[0].read_bytes()).hexdigest())
        (root / 'build-identity.json').write_text(json.dumps(identity, indent=2) + '\n')
    print(json.dumps(identity, indent=2))


if __name__ == '__main__':
    main()
