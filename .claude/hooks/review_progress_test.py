#!/usr/bin/env python3
"""Offline recovery and failure tests; never invokes the real GitHub client."""

import copy
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "review_progress", Path(__file__).with_name("review_progress.py")
)
progress = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(progress)


def initial():
    return {
        "schema": 1, "mission": "issue-123", "revision": 1,
        "head": "a" * 40, "stage": "review", "batches": 0, "stalled_batches": 0,
        "limits": {"batches": 3, "attempts": 3}, "authorization": None,
        "findings": {"F1": {"attempts": 0, "disposition": "open", "evidence": []}},
        "evidence": ["Reviewed prior PR history: no repair attempts yet."],
    }


class Remote:
    def __init__(self):
        self.comments = []
        self.posts = 0
        self.lose_post_response = False

    def gh(self, *args, payload=None):
        if args[:2] == ("repo", "view"):
            return {"nameWithOwner": "owner/repo"}
        if args[:2] == ("pr", "view"):
            return {"number": 123}
        if "POST" in args:
            self.posts += 1
            self.comments.append(payload.copy())
            if self.lose_post_response:
                raise ValueError("POST response lost")
            return {"id": self.posts}
        # Multiple pages, including normal comments that must not count.
        return [[{"body": "Tried again; ordinary discussion is not a counter."}],
                copy.deepcopy(self.comments)]

    def seed(self, record):
        self.comments.append({"body": progress.MARKER + json.dumps(record)})


class ProgressTests(unittest.TestCase):
    def setUp(self):
        self.remote = Remote()
        self.mock = patch.object(progress, "gh", side_effect=self.remote.gh)
        self.mock.start()
        self.addCleanup(self.mock.stop)

    def test_restart_reads_persisted_history_and_identical_retry_does_not_post(self):
        record = initial()
        progress.record_checkpoint("comments", record)
        self.assertEqual(progress.history("comments"), record)
        progress.record_checkpoint("comments", copy.deepcopy(record))
        self.assertEqual(self.remote.posts, 1)

    def test_lost_post_response_reconciles_without_duplicate(self):
        self.remote.lose_post_response = True
        with self.assertRaises(ValueError):
            progress.record_checkpoint("comments", initial())
        self.remote.lose_post_response = False
        self.assertEqual(progress.record_checkpoint("comments", initial()), initial())
        self.assertEqual(self.remote.posts, 1)

    def test_duplicate_identical_comments_are_idempotent(self):
        self.remote.seed(initial())
        self.remote.seed(initial())
        self.assertEqual(progress.history("comments"), initial())

    def test_conflicting_duplicate_revision_fails_closed(self):
        self.remote.seed(initial())
        other = initial()
        other["stage"] = "repair"
        self.remote.seed(other)
        with self.assertRaisesRegex(ValueError, "Conflicting"):
            progress.history("comments")

    def test_missing_history_is_unknown_and_cannot_authorize_repair(self):
        self.assertIsNone(progress.history("comments"))
        with self.assertRaisesRegex(ValueError, "No durable history"):
            progress.check_repair(None, ["F1"])

    def test_first_record_requires_explicit_history_evidence(self):
        record = initial()
        record["evidence"] = []
        with self.assertRaisesRegex(ValueError, "history"):
            progress.record_checkpoint("comments", record)
        self.assertEqual(self.remote.posts, 0)

    def test_null_record_is_not_an_idempotent_empty_history(self):
        with self.assertRaises(ValueError):
            progress.record_checkpoint("comments", None)
        self.assertEqual(self.remote.posts, 0)

    def test_stricter_limits_are_preserved(self):
        record = initial()
        record["limits"] = {"batches": 1, "attempts": 1}
        progress.validate(record)
        record["batches"] = 1
        with self.assertRaisesRegex(ValueError, "batch limit"):
            progress.check_repair(record, ["F1"])

    def test_two_batches_without_closure_stop_repairs_across_restart(self):
        record = initial()
        progress.record_checkpoint("comments", record)
        for count in (1, 2):
            progress.check_repair(progress.history("comments"), ["F1"])
            record = copy.deepcopy(record)
            record.update(revision=count + 1, batches=count, stalled_batches=count)
            record["findings"]["F1"]["attempts"] = count
            progress.record_checkpoint("comments", record)
        with self.assertRaisesRegex(ValueError, "Two batches"):
            progress.check_repair(progress.history("comments"), ["F1"])
        record = copy.deepcopy(record)
        record.update(revision=4, stage="awaiting_human")
        progress.record_checkpoint("comments", record)

    def test_stall_reset_requires_proven_closure_not_metadata(self):
        old = initial()
        old.update(batches=2, stalled_batches=2)
        record = copy.deepcopy(old)
        record.update(revision=2, stalled_batches=0, stage="review")
        with self.assertRaisesRegex(ValueError, "proven closure"):
            progress.validate(record, old)
        record["findings"]["F1"]["disposition"] = "fixed"
        with self.assertRaisesRegex(ValueError, "needs evidence"):
            progress.validate(record, old)
        record["findings"]["F1"]["evidence"] = ["Commit abc: targeted regression passed."]
        progress.validate(record, old)

    def test_new_batch_must_reserve_stall_count(self):
        old = initial()
        record = copy.deepcopy(old)
        record.update(revision=2, batches=1)
        with self.assertRaisesRegex(ValueError, "Reserve each batch"):
            progress.validate(record, old)

    def test_gh_subprocess_error_and_malformed_json_are_failures(self):
        # Exercise the actual transport function, not the Remote test double.
        self.mock.stop()
        with patch.object(progress.subprocess, "run") as run:
            run.return_value.returncode = 1
            run.return_value.stdout = ""
            with self.assertRaisesRegex(ValueError, "uncertain"):
                progress.gh("api", "comments")
            run.return_value.returncode = 0
            run.return_value.stdout = "not JSON"
            with self.assertRaises(ValueError):
                progress.gh("api", "comments")

    def test_adoption_preserves_observed_attempts_and_exhaustion(self):
        record = initial()
        record["batches"] = 3
        record["findings"]["F1"]["attempts"] = 3
        record["evidence"] = ["Prior batch comments 1, 2, 3 and their repair commits."]
        progress.record_checkpoint("comments", record)
        with self.assertRaisesRegex(ValueError, "batch limit"):
            progress.check_repair(progress.history("comments"), ["F1"])
        record["revision"] += 1
        record["stage"] = "awaiting_human"
        progress.record_checkpoint("comments", record)

    def test_counters_and_stable_ids_cannot_reset(self):
        old = initial()
        old["batches"] = 2
        old["findings"]["F1"]["attempts"] = 2
        self.remote.seed(old)
        for change in ("batches", "attempts", "id", "mission", "revision"):
            with self.subTest(change=change):
                record = copy.deepcopy(old)
                record["revision"] = 2
                if change == "attempts":
                    record["findings"]["F1"]["attempts"] = 0
                elif change == "id":
                    record["findings"]["F2"] = record["findings"].pop("F1")
                else:
                    record[change] = "other" if change == "mission" else 0
                with self.assertRaises(ValueError):
                    progress.record_checkpoint("comments", record)
        self.assertEqual(self.remote.posts, 0)

    def test_finite_limits_and_individual_attempt_gate(self):
        record = initial()
        progress.check_repair(record, ["F1"])
        record["findings"]["F1"]["attempts"] = 3
        with self.assertRaisesRegex(ValueError, "attempt limit"):
            progress.check_repair(record, ["F1"])
        with self.assertRaisesRegex(ValueError, "Record the finding"):
            progress.check_repair(record, ["unknown"])
        record["limits"]["batches"] = 4
        with self.assertRaisesRegex(ValueError, "authorization"):
            progress.validate(record)
        record["authorization"] = "Verified user decision: comment-4"
        progress.validate(record)
        next_record = copy.deepcopy(record)
        next_record["revision"] = 2
        next_record["limits"]["batches"] = 5
        with self.assertRaisesRegex(ValueError, "new authorization"):
            progress.validate(next_record, record)

    def test_missing_revision_and_unknown_schema_fail_closed(self):
        for field, value in (("revision", 2), ("schema", 2)):
            with self.subTest(field=field):
                record = initial()
                record[field] = value
                self.remote.comments = []
                self.remote.seed(record)
                with self.assertRaises(ValueError):
                    progress.history("comments")

    def test_failed_read_never_becomes_zero_or_triggers_write(self):
        with patch.object(progress, "gh", side_effect=ValueError("API failed")):
            with self.assertRaises(ValueError):
                progress.record_checkpoint("comments", initial())
        self.assertEqual(self.remote.posts, 0)

    def test_unconfirmed_post_stops_before_repair(self):
        responses = [[[]], {"id": 1}, [[]]]
        with patch.object(progress, "gh", side_effect=responses):
            with self.assertRaisesRegex(ValueError, "not confirmed"):
                progress.record_checkpoint("comments", initial())

    def test_show_cli_returns_json_for_fresh_process(self):
        self.remote.seed(initial())
        with patch("sys.stdout", new_callable=io.StringIO) as output:
            progress.main(["show", "123"])
        self.assertEqual(json.loads(output.getvalue()), initial())


if __name__ == "__main__":
    unittest.main()
