"""Offline transport and sole-parent derivation fixtures; no network or CIM."""
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
import zipfile
import launcher
import transport
import owner_perf_runner01 as owner


class Transport(unittest.TestCase):
    def case(self,change=None):
        payload=b'synthetic inner zip fixture'
        stream=io.BytesIO()
        names=['payload.zip']
        if change=='missing': names=[]
        if change=='ambiguous': names+=['nested/payload.zip']
        with zipfile.ZipFile(stream,'w') as z:
            for name in names:z.writestr(name,payload)
        raw=stream.getvalue();sha=hashlib.sha256(raw).hexdigest()
        binding={'artifact':dict(run_id=10,artifact_id=20,carrier_sha='c'*40,name='synthetic',download_sha256=sha),'package_sha256':hashlib.sha256(payload).hexdigest()}
        run=dict(id=10,run_attempt=1,head_sha='c'*40,conclusion='success')
        artifact=dict(id=20,name='synthetic',expired=False,workflow_run={'id':10},digest='sha256:'+sha)
        mutations={'run':(run,'id',11),'attempt':(run,'run_attempt',2),'carrier':(run,'head_sha','bad'),'expired':(artifact,'expired',True),'artifact':(artifact,'id',21),'artifact_run':(artifact,'workflow_run',{'id':11}),'name':(artifact,'name','other')}
        if change in mutations:
            obj,key,value=mutations[change];obj[key]=value
        if change=='outer': raw+=b'changed'
        if change=='inner':binding['package_sha256']='0'*64
        if change=='pending':binding['unbound']='PENDING'
        calls=[]
        def fetch(url,token):
            self.assertEqual(token,'secret-token');calls.append(url)
            if change=='api':raise HTTPError(url,403,'SECRET',{},None)
            if url.endswith('/zip'):raise HTTPError(url,302,'redirect',{'Location':'https://example.invalid/SECRET-SIGNED'},None)
            return io.BytesIO(json.dumps(run if '/runs/' in url else artifact).encode())
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);bp=root/'binding.json';owner.save(bp,binding);out=root/'out';out.mkdir()
            with patch.object(transport,'fetch',fetch),patch.object(transport.urllib.request,'urlopen',return_value=io.BytesIO(raw)),patch.dict(transport.os.environ,{'OWNER_TRANSPORT_TOKEN':'secret-token'}):
                if change:
                    with self.assertRaises((owner.InvalidEvidence,HTTPError)):transport.main(bp,out)
                else:transport.main(bp,out)
            receipt=owner.load(out/'transport.json');self.assertEqual(receipt['state'],'failed' if change else 'terminal')
            for name,identity in receipt['files'].items():self.assertEqual(transport.file_identity(out/name),identity)
            self.assertNotIn('SECRET',(out/'transport.json').read_text());self.assertNotIn('secret-token',(out/'transport.json').read_text())
            if change in ('outer','inner','missing','ambiguous') or change is None:self.assertEqual((out/'download.zip').read_bytes(),raw)
            if change in ('inner',None):self.assertEqual((out/'payload.zip').read_bytes(),payload)
            if change=='pending':self.assertEqual(calls,[])
    def test_success(self):self.case()
    def test_refusals_retain_exact_partial_evidence(self):
        for change in ('run','attempt','carrier','expired','artifact','artifact_run','name','outer','inner','missing','ambiguous','api','pending'):
            with self.subTest(change=change):self.case(change)


class Launcher(unittest.TestCase):
    def case(self,change=None):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);python=root/'python.exe';pwsh=root/'pwsh.exe';python.write_bytes(b'python');pwsh.write_bytes(b'pwsh')
            policy=dict(decision='https://github.com/milocaetano/quantick/issues/480#issuecomment-1',python_exe=str(python),python_sha256=owner.digest(python),pwsh_exe=str(pwsh),pwsh_sha256=owner.digest(pwsh))
            template={'allowed_processes':[],'fixed_protocol':'unchanged'}
            rows=[dict(pid=100,parent=50,name='python.exe',created='2026-01-01T00:00:01+00:00',image=str(python),image_sha256=owner.digest(python)),dict(pid=50,parent=1,name='pwsh.exe',created='2026-01-01T00:00:00+00:00',image=str(pwsh),image_sha256=owner.digest(pwsh))]
            second=copy.deepcopy(rows)
            if change=='recycled':second[1]['created']='2025-01-01T00:00:00+00:00'
            if change=='wrapper':rows[1]['name']='cmd.exe'
            if change=='hash':rows[1]['image_sha256']='bad'
            if change=='unknown':rows[1]['image_sha256']=None
            if change=='relationship':rows[0]['parent']=99
            if change=='missing':rows.pop()
            if change=='prepopulated':template['allowed_processes']=[dict(pid=1,created='old')]
            if change=='pending':policy['pwsh_sha256']='PENDING'
            tp=root/'template.json';pp=root/'policy.json';owner.save(tp,template);owner.save(pp,policy)
            ts=owner.digest(tp);ps=owner.digest(pp)
            if change=='tamper':tp.write_text('{}')
            events=dict(run='1',attempt='1',before='a',after='b',ref='synthetic')
            observations=iter([(100,50,rows),(100,50,list(reversed(second)))])
            def observe():return next(observations)
            out=root/'proof'
            if change:
                with self.assertRaises(owner.InvalidEvidence):launcher.derive(tp,ts,pp,ps,out,events,observe)
            else:
                result=launcher.derive(tp,ts,pp,ps,out,events,observe)
                self.assertEqual(owner.load(result),dict(template,allowed_processes=[{'pid':50,'created':rows[1]['created']}]))
                self.assertEqual(owner.digest(tp),ts)
            receipt=owner.load(out/'derivation.json');self.assertEqual(receipt['state'],'failed' if change else 'terminal')
            for name,sha in receipt['artifact_hashes'].items():self.assertEqual(owner.digest(out/name),sha)
            if change in ('recycled','wrapper','hash','unknown','relationship','missing'):self.assertTrue(receipt['observations'])
    def test_exact_sole_parent_and_only_changed_key(self):self.case()
    def test_refusals_preserve_observation(self):
        for change in ('recycled','wrapper','hash','unknown','relationship','missing','prepopulated','pending','tamper'):
            with self.subTest(change=change):self.case(change)
