#!/usr/bin/env python3
"""Report what a mission cost, from the three sources that already exist.

Implements ``docs/quality/mission-cost/method.md``. That document is the
specification; where this program disagrees with it, this program is the bug.

Two subcommands:

``report``
    One canonical-JSON document: per-mission tokens by kind, main thread and
    subagents apart, the three wall-clock quantities, the delivery timings, and
    the two buckets of cost the attribution could not place.

``compare``
    One metric across the registry's ``before`` and ``after`` groups, graded by
    the registered reduction rule.

Transcripts live on the trader's machine and are never committed; see
``README.md``. Run ``python tools/mission_cost/measure.py --help``.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
VERSION = 1
METHOD = "docs/quality/mission-cost/method.md"
DEFAULT_REGISTRY = os.path.join("docs", "quality", "mission-cost", "missions.json")


def load(name):
    key = f"quantick_mission_cost_{name}"
    if key in sys.modules:
        return sys.modules[key]
    spec = importlib.util.spec_from_file_location(key, os.path.join(HERE, f"{name}.py"))
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {name}.py beside {__file__}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module


TRANSCRIPTS = load("transcripts")
ATTRIBUTION = load("attribution")
DISPERSION = load("dispersion")
DELIVERY = load("delivery")

WALL_CLOCK = ("agent_seconds", "elapsed_seconds", "span_seconds")
DELIVERY_METRICS = ("pr_open_seconds", "ci_seconds", "ci_wall_seconds", "ci_runs")


def default_transcripts(repo):
    """Where this host keeps the transcripts of sessions rooted at ``repo``.

    The directory name is the absolute repository path with every character
    outside ``[A-Za-z0-9]`` replaced by a dash, which is how the projects
    directory is laid out.
    """
    slug = re.sub(r"[^A-Za-z0-9]", "-", os.path.abspath(repo))
    return os.path.join(os.path.expanduser("~"), ".claude", "projects", slug)


def collect(roots):
    """Discover every transcript under every root, in one stable order.

    Several roots because a session started in a worktree lands in its own
    projects directory: `C--src-quantick` and
    `C--src-quantick-worktrees-feat-something` are different places and the same
    mission can have sessions in both.
    """
    found, skipped = [], []
    for root in roots:
        items, missed = TRANSCRIPTS.discover(root)
        name = os.path.basename(os.path.abspath(root))
        found.extend(items)
        skipped.extend(f"{name}/{one}" for one in missed)
    found.sort(key=lambda item: (item.root, item.relative))
    return found, sorted(skipped)


def digest(found):
    """A stable fingerprint of the input set: paths, counts and the four sums."""
    lines = []
    for item in found:
        totals = item.totals()
        lines.append(
            "|".join(
                [f"{item.root}/{item.relative}", str(totals["requests"])]
                + [str(totals[name]) for name in TRANSCRIPTS.COUNTERS]
            )
        )
    text = "\n".join(sorted(lines))
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


class Pulls:
    """One `gh pr view` per pull request, however many callers want it.

    The window resolution and the delivery block both need the same facts, and
    a campaign registry is dozens of missions long: asking twice each turns one
    report into a hundred needless round trips.
    """

    def __init__(self, use_gh, as_of):
        self.use_gh = use_gh
        self.as_of = as_of
        self._facts = {}

    def facts(self, pr):
        if not self.use_gh or not pr:
            return None
        if pr not in self._facts:
            self._facts[pr] = DELIVERY.pull_facts(pr, as_of=self.as_of)
        return self._facts[pr]


def resolve_windows(missions, pulls):
    """Fill in the default window of any mission the registry left open.

    The registry is the authority; this only supplies what it left ``null``.
    """
    resolved = []
    notes = []
    for mission in missions:
        started, ended = mission.started_at, mission.ended_at
        if pulls.use_gh and mission.pr and (started is None or ended is None):
            try:
                facts = pulls.facts(mission.pr)
                if ended is None and facts["ended_at"]:
                    ended = TRANSCRIPTS.parse_timestamp(facts["ended_at"])
                if started is None:
                    first = DELIVERY.first_commit_instant(mission.pr)
                    started = TRANSCRIPTS.parse_timestamp(
                        first or facts["created_at"] or ""
                    )
            except DELIVERY.DeliveryError as problem:
                notes.append(f"{mission.branch}: {problem}")
        resolved.append(mission._replace(started_at=started, ended_at=ended))
        if started is None and ended is None:
            notes.append(
                f"{mission.branch}: no window, so only declared sessions reach it"
            )
    return resolved, notes


def delivery_for(mission, repo, pulls):
    found = pulls.facts(mission.pr)
    if found is None:
        return None
    facts = dict(found)
    facts.update(DELIVERY.ci_facts(mission.branch))
    facts["read_cost"] = DELIVERY.read_cost_row(
        os.path.join(repo, DELIVERY.LEDGER), mission.pr
    )
    return facts


def build_report(roots, registry_path, repo, use_gh, as_of):
    found, skipped = collect(roots)
    sessions = ATTRIBUTION.sessions_from(found)
    missions = ATTRIBUTION.load_registry(registry_path)
    pulls = Pulls(use_gh, as_of)
    missions, notes = resolve_windows(missions, pulls)
    placement = ATTRIBUTION.assign(sessions, missions)
    totals = ATTRIBUTION.totals_by_mission(sessions, missions, placement)

    reported = []
    for mission in missions:
        entry = dict(totals[mission.branch])
        entry.update(
            {
                "branch": mission.branch,
                "pr": mission.pr,
                "group": mission.group,
                "note": mission.note,
                "window": {
                    "started_at": mission.started_at.isoformat()
                    if mission.started_at
                    else None,
                    "ended_at": mission.ended_at.isoformat()
                    if mission.ended_at
                    else None,
                },
                "sessions": [
                    {"session": uuid, "method": placement.assigned[uuid][1]}
                    for uuid in placement.sessions_of(mission.branch)
                ],
                "delivery": delivery_for(mission, repo, pulls),
            }
        )
        reported.append(entry)

    shared = dict(totals[ATTRIBUTION.SHARED])
    shared["sessions"] = [
        dict(ATTRIBUTION.session_totals(sessions[uuid]), candidates=candidates)
        for uuid, candidates in placement.shared.items()
    ]
    unassigned = dict(totals[ATTRIBUTION.UNASSIGNED])
    unassigned["sessions"] = [
        ATTRIBUTION.session_totals(sessions[uuid]) for uuid in placement.unassigned
    ]

    measured = sum(item.totals()["billable_tokens"] for item in found)
    unplaced = (
        shared["total"]["billable_tokens"] + unassigned["total"]["billable_tokens"]
    )
    return {
        "schema": VERSION,
        "method": METHOD,
        "as_of": as_of,
        "thresholds": {
            "idle_gap_seconds": TRANSCRIPTS.IDLE_GAP_SECONDS,
            "min_group_n": DISPERSION.MIN_GROUP_N,
            "min_relative_change": DISPERSION.MIN_RELATIVE_CHANGE,
            "max_unplaced_share": DISPERSION.MAX_UNPLACED_SHARE,
        },
        "inputs": {
            "roots": sorted({os.path.basename(os.path.abspath(one)) for one in roots}),
            "transcripts": len(found),
            "sessions": len(sessions),
            "skipped": skipped,
            "unparsed_lines": sum(item.unparsed for item in found),
            "undated_usage_lines": sum(item.undated for item in found),
            "digest": digest(found),
        },
        "notes": notes,
        "missions": reported,
        "unplaced": {
            "shared": shared,
            "unassigned": unassigned,
            "billable_tokens": unplaced,
            "measured_billable_tokens": measured,
            "share_of_billable_tokens": (unplaced / measured) if measured else 0.0,
        },
    }


def metric_value(entry, metric):
    """Read one registered metric off a mission's report entry."""
    if "." in metric:
        section, field = metric.split(".", 1)
        return entry.get(section, {}).get(field)
    if metric in WALL_CLOCK:
        return entry.get(metric)
    if metric in DELIVERY_METRICS:
        return (entry.get("delivery") or {}).get(metric)
    return entry["total"].get(metric)


def _group_window(entries):
    starts = [
        entry["window"]["started_at"] for entry in entries if entry["window"]["started_at"]
    ]
    ends = [
        entry["window"]["ended_at"] for entry in entries if entry["window"]["ended_at"]
    ]
    return (min(starts) if starts else None, max(ends) if ends else None)


def _touches(session, window):
    start, end = window
    if start is None or end is None:
        return False
    first, last = session.get("first_timestamp_seen"), session.get("last_timestamp_seen")
    if first is None or last is None:
        return False
    return not (last < start or first > end)


def _unplaced_share(report, entries):
    """How much of a group's measured cost the attribution could not place.

    Session by session, not bucket by bucket: one stray session brushing the
    window must not charge the group every unplaced token in the directory.
    """
    window = _group_window(entries)
    placed = sum(entry["total"]["billable_tokens"] for entry in entries)
    unplaced = 0
    for bucket in ("shared", "unassigned"):
        for session in report["unplaced"][bucket]["sessions"]:
            if _touches(session, window):
                unplaced += session["total"]["billable_tokens"]
    measured = placed + unplaced
    return (unplaced / measured) if measured else 0.0


def _overlap(first, second):
    if None in first or None in second:
        return False
    return not (first[1] < second[0] or first[0] > second[1])


def build_comparison(report, metric):
    groups = {"before": [], "after": []}
    for entry in report["missions"]:
        if entry["group"] in groups:
            groups[entry["group"]].append(entry)
    values = {
        name: [
            metric_value(entry, metric)
            for entry in entries
            if metric_value(entry, metric) is not None
        ]
        for name, entries in groups.items()
    }
    windows = {name: _group_window(entries) for name, entries in groups.items()}
    overlap = _overlap(windows["before"], windows["after"])
    return {
        "schema": VERSION,
        "method": METHOD,
        "metric": metric,
        "as_of": report["as_of"],
        "inputs": report["inputs"],
        "windows": {
            name: {"started_at": window[0], "ended_at": window[1]}
            for name, window in windows.items()
        },
        "windows_overlap": overlap,
        "missions": {
            name: [entry["branch"] for entry in entries]
            for name, entries in groups.items()
        },
        "result": DISPERSION.compare(
            values["before"],
            values["after"],
            unplaced_share_before=_unplaced_share(report, groups["before"]),
            unplaced_share_after=_unplaced_share(report, groups["after"]),
            windows_overlap=overlap,
        ),
    }


def render(document):
    """Canonical JSON: sorted keys, ASCII, one trailing newline."""
    return json.dumps(document, sort_keys=True, indent=2, ensure_ascii=True) + "\n"


def add_common(parser):
    parser.add_argument("--repo", default=".", help="the repository to measure from")
    parser.add_argument(
        "--transcripts",
        action="append",
        default=None,
        metavar="DIR",
        help=(
            "a session transcript directory; repeatable, because a session "
            "started in a worktree has its own (default: this host's for --repo)"
        ),
    )
    parser.add_argument(
        "--registry", default=None, help=f"the mission registry (default: {DEFAULT_REGISTRY})"
    )
    parser.add_argument(
        "--as-of",
        default=None,
        help="bound an open pull request and an unfinished window at this instant",
    )
    parser.add_argument(
        "--no-gh",
        action="store_true",
        help="skip every gh and git lookup; transcripts and the registry only",
    )
    parser.add_argument("--out", default="-", help="where to write; - is stdout")


def emit(text, where):
    """Write the report with LF endings, whichever way it leaves.

    Through the text layer, Windows turns every newline into CRLF on the way to
    standard output while `--out` writes LF, so the same report would hash two
    ways depending on how it was captured. The bytes go out as bytes.
    """
    raw = text.encode("utf-8")
    if where == "-":
        stream = getattr(sys.stdout, "buffer", None)
        if stream is None:
            sys.stdout.write(text)
            return
        stream.write(raw)
        stream.flush()
        return
    with open(where, "wb") as stream:
        stream.write(raw)


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Measure what a mission cost, per docs/quality/mission-cost/method.md"
    )
    modes = parser.add_subparsers(dest="mode", required=True)
    add_common(modes.add_parser("report", help="tokens and wall clock per mission"))
    compare = modes.add_parser("compare", help="one metric across before and after")
    add_common(compare)
    compare.add_argument(
        "--metric",
        default="billable_tokens",
        help=(
            "a token kind, billable_tokens, main_thread.<kind>, subagents.<kind>, "
            "agent_seconds, elapsed_seconds, span_seconds, pr_open_seconds, "
            "ci_seconds, ci_wall_seconds or ci_runs"
        ),
    )
    options = parser.parse_args(argv)

    roots = options.transcripts or [default_transcripts(options.repo)]
    for root in roots:
        if not os.path.isdir(root):
            parser.error(
                f"no transcript directory at {root}. Transcripts are local to "
                "the trader's machine and are not in the repository; see "
                "tools/mission_cost/README.md"
            )
    registry = options.registry or os.path.join(options.repo, DEFAULT_REGISTRY)
    try:
        report = build_report(
            roots, registry, options.repo, not options.no_gh, options.as_of
        )
        if options.mode == "compare":
            report = build_comparison(report, options.metric)
    except (ATTRIBUTION.RegistryError, DELIVERY.DeliveryError) as problem:
        parser.error(str(problem))
    emit(render(report), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
