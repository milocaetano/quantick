#!/usr/bin/env python3
"""End-to-end tests for the mission-cost report: shape, determinism, silence.

`--no-gh` keeps every case offline. The delivery lookups have their own fake
runner below, so nothing here touches the network or the trader's machine.
"""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "fixtures")
TRANSCRIPTS = os.path.join(FIXTURES, "transcripts")
REGISTRY = os.path.join(FIXTURES, "missions.json")
DECLARED_ALL = os.path.join(FIXTURES, "missions-declared-all.json")
ALPHA = "aaaaaaaa-0000-4000-8000-000000000001"
MEASURE = os.path.join(HERE, "measure.py")

BAIT = ("MUST-NOT-APPEAR", "SHOULD-NOT-BE-READ")


def load(name):
    spec = importlib.util.spec_from_file_location(
        f"quantick_mission_cost_{name}", os.path.join(HERE, f"{name}.py")
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


measure = load("measure")
delivery = load("delivery")


def run(*args):
    result = subprocess.run(
        [sys.executable, MEASURE, *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout


def report_bytes(*extra):
    return run(
        "report",
        "--transcripts",
        TRANSCRIPTS,
        "--registry",
        REGISTRY,
        "--no-gh",
        *extra,
    )


class Command(unittest.TestCase):
    def test_help_exits_clean(self):
        subprocess.run(
            [sys.executable, MEASURE, "--help"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def test_the_one_documented_command_produces_a_report(self):
        found = json.loads(report_bytes())
        self.assertEqual(found["schema"], 1)
        self.assertEqual(found["method"], "docs/quality/mission-cost/method.md")
        self.assertEqual(
            [mission["branch"] for mission in found["missions"]],
            ["feat/fixture-alpha", "feat/fixture-beta"],
        )

    def test_the_report_states_the_registered_thresholds(self):
        found = json.loads(report_bytes())
        self.assertEqual(
            found["thresholds"],
            {
                "idle_gap_seconds": 300,
                "min_group_n": 5,
                "min_relative_change": 0.10,
                "max_unplaced_share": 0.20,
            },
        )


class Defaults(unittest.TestCase):
    def test_the_default_root_is_this_hosts_projects_directory_for_the_repo(self):
        found = measure.default_transcripts(r"C:\src\quantick")
        self.assertTrue(found.endswith("C--src-quantick"))
        self.assertIn(os.path.join(".claude", "projects"), found)

    def test_a_worktree_gets_its_own_root(self):
        main = measure.default_transcripts(r"C:\src\quantick")
        tree = measure.default_transcripts(r"C:\src\quantick-worktrees\feat-x")
        self.assertNotEqual(main, tree)
        self.assertTrue(tree.endswith("C--src-quantick-worktrees-feat-x"))


class Metrics(unittest.TestCase):
    ENTRY = {
        "total": {"billable_tokens": 10, "output_tokens": 3},
        "main_thread": {"billable_tokens": 6},
        "subagents": {"billable_tokens": 4},
        "agent_seconds": 120.0,
        "delivery": {"ci_seconds": 600.0},
    }

    def test_a_bare_name_reads_the_mission_total(self):
        self.assertEqual(measure.metric_value(self.ENTRY, "billable_tokens"), 10)

    def test_a_dotted_name_reads_one_thread(self):
        self.assertEqual(
            measure.metric_value(self.ENTRY, "main_thread.billable_tokens"), 6
        )
        self.assertEqual(
            measure.metric_value(self.ENTRY, "subagents.billable_tokens"), 4
        )

    def test_wall_clock_and_delivery_metrics_are_read_from_their_own_places(self):
        self.assertEqual(measure.metric_value(self.ENTRY, "agent_seconds"), 120.0)
        self.assertEqual(measure.metric_value(self.ENTRY, "ci_seconds"), 600.0)

    def test_a_delivery_metric_without_delivery_is_absent_not_zero(self):
        self.assertIsNone(
            measure.metric_value({"total": {}, "delivery": None}, "ci_seconds")
        )


class Determinism(unittest.TestCase):
    def test_two_runs_over_one_input_set_are_byte_identical(self):
        self.assertEqual(report_bytes(), report_bytes())

    def test_the_digest_changes_when_the_inputs_change(self):
        whole = json.loads(report_bytes())["inputs"]["digest"]
        alone = json.loads(
            run(
                "report",
                "--transcripts",
                os.path.join(
                    TRANSCRIPTS, "cccccccc-0000-4000-8000-000000000003", "subagents"
                ),
                "--registry",
                REGISTRY,
                "--no-gh",
            )
        )["inputs"]["digest"]
        self.assertNotEqual(whole, alone)

    def test_the_roots_are_named_without_their_absolute_paths(self):
        # An absolute path here would put the trader's user name into a
        # committed aggregate. The basename is enough to tell two roots apart.
        found = json.loads(report_bytes())
        self.assertEqual(found["inputs"]["roots"], ["transcripts"])
        for root in found["inputs"]["roots"]:
            self.assertNotIn(os.sep, root)
            self.assertNotIn("/", root)

    def test_several_roots_are_measured_together(self):
        found = json.loads(
            run(
                "report",
                "--transcripts",
                TRANSCRIPTS,
                "--transcripts",
                os.path.join(
                    TRANSCRIPTS, "cccccccc-0000-4000-8000-000000000003", "subagents"
                ),
                "--registry",
                REGISTRY,
                "--no-gh",
            )
        )
        self.assertEqual(found["inputs"]["roots"], ["subagents", "transcripts"])
        self.assertEqual(found["inputs"]["transcripts"], 8)

    def test_out_writes_the_same_bytes_it_would_have_printed(self):
        with tempfile.TemporaryDirectory() as room:
            target = os.path.join(room, "report.json")
            report_bytes("--out", target)
            with open(target, "rb") as stream:
                self.assertEqual(stream.read(), report_bytes())

    def test_the_output_ends_in_exactly_one_newline(self):
        raw = report_bytes()
        self.assertTrue(raw.endswith(b"\n"))
        self.assertFalse(raw.endswith(b"\n\n"))


class Silence(unittest.TestCase):
    def test_no_planted_transcript_content_reaches_the_report(self):
        raw = report_bytes().decode("utf-8")
        for bait in BAIT:
            self.assertNotIn(bait, raw)

    def test_no_session_body_key_reaches_the_report(self):
        raw = report_bytes().decode("utf-8")
        for key in ("gitBranch", "cwd", "service_tier", "isSidechain", "content"):
            self.assertNotIn(key, raw)


class Shape(unittest.TestCase):
    def setUp(self):
        self.found = json.loads(report_bytes())
        self.missions = {
            mission["branch"]: mission for mission in self.found["missions"]
        }

    def test_a_mission_reports_all_four_token_kinds_and_not_one_total(self):
        totals = self.missions["feat/fixture-alpha"]["total"]
        self.assertEqual(
            sorted(totals),
            [
                "billable_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
                "input_tokens",
                "output_tokens",
                "requests",
            ],
        )

    def test_a_mission_reports_all_three_wall_clock_quantities(self):
        mission = self.missions["feat/fixture-alpha"]
        self.assertEqual(mission["agent_seconds"], 480.0)
        self.assertEqual(mission["elapsed_seconds"], 420.0)
        self.assertGreater(mission["span_seconds"], mission["elapsed_seconds"])

    def test_each_assigned_session_names_the_rule_that_placed_it(self):
        mission = self.missions["feat/fixture-alpha"]
        self.assertEqual(
            mission["sessions"],
            [
                {
                    "session": "aaaaaaaa-0000-4000-8000-000000000001",
                    "method": "window",
                },
                {
                    "session": "eeeeeeee-0000-4000-8000-000000000005",
                    "method": "declared",
                },
            ],
        )

    def test_the_unplaced_buckets_are_reported_with_their_share(self):
        unplaced = self.found["unplaced"]
        self.assertEqual(unplaced["shared"]["total"]["billable_tokens"], 8)
        self.assertEqual(unplaced["unassigned"]["total"]["billable_tokens"], 8)
        self.assertAlmostEqual(
            unplaced["share_of_billable_tokens"], 16 / 8279, places=9
        )

    def test_a_shared_session_names_the_missions_it_could_not_be_split_between(self):
        self.assertEqual(
            self.found["unplaced"]["shared"]["sessions"],
            [
                {
                    "session": "bbbbbbbb-0000-4000-8000-000000000002",
                    "candidates": ["feat/fixture-alpha", "feat/fixture-beta"],
                    "first_timestamp_seen": "2026-01-01T01:00:00+00:00",
                    "last_timestamp_seen": "2026-01-01T02:30:00+00:00",
                    "total": {
                        "requests": 2,
                        "input_tokens": 2,
                        "cache_creation_input_tokens": 2,
                        "cache_read_input_tokens": 2,
                        "output_tokens": 2,
                        "billable_tokens": 8,
                    },
                }
            ],
        )

    def test_an_unplaced_session_carries_its_own_totals_and_extent(self):
        # Per session, not per bucket: the group share is computed from the
        # sessions whose time actually reaches that group's window, so one
        # stray session cannot charge a group every unplaced token there is.
        unassigned = self.found["unplaced"]["unassigned"]["sessions"]
        self.assertEqual(len(unassigned), 1)
        self.assertEqual(
            unassigned[0]["session"], "dddddddd-0000-4000-8000-000000000004"
        )
        self.assertEqual(unassigned[0]["total"]["billable_tokens"], 8)
        self.assertEqual(
            unassigned[0]["first_timestamp_seen"], "2026-01-02T10:00:00+00:00"
        )

    def test_delivery_is_explicitly_absent_rather_than_zero_when_skipped(self):
        self.assertIsNone(self.missions["feat/fixture-alpha"]["delivery"])


class Notes(unittest.TestCase):
    def test_a_note_is_typed_so_a_consumer_can_branch_on_it(self):
        missions = [
            measure.ATTRIBUTION.Mission(
                branch="feat/no-window",
                pr=None,
                sessions=(),
                transcripts=(),
                started_at=None,
                ended_at=None,
                group=None,
                note=None,
            )
        ]
        _, notes = measure.resolve_windows(missions, measure.Pulls(False, None))
        self.assertEqual(len(notes), 1)
        self.assertEqual(sorted(notes[0]), ["branch", "detail", "kind"])
        self.assertEqual(notes[0]["kind"], "no_window")
        self.assertIn(notes[0]["kind"], measure.NOTE_KINDS)

    def test_a_half_resolved_window_is_its_own_kind_of_note(self):
        missions = measure.ATTRIBUTION.parse_registry(
            {
                "schema": 1,
                "missions": [
                    {
                        "branch": "feat/half",
                        "pr": None,
                        "ended_at": "2026-01-01T02:00:00Z",
                    }
                ],
            }
        )
        _, notes = measure.resolve_windows(missions, measure.Pulls(False, None))
        self.assertEqual([note["kind"] for note in notes], ["partial_window"])
        self.assertIn("partial_window", measure.NOTE_KINDS)

    def test_a_resolved_window_leaves_no_note(self):
        missions = measure.ATTRIBUTION.load_registry(REGISTRY)
        _, notes = measure.resolve_windows(missions, measure.Pulls(False, None))
        self.assertEqual(notes, [])


class OneLoader(unittest.TestCase):
    def test_every_module_shares_the_single_loader_and_its_result(self):
        # Three verbatim copies of one loader is three things to keep in step.
        # The implementation lives in the leaf; everything else bootstraps it.
        attribution = measure.ATTRIBUTION
        self.assertIs(measure.TRANSCRIPTS, attribution.TRANSCRIPTS)
        self.assertIs(measure.TRANSCRIPTS, delivery.TRANSCRIPTS)
        self.assertIs(measure.TRANSCRIPTS.load, attribution.TRANSCRIPTS.load)

    def test_the_loader_reports_a_missing_file_rather_than_failing_oddly(self):
        with self.assertRaises(RuntimeError):
            measure.TRANSCRIPTS.load("not_a_module", "no/such/file.py")


class Commands(unittest.TestCase):
    def test_every_forge_command_is_built_in_one_place(self):
        self.assertEqual(
            delivery.pull_command(7)[:4], ["gh", "pr", "view", "7"]
        )
        self.assertEqual(
            delivery.commits_command(7)[-1], delivery.COMMIT_FIELDS
        )
        self.assertEqual(
            delivery.runs_command("feat/x", 5)[:6],
            ["gh", "run", "list", "--branch", "feat/x", "--limit"],
        )

    def test_the_run_limit_is_a_named_constant(self):
        self.assertEqual(delivery.CI_RUN_LIMIT, 300)
        self.assertIn(str(delivery.CI_RUN_LIMIT), delivery.runs_command("b", 300))


class Compare(unittest.TestCase):
    def comparison(self):
        return json.loads(
            run(
                "compare",
                "--transcripts",
                TRANSCRIPTS,
                "--registry",
                REGISTRY,
                "--no-gh",
                "--metric",
                "billable_tokens",
            )
        )

    def test_the_metric_and_both_groups_are_named(self):
        found = self.comparison()
        self.assertEqual(found["metric"], "billable_tokens")
        self.assertEqual(found["missions"]["before"], ["feat/fixture-alpha"])
        self.assertEqual(found["missions"]["after"], ["feat/fixture-beta"])

    def test_overlapping_fixture_windows_outrank_every_other_verdict(self):
        # The fixture's two windows overlap on purpose. The method grades that
        # first, ahead of the small n, because a comparison whose groups share
        # a window is not a comparison at all.
        found = self.comparison()
        self.assertTrue(found["windows_overlap"])
        self.assertEqual(found["result"]["verdict"], "cannot_be_attributed")
        self.assertIn("overlap", found["result"]["reason"])


class FakeRunner:
    """Answers `gh` and `git` from a table instead of the network."""

    def __init__(self, answers):
        self.answers = answers
        self.asked = []

    def __call__(self, args):
        self.asked.append(tuple(args))
        for prefix, answer in self.answers.items():
            if tuple(args)[: len(prefix)] == prefix:
                return answer
        raise AssertionError(f"unexpected command: {args}")


class Delivery(unittest.TestCase):
    def test_pull_facts_take_only_the_timings_the_method_names(self):
        runner = FakeRunner(
            {
                ("gh", "pr", "view"): json.dumps(
                    {
                        "number": 9001,
                        "headRefName": "feat/fixture-alpha",
                        "baseRefName": "main",
                        "headRefOid": "c0ffee",
                        "createdAt": "2026-01-01T00:00:00Z",
                        "mergedAt": "2026-01-01T01:00:00Z",
                        "closedAt": "2026-01-01T01:00:00Z",
                        "state": "MERGED",
                        "title": "TITLE-MUST-NOT-APPEAR",
                    }
                )
            }
        )
        facts = delivery.pull_facts(9001, runner=runner)
        self.assertEqual(facts["pr_open_seconds"], 3600.0)
        self.assertEqual(facts["head"], "c0ffee")
        self.assertNotIn("title", facts)

    def test_an_open_pull_request_is_bounded_by_as_of(self):
        runner = FakeRunner(
            {
                ("gh", "pr", "view"): json.dumps(
                    {
                        "number": 9002,
                        "headRefName": "feat/fixture-beta",
                        "baseRefName": "main",
                        "headRefOid": "beef",
                        "createdAt": "2026-01-01T00:00:00Z",
                        "mergedAt": None,
                        "closedAt": None,
                        "state": "OPEN",
                    }
                )
            }
        )
        facts = delivery.pull_facts(
            9002, runner=runner, as_of="2026-01-01T00:30:00Z"
        )
        self.assertEqual(facts["pr_open_seconds"], 1800.0)
        self.assertEqual(facts["ended_at_source"], "as_of")

    def test_an_open_pull_request_without_as_of_has_no_duration(self):
        runner = FakeRunner(
            {
                ("gh", "pr", "view"): json.dumps(
                    {
                        "number": 9002,
                        "headRefName": "feat/fixture-beta",
                        "baseRefName": "main",
                        "headRefOid": "beef",
                        "createdAt": "2026-01-01T00:00:00Z",
                        "mergedAt": None,
                        "closedAt": None,
                        "state": "OPEN",
                    }
                )
            }
        )
        facts = delivery.pull_facts(9002, runner=runner)
        self.assertIsNone(facts["pr_open_seconds"])

    def test_ci_time_sums_runs_and_the_wall_counts_parallel_runs_once(self):
        runner = FakeRunner(
            {
                ("gh", "run", "list"): json.dumps(
                    [
                        {
                            "status": "completed",
                            "conclusion": "success",
                            "headSha": "c0ffee",
                            "startedAt": "2026-01-01T00:00:00Z",
                            "updatedAt": "2026-01-01T00:10:00Z",
                        },
                        {
                            "status": "completed",
                            "conclusion": "success",
                            "headSha": "c0ffee",
                            "startedAt": "2026-01-01T00:05:00Z",
                            "updatedAt": "2026-01-01T00:12:00Z",
                        },
                        {
                            "status": "in_progress",
                            "conclusion": "",
                            "headSha": "c0ffee",
                            "startedAt": "2026-01-01T00:20:00Z",
                            "updatedAt": "2026-01-01T00:21:00Z",
                        },
                    ]
                )
            }
        )
        facts = delivery.ci_facts("feat/fixture-alpha", runner=runner)
        self.assertEqual(facts["ci_runs"], 2)
        self.assertEqual(facts["ci_seconds"], 1020.0)
        self.assertEqual(facts["ci_wall_seconds"], 720.0)

    def test_the_window_starts_at_the_earliest_commit_not_the_listed_first(self):
        runner = FakeRunner(
            {
                ("gh", "pr", "view"): json.dumps(
                    {
                        "commits": [
                            {"committedDate": "2026-01-01T09:00:00Z"},
                            {"committedDate": "2026-01-01T07:00:00Z"},
                            {"committedDate": "2026-01-01T11:00:00Z"},
                        ]
                    }
                )
            }
        )
        self.assertEqual(
            delivery.first_commit_instant(9001, runner=runner),
            "2026-01-01T07:00:00Z",
        )

    def test_a_pull_request_with_no_commits_has_no_first_instant(self):
        runner = FakeRunner({("gh", "pr", "view"): json.dumps({"commits": []})})
        self.assertIsNone(delivery.first_commit_instant(9001, runner=runner))

    def test_a_full_run_listing_is_flagged_rather_than_silently_short(self):
        runs = [
            {
                "status": "completed",
                "conclusion": "success",
                "headSha": "c0ffee",
                "startedAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:01:00Z",
            }
        ] * 3
        runner = FakeRunner({("gh", "run", "list"): json.dumps(runs)})
        self.assertTrue(
            delivery.ci_facts("feat/x", runner=runner, limit=3)["ci_listing_truncated"]
        )
        self.assertFalse(
            delivery.ci_facts("feat/x", runner=runner, limit=4)["ci_listing_truncated"]
        )

    def test_the_read_cost_row_is_reused_rather_than_recomputed(self):
        ledger = os.path.join(FIXTURES, "read-cost-ledger.md")
        self.assertEqual(delivery.read_cost_row(ledger, 9001), 17251)
        self.assertIsNone(delivery.read_cost_row(ledger, 4242))

class DeclaredTranscripts(unittest.TestCase):
    """Section 2 rule one, through the one documented command."""

    @classmethod
    def setUpClass(cls):
        cls.found = json.loads(
            run(
                "report",
                "--transcripts",
                TRANSCRIPTS,
                "--registry",
                DECLARED_ALL,
                "--no-gh",
            )
        )
        cls.missions = {entry["branch"]: entry for entry in cls.found["missions"]}

    def test_a_mission_that_declares_its_transcripts_leaves_nothing_unplaced(self):
        # #576's third acceptance criterion, read off the report the method
        # names: everything measured reached a mission.
        self.assertEqual(self.found["unplaced"]["share_of_billable_tokens"], 0.0)
        self.assertEqual(self.found["unplaced"]["billable_tokens"], 0)
        self.assertEqual(self.found["unplaced"]["shared"]["sessions"], [])
        self.assertEqual(self.found["unplaced"]["unassigned"]["sessions"], [])

    def test_each_declared_transcript_names_the_rule_that_placed_it(self):
        self.assertEqual(
            self.missions["feat/declared-contexts"]["transcripts"],
            [
                {"transcript": f"{ALPHA}.jsonl", "method": "declared"},
                {
                    "transcript": f"{ALPHA}/subagents/agent-1111.jsonl",
                    "method": "declared",
                },
                {
                    "transcript": f"{ALPHA}/subagents/agent-2222.jsonl",
                    "method": "declared",
                },
            ],
        )

    def test_a_mission_that_declared_no_transcript_reports_an_empty_list(self):
        siblings = self.missions["feat/declared-siblings"]
        self.assertEqual(siblings["transcripts"], [])
        self.assertEqual(len(siblings["sessions"]), 4)

    def test_a_transcript_placed_on_a_mission_is_not_also_a_session_of_it(self):
        # The coordinator session is divided, so naming it under `sessions`
        # would charge the mission with the whole of it a second time.
        self.assertEqual(self.missions["feat/declared-contexts"]["sessions"], [])

    def test_no_transcript_content_survives_a_declared_placement(self):
        raw = run(
            "report",
            "--transcripts",
            TRANSCRIPTS,
            "--registry",
            DECLARED_ALL,
            "--no-gh",
        ).decode("ascii")
        for bait in BAIT:
            self.assertNotIn(bait, raw)


class MissingDeclaredTranscriptNote(unittest.TestCase):
    def test_a_declared_transcript_no_root_holds_is_a_typed_note(self):
        gone = "ffffffff-0000-4000-8000-000000000006/subagents/agent-9999.jsonl"
        with tempfile.TemporaryDirectory() as folder:
            registry = os.path.join(folder, "missions.json")
            with open(registry, "w", encoding="utf-8") as stream:
                json.dump(
                    {
                        "schema": 1,
                        "missions": [
                            {
                                "branch": "feat/pruned",
                                "pr": None,
                                "transcripts": [gone],
                                "started_at": "2026-01-01T00:00:00Z",
                                "ended_at": "2026-01-01T01:00:00Z",
                            }
                        ],
                    },
                    stream,
                )
            found = json.loads(
                run(
                    "report",
                    "--transcripts",
                    TRANSCRIPTS,
                    "--registry",
                    registry,
                    "--no-gh",
                )
            )
        self.assertEqual(
            found["notes"],
            [
                {
                    "branch": "feat/pruned",
                    "kind": "declared_transcript_missing",
                    "detail": gone,
                }
            ],
        )
        self.assertIn("declared_transcript_missing", measure.NOTE_KINDS)


class Identify(unittest.TestCase):
    """The context claim, resolved. Section 9 of the experiment protocol.

    The fixture session is the exact shape the campaign runs in: a main thread
    that spans the whole session and dispatched agents living inside it. That
    is why `--role` exists, and these cases are what stops it being dropped as
    a redundant flag by someone who has not hit the problem.
    """

    ALPHA_MAIN = f"{ALPHA}.jsonl"
    ALPHA_ONE = f"{ALPHA}/subagents/agent-1111.jsonl"
    ALPHA_TWO = f"{ALPHA}/subagents/agent-2222.jsonl"

    def identify(self, start, end, role):
        return json.loads(
            run(
                "identify",
                "--transcripts",
                TRANSCRIPTS,
                "--session",
                ALPHA,
                "--from",
                start,
                "--to",
                end,
                "--role",
                role,
            )
        )

    def test_a_dispatched_mission_finds_its_own_context(self):
        found = self.identify(
            "2026-01-01T00:11:10Z", "2026-01-01T00:11:50Z", "subagent"
        )
        self.assertTrue(found["resolved"])
        self.assertFalse(found["contested"])
        self.assertEqual(found["transcripts"], [self.ALPHA_ONE])

    def test_role_is_what_keeps_a_child_from_being_contested(self):
        """The main thread contains the same window and must not compete.

        Without `--role` every campaign child would find two containing
        transcripts -- its own and the coordinator's -- and none of them would
        ever be gradeable.
        """
        window = ("2026-01-01T00:11:10Z", "2026-01-01T00:11:50Z")
        child = self.identify(*window, "subagent")
        parent = self.identify(*window, "main")
        self.assertEqual([row["relative"] for row in child["own"]], [self.ALPHA_ONE])
        self.assertEqual([row["relative"] for row in parent["own"]], [self.ALPHA_MAIN])
        self.assertFalse(child["contested"])
        self.assertFalse(parent["contested"])

    def test_a_mission_owns_the_agents_it_dispatched(self):
        """Being contained finds the descendants, whose cost is the mission's."""
        found = self.identify("2026-01-01T00:10:30Z", "2026-01-01T00:58:00Z", "main")
        self.assertEqual(found["transcripts"], [self.ALPHA_MAIN, self.ALPHA_ONE, self.ALPHA_TWO])
        self.assertEqual(
            sorted(row["relative"] for row in found["within"]),
            [self.ALPHA_ONE, self.ALPHA_TWO],
        )

    def test_a_window_nobody_contains_resolves_to_nothing(self):
        found = self.identify(
            "2026-01-01T00:00:00Z", "2026-01-01T00:05:00Z", "subagent"
        )
        self.assertFalse(found["resolved"])
        self.assertFalse(found["contested"])
        self.assertEqual(found["transcripts"], [])

    def test_two_siblings_spanning_the_window_are_contested(self):
        """Real overlap refuses to choose rather than choosing wrong.

        A contested claim is `void` under the protocol, which is the honest
        outcome: a mission nobody can attribute beats one attributed to the
        wrong context.
        """
        session = "ffffffff-0000-4000-8000-000000000006"
        line = (
            '{{"timestamp": "{when}", "message": {{"usage": '
            '{{"input_tokens": 1, "cache_creation_input_tokens": 0, '
            '"cache_read_input_tokens": 10, "output_tokens": 1}}}}}}\n'
        )
        with tempfile.TemporaryDirectory() as room:
            beneath = os.path.join(room, session, "subagents")
            os.makedirs(beneath)
            for name in ("agent-aaaa.jsonl", "agent-bbbb.jsonl"):
                with open(os.path.join(beneath, name), "w", encoding="utf-8") as out:
                    out.write(line.format(when="2026-03-01T00:00:00.000Z"))
                    out.write(line.format(when="2026-03-01T01:00:00.000Z"))
            found = json.loads(
                run(
                    "identify",
                    "--transcripts",
                    room,
                    "--session",
                    session,
                    "--from",
                    "2026-03-01T00:10:00Z",
                    "--to",
                    "2026-03-01T00:20:00Z",
                    "--role",
                    "subagent",
                )
            )
        self.assertTrue(found["contested"])
        self.assertFalse(found["resolved"])
        self.assertEqual(len(found["own"]), 2)

    def test_a_claim_that_missed_its_own_context_resolves_to_nothing(self):
        """No containing context means the contained agents are unanchored.

        They ran inside the window, but nothing says the window belongs to the
        mission that claimed it, so `transcripts` -- the field a registry
        record copies -- stays empty and the candidates stay under `within`.
        """
        found = self.identify(
            "2026-01-01T00:05:00Z", "2026-01-01T00:59:30Z", "subagent"
        )
        self.assertFalse(found["resolved"])
        self.assertEqual(found["own"], [])
        self.assertTrue(found["within"])
        self.assertEqual(found["transcripts"], [])

    def test_a_contested_claim_resolves_to_nothing_either(self):
        session = "ffffffff-0000-4000-8000-000000000007"
        line = (
            '{{"timestamp": "{when}", "message": {{"usage": '
            '{{"input_tokens": 1, "cache_creation_input_tokens": 0, '
            '"cache_read_input_tokens": 10, "output_tokens": 1}}}}}}\n'
        )
        with tempfile.TemporaryDirectory() as room:
            beneath = os.path.join(room, session, "subagents")
            os.makedirs(beneath)
            for name in ("agent-cccc.jsonl", "agent-dddd.jsonl"):
                with open(os.path.join(beneath, name), "w", encoding="utf-8") as out:
                    out.write(line.format(when="2026-04-01T00:00:00.000Z"))
                    out.write(line.format(when="2026-04-01T01:00:00.000Z"))
            found = json.loads(
                run(
                    "identify", "--transcripts", room, "--session", session,
                    "--from", "2026-04-01T00:10:00Z", "--to", "2026-04-01T00:20:00Z",
                    "--role", "subagent",
                )
            )
        self.assertTrue(found["contested"])
        self.assertEqual(found["transcripts"], [])

    def test_identify_reads_no_transcript_content(self):
        found = self.identify(
            "2026-01-01T00:11:10Z", "2026-01-01T00:11:50Z", "subagent"
        )
        rendered = json.dumps(found)
        for bait in BAIT:
            self.assertNotIn(bait, rendered)
        for forbidden in ("cwd", "gitBranch", "sessionId"):
            self.assertNotIn(forbidden, rendered)


class Contexts(unittest.TestCase):
    """How long the running context already is, against the cap.

    The rule lives in `.claude/skills/mission/SKILL.md` and its arithmetic in
    `docs/quality/velocity/context-cap.md`. A context cannot count its own
    turns from the inside, so the command is the only way it sees the number,
    and these cases pin down what it is allowed to claim about which context
    it found. The fixture session is the campaign shape again: a main thread
    spanning the session and two dispatched agents living inside it.
    """

    ALPHA_MAIN = f"{ALPHA}.jsonl"
    ALPHA_ONE = f"{ALPHA}/subagents/agent-1111.jsonl"

    def contexts(self, *extra):
        return json.loads(
            run("contexts", "--transcripts", TRANSCRIPTS, "--session", ALPHA, *extra)
        )

    def test_since_resolves_the_running_context_by_containment(self):
        """The claimed `started_at` names the context exactly, not by guess."""
        found = self.contexts("--since", "2026-01-01T00:11:30Z", "--role", "subagent")
        self.assertEqual(found["resolution"], "containment")
        self.assertFalse(found["ambiguous"])
        self.assertEqual(found["running"]["relative"], self.ALPHA_ONE)
        self.assertEqual(found["requests"], 2)

    def test_without_since_the_answer_says_it_is_the_newest_writer(self):
        """A guess has to be readable as a guess.

        Without a claimed instant the command can only offer whoever wrote
        last, which a sibling agent running beside the caller will often be.
        `resolution` carries that word so a reader can tell the heuristic from
        the measurement instead of reading the same number twice.
        """
        found = self.contexts()
        self.assertEqual(found["resolution"], "newest_write")
        self.assertEqual(found["running"]["relative"], self.ALPHA_MAIN)
        self.assertEqual(found["requests"], 4)

    def test_role_keeps_the_main_thread_from_competing_with_a_child(self):
        """The same reason `identify` has the flag, one layer along.

        The coordinator's thread contains every instant its children claimed,
        so a child asking by containment alone finds two contexts and gets no
        number at all. With `--role` it finds its own.
        """
        window = ("--since", "2026-01-01T00:11:30Z")
        both = self.contexts(*window)
        child = self.contexts(*window, "--role", "subagent")
        self.assertTrue(both["ambiguous"])
        self.assertIsNone(both["running"])
        self.assertNotIn("requests", both)
        self.assertEqual(
            sorted(row["relative"] for row in both["candidates"]),
            [self.ALPHA_MAIN, self.ALPHA_ONE],
        )
        self.assertFalse(child["ambiguous"])
        self.assertEqual(child["running"]["relative"], self.ALPHA_ONE)

    def test_remaining_and_over_cap_are_the_caps_arithmetic(self):
        """`remaining` is cap minus requests, and it is allowed to go negative.

        A context over the cap has to be told so, not shown a floor at zero:
        the number is how far past the handoff it already is.
        """
        window = ("--since", "2026-01-01T00:11:30Z", "--role", "subagent")
        under = self.contexts(*window)
        over = self.contexts(*window, "--cap", "1")
        self.assertEqual(under["remaining"], under["cap"] - under["requests"])
        self.assertFalse(under["over_cap"])
        self.assertEqual(over["cap"], 1)
        self.assertEqual(over["remaining"], -1)
        self.assertTrue(over["over_cap"])

    def test_a_missing_session_id_is_an_error_naming_the_variable(self):
        """The one failure a real context will hit, and it must read as advice.

        The session id comes from the environment and nowhere else, so a
        context started outside Claude Code has to be told which variable is
        missing. A traceback would say only that a string was None.
        """
        room = dict(os.environ)
        room.pop("CLAUDE_CODE_SESSION_ID", None)
        result = subprocess.run(
            [sys.executable, MEASURE, "contexts", "--transcripts", TRANSCRIPTS],
            env=room,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        complaint = result.stderr.decode("utf-8")
        self.assertEqual(result.returncode, 2)
        self.assertIn("CLAUDE_CODE_SESSION_ID", complaint)
        self.assertNotIn("Traceback", complaint)
        self.assertEqual(result.stdout, b"")


if __name__ == "__main__":
    unittest.main()
