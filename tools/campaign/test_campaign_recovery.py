"""Offline recovery across fresh processes; no live replacement claim.

Only remote I/O and the clock are faked. The maintained helper owns framing,
partition reconstruction, journal validation, counter folding and publication.
The driver, like a real caller, reconciles business readback and audits tasks;
the helper does not implement task scheduling or business authorization.
"""
import copy
import datetime
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


HERE = Path(__file__).resolve().parent
with patch("subprocess.run", side_effect=AssertionError("Process during import")):
    SPEC = importlib.util.spec_from_file_location(
        "maintained_campaign", HERE / "architecture-a-coordinator-v2.py"
    )
    c = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(c)

TIME = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
WRITER = "offline/same-logical-writer"
CAMPAIGN = "milocaetano/quantick#330"
PARENT = "https://github.com/milocaetano/quantick/issues/330"
MERGE = "b" * 40
RECOVERY_KEY = "publish-recovery-evidence"
AUDIT_KEY = "publish-dependency-audit"


def initial_state():
    """Literal synthetic history, independent of the helper's projection code."""
    return {
        "schema": 1, "campaign": CAMPAIGN, "sequence": 1,
        "publication_key": "fixture/initial", "previous": None,
        "created_at": TIME.isoformat(), "writer": WRITER, "phase": "running",
        "base_sha": "a" * 40, "integration": {"current_sha": MERGE},
        "authorization": {
            "source": "https://example.test/retained-user-grant",
            "allowed": ["evidence_publication"],
            "excluded": ["main_merge", "main_push", "auto_merge",
                         "merge_queue", "protection_changes"],
        },
        "lease": {"owner": WRITER, "expires_at": "2026-01-01T00:15:00+00:00"},
        "project": {"id": "fixture-project", "pending": []},
        "tasks": [
            {"key": "Q7", "state": "done", "review_repair_batches": None,
             "attempts": {"repair": 11, "operation": 0}},
            {"key": "Q10", "state": "done", "review_repair_batches": 4,
             "integration": {"merge_sha": MERGE, "ci": "https://example.test/ci"}},
            {"key": "Q8", "state": "blocked", "performance": {
                "repair_attempts": 2, "experiment_attempts": 2,
                "results": [{"experiment": 1, "passed": False},
                            {"experiment": 2, "passed": False}]}},
            {"key": "EXHAUSTED", "state": "blocked",
             "attempts": {"operation": 3, "repair": 3},
             "failures": [{"signature": "unchanged-failure", "attempts": 3}]},
            {"key": "AUDIT", "state": "ready", "depends_on": [
                {"task": "Q10", "condition": "integrated_campaign_with_green_ci",
                 "evidence": {"merge_sha": MERGE, "ci": "https://example.test/ci"}}]},
            {"key": "MAIN", "state": "awaiting_human", "class": "human_decision"},
        ],
        "inflight": None,
        "decisions": [{"id": "D1", "source": "https://example.test/retained-user-grant"}],
        "metrics": {"assessment": "unchanged; offline fixture is not score evidence"},
        "retry_policy": {"operation_attempts": 3, "repair_attempts": 3},
        "next_action": {"task": "AUDIT"}, "stop_reason": None,
        "operation_history": [
            {"key": "legacy-unknown", "attempts": None, "last_status": None,
             "history": "unknown", "source": "https://example.test/legacy"},
            {"key": "exhausted-repair", "attempts": 3, "last_status": "failed",
             "journal_urls": ["https://example.test/old-journal"],
             "result_evidence": ["https://example.test/failure"]},
        ],
        # Force multiple real codec parts, without historical files or a network.
        "retained_archive": "retained evidence; " * 1800,
    }


class FileRemote:
    """A fake external store; sequential processes share only this remote file."""

    def __init__(self, path):
        self.path = Path(path)

    def load(self):
        return json.loads(self.path.read_text(encoding="utf-8"))

    def save(self, value):
        self.path.write_text(json.dumps(value), encoding="utf-8")

    def append(self, text):
        value = self.load()
        url = PARENT + "#issuecomment-" + str(len(value["comments"]) + 1)
        value["comments"].append({"url": url, "body": text})
        self.save(value)
        return url

    def read(self, url):
        for row in self.load()["comments"]:
            if row["url"] == url:
                return row
        raise c.ReconcileError("Missing fake remote comment")

    def comments_since(self, url):
        rows = self.load()["comments"]
        index = next(i for i, row in enumerate(rows) if row["url"] == url)
        return rows[index:]

    def latest_checkpoint(self):
        return next(row["url"] for row in reversed(self.load()["comments"])
                    if row["body"].startswith(c.CHECKPOINT))

    def publish_evidence(self, key, target, payload, *, lose_response=False):
        value = self.load()
        # Deliberately no deduplication: a repeated business call is observable.
        effect = {"key": key, "target": target, "payload": payload,
                  "url": "https://example.test/effects/" + str(len(value["effects"]) + 1)}
        value["effects"].append(effect)
        self.save(value)
        if lose_response:
            raise TimeoutError("Fake remote accepted effect; response was lost")
        return effect

    def effect(self, key):
        matches = [effect for effect in self.load()["effects"] if effect["key"] == key]
        if len(matches) != 1:
            raise c.ReconcileError("Business effect missing or duplicated")
        return matches[0]


def seed_remote(path):
    remote = FileRemote(path)
    remote.save({"comments": [], "effects": [], "now": TIME.isoformat(),
                 "refs": {"campaign_sha": MERGE, "reachable_merges": [MERGE],
                          "ci": {MERGE: "success"}}})
    encoded = c.encode_checkpoint(initial_state())
    if encoded["complete"] is not None:
        raise AssertionError("Fixture must exercise partition reconstruction")
    manifest = encoded["manifest"]
    for text in encoded["parts"]:
        part = c.parse(text, c.PART)
        url = remote.append(text)
        manifest["parts"].append({"url": url, **{
            key: part[key] for key in ("index", "bytes", "sha256")}})
    remote.append(c.body(c.CHECKPOINT, manifest))
    return remote


def dependency_audit(state, refs):
    """Useful fixture-side work from reconstructed state and current fake refs."""
    tasks = {task["key"]: task for task in state["tasks"]}
    satisfied = []
    for dependency in tasks["AUDIT"]["depends_on"]:
        upstream = tasks[dependency["task"]]
        merge = dependency["evidence"]["merge_sha"]
        if (upstream["state"] != "done"
                or upstream["integration"]["merge_sha"] != merge
                or merge not in refs["reachable_merges"]
                or refs["ci"].get(merge) != "success"):
            raise c.ReconcileError("Audit dependency lacks current merge/CI proof")
        satisfied.append({"task": upstream["key"], "merge": merge,
                          "ci": dependency["evidence"]["ci"]})
    return {
        "campaign_sha": refs["campaign_sha"], "satisfied": satisfied,
        "blocked": [task["key"] for task in state["tasks"] if task["state"] == "blocked"],
        "pending_human": [task["key"] for task in state["tasks"]
                          if task["state"] == "awaiting_human"],
        "evidence_index": [row["source"] for row in state["operation_history"]
                           if "source" in row],
    }


def run_phase(phase, remote_path, cache_path):
    cache = Path(cache_path)
    if list(cache.iterdir()):
        raise AssertionError("Each process must start with an empty independent cache")
    remote = FileRemote(remote_path)
    c.ROOT = cache
    c.WRITER = WRITER
    publisher = c.activate({
        "adoption_checkpoint": "https://example.test/offline-adoption-fixture",
        "reviewed_sync_merge": MERGE, "source_main_sha": "a" * 40,
        "format": "quantick-snapshot-parts:v1",
    }, transport=remote)
    publisher.clock = lambda: datetime.datetime.fromisoformat(remote.load()["now"])
    url = remote.latest_checkpoint()
    state, _, operations = publisher.boundary(url, CAMPAIGN)
    state["previous"] = url
    receipt = {"pid": os.getpid(), "cache_was_empty": True, "initial_checkpoint": url}
    if phase == "interrupt":
        pending = c.intent(state, RECOVERY_KEY, "issue-evidence",
                           {"publish": "recovery input"},
                           {"state": "recorded", "campaign_sha": MERGE})
        try:
            remote.publish_evidence(RECOVERY_KEY, "issue-evidence",
                                    pending["record"]["expected_readback"], lose_response=True)
        except TimeoutError:
            # End this process before the terminal journal or complete checkpoint.
            print(json.dumps(receipt | {"response_lost": True}))
            return 23
        raise AssertionError("Expected a real lost-response boundary in the fake I/O")
    if phase != "resume":
        raise AssertionError("Unknown process phase")
    observed = operations[(RECOVERY_KEY, 1)]
    pending = {"url": observed["urls"][0], "record": observed["record"]}
    effect = remote.effect(RECOVERY_KEY)
    # The actual caller must inspect external business state before c.result.
    if (effect["target"] != pending["record"]["target"]
            or effect["payload"] != pending["record"]["expected_readback"]):
        raise c.ReconcileError("Observed business readback differs from pending intent")
    c.result(pending, "succeeded", [effect["url"]])
    recovered_url = c.checkpoint(state)
    recovered = c.read_checkpoint(recovered_url, remote)
    audit = dependency_audit(recovered, remote.load()["refs"])
    intent = c.intent(state, AUDIT_KEY, "audit-evidence", {"publish": "dependency audit"}, audit)
    effect = remote.publish_evidence(AUDIT_KEY, "audit-evidence", audit)
    c.result(intent, "succeeded", [effect["url"]])
    task = next(task for task in state["tasks"] if task["key"] == "AUDIT")
    task.update(state="done", evidence=[effect["url"]])
    final_url = c.checkpoint(state, release=True)
    print(json.dumps(receipt | {"recovered_checkpoint": recovered_url,
                               "final_checkpoint": final_url, "audit": audit}))
    return 0


class FreshProcessRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.remote = seed_remote(self.root / "fake-remote.json")
        self.first_cache = self.root / "first-cache"
        self.second_cache = self.root / "second-cache"
        self.first_cache.mkdir()
        self.second_cache.mkdir()

    def process(self, phase, cache):
        environment = {key: value for key, value in os.environ.items()
                       if key in {"PATH", "SYSTEMROOT", "SystemRoot", "WINDIR", "TEMP", "TMP"}}
        return subprocess.run(
            [sys.executable, "-I", "-B", str(Path(__file__).resolve()),
             "--phase", phase, str(self.remote.path), str(cache)],
            cwd=cache, env=environment, capture_output=True, text=True,
            encoding="utf-8", timeout=30, check=False,
        )

    def interrupt(self):
        result = self.process("interrupt", self.first_cache)
        self.assertEqual(result.returncode, 23, result.stderr)
        receipt = json.loads(result.stdout)
        self.assertTrue(receipt["response_lost"])
        self.assertTrue(receipt["cache_was_empty"])
        self.assertEqual(list(self.first_cache.iterdir()), [])
        self.assertEqual(len(self.remote.load()["effects"]), 1)
        operations = [c.parse(row["body"], c.OPERATION)
                      for row in self.remote.load()["comments"]
                      if row["body"].startswith(c.OPERATION)]
        self.assertEqual([(row["operation_key"], row["attempt"], row["status"])
                          for row in operations], [(RECOVERY_KEY, 1, "pending")])
        return receipt

    def test_fresh_process_reconciles_once_and_completes_dependency_audit(self):
        first = self.interrupt()
        result = self.process("resume", self.second_cache)
        self.assertEqual(result.returncode, 0, result.stderr)
        second = json.loads(result.stdout)
        self.assertTrue(second["cache_was_empty"])
        self.assertNotEqual(first["pid"], second["pid"])
        self.assertEqual(first["initial_checkpoint"], second["initial_checkpoint"])
        final = c.read_checkpoint(second["final_checkpoint"], self.remote)
        tasks = {task["key"]: task for task in final["tasks"]}
        # Literal outcome oracles, not a second call to the production projector.
        self.assertIsNone(tasks["Q7"]["review_repair_batches"])
        self.assertEqual(tasks["Q7"]["attempts"], {"repair": 11, "operation": 0})
        self.assertEqual(tasks["Q10"]["review_repair_batches"], 4)
        self.assertEqual(tasks["EXHAUSTED"]["attempts"], {"operation": 3, "repair": 3})
        self.assertEqual(tasks["EXHAUSTED"]["failures"], [
            {"signature": "unchanged-failure", "attempts": 3}])
        self.assertEqual(tasks["Q8"]["performance"], {
            "repair_attempts": 2, "experiment_attempts": 2,
            "results": [{"experiment": 1, "passed": False},
                        {"experiment": 2, "passed": False}]})
        self.assertEqual(tasks["MAIN"]["state"], "awaiting_human")
        self.assertEqual(final["authorization"], {
            "source": "https://example.test/retained-user-grant",
            "allowed": ["evidence_publication"],
            "excluded": ["main_merge", "main_push", "auto_merge",
                         "merge_queue", "protection_changes"]})
        self.assertEqual(final["operation_history"][:2], [
            {"key": "legacy-unknown", "attempts": None, "last_status": None,
             "history": "unknown", "source": "https://example.test/legacy"},
            {"key": "exhausted-repair", "attempts": 3, "last_status": "failed",
             "journal_urls": ["https://example.test/old-journal"],
             "result_evidence": ["https://example.test/failure"]}])
        self.assertEqual([(row["key"], row["attempts"], row["last_status"])
                          for row in final["operation_history"][2:]], [
            (RECOVERY_KEY, 1, "succeeded"), (AUDIT_KEY, 1, "succeeded")])
        for row in final["operation_history"][2:]:
            records = [c.parse(self.remote.read(url)["body"], c.OPERATION)
                       for url in row["journal_urls"]]
            self.assertEqual([record["status"] for record in records], ["pending", "succeeded"])
            self.assertEqual(c.operation_identity(records[0]), c.operation_identity(records[1]))
        self.assertEqual([effect["key"] for effect in self.remote.load()["effects"]],
                         [RECOVERY_KEY, AUDIT_KEY])
        self.assertEqual(second["audit"], {
            "campaign_sha": MERGE,
            "satisfied": [{"task": "Q10", "merge": MERGE, "ci": "https://example.test/ci"}],
            "blocked": ["Q8", "EXHAUSTED"], "pending_human": ["MAIN"],
            "evidence_index": ["https://example.test/legacy"]})
        self.assertEqual(tasks["AUDIT"]["depends_on"], [
            {"task": "Q10", "condition": "integrated_campaign_with_green_ci",
             "evidence": {"merge_sha": MERGE, "ci": "https://example.test/ci"}}])
        self.assertEqual(tasks["AUDIT"]["state"], "done")
        self.assertEqual(tasks["AUDIT"]["evidence"], [self.remote.effect(AUDIT_KEY)["url"]])
        self.assertEqual(final["sequence"], 3)
        self.assertIsNone(final["lease"])
        self.assertIsNone(final["inflight"])

    def test_changed_business_readback_refuses_result_and_dependent_publication(self):
        self.interrupt()
        value = self.remote.load()
        value["effects"][0]["payload"]["state"] = "different"
        self.remote.save(value)
        before = self.remote.path.read_bytes()
        result = self.process("resume", self.second_cache)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Observed business readback differs", result.stderr)
        self.assertEqual(self.remote.path.read_bytes(), before)

    def test_current_dependency_failure_refuses_audit_after_recovery(self):
        self.interrupt()
        value = self.remote.load()
        value["refs"]["ci"][MERGE] = "failure"
        self.remote.save(value)
        result = self.process("resume", self.second_cache)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Audit dependency lacks current merge/CI proof", result.stderr)
        self.assertEqual([effect["key"] for effect in self.remote.load()["effects"]], [RECOVERY_KEY])
        state = c.read_checkpoint(self.remote.latest_checkpoint(), self.remote)
        self.assertEqual(state["operation_history"][-1]["key"], RECOVERY_KEY)
        self.assertEqual(state["operation_history"][-1]["last_status"], "succeeded")
        self.assertEqual(next(task for task in state["tasks"] if task["key"] == "AUDIT")["state"], "ready")

    def test_fresh_process_refuses_broken_partition_pending_identity_owner_or_lease(self):
        self.interrupt()
        original = self.remote.load()
        errors = {
            "missing-part": "Missing fake remote comment",
            "corrupt-part": "Part digest/length mismatch",
            "pending-identity": "Conflicting operation identity/payload",
            "foreign-owner": "Different operation writer",
            "expired": "Lease expired",
        }
        for defect, expected_error in errors.items():
            with self.subTest(defect=defect):
                value = copy.deepcopy(original)
                if defect == "missing-part":
                    del value["comments"][0]
                elif defect == "corrupt-part":
                    part = c.parse(value["comments"][0]["body"], c.PART)
                    part["sha256"] = "0" * 64
                    value["comments"][0]["body"] = c.body(c.PART, part)
                elif defect == "pending-identity":
                    record = c.parse(value["comments"][-1]["body"], c.OPERATION)
                    record["expected_readback"] = {"state": "different"}
                    value["comments"].append({"url": PARENT + "#issuecomment-999",
                                              "body": c.body(c.OPERATION, record)})
                elif defect == "foreign-owner":
                    record = c.parse(value["comments"][-1]["body"], c.OPERATION)
                    record["owner"] = "offline/another-writer"
                    value["comments"][-1]["body"] = c.body(c.OPERATION, record)
                else:
                    value["now"] = "2026-01-01T01:00:00+00:00"
                self.remote.save(value)
                before = self.remote.path.read_bytes()
                result = self.process("resume", self.second_cache)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected_error, result.stderr)
                self.assertEqual(self.remote.path.read_bytes(), before)
                self.assertEqual(list(self.second_cache.iterdir()), [])


class CliPaginationTests(unittest.TestCase):
    def traverse(self, boundary):
        rows = [{"url": PARENT + "#issuecomment-" + str(index), "body": "record " + str(index)}
                for index in range(1, 16)]
        calls = []

        def raw_cli(argv, **kwargs):
            self.assertEqual(argv[:3], ["gh", "api", "graphql"])
            self.assertTrue(kwargs["capture_output"])
            self.assertTrue(kwargs["text"])
            self.assertEqual(kwargs["encoding"], "utf-8")
            query = next(arg for arg in argv if arg.startswith("query="))
            self.assertIn("comments(last:5,before:$before)", query)
            self.assertNotIn("last:100", query)
            self.assertIn("number=330", argv)
            cursor = next((arg for arg in argv if arg.startswith("before=")), None)
            self.assertEqual(cursor, [None, "before=cursor-11", "before=cursor-6"][len(calls)])
            page_index = len(calls)
            calls.append(list(argv))
            start = [10, 5, 0][page_index]
            envelope = {"data": {"repository": {"issue": {"comments": {
                "nodes": rows[start:start + 5],
                "pageInfo": {"hasPreviousPage": page_index < 2,
                             "startCursor": ["cursor-11", "cursor-6", "cursor-1"][page_index]},
            }}}}}
            return subprocess.CompletedProcess(argv, 0, stdout=json.dumps(envelope), stderr="")

        # Keep GhTransport -> data -> gh -> raw subprocess decoding intact.
        with patch.object(c, "_publisher", c.Publisher(None, WRITER)), patch.object(
                c.subprocess, "run", side_effect=raw_cli):
            result = c.GhTransport(330).comments_since(boundary)
        return result, calls

    def test_three_bounded_raw_cli_pages_preserve_order_and_find_boundary(self):
        rows, calls = self.traverse(PARENT + "#issuecomment-3")
        self.assertEqual(len(calls), 3)
        self.assertEqual([row["url"] for row in rows],
                         [PARENT + "#issuecomment-" + str(index) for index in range(1, 16)])
        self.assertEqual([row["body"] for row in rows], ["record " + str(index) for index in range(1, 16)])
        self.assertEqual(len({row["url"] for row in rows}), 15)

    def test_missing_boundary_refused_after_all_three_raw_cli_pages(self):
        with self.assertRaisesRegex(c.ReconcileError, "Expected boundary absent from all comment pages"):
            self.traverse(PARENT + "#issuecomment-404")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--phase":
        with patch("socket.socket", side_effect=AssertionError("Network in offline process")), patch.object(
                c.subprocess, "run", side_effect=AssertionError("CLI in fake-remote process")):
            sys.exit(run_phase(*sys.argv[2:]))
    unittest.main(verbosity=2)
