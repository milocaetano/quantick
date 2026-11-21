#!/usr/bin/env python3
"""Exercise review-key stability against real Git commits and tree changes."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("identity", Path(__file__).with_name("review_identity.py"))
identity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(identity)


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "core.autocrlf", "false")
        self.write("code.py", "# context\nif True:\n    action()\n")
        self.commit()
        self.git("branch", "base")
        self.write("code.py", "# context\nif True:\n    change()\n")
        self.commit()

    def git(self, *args):
        return identity.git(self.repo, *args)

    def write(self, name, text):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")

    def commit(self):
        self.git("add", "-A")
        self.git("commit", "-qm", "fixture")

    def key(self):
        return identity.identity(self.repo, "base")

    def test_context_only_sync_keeps_review_key(self):
        before = self.key()
        self.git("checkout", "-q", "base")
        self.write("code.py", "# added upstream\n# different context\nif True:\n    action()\n")
        self.commit()
        self.git("checkout", "-q", "main")
        self.git("rebase", "base")
        self.assertEqual(before, self.key())

    def test_fragment_edit_stales_review_key(self):
        before = self.key()
        self.write("crates/guards/app-lines-baseline.d/feat-example.txt", "crates/app +1\n!budget +1\n")
        self.commit()
        self.assertNotEqual(before, self.key())

    def test_indentation_only_substantive_edit_stales_review_key(self):
        before = self.key()
        self.write("code.py", "# context\nif True:\n        change()\n")
        self.commit()
        self.assertNotEqual(before, self.key())

    def test_moving_identical_edit_between_functions_stales_review_key(self):
        self.git("checkout", "-q", "base")
        self.write("policy.py", "def first():\n    return False\n\ndef second():\n    return False\n")
        self.commit()
        self.git("checkout", "-q", "-b", "semantic-head")
        self.write("policy.py", "def first():\n    return True\n\ndef second():\n    return False\n")
        self.commit()
        before = self.key()
        self.git("checkout", "-q", "base")
        self.write("policy.py", "def first():\n    return False\n\ndef second():\n    return True\n")
        self.commit()
        self.assertNotEqual(before, self.key())

    def test_moving_identical_edit_between_occurrences_stales_review_key(self):
        self.git("checkout", "-q", "base")
        self.write("policy.py", "def policy():\n    flags.append(False)\n    work()\n    flags.append(False)\n")
        self.commit()
        self.git("checkout", "-q", "-b", "first-occurrence")
        self.write("policy.py", "def policy():\n    flags.append(True)\n    work()\n    flags.append(False)\n")
        self.commit()
        before = self.key()
        self.git("checkout", "-q", "base")
        self.write("policy.py", "def policy():\n    flags.append(False)\n    work()\n    flags.append(True)\n")
        self.commit()
        self.assertNotEqual(before, self.key())

    def test_repeated_occurrence_identity_ignores_shifted_context(self):
        self.git("checkout", "-q", "base")
        self.write("policy.py", "def policy():\n    flags.append(False)\n    work()\n    flags.append(False)\n")
        self.commit()
        self.git("checkout", "-q", "-b", "repeated-head")
        self.write("policy.py", "def policy():\n    flags.append(False)\n    work()\n    flags.append(True)\n")
        self.commit()
        before = self.key()
        self.git("checkout", "-q", "base")
        self.write("policy.py", "# upstream context\ndef policy():\n    flags.append(False)\n    work()\n    flags.append(False)\n")
        self.commit()
        self.git("checkout", "-q", "repeated-head")
        self.git("rebase", "base")
        self.assertEqual(before, self.key())

    def test_rename_source_and_excluded_destination_both_affect_identity(self):
        self.git("checkout", "-q", "base")
        self.write("old_a.py", "same bytes\n")
        self.write("old_b.py", "same bytes\n")
        self.commit()
        self.git("checkout", "-q", "-b", "rename-head")
        destination = "schemas/control/example.schema.json"
        (self.repo / destination).parent.mkdir(parents=True, exist_ok=True)
        self.git("mv", "old_a.py", destination)
        self.commit()
        with patch.object(identity, "verified_outputs", return_value={destination}):
            before = self.key()
        self.git("checkout", "-q", "base")
        (self.repo / destination).parent.mkdir(parents=True, exist_ok=True)
        (self.repo / "old_b.py").replace(self.repo / destination)
        self.commit()
        with patch.object(identity, "verified_outputs", return_value={destination}):
            self.assertNotEqual(before, self.key())

    def test_baselines_inputs_and_unverified_outputs_stale(self):
        execute = identity.subprocess.run

        def unavailable(args, **kwargs):
            if args[0] == "cargo":
                raise OSError("generator unavailable")
            return execute(args, **kwargs)

        for name in ("crates/guards/app-lines-baseline.txt", "crates/control-handlers/src/generation.rs",
                     "schemas/control/example.schema.json", "docs/control-plane/ui-behaviour-matrix.md"):
            with self.subTest(name=name), patch.object(identity.subprocess, "run", side_effect=unavailable):
                before = self.key()
                self.write(name, "substantive\n")
                self.commit()
                self.assertNotEqual(before, self.key())

    def test_ledger_does_not_require_another_review(self):
        before = self.key()
        self.write(identity.LEDGER, "a row\n")
        self.commit()
        self.assertEqual(before, self.key())

    def test_file_modes_and_binary_content_stale(self):
        before = self.key()
        self.git("update-index", "--chmod=+x", "code.py")
        self.git("commit", "-qm", "mode")
        self.assertNotEqual(before, self.key())
        before = self.key()
        (self.repo / "data.bin").write_bytes(b"\0binary\xff")
        self.commit()
        self.assertNotEqual(before, self.key())

    def test_only_exact_generated_bytes_are_excluded_and_proof_is_tree_bound(self):
        generated = "schemas/control/example.schema.json"
        self.write(generated, "correct\n")
        self.commit()

        execute = identity.subprocess.run

        def produce(args, **kwargs):
            if args[0] != "cargo":
                return execute(args, **kwargs)
            output = Path(args[-1]) / generated
            output.parent.mkdir(parents=True)
            output.write_bytes(b"correct\n")

        with patch.object(identity.subprocess, "run", side_effect=produce) as generator:
            self.assertEqual(identity.verified_outputs(self.repo, [generated]), {generated})
            self.assertEqual(identity.verified_outputs(self.repo, [generated]), {generated})
            self.assertEqual(sum(call.args[0][0] == "cargo" for call in generator.call_args_list), 1)
            before = self.key()
            self.git("update-index", "--chmod=+x", generated)
            self.git("commit", "-qm", "generated mode")
            self.assertNotEqual(before, self.key())
            self.write(generated, "wrong\n")
            self.commit()
            self.assertEqual(identity.verified_outputs(self.repo, [generated]), set())
            self.assertGreaterEqual(sum(call.args[0][0] == "cargo" for call in generator.call_args_list), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
