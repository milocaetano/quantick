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


if __name__ == "__main__":
    unittest.main()
