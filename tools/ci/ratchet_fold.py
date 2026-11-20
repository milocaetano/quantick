#!/usr/bin/env python3
"""Maintain one user-merged ratchet fold PR; never merge or force-push."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys


def run(args, repo):
    return subprocess.check_output(args, cwd=repo, text=True, encoding="utf-8").strip()


def maintain(repo, run_id, execute=run):
    def call(*args):
        return execute(list(args), repo)

    call("git", "config", "user.name", "github-actions[bot]")
    call("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    existing = json.loads(call("gh", "pr", "list", "--base", "main", "--state", "open",
                              "--search", 'in:title "chore: fold ratchet fragments"',
                              "--json", "number,headRefName"))
    branch = existing[0]["headRefName"] if existing else f"docs/ratchet-fold-{run_id}"
    call("git", "checkout", *([branch] if existing else ["-b", branch]))
    before = call("git", "rev-parse", "HEAD")
    if existing:
        call("git", "merge", "origin/main", "--no-edit")
    call("cargo", "run", "-q", "-p", "quantick-guards", "--", "--tighten", "--fold")
    call(sys.executable, "tools/ci/ratchet_delta.py", "--base", "origin/main")
    paths = call("git", "ls-files", "--modified", "--deleted", "--others", "--exclude-standard", "-z")
    changed = sorted({path for path in paths.split("\0") if re.fullmatch(
        r"crates/guards/[^/]+-baseline(?:\.txt|\.d/[^/]+\.txt)", path)})
    if changed:
        call("git", "add", "-A", "--", *changed)
    if call("git", "diff", "--cached", "--name-only"):
        call("git", "commit", "-m", "chore: fold ratchet fragments")
    if call("git", "rev-parse", "HEAD") == before:
        return
    call("git", "push", "origin", branch)
    if not existing:
        try:
            call("gh", "pr", "create", "--base", "main", "--head", branch,
                 "--title", "chore: fold ratchet fragments", "--body",
                 "Consolidate signed ratchet fragments after downward-only tightening. "
                 "The user approves waiting workflow runs and merges after green CI; "
                 "no automatic merge is configured.")
        except subprocess.CalledProcessError as error:
            raise RuntimeError("Bot PR creation requires Settings > Actions > General > "
                               "Workflow permissions > Allow GitHub Actions to create and "
                               "approve pull requests. Enable it and rerun; this workflow "
                               "never merges automatically.") from error


if __name__ == "__main__":
    maintain(Path.cwd(), os.environ["GITHUB_RUN_ID"] + "-" + os.environ.get("GITHUB_RUN_ATTEMPT", "1"))
