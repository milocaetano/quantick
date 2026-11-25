#!/usr/bin/env python3
"""Whether a pull request's diff is prose that no Rust job can judge.

    python tools/ci/prose_only.py --base <rev> [--head <rev>]

Prints `prose` or `build` and explains the answer on stderr. The CI `gate`
job turns `prose` into skipped Rust jobs; see .claude/hooks/README.md.

A diff is prose when every changed path is Markdown or under `docs/`, none of
them is under a code root (PROSE_NEVER), and `affected_crates.select` reaches
no crate but the ones the gate tests itself (GATE_CRATES). That is the same
reading the draft `fast` job uses: a doc a test opens with `include_str!` or by
path is named in that crate's files, so the crate is reached and the diff
builds. A path assembled from pieces no file spells out is invisible to it,
which is why the gate also runs the generated-file drift tests on every event.
An empty diff builds: there is nothing to call prose.
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import affected_crates  # noqa: E402  (sibling script, not a package)

# Never prose, whatever the extension: these roots hold code, fixtures, the
# workflow itself and generated contracts.
PROSE_NEVER = ("crates/", "tools/", "bridge/", "schemas/", "assets/", ".github/", ".cargo/")

# Crates the gate job tests on every event, so reaching them needs no build.
GATE_CRATES = {"quantick-guards"}


def is_prose(path):
    return not path.startswith(PROSE_NEVER) and (path.startswith("docs/") or path.endswith(".md"))


def decide(changed, packages, referrers):
    """("prose" | "build", [reason, ...])."""
    if not changed:
        return "build", ["the diff is empty"]
    code = [path for path in changed if not is_prose(path)]
    if code:
        return "build", [f"{path} is not prose" for path in code]
    reached = affected_crates.select(changed, packages, referrers)
    reasons = [f"{name}: {why}" for name, why in sorted(reached.items()) if name not in GATE_CRATES]
    if reasons:
        return "build", reasons
    return "prose", [f"{path} is prose no crate reads" for path in changed]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--base", required=True, help="the PR's base commit")
    parser.add_argument("--head", default="HEAD")
    args = parser.parse_args(argv)

    root, packages = affected_crates.metadata_workspace()
    changed = affected_crates.changed_paths(root, args.base, args.head)
    verdict, reasons = decide(changed, packages, affected_crates.git_referrers(root, args.head))
    for reason in reasons:
        print(reason, file=sys.stderr)
    print(verdict)


if __name__ == "__main__":
    main()
