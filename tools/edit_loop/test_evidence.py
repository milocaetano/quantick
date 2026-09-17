"""Offline raw-evidence, process and workflow regression fixtures."""

from datetime import datetime, timedelta, timezone
from contextlib import redirect_stderr
import io
import json
import os
from pathlib import Path
import sys
import time
from types import SimpleNamespace
from unittest.mock import patch

import budgets
import evidence
import inputs
import measure
import sampling
from test_measure import Fixture


class EvidenceTests(Fixture):
    def complete(self):
        self.measurement_inputs()
        for name in ("a", "b", "c", "d"):
            self.write(f"crates/{name}/Cargo.toml", f'[package]\nname="quantick-{name}"\n')
            self.write(f"crates/{name}/src/lib.rs", "pub fn fixture() {}\n")
        sha = self.commit()
        ranking = inputs.ranked_crates(self.repo)
        report = {"schema": 1, "status": "complete", "source_sha": sha,
                  "source_tree": self.git("rev-parse", "HEAD^{tree}"),
                  "input_hashes": inputs.input_hashes(self.repo),
                  "measured_utc": datetime.now(timezone.utc).isoformat(),
                  "restored": True, "host_class": "fixture", "toolchain": "fixture",
                  "jobs": 2, "profile_hash": inputs.input_hashes(self.repo)["Cargo.toml"],
                  "ranking": ranking, "crates": []}
        report["protocol_hash"] = inputs.protocol_hash(report["input_hashes"])
        calls = []

        def fake(command, repo, env, output, timeout):
            calls.append(command)
            result = {"command": command, "elapsed_seconds": len(calls), "exit_code": 0,
                      "timed_out": False, "interrupted": None,
                      "tree_quiescent": True, "orphaned_descendants": False,
                      "started_utc": report["measured_utc"], "finished_utc": report["measured_utc"]}
            inputs.write_json(output / "process.json", result)
            stdout = "test result: ok. 1 passed; 0 failed; 0 ignored;\n"
            stderr = f"   Compiling {command[-1]} v0.1.0 (fixture)\n"
            (output / "stdout.log").write_text(stdout, encoding="utf-8")
            (output / "stderr.log").write_text(stderr, encoding="utf-8")
            return result | {"stdout_text": stdout, "stderr_text": stderr}

        with patch("inputs.command_environment", return_value=({}, [])):
            for row in ranking[:3]:
                report["crates"].append(measure.series(
                    row, self.repo, sha, self.root / (row["crate"] + "-target"),
                    self.output / row["crate"], process=fake, sleep=lambda _: None, observe=lambda: []))
        self.assertEqual(len(calls), 3 * (2 + inputs.SAMPLES))
        evidence.publish(self.output, report)
        return report

    def test_full_raw_series_roundtrip_and_fixed_proposal(self):
        report = self.complete()
        loaded = evidence.load(self.output / "report.json", self.repo)
        self.assertEqual(loaded, report)
        budgets.check(loaded, budgets.propose(loaded))

    def test_corrupt_raw_output_is_rejected_without_resigning(self):
        self.complete()
        (self.output / "a/touch-1/stdout.log").write_text("corrupt", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "hash"):
            evidence.load(self.output / "report.json", self.repo)

    def test_claims_cannot_replace_compile_or_restore_proof(self):
        original = self.complete()
        for mutation in ("claim", "restore", "rank", "time", "command"):
            report = json.loads(json.dumps(original))
            if mutation == "claim":
                report["crates"][0]["samples"][0]["elapsed_seconds"] = 0
            elif mutation == "restore":
                report["crates"][0]["restored"] = False
            elif mutation == "rank":
                report["crates"].reverse()
            elif mutation == "time":
                report["crates"][0]["samples"][0]["touched_mtime_ns"] = 0
            else:
                report["crates"][0]["control"]["command"] = ["cargo", "test", "--lib"]
            evidence.publish(self.output, report)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                evidence.load(self.output / "report.json", self.repo)

    def test_freshness_rejects_stale_future_and_local_naive_timestamps(self):
        now = datetime.now(timezone.utc)
        for value in (now - timedelta(days=30), now + timedelta(days=1), now.replace(tzinfo=None)):
            with self.subTest(value=value), self.assertRaises(ValueError):
                evidence.fresh({"measured_utc": value.isoformat()}, now)

    def test_self_consistent_wrong_ranking_and_omitted_inputs_are_recomputed(self):
        original = self.complete()
        for mutation in ("sorted_wrong_ranking", "wrong_top_three", "empty_inputs", "missing_input", "protocol"):
            report = json.loads(json.dumps(original))
            if mutation == "sorted_wrong_ranking":
                report["ranking"][0]["production_lines"] = 100000
                report["crates"][0]["production_lines"] = 100000
            elif mutation == "wrong_top_three":
                replacement = report["crates"][0] | report["ranking"][3]
                report["ranking"] = report["ranking"][1:]
                report["crates"] = report["crates"][1:] + [replacement]
            elif mutation == "empty_inputs":
                report["input_hashes"] = {}
            elif mutation == "missing_input":
                del report["input_hashes"]["Cargo.lock"]
            else:
                report["protocol_hash"] = "different"
            evidence.publish(self.output, report)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                evidence.load(self.output / "report.json", self.repo)

    def test_manifest_escape_duplicate_and_missing_files_are_rejected(self):
        self.complete()
        path = self.output / "manifest.json"
        original = evidence.read_json(path)
        for rows in (original + [original[0]], original[1:],
                     original + [{"path": "../escape", "bytes": 0, "sha256": "fake"}]):
            inputs.write_json(path, rows)
            with self.subTest(rows=rows[-1:]), self.assertRaises((ValueError, OSError)):
                evidence.load(self.output / "report.json", self.repo)

    def test_manifest_refresh_cannot_supply_missing_or_invalid_owned_tree_proof(self):
        original = self.complete()
        raw_path = self.output / "a/touch-1/process.json"
        original_raw = evidence.read_json(raw_path)
        cases = [("tree_quiescent", value) for value in ("missing", None, False)]
        cases += [("orphaned_descendants", value) for value in ("missing", None, True)]
        for key, value in cases:
            report = json.loads(json.dumps(original))
            raw = dict(original_raw)
            sample = report["crates"][0]["samples"][0]
            if value == "missing":
                del raw[key]
                del sample[key]
            else:
                raw[key] = value
                sample[key] = value
            inputs.write_json(raw_path, raw)
            evidence.publish(self.output, report)
            with self.subTest(key=key, value=value), self.assertRaisesRegex(ValueError, "owned-tree quiescence"):
                evidence.load(self.output / "report.json", self.repo)


class ProcessTests(Fixture):
    def test_after_sample_unknown_compiler_refuses_metadata_restoration(self):
        path = self.write("crates/a/src/lib.rs", "pub fn fixture() {}\n")
        sha = self.commit()
        before = path.stat().st_mtime_ns
        row = {"crate": "a", "package": "quantick-a", "source": "crates/a/src/lib.rs"}
        observations = iter([[], [], [], [], [], [{"pid": "unknown-owner"}]])

        def fake(command, repo, env, output, timeout):
            return {"command": command, "elapsed_seconds": 1, "exit_code": 0,
                    "tree_quiescent": True, "orphaned_descendants": False,
                    "timed_out": False, "stdout_text": "test result: ok. 1 passed; 0 failed;",
                    "stderr_text": "Compiling quantick-a v1.0"}

        with patch("inputs.command_environment", return_value=({}, [])):
            with self.assertRaisesRegex(ValueError, "termination is unproven"):
                measure.series(row, self.repo, sha, self.root / "target", self.output / "a",
                               process=fake, sleep=lambda _: None, observe=lambda: next(observations))
        self.assertGreater(path.stat().st_mtime_ns, before)
        self.assertEqual(evidence.read_json(self.output / "a/recovery.json")["state"], "restore_refused")

    def test_failed_series_keeps_completed_stages_and_restores_source(self):
        path = self.write("crates/a/src/lib.rs", "pub fn fixture() {}\n")
        sha = self.commit()
        before = path.stat().st_mtime_ns
        row = {"crate": "a", "package": "quantick-a", "source": "crates/a/src/lib.rs"}
        calls = []

        def fake(command, repo, env, output, timeout):
            calls.append(command)
            return {"command": command, "elapsed_seconds": 1, "exit_code": 1 if len(calls) == 3 else 0,
                    "tree_quiescent": True, "orphaned_descendants": False,
                    "timed_out": False, "stdout_text": "test result: ok. 1 passed; 0 failed;",
                    "stderr_text": "Compiling quantick-a v1.0.0"}

        with patch("inputs.command_environment", return_value=({}, [])):
            with self.assertRaisesRegex(ValueError, "Cargo failed"):
                measure.series(row, self.repo, sha, self.root / "target", self.output / "a",
                               process=fake, sleep=lambda _: None, observe=lambda: [])
        retained = evidence.read_json(self.output / "a/series.json")
        self.assertIn("warmup", retained)
        self.assertIn("control", retained)
        self.assertEqual(retained["samples"], [])
        self.assertIn("error", retained)
        self.assertEqual(evidence.read_json(self.output / "a/recovery.json")["state"], "restored")
        self.assertTrue((self.output / "a/touch-1/validation.json").is_file())
        self.assertEqual(path.stat().st_mtime_ns, before)

    def test_owned_nonzero_process_retains_raw_output_and_exit(self):
        command = [sys.executable, "-c", "import sys; print('partial', flush=True); sys.exit(7)"]
        result = sampling.run_process(command, self.repo, dict(os.environ), self.output, 10)
        self.assertEqual(result["exit_code"], 7)
        self.assertIn("partial", result["stdout_text"])
        self.assertEqual(evidence.read_json(self.output / "process.json")["command"], command)

    def test_owned_timeout_retains_partial_output_and_does_not_pass(self):
        command = [sys.executable, "-c", "import time; print('started', flush=True); time.sleep(30)"]
        if os.name == "nt":
            result = sampling.run_process(command, self.repo, dict(os.environ), self.output, 1)
            self.assertTrue(result["tree_quiescent"])
        else:
            with self.assertRaises(sampling.ProcessStillRunning):
                sampling.run_process(command, self.repo, dict(os.environ), self.output, 1)
            result = evidence.read_json(self.output / "process.json")
            self.assertFalse(result["tree_quiescent"])
            result["stdout_text"] = (self.output / "stdout.log").read_text(encoding="utf-8")
        self.assertTrue(result["timed_out"])
        self.assertIn("started", result["stdout_text"])
        with self.assertRaises(ValueError):
            sampling.validate_sample(result, "fixture", True)

    def test_parent_exit_does_not_award_success_to_surviving_owned_child(self):
        ready, survived = self.root / "child-ready", self.root / "child-survived"
        child = ("from pathlib import Path; import time; "
                 f"Path({str(ready)!r}).write_text('ready'); time.sleep(2); "
                 f"Path({str(survived)!r}).write_text('still running')")
        parent = ("import subprocess,sys,time; from pathlib import Path; "
                  f"subprocess.Popen([sys.executable,'-c',{child!r}]); "
                  f"ready=Path({str(ready)!r}); "
                  "\nwhile not ready.exists(): time.sleep(0.01)\n")
        command = [sys.executable, "-c", parent]
        if os.name == "nt":
            result = sampling.run_process(command, self.repo, dict(os.environ), self.output, 10)
            self.assertEqual(result["exit_code"], 0)
            self.assertTrue(result["tree_quiescent"])
            self.assertTrue(result["orphaned_descendants"])
            observation = result["ownership_observations"][0]
            self.assertGreater(observation["accounting_active"], 1)
            children = [row for row in observation["processes"]
                        if row["pid"] != result["supervisor_pid"]]
            self.assertTrue(children, "the live fixture child must remain visible in diagnostics")
            self.assertTrue(all(row["in_owned_job"] for row in children))
            self.assertTrue(all(row["creation_filetime"] > 0 for row in children))
            with self.assertRaises(ValueError):
                sampling.validate_sample(result, "fixture", True)
        else:
            with self.assertRaises(sampling.ProcessStillRunning):
                sampling.run_process(command, self.repo, dict(os.environ), self.output, 10)
            self.assertFalse(evidence.read_json(self.output / "process.json")["tree_quiescent"])
        self.assertTrue(ready.exists())
        time.sleep(2.2)
        self.assertFalse(survived.exists(), "the owned child survived its parent's exit and cleanup")

    def test_unproven_process_termination_retains_touched_metadata_for_recovery(self):
        path = self.write("crates/a/src/lib.rs", "pub fn fixture() {}\n")
        sha = self.commit()
        before = path.stat().st_mtime_ns
        with self.assertRaisesRegex(ValueError, "termination"):
            with sampling.SourceTouch(self.repo, path, sha, self.output) as source:
                source.touch(before + 3_000_000_000)
                raise sampling.ProcessStillRunning("injected")
        self.assertGreater(path.stat().st_mtime_ns, before)
        self.assertEqual(evidence.read_json(self.output / "recovery.json")["state"], "restore_refused")


class WorkflowTests(Fixture):
    def test_windows_preserves_full_ordered_loop_and_named_authority_check(self):
        source = (inputs.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        windows = source.split("\n  windows:", 1)[1]
        commands = ["cargo fmt --all -- --check", "cargo clippy --workspace --all-targets",
                    "cargo build --workspace", "cargo test --workspace"]
        positions = [windows.index("run: " + command) for command in commands]
        self.assertEqual(positions, sorted(positions))
        self.assertIn("cargo test -p quantick-control-local -- --nocapture", windows)
        self.assertNotIn("continue-on-error", windows)

    def test_schedule_and_pr_share_failing_checker_and_always_upload(self):
        source = (inputs.ROOT / ".github/workflows/edit-loop.yml").read_text(encoding="utf-8")
        for required in ("schedule:", "pull_request:", "workflow_dispatch:", "if: always()",
                         "if-no-files-found: error", "--budgets tools/edit_loop/budgets.json",
                         "cancel-in-progress: false", "persist-credentials: false"):
            self.assertIn(required, source)
        self.assertEqual(source.count("measure.py run"), 1)
        self.assertNotIn("continue-on-error", source)


class ConfigurationTests(Fixture):
    def test_global_jobs_are_declared_and_overridden_but_other_config_is_refused(self):
        cargo_home = self.root / "cargo-home"
        cargo_home.mkdir()
        config = cargo_home / "config.toml"
        config.write_text("[build]\njobs=99\n", encoding="utf-8")
        identity = inputs.cargo_configuration(self.repo, {"CARGO_HOME": str(cargo_home)})
        self.assertEqual(identity["configs"][0]["normalized_keys"], ["build.jobs"])
        self.assertEqual(identity["configs"][0]["sha256"], inputs.digest(config.read_bytes()))
        config.write_text('[build]\nrustc-wrapper="unreported-cache"\n', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "configuration"):
            inputs.cargo_configuration(self.repo, {"CARGO_HOME": str(cargo_home)})

    def test_more_compiler_and_profile_overrides_are_rejected(self):
        for key in ("RUSTC", "RUSTDOC", "CARGO_ENCODED_RUSTDOCFLAGS", "RUSTC_BOOTSTRAP",
                    "CARGO_PROFILE_TEST_OPT_LEVEL", "CARGO_NET_OFFLINE"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                inputs.command_environment(self.repo, self.root / "target", 2, {key: "1"})


class RunnerTests(Fixture):
    def test_real_orchestrator_rejects_a_different_source_checkout(self):
        self.measurement_inputs()
        sha = self.commit()
        other = self.root / "other-checkout"
        other.mkdir()
        args = SimpleNamespace(repo=str(self.repo), sha=sha, worktree=str(self.root / "bench"),
                               target=str(self.root / "targets"), output=str(self.root / "run-output"),
                               budgets=str(self.repo / "tools/edit_loop/budgets.json"))
        error_output = io.StringIO()
        with patch("inputs.ROOT", other.resolve()), redirect_stderr(error_output):
            self.assertEqual(measure.run(args), 1)
        self.assertIn("exact named source checkout", error_output.getvalue())
        self.assertFalse(Path(args.worktree).exists())
        self.assertFalse(Path(args.target).exists())

    def test_uncalibrated_real_orchestrator_fails_with_complete_fake_raw_series(self):
        self.measurement_inputs()
        for name in ("a", "b", "c"):
            self.write(f"crates/{name}/Cargo.toml", f'[package]\nname="quantick-{name}"\n')
            self.write(f"crates/{name}/src/lib.rs", "pub fn fixture() {}\n")
        budget = self.write("tools/edit_loop/budgets.json", '{"schema":1,"status":"uncalibrated"}')
        self.write("Cargo.toml", "[workspace]\nmembers=[]\n")
        sha = self.commit()
        args = SimpleNamespace(repo=str(self.repo), sha=sha, worktree=str(self.root / "bench"),
                               target=str(self.root / "targets"), output=str(self.root / "run-output"),
                               budgets=str(budget))
        actual_series = measure.series

        def fake_process(command, repo, env, output, timeout):
            result = {"command": command, "elapsed_seconds": 1, "exit_code": 0,
                      "tree_quiescent": True, "orphaned_descendants": False,
                      "timed_out": False, "interrupted": None}
            inputs.write_json(output / "process.json", result)
            stdout, stderr = "test result: ok. 1 passed; 0 failed;", f"Compiling {command[-1]} v1.0"
            (output / "stdout.log").write_text(stdout, encoding="utf-8")
            (output / "stderr.log").write_text(stderr, encoding="utf-8")
            return result | {"stdout_text": stdout, "stderr_text": stderr}

        def fake_series(*args):
            return actual_series(*args, process=fake_process, sleep=lambda _: None, observe=lambda: [])

        # Production ROOT is canonical (__file__.resolve()). Windows TEMP can
        # use an 8.3 ancestor spelling; the injected root must obey that contract.
        with (patch("inputs.ROOT", self.repo.resolve()), patch("inputs.command_environment", return_value=({}, [])),
              patch("inputs.cargo_configuration", return_value={}),
              patch("inputs.host_identity", return_value={"class": "fixture"}),
              patch("measure.version", return_value="fixture"), patch("measure.series", fake_series),
              patch("measure.guard_crosscheck", return_value={"counts": {
                  "crate.lines.a": 1, "crate.lines.b": 1, "crate.lines.c": 1}})):
            error_output = io.StringIO()
            with redirect_stderr(error_output):
                self.assertEqual(measure.run(args), 1)
            self.assertIn("budgets are uncalibrated", error_output.getvalue())
        report_path = Path(args.output) / "report.json"
        report = evidence.load(report_path, self.repo)
        self.assertEqual(report["status"], "complete")
        self.assertEqual(report["budget_status"], "failed")
        self.assertIn("uncalibrated", report["error"])
        self.assertEqual([len(row["samples"]) for row in report["crates"]], [5, 5, 5])
        self.assertEqual(budgets.propose(report)["status"], "calibrated")
