"""Mocked host observations only; no live identity queries."""
import copy
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import hosted
import launcher
import owner_perf_runner01 as owner

class HostIdentityTests(unittest.TestCase):
    def case(self,change=None,reason=None):
        current,parent=os.getpid(),os.getppid()
        rows=[dict(pid=current,parent=parent,name='python.exe',created='2026-01-02T00:00:00+00:00',image=sys.executable,image_sha256='a'*64),dict(pid=parent,parent=1,name='pwsh.exe',created='2026-01-01T00:00:00+00:00',image=str(Path(sys.executable).with_name('pwsh.exe')),image_sha256='b'*64)]
        if change=='duplicate':rows[1]=copy.deepcopy(rows[0])
        if change=='missing':rows.pop()
        if change=='python':rows[0]['image']=str(Path(sys.executable).with_name('other.exe'))
        if change=='wrapper':rows[1]['name']='cmd.exe'
        if change=='parent':rows[0]['parent']=0
        if change=='unknown':rows[1]['image_sha256']=None
        if change=='malformed':rows[1]['image_sha256']='bad'
        if change=='reversed':rows[1]['created']='2027-01-01T00:00:00+00:00'
        if change=='badtime':rows[1]['created']='not time'
        second=copy.deepcopy(rows)
        if change=='drift':second[1]['image_sha256']='c'*64
        values=[(current,parent,rows),(current,parent,second)]
        if change=='query':values=[OSError('SENSITIVE-DO-NOT-RECORD')]
        if change=='partial':values[1]=OSError('SENSITIVE-DO-NOT-RECORD')
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder).resolve()
            with patch.object(hosted,'ROOT',root),patch.object(launcher,'query_identity',side_effect=values):
                if change:
                    with self.assertRaisesRegex((owner.InvalidEvidence,OSError,ValueError),reason):hosted.observe_launcher_host()
                else:hosted.observe_launcher_host()
            receipt=owner.load(root/'launcher-host-identity.json')
            self.assertEqual(receipt['state'],'failed' if change else 'terminal')
            self.assertEqual(len(receipt['observations']),0 if change=='query' else (2 if change in (None,'drift') else 1))
            self.assertNotIn('SENSITIVE', (root/'launcher-host-identity.json').read_text())
            self.assertFalse((root/'runtime.json').exists())
            self.assertNotIn('allowed_processes',receipt)
    def test_exact_observation(self):self.case()
    def test_refusals(self):
        for change,reason in [('duplicate','ambiguous'),('missing','ambiguous'),('python','Python executable mismatch'),('wrapper','not immediate pwsh parent'),('parent','not immediate pwsh parent'),('unknown','hash unavailable'),('malformed','hash unavailable'),('reversed','ordering invalid'),('badtime','Invalid isoformat'),('drift','changed between observations'),('query','SENSITIVE'),('partial','SENSITIVE')]:
            with self.subTest(change=change):self.case(change,reason)
