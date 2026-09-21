#!/usr/bin/env python3
"""Place each session on a mission, and split its cost main thread from subagents.

Sections 1, 2 and 6 of ``docs/quality/mission-cost/method.md``. A mission is one
branch and one pull request; a session is the unit of assignment and is never
split; a session that two missions could claim goes to the shared bucket with
both names on it, because there is no fair way to divide it from timestamps
alone.

The assignment reads timestamps and the file layout. It does not read ``cwd`` or
``gitBranch``, which are in the transcripts and would make it exact -- error
mode E2 in the method says why, and what that costs.
"""

import collections
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def _bootstrap():
    """Load `transcripts.py` beside this file, once, under a stable key.

    Repeated in each module that needs it and nowhere else: the shared loader
    lives in `transcripts.load`, and something has to load the module that
    holds it. Everything past this line goes through that one implementation.
    """
    key = "quantick_mission_cost_transcripts"
    if key in sys.modules:
        return sys.modules[key]
    spec = importlib.util.spec_from_file_location(
        key, os.path.join(HERE, "transcripts.py")
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load transcripts.py beside {__file__}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module


TRANSCRIPTS = _bootstrap()

SCHEMA = 1
GROUPS = (None, "before", "after")

SHARED = "<shared>"
UNASSIGNED = "<unassigned>"


class RegistryError(RuntimeError):
    """The mission registry could not be read as the method describes it."""


Mission = collections.namedtuple(
    "Mission", ("branch", "pr", "sessions", "started_at", "ended_at", "group", "note")
)


class Session:
    """One session UUID: its main transcript, if any, and its subagents."""

    def __init__(self, uuid):
        self.uuid = uuid
        self.main = []
        self.subagents = []

    @property
    def transcripts(self):
        return self.main + self.subagents

    def first(self):
        stamps = [item.first() for item in self.transcripts if item.first()]
        return min(stamps) if stamps else None

    def last(self):
        stamps = [item.last() for item in self.transcripts if item.last()]
        return max(stamps) if stamps else None


def sessions_from(found):
    """Group discovered transcripts by session, in discovery order."""
    sessions = collections.OrderedDict()
    for item in found:
        session = sessions.setdefault(item.session, Session(item.session))
        if item.kind == "main":
            session.main.append(item)
        else:
            session.subagents.append(item)
    return sessions


def _instant(entry, field):
    """One registry window end, through the package's one instant rule.

    The rule lives in `transcripts.require_instant`. The failure is re-raised as
    a `RegistryError` because here it is a fault in the registry document, which
    is what this module's callers already catch; the wording is unchanged.
    """
    value = entry.get(field)
    if value is None:
        return None
    try:
        return TRANSCRIPTS.require_instant(value, f"{entry.get('branch')}: {field}")
    except TRANSCRIPTS.InstantError as problem:
        raise RegistryError(str(problem)) from problem


def parse_registry(data):
    """Validate the registry document and return its missions, in file order."""
    if not isinstance(data, dict):
        raise RegistryError("the registry must be a JSON object")
    if data.get("schema") != SCHEMA:
        raise RegistryError(f"unknown registry schema: {data.get('schema')!r}")
    entries = data.get("missions")
    if not isinstance(entries, list):
        raise RegistryError("the registry needs a `missions` list")
    missions = []
    seen_branches = set()
    seen_sessions = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise RegistryError("every mission must be a JSON object")
        branch = entry.get("branch")
        if not isinstance(branch, str) or not branch:
            raise RegistryError("every mission needs a `branch`")
        if branch in seen_branches:
            raise RegistryError(f"{branch}: recorded twice")
        seen_branches.add(branch)
        group = entry.get("group")
        if group not in GROUPS:
            raise RegistryError(f"{branch}: unknown group {group!r}")
        sessions = entry.get("sessions") or []
        if not isinstance(sessions, list):
            raise RegistryError(f"{branch}: `sessions` must be a list")
        for uuid in sessions:
            if not isinstance(uuid, str) or not uuid:
                raise RegistryError(f"{branch}: a session id must be a string")
            if uuid in seen_sessions:
                raise RegistryError(
                    f"{uuid}: declared by both {seen_sessions[uuid]} and {branch}"
                )
            seen_sessions[uuid] = branch
        missions.append(
            Mission(
                branch=branch,
                pr=entry.get("pr"),
                sessions=tuple(sessions),
                started_at=_instant(entry, "started_at"),
                ended_at=_instant(entry, "ended_at"),
                group=group,
                note=entry.get("note"),
            )
        )
    return missions


def load_registry(path):
    try:
        with open(path, "r", encoding="utf-8") as stream:
            data = json.load(stream)
    except OSError as problem:
        raise RegistryError(f"cannot read the registry at {path}: {problem}") from problem
    except ValueError as problem:
        raise RegistryError(f"{path} is not JSON: {problem}") from problem
    return parse_registry(data)


class Placement:
    """Which mission each session landed on, and which ones landed nowhere."""

    def __init__(self):
        self.assigned = collections.OrderedDict()
        self.shared = collections.OrderedDict()
        self.unassigned = []

    def sessions_of(self, branch):
        return [uuid for uuid, (found, _) in self.assigned.items() if found == branch]


def has_window(mission):
    """A window is `[started_at, ended_at)`, so it needs both of its ends.

    Half of one is not a narrower window, it is an unbounded one: a mission
    whose `started_at` never resolved would otherwise claim every session in
    the directory that ended before its `ended_at`, silently inflating its
    cost with work done weeks before the branch existed.
    """
    return mission.started_at is not None and mission.ended_at is not None


def _overlaps(session, mission):
    """Does this session's timestamp interval touch this mission's window?"""
    first, last = session.first(), session.last()
    if first is None or not has_window(mission):
        return False
    return not (last < mission.started_at or first > mission.ended_at)


def assign(sessions, missions):
    """Apply the method's two rules, in order: declared, then window."""
    declared = {
        uuid: mission.branch for mission in missions for uuid in mission.sessions
    }
    placement = Placement()
    for uuid, session in sessions.items():
        if uuid in declared:
            placement.assigned[uuid] = (declared[uuid], "declared")
            continue
        candidates = [
            mission.branch for mission in missions if _overlaps(session, mission)
        ]
        if len(candidates) == 1:
            placement.assigned[uuid] = (candidates[0], "window")
        elif candidates:
            placement.shared[uuid] = sorted(candidates)
        else:
            placement.unassigned.append(uuid)
    return placement


def _add(into, totals):
    for name, value in totals.items():
        into[name] = into.get(name, 0) + value


def bucket(owned):
    """Sum one set of sessions into the report's per-mission shape."""
    main, subagents = {}, {}
    spans = []
    stamps = []
    agent_seconds = 0.0
    has_main = False
    for session in owned:
        for item in session.main:
            has_main = has_main or item.requests > 0
            _add(main, item.totals())
        for item in session.subagents:
            _add(subagents, item.totals())
        for item in session.transcripts:
            spans.extend(item.intervals())
            agent_seconds += item.active_seconds()
            stamps.extend(
                stamp for stamp in (item.first(), item.last()) if stamp is not None
            )
    names = TRANSCRIPTS.EMPTY_TOTALS
    main = {name: main.get(name, 0) for name in names}
    subagents = {name: subagents.get(name, 0) for name in names}
    return {
        "main_thread": main,
        "subagents": subagents,
        "total": {name: main[name] + subagents[name] for name in names},
        "agent_seconds": round(agent_seconds, 3),
        "elapsed_seconds": round(TRANSCRIPTS.union_seconds(spans), 3),
        "span_seconds": round(
            (max(stamps) - min(stamps)).total_seconds() if len(stamps) > 1 else 0.0, 3
        ),
        "first_timestamp_seen": min(stamps).isoformat() if stamps else None,
        "last_timestamp_seen": max(stamps).isoformat() if stamps else None,
        "partial_main_thread": not has_main,
    }


def session_totals(session):
    """One session's own totals and extent.

    The unplaced buckets carry these per session, so a group's unplaced share
    counts only the sessions whose time actually reaches that group's window.
    Without it a single stray session brushing one window charges that group
    the whole unplaced total and every comparison collapses to
    `cannot_be_attributed`.
    """
    names = TRANSCRIPTS.EMPTY_TOTALS
    total = {}
    for item in session.transcripts:
        _add(total, item.totals())
    first, last = session.first(), session.last()
    return {
        "session": session.uuid,
        "total": {name: total.get(name, 0) for name in names},
        "first_timestamp_seen": first.isoformat() if first else None,
        "last_timestamp_seen": last.isoformat() if last else None,
    }


def totals_by_mission(sessions, missions, placement):
    """Per-mission aggregates, plus the two unplaced buckets, keyed by branch."""
    found = collections.OrderedDict()
    for mission in missions:
        found[mission.branch] = bucket(
            [sessions[uuid] for uuid in placement.sessions_of(mission.branch)]
        )
    found[SHARED] = bucket([sessions[uuid] for uuid in placement.shared])
    found[UNASSIGNED] = bucket([sessions[uuid] for uuid in placement.unassigned])
    return found
