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


class Windows(unittest.TestCase):
    def mission(self, **fields):
        entry = {"branch": "feat/half", "pr": 1}
        entry.update(fields)
        return attribution.parse_registry({"schema": 1, "missions": [entry]})[0]

    def test_a_window_needs_both_of_its_ends(self):
        whole = self.mission(
            started_at="2026-01-01T00:00:00Z", ended_at="2026-01-01T02:00:00Z"
        )
        self.assertTrue(attribution.has_window(whole))
        for half in (
            self.mission(started_at="2026-01-01T00:00:00Z"),
            self.mission(ended_at="2026-01-01T02:00:00Z"),
            self.mission(),
        ):
            self.assertFalse(attribution.has_window(half))

    def test_a_half_resolved_window_claims_nothing_by_time(self):
        # Half a window is not a narrower window, it is an unbounded one. A
        # mission whose started_at never resolved would otherwise claim every
        # session that ended before its ended_at -- weeks of work done before
        # the branch existed, charged to it in silence.
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        missions = [self.mission(ended_at="2026-06-01T00:00:00Z")]
        placed = attribution.assign(sessions, missions)
        self.assertEqual(placed.assigned, {})
        self.assertEqual(len(placed.unassigned), len(sessions))

    def test_a_declared_session_still_reaches_a_mission_with_no_window(self):
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        missions = [self.mission(sessions=[ALPHA])]
        placed = attribution.assign(sessions, missions)
        self.assertEqual(placed.assigned[ALPHA], ("feat/half", "declared"))


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
        self.totals = attribution.totals_by_mission(self.missions, self.placed)

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

SHARED_SESSION = os.path.join(FIXTURES, "missions-shared-session.json")
DECLARED_ALL = os.path.join(FIXTURES, "missions-declared-all.json")

ONE = f"{ALPHA}/subagents/agent-1111.jsonl"
TWO = f"{ALPHA}/subagents/agent-2222.jsonl"


def placement_over(registry):
    found, _ = transcripts.discover(TRANSCRIPTS)
    sessions = attribution.sessions_from(found)
    missions = attribution.load_registry(registry)
    return sessions, missions, attribution.assign(sessions, missions)


class TranscriptPaths(unittest.TestCase):
    """Section 2's layout rule, asked of a path a person wrote down."""

    def test_a_root_file_and_a_subagent_file_are_addressable(self):
        self.assertEqual(transcripts.classify(f"{ALPHA}.jsonl"), (ALPHA, "main"))
        self.assertEqual(transcripts.classify(ONE), (ALPHA, "subagent"))

    def test_anything_the_layout_cannot_address_is_not_a_transcript(self):
        for path in (
            "notes.txt",
            "",
            ".jsonl",
            f"{ALPHA}/agents/agent-1111.jsonl",
            f"../{ALPHA}.jsonl",
            f"{ALPHA}//subagents/agent-1111.jsonl",
            None,
            17,
        ):
            self.assertIsNone(transcripts.classify(path), path)


class DeclaredTranscriptRegistry(unittest.TestCase):
    def registry(self, *entries):
        return attribution.parse_registry({"schema": 1, "missions": list(entries)})

    def test_a_declared_transcript_is_read_off_the_record(self):
        missions = attribution.load_registry(SHARED_SESSION)
        self.assertEqual(missions[0].transcripts, (ONE,))
        self.assertEqual(missions[1].transcripts, (TWO,))
        self.assertEqual(missions[1].sessions, (EPSILON,))

    def test_a_record_may_declare_both_sessions_and_transcripts(self):
        missions = self.registry(
            {"branch": "feat/x", "pr": 1, "sessions": [BETA], "transcripts": [ONE]}
        )
        self.assertEqual(missions[0].sessions, (BETA,))
        self.assertEqual(missions[0].transcripts, (ONE,))

    def test_a_transcript_declared_twice_is_refused(self):
        with self.assertRaises(attribution.RegistryError):
            self.registry(
                {"branch": "feat/x", "pr": 1, "transcripts": [ONE]},
                {"branch": "feat/y", "pr": 2, "transcripts": [ONE]},
            )

    def test_a_transcript_of_a_session_another_mission_declares_is_refused(self):
        # Both orders: the conflict is the claim, not the order it was written.
        with self.assertRaises(attribution.RegistryError):
            self.registry(
                {"branch": "feat/x", "pr": 1, "sessions": [ALPHA]},
                {"branch": "feat/y", "pr": 2, "transcripts": [ONE]},
            )
        with self.assertRaises(attribution.RegistryError):
            self.registry(
                {"branch": "feat/y", "pr": 2, "transcripts": [ONE]},
                {"branch": "feat/x", "pr": 1, "sessions": [ALPHA]},
            )

    def test_one_mission_may_declare_a_session_and_a_transcript_inside_it(self):
        missions = self.registry(
            {"branch": "feat/x", "pr": 1, "sessions": [ALPHA], "transcripts": [ONE]}
        )
        self.assertEqual(missions[0].transcripts, (ONE,))

    def test_a_path_the_layout_cannot_address_is_refused(self):
        with self.assertRaises(attribution.RegistryError):
            self.registry({"branch": "feat/x", "pr": 1, "transcripts": ["notes.txt"]})

    def test_transcripts_must_be_a_list_of_strings(self):
        with self.assertRaises(attribution.RegistryError):
            self.registry({"branch": "feat/x", "pr": 1, "transcripts": ONE})
        with self.assertRaises(attribution.RegistryError):
            self.registry({"branch": "feat/x", "pr": 1, "transcripts": [3]})


class SharedSessionDivided(unittest.TestCase):
    """The shape #576 exists for: siblings dispatched under one session."""

    def setUp(self):
        self.sessions, self.missions, self.placed = placement_over(SHARED_SESSION)
        self.totals = attribution.totals_by_mission(self.missions, self.placed)

    def test_each_child_owns_the_transcript_it_declared(self):
        self.assertEqual(self.placed.transcripts[ONE], "feat/child-one")
        self.assertEqual(self.placed.transcripts[TWO], "feat/child-two")

    def test_a_declared_transcript_reaches_a_mission_with_no_window(self):
        # Neither child has a window. Under the session rule each would have
        # had to declare the coordinator, and each would then have claimed the
        # other. Declaration is rule one and needs no window.
        self.assertEqual(
            self.totals["feat/child-one"]["subagents"],
            {
                "requests": 2,
                "input_tokens": 20,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 2000,
                "output_tokens": 10,
                "billable_tokens": 2030,
            },
        )
        self.assertEqual(self.totals["feat/child-one"]["main_thread"]["requests"], 0)
        self.assertTrue(self.totals["feat/child-one"]["partial_main_thread"])
        self.assertEqual(self.totals["feat/child-one"]["agent_seconds"], 60.0)

    def test_a_sibling_is_not_charged_for_the_other_child(self):
        two = self.totals["feat/child-two"]
        self.assertEqual(two["subagents"]["billable_tokens"], 23)
        # Its own declared session, whole, on the same record.
        self.assertEqual(two["main_thread"]["billable_tokens"], 4000)
        self.assertFalse(two["partial_main_thread"])

    def test_the_undeclared_remainder_is_placed_on_its_own_timestamps(self):
        # The coordinator's own main thread belongs to neither child, and with
        # no window anywhere it lands unassigned rather than on a sibling.
        self.assertEqual(self.placed.unassigned, [ALPHA, BETA, GAMMA, DELTA])
        self.assertEqual(self.placed.remainders[ALPHA].subagents, [])
        unassigned = self.totals["<unassigned>"]
        self.assertEqual(unassigned["main_thread"]["requests"], 7)
        self.assertEqual(unassigned["subagents"]["requests"], 2)

    def test_the_remainder_carries_only_what_is_left_of_it(self):
        left = attribution.session_totals(self.placed.remainders[ALPHA])
        self.assertEqual(left["total"]["billable_tokens"], 2170)
        self.assertEqual(left["first_timestamp_seen"], "2026-01-01T00:10:00+00:00")

    def test_every_transcript_lands_in_exactly_one_place(self):
        placed = []
        for mission in self.missions:
            for parcel in self.placed.parcels_of(mission.branch):
                placed.extend(item.relative for item in parcel.transcripts)
        for uuid in list(self.placed.shared) + self.placed.unassigned:
            parcel = self.placed.parcel_for(uuid)
            placed.extend(item.relative for item in parcel.transcripts)
        every = [
            item.relative
            for session in self.sessions.values()
            for item in session.transcripts
        ]
        self.assertEqual(sorted(placed), sorted(every))
        self.assertEqual(len(placed), len(set(placed)))


class DeclaredTranscriptsPlaceEverything(unittest.TestCase):
    def test_nothing_is_left_for_a_bucket_to_hold(self):
        sessions, missions, placed = placement_over(DECLARED_ALL)
        totals = attribution.totals_by_mission(missions, placed)
        self.assertEqual(placed.unassigned, [])
        self.assertEqual(dict(placed.shared), {})
        self.assertEqual(totals["<unassigned>"]["total"]["billable_tokens"], 0)
        self.assertEqual(totals["<shared>"]["total"]["billable_tokens"], 0)
        self.assertEqual(
            totals["feat/declared-contexts"]["total"]["billable_tokens"],
            2170 + 2030 + 23,
        )


class MissingDeclaredTranscript(unittest.TestCase):
    def test_a_transcript_no_root_holds_is_reported_rather_than_refused(self):
        # E6: the host prunes transcripts and a committed registry outlives
        # them. Refusing would make the registry unreadable a month from now.
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        gone = "ffffffff-0000-4000-8000-000000000006/subagents/agent-9999.jsonl"
        missions = attribution.parse_registry(
            {
                "schema": 1,
                "missions": [
                    {"branch": "feat/pruned", "pr": 1, "transcripts": [gone, ONE]}
                ],
            }
        )
        placed = attribution.assign(sessions, missions)
        self.assertEqual(placed.missing, [("feat/pruned", gone)])
        self.assertEqual(placed.transcripts[ONE], "feat/pruned")


class NoTranscriptsDeclaredChangesNothing(unittest.TestCase):
    def test_the_original_registry_assigns_exactly_what_it_did(self):
        # Section 9's own claim: a registry without the field assigns what it
        # assigned before. Rules two and three see whole sessions, and no
        # remainder is recorded.
        sessions, _, placed = fixture_assignment()
        self.assertEqual(dict(placed.remainders), {})
        self.assertEqual(dict(placed.transcripts), {})
        self.assertEqual(placed.missing, [])
        for uuid in sessions:
            self.assertIs(placed.parcel_for(uuid), sessions[uuid])

class DeclarationOutranksTheWindow(unittest.TestCase):
    """Rule one runs before rule three, and takes the transcript with it."""

    def test_a_named_transcript_leaves_a_window_that_covers_its_session(self):
        found, _ = transcripts.discover(TRANSCRIPTS)
        sessions = attribution.sessions_from(found)
        missions = attribution.parse_registry(
            {
                "schema": 1,
                "missions": [
                    {
                        "branch": "feat/by-window",
                        "pr": 1,
                        "started_at": "2026-01-01T00:00:00Z",
                        "ended_at": "2026-01-01T01:00:00Z",
                    },
                    {"branch": "feat/by-name", "pr": 2, "transcripts": [ONE]},
                ],
            }
        )
        placed = attribution.assign(sessions, missions)
        self.assertEqual(placed.transcripts[ONE], "feat/by-name")
        # The rest of the coordinator session still lands by its own
        # timestamps, on the mission whose window covers it.
        self.assertEqual(placed.assigned[ALPHA], ("feat/by-window", "window"))
        totals = attribution.totals_by_mission(missions, placed)
        self.assertEqual(totals["feat/by-name"]["total"]["billable_tokens"], 2030)
        # The coordinator remainder's 2170 plus agent-2222's 23, and session
        # beta's 8, which the same window also covers -- but not agent-1111's
        # 2030, which rule one took before the window was ever consulted.
        self.assertEqual(totals["feat/by-window"]["total"]["billable_tokens"], 2201)


class OnePathUnderTwoRoots(unittest.TestCase):
    def test_a_path_two_roots_hold_is_refused_rather_than_summed(self):
        # A run may be given several transcript roots, and a declared path is
        # relative to one of them. Summing both would charge the mission twice
        # and read exactly like a mission that worked twice as long.
        relative = f"{ALPHA}/subagents/agent-1111.jsonl"
        twice = [
            transcripts.Transcript(
                f"{root}/{relative}", relative, ALPHA, "subagent", root
            )
            for root in ("C--src-quantick", "C--src-quantick-worktrees-one")
        ]
        sessions = attribution.sessions_from(twice)
        missions = attribution.parse_registry(
            {
                "schema": 1,
                "missions": [
                    {"branch": "feat/x", "pr": 1, "transcripts": [relative]}
                ],
            }
        )
        with self.assertRaises(attribution.RegistryError):
            attribution.assign(sessions, missions)

class PlacementOwnsWhatItPlaced(unittest.TestCase):
    def test_a_placement_carries_the_sessions_it_was_built_from(self):
        # So no caller can hand it a second map that disagrees with the one
        # the rules were applied to.
        sessions, _, placed = fixture_assignment()
        self.assertIs(placed.sessions, sessions)

    def test_a_branch_index_is_built_while_placing_not_scanned_after(self):
        sessions, missions, placed = placement_over(SHARED_SESSION)
        self.assertEqual(placed.transcripts_of("feat/child-one"), [ONE])
        self.assertEqual(placed.transcripts_of("feat/child-two"), [TWO])
        self.assertEqual(placed.sessions_of("feat/child-two"), [EPSILON])
        self.assertEqual(placed.sessions_of("feat/child-one"), [])
        # An unknown branch answers empty without being recorded.
        self.assertEqual(placed.sessions_of("feat/never-ran"), [])
        self.assertEqual(placed.transcripts_of("feat/never-ran"), [])
        self.assertEqual(
            sorted(placed.transcripts_of(m.branch) for m in missions),
            sorted([[ONE], [TWO]]),
        )


if __name__ == "__main__":
    unittest.main()
