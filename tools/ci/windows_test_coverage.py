#!/usr/bin/env python3
"""Assert that the Windows jobs test every workspace member exactly once.

`cargo test --workspace` in one job needed no guard: cargo found the members,
so a crate added on Monday was tested on Monday. Splitting that run across
jobs — one carrying `--workspace --exclude <heavy crates>`, the others
carrying `-p <heavy crate>` — keeps the property only as long as the shares
stay a partition, and nothing in a workflow file checks that. A typo in an
`--exclude`, or a second heavy crate peeled off and its `-p` job forgotten,
would drop a crate's Windows tests silently and leave every run green.

Both directions matter. A crate no share claims loses its Windows tests. A
crate two shares both claim is paid for twice: the split exists to spend fewer
runner minutes, and an overlap quietly spends them again, which is how a
partition rots back into the serial job it replaced without anyone reading a
red line.

So the shares are read back out of `ci.yml` and compared with the member list
`cargo metadata` reports. A share is a step named exactly `Test`: that is a
job's slice of the workspace run. A step with any other name is a deliberate
extra — `windows` runs one crate again under `--nocapture` for its Windows
descriptor diagnostics — and counts as coverage without counting as a share.
The check is over the Windows jobs specifically: the Linux side still runs one
`cargo test --workspace`.
"""

import collections
import json
import re
import subprocess
import sys

WORKFLOW = ".github/workflows/ci.yml"

SHARE_STEP = "Test"


def windows_jobs(workflow_text):
    """Return {job name: job body} for every job that runs on Windows."""
    jobs = {}
    name = None
    body = []
    for line in workflow_text.splitlines():
        match = re.match(r"^  ([a-z0-9-]+):$", line)
        if match:
            if name is not None:
                jobs[name] = "\n".join(body)
            name, body = match.group(1), []
            continue
        if name is not None:
            body.append(line)
    if name is not None:
        jobs[name] = "\n".join(body)
    return {n: b for n, b in jobs.items() if "runs-on: windows-latest" in b}


def test_steps(body):
    """Return (step name, command) for every `cargo test` step in a job.

    A step's name decides whether it is a share, so an unnamed step must come
    back unnamed rather than inheriting the name of the step above it: a
    diagnostic written without a name would otherwise be counted as a second
    share and report a crate tested twice. A step may also be written with its
    `run:` on the item line itself, and that is still a run of the tests.
    """
    found = []
    step_name = None
    for line in body.splitlines():
        if re.match(r"^\s*-\s", line):
            step_name = None
        named = re.match(r"^\s*- name: (.+?)\s*$", line)
        if named:
            step_name = named.group(1)
            continue
        command = re.match(r"^\s*(?:- )?run: (cargo test .*?)\s*$", line)
        if command:
            found.append((step_name, command.group(1)))
    return found


def selected(command, members):
    """The workspace members one `cargo test` command runs."""
    # Anything after a bare `--` is a test-binary argument, not a package
    # selector.
    selectors = command.split(" -- ")[0].split()
    reached = set()
    if "--workspace" in selectors:
        excluded = {
            selectors[i + 1]
            for i, token in enumerate(selectors)
            if token == "--exclude" and i + 1 < len(selectors)
        }
        unknown = excluded - members
        if unknown:
            raise ValueError(f"--exclude names no workspace member: {sorted(unknown)}")
        reached |= members - excluded
    for i, token in enumerate(selectors):
        if token in ("-p", "--package") and i + 1 < len(selectors):
            package = selectors[i + 1]
            if package not in members:
                raise ValueError(f"-p names no workspace member: {package}")
            reached.add(package)
    return reached


def covered(workflow_text, members):
    """The workspace members the Windows jobs run `cargo test` over."""
    reached = set()
    for body in windows_jobs(workflow_text).values():
        for _, command in test_steps(body):
            reached |= selected(command, members)
    return reached


def shares(workflow_text, members):
    """How many Windows shares claim each workspace member."""
    counts = collections.Counter()
    for body in windows_jobs(workflow_text).values():
        for step_name, command in test_steps(body):
            if step_name != SHARE_STEP:
                continue
            counts.update(selected(command, members))
    return counts


def problems(workflow_text, members):
    """Every way the Windows shares fail to be exactly the workspace.

    A selector naming no workspace member is one of those ways, so it comes
    back as a problem rather than as a traceback: a caller that asks what is
    wrong should get one answer shape for every kind of wrong. Nothing else
    is reported alongside it, because the counts are meaningless once a
    selector does not resolve.
    """
    found = []
    try:
        claimed = shares(workflow_text, members)
        reached = covered(workflow_text, members)
    except ValueError as error:
        return [str(error)]
    missed = sorted(members - set(reached))
    if missed:
        found.append(f"the Windows jobs run no tests for: {missed}")
    unshared = sorted(members - set(claimed) - set(missed))
    if unshared:
        found.append(f"no Windows share owns, so nothing budgets for: {unshared}")
    twice = sorted(name for name, count in claimed.items() if count > 1)
    if twice:
        found.append(f"more than one Windows share tests, so the runner pays twice for: {twice}")
    return found


def workspace_members():
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True,
        text=True,
        check=True,
    )
    return {package["name"] for package in json.loads(metadata.stdout)["packages"]}


def main():
    members = workspace_members()
    with open(WORKFLOW, encoding="utf-8") as handle:
        workflow_text = handle.read()
    found = problems(workflow_text, members)
    if found:
        for line in found:
            print(line, file=sys.stderr)
        return 1
    claimed = shares(workflow_text, members)
    share_count = sum(
        1
        for body in windows_jobs(workflow_text).values()
        for step_name, _ in test_steps(body)
        if step_name == SHARE_STEP
    )
    print(
        f"{len(claimed)} of {len(members)} workspace members tested on Windows, "
        f"once each, across {share_count} shares"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
