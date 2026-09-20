#!/usr/bin/env python3
"""Cases for the Windows test-coverage guard.

The guard's whole value is the failure it catches, so the cases are mostly
failures: a crate peeled off and forgotten, an `--exclude` that names nothing,
a `-p` that names nothing, and a crate two shares both pay to test. The last
cases read the real `ci.yml`, because a guard that passes on fixtures and
disagrees with the workflow it guards is worse than none.
"""

import contextlib
import io
import pathlib
import sys
import tempfile
import unittest
import unittest.mock

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

# Three shares rather than two: the shape `ci.yml` carries once the long
# Windows test job is split as well.
THREE_WAY = """
  windows-heavy-tests:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test -p quantick-engine
  windows-tests:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test --workspace --exclude quantick-app --exclude quantick-engine
  windows-app-tests:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test -p quantick-app
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


class Shares(unittest.TestCase):
    """The shares must be *exactly* the workspace: all of it, once each."""

    def test_three_shares_partition_the_workspace(self):
        self.assertEqual(
            {"quantick-app": 1, "quantick-engine": 1, "quantick-chart": 1},
            dict(guard.shares(THREE_WAY, MEMBERS)),
        )

    def test_a_crate_two_shares_both_claim_is_counted_twice(self):
        overlapping = THREE_WAY.replace(
            "--exclude quantick-app --exclude quantick-engine",
            "--exclude quantick-app",
        )
        self.assertEqual(2, guard.shares(overlapping, MEMBERS)["quantick-engine"])

    def test_a_crate_no_share_claims_is_absent(self):
        dropped = THREE_WAY.replace(
            "cargo test -p quantick-engine", "cargo test -p quantick-app"
        )
        self.assertNotIn("quantick-engine", guard.shares(dropped, MEMBERS))

    def test_a_named_diagnostic_step_is_not_a_share(self):
        # `windows` runs one crate's tests again under `--nocapture` on
        # purpose. Counting that step as a share would report a crate tested
        # twice and redden a workflow doing exactly what it should.
        diagnostic = (
            THREE_WAY
            + """
  windows:
    runs-on: windows-latest
    steps:
      - name: Test Windows descriptor authority and local transport
        run: cargo test -p quantick-chart -- --nocapture
"""
        )
        self.assertEqual(1, guard.shares(diagnostic, MEMBERS)["quantick-chart"])
        self.assertEqual(MEMBERS, guard.covered(diagnostic, MEMBERS))

    def test_a_missed_crate_fails_the_report(self):
        dropped = THREE_WAY.replace(
            "cargo test -p quantick-engine", "cargo test -p quantick-app"
        )
        self.assertTrue(
            any("no tests for" in line for line in guard.problems(dropped, MEMBERS))
        )

    def test_a_doubly_tested_crate_fails_the_report(self):
        overlapping = THREE_WAY.replace(
            "--exclude quantick-app --exclude quantick-engine",
            "--exclude quantick-app",
        )
        problems = guard.problems(overlapping, MEMBERS)
        self.assertTrue(
            any("quantick-engine" in line and "twice" in line for line in problems)
        )

    def test_a_crate_only_a_diagnostic_runs_is_reported_as_unshared(self):
        # The middle case between the two obvious ones: the crate is tested,
        # so nothing is silently unverified, but no share owns it, so the
        # first rebalance that reads the shares would budget as if it did not
        # exist — and deleting the diagnostic would drop it outright.
        diagnostic_only = (
            THREE_WAY.replace(
                "--exclude quantick-app --exclude quantick-engine",
                "--exclude quantick-app --exclude quantick-engine --exclude quantick-chart",
            )
            + """
  windows:
    runs-on: windows-latest
    steps:
      - name: Test Windows descriptor authority and local transport
        run: cargo test -p quantick-chart -- --nocapture
"""
        )
        problems = guard.problems(diagnostic_only, MEMBERS)
        self.assertEqual(MEMBERS, guard.covered(diagnostic_only, MEMBERS))
        self.assertTrue(
            any("quantick-chart" in line and "no Windows share owns" in line for line in problems)
        )

    def test_an_unnamed_step_does_not_inherit_the_share_above_it(self):
        # Steps may be written without a name. Carrying the previous step's
        # name forward would make the second run a second share and report a
        # crate tested twice, reddening a workflow that is correct.
        unnamed = (
            THREE_WAY
            + """
  windows:
    runs-on: windows-latest
    steps:
      - name: Test
        run: cargo test -p quantick-chart -- --nocapture
      - run: cargo test -p quantick-chart -- --nocapture
"""
        )
        self.assertEqual(
            [("Test", "cargo test -p quantick-chart -- --nocapture"),
             (None, "cargo test -p quantick-chart -- --nocapture")],
            guard.test_steps(guard.windows_jobs(unnamed)["windows"]),
        )

    def test_an_unnamed_step_still_counts_as_coverage(self):
        # It is a real run of those tests, so it must not read as a crate
        # nothing tests; it is simply not a share anyone budgeted for.
        unnamed = THREE_WAY.replace(
            "      - name: Test\n        run: cargo test -p quantick-app",
            "      - run: cargo test -p quantick-app",
        )
        self.assertIn("quantick-app", guard.covered(unnamed, MEMBERS))
        self.assertNotIn("quantick-app", guard.shares(unnamed, MEMBERS))

    def test_a_workspace_partition_has_no_problems(self):
        self.assertEqual([], guard.problems(THREE_WAY, MEMBERS))


class TheCommandCiRuns(unittest.TestCase):
    """`main()` is the whole contract with the `ci` job, so it has cases too.

    Without them, a `main()` edited to print its problems and fall through to
    `return 0` leaves every other case passing while the guard stops
    guarding — the exact silent failure this file exists to prevent.
    """

    @contextlib.contextmanager
    def _guarding(self, workflow_text):
        with tempfile.TemporaryDirectory() as directory:
            workflow = pathlib.Path(directory, "ci.yml")
            workflow.write_text(workflow_text, encoding="utf-8")
            with unittest.mock.patch.object(guard, "WORKFLOW", str(workflow)), \
                 unittest.mock.patch.object(guard, "workspace_members", lambda: MEMBERS):
                yield

    def test_a_broken_partition_exits_non_zero_and_says_so_on_stderr(self):
        overlapping = THREE_WAY.replace(
            "--exclude quantick-app --exclude quantick-engine",
            "--exclude quantick-app",
        )
        stderr = io.StringIO()
        with self._guarding(overlapping), contextlib.redirect_stderr(stderr):
            status = guard.main()
        self.assertEqual(1, status)
        self.assertIn("quantick-engine", stderr.getvalue())

    def test_a_whole_partition_exits_zero(self):
        with self._guarding(THREE_WAY), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(0, guard.main())

    def test_a_selector_naming_no_member_is_a_problem_not_a_traceback(self):
        typo = THREE_WAY.replace("-p quantick-engine", "-p quantick-engin")
        stderr = io.StringIO()
        with self._guarding(typo), contextlib.redirect_stderr(stderr):
            status = guard.main()
        self.assertEqual(1, status)
        self.assertIn("quantick-engin", stderr.getvalue())


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

    def test_the_real_shares_are_exactly_the_workspace(self):
        workflow = pathlib.Path(guard.WORKFLOW).read_text(encoding="utf-8")
        members = guard.workspace_members()
        self.assertEqual([], guard.problems(workflow, members))


if __name__ == "__main__":
    unittest.main(verbosity=2)
