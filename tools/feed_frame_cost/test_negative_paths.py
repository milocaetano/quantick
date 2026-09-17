"""Harmless Python child/source-fixture tests; no app, Cargo or timing protocol."""
from pathlib import Path
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile
import build
import lifecycle
import run_pairs
import run_geometry
import verify


class NegativePaths(unittest.TestCase):
    def test_receipt_serialization_is_platform_independent_lf(self):
        receipt = self.root / 'stable.json'
        lifecycle.write(receipt, {'one': 1})
        self.assertEqual(receipt.read_bytes(), b'{\n  "one": 1\n}\n')

    def test_protocol_newline_separates_pretty_libtest_prefix(self):
        prefix = 'running 1 test\ntest app::tests::f2_frame_protocol_tests::f2_frame_geometry ... '
        geometry = dict(mode='whole', size=[1440, 900], scale=1.5,
                        delivery_text=[], panel_geometry=None)
        framed = prefix + '\nF2_FRAME_GEOMETRY ' + json.dumps(geometry) + '\nok\n'
        self.assertEqual(run_geometry.records(framed), [geometry])
        with self.assertRaises(json.JSONDecodeError):
            run_geometry.records(prefix + '\nF2_FRAME_GEOMETRY {invalid\n')
        with self.assertRaisesRegex(ValueError, 'timing sample'):
            run_geometry.records(framed + 'F2_FRAME_SAMPLE {}\n')
        sample = self.root / 'sample.stdout'
        sample.write_text(prefix + '\nF2_FRAME_SAMPLE {"frames":600}\nok\n')
        self.assertEqual(run_pairs.sample_from(sample), {'frames': 600})
        sample.write_text(sample.read_text() + '\nF2_FRAME_SAMPLE {"frames":600}\n')
        with self.assertRaisesRegex(ValueError, 'exactly one'):
            run_pairs.sample_from(sample)

    def test_untimed_geometry_requires_every_case_and_matching_paint(self):
        control = [dict(mode='whole', size=size, scale=1.5, delivery_text=[], panel_geometry=None)
                   for size in [[1440.0, 900.0], [1000.0, 700.0]]]
        run_geometry.validate(control, 'control')
        for broken in [control[:1], control + control[:1]]:
            with self.assertRaisesRegex(ValueError, 'Incomplete or duplicate'):
                run_geometry.validate(broken, 'control')
        rows = []
        for size in [[1440.0, 900.0], [1000.0, 700.0]]:
            for mode in ['whole', 'panel_actual', 'panel_reference']:
                rows.append(dict(mode=mode, size=size, scale=1.5, delivery_text=['a', 'b', 'c'],
                    panel_geometry=dict(panel=[0, 50, size[0], 100], expected_height=50,
                        galleys=[dict(text=text, rect=[2, 55+n*10, 30, 60+n*10],
                                      clip=[0, 50, size[0], 100])
                                 for n, text in enumerate(['a', 'b', 'c'])])))
        run_geometry.validate(rows, 'candidate')
        changed = json.loads(json.dumps(rows))
        changed[2]['panel_geometry']['galleys'][0]['rect'][0] = 3
        with self.assertRaisesRegex(ValueError, 'focused paint differs'):
            run_geometry.validate(changed, 'candidate')
        changed = json.loads(json.dumps(rows))
        changed[0]['scale'] = 1
        with self.assertRaisesRegex(ValueError, 'scale'):
            run_geometry.validate(changed, 'candidate')

    def setUp(self):
        evidence = Path(__file__).parent / 'negative-path-evidence02'
        evidence.mkdir(exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix=self._testMethodName + '-', dir=evidence))

    def tearDown(self):
        pass  # Retain actual child/raw/receipt evidence; no test artifacts are removed.

    def terminal(self, tag):
        return json.loads((self.root / (tag + '.terminal.json')).read_text())

    def child(self, code, tag='child', **kwargs):
        # Explicit test-only direct-child policy: this Python child spawns none.
        return lifecycle.run_owned([sys.executable, '-c', code], self.root,
                                   {'SystemRoot': os.environ.get('SystemRoot', ''), 'PATH': os.defpath},
                                   self.root, tag, timeout=kwargs.pop('timeout', 5), linux=False, **kwargs)

    def test_environment_secret_and_override_exclusion(self):
        with patch.dict(os.environ, {'CARGO_REGISTRIES_X_TOKEN': 'CANARY', 'RUSTFLAGS': 'CANARY',
                                    'RUSTC_WRAPPER': 'CANARY', 'QUANTICK_CONTROL_ENABLE': 'CANARY',
                                    'F2_FRAME_MODE': 'CANARY', 'RUST_TEST_THREADS': 'CANARY'}):
            env = lifecycle.environment(self.root / 'runtime', mode='whole', size='normal')
        self.assertNotIn('CANARY', json.dumps(env))
        self.assertEqual(env['F2_FRAME_MODE'], 'whole')
        self.assertEqual(set(env), {'PATH', 'LANG', 'LC_ALL', 'HOME', 'TMPDIR', 'XDG_CONFIG_HOME',
                                   'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'F2_FRAME_MODE', 'F2_FRAME_SIZE'})

    def test_platform_refusal_precedes_child(self):
        with patch('lifecycle.platform.system', return_value='Windows'), patch('lifecycle.subprocess.Popen') as child:
            with self.assertRaisesRegex(RuntimeError, 'Linux-only'):
                lifecycle.run_owned(['never'], self.root, {}, self.root, 'platform', timeout=1)
        child.assert_not_called()
        self.assertIsNone(self.terminal('platform')['exit'])
        self.assertEqual(self.terminal('platform')['status'], 'FAIL')

    def test_inspector_failure_owns_child_until_terminal(self):
        def broken():
            raise ValueError('inspector-primary')
        with self.assertRaisesRegex(ValueError, 'inspector-primary'):
            self.child('import time; time.sleep(60)', inspect=broken)
        record = self.terminal('child')
        self.assertIsNotNone(record['exit'])
        self.assertEqual(record['primary']['message'], 'inspector-primary')

    def test_timeout_retains_partial_stream_and_actual_exit(self):
        with self.assertRaises(TimeoutError):
            self.child('import time; print("partial",flush=True); time.sleep(60)', timeout=.5)
        self.assertIn('partial', (self.root / 'child.stdout').read_text())
        self.assertIsNotNone(self.terminal('child')['exit'])

    def test_nonzero_exit_preserved(self):
        with self.assertRaises(RuntimeError):
            self.child('raise SystemExit(7)')
        self.assertEqual(self.terminal('child')['exit'], 7)

    def test_secondary_failure_does_not_replace_primary(self):
        with self.assertRaisesRegex(ValueError, 'first'):
            with lifecycle.Stage(self.root, 'stage') as stage:
                try:
                    raise ValueError('first')
                finally:
                    lifecycle.secondary(stage, lambda: (_ for _ in ()).throw(RuntimeError('second')), 'post')
        record = self.terminal('stage')
        self.assertEqual(record['primary']['message'], 'first')
        self.assertEqual(record['secondary'][0]['message'], 'second')

    def test_setup_failure_has_started_and_terminal(self):
        with self.assertRaises(FileNotFoundError):
            with lifecycle.Stage(self.root, 'setup'):
                (self.root / 'missing').read_bytes()
        self.assertTrue((self.root / 'setup.started.json').exists())
        self.assertEqual(self.terminal('setup')['status'], 'FAIL')

    def test_malformed_sample_and_artifact_ambiguity_fail_protocol(self):
        with self.assertRaises(json.JSONDecodeError):
            with lifecycle.Stage(self.root, 'protocol'):
                self.child('print("F2_FRAME_SAMPLE {invalid")')
                run_pairs.sample_from(self.root / 'child.stdout')
        self.assertEqual(self.terminal('child')['exit'], 0)
        self.assertEqual(self.terminal('protocol')['status'], 'FAIL')
        log = self.root / 'artifact'
        for rows in [[], [{'reason': 'compiler-artifact', 'target': {'name': 'quantick-app'},
                          'profile': {'test': True}, 'executable': 'x'}] * 2]:
            log.write_text('\n'.join(json.dumps(row) for row in rows))
            with self.assertRaises(ValueError):
                build.artifact(log)

    def test_complete_source_changes_and_additions_rejected(self):
        originals = {'dep.rs': b'dep', 'Cargo.lock': b'lock', 'modified.rs': b'old'}
        overlay = {'modified.rs': hashlib.sha256(b'new').hexdigest(), 'added.rs': hashlib.sha256(b'added').hexdigest()}
        expected = dict(originals, **{'modified.rs': b'new', 'added.rs': b'added'})
        for name, value in expected.items():
            (self.root / name).write_bytes(value)
        verify.verify_tree(self.root, originals, overlay)
        for name in expected:
            with self.subTest(name=name):
                (self.root / name).write_bytes(b'changed')
                with self.assertRaises(ValueError):
                    verify.verify_tree(self.root, originals, overlay)
                (self.root / name).write_bytes(expected[name])
        (self.root / 'extra.rs').write_bytes(b'extra')
        with self.assertRaises(ValueError):
            verify.verify_tree(self.root, originals, overlay)
        (self.root / 'extra.rs').unlink()
        (self.root / 'dep.rs').unlink()
        with self.assertRaises(ValueError):
            verify.verify_tree(self.root, originals, overlay)

    def test_archive_traversal_duplicate_and_symlink_rejected(self):
        for kind in ['traversal', 'duplicate', 'symlink']:
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, 'w') as archive:
                if kind == 'traversal':
                    archive.writestr('../bad', b'bad')
                elif kind == 'duplicate':
                    archive.writestr('a', b'1')
                    archive.writestr('a', b'2')
                else:
                    info = zipfile.ZipInfo('link')
                    info.external_attr = 0o120777 << 16
                    archive.writestr(info, b'target')
            with self.assertRaises(ValueError):
                verify.archive_files(stream.getvalue())

    def test_geometry_refuses_clipped_actual_text(self):
        sample = {'size': [100, 100], 'scale': 1.5, 'delivery_text': ['a', 'b', 'c'],
                  'panel_geometry': {'panel': [0, 50, 100, 100], 'expected_height': 50,
                    'galleys': [{'text': text, 'rect': [2, 55 + n*10, 30, 60 + n*10],
                                'clip': [0, 50, 100, 100]} for n, text in enumerate(['a', 'b', 'c'])]}}
        run_pairs.geometry(sample)
        sample['panel_geometry']['galleys'][0]['clip'] = [0, 0, 1, 1]
        with self.assertRaisesRegex(ValueError, 'outside'):
            run_pairs.geometry(sample)

    def test_focused_geometry_must_match_exactly(self):
        left = {'facts': {'trades': 8000}, 'delivery_text': ['a', 'b', 'c'], 'panel_geometry': {'panel': [0, 50, 100, 100]}}
        right = json.loads(json.dumps(left))
        run_pairs.paired_facts(left, right, 'panel')
        right['panel_geometry']['panel'][1] = 51
        with self.assertRaisesRegex(ValueError, 'geometry differs'):
            run_pairs.paired_facts(left, right, 'panel')

    def test_source_mutation_after_owned_child_is_failure(self):
        source = self.root / 'source'
        source.mkdir()
        (source / 'dep.rs').write_bytes(b'original')
        with self.assertRaisesRegex(ValueError, 'Full source mismatch'):
            with lifecycle.Stage(self.root, 'postimage') as stage:
                try:
                    self.child('from pathlib import Path; Path("source/dep.rs").write_bytes(b"changed")')
                finally:
                    lifecycle.secondary(stage, lambda: verify.verify_tree(source, {'dep.rs': b'original'}, {}), 'post-source')
        self.assertEqual(self.terminal('child')['exit'], 0)
        self.assertEqual(self.terminal('postimage')['status'], 'FAIL')


if __name__ == '__main__':
    unittest.main(verbosity=2)
