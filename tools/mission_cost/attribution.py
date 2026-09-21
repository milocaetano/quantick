#!/usr/bin/env python3
"""Place each transcript on a mission, and split the cost main thread from subagents.

Sections 1, 2 and 6 of ``docs/quality/mission-cost/method.md``. A mission is one
branch and one pull request; the session is the unit of assignment and inference
never divides one, so a session that two missions could claim goes to the shared
bucket with both names on it rather than being split by timestamps that cannot
split it fairly.

A record may name an individual transcript instead, and then the session *is*
divided -- by a person, not by a guess. That is rule one because the session
stopped being the thing a mission owns: siblings dispatched under one
coordinator session share its directory, and a mission that caps a context and
hands off to a fresh one owns several transcripts.

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
    "Mission",
    (
        "branch",
        "pr",
        "sessions",
        "transcripts",
        "started_at",
        "ended_at",
        "group",
        "note",
    ),
)


class Session:
    """One session UUID: its main transcript, if any, and its subagents.

    Also the shape of a *part* of one. A mission that declared two transcripts
    out of a shared session owns a parcel with the same two lists and no root
    file, and everything downstream -- the totals, the main/subagent split,
    ``partial_main_thread`` -- reads it without knowing the difference.
    """

    def __init__(self, uuid):
        self.uuid = uuid
        self.main = []
        self.subagents = []

    def add(self, item):
        """File one transcript under the thread its layout says it is."""
        if item.kind == "main":
            self.main.append(item)
        else:
            self.subagents.append(item)

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
        sessions.setdefault(item.session, Session(item.session)).add(item)
    return sessions


def _instant(entry, field):
    """One registry window end, through the package's one instant rule.

    The rule lives in `transcripts.require_instant`. The failure is re-raised as
    a `RegistryError` because here it is a fault in the registry document, which
    is what this module's callers already catch. The message now ends with the
    offending value, which the shared rule appends and this one did not.
    """
    value = entry.get(field)
    if value is None:
        return None
    try:
        return TRANSCRIPTS.require_instant(value, f"{entry.get('branch')}: {field}")
    except TRANSCRIPTS.InstantError as problem:
        raise RegistryError(str(problem)) from problem


def _declared_sessions(branch, entry, seen):
    """Read one record's `sessions`, refusing a session two missions claim."""
    sessions = entry.get("sessions") or []
    if not isinstance(sessions, list):
        raise RegistryError(f"{branch}: `sessions` must be a list")
    for uuid in sessions:
        if not isinstance(uuid, str) or not uuid:
            raise RegistryError(f"{branch}: a session id must be a string")
        if uuid in seen:
            raise RegistryError(
                f"{uuid}: declared by both {seen[uuid]} and {branch}"
            )
        seen[uuid] = branch
    return tuple(sessions)


def _declared_transcripts(branch, entry, seen):
    """Read one record's `transcripts`, refusing a path two missions claim.

    The path is checked against the layout rule rather than against the disk:
    a registry is read on machines that never held the transcripts, and
    ``assign`` reports what it could not find (E6). What is refused here is a
    path that could never name a transcript anywhere.
    """
    declared = entry.get("transcripts") or []
    if not isinstance(declared, list):
        raise RegistryError(f"{branch}: `transcripts` must be a list")
    for path in declared:
        if TRANSCRIPTS.classify(path) is None:
            raise RegistryError(
                f"{branch}: not a transcript path: {path!r}. Expected "
                "`<session>.jsonl` or `<session>/subagents/<name>.jsonl`"
            )
        if path in seen:
            raise RegistryError(
                f"{path}: declared by both {seen[path]} and {branch}"
            )
        seen[path] = branch
    return tuple(declared)


def _refuse_crossed_claims(seen_sessions, seen_transcripts):
    """Refuse a transcript whose session a *different* mission declares.

    Both claims are exact and they contradict each other, so there is no
    reading of the registry that honours both. The path spells its session, so
    this is caught without opening a file -- and it is caught after every
    record has been read, because the conflict is the claim and not the order
    the two were written in.
    """
    for path, branch in seen_transcripts.items():
        session, _ = TRANSCRIPTS.classify(path)
        owner = seen_sessions.get(session)
        if owner is not None and owner != branch:
            raise RegistryError(
                f"{path}: declared by {branch}, but its session {session} is "
                f"declared by {owner}"
            )


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
    seen_transcripts = {}
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
        missions.append(
            Mission(
                branch=branch,
                pr=entry.get("pr"),
                sessions=_declared_sessions(branch, entry, seen_sessions),
                transcripts=_declared_transcripts(branch, entry, seen_transcripts),
                started_at=_instant(entry, "started_at"),
                ended_at=_instant(entry, "ended_at"),
                group=group,
                note=entry.get("note"),
            )
        )
    _refuse_crossed_claims(seen_sessions, seen_transcripts)
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
    """Which mission each transcript landed on, and which ones landed nowhere.

    Whole sessions are still the common case and still keyed by UUID in
    ``assigned``, ``shared`` and ``unassigned``. What a record named outright
    is in ``transcripts``, and where naming one divided a session the part
    nobody named is kept in ``remainders`` -- so a caller asking for a
    session's cost gets the part that is actually in play rather than the
    whole of it counted twice.
    """

    def __init__(self):
        self.assigned = collections.OrderedDict()
        self.shared = collections.OrderedDict()
        self.unassigned = []
        self.transcripts = collections.OrderedDict()
        self.remainders = collections.OrderedDict()
        self.missing = []
        self._claimed = collections.OrderedDict()

    def claim(self, branch, uuid, item):
        """Give one named transcript to one mission, keeping its session."""
        parcels = self._claimed.setdefault(branch, collections.OrderedDict())
        parcels.setdefault(uuid, Session(uuid)).add(item)
        self.transcripts[item.relative] = branch

    def sessions_of(self, branch):
        return [uuid for uuid, (found, _) in self.assigned.items() if found == branch]

    def transcripts_of(self, branch):
        return [path for path, found in self.transcripts.items() if found == branch]

    def parcel_for(self, sessions, uuid):
        """The part of one session that is in play: its remainder, or all of it."""
        return self.remainders.get(uuid, sessions[uuid])

    def parcels_of(self, branch, sessions):
        """Every session-shaped parcel this mission owns, whole or in part."""
        found = [self.parcel_for(sessions, uuid) for uuid in self.sessions_of(branch)]
        found.extend(self._claimed.get(branch, {}).values())
        return found


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
    """Apply the method's three rules, in order, each to what the last left.

    Rule one takes the transcripts a record named. Rules two and three see
    what is left of each session, which for a registry that names no
    transcript is the whole of it -- so such a registry is assigned exactly
    what it was assigned before rule one existed.
    """
    declared_sessions = {
        uuid: mission.branch for mission in missions for uuid in mission.sessions
    }
    declared_paths = {
        path: mission.branch for mission in missions for path in mission.transcripts
    }
    placement = Placement()
    for uuid, session in sessions.items():
        remainder = Session(uuid)
        for item in session.transcripts:
            branch = declared_paths.get(item.relative)
            if branch is None:
                remainder.add(item)
            else:
                placement.claim(branch, uuid, item)
        if len(remainder.transcripts) != len(session.transcripts):
            placement.remainders[uuid] = remainder
        if not remainder.transcripts:
            continue
        if uuid in declared_sessions:
            placement.assigned[uuid] = (declared_sessions[uuid], "declared")
            continue
        candidates = [
            mission.branch for mission in missions if _overlaps(remainder, mission)
        ]
        if len(candidates) == 1:
            placement.assigned[uuid] = (candidates[0], "window")
        elif candidates:
            placement.shared[uuid] = sorted(candidates)
        else:
            placement.unassigned.append(uuid)
    for mission in missions:
        placement.missing.extend(
            (mission.branch, path)
            for path in mission.transcripts
            if path not in placement.transcripts
        )
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
            placement.parcels_of(mission.branch, sessions)
        )
    found[SHARED] = bucket(
        [placement.parcel_for(sessions, uuid) for uuid in placement.shared]
    )
    found[UNASSIGNED] = bucket(
        [placement.parcel_for(sessions, uuid) for uuid in placement.unassigned]
    )
    return found
