"""Offline compile-supervision and disposal tests; no child is launched."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import hosted
import owner_perf_runner01 as owner


def success():
    return dict(tree_quiescent=True, ownership_observations=[dict(stage='after_quiescence', accounting_active=0)], exit_code=0, timed_out=False, interrupted=None, state='finished', orphaned_descendants=True)


class CompileRepairs(unittest.TestCase):
    def test_unchanged_import_closure(self):
        self.assertTrue(callable(hosted.load_supervisor().run_process))

    def run_case(self, record, accepted):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder); (root/'receipts').mkdir()
            def run(args,cwd,env,directory,timeout):
                self.assertEqual(timeout,3600)
                for name in ('stdout.log','stderr.log','process.json','supervisor.log'):
                    (directory/name).write_text('synthetic only',encoding='utf-8')
                return record
            with patch.object(hosted,'ROOT',root), patch.object(hosted,'load_supervisor',return_value=SimpleNamespace(run_process=run)):
                if accepted:
                    self.assertEqual(hosted.command(['synthetic'],root,{},'case'), root/'receipts/case/stdout.log')
                else:
                    with self.assertRaises(owner.InvalidEvidence): hosted.command(['synthetic'],root,{},'case')
            receipt=owner.load(root/'receipts/case/closure.json')
            self.assertEqual(receipt['state'],'terminal' if accepted else 'failed')
            self.assertEqual(receipt['supervision'],record)
            if not record['tree_quiescent'] or not record['ownership_observations']:
                self.assertNotIn('closed_log_hashes',receipt)
            else:
                for name,sha in receipt['closed_log_hashes'].items(): self.assertEqual(owner.digest(root/'receipts/case'/name),sha)

    def test_contained_helpers_allowed_only_after_zero_accounting(self): self.run_case(success(),True)
    def test_unknown_tree_refused(self): self.run_case(dict(success(),tree_quiescent=False),False)
    def test_missing_zero_accounting_refused(self): self.run_case(dict(success(),ownership_observations=[]),False)
    def test_timeout_refused(self): self.run_case(dict(success(),timed_out=True),False)
    def test_interruption_refused(self): self.run_case(dict(success(),interrupted='synthetic'),False)
    def test_nonzero_refused(self): self.run_case(dict(success(),exit_code=1),False)
    def test_unfinished_refused(self): self.run_case(dict(success(),state='starting'),False)

    def disposal(self, mutation=None):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder); target=root/'B-target';target.mkdir()
            exe=target/'app.exe';exe.write_bytes(b'exe')
            pdb=target/'app.pdb';pdb.write_bytes(b'pdb')
            trash=target/'intermediate';trash.write_bytes(b'object')
            source=root/'source';source.write_bytes(b'protected')
            keep={exe:owner.digest(exe),pdb:owner.digest(pdb)}
            closures=[dict(state='terminal',supervision=success()) for _ in range(2)]
            if mutation: mutation(closures,keep)
            with patch.object(hosted,'ROOT',root):
                if mutation:
                    with self.assertRaises(owner.InvalidEvidence): hosted.dispose_intermediates(target,keep,root/'disposal.json',closures)
                    self.assertTrue(trash.exists())
                else:
                    hosted.dispose_intermediates(target,keep,root/'disposal.json',closures)
                    self.assertFalse(trash.exists())
                    receipt=owner.load(root/'disposal.json');self.assertEqual(receipt['planned'],receipt['removed']);self.assertEqual(receipt['state'],'terminal')
            self.assertEqual(source.read_bytes(),b'protected')
            self.assertEqual(exe.read_bytes(),b'exe');self.assertEqual(pdb.read_bytes(),b'pdb')

    def test_only_intermediates_removed(self): self.disposal()
    def test_failed_profile_prevents_any_deletion(self): self.disposal(lambda c,k:c[0].update(state='failed'))
    def test_unresolved_tree_prevents_any_deletion(self): self.disposal(lambda c,k:c[0]['supervision'].update(tree_quiescent=False))
    def test_artifact_mismatch_prevents_any_deletion(self): self.disposal(lambda c,k:k.update({next(iter(k)):'bad'}))
    def test_junction_member_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);target=root/'C-target';target.mkdir(); item=target/'item';item.write_bytes(b'kept')
            with patch.object(hosted,'ROOT',root),patch.object(Path,'is_junction',lambda p:p==item):
                with self.assertRaises(owner.InvalidEvidence): hosted.dispose_intermediates(target,{},root/'receipt.json',[dict(state='terminal',supervision=success())]*2)
            self.assertTrue(item.exists())
    def test_other_target_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            with patch.object(hosted,'ROOT',root):
                with self.assertRaises(owner.InvalidEvidence): hosted.dispose_intermediates(root/'other',{},root/'receipt.json',[])

    def test_two_harmless_owned_children_preserve_import_closure(self):
        import os
        import sys
        if sys.platform != "win32": self.skipTest("Windows Job proof requires Windows")
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'receipts').mkdir()
            with patch.object(hosted,'ROOT',root):
                for label in ('first','second'):
                    log=hosted.command([sys.executable,'-c','print("synthetic owned child only")'],root,hosted.safe_environment(os.environ),label)
                    self.assertIn('synthetic owned child only',log.read_text())
                    receipt=owner.load(log.parent/'closure.json')
                    self.assertEqual(receipt['state'],'terminal')
                    hosted.require_quiescence(receipt['supervision'])
                    hosted.load_supervisor()
            self.assertEqual(list((hosted.HERE/'supervisor_closure').rglob('__pycache__')),[])
