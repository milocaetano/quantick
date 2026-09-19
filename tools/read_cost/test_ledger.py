#!/usr/bin/env python3
"""Offline tests for the read-cost ledger: rows, the median, the ceiling."""

import importlib.util
import os
import sys
import tempfile
import unittest

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


class MergeCommits(unittest.TestCase):
    def fetch(self, associated):
        def fake(*args):
            self.asked = args
            return associated

        return fake

    def test_the_pull_request_this_commit_merged_is_the_one_recorded(self):
        sha = "c" * 40
        associated = [
            {"number": 1, "merge_commit_sha": "d" * 40, "merged_at": "2026-09-01"},
            {"number": 2, "merge_commit_sha": sha, "merged_at": "2026-09-19"},
        ]

        self.assertEqual(ledger.merged_pull_for_commit(sha, self.fetch(associated)), 2)

    def test_a_commit_that_merged_nothing_gains_no_row(self):
        sha = "c" * 40
        open_pull = [{"number": 3, "merge_commit_sha": None, "merged_at": None}]
        # A pull request that merely contains the commit, merged elsewhere.
        containing = [{"number": 4, "merge_commit_sha": "e" * 40, "merged_at": "x"}]

        self.assertIsNone(ledger.merged_pull_for_commit(sha, self.fetch([])))
        self.assertIsNone(ledger.merged_pull_for_commit(sha, self.fetch(open_pull)))
        self.assertIsNone(ledger.merged_pull_for_commit(sha, self.fetch(containing)))


class RowsOnTheRecord(unittest.TestCase):
    def ledger(self):
        return written(
            HEADER
            + ledger.format_row(
                {
                    "pr": 11,
                    "date": "2026-09-19",
                    "branch": "feat/recorded",
                    "base": "main",
                    "read_cost": 5,
                    "changed": 1,
                    "top": "",
                }
            )
            + "\n"
        )

    def test_a_recorded_pull_request_is_found_and_an_absent_one_is_not(self):
        path = self.ledger()

        self.assertEqual(ledger.row_for(path, 11)["branch"], "feat/recorded")
        self.assertIsNone(ledger.row_for(path, 12))
        self.assertIsNone(ledger.row_for(os.path.join(HERE, "absent.md"), 11))

    def test_the_merge_check_answers_three_ways(self):
        path = self.ledger()
        merged = {}

        def stub(sha, fetch=None):
            return merged.get(sha)

        held, ledger.merged_pull_for_commit = ledger.merged_pull_for_commit, stub
        try:
            merged["recorded"] = 11
            merged["unrecorded"] = 12
            # An ordinary push that merged nothing owes no row, which is not
            # the same answer as a merge that left none.
            self.assertIsNone(ledger.verify(path, "plain"))
            self.assertTrue(ledger.verify(path, "recorded"))
            self.assertFalse(ledger.verify(path, "unrecorded"))
        finally:
            ledger.merged_pull_for_commit = held


class RecordOutcome(unittest.TestCase):
    """The one command here that acts rather than advises says what it did."""

    def run_with(self, outcome):
        held = ledger.record
        ledger.record = lambda repo, path, number: outcome
        try:
            return ledger.main(["--ledger", "unused.md", "record", "--repo", ".", "--pr", "1"])
        finally:
            ledger.record = held

    def test_a_recorded_row_and_an_already_recorded_one_both_succeed(self):
        # Idempotent on the pull-request number, so a second run is not a
        # failure - the row it would have written is already there.
        self.assertEqual(self.run_with(ledger.APPENDED), 0)
        self.assertEqual(self.run_with(ledger.ALREADY_RECORDED), 0)

    def test_a_row_that_could_not_be_written_exits_non_zero(self):
        self.assertEqual(self.run_with(ledger.FAILED), 1)


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
