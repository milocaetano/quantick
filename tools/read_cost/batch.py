#!/usr/bin/env python3
"""Batch merged-PR measurements into one maintenance PR; never merge it."""

import json
import os
from pathlib import Path
import subprocess

import ledger

TITLE = "chore: batch read-cost ledger"
PREFIX = "docs/read-cost-ledger-"


def run(args, repo):
    completed = subprocess.run(args, cwd=repo, text=True, encoding="utf-8",
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    return completed.stdout.strip()


def confirmed_pr_permission_denial(error):
    detail = "\n".join(part for part in (error.stdout, error.stderr) if part).lower()
    return ("resource not accessible by integration" in detail
            or "github actions is not permitted to create or approve pull requests" in detail)


def maintain(repo, run_id, execute=run, collect=ledger.batch):
    def call(*args):
        return execute(list(args), repo)

    pulls = json.loads(call("gh", "pr", "list", "--base", "main", "--state", "open",
                           "--search", f'in:title "{TITLE}"',
                           "--json", "number,headRefName,isCrossRepository"))
    existing = [pull for pull in pulls if pull["headRefName"].startswith(PREFIX)
                and not pull["isCrossRepository"]]
    if len(existing) > 1:
        raise RuntimeError("Multiple ledger PRs exist; reconcile before batching.")
    call("git", "config", "user.name", "github-actions[bot]")
    call("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    branch = existing[0]["headRefName"] if existing else PREFIX + run_id
    if existing:
        call("git", "fetch", "origin", branch)
        call("git", "checkout", "-B", branch, "origin/" + branch)
    else:
        call("git", "checkout", "-b", branch, "origin/main")
    before = call("git", "rev-parse", "HEAD")
    if existing:
        call("git", "merge", "origin/main", "--no-edit")
    collect(str(repo), str(Path(repo) / ledger.DEFAULT_PATH))
    call("git", "add", "--", ledger.DEFAULT_PATH)
    if call("git", "diff", "--cached", "--name-only"):
        call("git", "commit", "-m", TITLE)
    if call("git", "rev-parse", "HEAD") == before:
        return
    call("git", "push", "origin", branch)
    if not existing:
        body = Path(repo) / ".git-ledger-pr-body.tmp"
        body.write_text("Collect measured rows for merged PRs without changing their reviewed heads.\n\n"
                        "The user merges this maintenance PR after final-head CI passes.\n", encoding="utf-8")
        try:
            call("gh", "pr", "create", "--base", "main", "--head", branch,
                 "--title", TITLE, "--body-file", str(body))
        except subprocess.CalledProcessError as error:
            if confirmed_pr_permission_denial(error):
                message = ("Bot PR creation was denied by GitHub authorization. Enable the "
                           "repository's existing Actions PR permission and rerun; no settings "
                           "or merge authority are changed by this job.")
            else:
                message = ("Bot PR creation did not return a confirmed result. Read back open "
                           "ledger PRs, then retry; the request may have failed before or after "
                           "GitHub created the PR.")
            raise RuntimeError(message) from error
        finally:
            body.unlink()


if __name__ == "__main__":
    maintain(Path.cwd(), os.environ["GITHUB_RUN_ID"] + "-" + os.environ.get("GITHUB_RUN_ATTEMPT", "1"))
