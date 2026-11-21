"""C14 semantic tests: python tools/change_cost/test_combined.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from combined import Counts, compare, history_rows, validate_history, validate_results
from measure import INTEGRATION_BASE, REQUIRED_COMMANDS, validate_evidence


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
        head, tree, base = "a" * 40, "b" * 40, "c" * 40
        validate_results(required, [
            {"command": "C0", "exit_code": 0, "sha": head, "tree": tree, "base": base},
            {"command": "C13", "exit_code": 0, "sha": head, "tree": tree, "base": base},
        ], head, tree, base)
        bad = [
            [{"command": "C0", "exit_code": 0, "sha": "a"}],
            [{"command": "C0", "exit_code": 1, "sha": head, "tree": tree, "base": base}, {"command": "C13", "exit_code": 0, "sha": head, "tree": tree, "base": base}],
            [{"command": "C0", "exit_code": 0, "sha": head, "tree": tree, "base": base}, {"command": "C0", "exit_code": 0, "sha": head, "tree": tree, "base": base}],
            [{"command": "C0", "exit_code": 0, "sha": "d" * 40, "tree": tree, "base": base}, {"command": "C13", "exit_code": 0, "sha": head, "tree": tree, "base": base}],
        ]
        for results in bad:
            with self.assertRaises(ValueError):
                validate_results(required, results, head, tree, base)

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

    def test_history_derivation_rejects_a_contradictory_source_state(self):
        with self.assertRaises(ValueError):
            history_rows([{"metric": "future", "state": "measured", "value": 0, "sources": ["x"], "after": "later"}])

    def test_measurement_boundary_rejects_stale_commands_and_history(self):
        head, tree = "a" * 40, "b" * 40
        results = [
            {"command": command, "exit_code": 0, "sha": head, "tree": tree, "base": INTEGRATION_BASE}
            for command in REQUIRED_COMMANDS
        ]
        evidence = {"history": [{"metric": "future", "state": "measured", "value": 0, "sources": ["x"], "after": "later"}]}
        with self.assertRaises(ValueError):
            validate_evidence(evidence, results, head, tree)
        evidence["history"][0]["state"] = "unobservable"
        results[0]["sha"] = "c" * 40
        with self.assertRaises(ValueError):
            validate_evidence(evidence, results, head, tree)


if __name__ == "__main__":
    unittest.main()
