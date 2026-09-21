#!/usr/bin/env python3
"""Offline tests for the registry grouping rule."""

import importlib.util
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(
        f"quantick_mission_cost_{name}", os.path.join(HERE, f"{name}.py")
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


group_registry = load("group_registry")

PIVOT = "2026-09-20T01:43:13Z"


def mission(branch, started, ended, group=None):
    return {
        "branch": branch,
        "pr": 1,
        "sessions": [],
        "started_at": started,
        "ended_at": ended,
        "group": group,
        "note": "",
    }


class GroupFor(unittest.TestCase):
    def test_a_window_that_closed_before_the_pivot_is_before(self):
        found = mission("a", "2026-09-19T00:00:00Z", "2026-09-19T12:00:00Z")
        self.assertEqual(group_registry.group_for(found, PIVOT), "before")

    def test_a_window_that_opened_at_or_after_the_pivot_is_after(self):
        at = mission("a", PIVOT, "2026-09-20T09:00:00Z")
        later = mission("b", "2026-09-20T09:00:00Z", "2026-09-20T12:00:00Z")
        self.assertEqual(group_registry.group_for(at, PIVOT), "after")
        self.assertEqual(group_registry.group_for(later, PIVOT), "after")

    def test_a_window_that_straddles_the_pivot_joins_neither_group(self):
        found = mission("a", "2026-09-19T00:00:00Z", "2026-09-20T09:00:00Z")
        self.assertIsNone(group_registry.group_for(found, PIVOT))

    def test_the_mission_that_delivered_the_change_joins_neither_group(self):
        found = mission("a", "2026-09-19T12:53:00Z", PIVOT)
        self.assertIsNone(group_registry.group_for(found, PIVOT))

    def test_a_mission_without_a_window_joins_neither_group(self):
        self.assertIsNone(group_registry.group_for(mission("a", None, None), PIVOT))
        self.assertIsNone(
            group_registry.group_for(mission("a", None, "2026-09-25T00:00:00Z"), PIVOT)
        )

    def test_the_same_moment_spelled_two_ways_lands_on_the_same_side(self):
        # `Z` sorts after `+` in ASCII, so a string comparison would put these
        # two spellings of one instant on opposite sides of each other.
        offset = "2026-09-20T01:43:13+00:00"
        self.assertEqual(group_registry.group_for(mission("a", offset, None), PIVOT), "after")
        self.assertEqual(group_registry.group_for(mission("b", PIVOT, None), offset), "after")
        self.assertIsNone(group_registry.group_for(mission("c", None, offset), PIVOT))
        self.assertIsNone(group_registry.group_for(mission("d", None, PIVOT), offset))

    def test_an_instant_that_is_not_one_is_refused_rather_than_compared(self):
        with self.assertRaises(group_registry.InstantError):
            group_registry.group_for(mission("a", "yesterday", None), PIVOT)
        with self.assertRaises(group_registry.InstantError):
            group_registry.group_for(mission("a", None, None), "soon")


class Regroup(unittest.TestCase):
    def test_every_previous_group_is_recomputed_rather_than_kept(self):
        document = {
            "schema": 1,
            "missions": [
                mission("a", "2026-09-19T00:00:00Z", "2026-09-19T12:00:00Z", "after"),
                mission("b", "2026-09-20T09:00:00Z", "2026-09-20T12:00:00Z", "before"),
            ],
        }
        found = group_registry.regroup(document, PIVOT)
        self.assertEqual([one["group"] for one in found["missions"]], ["before", "after"])

    def test_the_source_registry_is_left_alone(self):
        source = mission("a", "2026-09-19T00:00:00Z", "2026-09-19T12:00:00Z", None)
        group_registry.regroup({"schema": 1, "missions": [source]}, PIVOT)
        self.assertIsNone(source["group"])

    def test_every_other_field_survives_the_grouping(self):
        found = mission("a", "2026-09-19T00:00:00Z", "2026-09-19T12:00:00Z")
        found["sessions"] = ["8f14e45f-ceea-467a-9d1a-000000000000"]
        found["note"] = "kept"
        grouped = group_registry.regroup({"schema": 1, "missions": [found]}, PIVOT)
        entry = grouped["missions"][0]
        self.assertEqual(entry["sessions"], ["8f14e45f-ceea-467a-9d1a-000000000000"])
        self.assertEqual(entry["note"], "kept")
        self.assertEqual(entry["pr"], 1)


class Command(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.registry = os.path.join(self.directory.name, "missions.json")
        with open(self.registry, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(
                {
                    "schema": 1,
                    "missions": [
                        mission("a", "2026-09-19T00:00:00Z", "2026-09-19T12:00:00Z"),
                        mission("b", "2026-09-20T09:00:00Z", "2026-09-20T12:00:00Z"),
                    ],
                },
                stream,
            )

    def test_the_command_writes_lf_terminated_canonical_json(self):
        out = os.path.join(self.directory.name, "grouped.json")
        code = group_registry.main(
            ["--registry", self.registry, "--pivot", PIVOT, "--out", out]
        )
        self.assertEqual(code, 0)
        with open(out, "rb") as stream:
            raw = stream.read()
        self.assertNotIn(b"\r\n", raw)
        self.assertTrue(raw.endswith(b"\n"))
        found = json.loads(raw.decode("utf-8"))
        self.assertEqual(
            [one["group"] for one in found["missions"]], ["before", "after"]
        )

    def test_two_runs_over_the_same_registry_are_byte_identical(self):
        first = os.path.join(self.directory.name, "one.json")
        second = os.path.join(self.directory.name, "two.json")
        for where in (first, second):
            group_registry.main(
                ["--registry", self.registry, "--pivot", PIVOT, "--out", where]
            )
        with open(first, "rb") as one, open(second, "rb") as two:
            self.assertEqual(one.read(), two.read())

    def test_a_registry_of_an_unknown_schema_is_refused(self):
        broken = os.path.join(self.directory.name, "broken.json")
        with open(broken, "w", encoding="utf-8", newline="\n") as stream:
            json.dump({"schema": 99, "missions": []}, stream)
        with self.assertRaises(SystemExit):
            group_registry.main(["--registry", broken, "--pivot", PIVOT])


class NotARegistry(unittest.TestCase):
    """A document that is not a registry is refused, not grouped to all-null.

    A measurement report also carries `schema: 1` and a `missions` list, and its
    entries keep their window under a nested `window` key. Grouped, every one of
    them would come back `null` -- a well-formed document that reads exactly
    like an honest "every mission straddled the pivot". That is the worst
    available failure mode for a campaign whose output is verdicts, and it is
    one mistyped `--registry` away.
    """

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)

    def write(self, document):
        path = os.path.join(self.directory.name, "document.json")
        with open(path, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(document, stream)
        return path

    def test_a_measurement_report_is_refused(self):
        report = self.write(
            {
                "schema": 1,
                "missions": [
                    {
                        "branch": "feat/example",
                        "pr": 1,
                        "group": None,
                        "sessions": [{"session": "8f14e45f", "method": "window"}],
                        "window": {
                            "started_at": "2026-09-19T00:00:00+00:00",
                            "ended_at": "2026-09-19T12:00:00+00:00",
                        },
                    }
                ],
            }
        )
        with self.assertRaises(group_registry.RegistryError):
            group_registry.load(report)

    def test_the_committed_baseline_report_is_refused(self):
        here = os.path.dirname(os.path.dirname(HERE))
        report = os.path.join(here, "docs", "quality", "velocity", "baseline-report.json")
        if not os.path.isfile(report):
            self.skipTest("the committed baseline report is not in this tree")
        with self.assertRaises(group_registry.RegistryError):
            group_registry.load(report)

    def test_a_document_with_no_missions_list_is_refused(self):
        path = self.write({"schema": 1, "metric": "opening_prompt_tokens"})
        with self.assertRaises(group_registry.RegistryError):
            group_registry.load(path)


class CommittedRegistries(unittest.TestCase):
    """The seven grouped registries still re-derive from the one registry.

    `docs/quality/velocity/baseline.md` states that each of them is exactly this
    tool run at that pull request's merge instant, and the report's four
    wall-clock tables are reconciled through them. The population is defined by
    a rule over a window that grows, so the day an 88th mission joins
    `missions.json` the seven copies go stale and the tables quietly grade a
    different population than the registry holds. This is what notices.
    """

    PIVOTS = {
        546: "2026-09-20T01:43:13Z",
        547: "2026-09-19T21:42:46Z",
        550: "2026-09-20T03:40:45Z",
        552: "2026-09-20T01:55:27Z",
        553: "2026-09-20T10:12:23Z",
        556: "2026-09-20T15:41:43Z",
        557: "2026-09-20T21:45:30Z",
    }

    def setUp(self):
        self.repo = os.path.dirname(os.path.dirname(HERE))
        self.registry = os.path.join(
            self.repo, "docs", "quality", "mission-cost", "missions.json"
        )
        self.grouped = os.path.join(self.repo, "docs", "quality", "velocity", "registries")
        if not os.path.isfile(self.registry) or not os.path.isdir(self.grouped):
            self.skipTest("the committed registries are not in this tree")

    def test_each_committed_registry_is_byte_identical_to_a_fresh_grouping(self):
        document = group_registry.load(self.registry)
        for pr, pivot in sorted(self.PIVOTS.items()):
            path = os.path.join(self.grouped, f"pr-{pr}.json")
            with self.subTest(pr=pr):
                self.assertTrue(os.path.isfile(path), path)
                fresh = group_registry.render(
                    group_registry.regroup(document, pivot)
                ).encode("utf-8")
                with open(path, "rb") as stream:
                    self.assertEqual(stream.read(), fresh)

    def test_each_committed_registry_covers_the_whole_population(self):
        document = group_registry.load(self.registry)
        expected = len(document["missions"])
        for pr in sorted(self.PIVOTS):
            path = os.path.join(self.grouped, f"pr-{pr}.json")
            with self.subTest(pr=pr), open(path, encoding="utf-8") as stream:
                self.assertEqual(len(json.load(stream)["missions"]), expected)


if __name__ == "__main__":
    unittest.main()
