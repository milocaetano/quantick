"""C14 semantic tests: python tools/change_cost/test_combined.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from combined import Counts, compare, validate_history, validate_results


class ExactRatios(unittest.TestCase):
    def test_equal_and_one_unit_boundaries(self):
        base = Counts(10, 20, 2)
        self.assertTrue(compare(base, Counts(20, 40, 4))["test_non_regression"])
        self.assertTrue(compare(base, Counts(20, 41, 4))["test_non_regression"])
        self.assertFalse(compare(base, Counts(20, 39, 4))["test_non_regression"])
        self.assertTrue(compare(base, Counts(20, 40, 3))["panic_non_regression"])
        self.assertFalse(compare(base, Counts(20, 40, 5))["panic_non_regression"])

    def test_rounded_display_cannot_hide_regression(self):
        result = compare(Counts(268631, 311966, 111), Counts(271047, 313459, 111))
        self.assertFalse(result["test_non_regression"])
        self.assertEqual(result["test_deficit"], 352643773)
        self.assertTrue(result["panic_non_regression"])

    def test_invalid_counts_are_rejected(self):
        for counts in [Counts(0, 1, 1), Counts(1, -1, 1), Counts(1, 1, -1)]:
            with self.assertRaises(ValueError):
                compare(counts, Counts(1, 1, 1))


class EvidenceContracts(unittest.TestCase):
    def test_every_command_is_unique_successful_and_sha_pinned(self):
        required = ["C0", "C13"]
        validate_results(required, [
            {"command": "C0", "exit_code": 0, "sha": "a" * 40},
            {"command": "C13", "exit_code": 0, "sha": "a" * 40},
        ])
        bad = [
            [{"command": "C0", "exit_code": 0, "sha": "a"}],
            [{"command": "C0", "exit_code": 1, "sha": "a"}, {"command": "C13", "exit_code": 0, "sha": "a"}],
            [{"command": "C0", "exit_code": 0, "sha": "a"}, {"command": "C0", "exit_code": 0, "sha": "a"}],
            [{"command": "C0", "exit_code": 0, "sha": ""}, {"command": "C13", "exit_code": 0, "sha": "a"}],
        ]
        for results in bad:
            with self.assertRaises(ValueError):
                validate_results(required, results)

    def test_future_windows_are_gaps_and_measured_rows_keep_sources(self):
        validate_history([
            {"metric": "current", "state": "measured", "value": 3, "sources": ["run.json"]},
            {"metric": "after_14d", "state": "unobservable", "value": None, "sources": ["baseline.json"], "after": "14 days after main merge"},
        ])
        for row in [
            {"metric": "future", "state": "measured", "value": 0, "sources": ["x"], "after": "later"},
            {"metric": "past", "state": "measured", "value": 1, "sources": []},
            {"metric": "future", "state": "unobservable", "value": None, "sources": ["x"]},
        ]:
            with self.assertRaises(ValueError):
                validate_history([row])


if __name__ == "__main__":
    unittest.main()
