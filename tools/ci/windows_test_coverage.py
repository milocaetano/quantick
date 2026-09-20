#!/usr/bin/env python3
"""Assert that the Windows jobs still test every workspace member.

`cargo test --workspace` in one job needed no guard: cargo found the members,
so a crate added on Monday was tested on Monday. Splitting that run across
jobs — one carrying `--workspace --exclude <heavy crate>`, another carrying
`-p <heavy crate>` — keeps the property only as long as the two halves are
complements, and nothing in a workflow file checks that. A typo in an
`--exclude`, or a second heavy crate peeled off and its `-p` job forgotten,
would drop a crate's Windows tests silently and leave every run green.

So the halves are read back out of `ci.yml` and compared with the member list
`cargo metadata` reports. The check is over the Windows jobs specifically: the
Linux side still runs one `cargo test --workspace`.
"""

import json
import re
import subprocess
import sys

WORKFLOW = ".github/workflows/ci.yml"


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


def covered(workflow_text, members):
    """The workspace members the Windows jobs run `cargo test` over."""
    reached = set()
    for body in windows_jobs(workflow_text).values():
        for command in re.findall(r"run: (cargo test [^\n]*)", body):
            # Anything after a bare `--` is a test-binary argument, not a
            # package selector.
            selectors = command.split(" -- ")[0].split()
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
        reached = covered(handle.read(), members)
    missed = sorted(members - reached)
    if missed:
        print(
            f"the Windows jobs run no tests for: {missed}",
            file=sys.stderr,
        )
        return 1
    print(f"{len(reached)} of {len(members)} workspace members tested on Windows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
