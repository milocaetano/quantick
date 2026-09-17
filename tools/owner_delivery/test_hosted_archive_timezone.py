"""Pure archive-child boundary tests; no Git subprocess or product runs."""
import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile
import hosted
import owner_perf_runner01 as owner

class ArchiveTimezoneTests(unittest.TestCase):
    def case(self,wrong=False):
        original=b'synthetic original\n';stream=io.BytesIO()
        with zipfile.ZipFile(stream,'w') as z:
            info=zipfile.ZipInfo(hosted.OVERLAY);info.create_system=3;info.external_attr=0o100644<<16;z.writestr(info,original)
        raw=stream.getvalue();subject={'commit':'a'*40,'archive_sha256':'0'*64 if wrong else hashlib.sha256(raw).hexdigest(),'files':{hosted.OVERLAY:hashlib.sha256(original).hexdigest()}}
        caller={'PATH':'synthetic','TZ':'original-zone','CARGO_BUILD_JOBS':'1'};before=dict(caller);build=dict(caller,CARGO_TARGET_DIR='unchanged');build_before=dict(build)
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder).resolve()
            def fake(argv,*,cwd,env):
                self.assertEqual(argv,['git','archive','--format=zip',subject['commit']]);self.assertEqual(cwd,root)
                self.assertEqual(env,dict(caller,TZ='UTC'));self.assertIsNot(env,caller)
                return raw
            with patch.object(hosted,'ROOT',root),patch.object(hosted.subprocess,'check_output',side_effect=fake) as query:
                if wrong:
                    with self.assertRaisesRegex(owner.InvalidEvidence,'archive identity mismatch'):hosted.source_prepare(root,subject,'B',caller)
                    self.assertFalse((root/'B-source').exists())
                else:
                    source,archive,post=hosted.source_prepare(root,subject,'B',caller)
                    self.assertEqual(archive.read_bytes(),raw)
                    self.assertEqual((source/hosted.OVERLAY).read_bytes(),original+b'\n'+(hosted.HERE/'owner-action-measurement-suffix02.rs').read_bytes())
                    self.assertEqual(post[hosted.OVERLAY],owner.digest(source/hosted.OVERLAY))
                self.assertEqual(query.call_count,1)
            self.assertEqual(caller,before);self.assertEqual(build,build_before)
    def test_utc_archive_only_preserves_argv_and_caller_environment(self):self.case()
    def test_wrong_raw_hash_refused_before_extraction(self):self.case(True)
