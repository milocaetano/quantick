#!/usr/bin/env python3
"""Deterministic orchestration fixtures: python tools/ci/test_ratchet_delta.py."""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ratchet_delta as delta


class MeasurementTests(unittest.TestCase):
    def test_changed_measurement_requires_exact_signed_coverage(self):
        before = {"ratchet.app-lines.measured": 20}
        after = {"ratchet.app-lines.measured": 21}
        for signed in ({}, {"ratchet.app-lines.measured": -1}, {"other": 1}):
            with self.assertRaisesRegex(ValueError, "disagreement"):
                delta.compare_measurements(before, after, signed)
        delta.compare_measurements(before, after, {"ratchet.app-lines.measured": 1})

    def test_reports_refuse_failed_missing_and_duplicate_measurements(self):
        for text in ("", "ratchet.app-lines.measured\tfailed", "a\t1\na\t2"):
            with self.assertRaises(ValueError):
                delta.measurements(text)
        self.assertEqual(delta.measurements("x.measured\t3\nx.budget\t4"), {"x.measured": 3})

    def test_stale_coverage_cannot_be_reused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            head, base = root / "head", root / "base"
            relative = Path("crates/guards/app-lines-baseline.d/branch.txt")
            for tree in (head, base):
                (tree / relative).parent.mkdir(parents=True)
                (tree / relative).write_text("!measure-change x +1\n", encoding="utf-8")
            self.assertEqual(delta.coverage(head, base), {})
            (head / relative.parent / "new.txt").write_text("!measure-change x +1\n", encoding="utf-8")
            self.assertEqual(delta.coverage(head, base), {"x": 1})

    def test_changed_guard_or_baseline_triggers_old_binary_comparison(self):
        for name in ("crates/guards/src/size.rs", "crates/guards/app-lines-baseline.txt",
                     "crates/guards/app-lines-baseline.d/branch.txt"):
            with patch.object(delta, "run", side_effect=[name, ""]):
                self.assertTrue(delta.measurement_inputs_changed(Path("."), "base"))
        with patch.object(delta, "run", side_effect=["crates/guards/tests/cli.rs", ""]):
            self.assertFalse(delta.measurement_inputs_changed(Path("."), "base"))

    def test_old_binary_measures_head_and_unsigned_disagreement_fails(self):
        repo, base = Path("head"), Path("base")
        with patch.object(delta, "base_checkout", return_value=("sha", base)), \
                patch.object(delta, "measurement_inputs_changed", return_value=True), \
                patch.object(delta, "build", return_value=Path("old-binary")), \
                patch.object(delta, "report", side_effect=[{"x": 1}, {"x": 2}]) as report, \
                patch.object(delta, "coverage", return_value={}):
            with self.assertRaisesRegex(ValueError, "disagreement"):
                delta.check(repo, "target", Path("head-binary"))
            self.assertEqual(report.call_args_list[0].args, (Path("old-binary"), repo))
            self.assertEqual(report.call_args_list[1].args, (Path("head-binary"), repo))

    def test_failed_old_build_is_not_a_clean_measurement(self):
        with patch.object(delta, "base_checkout", return_value=("sha", Path("base"))), \
                patch.object(delta, "measurement_inputs_changed", return_value=True), \
                patch.object(delta, "build", side_effect=ValueError("build failed")):
            with self.assertRaisesRegex(ValueError, "build failed"):
                delta.check(Path("head"), "target", Path("head-binary"))


if __name__ == "__main__":
    unittest.main()
