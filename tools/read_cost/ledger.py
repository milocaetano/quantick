#!/usr/bin/env python3
"""The committed record of what each merged pull request cost to read.

One row per merged pull request, so the trend is in the repository rather
than in thirty expired artifact downloads. The ledger is a directory: each row
is its own ``<pr>.md``, so two pull requests that each record a row write
disjoint paths and never conflict. Its ``README.md`` carries the ceiling the
feature-branch warning is measured against, as a marker line, because a
number that lives in two places drifts and the first symptom is a warning
nobody can reproduce.

Rows are generated from the first-parent history of ``origin/main`` after
merging, never by feature pull requests. ``backfill`` only adds row files: it
never rewrites an existing one or the README with the manually chosen
ceiling; CI publishes the updated directory as an artifact, and a separate
maintenance PR can retain it in the repository.

The ceiling is not computed here on every run. It was chosen once from the
backfill — the median of the ``feat/`` and ``fix/`` rows, rounded to the
thousand — and ``median`` recomputes that suggestion on demand, for a person
to decide with. A ceiling that moves by itself is not a bound; it is a
description of whatever just happened.

Subcommands: ``backfill`` the last N main commits through the calculator,
``median`` to see what the rows suggest, and ``ceiling`` to print the bound.
"""

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_PATH = "docs/quality/read-cost/ledger.d"
CEILING_FILE = "README.md"
# No leading zero: one number has one name, so no row can be read twice.
ROW_FILE = re.compile(r"^([1-9]\d*)\.md$")
# The recorded ceiling, as data on its own line. A marker rather than prose so
# the hook reads the same bytes a person does and neither has to guess which
# sentence held the number.
CEILING_MARKER = re.compile(r"^<!-- read-cost-ceiling:v1 (\d+) -->$", re.MULTILINE)
ROW = re.compile(r"^\|\s*(\d+)\s*\|")
COLUMNS = ("pr", "date", "branch", "base", "read_cost", "changed", "top")
FEATURE_PREFIXES = ("feat/", "fix/")
HEADING = "| PR | Date | Branch | Base | Read cost | Changed | Top referenced |"
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


def ceiling_or_none(directory):
    """The recorded ceiling, or ``None`` when there is not one to read.

    Missing directory, unreadable README and missing marker all answer
    ``None``. The callers are advisory and must not turn their own absence
    into a finding.
    """
    try:
        found = CEILING_MARKER.search(read(os.path.join(directory, CEILING_FILE)))
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


def row_path(directory, pr):
    return os.path.join(directory, f"{int(pr)}.md")


def load_rows(directory):
    """Every recorded row, in pull-request order.

    Read from the row files alone, so prose in the README can never be
    mistaken for a row, and ordered by number rather than by name, so 999
    stays before 1000.
    """
    names = sorted(
        (int(found.group(1)), found.group(0))
        for found in map(ROW_FILE.match, os.listdir(directory))
        if found
    )
    rows = []
    for _, name in names:
        rows.extend(parse_rows(read(os.path.join(directory, name))))
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


def write_row(directory, row):
    """Write one pull request's row file, unless that file already exists.

    Idempotent on the pull-request number because a workflow re-run is an
    ordinary event and a ledger that double-counts a merge misreports the
    median that the ceiling comes from.
    """
    try:
        with open(row_path(directory, row["pr"]), "x", encoding="utf-8",
                  newline="\n") as stream:
            stream.write(f"{HEADING}\n{RULE}\n{format_row(row)}\n")
    except FileExistsError:
        return False
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


def gh_json(repo, *args):
    result = subprocess.run(
        ["gh", *args],
        cwd=repo,
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


def merged_pull_for_commit(repo, sha):
    """Only the main PR merged by this commit, not PRs containing it."""
    associated = gh_json(repo, "api", f"repos/{{owner}}/{{repo}}/commits/{sha}/pulls")
    for pull in associated:
        if (pull.get("merge_commit_sha") == sha and pull.get("merged_at")
                and pull["base"]["ref"] == "main"):
            return pull
    return None


def backfill(repo, directory, limit):
    # Pin the history before consulting GitHub. Unmerged branches and merges
    # into integration branches cannot add their own rows.
    commits = MEASURE.git(
        repo, "rev-list", "--first-parent", f"--max-count={limit}", "origin/main"
    ).splitlines()
    existing = {row["pr"] for row in load_rows(directory)}
    rows = []
    for sha in reversed(commits):
        pull = merged_pull_for_commit(repo, sha)
        if pull is None or pull["number"] in existing:
            continue
        number = pull["number"]
        for side in ("base", "head"):
            if not fetch_pull(repo, number, pull[side]["sha"]):
                raise MEASURE.ReadCostError(f"#{number}: {side} commit unreachable")
        report = MEASURE.measure(repo, pull["base"]["sha"], pull["head"]["sha"])
        rows.append(row_from_report(
            report, number, pull["head"]["ref"], "main", pull["merged_at"][:10]
        ))
        existing.add(number)
        print(f"#{number}: {report['production_lines']} lines measured", file=sys.stderr)
    # A failed lookup or measurement leaves the ledger untouched.
    return sum(write_row(directory, row) for row in rows)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", default=DEFAULT_PATH, help="the ledger directory")
    modes = parser.add_subparsers(dest="mode", required=True)

    fill = modes.add_parser("backfill", help="record merged PRs from the last N origin/main commits")
    fill.add_argument("--repo", required=True, help="path to the Git repository")
    fill.add_argument("--limit", type=int, default=30)

    modes.add_parser("median", help="what the rows now suggest as a ceiling")
    modes.add_parser("ceiling", help="print the recorded ceiling")

    args = parser.parse_args(argv)
    if args.mode == "backfill":
        if args.limit < 1:
            parser.error("--limit must be positive")
        try:
            print(f"{backfill(args.repo, args.ledger, args.limit)} row file(s) written")
        except (OSError, subprocess.CalledProcessError, json.JSONDecodeError,
                MEASURE.ReadCostError) as problem:
            print(f"error: {problem}", file=sys.stderr)
            return 1
        return 0
    if args.mode == "median":
        try:
            suggestion = suggested_ceiling(load_rows(args.ledger))
        except OSError as problem:
            parser.exit(2, f"error: cannot read the ledger: {problem}\n")
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
