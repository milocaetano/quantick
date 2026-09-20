#!/usr/bin/env python3
"""Turn a read-cost measurement into something a person acts on.

``measure.py`` answers how many production lines a pull request asks a reader
to hold. It answers in canonical JSON, uploaded as a CI artifact, which is
evidence nobody downloads. This module renders that JSON twice:

``comment``
    The sticky pull-request comment, keyed by :data:`MARKER` so every build
    updates one comment rather than stacking a new one per push.
``warn``
    What ``pr-gate`` prints, as a JSON string ready to drop into a hook
    payload: that a feature branch is over the ledger's ceiling, that the
    pull request has not recorded its row yet, or nothing at all.
``marker``
    The comment's key, for the workflow that has to find last build's comment
    before it can update it in place.

Both name the same thing: the referenced files, largest first. A referenced
file is one the diff never touched and a reader still has to open, so it is
the only part of the number an author can lower by detaching something.
"""

import argparse
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MARKER = "<!-- quantick-read-cost:v1 -->"
# How a pull request adds the row it owes. Written once, here, because the
# comment and the hook both have to say it and a reader who is told two
# different commands tries neither.
ROW_COMMAND = "python tools/read_cost/ledger.py record --repo . --pr"


def load(name, filename):
    path = os.path.join(HERE, filename)
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


LEDGER = load("quantick_read_cost_ledger", "ledger.py")
# The measurement's shape, through the ledger's own load: one module, so the
# "referenced but not touched" rule cannot exist twice.
SHAPE = LEDGER.SHAPE
# The calculator, through the ledger: one load, so the hook and the comment
# cannot end up measuring with two copies of the same module.
MEASURE = LEDGER.MEASURE


def comment(report, ceiling=None, row_missing_for=None):
    """The sticky comment body, marker first."""
    touched = SHAPE.touched_files(report)
    referenced = SHAPE.referenced_files(report)
    top = referenced[: SHAPE.TOP_REFERENCED]
    lines = [
        MARKER,
        f"### Read cost: {report['production_lines']} production lines",
        "",
        f"{len(touched)} changed file(s) pull in {len(referenced)} referenced "
        "file(s). The number is the production lines of both sets together — "
        "what this change asks a reader to hold.",
        "",
    ]
    if ceiling is not None:
        verdict = "over" if report["production_lines"] > ceiling else "within"
        lines += [
            f"Feature-branch ceiling: {ceiling} — this change is {verdict} it. "
            "The ceiling never blocks; it is the median of the feature pull "
            "requests in `docs/quality/read-cost/ledger.md`.",
            "",
        ]
    if top:
        lines += [
            f"Top {len(top)} referenced file(s) by lines — the reading this "
            "change inherits rather than writes:",
            "",
            "| Referenced file | Lines |",
            "| --- | ---: |",
        ]
        lines += [f"| `{entry['path']}` | {entry['lines']} |" for entry in top]
    else:
        lines.append("No file is pulled in by reference alone.")
    if row_missing_for is not None:
        lines += [
            "",
            "This pull request has no row in "
            "`docs/quality/read-cost/ledger.md` yet. Add it on the branch, so "
            "the merge carries its own measurement:",
            "",
            f"```sh\n{ROW_COMMAND} {row_missing_for}\n```",
        ]
    lines += [
        "",
        f"<sub>base `{report['base'][:12]}` head `{report['head'][:12]}` "
        "direct references only, measured by `tools/read_cost/measure.py`; "
        "`tools/read_cost/report.py` renders it.</sub>",
    ]
    return "\n".join(lines) + "\n"


def ceiling_warning(report, ceiling, branch):
    """The advisory text for a branch over the ceiling, or ``None``.

    ``None`` is the answer for every case that is not a feature branch over
    the ceiling, because this text only ever reaches a reader as a warning.
    """
    if not LEDGER.is_feature_branch(branch):
        return None
    if ceiling is None or report["production_lines"] <= ceiling:
        return None
    top = SHAPE.top_referenced(report)
    detail = "".join(
        f"\n  {entry['lines']:>6}  {entry['path']}" for entry in top
    ) or "\n  (nothing is pulled in by reference alone)"
    return (
        f"Read cost: this branch asks a reader to hold "
        f"{report['production_lines']} production lines, over the "
        f"{ceiling} ceiling for feature branches in "
        "docs/quality/read-cost/ledger.md. Nothing is blocked. The largest "
        "files it pulls in by reference alone, which is what detaching "
        f"something would remove:{detail}"
    )


def row_reminder(number):
    """What to say to a pull request that has not recorded its row."""
    return (
        f"Read cost: pull request #{number} has no row in "
        "docs/quality/read-cost/ledger.md. The row travels with the pull "
        "request, so add it on the branch before the merge:\n\n  "
        f"{ROW_COMMAND} {number}"
    )


def advisory(notes):
    """One payload for the hook, or ``None`` when there is nothing to say."""
    kept = [note for note in notes if note]
    return "\n\n".join(kept) if kept else None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)

    body = modes.add_parser("comment", help="render the sticky PR comment")
    body.add_argument("--json", required=True, help="a measure.py report")
    body.add_argument("--pr", type=int, help="the pull request this measures")
    body.add_argument("--ledger", help="ledger to read the ceiling from")

    warn = modes.add_parser("warn", help="the pr-gate advisory, as a JSON string")
    warn.add_argument("--repo", required=True, help="path to the Git repository")
    warn.add_argument("--base", required=True, help="base revision")
    warn.add_argument("--head", required=True, help="head revision")
    warn.add_argument("--branch", required=True, help="the branch being shipped")
    warn.add_argument("--pr", type=int, help="the pull request, when one is named")
    warn.add_argument("--ledger", help="ledger to read the ceiling from")

    modes.add_parser("marker", help="print the sticky comment's key")

    args = parser.parse_args(argv)
    if args.mode == "marker":
        # The one caller is the workflow that updates the comment in place. It
        # asks rather than restating the string, because a second copy of the
        # key drifts silently: the search stops matching, and every build
        # posts a new comment instead of updating the one before it.
        print(MARKER)
        return 0
    ledger_path = args.ledger
    if args.mode == "comment":
        with open(args.json, encoding="utf-8") as stream:
            report = json.load(stream)
        if ledger_path is None:
            ledger_path = LEDGER.DEFAULT_PATH
        missing = None
        if args.pr is not None and LEDGER.row_for(ledger_path, args.pr) is None:
            missing = args.pr
        sys.stdout.write(
            comment(report, LEDGER.ceiling_or_none(ledger_path), missing)
        )
        return 0

    # Every failure here is silence. The advisory is advice, `pr-gate` fails
    # open by design, and a hook that reports its own breakage in the place a
    # read-cost finding belongs teaches its reader to skip both.
    if ledger_path is None:
        ledger_path = os.path.join(args.repo, LEDGER.DEFAULT_PATH)
    ceiling = LEDGER.ceiling_or_none(ledger_path)
    over = None
    if ceiling is not None and LEDGER.is_feature_branch(args.branch):
        try:
            over = ceiling_warning(
                MEASURE.measure(args.repo, args.base, args.head),
                ceiling,
                args.branch,
            )
        except MEASURE.ReadCostError:
            over = None
    # The row is owed by every pull request, not only by the feature branches
    # the ceiling is about, and asking for it costs no measurement.
    missing = None
    if args.pr is not None and LEDGER.row_for(ledger_path, args.pr) is None:
        missing = row_reminder(args.pr)
    text = advisory([over, missing])
    if text is not None:
        sys.stdout.write(json.dumps(text) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
