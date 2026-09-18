#!/usr/bin/env python3
"""Offline edit-loop safety fixtures; no fixture launches Cargo."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

import budgets
import inputs
import sampling
import snapshot


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.output = self.root / "output"
        self.output.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def write(self, relative, content):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8", newline="\n")
        return path

    def git(self, *args):
        return subprocess.check_output(
            ["git", "-C", str(self.repo), *args], text=True
        ).strip()

    def commit(self):
        self.git("init", "--quiet")
        self.git("config", "core.autocrlf", "false")
        self.git("add", "-A")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "-m", "fixture", "--quiet")
        return self.git("rev-parse", "HEAD")

    def measurement_inputs(self):
        self.write("Cargo.toml", "[workspace]\nmembers=[]\n")
        self.write("Cargo.lock", "# Offline fixture, never compiled.\n")
        self.write("rust-toolchain.toml", '[toolchain]\nchannel="fixture"\n')
        self.write("crates/app/config/bubbles.toml", "# Offline tracked configuration.\n")
        self.write("tools/edit_loop/measure.py", "# Offline identity fixture; never executed.\n")
        self.write("tools/edit_loop/budgets.json", '{"schema":1,"status":"uncalibrated"}')
        self.write("tools/outside_score/measure.py", inputs.FROZEN_LEXER.read_text(encoding="utf-8"))


class SelectionTests(Fixture):
    def test_archive_paths_and_export_omissions_are_not_silently_accepted(self):
        for relative in ("../escape", "/absolute", "C:/outside", "crates/../../escape", "crates\\escape"):
            with self.subTest(relative=relative), self.assertRaises(ValueError):
                snapshot.relative_path(relative)
        self.measurement_inputs()
        for name in ("a", "b", "c"):
            self.write(f"crates/{name}/Cargo.toml", f'[package]\nname="quantick-{name}"\n')
            self.write(f"crates/{name}/src/lib.rs", "pub fn fixture() {}\n")
        self.write(".gitattributes", "crates/a/src/lib.rs export-ignore\n")
        sha = self.commit()
        with self.assertRaisesRegex(ValueError, "omitted"):
            snapshot.facts(self.repo, sha)

    def test_frozen_ranking_excludes_tests_and_uses_stable_ties(self):
        for name, lines in (("b", 4), ("a", 4), ("c", 3), ("d", 2)):
            self.write(f"crates/{name}/Cargo.toml", f'[package]\nname="quantick-{name}"\n')
            self.write(f"crates/{name}/src/lib.rs", "pub fn item() {}\n" * lines)
        self.write("crates/d/tests/large.rs", "fn test() {}\n" * 200)
        self.write("crates/d/src/tests.rs", "fn test() {}\n" * 200)
        ranked = inputs.ranked_crates(self.repo)
        self.assertEqual([r["crate"] for r in ranked[:3]], ["a", "b", "c"])
        self.assertEqual([r["production_lines"] for r in ranked[:3]], [4, 4, 3])
        self.assertEqual(ranked[0]["package"], "quantick-a")

    def test_clean_exact_head_is_required(self):
        self.write("file", "original")
        sha = self.commit()
        inputs.clean_head(self.repo, sha)
        with self.assertRaisesRegex(ValueError, "commit"):
            snapshot.facts(self.repo, self.git("rev-parse", "HEAD^{tree}"))
        with self.assertRaisesRegex(ValueError, "exact"):
            inputs.clean_head(self.repo, sha[:12])
        self.write("untracked", "unexpected")
        with self.assertRaisesRegex(ValueError, "clean"):
            inputs.clean_head(self.repo, sha)

    def test_existing_or_overlapping_measurement_paths_are_refused(self):
        with self.assertRaises(ValueError):
            inputs.new_paths(self.repo, self.repo / "nested", self.root / "target", self.output)
        with self.assertRaises(ValueError):
            inputs.new_paths(self.repo, self.root / "bench", self.root / "bench/t", self.root / "out")

    def test_environment_normalizes_only_declared_fixture_and_rejects_build_overrides(self):
        env, changes = inputs.command_environment(self.repo, self.root / "target", 2,
                                                  {"PATH": "bin", "QUANTICK_BUBBLES": "personal"})
        self.assertEqual(env["QUANTICK_BUBBLES"], str(self.repo / "crates/app/config/bubbles.toml"))
        self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
        self.assertIn("QUANTICK_BUBBLES", changes)
        for overrides in ({"RUSTFLAGS": "-O"}, {"CARGO_INCREMENTAL": "0"},
                          {"QUANTICK_UPDATE_SCHEMAS": "1"}, {"RUSTC_WRAPPER": "sccache"}):
            with self.subTest(overrides=overrides), self.assertRaises(ValueError):
                inputs.command_environment(self.repo, self.root / "target", 2, overrides)


class TouchTests(Fixture):
    def test_hard_linked_source_is_refused_without_touching_either_path(self):
        path = self.write("crates/a/src/lib.rs", "pub fn item() {}\n")
        sha = self.commit()
        linked = self.root / "linked.rs"
        os.link(path, linked)
        before = linked.stat().st_mtime_ns
        with self.assertRaisesRegex(ValueError, "hard-linked"):
            sampling.SourceTouch(self.repo, path, sha, self.output)
        self.assertEqual(linked.stat().st_mtime_ns, before)

    def test_touch_waits_for_a_coarse_clock_to_pass_a_fresh_mtime(self):
        now = time.time_ns()
        self.assertGreater(sampling.SourceTouch._clock_past(now), now)

    def test_touch_still_refuses_a_clock_that_stays_behind(self):
        future = time.time_ns() + 60_000_000_000
        self.assertLessEqual(sampling.SourceTouch._clock_past(future, limit_ns=5_000_000), future)

    def test_touch_changes_only_mtime_and_restores_metadata_after_exception(self):
        path = self.write("crates/a/src/lib.rs", "pub fn unchanged() {}\n")
        sha = self.commit()
        before = path.stat()
        with self.assertRaisesRegex(RuntimeError, "injected"):
            with sampling.SourceTouch(self.repo, path, sha, self.output) as touched:
                touched.touch(now_ns=before.st_mtime_ns + 3_000_000_000)
                self.assertEqual(path.read_bytes(), b"pub fn unchanged() {}\n")
                self.assertGreater(path.stat().st_mtime_ns, before.st_mtime_ns)
                raise RuntimeError("injected")
        after = path.stat()
        self.assertEqual(after.st_mtime_ns, before.st_mtime_ns)
        self.assertEqual(after.st_atime_ns, before.st_atime_ns)
        self.assertEqual(after.st_mode, before.st_mode)
        recovery = json.loads((self.output / "recovery.json").read_text())
        self.assertEqual(recovery["state"], "restored")

    def test_concurrent_byte_change_is_never_overwritten(self):
        path = self.write("crates/a/src/lib.rs", "original")
        sha = self.commit()
        with self.assertRaisesRegex(ValueError, "changed"):
            with sampling.SourceTouch(self.repo, path, sha, self.output):
                path.write_text("someone else's edit", encoding="utf-8")
        self.assertEqual(path.read_text(), "someone else's edit")
        recovery = json.loads((self.output / "recovery.json").read_text())
        self.assertEqual(recovery["state"], "restore_refused")
        self.assertTrue((self.output / "source-original.bin").exists())

    def test_escape_untracked_test_and_symlink_sources_are_refused(self):
        path = self.write("crates/a/src/lib.rs", "pub fn item() {}")
        self.write("crates/a/tests/test.rs", "fn fixture() {}")
        sha = self.commit()
        for invalid in (self.root / "outside.rs", self.repo / "crates/a/tests/test.rs"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                sampling.SourceTouch(self.repo, invalid, sha, self.output)
        with patch("inputs.is_link", return_value=True), self.assertRaises(ValueError):
            sampling.SourceTouch(self.repo, path, sha, self.output)


class ObservationTests(unittest.TestCase):
    def test_compile_and_successful_tests_are_both_required(self):
        good = {"exit_code": 0, "elapsed_seconds": 3.0, "timed_out": False,
                "tree_quiescent": True, "orphaned_descendants": False,
                "stdout_text": "test result: ok. 2 passed; 0 failed; 0 ignored;\n",
                "stderr_text": "   Compiling quantick-a v0.1.0 (fixture)\n"}
        sampling.validate_sample(good, "quantick-a", touched=True)
        for changed in ({"stderr_text": "Finished test profile"}, {"exit_code": 1},
                        {"timed_out": True}, {"interrupted": "KeyboardInterrupt"}, {"stdout_text": ""},
                        {"elapsed_seconds": float("nan")}, {"elapsed_seconds": -1},
                        {"elapsed_seconds": True}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                sampling.validate_sample(good | changed, "quantick-a", touched=True)
        sampling.validate_sample(good | {"stderr_text": "Finished"}, "quantick-a", touched=False)

    def test_all_samples_are_retained_in_summary_not_best_selected(self):
        samples = [{"elapsed_seconds": value} for value in (9, 2, 8, 3, 7)]
        self.assertEqual(sampling.summary(samples), {"count": 5, "median_seconds": 7,
                                                    "min_seconds": 2, "max_seconds": 9})

class BudgetTests(unittest.TestCase):
    def test_budget_uses_fixed_baseline_and_rejects_drift_or_overrun(self):
        report = {"schema": 1, "status": "complete", "source_sha": "a" * 40,
                  "measured_utc": "2026-09-14T00:00:00+00:00", "host_class": "fixture",
                  "toolchain": "pinned", "jobs": 2, "profile_hash": "profile",
                  "protocol_hash": "protocol",
                  "restored": True, "crates": []}
        for name in ("a", "b", "c"):
            report["crates"].append({"crate": name, "package": "quantick-" + name,
                                      "source": f"crates/{name}/src/lib.rs",
                                      "samples": [{"elapsed_seconds": v, "recompiled": True,
                                                   "tests_passed": True, "exit_code": 0,
                                                   "timed_out": False, "tree_quiescent": True,
                                                   "orphaned_descendants": False} for v in (1, 2, 3, 4, 5)]})
        baseline = budgets.propose(report)
        budgets.check(report, baseline)
        changed = json.loads(json.dumps(report))
        changed["crates"][0]["samples"][4]["elapsed_seconds"] = 10000
        with self.assertRaisesRegex(ValueError, "budget"):
            budgets.check(changed, baseline)
        for field, value in (("host_class", "different"), ("toolchain", "different"),
                             ("status", "failed")):
            with self.subTest(field=field), self.assertRaises(ValueError):
                budgets.check(report | {field: value}, baseline)
        with self.assertRaises(ValueError):
            budgets.check(report, {"schema": 1, "status": "uncalibrated"})


if __name__ == "__main__":
    unittest.main()
