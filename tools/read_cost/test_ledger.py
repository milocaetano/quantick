#!/usr/bin/env python3
"""Offline tests for the read-cost ledger: rows, the median, the ceiling."""

import copy
import contextlib
import io
import importlib.util
import os
import sys
import tempfile
import unittest
from unittest import mock

from test_measure import Repository, commit, run, write

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "quantick_read_cost_ledger_under_test", os.path.join(HERE, "ledger.py")
)
ledger = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ledger
SPEC.loader.exec_module(ledger)

HEADER = "\n".join(["# Ledger", "", ledger.HEADING, ledger.RULE, ""])


def report(files, production_lines=None):
    rows = [
        {"crate": "core", "lines": lines, "path": path, "roles": sorted(roles)}
        for path, lines, roles in files
    ]
    total = sum(row["lines"] for row in rows)
    return {
        "base": "0" * 40,
        "head": "1" * 40,
        "files": rows,
        "production_lines": total if production_lines is None else production_lines,
    }


def written(text):
    handle = tempfile.NamedTemporaryFile(
        "w", suffix=".md", delete=False, encoding="utf-8", newline="\n"
    )
    with handle:
        handle.write(text)
    return handle.name


class Rows(unittest.TestCase):
    def test_a_row_survives_formatting_and_parsing(self):
        row = {
            "pr": 42,
            "date": "2026-09-19",
            "branch": "feat/thing",
            "base": "main",
            "read_cost": 1234,
            "changed": 7,
            "top": "`crates/app/src/a.rs` (900)",
        }

        parsed = ledger.parse_rows(ledger.format_row(row))

        self.assertEqual(parsed, [row])

    def test_prose_and_malformed_lines_are_not_rows(self):
        text = "\n".join(
            [
                "| PR | Merged |",
                "| ---: | --- |",
                "| 42 | 2026-09-19 |",
                "Ordinary prose naming | 42 | is not a row.",
            ]
        )

        self.assertEqual(ledger.parse_rows(text), [])

    def test_a_pull_request_is_recorded_once(self):
        path = written(HEADER)
        row = {
            "pr": 7,
            "date": "2026-09-19",
            "branch": "fix/thing",
            "base": "main",
            "read_cost": 10,
            "changed": 1,
            "top": "",
        }

        first = ledger.append_row(path, row)
        second = ledger.append_row(path, dict(row, read_cost=99))
        rows = ledger.parse_rows(ledger.read(path))

        self.assertTrue(first)
        self.assertFalse(second)
        self.assertEqual([entry["read_cost"] for entry in rows], [10])

    def test_an_empty_top_column_is_a_dash_rather_than_a_gap(self):
        row = {
            "pr": 1,
            "date": "2026-09-19",
            "branch": "docs/thing",
            "base": "main",
            "read_cost": 0,
            "changed": 0,
            "top": "",
        }

        self.assertEqual(ledger.parse_rows(ledger.format_row(row))[0]["top"], "—")


class Derivation(unittest.TestCase):
    def test_the_row_carries_the_measured_number_and_the_largest_reference(self):
        measured = report(
            [
                ("crates/core/src/lib.rs", 10, ["touched"]),
                ("crates/core/src/big.rs", 400, ["referenced"]),
            ]
        )

        row = ledger.row_from_report(measured, 5, "feat/thing", "main", "2026-09-19")

        self.assertEqual(row["read_cost"], 410)
        self.assertEqual(row["changed"], 1)
        self.assertEqual(row["top"], "`crates/core/src/big.rs` (400)")


class MainHistory(unittest.TestCase):
    def setUp(self):
        self.repo = Repository()
        self.addCleanup(self.repo.close)
        self.repo.base_crate()
        write(self.repo.root, "crates/core/src/lib.rs", "pub fn old() {}\n")
        self.base = commit(self.repo.root, "base")
        write(self.repo.root, "crates/core/src/lib.rs", "pub fn first() {}\n")
        self.first = commit(self.repo.root, "first merged change")
        write(self.repo.root, "crates/core/src/lib.rs", "pub fn second() {}\n")
        self.second = commit(self.repo.root, "second merged change")
        run(self.repo.root, "git", "update-ref", "refs/remotes/origin/main", self.second)
        write(self.repo.root, "crates/core/src/lib.rs", "pub fn pending() {}\npub fn pending_too() {}\n")
        self.pending = commit(self.repo.root, "unmerged branch")
        self.path = os.path.join(self.repo.root, "ledger.md")
        header = HEADER + "<!-- read-cost-ceiling:v1 18000 -->\n"
        with open(self.path, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(header)
        self.pulls = {
            self.first: [self.pull(1, self.base, self.first)],
            self.second: [self.pull(2, self.first, self.second)],
            self.pending: [self.pull(3, self.second, self.pending)],
        }
        self.requests = []
        patch = mock.patch.object(ledger, "gh_json", side_effect=self.fetch)
        patch.start()
        self.addCleanup(patch.stop)

    def pull(self, number, base, head):
        return {"number": number, "merge_commit_sha": head,
                "merged_at": "2026-09-19T12:00:00Z",
                "base": {"ref": "main", "sha": base},
                "head": {"ref": f"feat/change-{number}", "sha": head}}

    def fetch(self, repo, *args):
        self.assertEqual(repo, self.repo.root)
        sha = args[-1].split("/")[-2]
        self.requests.append(sha)
        return self.pulls.get(sha, [])

    def execute(self, limit=30):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return ledger.main(["--ledger", self.path, "backfill", "--repo",
                                self.repo.root, "--limit", str(limit)])

    def test_only_main_history_is_recorded_and_a_repeat_is_byte_identical(self):
        self.assertEqual(self.execute(), 0)
        rows = ledger.parse_rows(ledger.read(self.path))
        self.assertEqual([row["pr"] for row in rows], [1, 2])
        self.assertEqual([row["read_cost"] for row in rows], [1, 1])
        self.assertEqual(rows[0]["date"], "2026-09-19")
        self.assertNotIn(self.pending, self.requests)
        self.assertEqual(ledger.ceiling_or_none(self.path), 18000)
        before = ledger.read(self.path)
        with mock.patch.object(ledger.MEASURE, "measure", side_effect=AssertionError):
            self.assertEqual(self.execute(), 0)
        self.assertEqual(ledger.read(self.path), before)

    def test_limit_bounds_main_commits_and_uses_the_recorded_pr_range(self):
        # A squash's PR head need not be its merge commit. Its entire range
        # must be measured, rather than the final individual rebased commit.
        self.pulls[self.second][0]["head"]["sha"] = self.pending
        self.assertEqual(self.execute(limit=1), 0)
        self.assertEqual(self.requests, [self.second])
        rows = ledger.parse_rows(ledger.read(self.path))
        self.assertEqual([row["pr"] for row in rows], [2])
        self.assertEqual(rows[0]["read_cost"], 2)

    def test_open_integration_and_merely_containing_prs_do_not_gain_rows(self):
        valid = self.pulls[self.first][0]
        wrong_merge = dict(valid, number=4, merge_commit_sha=self.pending)
        unmerged = dict(valid, number=5, merged_at=None)
        integration = copy.deepcopy(valid)
        integration["number"], integration["base"]["ref"] = 6, "feature/integration"
        self.pulls[self.first] = [wrong_merge, unmerged, integration, valid]
        self.pulls[self.second] = [integration]
        self.assertEqual(self.execute(), 0)
        self.assertEqual([r["pr"] for r in ledger.parse_rows(ledger.read(self.path))], [1])

    def test_lookup_and_measurement_failures_leave_the_entire_batch_unwritten(self):
        before = ledger.read(self.path)
        for failure in (OSError("offline"), ledger.MEASURE.ReadCostError("missing object")):
            with self.subTest(failure=failure):
                with mock.patch.object(ledger, "merged_pull_for_commit",
                                       side_effect=[self.pulls[self.first][0], failure]):
                    self.assertEqual(self.execute(), 1)
                self.assertEqual(ledger.read(self.path), before)
        with mock.patch.object(ledger.MEASURE, "measure", side_effect=[
                report([]), ledger.MEASURE.ReadCostError("bad measurement")]):
            self.assertEqual(self.execute(), 1)
        self.assertEqual(ledger.read(self.path), before)
        with mock.patch.object(ledger, "fetch_pull", return_value=False):
            self.assertEqual(self.execute(), 1)
        self.assertEqual(ledger.read(self.path), before)

    def test_missing_main_and_invalid_limits_do_not_write(self):
        before = ledger.read(self.path)
        run(self.repo.root, "git", "update-ref", "-d", "refs/remotes/origin/main")
        self.assertEqual(self.execute(), 1)
        with self.assertRaises(SystemExit):
            self.execute(limit=0)
        self.assertEqual(ledger.read(self.path), before)


class Ceiling(unittest.TestCase):
    def test_the_median_ignores_every_branch_the_ceiling_is_not_about(self):
        rows = ledger.parse_rows(
            "\n".join(
                ledger.format_row(
                    {
                        "pr": index,
                        "date": "2026-09-19",
                        "branch": branch,
                        "base": "main",
                        "read_cost": cost,
                        "changed": 1,
                        "top": "",
                    }
                )
                for index, (branch, cost) in enumerate(
                    [
                        ("feat/a", 1000),
                        ("fix/b", 3000),
                        ("feat/c", 8000),
                        ("docs/d", 900_000),
                        ("chore/e", 900_000),
                        ("campaign/f", 900_000),
                    ]
                )
            )
        )

        suggestion = ledger.suggested_ceiling(rows)

        self.assertEqual(suggestion["samples"], 3)
        self.assertEqual(suggestion["median"], 3000.0)
        self.assertEqual(suggestion["ceiling"], 3000)

    def test_an_even_sample_averages_the_middle_pair(self):
        self.assertEqual(ledger.median([1, 2, 3, 4]), 2.5)
        self.assertEqual(ledger.median([5]), 5.0)
        self.assertIsNone(ledger.median([]))

    def test_rounding_is_half_up_so_two_equal_halves_do_not_disagree(self):
        self.assertEqual(ledger.round_to_thousand(8500), 9000)
        self.assertEqual(ledger.round_to_thousand(9500), 10000)
        self.assertEqual(ledger.round_to_thousand(8499), 8000)
        self.assertEqual(ledger.round_to_thousand(18242.0), 18000)

    def test_no_feature_row_suggests_nothing_rather_than_zero(self):
        self.assertIsNone(ledger.suggested_ceiling([]))

    def test_a_missing_file_marker_or_number_reads_as_no_ceiling(self):
        self.assertIsNone(ledger.ceiling_or_none(os.path.join(HERE, "absent.md")))
        self.assertIsNone(ledger.ceiling_or_none(written("# Ledger\n\nNo marker.\n")))
        self.assertEqual(
            ledger.ceiling_or_none(written("<!-- read-cost-ceiling:v1 18000 -->\n")),
            18000,
        )

    def test_the_committed_ledger_records_a_ceiling_and_its_own_rows(self):
        path = os.path.join(HERE, "..", "..", ledger.DEFAULT_PATH)
        text = ledger.read(path)
        rows = ledger.parse_rows(text)
        recorded = ledger.ceiling_or_none(path)

        self.assertIsNotNone(recorded)
        # The marker is what the hook reads and the prose is what a person
        # reads. A ledger whose two halves name different numbers produces a
        # warning nobody can reproduce from the file it cites.
        self.assertIn(f"**{recorded:,} production lines.**", text)
        self.assertGreaterEqual(len(rows), 30)
        self.assertEqual(len({row["pr"] for row in rows}), len(rows))


if __name__ == "__main__":
    unittest.main()
