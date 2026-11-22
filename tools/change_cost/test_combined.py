"""C14 semantic tests: python tools/change_cost/test_combined.py"""

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from combined import Counts, compare, history_rows, validate_history, validate_results
from measure import (
    BASE,
    DEFAULT_EVIDENCE_DIR,
    INTEGRATION_BASE,
    REQUIRED_COMMANDS,
    arguments,
    validate_canonical,
    validate_evidence,
)


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
    def test_measurement_inputs_are_caller_selected_and_portable(self):
        selected = arguments(["--evidence", "one.json", "--command-results", "two.json"])
        self.assertEqual(selected.evidence, Path("one.json"))
        self.assertEqual(selected.command_results, Path("two.json"))
        defaults = arguments([])
        self.assertEqual(defaults.evidence.parent, DEFAULT_EVIDENCE_DIR)
        self.assertEqual(defaults.command_results.parent, DEFAULT_EVIDENCE_DIR)

    def test_every_command_is_unique_successful_and_sha_pinned(self):
        required = ["C0", "C13"]
        head, tree, base = "a" * 40, "b" * 40, "c" * 40
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            results = self.command_results(root, required, head, tree, base)
            validate_results(required, results, head, tree, base, root)
            bad = [
                [{"command": "C0", "exit_code": 0, "sha": "a"}],
                [{**results[0], "exit_code": 1}, results[1]],
                [results[0], results[0]],
                [{**results[0], "sha": "d" * 40}, results[1]],
            ]
            for candidate in bad:
                with self.assertRaises(ValueError):
                    validate_results(required, candidate, head, tree, base, root)

    @staticmethod
    def command_results(root, required, head, tree, base):
        results = []
        for name in required:
            output = root / f"{name}.log"
            output.write_bytes(f"native output for {name}\n".encode())
            capture = {
                "command": name,
                "sha": head,
                "tree": tree,
                "base": base,
                "outputs": [
                    {
                        "path": output.name,
                        "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                    }
                ],
            }
            capture_path = root / f"{name}.capture.json"
            capture_path.write_text(json.dumps(capture), encoding="utf-8")
            results.append(
                {
                    "command": name,
                    "exit_code": 0,
                    "sha": head,
                    "tree": tree,
                    "base": base,
                    "capture": capture_path.name,
                }
            )
        return results

    def test_command_evidence_requires_bound_captured_output(self):
        head, tree, base = "a" * 40, "b" * 40, "c" * 40
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            results = self.command_results(root, ["C0", "C13"], head, tree, base)
            validate_results(["C0", "C13"], results, head, tree, base, root)

            (root / "C0.log").unlink()
            with self.assertRaisesRegex(ValueError, "C0"):
                validate_results(["C0", "C13"], results, head, tree, base, root)
            (root / "C0.log").write_bytes((root / "C13.log").read_bytes())
            with self.assertRaisesRegex(ValueError, "C0"):
                validate_results(["C0", "C13"], results, head, tree, base, root)
            results = self.command_results(root, ["C0", "C13"], head, tree, base)
            results[0]["capture"] = "C13.capture.json"
            with self.assertRaisesRegex(ValueError, "C0"):
                validate_results(["C0", "C13"], results, head, tree, base, root)
            results = self.command_results(root, ["C0", "C13"], head, tree, base)
            capture = json.loads((root / "C0.capture.json").read_text(encoding="utf-8"))
            capture["sha"] = "d" * 40
            (root / "C0.capture.json").write_text(json.dumps(capture), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "C0"):
                validate_results(["C0", "C13"], results, head, tree, base, root)

    def test_canonical_baseline_rejects_identity_version_and_count_mutations(self):
        canonical = {
            "tool": "tools/outside_score/measure.py",
            "version": 1,
            "production": BASE.production,
            "test": BASE.tests,
            "panic": BASE.panic,
        }
        validate_canonical(canonical)
        for field, value in [
            ("tool", "other.py"),
            ("version", 2),
            ("production", BASE.production + 1),
            ("test", BASE.tests + 1),
            ("panic", BASE.panic + 1),
        ]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_canonical({**canonical, field: value})

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
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            results = self.command_results(root, REQUIRED_COMMANDS, head, tree, INTEGRATION_BASE)
            evidence = {"history": [{"metric": "future", "state": "measured", "value": 0, "sources": ["x"], "after": "later"}]}
            with self.assertRaises(ValueError):
                validate_evidence(evidence, results, head, tree, root)
            evidence["history"][0]["state"] = "unobservable"
            results[0]["sha"] = "c" * 40
            with self.assertRaises(ValueError):
                validate_evidence(evidence, results, head, tree, root)


if __name__ == "__main__":
    unittest.main()
