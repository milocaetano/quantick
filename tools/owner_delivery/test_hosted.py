"""Synthetic packaging/admission tests only; never compile or run the app."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

import hosted
import owner_perf_runner01 as owner


class HostedTests(unittest.TestCase):
    def test_both_actual_cargo_profiles_keep_raw_default_marker(self):
        with tempfile.TemporaryDirectory() as folder:
            manifest = Path(folder) / "Cargo.toml"
            manifest.write_text('[features]\ndefault = []\n', encoding="utf-8")
            for optimization in ("1", "3"):
                artifact = {"features": ["default"], "manifest_path": str(manifest), "target": {"name": "quantick-app"}, "profile": {"test": True, "opt_level": optimization}}
                self.assertEqual(hosted.owner.ordinary_default_features(manifest, artifact), [])
                self.assertEqual(artifact["features"], ["default"])
                with self.assertRaises(owner.InvalidEvidence):
                    hosted.owner.ordinary_default_features(manifest, {**artifact, "features": ["default", "drawing-harness"]})
            manifest.write_text('[features]\ndefault = ["drawing-harness"]\n', encoding="utf-8")
            with self.assertRaises(owner.InvalidEvidence):
                hosted.owner.ordinary_default_features(manifest, artifact)

    def test_clean_child_environment_drops_credentials_and_scenarios(self):
        source = {"PATH": "system", "SystemRoot": "system", "GITHUB_TOKEN": "secret", "ACTIONS_RUNTIME_TOKEN": "secret", "OWNER_TRANSPORT_TOKEN": "secret", "AWS_SECRET_ACCESS_KEY": "secret", "QUANTICK_CONTROL_ACCESS": "1", "CARGO_REGISTRIES_CRATES_IO_TOKEN": "secret", "RUSTFLAGS": "override"}
        self.assertEqual(hosted.safe_environment(source), {"PATH": "system", "SystemRoot": "system", "PYTHONDONTWRITEBYTECODE": "1"})

    def test_finite_event_and_placeholder_fail_closed(self):
        binding = {"phase": "compile", "decision": "https://github.com/milocaetano/quantick/issues/480#issuecomment-123", "ref": "refs/heads/synthetic", "before": "b" * 40, "products": {"B": {"commit": owner.BASELINE}, "C": {"commit": "c" * 40}}}
        event = {"event": "push", "attempt": "1", "repository": "milocaetano/quantick", "ref": binding["ref"], "before": binding["before"]}
        hosted.admit(binding, event, "compile")
        for key, value in (("attempt", "2"), ("before", "a" * 40), ("event", "workflow_dispatch"), ("ref", "refs/heads/main")):
            with self.assertRaises(owner.InvalidEvidence):
                hosted.admit(binding, {**event, key: value}, "compile")
        with self.assertRaises(owner.InvalidEvidence):
            hosted.admit({**binding, "unbound": "PENDING_FINAL_ROOT_REVIEW"}, event, "compile")

    def test_roundtrip_fixed_relative_identity_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as folder:
            base = Path(folder)
            source = base / "source"
            source.mkdir()
            (source / "nested").mkdir()
            (source / "nested/source.rs").write_bytes(b"synthetic source\r\n")
            (source / "binary.exe").write_bytes(b"synthetic non-executable bytes")
            archive = base / "payload.zip"
            mapping = hosted.package(source, archive, {"nested/source.rs", "binary.exe"})
            restored = base / "restored"
            hosted.restore(archive, owner.digest(archive), restored)
            owner.verify_files(restored, mapping, True)
            with self.assertRaises(owner.InvalidEvidence):
                hosted.restore(archive, owner.digest(archive), restored)
            with self.assertRaises(owner.InvalidEvidence):
                hosted.package(source, archive, {"binary.exe"})

    def test_wrong_transport_hash_does_not_restore(self):
        with tempfile.TemporaryDirectory() as folder:
            base = Path(folder)
            archive = base / "payload.zip"
            archive.write_bytes(b"synthetic corrupt transport")
            target = base / "out"
            with self.assertRaises(owner.InvalidEvidence):
                hosted.restore(archive, "0" * 64, target)
            self.assertFalse(target.exists())

    def test_missing_extra_and_escape_members_refused(self):
        cases = [({"missing": "0" * 64}, {}), ({}, {"extra": b"x"}), ({"../outside": hashlib.sha256(b"x").hexdigest()}, {"../outside": b"x"})]
        for mapping, contents in cases:
            with tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                archive = root / "payload.zip"
                with zipfile.ZipFile(archive, "w") as z:
                    for name, data in {"manifest.json": json.dumps(mapping).encode(), **contents}.items():
                        info = zipfile.ZipInfo(name)
                        info.create_system = 3
                        info.external_attr = 0o100644 << 16
                        z.writestr(info, data)
                target = root / "restored"
                with self.assertRaises(owner.InvalidEvidence):
                    hosted.restore(archive, owner.digest(archive), target)
                self.assertFalse(target.exists())

    def test_original_reviewed_measurement_tools_unchanged(self):
        expected = {"owner_perf_runner01.py": "847b00607322fa0c0f260fd18c56b7376cb84b6bd8830b35532169a2f77c2312", "owner_perf_parser01.py": "8b5b32768a1ec1fd9234fd0fe7fb283a52096f8222edfc18a5a095f35fda60ca", "owner-action-measurement-suffix02.rs": "e34b57b032e8d88cb68b85606b7f479b97446db5f5cb93595a053dff9013340a"}
        for name, sha in expected.items():
            self.assertEqual(owner.digest(hosted.HERE / name), sha)


if __name__ == "__main__":
    unittest.main()
