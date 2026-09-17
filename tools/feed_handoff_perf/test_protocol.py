"""Offline admission/statistics/source-identity checks; no product timing."""
from pathlib import Path
import hashlib
import io
import json
import os
import shlex
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
    def test_settling_is_one_fixed_interval_without_admission_polling(self):
        sleeps=[]
        clock=iter([41.0,51.0])
        utc=iter(['start','finish'])
        receipt=host.settle_once(sleep=sleeps.append, monotonic=lambda:next(clock), utc=lambda:next(utc))
        self.assertEqual(sleeps,[10])
        self.assertEqual(receipt,{'seconds_requested':10,'start_utc':'start','finish_utc':'finish','start_monotonic':41.0,'finish_monotonic':51.0})
        self.assertEqual(host.OBSERVATION_SECONDS,3)
        self.assertEqual(host.MATERIAL_CORES,0.10)


    def workflow_step(self, name):
        text = (ROOT.parents[1]/'.github/workflows/feed-handoff-performance.yml').read_text()
        step = text.split('      - name: '+name+'\n', 1)[1].split('      - ', 1)[0]
        script = step.split('        run: |\n', 1)[1]
        return '\n'.join(line[10:] for line in script.splitlines())+'\n'

    def bash(self, script, env):
        binary = shutil.which('bash') if os.name != 'nt' else str(Path(os.environ['ProgramFiles'])/'Git/bin/bash.exe')
        return subprocess.run([binary, '--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', script],
                              env=env, capture_output=True, text=True)

    def test_actual_first_step_exports_receipt_and_subsequent_step_environment(self):
        with tempfile.TemporaryDirectory(prefix='quantick f2 env ') as temporary:
            directory = Path(temporary)
            environment_file = directory/'github env'
            # GitHub provides an empty per-step environment file. Its contents
            # are environment records, not a shell script to source.
            environment_file.touch()
            env = {key: value for key, value in os.environ.items() if not key.startswith('F2_')}
            env.update(RUNNER_TEMP=directory.as_posix(), GITHUB_ENV=environment_file.as_posix(),
                       CANDIDATE='a'*40, RUN_ID='offline-fixture', RUN_ATTEMPT='1', F2_ATTEMPT_LEASE='offline')
            python = shlex.quote(Path(sys.executable).as_posix())
            script = 'python3() { '+python+' "$@"; };\n'+self.workflow_step(
                'Record attempt identity before any fallible validation')
            result = self.bash(script, env)
            self.assertEqual(result.returncode, 0, result.stderr)
            receipt = json.loads((directory/'f2-receipts/attempt.json').read_text())
            self.assertEqual(receipt['candidate'], 'a'*40)
            self.assertEqual(receipt['run_id'], 'offline-fixture')
            self.assertEqual(receipt['attempt'], '1')
            self.assertEqual(receipt['lease'], 'offline')
            expected = {name: directory.as_posix()+'/'+suffix for name, suffix in [
                ('F2_EXPORTS', 'f2-exports'), ('F2_ATTEMPT', 'f2-attempt'), ('F2_RECEIPTS', 'f2-receipts')]}
            lines = environment_file.read_text().splitlines()
            self.assertEqual(len(lines), 3)
            entries = dict(line.split('=', 1) for line in lines)
            self.assertEqual(entries, expected)
            subsequent = self.bash('printf \'%s\\n\' "$F2_EXPORTS" "$F2_ATTEMPT" "$F2_RECEIPTS"',
                                   dict(env, **entries))
            self.assertEqual(subsequent.returncode, 0, subsequent.stderr)
            self.assertEqual(subsequent.stdout.splitlines(), list(expected.values()))

    def test_actual_event_admission_and_rerun_refusal(self):
        text = (ROOT.parents[1]/'.github/workflows/feed-handoff-performance.yml').read_text()
        self.assertEqual(text.split('on:\n', 1)[1].split('\npermissions:', 1)[0].strip(),
                         'pull_request:\n    types: [opened, reopened]')
        script = self.workflow_step('Refuse a rerun before building or sampling')
        with tempfile.TemporaryDirectory(prefix='quantick-f2-admission-') as temporary:
            for attempt, expected_exit in [('1', 0), ('2', 1)]:
                result = self.bash(script, dict(os.environ, F2_RECEIPTS=Path(temporary).as_posix(),
                                               RUN_ATTEMPT=attempt))
                self.assertEqual(result.returncode, expected_exit, result.stderr)
                self.assertEqual((Path(temporary)/'rerun-admission.log').read_text(),
                                 'run_attempt='+attempt+'\n')

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
                    'def settle_once(): return {"seconds_requested":10, "offline_stub":True}\n'
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
                self.assertEqual(json.loads((out/'settling.json').read_text())['seconds_requested'],10)
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
