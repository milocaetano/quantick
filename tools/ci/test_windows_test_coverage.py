#!/usr/bin/env python3
"""Cases for the Windows test-coverage guard.

The guard's whole value is the failure it catches, so the cases are mostly
failures: a crate peeled off and forgotten, an `--exclude` that names nothing,
and a `-p` that names nothing. The last case reads the real `ci.yml`, because
a guard that passes on fixtures and disagrees with the workflow it guards is
worse than none.
"""

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import windows_test_coverage as guard  # noqa: E402

MEMBERS = {"quantick-app", "quantick-engine", "quantick-chart"}

SPLIT = """
  windows:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test --workspace --exclude quantick-app
  windows-app:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test -p quantick-app
  ci:
    runs-on: ubuntu-latest
    steps:
      - name: Test
        run: cargo test --workspace
"""


class Coverage(unittest.TestCase):
    def test_complementary_halves_cover_the_workspace(self):
        self.assertEqual(MEMBERS, guard.covered(SPLIT, MEMBERS))

    def test_a_peeled_crate_with_no_job_is_missed(self):
        orphaned = SPLIT.replace(
            "cargo test -p quantick-app", "cargo test -p quantick-chart"
        )
        self.assertNotIn("quantick-app", guard.covered(orphaned, MEMBERS))

    def test_a_linux_job_does_not_count_as_windows_coverage(self):
        linux_only = """
  ci:
    runs-on: ubuntu-latest
    steps:
      - name: Test
        run: cargo test --workspace
"""
        self.assertEqual(set(), guard.covered(linux_only, MEMBERS))

    def test_test_binary_arguments_are_not_package_selectors(self):
        nocapture = """
  windows:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test --workspace -- --nocapture -p not-a-package
"""
        self.assertEqual(MEMBERS, guard.covered(nocapture, MEMBERS))

    def test_an_exclude_naming_no_member_is_refused(self):
        typo = SPLIT.replace("--exclude quantick-app", "--exclude quantick-ap")
        with self.assertRaises(ValueError):
            guard.covered(typo, MEMBERS)

    def test_a_package_naming_no_member_is_refused(self):
        typo = SPLIT.replace("-p quantick-app", "-p quantick-ap")
        with self.assertRaises(ValueError):
            guard.covered(typo, MEMBERS)


class AgainstTheRealWorkflow(unittest.TestCase):
    def test_the_workflow_names_at_least_two_windows_jobs(self):
        workflow = pathlib.Path(guard.WORKFLOW).read_text(encoding="utf-8")
        self.assertGreaterEqual(len(guard.windows_jobs(workflow)), 2)

    def test_every_windows_selector_names_a_real_crate(self):
        # Parsing failures surface here rather than only in CI's own run: the
        # guard raises on a selector that names nothing, and the member list
        # comes from the same `cargo metadata` the guard uses.
        workflow = pathlib.Path(guard.WORKFLOW).read_text(encoding="utf-8")
        members = guard.workspace_members()
        self.assertEqual(members, guard.covered(workflow, members))


if __name__ == "__main__":
    unittest.main(verbosity=2)
