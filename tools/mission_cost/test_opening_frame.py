#!/usr/bin/env python3
"""Offline tests for the session-opening frame reading."""

import importlib.util
import json
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "fixtures", "transcripts")


def load(name):
    spec = importlib.util.spec_from_file_location(
        f"quantick_mission_cost_{name}", os.path.join(HERE, f"{name}.py")
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


opening_frame = load("opening_frame")

FIRST_SESSION = "aaaaaaaa-0000-4000-8000-000000000001"


class Openings(unittest.TestCase):
    def setUp(self):
        self.rows = opening_frame.openings([FIXTURES])

    def test_the_first_usage_line_is_the_opening_and_a_line_without_one_is_skipped(self):
        first = [row for row in self.rows if row["session"] == FIRST_SESSION]
        self.assertEqual(len(first), 1)
        # The fixture's first line is a user turn with no usage block; the
        # opening is the assistant turn after it: 100 + 200 + 300.
        self.assertEqual(first[0]["prompt_tokens"], 600)
        self.assertEqual(first[0]["at"], "2026-01-01T00:10:00+00:00")

    def test_only_main_thread_transcripts_are_read(self):
        # The fixture has a session whose only transcripts are subagents.
        sessions = {row["session"] for row in self.rows}
        self.assertNotIn("cccccccc-0000-4000-8000-000000000003", sessions)

    def test_the_rows_come_back_oldest_first(self):
        stamps = [row["at"] for row in self.rows]
        self.assertEqual(stamps, sorted(stamps))

    def test_nothing_but_the_five_fields_reaches_a_row(self):
        raw = json.dumps(self.rows)
        self.assertNotIn("SHOULD-NOT-BE-READ", raw)
        self.assertNotIn("MUST-NOT-APPEAR", raw)
        self.assertEqual(
            sorted(self.rows[0].keys()), ["at", "prompt_tokens", "root", "session"]
        )


class Build(unittest.TestCase):
    def rows(self):
        return [
            {"root": "r", "session": "a", "at": "2026-01-01T00:00:00+00:00", "prompt_tokens": 10},
            {"root": "r", "session": "b", "at": "2026-01-02T00:00:00+00:00", "prompt_tokens": 20},
            {"root": "r", "session": "c", "at": "2026-01-03T00:00:00+00:00", "prompt_tokens": 30},
        ]

    def test_since_drops_the_sessions_that_opened_earlier(self):
        found = opening_frame.build(self.rows(), "2026-01-02T00:00:00+00:00", None)
        self.assertEqual([row["session"] for row in found["sessions"]], ["b", "c"])
        self.assertEqual(found["summary"]["n"], 2)

    def test_without_a_pivot_there_is_no_verdict(self):
        found = opening_frame.build(self.rows(), None, None)
        self.assertNotIn("result", found)

    def test_a_pivot_grades_the_split_with_the_registered_thresholds(self):
        found = opening_frame.build(self.rows(), None, "2026-01-03T00:00:00+00:00")
        self.assertEqual(found["result"]["thresholds"]["min_group_n"], 5)
        self.assertEqual(found["result"]["verdict"], "inconclusive")

    def test_the_same_moment_spelled_two_ways_splits_the_same_way(self):
        rows = self.rows()
        rows[1]["at"] = "2026-01-02T00:00:00Z"
        with_zulu = opening_frame.build(rows, None, "2026-01-02T00:00:00+00:00")
        self.assertEqual([r["session"] for r in with_zulu["sessions"]], ["a", "b", "c"])
        kept = opening_frame.build(rows, "2026-01-02T00:00:00+00:00", None)
        self.assertEqual([r["session"] for r in kept["sessions"]], ["b", "c"])

    def test_an_instant_that_is_not_one_is_refused_rather_than_compared(self):
        with self.assertRaises(opening_frame.InstantError):
            opening_frame.build(self.rows(), "yesterday", None)
        with self.assertRaises(opening_frame.InstantError):
            opening_frame.build(self.rows(), None, "soon")

    def test_the_report_says_it_is_not_the_registered_comparison(self):
        found = opening_frame.build(self.rows(), None, None)
        self.assertIs(found["registered_comparison"], False)
        self.assertEqual(found["metric"], "opening_prompt_tokens")

    def test_the_reading_carries_the_input_set_it_came_from(self):
        found = opening_frame.build(self.rows(), None, None)
        self.assertEqual(found["inputs"]["roots"], ["r"])
        self.assertEqual(found["inputs"]["transcripts"], 3)
        self.assertTrue(found["inputs"]["digest"].startswith("sha256:"))

    def test_the_digest_covers_every_opening_found_not_only_the_kept_ones(self):
        """`--since` narrows the reading; it must not narrow its provenance.

        The transcript directory is live and append-only. A digest taken after
        the filter would be blind to a session that opened before the floor,
        and two runs over a directory that grew would agree while the reading
        behind them had changed.
        """
        whole = opening_frame.build(self.rows(), None, None)
        narrowed = opening_frame.build(self.rows(), "2026-01-02T00:00:00+00:00", None)
        self.assertEqual(narrowed["summary"]["n"], 2)
        self.assertEqual(narrowed["inputs"]["digest"], whole["inputs"]["digest"])
        self.assertEqual(narrowed["inputs"]["transcripts"], 3)

    def test_one_more_session_changes_the_digest(self):
        rows = self.rows()
        before = opening_frame.build(rows, None, None)["inputs"]["digest"]
        rows.append(
            {
                "root": "r",
                "session": "d",
                "at": "2026-01-04T00:00:00+00:00",
                "prompt_tokens": 40,
            }
        )
        self.assertNotEqual(opening_frame.build(rows, None, None)["inputs"]["digest"], before)

    def test_the_digest_does_not_depend_on_the_order_rows_arrive_in(self):
        rows = self.rows()
        forwards = opening_frame.build(rows, None, None)["inputs"]["digest"]
        backwards = opening_frame.build(list(reversed(rows)), None, None)["inputs"]["digest"]
        self.assertEqual(forwards, backwards)


class Command(unittest.TestCase):
    def test_the_command_writes_lf_terminated_canonical_json(self):
        import tempfile

        with tempfile.TemporaryDirectory() as where:
            out = os.path.join(where, "frame.json")
            code = opening_frame.main(["--transcripts", FIXTURES, "--out", out])
            self.assertEqual(code, 0)
            with open(out, "rb") as stream:
                raw = stream.read()
        self.assertNotIn(b"\r\n", raw)
        self.assertTrue(raw.endswith(b"\n"))
        self.assertIn("sessions", json.loads(raw.decode("utf-8")))

    def test_a_missing_transcript_directory_is_refused(self):
        with self.assertRaises(SystemExit):
            opening_frame.main(["--transcripts", os.path.join(HERE, "no-such-dir")])


if __name__ == "__main__":
    unittest.main()
