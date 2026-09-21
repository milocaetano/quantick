#!/usr/bin/env python3
"""Offline tests for mission assignment and the main/subagent split."""

import importlib.util
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "fixtures")
TRANSCRIPTS = os.path.join(FIXTURES, "transcripts")


def load(name):
    spec = importlib.util.spec_from_file_location(
        f"quantick_mission_cost_{name}", os.path.join(HERE, f"{name}.py")
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


transcripts = load("transcripts")
attribution = load("attribution")

ALPHA = "aaaaaaaa-0000-4000-8000-000000000001"
BETA = "bbbbbbbb-0000-4000-8000-000000000002"
GAMMA = "cccccccc-0000-4000-8000-000000000003"
DELTA = "dddddddd-0000-4000-8000-000000000004"
EPSILON = "eeeeeeee-0000-4000-8000-000000000005"


def fixture_assignment():
    found, _ = transcripts.discover(TRANSCRIPTS)
    sessions = attribution.sessions_from(found)
    missions = attribution.load_registry(os.path.join(FIXTURES, "missions.json"))
    return sessions, missions, attribution.assign(sessions, missions)


class RegistrySchema(unittest.TestCase):
    def test_the_fixture_registry_loads(self):
        missions = attribution.load_registry(os.path.join(FIXTURES, "missions.json"))
        self.assertEqual(
            [mission.branch for mission in missions],
            ["feat/fixture-alpha", "feat/fixture-beta"],
        )
        self.assertEqual(missions[0].pr, 9001)
        self.assertEqual(missions[0].group, "before")
        self.assertEqual(missions[1].group, "after")

    def test_an_unknown_schema_is_refused(self):
        with self.assertRaises(attribution.RegistryError):
            attribution.parse_registry({"schema": 2, "missions": []})

    def test_a_duplicate_branch_is_refused(self):
        entry = {"branch": "feat/x", "pr": 1}
        with self.assertRaises(attribution.RegistryError):
            attribution.parse_registry({"schema": 1, "missions": [entry, dict(entry)]})

    def test_a_session_declared_twice_is_refused(self):
        with self.assertRaises(attribution.RegistryError):
            attribution.parse_registry(
                {
                    "schema": 1,
                    "missions": [
                        {"branch": "feat/x", "pr": 1, "sessions": ["s1"]},
                        {"branch": "feat/y", "pr": 2, "sessions": ["s1"]},
                    ],
                }
            )

    def test_an_unknown_group_is_refused(self):
        with self.assertRaises(attribution.RegistryError):
            attribution.parse_registry(
                {
                    "schema": 1,
                    "missions": [{"branch": "feat/x", "pr": 1, "group": "during"}],
                }
            )

    def test_a_mission_without_a_branch_is_refused(self):
        with self.assertRaises(attribution.RegistryError):
            attribution.parse_registry({"schema": 1, "missions": [{"pr": 1}]})


class Sessions(unittest.TestCase):
    def test_a_session_groups_its_main_thread_and_its_subagents(self):
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        self.assertEqual(
            sorted(sessions), sorted([ALPHA, BETA, GAMMA, DELTA, EPSILON])
        )
        alpha = sessions[ALPHA]
        self.assertEqual([item.relative for item in alpha.main], [f"{ALPHA}.jsonl"])
        self.assertEqual(
            [item.relative for item in alpha.subagents],
            [
                f"{ALPHA}/subagents/agent-1111.jsonl",
                f"{ALPHA}/subagents/agent-2222.jsonl",
            ],
        )

    def test_a_session_without_a_root_file_has_no_main_thread(self):
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        self.assertEqual(sessions[GAMMA].main, [])
        self.assertEqual(len(sessions[GAMMA].subagents), 1)


class Assignment(unittest.TestCase):
    def setUp(self):
        self.sessions, self.missions, self.placed = fixture_assignment()

    def test_a_declared_session_wins_over_the_window_rule(self):
        # Epsilon's timestamps are a month outside alpha's window, and alpha
        # declares it. Declaration is rule one.
        self.assertEqual(
            self.placed.assigned[EPSILON],
            ("feat/fixture-alpha", "declared"),
        )

    def test_a_session_inside_exactly_one_window_is_assigned_to_it(self):
        self.assertEqual(
            self.placed.assigned[ALPHA], ("feat/fixture-alpha", "window")
        )

    def test_a_session_overlapping_two_windows_is_shared_not_split(self):
        self.assertNotIn(BETA, self.placed.assigned)
        self.assertEqual(
            self.placed.shared[BETA],
            ["feat/fixture-alpha", "feat/fixture-beta"],
        )

    def test_a_session_outside_every_window_is_unassigned(self):
        self.assertNotIn(DELTA, self.placed.assigned)
        self.assertIn(DELTA, self.placed.unassigned)

    def test_subagents_inherit_their_session_assignment(self):
        self.assertEqual(
            self.placed.assigned[GAMMA], ("feat/fixture-beta", "window")
        )

    def test_every_session_lands_in_exactly_one_bucket(self):
        placed = (
            set(self.placed.assigned)
            | set(self.placed.shared)
            | set(self.placed.unassigned)
        )
        self.assertEqual(placed, set(self.sessions))
        self.assertEqual(
            len(placed),
            len(self.placed.assigned)
            + len(self.placed.shared)
            + len(self.placed.unassigned),
        )


class EdgeCases(unittest.TestCase):
    def test_a_session_with_no_usage_lines_is_unassigned_not_a_crash(self):
        # A transcript opened and abandoned has no timestamps to place it by.
        empty = transcripts.Transcript("nowhere.jsonl", "nowhere.jsonl", "n", "main")
        sessions = attribution.sessions_from([empty])
        missions = attribution.load_registry(os.path.join(FIXTURES, "missions.json"))
        placed = attribution.assign(sessions, missions)
        self.assertEqual(placed.unassigned, ["n"])

    def test_a_mission_with_no_window_at_all_claims_nothing_by_time(self):
        # An unbounded window would claim every session in the directory,
        # which is a worse answer than saying the mission has no window.
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        missions = attribution.parse_registry(
            {"schema": 1, "missions": [{"branch": "feat/no-window", "pr": 1}]}
        )
        placed = attribution.assign(sessions, missions)
        self.assertEqual(placed.assigned, {})
        self.assertEqual(len(placed.unassigned), len(sessions))


class MainVersusSubagent(unittest.TestCase):
    def setUp(self):
        self.sessions, self.missions, self.placed = fixture_assignment()
        self.totals = attribution.totals_by_mission(
            self.sessions, self.missions, self.placed
        )

    def test_the_two_threads_are_reported_separately(self):
        alpha = self.totals["feat/fixture-alpha"]
        # Main: session alpha (460/230/1260/220 over four requests) plus the
        # declared session epsilon (2000/0/0/2000 over two).
        self.assertEqual(
            alpha["main_thread"],
            {
                "requests": 6,
                "input_tokens": 2460,
                "cache_creation_input_tokens": 230,
                "cache_read_input_tokens": 1260,
                "output_tokens": 2220,
                "billable_tokens": 6170,
            },
        )
        # Subagents: agent-1111 twice and agent-2222 once.
        self.assertEqual(
            alpha["subagents"],
            {
                "requests": 3,
                "input_tokens": 27,
                "cache_creation_input_tokens": 3,
                "cache_read_input_tokens": 2011,
                "output_tokens": 12,
                "billable_tokens": 2053,
            },
        )

    def test_the_total_is_the_two_added(self):
        alpha = self.totals["feat/fixture-alpha"]
        for field in alpha["total"]:
            self.assertEqual(
                alpha["total"][field],
                alpha["main_thread"][field] + alpha["subagents"][field],
                field,
            )

    def test_a_mission_seen_only_through_subagents_is_marked_partial(self):
        beta = self.totals["feat/fixture-beta"]
        self.assertTrue(beta["partial_main_thread"])
        self.assertEqual(beta["main_thread"]["requests"], 0)
        self.assertEqual(beta["subagents"]["requests"], 2)

    def test_a_mission_with_a_main_transcript_is_not_marked_partial(self):
        self.assertFalse(self.totals["feat/fixture-alpha"]["partial_main_thread"])

    def test_agent_seconds_count_concurrency_and_elapsed_does_not(self):
        alpha = self.totals["feat/fixture-alpha"]
        # Main 300s in the alpha session plus 120s in epsilon; subagents 60s,
        # wholly inside the alpha main interval.
        self.assertEqual(alpha["agent_seconds"], 480.0)
        self.assertEqual(alpha["elapsed_seconds"], 420.0)

    def test_the_unplaced_buckets_carry_their_own_totals(self):
        shared = self.totals["<shared>"]
        self.assertEqual(shared["total"]["requests"], 2)
        self.assertEqual(shared["total"]["billable_tokens"], 8)
        unassigned = self.totals["<unassigned>"]
        self.assertEqual(unassigned["total"]["requests"], 1)
        self.assertEqual(unassigned["total"]["billable_tokens"], 8)

    def test_first_timestamp_seen_is_reported_so_pruning_is_visible(self):
        alpha = self.totals["feat/fixture-alpha"]
        self.assertEqual(alpha["first_timestamp_seen"], "2026-01-01T00:10:00+00:00")
        self.assertEqual(alpha["last_timestamp_seen"], "2026-02-01T00:02:00+00:00")


if __name__ == "__main__":
    unittest.main()
