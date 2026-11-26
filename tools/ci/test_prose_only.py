#!/usr/bin/env python3
"""Tests for prose_only.py: python tools/ci/test_prose_only.py

The synthetic cases pin the rules on a hand-drawn workspace. The workspace
cases read this repository: a doc a crate's test opens must build, and a doc
nothing names must not.
"""

import os
import shutil
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import affected_crates  # noqa: E402
import prose_only  # noqa: E402

PACKAGES = {
    "quantick-pine": {"dir": "crates/pine", "deps": set()},
    "quantick-guards": {"dir": "crates/guards", "deps": set()},
}


def referrers_from(table):
    """A fake search: {needle: [referrer path, ...]}."""
    def referrers(wanted):
        return sorted((path, needle) for needle in wanted for path in table.get(needle, ()))
    return referrers


NOBODY = referrers_from({})


class Synthetic(unittest.TestCase):
    def test_unreferenced_docs_and_markdown_are_prose(self):
        verdict, reasons = prose_only.decide(
            ["docs/ideas/one.md", "docs/ux/mock.png", ".claude/skills/x/SKILL.md", "AGENTS.md"],
            PACKAGES, NOBODY)
        self.assertEqual("prose", verdict)
        self.assertEqual(4, len(reasons))

    def test_any_code_path_builds(self):
        for path in ("crates/pine/src/lib.rs", "tools/ci/prose_only.py", ".github/workflows/ci.yml",
                     ".claude/hooks/full_ci.sh", "Cargo.lock", "quantick.toml"):
            with self.subTest(path=path):
                verdict, reasons = prose_only.decide(["docs/a.md", path], PACKAGES, NOBODY)
                self.assertEqual("build", verdict)
                self.assertEqual([f"{path} is not prose"], reasons)

    def test_markdown_under_a_code_root_builds(self):
        for path in ("crates/engine/src/notes.md", "bridge/mt5/PROTOCOL.md", "tools/x/README.md",
                     "schemas/control/README.md", ".github/PULL_REQUEST_TEMPLATE.md"):
            with self.subTest(path=path):
                self.assertEqual("build", prose_only.decide([path], PACKAGES, NOBODY)[0])

    def test_a_doc_a_crate_reads_builds(self):
        referrers = referrers_from({"docs/pine-dialect.md": ["crates/pine/tests/combined_parity.rs"]})
        verdict, reasons = prose_only.decide(["docs/pine-dialect.md"], PACKAGES, referrers)
        self.assertEqual("build", verdict)
        self.assertIn("quantick-pine", reasons[0])

    def test_a_doc_only_the_gate_crate_reads_stays_prose(self):
        referrers = referrers_from({"docs/workflow/delivery.md": ["crates/guards/src/context.rs"]})
        self.assertEqual("prose", prose_only.decide(["docs/workflow/delivery.md"], PACKAGES, referrers)[0])

    def test_a_named_ancestor_directory_builds(self):
        referrers = referrers_from({"docs/quality": ["crates/pine/src/lib.rs"]})
        self.assertEqual("build", prose_only.decide(["docs/quality/x/y.md"], PACKAGES, referrers)[0])

    def test_a_reader_in_any_searched_revision_builds(self):
        head = referrers_from({})
        merge = referrers_from({"docs/foo.md": ["crates/pine/tests/doc.rs"]})
        either = prose_only.union_referrers([head, merge])
        self.assertEqual("prose", prose_only.decide(["docs/foo.md"], PACKAGES, head)[0])
        self.assertEqual("build", prose_only.decide(["docs/foo.md"], PACKAGES, either)[0])

    def test_an_empty_diff_builds(self):
        self.assertEqual(("build", ["the diff is empty"]), prose_only.decide([], PACKAGES, NOBODY))


@unittest.skipUnless(shutil.which("cargo") and shutil.which("git"), "needs cargo and git")
class Workspace(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root, cls.packages = affected_crates.metadata_workspace()
        cls.referrers = staticmethod(affected_crates.git_referrers(cls.root, "HEAD"))

    def decide(self, path):
        return prose_only.decide([path], self.packages, self.referrers)[0]

    def test_docs_a_test_reads_build(self):
        for path in ("docs/pine-dialect.md", "docs/quality/live-envelope.md",
                     "docs/control-plane/ui-behaviour-matrix.md",
                     ".claude/skills/ui-harness/references/hook-registry.md"):
            with self.subTest(path=path):
                self.assertEqual("build", self.decide(path))

    def test_a_doc_no_crate_names_is_prose(self):
        # A path made up for this test, so a crate citing a real doc in a
        # comment can never turn it red.
        self.assertEqual("prose", self.decide("docs/prose-only-fixture/nothing-names-this.md"))

    def test_a_skill_file_crates_only_document_is_prose(self):
        # Crates name this skill's directory in doc comments and its sibling
        # `.claude/skills/ui-harness/` in code; neither reads it (PR #1238).
        self.assertEqual("prose", self.decide(".claude/skills/implement/references/execute.md"))

    def test_the_cli_prints_the_verdict(self):
        head = subprocess.run(("git", "rev-parse", "HEAD"), cwd=self.root, check=True,
                              capture_output=True, text=True).stdout.strip()
        result = subprocess.run((sys.executable, os.path.join(HERE, "prose_only.py"),
                                 "--base", head, "--head", head, "--search", "HEAD"),
                                cwd=self.root, check=True, capture_output=True, text=True)
        self.assertEqual("build", result.stdout.strip())
        self.assertIn("the diff is empty", result.stderr)


if __name__ == "__main__":
    unittest.main()
