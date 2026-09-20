#!/usr/bin/env python3
"""Offline tests for what a measurement says about the files it counted."""

import importlib.util
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "quantick_read_cost_shape_under_test", os.path.join(HERE, "shape.py")
)
shape = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = shape
SPEC.loader.exec_module(shape)


def report(files):
    return {
        "files": [
            {"crate": "core", "lines": lines, "path": path, "roles": sorted(roles)}
            for path, lines, roles in files
        ]
    }


class Files(unittest.TestCase):
    def setUp(self):
        self.measured = report(
            [
                ("crates/core/src/lib.rs", 10, ["touched"]),
                ("crates/core/src/both.rs", 500, ["referenced", "touched"]),
                ("crates/core/src/big.rs", 400, ["referenced"]),
                ("crates/core/src/small.rs", 20, ["referenced"]),
            ]
        )

    def test_a_touched_file_is_never_offered_as_detachable(self):
        # `both.rs` is the case: the diff already opened it, so detaching the
        # reference would save a reader nothing.
        self.assertEqual(
            [entry["path"] for entry in shape.referenced_files(self.measured)],
            ["crates/core/src/big.rs", "crates/core/src/small.rs"],
        )
        self.assertEqual(len(shape.touched_files(self.measured)), 2)

    def test_the_largest_comes_first_and_the_list_is_capped(self):
        many = report(
            [(f"crates/core/src/r{index}.rs", index, ["referenced"]) for index in range(9)]
        )

        top = shape.top_referenced(many)

        self.assertEqual(len(top), shape.TOP_REFERENCED)
        self.assertEqual([entry["lines"] for entry in top], [8, 7, 6, 5, 4])
        self.assertEqual(len(shape.top_referenced(many, 2)), 2)

    def test_files_of_equal_size_are_ordered_by_path(self):
        tied = report(
            [
                ("crates/core/src/b.rs", 5, ["referenced"]),
                ("crates/core/src/a.rs", 5, ["referenced"]),
            ]
        )

        self.assertEqual(
            [entry["path"] for entry in shape.referenced_files(tied)],
            ["crates/core/src/a.rs", "crates/core/src/b.rs"],
        )


if __name__ == "__main__":
    unittest.main()
