"""Owned-job observations cannot substitute for accounting or cleanup proof."""

from datetime import datetime
import os
import sys
from unittest.mock import Mock, patch

import evidence
import process_owner
import sampling
from test_measure import Fixture


class DiagnosticTests(Fixture):
    def test_missing_diagnostic_rows_cannot_forgive_accounted_descendants(self):
        job = Mock()
        job.active.side_effect = [2, 0]
        job.snapshot.return_value = {"listed": 0, "processes": []}
        owner = process_owner.OwnedTree.__new__(process_owner.OwnedTree)
        owner.job, owner.process, owner.observations = job, Mock(), []
        self.assertTrue(owner.stop(), "diagnostic rows must not override the active-process count")
        self.assertEqual(owner.observations[0]["accounting_active"], 2)
        job.terminate.assert_called_once()
        owner.process.wait.assert_called_once()

    def test_diagnostic_failure_never_awards_quiescence_or_safe_restore(self):
        if os.name != "nt":
            # Exercise the same supervisor decision without requiring Win32;
            # Linux's real session cleanup remains covered by its process suite.
            real_stop = process_owner.OwnedTree.stop

            def cleanup_then_fail(owner):
                real_stop(owner)
                raise OSError("injected diagnostic failure")

            context = patch.object(process_owner.OwnedTree, "stop", cleanup_then_fail)
        else:
            context = patch.object(process_owner.WindowsJob, "snapshot",
                                   side_effect=OSError("injected diagnostic failure"))
        command = [sys.executable, "-c", "print('finished before diagnostics', flush=True)"]
        with context, self.assertRaises(sampling.ProcessStillRunning):
            sampling.run_process(command, self.repo, dict(os.environ), self.output, 10)
        result = evidence.read_json(self.output / "process.json")
        self.assertEqual(result["exit_code"], 0)
        self.assertFalse(result["tree_quiescent"])
        self.assertIn("diagnostic failure", result["cleanup_error"])
        with self.assertRaises(ValueError):
            sampling.validate_sample(result, "fixture", False)

    def test_lifetime_diagnostics_follow_cargo_stopwatch_and_preserve_worker_result(self):
        command = [sys.executable, "-c", "print('fixture result', flush=True)"]
        result = sampling.run_process(command, self.repo, dict(os.environ), self.output, 10)
        self.assertEqual(result["exit_code"], 0)
        self.assertTrue(result["tree_quiescent"])
        self.assertFalse(result["orphaned_descendants"])
        finished = datetime.fromisoformat(result["finished_utc"])
        self.assertGreaterEqual(datetime.fromisoformat(result["supervision_finished_utc"]), finished)
        if os.name == "nt":
            before, after = result["ownership_observations"]
            self.assertGreaterEqual(datetime.fromisoformat(before["observed_utc"]), finished)
            self.assertEqual(before["accounting_active"], 1)
            self.assertEqual(after["accounting_active"], 0)
            supervisor = next(row for row in before["processes"]
                              if row["pid"] == result["supervisor_pid"])
            self.assertTrue(supervisor["in_owned_job"])
            self.assertGreater(supervisor["creation_filetime"], 0)
            self.assertEqual(supervisor["exit_filetime"], 0)
            self.assertEqual(supervisor["wait_status"], 258)
            self.assertTrue(supervisor["image"].lower().endswith("python.exe"))
            self.assertEqual(after["processes"], [])
        else:
            self.assertEqual(result["ownership_observations"], [])
        raw = evidence.read_json(self.output / "process.json")
        self.assertEqual(raw["elapsed_seconds"], result["elapsed_seconds"])
        self.assertEqual(raw["finished_utc"], result["finished_utc"])
