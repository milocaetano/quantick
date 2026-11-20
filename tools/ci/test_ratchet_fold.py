#!/usr/bin/env python3
"""Exercise the bot lifecycle with local Git and no remote operations."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import ratchet_fold


class FoldWorkflow(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.calls = []
        self.existing = []
        self.fold = lambda: None
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.test")
        self.baseline = self.repo / "crates/guards/app-lines-baseline.txt"
        self.baseline.parent.mkdir(parents=True)
        self.baseline.write_text("!budget 100\ncrates/app 100\n", encoding="utf-8")
        self.git("add", ".")
        self.git("commit", "-m", "fixture")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")

    def git(self, *args):
        return ratchet_fold.run(["git", *args], self.repo)

    def execute(self, args, repo):
        self.assertEqual(repo, self.repo)
        self.calls.append(args)
        if args[:3] == ["gh", "pr", "list"]:
            return json.dumps(self.existing)
        if args[:3] == ["gh", "pr", "create"] or args[:2] == ["git", "push"]:
            return ""
        if args[0] == "cargo":
            self.fold()
            return ""
        if args[0] == sys.executable:
            self.assertEqual(args[1:], ["tools/ci/ratchet_delta.py", "--base", "origin/main"])
            return ""
        return ratchet_fold.run(args, repo)

    def count(self, prefix):
        return sum(call[:len(prefix)] == prefix for call in self.calls)

    def test_zero_fragments_no_change_has_no_push_or_pr(self):
        before = self.git("rev-parse", "HEAD")
        ratchet_fold.maintain(self.repo, "noop", self.execute)
        self.assertEqual(self.git("rev-parse", "HEAD"), before)
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.assertEqual(self.count(["git", "push"]), 0)
        self.assertEqual(self.count(["gh", "pr", "create"]), 0)

    def test_initial_pr_stages_removed_last_fragment_and_new_baseline(self):
        fragment = self.baseline.with_suffix(".d") / "branch.txt"
        fragment.parent.mkdir()
        fragment.write_text("!budget +1\ncrates/app +1\n", encoding="utf-8")
        self.git("add", ".")
        self.git("commit", "-m", "signed raise")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")

        def fold():
            self.baseline.write_text("!budget 101\ncrates/app 101\n", encoding="utf-8")
            fragment.unlink()
        self.fold = fold
        ratchet_fold.maintain(self.repo, "initial", self.execute)
        self.assertEqual(self.count(["git", "push"]), 1)
        self.assertEqual(self.count(["gh", "pr", "create"]), 1)
        self.assertIn("D\tcrates/guards/app-lines-baseline.d/branch.txt",
                      self.git("show", "--format=", "--name-status", "HEAD"))
        self.assertEqual(self.git("status", "--porcelain"), "")

    def existing_branch(self):
        self.git("branch", "docs/fold-existing")
        self.existing = [{"number": 12, "headRefName": "docs/fold-existing"}]

    def test_existing_pr_updates_once_without_creating_another(self):
        self.existing_branch()
        self.fold = lambda: self.baseline.write_text("!budget 99\ncrates/app 99\n", encoding="utf-8")
        ratchet_fold.maintain(self.repo, "update", self.execute)
        ratchet_fold.maintain(self.repo, "retry", self.execute)
        self.assertEqual(self.count(["git", "push"]), 1)
        self.assertEqual(self.count(["gh", "pr", "create"]), 0)
        self.assertEqual(self.git("branch", "--show-current"), "docs/fold-existing")

    def test_existing_pr_pushes_merged_main_even_without_new_fragments(self):
        self.existing_branch()
        (self.repo / "new-main.txt").write_text("main update", encoding="utf-8")
        self.git("add", ".")
        self.git("commit", "-m", "advance main")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        ratchet_fold.maintain(self.repo, "sync", self.execute)
        self.assertEqual(self.count(["git", "push"]), 1)
        self.assertEqual(self.count(["gh", "pr", "create"]), 0)

    def test_validation_failure_cannot_publish(self):
        def fail():
            raise subprocess.CalledProcessError(1, "cargo")
        self.fold = fail
        with self.assertRaises(subprocess.CalledProcessError):
            ratchet_fold.maintain(self.repo, "invalid", self.execute)
        self.assertEqual(self.count(["git", "push"]), 0)
        self.assertEqual(self.count(["gh", "pr", "create"]), 0)


if __name__ == "__main__":
    unittest.main()
