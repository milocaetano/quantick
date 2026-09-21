#!/usr/bin/env python3
"""Offline tests for the dispersion summary and the registered reduction rule."""

import importlib.util
import os
import sys
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


dispersion = load("dispersion")


class Thresholds(unittest.TestCase):
    def test_the_registered_numbers_are_the_ones_in_the_method(self):
        self.assertEqual(dispersion.MIN_GROUP_N, 5)
        self.assertEqual(dispersion.MIN_RELATIVE_CHANGE, 0.10)
        self.assertEqual(dispersion.MAX_UNPLACED_SHARE, 0.20)


class Summary(unittest.TestCase):
    def test_a_summary_carries_dispersion_and_not_only_a_mean(self):
        found = dispersion.summary([1, 2, 3, 4, 5])
        self.assertEqual(
            found,
            {
                "n": 5,
                "min": 1.0,
                "p25": 2.0,
                "median": 3.0,
                "p75": 4.0,
                "max": 5.0,
                "mean": 3.0,
            },
        )

    def test_the_mean_never_stands_alone(self):
        self.assertEqual(
            sorted(dispersion.summary([1, 2])),
            ["max", "mean", "median", "min", "n", "p25", "p75"],
        )

    def test_one_value_reports_itself_at_every_percentile(self):
        found = dispersion.summary([7])
        self.assertEqual(found["n"], 1)
        self.assertEqual(found["p25"], 7.0)
        self.assertEqual(found["median"], 7.0)
        self.assertEqual(found["p75"], 7.0)

    def test_no_values_report_null_rather_than_zero(self):
        found = dispersion.summary([])
        self.assertEqual(found["n"], 0)
        self.assertIsNone(found["median"])
        self.assertIsNone(found["mean"])

    def test_a_skewed_sample_shows_its_skew(self):
        found = dispersion.summary([1, 1, 1, 1, 100])
        self.assertEqual(found["median"], 1.0)
        self.assertEqual(found["mean"], 20.8)


def compare(before, after, **kwargs):
    options = {
        "unplaced_share_before": 0.0,
        "unplaced_share_after": 0.0,
        "windows_overlap": False,
    }
    options.update(kwargs)
    return dispersion.compare(before, after, **options)


class Verdict(unittest.TestCase):
    def test_a_clear_fall_past_the_before_quartile_is_a_reduction(self):
        found = compare([100] * 5, [50] * 5)
        self.assertEqual(found["verdict"], "reduction")
        self.assertAlmostEqual(found["relative_change"], 0.5)

    def test_a_fall_smaller_than_a_tenth_is_only_directional(self):
        found = compare([100] * 5, [95] * 5)
        self.assertEqual(found["verdict"], "directional")

    def test_a_fall_that_does_not_clear_the_before_quartile_is_directional(self):
        found = compare([10, 50, 100, 150, 200], [60] * 5)
        self.assertEqual(found["verdict"], "directional")

    def test_a_median_that_did_not_fall_is_no_reduction(self):
        self.assertEqual(compare([100] * 5, [100] * 5)["verdict"], "no_reduction")
        self.assertEqual(compare([100] * 5, [140] * 5)["verdict"], "no_reduction")

    def test_too_few_missions_is_inconclusive_however_large_the_fall(self):
        found = compare([100] * 4, [1] * 9)
        self.assertEqual(found["verdict"], "inconclusive")
        self.assertIn("n", found["reason"])

    def test_weak_attribution_outranks_every_other_verdict(self):
        found = compare([100] * 5, [1] * 5, unplaced_share_after=0.21)
        self.assertEqual(found["verdict"], "cannot_be_attributed")

    def test_overlapping_windows_cannot_be_attributed(self):
        found = compare([100] * 5, [1] * 5, windows_overlap=True)
        self.assertEqual(found["verdict"], "cannot_be_attributed")

    def test_the_share_threshold_is_inclusive_at_its_edge(self):
        found = compare([100] * 5, [50] * 5, unplaced_share_before=0.20)
        self.assertEqual(found["verdict"], "reduction")

    def test_an_empty_before_group_is_inconclusive_not_a_division_by_zero(self):
        found = compare([], [50] * 5)
        self.assertEqual(found["verdict"], "inconclusive")
        self.assertIsNone(found["relative_change"])

    def test_a_result_always_carries_both_groups_and_both_counts(self):
        found = compare([100] * 5, [50] * 5)
        self.assertEqual(found["before"]["n"], 5)
        self.assertEqual(found["after"]["n"], 5)
        self.assertEqual(found["before"]["median"], 100.0)
        self.assertEqual(found["after"]["median"], 50.0)

    def test_a_zero_before_median_cannot_yield_a_relative_change(self):
        found = compare([0] * 5, [0] * 5)
        self.assertIsNone(found["relative_change"])
        self.assertEqual(found["verdict"], "no_reduction")


if __name__ == "__main__":
    unittest.main()
