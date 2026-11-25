#!/usr/bin/env python3
"""Who runs when, read from the real workflow: python tools/ci/test_workflow_policy.py

Evaluates ci.yml's `if:` and `concurrency:` expressions against event
fixtures, so a predicate that stops matching the policy in
.claude/hooks/README.md ("Full CI at the exact head") fails here, not on a
pull request.
"""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
FULL = (ROOT / ".claude/hooks/full_ci.sh").read_text(encoding="utf-8")
JOBS = WORKFLOW.split("\njobs:\n", 1)[1]


def expression(source, event):
    """Evaluate the workflow's small expression subset against an event fixture."""
    source = re.sub(r"(?:github|needs)\.[\w.*-]+", lambda m: repr(event[m[0]]), source)
    source = source.replace("&&", " and ").replace("||", " or ")
    source = re.sub(r"!(?!=)", " not ", source)
    return eval(" ".join(source.splitlines()), {"__builtins__": {}, "format": str.format,
                                                "contains": lambda values, value: value in values})


def event(name="pull_request", number=7, run=100, draft=False, action="synchronize",
          labels=(), label="", mode="build"):
    return {"github.workflow": "CI", "github.event_name": name,
            "github.event.pull_request.number": number, "github.run_id": run,
            "github.event.pull_request.draft": draft, "github.event.action": action,
            "github.event.pull_request.labels.*.name": labels,
            "github.event.label.name": label,
            "github.event.pull_request.base.sha": "base-sha" if name == "pull_request" else None,
            "needs.gate.outputs.mode": mode}


def scalar(body, key):
    match = re.search(rf"^ +{re.escape(key)}: (.+)$", body, re.MULTILINE)
    if match is None:
        raise AssertionError(f"missing {key}")
    return match[1]


def folded_if(body):
    """A job's `if:`, inline or folded (`>-`), as one expression."""
    match = re.search(r"^    if: (?:&[\w-]+ )?>-\n(.*?)^    [\w-]+:", body, re.MULTILINE | re.DOTALL)
    if match:
        return match[1]
    inline = scalar(body, "if")
    # `*full-ci` is the YAML alias of the predicate `ci` anchors.
    return FULL_PREDICATE if inline == "*full-ci" else inline


JOB_BODIES = dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)", JOBS, re.MULTILINE | re.DOTALL))
REQUIRED = re.search(r"^FULL_CI_CHECKS='([^']+)'", FULL, re.MULTILINE)[1].split()
RUST_JOBS = ["fast"] + [job for job in REQUIRED if job != "gate"]
FULL_PREDICATE = re.search(r"^    if: &full-ci >-\n(.*?)^    [\w-]+:", JOB_BODIES["ci"],
                           re.MULTILINE | re.DOTALL)[1]


class WorkflowPolicy(unittest.TestCase):
    def test_only_superseded_pr_runs_cancel(self):
        policy = WORKFLOW.split("\nconcurrency:\n", 1)[1].split("\nenv:\n", 1)[0]
        group = scalar(policy, "group")
        cancel = scalar(policy, "cancel-in-progress")[3:-3]

        def evaluated(fixture):
            return re.sub(r"\$\{\{(.*?)\}\}", lambda m: str(expression(m[1], fixture)), group)

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

    def test_required_jobs_are_every_gated_job(self):
        self.assertEqual("gate", REQUIRED[0])
        self.assertEqual(set(REQUIRED) | {"fast", "rust-skip", "read-cost", "read-cost-ledger"},
                         set(JOB_BODIES))
        self.assertIn("RUST_SKIP_CHECK='rust-skip'", FULL)

    def test_every_rust_job_waits_on_a_green_gate(self):
        for name in RUST_JOBS:
            with self.subTest(job=name):
                body = JOB_BODIES[name]
                self.assertEqual("gate", scalar(body, "needs"))
                self.assertNotRegex(body, r"(?:always|failure|cancelled)\s*\(")
                self.assertNotIn("continue-on-error:", body)
        for name in set(REQUIRED) - {"gate", "ci"}:
            with self.subTest(job=name):
                self.assertEqual("*full-ci", scalar(JOB_BODIES[name], "if"))

    def test_the_gate_runs_on_every_validating_event(self):
        gate = folded_if(JOB_BODIES["gate"])
        for fixture, expected in [(event(name="push"), True), (event(), True),
                                  (event(draft=True), True),
                                  (event(action="ready_for_review"), True),
                                  (event(action="labeled", label="full-ci"), True),
                                  (event(action="labeled", label="unrelated"), False)]:
            with self.subTest(event=fixture):
                self.assertEqual(expected, expression(gate, fixture))

    def test_no_edit_event_starts_a_run(self):
        types = re.search(r"^    types: \[(.*)\]$", WORKFLOW, re.MULTILINE)[1].split(", ")
        self.assertNotIn("edited", types)

    def test_only_a_pr_into_the_default_branch_can_be_prose(self):
        decide = JOB_BODIES["gate"].split("name: Decide what this head needs", 1)[1]
        self.assertIn('elif [ "$BASE_REF" = "$DEFAULT_BRANCH" ]; then\n'
                      '              mode=$(python3 tools/ci/prose_only.py', decide)
        self.assertEqual("${{ github.event.repository.default_branch }}", scalar(decide, "DEFAULT_BRANCH"))
        self.assertEqual("${{ github.event.pull_request.base.ref }}", scalar(decide, "BASE_REF"))

    def test_the_gate_carries_the_fast_checks(self):
        gate = JOB_BODIES["gate"]
        self.assertNotRegex(gate, r"(?m)^    (?:needs|continue-on-error):")
        self.assertIn("fetch-depth: 0", gate)
        for step in ("cargo fmt --all -- --check",
                     'git worktree add --detach "$RUNNER_TEMP/head" "$HEAD_SHA"',
                     "HEAD_SHA: ${{ github.event.pull_request.head.sha || github.sha }}",
                     'python3 tools/ci/ratchet_delta.py --base "$RATCHET_BASE" --export-env "$GITHUB_ENV"',
                     "cargo test -p quantick-guards",
                     "cargo test -p quantick-control -p quantick-operability",
                     "sh .claude/hooks/guardrails_test.sh",
                     "python3 tools/ci/test_workflow_policy.py",
                     "python3 tools/ci/test_prose_only.py",
                     'python3 tools/ci/prose_only.py --base "$BASE_SHA" --head "$HEAD_SHA" --search HEAD'):
            with self.subTest(step=step):
                self.assertTrue(step in gate, step)
        base = scalar(gate, "RATCHET_BASE")[3:-3]
        self.assertEqual("base-sha", expression(base, event()))
        self.assertEqual("HEAD^", expression(base, event(name="push")))
        # The guard and drift steps fail before the slower suites start.
        self.assertLess(gate.index("cargo test -p quantick-guards"), gate.index("guardrails_test.sh"))

    def test_no_network_fetch_holds_the_rust_jobs(self):
        self.assertNotIn("pipx", JOB_BODIES["gate"])
        self.assertIn("pipx run ruff check --select F tools/ bridge/mt5/", JOB_BODIES["ci"])

    def test_fmt_runs_once_on_linux(self):
        linux_fmt = [name for name, body in JOB_BODIES.items()
                     if "cargo fmt --all -- --check" in body and "windows-latest" not in body]
        self.assertEqual(["gate"], linux_fmt)

    def test_full_jobs_preserve_event_coverage(self):
        fixtures = [(event(name="push"), True), (event(), True),
                    (event(draft=True), False),
                    (event(draft=True, labels=("full-ci",)), True),
                    (event(action="ready_for_review"), True),
                    (event(draft=True, action="labeled", label="full-ci"), True),
                    (event(action="labeled", label="unrelated"), False)]
        for fixture, expected in fixtures:
            with self.subTest(event=fixture):
                self.assertEqual(expected, expression(FULL_PREDICATE, fixture))

    def test_a_head_that_needs_no_build_runs_no_rust_job(self):
        for mode in ("prose", "reused"):
            for fixture in (event(mode=mode), event(draft=True, mode=mode),
                            event(action="ready_for_review", labels=("full-ci",), mode=mode),
                            event(draft=True, action="labeled", label="full-ci", mode=mode)):
                with self.subTest(mode=mode, event=fixture):
                    for name in RUST_JOBS:
                        self.assertFalse(expression(folded_if(JOB_BODIES[name]), fixture), name)
                    self.assertTrue(expression(folded_if(JOB_BODIES["rust-skip"]), fixture))
        self.assertFalse(expression(folded_if(JOB_BODIES["rust-skip"]), event()))
        self.assertEqual("gate", scalar(JOB_BODIES["rust-skip"], "needs"))

    def test_draft_fast_signal(self):
        fast = folded_if(JOB_BODIES["fast"])
        self.assertTrue(expression(fast, event(draft=True)))
        self.assertTrue(expression(fast, event(draft=True, labels=("full-ci",))))
        self.assertFalse(expression(fast, event()))
        self.assertFalse(expression(fast, event(name="push")))

    def test_ready_after_a_full_ci_label_reuses_only_a_finished_green_run(self):
        decide = JOB_BODIES["gate"].split("name: Decide what this head needs", 1)[1]
        self.assertIn('[ "$ACTION" = ready_for_review ] && [ "$FULL_CI_LABELLED" = true ]', decide)
        self.assertIn('QUANTICK_FULL_CI_FOR_REUSE=1 sh .claude/hooks/full_ci.sh verify . "$HEAD_SHA"',
                      decide)
        self.assertIn("contains(github.event.pull_request.labels.*.name, 'full-ci')",
                      scalar(decide, "FULL_CI_LABELLED"))
        self.assertIn("checks: read", JOB_BODIES["gate"])


if __name__ == "__main__":
    unittest.main()
