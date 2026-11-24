"""Git orchestration fixtures; ratchet decisions are tested by their Rust owner."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import ratchet_delta as delta


class DeltaRunner(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name).resolve()
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        (self.repo / ".gitignore").write_text("target/\n", encoding="utf-8")
        (self.repo / "tracked").write_text("base\n", encoding="utf-8")
        self.commit("base")
        self.base_sha = self.git("rev-parse", "HEAD").strip()
        self.git("switch", "-c", "task")
        (self.repo / "tracked").write_text("task\n", encoding="utf-8")
        self.commit("task")
        self.git("switch", "main")
        (self.repo / "target-only").write_text("target\n", encoding="utf-8")
        self.commit("target advanced")
        self.git("switch", "task")

    def git(self, *args):
        return delta.run(["git", *args], self.repo)

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "-m", message)

    def test_actual_merge_base_is_pristine_and_reused(self):
        sha, base = delta.base_checkout(self.repo, "main")
        self.assertEqual(sha, self.base_sha)
        self.assertEqual((base / "tracked").read_text(), "base\n")
        self.assertFalse((base / "target-only").exists())
        self.assertEqual(delta.base_checkout(self.repo, "main"), (sha, base))
        for path in [base / "tracked", base / "untracked", base / "target/ignored"]:
            path.parent.mkdir(parents=True, exist_ok=True)
            previous = path.read_bytes() if path.exists() else None
            path.write_text("dirty\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "not pristine"):
                delta.base_checkout(self.repo, "main")
            if previous is None:
                path.unlink()
            else:
                path.write_bytes(previous)

    def test_missing_reference_fails_before_build_or_export(self):
        export = self.repo / "environment"
        with patch.object(delta, "build") as build:
            with self.assertRaises(ValueError):
                delta.check(self.repo, "missing-ref", export_env=export)
            build.assert_not_called()
        self.assertFalse(export.exists())

    def test_one_head_build_and_base_export_only_after_success(self):
        export = self.repo / "environment"
        original_run = delta.run
        calls = []

        def run(args, repo, env=None):
            if args[0] == "fixture-guard":
                calls.append((args, repo, env))
                return "guard passed\n"
            return original_run(args, repo, env)

        with patch.object(delta, "build", return_value=Path("fixture-guard")) as build:
            with patch.object(delta, "run", side_effect=run):
                output = delta.check(self.repo, "main", file="tracked", export_env=export)
            build.assert_called_once_with(self.repo)
        self.assertIn(f"base={self.base_sha}", output)
        self.assertIn("guard passed", output)
        args, repo, env = calls[0]
        self.assertEqual(args[1], "--base-root")
        self.assertEqual(args[3:], ["--file", "tracked"])
        self.assertEqual(repo, self.repo)
        self.assertEqual(env["QUANTICK_GUARDS_ROOT"], str(self.repo))
        self.assertEqual(export.read_text(), f"QUANTICK_GUARDS_BASE_ROOT={args[2]}\n")
        export.unlink()

        def fail(args, repo, env=None):
            if args[0] == "fixture-guard":
                raise ValueError("guard rejected growth")
            return original_run(args, repo, env)

        with patch.object(delta, "build") as build, patch.object(delta, "run", side_effect=fail):
            with self.assertRaisesRegex(ValueError, "guard rejected growth"):
                delta.check(self.repo, "main", binary=Path("fixture-guard"), export_env=export)
            build.assert_not_called()
        self.assertFalse(export.exists())

    def test_cli_failure_has_nonzero_exit(self):
        result = subprocess.run(
            [os.sys.executable, str(Path(delta.__file__).resolve()), "--repo", str(self.repo),
             "--base", "missing-ref"], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing-ref", result.stderr)


if __name__ == "__main__":
    unittest.main()
