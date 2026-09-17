"""Linux-only sequential compile preparation; does not authorize sampling."""
from pathlib import Path
import argparse
import json
import sys
import host
from lifecycle import Stage, decision_url, environment, linux_only, run_owned, secondary, sha, write
from verify import verify_exports


def artifact(log):
    entries = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines() if line.startswith('{')]
    binaries = [Path(row['executable']) for row in entries if row.get('reason') == 'compiler-artifact'
                and row.get('target', {}).get('name') == 'quantick-app'
                and row.get('profile', {}).get('test') and row.get('executable')]
    if len(binaries) != 1:
        raise ValueError('Expected exactly one app unit-test binary')
    return binaries[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exports', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--rustup-home', required=True, type=Path)
    parser.add_argument('--compile-decision', required=True)
    parser.add_argument('--manifest-sha256', required=True)
    args = parser.parse_args()
    root, out = args.exports.resolve(), args.output.resolve()
    if out.exists():
        raise ValueError('Refuse existing compile output')
    with Stage(out, 'build', decision=args.compile_decision) as stage:
        decision_url(args.compile_decision)
        linux_only()
        if sha(root / 'source-manifest.json') != args.manifest_sha256:
            raise ValueError('Decision-bound manifest mismatch')
        manifest = verify_exports(root)
        digest = sha(root / 'source-manifest.json')
        identities = {}
        try:
            proof_env = environment(out / 'linux-proof-environment', build=True,
                                    rustup_home=args.rustup_home, target=out / 'linux-proof-unused-target')
            run_owned([sys.executable, '-m', 'unittest', 'test_linux_cleanup', '-v'],
                      Path(__file__).resolve().parent, proof_env, out, 'linux-cleanup-proof', timeout=60)
            for side in ['control', 'candidate']:
                with Stage(out, side + '-protocol') as protocol:
                    verify_exports(root)
                    env = environment(out / (side + '-environment'), build=True,
                                      rustup_home=args.rustup_home, target=out / (side + '-target'))
                    versions = {}
                    for tool, option in [('rustc', '-Vv'), ('cargo', '-V')]:
                        tag = side + '-' + tool
                        run_owned([tool, option], root / side, env, out, tag, timeout=60, inspect=host.inventory)
                        versions[tool] = (out / (tag + '.stdout')).read_text(encoding='utf-8')
                    command = ['cargo', 'test', '-p', 'quantick-app', '--bin', 'quantick-app', '--release',
                               '--locked', '--no-run', '--message-format=json']
                    record = run_owned(command, root / side, env, out, side + '-compile',
                                       timeout=2400, inspect=host.inventory)
                    binary = artifact(out / (side + '-compile.stdout')).resolve()
                    record.update(source_commit=manifest['sides'][side]['commit'], profile='release', jobs=1,
                                  manifest_sha256=digest, binary=str(binary), binary_sha256=sha(binary), **versions)
                    identities[side] = record
                    protocol.record['identity'] = record
                    verify_exports(root)
            write(out / 'build-identity.json', identities)
        finally:
            secondary(stage, lambda: verify_exports(root), 'full-source-after-build')


if __name__ == '__main__':
    main()
