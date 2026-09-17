"""Offline admission/statistics/source-identity checks; no product timing."""
from pathlib import Path
import hashlib
import io
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile

from analysis import summarize
import host
from build import run_build
from source import extract_archive, validate_archive_path, verify_binary_identity

ROOT = Path(__file__).resolve().parent


class ProtocolTests(unittest.TestCase):
    def test_actual_workflow_pipelines_propagate_upstream_failure(self):
        workflow = ROOT.parents[1]/'.github/workflows/feed-handoff-performance.yml'
        text = workflow.read_text()
        self.assertIn('defaults:\n  run:\n    shell: bash\n', text)
        # Extract the actual Python pipelines, including multiline arguments.
        pipelines = []
        pending = []
        for line in text.splitlines():
            command = line.strip().removeprefix('run: ')
            if command.startswith('python3 ') and ('| tee' in command or command.endswith('\\')):
                pending = [command]
            elif pending:
                pending.append(command)
            if pending and not command.endswith('\\'):
                pipelines.append('\n'.join(pending))
                pending = []
        self.assertEqual(len(pipelines), 4)
        bash = shutil.which('bash') if os.name != 'nt' else str(Path(os.environ['ProgramFiles'])/'Git/bin/bash.exe')
        with tempfile.TemporaryDirectory(prefix='quantick-f2-pipefail-') as temporary:
            env = dict(os.environ, F2_RECEIPTS=Path(temporary).as_posix(), F2_EXPORTS='fixture',
                       F2_ATTEMPT='fixture', F2_ATTEMPT_LEASE='offline', CONTROL='fixture', CANDIDATE='fixture')
            for pipeline in pipelines:
                # A failed fake upstream exercises each real tee pipeline;
                # neither a product binary nor a compiler is invoked.
                command = 'python3() { return 7; };\n' + pipeline
                failed = subprocess.run([bash, '--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', command], env=env, capture_output=True)
                self.assertEqual(failed.returncode, 7, pipeline)
                masked = subprocess.run([bash, '--noprofile', '--norc', '-e', '-c', command], env=env, capture_output=True)
                self.assertEqual(masked.returncode, 0, pipeline)

    def test_actual_failed_build_command_retains_source_and_terminal_receipt(self):
        with tempfile.TemporaryDirectory(prefix='quantick-f2-build-') as temporary:
            root = Path(temporary)
            record = {'source_commit': 'a'*40, 'command': [sys.executable, '-c', 'print("failed fixture"); raise SystemExit(7)']}
            run_build(record, root, os.environ.copy(), root/'stdout.log', root/'stderr.log',
                      root/'receipt.json', inspect=lambda: [])
            retained = json.loads((root/'receipt.json').read_text())
            self.assertEqual(retained['source_commit'], 'a'*40)
            self.assertEqual(retained['exit'], 7)
            self.assertIn('failed fixture', (root/'stdout.log').read_text())

    def test_real_git_export_is_immutable_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory(prefix='quantick-f2-export-') as temporary:
            root = Path(temporary)
            repo = root/'repo'
            repo.mkdir()
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], stderr=subprocess.STDOUT)
            git('init', '-q')
            (repo/'input.txt').write_text('committed input')
            git('add', 'input.txt')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture')
            commit = git('rev-parse', 'HEAD').decode().strip()
            archive = git('archive', '--format=zip', commit)
            (repo/'input.txt').write_text('dirty source must never be exported')
            extract_archive(archive, root/'export')
            self.assertEqual((root/'export/input.txt').read_text(), 'committed input')
            with self.assertRaises(FileExistsError):
                extract_archive(archive, root/'export')

    def test_archive_link_and_traversal_fail_before_writing(self):
        for linked in [False, True]:
            with self.subTest(linked=linked), tempfile.TemporaryDirectory(prefix='quantick-f2-archive-') as temporary:
                buffer = io.BytesIO()
                with zipfile.ZipFile(buffer, 'w') as stream:
                    stream.writestr('safe.txt', 'safe')
                    entry = zipfile.ZipInfo('link' if linked else '../escape')
                    if linked:
                        entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                    stream.writestr(entry, '../outside')
                destination = Path(temporary)/'export'
                with self.assertRaises(ValueError):
                    extract_archive(buffer.getvalue(), destination)
                self.assertFalse(destination.exists())

    def test_archive_cannot_escape_on_either_supported_platform(self):
        validate_archive_path('crates/feed/src/lib.rs')
        for name in ['../escape', '/absolute', 'C:/drive', 'folder\\escape', 'a/../../escape']:
            with self.subTest(name=name), self.assertRaises(ValueError):
                validate_archive_path(name)

    def test_build_and_binary_identity_mismatch_fail_closed(self):
        record = {'profile': 'release', 'jobs': 1, 'rustc': 'pinned', 'cargo': 'pinned',
                  'binary_sha256': 'abc', 'command': ['cargo', 'test', '--no-run'], 'exit': 0}
        build = {'control': dict(record), 'candidate': dict(record)}
        verify_binary_identity(build, {'control': 'a', 'candidate': 'b'}, lambda _: 'abc')
        build['candidate']['jobs'] = 2
        with self.assertRaisesRegex(ValueError, 'jobs'):
            verify_binary_identity(build, {'control': 'a', 'candidate': 'b'}, lambda _: 'abc')
        build['candidate']['jobs'] = 1
        with self.assertRaisesRegex(ValueError, 'hash'):
            verify_binary_identity(build, {'control': 'a', 'candidate': 'b'}, lambda _: 'changed')

    def test_literal_corpus_is_the_preserved_input(self):
        self.assertEqual(hashlib.sha256((ROOT/'fixture.json').read_bytes()).hexdigest(),
                         'b7e798913b7ddd7269949c60c4035da39f3d212b0656ebf0bcd76878dd268d65')

    def records(self, control, candidate):
        return [{'case': case, 'phase': 'measured', 'side': side,
                 'sample': {'elapsed_ns': value}}
                for case in ['binance', 'dense', 'exclusion']
                for side, values in [('control', control), ('candidate', candidate)]
                for value in values]

    def test_fixed_limits_and_no_outlier_discard(self):
        passing = summarize(self.records([100]*15, [105]*15))['binance']
        self.assertEqual(passing['verdict'], 'PASS')
        self.assertEqual(passing['median_label'], 'flat/noise')
        failing = summarize(self.records([100]*15, [106]*15))['dense']
        self.assertEqual(failing['verdict'], 'FAIL')
        tail = summarize(self.records([100]*15, [100]*14+[111]))['exclusion']
        self.assertEqual(tail['stats']['candidate']['p95_ns'], 111)
        self.assertEqual(tail['verdict'], 'FAIL')
        noisy = summarize(self.records([100]*15, [100]*14+[200]))['binance']
        self.assertEqual(noisy['verdict'], 'INVALID_NOISE')

    def test_warmups_are_retained_but_not_used_and_missing_samples_refused(self):
        records = self.records([100]*15, [100]*15)
        records.append({'case': 'binance', 'phase': 'warmup', 'side': 'candidate',
                        'sample': {'elapsed_ns': 999999}})
        self.assertEqual(summarize(records)['binance']['verdict'], 'PASS')
        with self.assertRaises(AssertionError):
            summarize(self.records([100]*14, [100]*15))

    def test_compiler_descendant_activity_and_pid_reuse(self):
        old = {'pid': 4, 'ppid': 1, 'start': 10, 'name': 'helper', 'ticks': 100}
        active = dict(old, ticks=101)
        found = host.blockers([old], [active], 3, 100, {(4, 10)})
        self.assertEqual(found[0]['reason'], 'active_build_descendant')
        self.assertEqual(host.blockers([old], [old], 3, 100, {(4, 10)}), [])
        reused = dict(old, start=11, ticks=0)
        self.assertEqual(host.blockers([old], [reused], 3, 100, {(4, 10)}), [])
        compiler = dict(old, name='rustc')
        self.assertEqual(host.blockers([compiler], [compiler], 3, 100, set())[0]['reason'],
                         'compiler_or_linker_present')

    def test_descendant_chain_is_creation_bound(self):
        items = [{'pid': 1, 'ppid': 0, 'start': 5}, {'pid': 2, 'ppid': 1, 'start': 6},
                 {'pid': 3, 'ppid': 2, 'start': 7}]
        self.assertEqual(host.descendants(items, {(1, 5)}), {(1, 5), (2, 6), (3, 7)})
        self.assertEqual(host.descendants(items, {(1, 99)}), set())

    def test_dirty_candidate_refused_before_export(self):
        with tempfile.TemporaryDirectory(prefix='quantick-f2-tooling-') as temporary:
            root = Path(temporary)
            repo = root/'repo'
            repo.mkdir()
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], stderr=subprocess.STDOUT).decode().strip()
            git('init', '-q')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '--allow-empty', '-qm', 'fixture')
            (repo/'dirty.txt').write_text('dirty')
            result = subprocess.run([sys.executable, str(ROOT/'prepare.py'), '--repo', str(repo),
                                     '--output', str(root/'exports'), '--candidate-commit', git('rev-parse', 'HEAD')],
                                    capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'must be clean', result.stderr)
            receipt = json.loads((root/'exports/prepare-receipt.json').read_text())
            self.assertEqual(receipt['exit'], 1)
            self.assertEqual(receipt['candidate'], git('rev-parse', 'HEAD'))
            self.assertFalse((root/'exports/control').exists())

    def test_admission_and_execution_failures_leave_receipts_without_retry(self):
        for blocked in [True, False]:
            with self.subTest(blocked=blocked), tempfile.TemporaryDirectory(prefix='quantick-f2-retention-') as temporary:
                root = Path(temporary)
                for name in ['run_pairs.py', 'analysis.py', 'source.py']:
                    shutil.copyfile(ROOT/name, root/name)
                (root/'host.py').write_text(
                    'def system_identity(): return {}\n'
                    f'def observe(known): return {{"blockers": {repr(["fixture busy"] if blocked else [])}}}\n')
                binary = Path(sys.executable)
                record = {'profile': 'release', 'jobs': 1, 'rustc': 'fixture', 'cargo': 'fixture',
                          'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                          'command': ['fixture'], 'exit': 0, 'observed_build_descendants': []}
                (root/'build.json').write_text(json.dumps({'control': record, 'candidate': record}))
                (root/'source.json').write_text('{}')
                out = root/'attempt'
                result = subprocess.run([sys.executable, str(root/'run_pairs.py'), '--lease', 'offline-fixture',
                                         '--control', str(binary), '--candidate', str(binary), '--output', str(out),
                                         '--build-identity', str(root/'build.json'), '--source-manifest', str(root/'source.json')],
                                        capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue((out/'identity.json').exists())
                self.assertTrue((out/'process-before.json').exists())
                if blocked:
                    self.assertTrue((out/'stopped-host.json').exists())
                    self.assertFalse((out/'samples.jsonl').exists())
                else:
                    records = (out/'samples.jsonl').read_text().splitlines()
                    self.assertEqual(len(records), 1)
                    self.assertNotEqual(json.loads(records[0])['exit'], 0)
                    self.assertTrue((out/'binance-warmup-01-control.log').exists())


if __name__ == '__main__':
    unittest.main()
