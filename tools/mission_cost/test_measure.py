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
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "fixtures")
TRANSCRIPTS = os.path.join(FIXTURES, "transcripts")
REGISTRY = os.path.join(FIXTURES, "missions.json")
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

    def test_the_root_is_named_without_its_absolute_path(self):
        found = json.loads(report_bytes())
        self.assertEqual(found["inputs"]["root"], "transcripts")
        self.assertNotIn(os.sep, found["inputs"]["root"])

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
                }
            ],
        )

    def test_delivery_is_explicitly_absent_rather_than_zero_when_skipped(self):
        self.assertIsNone(self.missions["feat/fixture-alpha"]["delivery"])


class Compare(unittest.TestCase):
    def test_the_fixture_groups_are_too_small_and_say_so(self):
        found = json.loads(
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
        self.assertEqual(found["metric"], "billable_tokens")
        self.assertEqual(found["result"]["verdict"], "inconclusive")

    def test_overlapping_fixture_windows_are_reported(self):
        found = json.loads(
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
        self.assertTrue(found["windows_overlap"])


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

    def test_the_read_cost_row_is_reused_rather_than_recomputed(self):
        ledger = os.path.join(FIXTURES, "read-cost-ledger.md")
        self.assertEqual(delivery.read_cost_row(ledger, 9001), 17251)
        self.assertIsNone(delivery.read_cost_row(ledger, 4242))


if __name__ == "__main__":
    unittest.main()
