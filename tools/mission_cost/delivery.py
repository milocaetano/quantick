#!/usr/bin/env python3
"""Pull-request and CI timings from `gh`, and the read-cost row from the ledger.

Section 4 of ``docs/quality/mission-cost/method.md``. Only the timings the
method names are taken: a pull request's open duration, how long CI ran and how
many runs there were. The title, the body and the review conversation are not
read, for the same reason the transcript reader takes five fields.

Every call goes through an injectable ``runner`` so the tests answer from a
table rather than the network.
"""

import datetime
import importlib.util
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
LEDGER = os.path.join("docs", "quality", "read-cost", "ledger.md")

PULL_FIELDS = (
    "number,headRefName,baseRefName,headRefOid,createdAt,mergedAt,closedAt,state"
)
RUN_FIELDS = "status,conclusion,headSha,startedAt,updatedAt,workflowName"

# How many workflow runs to ask `gh` for. A busy campaign branch can have
# hundreds, and a listing cut off at the limit undercounts CI time without
# saying so, which is worse than a slow call. The report says when the listing
# came back full, so a truncated one is visible rather than quietly low.
CI_RUN_LIMIT = 300


class DeliveryError(RuntimeError):
    """A delivery timing could not be read."""


def load(name, path=None):
    """Load a module by path under a stable key, the way `tools/read_cost` does.

    One helper, the same shape in every module of this package, so a missing
    file is one error rather than three different ones.
    """
    key = f"quantick_mission_cost_{name}"
    if key in sys.modules:
        return sys.modules[key]
    path = path or os.path.join(HERE, f"{name}.py")
    spec = importlib.util.spec_from_file_location(key, path)
    if spec is None or spec.loader is None:
        raise DeliveryError(f"cannot load {name} at {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module


TRANSCRIPTS = load("transcripts")


def load_read_cost_ledger():
    """Reuse `tools/read_cost/ledger.py`'s parser rather than a second one."""
    return load(
        "read_cost_ledger", os.path.join(HERE, "..", "read_cost", "ledger.py")
    )


def shell(args):
    """Run a command and return its standard output."""
    try:
        done = subprocess.run(
            list(args), check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
    except (OSError, subprocess.CalledProcessError) as problem:
        detail = getattr(problem, "stderr", b"") or b""
        raise DeliveryError(
            f"{' '.join(args)} failed: {detail.decode('utf-8', 'replace').strip()}"
        ) from problem
    return done.stdout.decode("utf-8", "replace")


def _seconds(start, end):
    first = TRANSCRIPTS.parse_timestamp(start)
    last = TRANSCRIPTS.parse_timestamp(end)
    if first is None or last is None:
        return None
    return (last - first).total_seconds()


def pull_facts(number, runner=shell, as_of=None):
    """The timings of one pull request, and nothing else it carries."""
    raw = runner(["gh", "pr", "view", str(number), "--json", PULL_FIELDS])
    try:
        found = json.loads(raw)
    except ValueError as problem:
        raise DeliveryError(f"gh pr view {number} did not return JSON") from problem
    ended, source = found.get("mergedAt"), "merged"
    if not ended:
        ended, source = found.get("closedAt"), "closed"
    if not ended:
        ended, source = as_of, "as_of"
    if not ended:
        source = "open"
    return {
        "pr": found.get("number"),
        "branch": found.get("headRefName"),
        "base": found.get("baseRefName"),
        "head": found.get("headRefOid"),
        "state": found.get("state"),
        "created_at": found.get("createdAt"),
        "ended_at": ended,
        "ended_at_source": source,
        "pr_open_seconds": _seconds(found.get("createdAt"), ended) if ended else None,
    }


def ci_facts(branch, runner=shell, limit=CI_RUN_LIMIT):
    """Completed workflow runs on one branch: their sum, their wall and their count.

    ``ci_seconds`` adds the runs up; ``ci_wall_seconds`` counts two workflows
    running side by side once. Error mode E10: both include queue time.

    ``ci_listing_truncated`` says the listing came back exactly full, which is
    how a branch with more runs than ``CI_RUN_LIMIT`` announces that its CI
    time is a floor rather than a total.
    """
    raw = runner(
        [
            "gh",
            "run",
            "list",
            "--branch",
            branch,
            "--limit",
            str(limit),
            "--json",
            RUN_FIELDS,
        ]
    )
    try:
        runs = json.loads(raw)
    except ValueError as problem:
        raise DeliveryError(f"gh run list for {branch} did not return JSON") from problem
    spans = []
    for run in runs:
        if run.get("status") != "completed":
            continue
        start = TRANSCRIPTS.parse_timestamp(run.get("startedAt"))
        end = TRANSCRIPTS.parse_timestamp(run.get("updatedAt"))
        if start is None or end is None or end < start:
            continue
        spans.append((start, end))
    return {
        "ci_runs": len(spans),
        "ci_seconds": round(
            sum((end - start).total_seconds() for start, end in spans), 3
        ),
        "ci_wall_seconds": round(TRANSCRIPTS.union_seconds(spans), 3),
        "ci_listing_truncated": len(runs) >= limit,
    }


def first_commit_instant(number, runner=shell):
    """The committer date of the pull request's earliest own commit.

    GitHub's own commit list, not a local ``git merge-base``: once a branch is
    merged its head is an ancestor of the base ref, the merge base collapses
    onto the head and the range comes back empty. The set is the same one the
    method names -- the commits reachable from the head and not from the base.

    Error mode E3: a rebase rewrites committer dates, so the registry's
    explicit ``started_at`` overrides whatever this says.
    """
    raw = runner(["gh", "pr", "view", str(number), "--json", "commits"])
    try:
        commits = json.loads(raw).get("commits") or []
    except (ValueError, AttributeError):
        return None
    dates = [
        commit.get("committedDate")
        for commit in commits
        if isinstance(commit, dict) and commit.get("committedDate")
    ]
    return min(dates) if dates else None


def read_cost_row(path, pr):
    """That pull request's recorded read cost, or ``None`` when it has no row."""
    ledger = load_read_cost_ledger()
    try:
        with open(path, "r", encoding="utf-8") as stream:
            text = stream.read()
    except OSError:
        return None
    for row in ledger.parse_rows(text):
        if row["pr"] == pr:
            return row["read_cost"]
    return None


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()
