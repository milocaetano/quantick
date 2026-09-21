#!/usr/bin/env python3
"""Offline tests for the transcript reader's field filter and idle bound.

The filter is the privacy boundary campaign #563 depends on, so these tests
assert the negative: bait strings planted in the fixture must not survive into
a record, a repr or a rendered report.
"""

import datetime
import importlib.util
import os
import sys
import tempfile
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


transcripts = load("transcripts")

ALPHA = "aaaaaaaa-0000-4000-8000-000000000001"
BETA = "bbbbbbbb-0000-4000-8000-000000000002"
GAMMA = "cccccccc-0000-4000-8000-000000000003"
DELTA = "dddddddd-0000-4000-8000-000000000004"
EPSILON = "eeeeeeee-0000-4000-8000-000000000005"

# Every bait string planted in the fixture. A reader widened by one field
# starts printing one of these.
BAIT = (
    "MUST-NOT-APPEAR",
    "SHOULD-NOT-BE-READ",
)


def by_relative(found):
    return {item.relative: item for item in found}


class Discovery(unittest.TestCase):
    def setUp(self):
        self.found, self.skipped = transcripts.discover(FIXTURES)
        self.index = by_relative(self.found)

    def test_finds_every_transcript_in_both_layouts(self):
        self.assertEqual(
            sorted(self.index),
            [
                f"{ALPHA}.jsonl",
                f"{ALPHA}/subagents/agent-1111.jsonl",
                f"{ALPHA}/subagents/agent-2222.jsonl",
                f"{BETA}.jsonl",
                f"{GAMMA}/subagents/agent-3333.jsonl",
                f"{DELTA}.jsonl",
                f"{EPSILON}.jsonl",
            ],
        )
        self.assertEqual(self.skipped, [])

    def test_discovery_is_ordered_so_two_runs_agree(self):
        again, _ = transcripts.discover(FIXTURES)
        self.assertEqual(
            [item.relative for item in self.found],
            [item.relative for item in again],
        )
        self.assertEqual(
            [item.relative for item in self.found],
            sorted(item.relative for item in self.found),
        )

    def test_a_root_file_is_the_main_thread_of_its_session(self):
        item = self.index[f"{ALPHA}.jsonl"]
        self.assertEqual(item.kind, "main")
        self.assertEqual(item.session, ALPHA)

    def test_a_subagents_file_is_a_subagent_of_the_owning_session(self):
        item = self.index[f"{ALPHA}/subagents/agent-1111.jsonl"]
        self.assertEqual(item.kind, "subagent")
        self.assertEqual(item.session, ALPHA)

    def test_a_session_directory_without_a_root_file_still_has_subagents(self):
        item = self.index[f"{GAMMA}/subagents/agent-3333.jsonl"]
        self.assertEqual(item.kind, "subagent")
        self.assertEqual(item.session, GAMMA)
        self.assertNotIn(f"{GAMMA}.jsonl", self.index)

    def test_an_unrecognised_layout_is_reported_rather_than_guessed(self):
        with tempfile.TemporaryDirectory() as root:
            stray = os.path.join(root, "session-x", "notes", "stray.jsonl")
            os.makedirs(os.path.dirname(stray))
            with open(stray, "w", encoding="utf-8", newline="\n") as stream:
                stream.write("{}\n")
            found, skipped = transcripts.discover(root)
            self.assertEqual(found, [])
            self.assertEqual(skipped, ["session-x/notes/stray.jsonl"])


LOADED = {
    "type": "assistant",
    "sessionId": "SHOULD-NOT-BE-READ",
    "cwd": "C:/src/SHOULD-NOT-BE-READ",
    "gitBranch": "feat/SHOULD-NOT-BE-READ",
    "timestamp": "2026-01-01T00:10:00.000Z",
    "message": {
        "role": "assistant",
        "model": "SHOULD-NOT-BE-READ",
        "content": "FIXTURE-ANSWER-MUST-NOT-APPEAR",
        "usage": {
            "input_tokens": 100,
            "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 300,
            "output_tokens": 40,
            "service_tier": "SHOULD-NOT-BE-READ",
            "speed": "SHOULD-NOT-BE-READ",
            "iterations": 7,
        },
    },
}

USAGE_LINE = (
    '{"timestamp":"%s","message":{"usage":{"output_tokens":%d}}}\n'
)


def write_lines(root, *lines):
    path = os.path.join(root, "s.jsonl")
    with open(path, "w", encoding="utf-8", newline="\n") as stream:
        for line in lines:
            stream.write(line)
    return path


class FieldFilter(unittest.TestCase):
    def setUp(self):
        self.found, _ = transcripts.discover(FIXTURES)
        self.index = by_relative(self.found)

    def test_a_record_carries_exactly_the_five_registered_values(self):
        record = transcripts.record_from(LOADED)
        self.assertEqual(
            record._fields,
            (
                "timestamp",
                "input_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
                "output_tokens",
            ),
        )
        self.assertEqual(len(record), 5)

    def test_the_filter_leaves_no_trace_of_the_line_it_read(self):
        text = repr(transcripts.record_from(LOADED))
        for bait in BAIT:
            self.assertNotIn(bait, text)

    def test_nothing_a_transcript_retains_carries_a_bait_string(self):
        # Stronger than checking the records, because there are no records to
        # check: this reads everything the object still holds after the fold.
        for item in self.found:
            text = repr(vars(item))
            for bait in BAIT:
                self.assertNotIn(bait, text, item.relative)

    def test_uncounted_usage_keys_are_discarded(self):
        # The line carries service_tier, speed and iterations inside usage.
        # The method counts four counters and nothing else.
        record = transcripts.record_from(LOADED)
        self.assertEqual(record.input_tokens, 100)
        self.assertEqual(record.cache_creation_input_tokens, 200)
        self.assertEqual(record.cache_read_input_tokens, 300)
        self.assertEqual(record.output_tokens, 40)

    def test_a_line_without_usage_is_not_a_record(self):
        self.assertIsNone(
            transcripts.record_from({"timestamp": "2026-01-01T00:00:00Z"})
        )
        self.assertIsNone(transcripts.record_from({"message": {"role": "user"}}))
        self.assertIsNone(transcripts.record_from("not an object"))

    def test_a_line_without_usage_is_seen_but_contributes_nothing(self):
        item = self.index[f"{ALPHA}.jsonl"]
        self.assertEqual(item.lines_seen, 5)
        self.assertEqual(item.requests, 4)

    def test_a_missing_counter_reads_as_zero(self):
        with tempfile.TemporaryDirectory() as root:
            path = write_lines(root, USAGE_LINE % ("2026-01-01T00:00:00.000Z", 3))
            item = transcripts.read(path, "s.jsonl", "s", "main")
            self.assertEqual(item.totals()["input_tokens"], 0)
            self.assertEqual(item.totals()["output_tokens"], 3)

    def test_an_unparsable_line_is_counted_rather_than_raised(self):
        with tempfile.TemporaryDirectory() as root:
            path = write_lines(
                root,
                "not json at all\n",
                USAGE_LINE % ("2026-01-01T00:00:00.000Z", 3),
            )
            item = transcripts.read(path, "s.jsonl", "s", "main")
            self.assertEqual(item.unparsed, 1)
            self.assertEqual(item.requests, 1)

    def test_a_usage_line_without_a_timestamp_is_not_counted(self):
        # The method takes a timestamp and four counters. A usage object with
        # no timestamp cannot be placed in a window, so it is not a record.
        with tempfile.TemporaryDirectory() as root:
            path = write_lines(root, '{"message":{"usage":{"output_tokens":3}}}\n')
            item = transcripts.read(path, "s.jsonl", "s", "main")
            self.assertEqual(item.requests, 0)
            self.assertEqual(item.undated, 1)

    def test_a_line_out_of_order_is_counted_rather_than_folded(self):
        # A transcript is an append-only log, so this should not happen. If it
        # ever does, a negative gap must not reach the wall clock.
        with tempfile.TemporaryDirectory() as root:
            path = write_lines(
                root,
                USAGE_LINE % ("2026-01-01T00:05:00.000Z", 1),
                USAGE_LINE % ("2026-01-01T00:00:00.000Z", 1),
            )
            item = transcripts.read(path, "s.jsonl", "s", "main")
            self.assertEqual(item.out_of_order, 1)
            self.assertEqual(item.requests, 2)
            self.assertEqual(item.active_seconds(), 0.0)
            self.assertEqual(
                item.first(),
                transcripts.parse_timestamp("2026-01-01T00:00:00.000Z"),
            )


class Totals(unittest.TestCase):
    def setUp(self):
        self.found, _ = transcripts.discover(FIXTURES)
        self.index = by_relative(self.found)

    def test_totals_sum_the_four_kinds_separately(self):
        self.assertEqual(
            self.index[f"{ALPHA}.jsonl"].totals(),
            {
                "requests": 4,
                "input_tokens": 460,
                "cache_creation_input_tokens": 230,
                "cache_read_input_tokens": 1260,
                "output_tokens": 220,
                "billable_tokens": 2170,
            },
        )

    def test_billable_tokens_is_the_sum_of_all_four(self):
        totals = self.index[f"{ALPHA}/subagents/agent-1111.jsonl"].totals()
        self.assertEqual(
            totals["billable_tokens"],
            totals["input_tokens"]
            + totals["cache_creation_input_tokens"]
            + totals["cache_read_input_tokens"]
            + totals["output_tokens"],
        )


class IdleBound(unittest.TestCase):
    def setUp(self):
        self.found, _ = transcripts.discover(FIXTURES)
        self.index = by_relative(self.found)

    def test_the_registered_bound_is_three_hundred_seconds(self):
        self.assertEqual(transcripts.IDLE_GAP_SECONDS, 300)

    def test_a_gap_longer_than_the_bound_contributes_nothing(self):
        # 00:10:00 to 00:10:30 is 30s, 00:10:30 to 00:15:00 is 270s, and
        # 00:15:00 to 00:59:00 is 2640s, which the bound drops entirely.
        self.assertEqual(self.index[f"{ALPHA}.jsonl"].active_seconds(), 300.0)

    def test_a_gap_within_the_bound_contributes_its_whole_length(self):
        self.assertEqual(
            self.index[f"{ALPHA}/subagents/agent-1111.jsonl"].active_seconds(),
            60.0,
        )

    def test_a_single_request_contributes_no_active_time(self):
        self.assertEqual(
            self.index[f"{ALPHA}/subagents/agent-2222.jsonl"].active_seconds(),
            0.0,
        )

    def test_the_bound_drops_only_the_gap_and_keeps_the_requests(self):
        # 00:59:00 is 2,640 seconds after 00:15:00, so the bound drops that gap
        # whole. The request itself still counts: four requests, 300 seconds.
        item = self.index[f"{ALPHA}.jsonl"]
        self.assertEqual(item.requests, 4)
        self.assertEqual(item.active_seconds(), 300.0)
        self.assertEqual(
            (item.last() - item.first()).total_seconds(), 2940.0
        )


class Union(unittest.TestCase):
    def test_overlapping_intervals_are_counted_once(self):
        start = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
        step = datetime.timedelta(seconds=1)
        spans = [
            (start, start + 100 * step),
            (start + 50 * step, start + 150 * step),
            (start + 300 * step, start + 310 * step),
        ]
        self.assertEqual(transcripts.union_seconds(spans), 160.0)

    def test_an_empty_set_of_intervals_is_zero(self):
        self.assertEqual(transcripts.union_seconds([]), 0.0)

    def test_touching_intervals_merge(self):
        start = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
        step = datetime.timedelta(seconds=1)
        spans = [(start, start + 10 * step), (start + 10 * step, start + 20 * step)]
        self.assertEqual(transcripts.union_seconds(spans), 20.0)


if __name__ == "__main__":
    unittest.main()
