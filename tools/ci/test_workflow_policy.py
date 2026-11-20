#!/usr/bin/env python3
"""C6: evaluate cancellation and prerequisite policy from the real workflow."""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
FULL = (ROOT / ".claude/hooks/full_ci.sh").read_text(encoding="utf-8")


def expression(source, event):
    """Evaluate the workflow's small expression subset against event fixtures."""
    source = re.sub(r"github\.[\w.*]+", lambda m: repr(event[m[0]]), source)
    source = source.replace("&&", " and ").replace("||", " or ")
    source = re.sub(r"!(?!=)", " not ", source)
    return eval(" ".join(source.splitlines()), {"__builtins__": {}, "format": str.format,
                                "contains": lambda values, value: value in values})


def event(name="pull_request", number=7, run=100, draft=False,
          action="synchronize", labels=(), label=""):
    return {"github.workflow": "CI", "github.event_name": name,
            "github.event.pull_request.number": number, "github.run_id": run,
            "github.event.pull_request.draft": draft, "github.event.action": action,
            "github.event.pull_request.labels.*.name": labels,
            "github.event.label.name": label,
            "github.event.pull_request.base.sha": "base-sha" if name == "pull_request" else None}


def scalar(body, key):
    match = re.search(rf"^ +{re.escape(key)}: (.+)$", body, re.MULTILINE)
    if match is None:
        raise AssertionError(f"missing {key}")
    return match[1]


class WorkflowPolicy(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.jobs = dict(re.findall(
            r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)",
            WORKFLOW.split("\njobs:\n", 1)[1], re.MULTILINE | re.DOTALL))
        cls.required = re.search(r"^FULL_CI_CHECKS='([^']+)'", FULL, re.MULTILINE)[1].split()

    def test_C6_only_superseded_pr_runs_cancel(self):
        policy = WORKFLOW.split("\nconcurrency:\n", 1)[1].split("\nenv:\n", 1)[0]
        group = scalar(policy, "group")
        cancel = scalar(policy, "cancel-in-progress")[3:-3]

        def evaluated(fixture):
            return re.sub(r"\$\{\{(.*?)\}\}",
                          lambda m: str(expression(m[1], fixture)), group)

        self.assertTrue(expression(cancel, event()))
        self.assertEqual(evaluated(event(run=100)), evaluated(event(run=101)))
        self.assertNotEqual(evaluated(event(number=7)), evaluated(event(number=8)))
        unrelated = event(action="labeled", label="unrelated")
        self.assertFalse(expression(cancel, unrelated))
        self.assertNotEqual(evaluated(event()), evaluated(unrelated))
        self.assertTrue(expression(cancel, event(action="labeled", label="full-ci")))
        for name in ("push", "workflow_dispatch", "schedule"):
            with self.subTest(event=name):
                self.assertFalse(expression(cancel, event(name=name)))
                self.assertNotEqual(evaluated(event(name=name, run=100)),
                                    evaluated(event(name=name, run=101)))
                self.assertNotEqual(evaluated(event()), evaluated(event(name=name)))
        self.assertNotIn("concurrency:", WORKFLOW.split("\njobs:\n", 1)[1])

    def test_C6_expensive_jobs_require_successful_guards(self):
        self.assertEqual({"guards", "ci", "linux-tests", "harness-app",
                          "harness-combined", "harness-scenario", "windows",
                          "windows-tests", "windows-heavy-tests", "windows-app-tests"},
                         set(self.required))
        self.assertEqual(set(self.required), set(self.jobs) - {"fast", "read-cost", "read-cost-ledger"})
        for name in ["fast"] + [job for job in self.required if job != "guards"]:
            with self.subTest(job=name):
                self.assertEqual("guards", scalar(self.jobs[name], "needs"))
                self.assertNotRegex(self.jobs[name], r"(?:always|failure|cancelled)\s*\(")
                self.assertNotIn("continue-on-error:", self.jobs[name])
        guards = self.jobs["guards"]
        self.assertNotRegex(guards, r"(?m)^    (?:if|needs|continue-on-error):")
        self.assertIn("fetch-depth: 0", guards)
        self.assertIn('python tools/ci/ratchet_delta.py --base "$RATCHET_BASE" --export-env "$GITHUB_ENV"', guards)
        self.assertIn("cargo test -p quantick-guards", guards)
        base = scalar(guards, "RATCHET_BASE")[3:-3]
        self.assertEqual("base-sha", expression(base, event()))
        self.assertEqual("HEAD^", expression(base, event(name="push")))

    def test_C6_full_jobs_preserve_event_coverage(self):
        predicate = re.search(r"    if: &full-ci >-\n(.*?)^    runs-on:",
                              self.jobs["ci"], re.MULTILINE | re.DOTALL)[1]
        fixtures = [(event(name="push"), True), (event(), True),
                    (event(draft=True), False),
                    (event(draft=True, labels=("full-ci",)), True),
                    (event(action="ready_for_review"), True),
                    (event(draft=True, action="labeled", label="full-ci"), True),
                    (event(action="labeled", label="unrelated"), False)]
        for fixture, expected in fixtures:
            with self.subTest(event=fixture):
                self.assertEqual(expected, expression(predicate, fixture))
        for name in set(self.required) - {"ci", "guards"}:
            self.assertEqual("*full-ci", scalar(self.jobs[name], "if"))
        fast = re.search(r"    if: >-\n(.*?)^    runs-on:", self.jobs["fast"],
                         re.MULTILINE | re.DOTALL)[1]
        self.assertTrue(expression(fast, event(draft=True)))
        self.assertTrue(expression(fast, event(draft=True, labels=("full-ci",))))
        self.assertFalse(expression(fast, event()))
        self.assertFalse(expression(fast, event(name="push")))
        self.assertFalse(expression(fast, event(draft=True, action="labeled", label="full-ci")))
        self.assertIn("python3 tools/ci/test_workflow_policy.py", self.jobs["guards"])
        self.assertIn("sh .claude/hooks/full_ci_test.sh", self.jobs["guards"])
        self.assertIn("sh .claude/hooks/guardrails_test.sh", self.jobs["guards"])


if __name__ == "__main__":
    unittest.main()
