"""Required pre-Cargo Linux proof; no Rust/app/performance work."""
from pathlib import Path
import json
import os
import platform
import sys
import tempfile
import unittest
import lifecycle


@unittest.skipUnless(platform.system() == 'Linux', 'Actual Linux session cleanup remains unexecuted on other platforms')
class LinuxCleanup(unittest.TestCase):
    def run_case(self, mode):
        root = Path(tempfile.mkdtemp(prefix='f2-linux-owned-proof-'))
        code = ('import subprocess,sys,time; from pathlib import Path; '
                'p=subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"]); '
                'Path("grandchild.pid").write_text(str(p.pid)); print("child-ready",flush=True); '
                + ('raise SystemExit(0)' if mode == 'success' else 'time.sleep(60)'))
        def inspect():
            if mode == 'inspection' and (root / 'grandchild.pid').exists():
                raise ValueError('Linux inspector injected failure')
            return []  # Test callback only; no host admission is run or replaced.
        try:
            lifecycle.run_owned([sys.executable, '-c', code], root, {'PATH': os.defpath}, root,
                                'child', timeout=1 if mode == 'timeout' else 5, inspect=inspect)
            self.assertEqual(mode, 'success')
        except (TimeoutError, ValueError):
            self.assertNotEqual(mode, 'success')
        record = json.loads((root / 'child.terminal.json').read_text())
        self.assertIsNotNone(record['exit'])
        self.assertNotIn('ownership_unresolved', record)
        self.assertTrue(record['owned_group_signalled'])
        self.assertEqual(record['status'], 'PASS' if mode == 'success' else 'FAIL')
        grandchild = int((root / 'grandchild.pid').read_text())
        path = Path('/proc') / str(grandchild) / 'stat'
        if path.exists():
            stat = path.read_text()
            self.assertIn(stat[stat.rindex(')') + 2:].split()[0], ('Z', 'X'))
        print('Retained Linux cleanup proof:', root)

    def test_normal_leader_exit_with_live_child(self):
        self.run_case('success')

    def test_timeout_with_live_child(self):
        self.run_case('timeout')

    def test_inspector_failure_with_live_child(self):
        self.run_case('inspection')


if __name__ == '__main__':
    unittest.main(verbosity=2)
