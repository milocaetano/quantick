#!/usr/bin/env python3
"""The committed record of what each merged pull request cost to read.

One row per merged pull request, so the trend is in the repository rather
than in thirty expired artifact downloads. The same file carries the ceiling
the feature-branch warning is measured against, as a marker line, because a
number that lives in two places drifts and the first symptom is a warning
nobody can reproduce.

A pull request carries its own row: ``record --pr <n>`` appends it to the
branch, in the diff, before the merge. The alternative was a workflow pushing
straight to ``main``, which this repository's ruleset does not allow a bot to
do and would not allow without a long-lived write credential in CI. The push
side is therefore a check rather than a writer — ``verify`` says whether the
pull request a merge commit closed left a row behind.

The ceiling is not computed here on every run. It was chosen once from the
backfill — the median of the ``feat/`` and ``fix/`` rows, rounded to the
thousand — and ``median`` recomputes that suggestion on demand, for a person
to decide with. A ceiling that moves by itself is not a bound; it is a
description of whatever just happened.

Subcommands: ``record`` this pull request's row, ``verify`` that a merge
commit's pull request left one, ``backfill`` the last N merged pull requests
through the calculator, ``median`` to see what the rows now suggest, and
``ceiling`` to print the recorded one.
"""

import argparse
import datetime
import importlib.util
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_PATH = "docs/quality/read-cost/ledger.md"
# The recorded ceiling, as data on its own line. A marker rather than prose so
# the hook reads the same bytes a person does and neither has to guess which
# sentence held the number.
CEILING_MARKER = re.compile(r"^<!-- read-cost-ceiling:v1 (\d+) -->$", re.MULTILINE)
ROW = re.compile(r"^\|\s*(\d+)\s*\|")
COLUMNS = ("pr", "date", "branch", "base", "read_cost", "changed", "top")
FEATURE_PREFIXES = ("feat/", "fix/")
HEADING = "| PR | Date | Branch | Base | Read cost | Changed | Top referenced |"
# What `record` did. "Already recorded" is a success: the command is
# idempotent on the pull-request number, so a second run is not a failure.
APPENDED = "appended"
ALREADY_RECORDED = "already recorded"
FAILED = "failed"
RULE = "| ---: | --- | --- | --- | ---: | ---: | --- |"


def load(name, filename):
    path = os.path.join(HERE, filename)
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


MEASURE = load("quantick_read_cost_measure_for_ledger", "measure.py")
SHAPE = load("quantick_read_cost_shape", "shape.py")


def is_feature_branch(branch):
    return branch.startswith(FEATURE_PREFIXES)


def read(path):
    with open(path, encoding="utf-8") as stream:
        return stream.read()


def ceiling_or_none(path):
    """The recorded ceiling, or ``None`` when there is not one to read.

    Missing file, unreadable file and missing marker all answer ``None``. The
    callers are advisory and must not turn their own absence into a finding.
    """
    try:
        found = CEILING_MARKER.search(read(path))
    except OSError:
        return None
    return int(found.group(1)) if found else None


def parse_rows(text):
    rows = []
    for line in text.splitlines():
        if not ROW.match(line):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) != len(COLUMNS):
            continue
        row = dict(zip(COLUMNS, cells))
        row["pr"] = int(row["pr"])
        row["read_cost"] = int(row["read_cost"])
        row["changed"] = int(row["changed"])
        rows.append(row)
    return rows


def format_row(row):
    # Rendered from COLUMNS, in COLUMNS order, so the order is written
    # once: `parse_rows` only checks how many cells a line has, so a row
    # written in a different order than it is read parses into shifted
    # fields rather than failing.
    cells = [str(row[column]) for column in COLUMNS]
    cells[COLUMNS.index("top")] = row["top"] or "—"
    return "| " + " | ".join(cells) + " |"


def row_from_report(report, pr, branch, base_ref, date):
    top = SHAPE.top_referenced(report, 1)
    return {
        "pr": int(pr),
        "date": date,
        "branch": branch,
        "base": base_ref,
        "read_cost": report["production_lines"],
        "changed": len(SHAPE.touched_files(report)),
        "top": f"`{top[0]['path']}` ({top[0]['lines']})" if top else "",
    }


def append_row(path, row):
    """Append one row, unless that pull request already has one.

    Idempotent on the pull-request number because a workflow re-run is an
    ordinary event and a ledger that double-counts a merge misreports the
    median that the ceiling comes from.
    """
    text = read(path)
    if any(existing["pr"] == row["pr"] for existing in parse_rows(text)):
        return False
    if not text.endswith("\n"):
        text += "\n"
    with open(path, "w", encoding="utf-8", newline="\n") as stream:
        stream.write(text + format_row(row) + "\n")
    return True


def median(values):
    ordered = sorted(values)
    if not ordered:
        return None
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return float(ordered[middle])
    return (ordered[middle - 1] + ordered[middle]) / 2


def round_to_thousand(value):
    # Half up, so a median of 8,500 rounds to 9,000. Python's round() is half
    # to even, which would answer 8,000 here and 9,000 for 9,500 — the same
    # input shape producing two different roundings is not something a reader
    # of the ledger should have to know about.
    return int((value + 500) // 1000) * 1000


def suggested_ceiling(rows):
    feature = [row["read_cost"] for row in rows if is_feature_branch(row["branch"])]
    middle = median(feature)
    if middle is None:
        return None
    return {
        "samples": len(feature),
        "median": middle,
        "ceiling": round_to_thousand(middle),
    }


def gh_json(*args):
    result = subprocess.run(
        ["gh", *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return json.loads(result.stdout.decode("utf-8"))


def have_commit(repo, sha):
    return (
        subprocess.run(
            ["git", "-C", repo, "cat-file", "-e", f"{sha}^{{commit}}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ).returncode
        == 0
    )


def fetch_pull(repo, number, sha):
    """Make a merged pull request's own objects reachable locally.

    A merged branch is usually deleted, so its head commit can be absent from
    a fresh clone even though GitHub still serves it under ``refs/pull``.
    """
    if have_commit(repo, sha):
        return True
    subprocess.run(
        ["git", "-C", repo, "fetch", "--quiet", "origin", f"pull/{number}/head"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return have_commit(repo, sha)


def pull_facts(number):
    """Base and head as the pull request itself recorded them.

    Not the base branch's current tip: once the branch has merged, the merge
    base of today's main and that head is the head itself, and the change
    measures as empty.
    """
    data = gh_json("api", f"repos/{{owner}}/{{repo}}/pulls/{number}")
    return {
        "pr": data["number"],
        "branch": data["head"]["ref"],
        "base": data["base"]["ref"],
        "merged": (data["merged_at"] or "")[:10],
        "base_sha": data["base"]["sha"],
        "head_sha": data["head"]["sha"],
    }


def merged_pull_for_commit(sha, fetch=None):
    """The pull request this commit *is the merge of*, or ``None``.

    ``fetch`` is the seam the tests replace. GitHub associates a commit with
    every pull request that contains it, so the association is filtered down
    to the one whose merge commit this is: a push that is not a merge, and a
    merge of a branch that was never a pull request, both answer ``None`` and
    the ledger gains no row.
    """
    fetch = fetch or gh_json
    try:
        associated = fetch("api", f"repos/{{owner}}/{{repo}}/commits/{sha}/pulls")
    except (subprocess.CalledProcessError, json.JSONDecodeError):
        return None
    for pull in associated:
        if pull.get("merge_commit_sha") == sha and pull.get("merged_at"):
            return pull["number"]
    return None


def row_for(path, pr):
    """The recorded row for a pull request, or ``None``."""
    try:
        rows = parse_rows(read(path))
    except OSError:
        return None
    for row in rows:
        if row["pr"] == int(pr):
            return row
    return None


def today():
    return datetime.datetime.now(datetime.timezone.utc).date().isoformat()


def record(repo, path, number):
    """Append the row for pull request `number`, measured as GitHub sees it.

    Answers which of the three things happened, because this is the one call
    here that is an action rather than advice, and a caller that cannot tell
    "recorded" from "could not record" commits nothing and finds out at the
    merge. Works before the merge as well as after it, and measures the same
    two objects either way: the base the pull request was opened against and
    its current head. A row added on the branch and a row added from the merge
    commit are therefore the same row.
    """
    facts = pull_facts(number)
    for key in ("base_sha", "head_sha"):
        if not fetch_pull(repo, number, facts[key]):
            print(f"#{number}: objects unreachable, no row", file=sys.stderr)
            return FAILED
    try:
        report = MEASURE.measure(repo, facts["base_sha"], facts["head_sha"])
    except MEASURE.ReadCostError as problem:
        print(f"#{number}: {problem}", file=sys.stderr)
        return FAILED
    row = row_from_report(
        report,
        number,
        facts["branch"],
        facts["base"],
        facts["merged"] or today(),
    )
    outcome = APPENDED if append_row(path, row) else ALREADY_RECORDED
    print(f"#{number}: {row['read_cost']} lines, {outcome}", file=sys.stderr)
    return outcome


def verify(path, sha):
    """Whether the pull request this commit merged left a row behind.

    ``None`` when the commit merged no pull request, which is not a finding:
    an ordinary push has no row to carry.
    """
    number = merged_pull_for_commit(sha)
    if number is None:
        return None
    return row_for(path, number) is not None


def backfill(repo, path, limit):
    merged = gh_json(
        "pr", "list", "--state", "merged", "--limit", str(limit), "--json", "number"
    )
    numbers = sorted(entry["number"] for entry in merged)
    written = 0
    for number in numbers:
        facts = pull_facts(number)
        if not facts["merged"]:
            print(f"#{number}: not merged, skipped", file=sys.stderr)
            continue
        reachable = all(
            fetch_pull(repo, number, facts[key]) for key in ("base_sha", "head_sha")
        )
        if not reachable:
            print(f"#{number}: objects unreachable, skipped", file=sys.stderr)
            continue
        try:
            report = MEASURE.measure(repo, facts["base_sha"], facts["head_sha"])
        except MEASURE.ReadCostError as problem:
            print(f"#{number}: {problem}", file=sys.stderr)
            continue
        row = row_from_report(
            report, number, facts["branch"], facts["base"], facts["merged"]
        )
        if append_row(path, row):
            written += 1
        print(f"#{number}: {row['read_cost']} lines", file=sys.stderr)
    return written


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", default=DEFAULT_PATH, help="the ledger file")
    modes = parser.add_subparsers(dest="mode", required=True)

    fill = modes.add_parser("backfill", help="measure the last N merged PRs")
    fill.add_argument("--repo", required=True, help="path to the Git repository")
    fill.add_argument("--limit", type=int, default=30)

    keep = modes.add_parser("record", help="append this pull request's row")
    keep.add_argument("--repo", required=True, help="path to the Git repository")
    keep.add_argument("--pr", required=True, type=int, help="the pull request")

    check = modes.add_parser("verify", help="did the merged pull request leave a row")
    check.add_argument("--commit", required=True, help="the pushed commit")

    modes.add_parser("median", help="what the rows now suggest as a ceiling")
    modes.add_parser("ceiling", help="print the recorded ceiling")

    args = parser.parse_args(argv)
    if args.mode == "backfill":
        print(f"{backfill(args.repo, args.ledger, args.limit)} row(s) appended")
        return 0
    if args.mode == "record":
        # The one non-zero exit here. A caller that is told nothing happened
        # can stop and look; a caller told nothing at all commits an empty
        # change and finds out at the merge.
        return 0 if record(args.repo, args.ledger, args.pr) != FAILED else 1
    if args.mode == "verify":
        # Also always 0. This runs on `main`, after the merge, where the row
        # can no longer be added by the pull request that owed it: reddening
        # the default branch over a missing line of Markdown would be a worse
        # trade than an annotation nobody has to act on today.
        carried = verify(args.ledger, args.commit)
        if carried is None:
            print("no merged pull request, nothing to verify")
        elif carried:
            print("the merged pull request left its row")
        else:
            print("::warning::the merged pull request left no read-cost row")
        return 0
    if args.mode == "median":
        suggestion = suggested_ceiling(parse_rows(read(args.ledger)))
        if suggestion is None:
            parser.exit(2, "error: the ledger holds no feature rows\n")
        print(
            f"samples {suggestion['samples']} "
            f"median {suggestion['median']:.1f} "
            f"ceiling {suggestion['ceiling']}"
        )
        return 0
    recorded = ceiling_or_none(args.ledger)
    if recorded is None:
        parser.exit(2, "error: the ledger records no ceiling\n")
    print(recorded)
    return 0


if __name__ == "__main__":
    sys.exit(main())
