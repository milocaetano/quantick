#!/usr/bin/env python3
"""Cases for the default-binary hook scan.

The scan itself reads a half-gigabyte artifact, so these run against bytes
built here. The first class of case is the one that matters: the fast one-pass
scan must accept exactly the names the per-name lookbehind accepted, because
the whole claim of the rewrite is that it changed the cost and not the
verdict. `reference_present` is that old form, kept as an oracle.
"""

import random
import re
import sys
import unittest

sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parent))

import binary_hook_scan as scan  # noqa: E402


def reference_present(binary, names):
    """The pre-rewrite test, verbatim in behaviour: one lookbehind per name."""
    return sorted(
        name
        for name in names
        if re.search(
            rb"(?<![A-Z0-9_])" + name.encode() + rb"(?![A-Z0-9_])",
            binary,
        )
    )


LAUNCH = """
    declare_hooks![
        ("QUANTICK_CONFIG", config),
        ("QUANTICK_FEEDS", feeds),
    ];
    let other = "QUANTICK_NOT_DECLARED";
"""

REGISTRY = """
| Hook | Meaning |
| --- | --- |
| `QUANTICK_CONFIG` | configuration, not a harness hook |
| `QUANTICK_CAPTURE` | a harness hook |
| `QUANTICK_LOAD_OLDER` | a harness hook named in published prose |
"""


class BoundaryNames(unittest.TestCase):
    def test_a_name_on_its_own_is_found(self):
        self.assertIn("QUANTICK_CAPTURE", scan.boundary_names(b"\x00QUANTICK_CAPTURE\x00"))

    def test_a_name_at_offset_zero_is_found(self):
        self.assertIn("QUANTICK_CAPTURE", scan.boundary_names(b"QUANTICK_CAPTURE\x00"))

    def test_a_name_continued_on_the_right_is_a_different_name(self):
        found = scan.boundary_names(b"\x00QUANTICK_CAPTUREX\x00")
        self.assertNotIn("QUANTICK_CAPTURE", found)
        self.assertIn("QUANTICK_CAPTUREX", found)

    def test_a_name_preceded_by_a_name_character_is_rejected(self):
        self.assertEqual(set(), scan.boundary_names(b"\x00XQUANTICK_CAPTURE\x00"))

    def test_a_lowercase_neighbour_is_not_a_name_character(self):
        # The class the old pattern used is upper case, digits and underscore,
        # so a lowercase byte is a boundary like any other.
        self.assertIn("QUANTICK_CAPTURE", scan.boundary_names(b"xQUANTICK_CAPTUREx"))

    def test_several_names_in_one_buffer(self):
        buffer = b"\x00QUANTICK_A\x00padding\x00QUANTICK_B\x00"
        self.assertEqual({"QUANTICK_A", "QUANTICK_B"}, scan.boundary_names(buffer))


class MatchesTheOldScan(unittest.TestCase):
    """Random buffers, both forms, same answer."""

    def test_randomised_equivalence(self):
        names = [f"QUANTICK_{letter}" for letter in "ABCDEFGH"]
        neighbours = [b"", b"\x00", b"X", b"_", b"9", b"x", b"\xff"]
        rng = random.Random(20260919)
        for _ in range(200):
            pieces = []
            for _ in range(rng.randrange(0, 12)):
                name = rng.choice(names).encode()
                pieces.append(rng.choice(neighbours) + name + rng.choice(neighbours))
            binary = b"\x00".join(pieces)
            fast = sorted(set(names) & scan.boundary_names(binary))
            self.assertEqual(reference_present(binary, names), fast, binary)


class Verdict(unittest.TestCase):
    def test_a_clean_binary_passes(self):
        binary = b"\x00QUANTICK_CONFIG\x00QUANTICK_FEEDS\x00"
        configuration, hooks, missing, present = scan.verdict(binary, LAUNCH, REGISTRY)
        self.assertEqual({"QUANTICK_CONFIG", "QUANTICK_FEEDS"}, configuration)
        self.assertEqual({"QUANTICK_CAPTURE", "QUANTICK_LOAD_OLDER"}, hooks)
        self.assertEqual([], missing)
        self.assertEqual([], present)

    def test_a_harness_hook_in_the_binary_is_reported(self):
        binary = b"\x00QUANTICK_CONFIG\x00QUANTICK_FEEDS\x00QUANTICK_CAPTURE\x00"
        _, _, missing, present = scan.verdict(binary, LAUNCH, REGISTRY)
        self.assertEqual([], missing)
        self.assertEqual(["QUANTICK_CAPTURE"], present)

    def test_the_prose_hook_is_exempt(self):
        binary = b"\x00QUANTICK_CONFIG\x00QUANTICK_FEEDS\x00QUANTICK_LOAD_OLDER\x00"
        _, _, _, present = scan.verdict(binary, LAUNCH, REGISTRY)
        self.assertEqual([], present)

    def test_missing_configuration_is_reported(self):
        binary = b"\x00QUANTICK_CONFIG\x00"
        _, _, missing, _ = scan.verdict(binary, LAUNCH, REGISTRY)
        self.assertEqual(["QUANTICK_FEEDS"], missing)

    def test_a_declared_name_outside_the_macro_is_not_configuration(self):
        configuration = scan.declared_configuration(LAUNCH)
        self.assertNotIn("QUANTICK_NOT_DECLARED", configuration)


if __name__ == "__main__":
    unittest.main(verbosity=2)
