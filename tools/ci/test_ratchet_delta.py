#!/usr/bin/env python3
"""Deterministic orchestration fixtures: python tools/ci/test_ratchet_delta.py."""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ratchet_delta as delta


class MeasurementTests(unittest.TestCase):
    def test_signed_coverage_cannot_delete_a_required_measurement(self):
        row = "ratchet.app-lines.measured"
        with self.assertRaisesRegex(ValueError, "missing required"):
            delta.compare_measurements({row: 20}, {}, {row: -20})

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

    def test_cached_base_rejects_ignored_source_that_would_inflate_measurements(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            delta.run(["git", "init", "-q"], repo)
            (repo / ".gitignore").write_text("__pycache__/\ntarget/\n", encoding="utf-8")
            delta.run(["git", "add", ".gitignore"], repo)
            delta.run(["git", "-c", "user.name=fixture", "-c", "user.email=fixture@example.test",
                       "commit", "-qm", "fixture"], repo)
            _, base = delta.base_checkout(repo, "HEAD")
            source = base / "crates/app/src/__pycache__/inflated.rs"
            source.parent.mkdir(parents=True)
            source.write_text("fn inflated() {}\n" * 1000, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "not pristine"):
                delta.base_checkout(repo, "HEAD")

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
            self.assertEqual(delta.coverage(head, base), {"x": delta.Coverage(1, None)})

    def test_anchored_coverage_is_read_from_current_and_base_fragments(self):
        anchor = "1" * 40
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            head, base = root / "head", root / "base"
            relative = Path("crates/guards/app-lines-baseline.d/branch.txt")
            for tree in (head, base):
                (tree / relative).parent.mkdir(parents=True)
                (tree / relative).write_text(
                    f"!measure-change graph.edges +1 {anchor}\n", encoding="utf-8")
            self.assertEqual(
                delta.coverage(head, base),
                {"graph.edges": delta.Coverage(1, anchor)},
            )

    def test_base_anchored_authority_is_semantically_immutable_in_every_ratchet(self):
        anchor = "1" * 40
        original = f"# base\n!measure-change graph.edges +1 {anchor}\n"
        changes = {
            "append": original + f"!measure-change other +1 {anchor}\n",
            "change": f"!measure-change graph.edges +2 {anchor}\n",
            "remove": "# authority removed\n",
            "duplicate_removed": original,
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            head, base = root / "head", root / "base"
            relative = Path("crates/guards/context-baseline.d/existing.txt")
            for tree in (head, base):
                (tree / relative).parent.mkdir(parents=True)
            (base / relative).write_text(original, encoding="utf-8")
            (head / relative).write_text(
                f"  !measure-change   graph.edges +1 {anchor} # same authority\n",
                encoding="utf-8",
            )
            self.assertEqual(
                delta.coverage(head, base),
                {"graph.edges": delta.Coverage(1, anchor)},
            )
            for name, changed in changes.items():
                with self.subTest(name=name):
                    base_text = original
                    if name == "duplicate_removed":
                        base_text += f"!measure-change graph.edges +1 {anchor}\n"
                    (base / relative).write_text(base_text, encoding="utf-8")
                    (head / relative).write_text(changed, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, "changed from base"):
                        delta.coverage(head, base)

    def test_invalid_missing_and_duplicate_anchor_coverage_fails_closed(self):
        anchor = "1" * 40
        cases = [
            "!measure-change x +1 short\n",
            f"!measure-change x +1 {anchor}\n!measure-change x +1 {anchor}\n",
            "!measure-change x +1 extra fields\n",
        ]
        for content in cases:
            with self.subTest(content=content), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                head, base = root / "head", root / "base"
                relative = Path("crates/guards/app-lines-baseline.d/branch.txt")
                (head / relative).parent.mkdir(parents=True)
                (head / relative).write_text(content, encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "anchor|duplicate|invalid"):
                    delta.coverage(head, base)

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

    def test_anchored_current_base_uses_normal_exact_comparison(self):
        anchor = "a" * 40
        declaration = {"x": delta.Coverage(1, anchor)}
        with patch.object(delta, "coverage", return_value=declaration), \
                patch.object(delta, "build", return_value=Path("base-binary")), \
                patch.object(delta, "report", side_effect=[{"x": 1}, {"x": 2}]), \
                patch.object(delta, "compare_measurements") as compare:
            delta.verify_measurements(Path("head"), anchor, Path("base"), Path("head-binary"))
            compare.assert_called_once_with({"x": 1}, {"x": 2}, {"x": 1})

    def test_anchored_historical_child_measures_same_head_with_three_binaries(self):
        anchor, base_sha = "a" * 40, "b" * 40
        declaration = {"x": delta.Coverage(1, anchor)}
        with patch.object(delta, "coverage", return_value=declaration), \
                patch.object(delta, "run", side_effect=["", anchor]), \
                patch.object(delta, "base_checkout", return_value=(anchor, Path("anchor"))), \
                patch.object(delta, "build", side_effect=[Path("base-bin"), Path("anchor-bin")]), \
                patch.object(delta, "report", side_effect=[{"x": 2}, {"x": 2}, {"x": 1}]) as report, \
                patch.object(delta, "compare_measurements") as compare:
            delta.verify_measurements(Path("head"), base_sha, Path("base"), Path("head-bin"))
            self.assertEqual([call.args[1] for call in report.call_args_list], [Path("head")] * 3)
            self.assertEqual(compare.call_args_list[0].args, ({"x": 2}, {"x": 2}, {}))
            self.assertEqual(compare.call_args_list[1].args, ({"x": 1}, {"x": 2}, {"x": 1}))
            self.assertEqual(compare.call_args_list[2].args, ({"x": 1}, {"x": 2}, {"x": 1}))

    def test_nonancestor_anchor_fails_before_anchor_build(self):
        anchor, base_sha = "a" * 40, "b" * 40
        with patch.object(delta, "coverage", return_value={"x": delta.Coverage(1, anchor)}), \
                patch.object(delta, "build", return_value=Path("base-bin")), \
                patch.object(delta, "report", side_effect=[{"x": 2}, {"x": 2}]), \
                patch.object(delta, "run", side_effect=ValueError("not ancestor")):
            with self.assertRaisesRegex(ValueError, "not an ancestor"):
                delta.verify_measurements(Path("head"), base_sha, Path("base"), Path("head-bin"))

    def test_anchored_stale_missing_and_unrelated_rows_fail_exactly(self):
        anchor, base_sha = "a" * 40, "b" * 40
        for base_report, head_report, signed in [
            ({"x": 2}, {"x": 3}, {"x": 1}),
            ({"x": 2}, {"x": 2}, {"missing": 1}),
            ({"x": 2, "other": 2}, {"x": 2, "other": 2}, {"x": 1}),
        ]:
            with self.subTest(signed=signed), patch.object(
                delta, "coverage", return_value={key: delta.Coverage(value, anchor)
                                                  for key, value in signed.items()}
            ), patch.object(delta, "build", side_effect=[Path("base-bin"), Path("anchor-bin")]), \
                    patch.object(delta, "report", side_effect=[base_report, head_report, {"x": 1, "other": 1}]), \
                    patch.object(delta, "run", side_effect=["", anchor]), \
                    patch.object(delta, "base_checkout", return_value=(anchor, Path("anchor"))):
                with self.assertRaises(ValueError):
                    delta.verify_measurements(Path("head"), base_sha, Path("base"), Path("head-bin"))

    def test_failed_old_build_is_not_a_clean_measurement(self):
        with patch.object(delta, "base_checkout", return_value=("sha", Path("base"))), \
                patch.object(delta, "measurement_inputs_changed", return_value=True), \
                patch.object(delta, "build", side_effect=ValueError("build failed")):
            with self.assertRaisesRegex(ValueError, "build failed"):
                delta.check(Path("head"), "target", Path("head-binary"))


if __name__ == "__main__":
    unittest.main()
