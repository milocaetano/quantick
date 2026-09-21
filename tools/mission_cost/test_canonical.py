#!/usr/bin/env python3
"""Offline tests for the shared canonical-JSON writer."""

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


canonical = load("canonical")


class Render(unittest.TestCase):
    def test_keys_are_sorted_and_the_text_ends_in_one_newline(self):
        found = canonical.render({"b": 1, "a": {"d": 2, "c": 3}})
        self.assertTrue(found.endswith("}\n"))
        self.assertFalse(found.endswith("}\n\n"))
        self.assertLess(found.index('"a"'), found.index('"b"'))
        self.assertLess(found.index('"c"'), found.index('"d"'))

    def test_non_ascii_is_escaped_rather_than_encoded(self):
        found = canonical.render({"name": "López"})
        self.assertIn("\\u00f3", found)
        self.assertEqual(json.loads(found)["name"], "López")


class Emit(unittest.TestCase):
    def test_a_file_gets_lf_endings_and_nothing_else(self):
        with tempfile.TemporaryDirectory() as where:
            path = os.path.join(where, "out.json")
            canonical.emit("one\ntwo\n", path)
            with open(path, "rb") as stream:
                raw = stream.read()
        self.assertEqual(raw, b"one\ntwo\n")


class OneOwner(unittest.TestCase):
    def test_every_command_in_the_package_writes_through_this_module(self):
        for name in ("measure", "group_registry", "opening_frame"):
            module = load(name)
            self.assertIs(module.render, canonical.render, name)
            self.assertIs(module.emit, canonical.emit, name)

    def test_every_command_that_compares_instants_uses_the_one_rule(self):
        """The older contract, and the more load-bearing one.

        Which side of a pivot an instant falls on decides every verdict this
        package produces. A second implementation that tolerated a naive
        timestamp, or compared before normalising to UTC, would group a mission
        the opposite way from `measure.py` over the same registry and nothing
        would notice. So the rule has one owner, like the byte contract.
        """
        transcripts = load("transcripts")
        for name in ("group_registry", "opening_frame"):
            module = load(name)
            self.assertIs(module.instant, transcripts.require_instant, name)
            self.assertIs(module.InstantError, transcripts.InstantError, name)

    def test_the_registry_reader_refuses_a_bad_instant_through_the_same_rule(self):
        """`attribution` re-raises it as a registry fault, and says so."""
        attribution = load("attribution")
        with self.assertRaises(attribution.RegistryError):
            attribution._instant({"branch": "feat/x", "ended_at": "soon"}, "ended_at")


if __name__ == "__main__":
    unittest.main()
