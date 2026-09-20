#!/usr/bin/env python3
"""Offline tests for what a person is shown: the comment and the warning."""

import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "quantick_read_cost_report_under_test", os.path.join(HERE, "report.py")
)
report = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = report
SPEC.loader.exec_module(report)

CEILING_FILE = "<!-- read-cost-ceiling:v1 1000 -->\n"


def measured(files):
    rows = [
        {"crate": "core", "lines": lines, "path": path, "roles": sorted(roles)}
        for path, lines, roles in files
    ]
    return {
        "base": "a" * 40,
        "head": "b" * 40,
        "files": rows,
        "production_lines": sum(row["lines"] for row in rows),
    }


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as stream:
        stream.write(text)


def run(root, *args):
    return subprocess.run(
        args,
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    ).stdout.strip()


def captured(argv):
    held, sys.stdout = sys.stdout, io.StringIO()
    try:
        status = report.main(argv)
        return status, sys.stdout.getvalue()
    finally:
        sys.stdout = held


class Comment(unittest.TestCase):
    def setUp(self):
        self.report = measured(
            [("crates/core/src/lib.rs", 10, ["touched"])]
            + [
                (f"crates/core/src/r{index}.rs", 100 - index, ["referenced"])
                for index in range(7)
            ]
        )

    def test_the_marker_leads_so_the_comment_can_be_found_again(self):
        body = report.comment(self.report)

        self.assertTrue(body.startswith(report.MARKER))
        self.assertIn("### Read cost: 689 production lines", body)
        self.assertIn("1 changed file(s) pull in 7 referenced file(s)", body)

    def test_only_the_top_five_referenced_files_are_listed(self):
        body = report.comment(self.report)

        listed = [line for line in body.splitlines() if line.startswith("| `")]
        self.assertEqual(len(listed), 5)
        self.assertIn("| `crates/core/src/r0.rs` | 100 |", listed[0])
        self.assertNotIn("r5.rs", body)

    def test_the_ceiling_is_named_only_when_there_is_one_to_name(self):
        self.assertNotIn("ceiling", report.comment(self.report))
        self.assertIn("within it", report.comment(self.report, 10_000))
        self.assertIn("over it", report.comment(self.report, 100))

    def test_a_change_that_pulls_in_nothing_says_so(self):
        body = report.comment(measured([("crates/core/src/lib.rs", 4, ["touched"])]))

        self.assertIn("No file is pulled in by reference alone.", body)
        self.assertNotIn("| Referenced file |", body)


class CeilingWarning(unittest.TestCase):
    def setUp(self):
        self.over = measured(
            [
                ("crates/core/src/lib.rs", 10, ["touched"]),
                ("crates/core/src/big.rs", 400, ["referenced"]),
            ]
        )

    def test_a_feature_branch_over_the_ceiling_is_told_what_to_detach(self):
        text = report.ceiling_warning(self.over, 100, "feat/thing")

        self.assertIn("410 production lines", text)
        self.assertIn("over the 100 ceiling", text)
        self.assertIn("Nothing is blocked", text)
        self.assertIn("crates/core/src/big.rs", text)

    def test_nothing_is_said_where_the_ceiling_does_not_apply(self):
        self.assertIsNone(report.ceiling_warning(self.over, 100, "docs/thing"))
        self.assertIsNone(report.ceiling_warning(self.over, 100, "campaign/thing"))
        self.assertIsNone(report.ceiling_warning(self.over, 10_000, "feat/thing"))
        self.assertIsNone(report.ceiling_warning(self.over, None, "feat/thing"))

    def test_the_ceiling_itself_is_not_over_the_ceiling(self):
        self.assertIsNone(report.ceiling_warning(self.over, 410, "fix/thing"))
        self.assertIsNotNone(report.ceiling_warning(self.over, 409, "fix/thing"))


class RowReminder(unittest.TestCase):
    def test_the_reminder_names_the_pull_request_and_the_command(self):
        text = report.row_reminder(42)

        self.assertIn("#42", text)
        self.assertIn(f"{report.ROW_COMMAND} 42", text)

    def test_two_notes_arrive_as_one_payload_and_none_as_nothing(self):
        joined = report.advisory(["first", None, "second"])

        self.assertEqual(joined, "first\n\nsecond")
        self.assertIsNone(report.advisory([None, None]))
        self.assertIsNone(report.advisory([]))


class Command(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = self.tmp.name

    def ledger(self, text=CEILING_FILE, name="ledger.md"):
        path = os.path.join(self.root, name)
        write(path, text)
        return path

    def json_report(self, data):
        path = os.path.join(self.root, "read-cost.json")
        write(path, json.dumps(data))
        return path

    def repository(self):
        run(self.root, "git", "init", "--quiet")
        write(os.path.join(self.root, "Cargo.toml"), '[workspace]\nmembers = ["crates/*"]\n')
        write(
            os.path.join(self.root, "crates", "core", "Cargo.toml"),
            '[package]\nname = "core"\n',
        )
        write(os.path.join(self.root, "crates", "core", "src", "helper.rs"), "pub fn helper() {}\n")
        write(
            os.path.join(self.root, "crates", "core", "src", "lib.rs"),
            "mod helper;\npub fn before() {}\n",
        )
        run(self.root, "git", "add", "-A")
        run(
            self.root,
            "git",
            "-c",
            "user.name=Read Cost Fixture",
            "-c",
            "user.email=read-cost@example.invalid",
            "commit",
            "-m",
            "base",
        )
        base = run(self.root, "git", "rev-parse", "HEAD")
        write(
            os.path.join(self.root, "crates", "core", "src", "lib.rs"),
            "mod helper;\npub fn before() {}\npub fn after() {}\n",
        )
        run(self.root, "git", "add", "-A")
        run(
            self.root,
            "git",
            "-c",
            "user.name=Read Cost Fixture",
            "-c",
            "user.email=read-cost@example.invalid",
            "commit",
            "-m",
            "head",
        )
        return base, run(self.root, "git", "rev-parse", "HEAD")

    def test_marker_mode_prints_the_key_the_comment_carries(self):
        status, printed = captured(["marker"])

        self.assertEqual(status, 0)
        self.assertEqual(printed.strip(), report.MARKER)
        self.assertTrue(
            report.comment(measured([("crates/core/src/lib.rs", 1, ["touched"])]))
            .startswith(printed.strip())
        )

    def test_comment_mode_renders_the_ceiling_from_the_ledger(self):
        path = self.json_report(
            measured([("crates/core/src/lib.rs", 10, ["touched"])])
        )

        status, body = captured(["comment", "--json", path, "--ledger", self.ledger()])

        self.assertEqual(status, 0)
        self.assertIn("Feature-branch ceiling: 1000", body)

    def test_comment_mode_asks_for_the_row_only_while_it_is_missing(self):
        path = self.json_report(
            measured([("crates/core/src/lib.rs", 10, ["touched"])])
        )
        recorded = self.ledger(
            CEILING_FILE
            + "\n| 42 | 2026-09-19 | feat/thing | main | 10 | 1 | \u2014 |\n",
            name="recorded.md",
        )

        _, missing = captured(
            ["comment", "--json", path, "--pr", "42", "--ledger", self.ledger()]
        )
        _, present = captured(
            ["comment", "--json", path, "--pr", "42", "--ledger", recorded]
        )

        self.assertIn(f"{report.ROW_COMMAND} 42", missing)
        self.assertNotIn(report.ROW_COMMAND, present)

    def test_warn_mode_asks_for_a_missing_row_without_measuring_anything(self):
        base, head = self.repository()

        status, printed = captured(
            [
                "warn",
                "--repo",
                self.root,
                "--base",
                base,
                "--head",
                head,
                "--branch",
                "docs/thing",
                "--pr",
                "42",
                "--ledger",
                self.ledger(),
            ]
        )

        self.assertEqual(status, 0)
        self.assertIn("#42 has no row", json.loads(printed))

    def test_warn_mode_prints_a_json_string_a_hook_can_paste(self):
        base, head = self.repository()

        status, printed = captured(
            [
                "warn",
                "--repo",
                self.root,
                "--base",
                base,
                "--head",
                head,
                "--branch",
                "feat/thing",
                "--ledger",
                self.ledger("<!-- read-cost-ceiling:v1 1 -->\n"),
            ]
        )

        self.assertEqual(status, 0)
        decoded = json.loads(printed)
        self.assertIn("over the 1 ceiling", decoded)
        self.assertIn("crates/core/src/helper.rs", decoded)

    def test_warn_mode_is_silent_where_it_has_nothing_to_report(self):
        base, head = self.repository()
        arguments = ["warn", "--repo", self.root, "--base", base, "--head", head]

        under = captured(arguments + ["--branch", "feat/thing", "--ledger", self.ledger()])
        other = captured(
            arguments
            + ["--branch", "docs/thing", "--ledger", self.ledger("<!-- read-cost-ceiling:v1 1 -->\n")]
        )
        absent = captured(
            arguments
            + ["--branch", "feat/thing", "--ledger", os.path.join(self.root, "none.md")]
        )
        broken = captured(
            [
                "warn",
                "--repo",
                self.root,
                "--base",
                "0" * 40,
                "--head",
                head,
                "--branch",
                "feat/thing",
                "--ledger",
                self.ledger("<!-- read-cost-ceiling:v1 1 -->\n"),
            ]
        )

        self.assertEqual([status for status, _ in (under, other, absent, broken)], [0, 0, 0, 0])
        self.assertEqual([text for _, text in (under, other, absent, broken)], ["", "", "", ""])


if __name__ == "__main__":
    unittest.main()
