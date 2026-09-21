#!/usr/bin/env python3
"""Read token counters and timestamps out of local session transcripts.

This module is the privacy boundary of the mission-cost harness. A transcript
line is the trader's own conversation; the registered method
(``docs/quality/mission-cost/method.md``, section 3) allows exactly five values
out of it, and :class:`Record` has exactly five fields so that nothing else can
reach a report by accident. Everything else on the line -- the message content,
``cwd``, ``gitBranch``, the model, and the ``usage`` keys that are not token
counters -- is dropped inside :func:`read` and never leaves it.

The layout, from section 2: ``<session>.jsonl`` at the root is a main thread,
``<session>/subagents/*.jsonl`` beneath it are its subagents, and anything else
is reported as skipped rather than guessed at.
"""

import collections
import datetime
import importlib.util
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name, path=None):
    """Load a module by path under a stable key, the way `tools/read_cost` does.

    The one implementation in this package. It lives in the leaf module because
    the leaf is the only one that imports nothing local, so every other module
    can reach it after a small bootstrap and none of them needs a copy.
    """
    key = f"quantick_mission_cost_{name}"
    if key in sys.modules:
        return sys.modules[key]
    path = path or os.path.join(HERE, f"{name}.py")
    # `spec_from_file_location` happily builds a spec for a path that is not
    # there, and the failure then surfaces from deep inside importlib as a
    # FileNotFoundError naming a half-joined path. Check first, say which
    # module and where it was looked for.
    if not os.path.isfile(path):
        raise RuntimeError(f"cannot load {name}: no file at {path}")
    spec = importlib.util.spec_from_file_location(key, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {name} at {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module


def default_transcripts(repo):
    """Where this host keeps the transcripts of sessions rooted at ``repo``.

    The directory name is the absolute repository path with every character
    outside ``[A-Za-z0-9]`` replaced by a dash, which is how the projects
    directory is laid out. It lives beside `discover` because both answer the
    same question -- where the transcripts are -- and a second copy in a second
    command is a second place for the host's layout to be spelled wrong.
    """
    slug = re.sub(r"[^A-Za-z0-9]", "-", os.path.abspath(repo))
    return os.path.join(os.path.expanduser("~"), ".claude", "projects", slug)


# Section 4 of the method. A gap longer than this is idle, not work.
IDLE_GAP_SECONDS = 300

# Section 3 of the method. These four, separately, and nothing else.
COUNTERS = (
    "input_tokens",
    "cache_creation_input_tokens",
    "cache_read_input_tokens",
    "output_tokens",
)

EMPTY_TOTALS = {"requests": 0, "billable_tokens": 0}
EMPTY_TOTALS.update({name: 0 for name in COUNTERS})

Record = collections.namedtuple("Record", ("timestamp",) + COUNTERS)
"""One request's cost. Five values, by construction, so a widened reader fails
the test rather than quietly publishing a sixth."""


def parse_timestamp(value):
    """Parse an ISO-8601 instant, or return ``None`` if it is not one."""
    if not isinstance(value, str):
        return None
    text = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        found = datetime.datetime.fromisoformat(text)
    except ValueError:
        return None
    if found.tzinfo is None:
        found = found.replace(tzinfo=datetime.timezone.utc)
    return found.astimezone(datetime.timezone.utc)


class InstantError(ValueError):
    """Something this package was asked to treat as an instant is not one."""


def require_instant(value, what):
    """Parse an ISO-8601 instant, or refuse rather than compare its spelling.

    The one owner of that rule, for the same reason `canonical.py` owns the byte
    contract. ``Z`` sorts after ``+`` in ASCII, so ``2026-09-20T01:43:13Z`` and
    ``2026-09-20T01:43:13+00:00`` -- one moment, two legal spellings -- land on
    opposite sides of each other when compared as text. Every module that puts a
    mission on one side of a pivot goes through here, so a fifth command cannot
    invent a fifth rule, and a caller catches one exception type rather than one
    per module.
    """
    found = parse_timestamp(value)
    if found is None:
        raise InstantError(f"{what} is not an ISO-8601 instant: {value!r}")
    return found


def _counter(usage, name):
    value = usage.get(name)
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


def record_from(parsed):
    """Reduce one parsed transcript line to a :class:`Record`, or ``None``.

    This is the filter. It is a named function rather than five lines inside
    the read loop so that it can be tested on its own: give it a line carrying
    message content, `cwd` and `gitBranch`, and what comes back has five
    fields and no room for a sixth.
    """
    if not isinstance(parsed, dict):
        return None
    message = parsed.get("message")
    usage = message.get("usage") if isinstance(message, dict) else None
    if not isinstance(usage, dict):
        return None
    stamp = parse_timestamp(parsed.get("timestamp"))
    if stamp is None:
        return None
    return Record(stamp, *(_counter(usage, name) for name in COUNTERS))


class Transcript:
    """One transcript file, folded into counters and work intervals.

    Nothing per-request is retained. A projects directory that has not been
    pruned holds hundreds of thousands of requests, and this harness is meant
    to be re-run across a growing window all campaign long, so what it keeps
    is proportional to the number of work blocks rather than to the number of
    requests ever made.
    """

    def __init__(self, path, relative, session, kind, root=""):
        self.path = path
        self.relative = relative
        self.root = root
        self.session = session
        self.kind = kind
        self.requests = 0
        self.counters = {name: 0 for name in COUNTERS}
        self.spans = []
        self.lines_seen = 0
        self.unparsed = 0
        self.undated = 0
        self.out_of_order = 0
        self._first = None
        self._last = None

    def add(self, record):
        """Fold one record in, then forget it."""
        self.requests += 1
        for name in COUNTERS:
            self.counters[name] += getattr(record, name)
        if self._first is None or record.timestamp < self._first:
            self._first = record.timestamp
        if self._last is None:
            self._last = record.timestamp
            return
        gap = (record.timestamp - self._last).total_seconds()
        if gap < 0:
            # A transcript is an append-only log, so this should not happen.
            # Counting it beats folding a negative gap into the wall clock and
            # reporting a number nobody can explain.
            self.out_of_order += 1
            return
        if gap <= IDLE_GAP_SECONDS:
            if self.spans and self.spans[-1][1] == self._last:
                self.spans[-1] = (self.spans[-1][0], record.timestamp)
            else:
                self.spans.append((self._last, record.timestamp))
        self._last = record.timestamp

    def totals(self):
        found = dict(EMPTY_TOTALS)
        found["requests"] = self.requests
        for name in COUNTERS:
            found[name] = self.counters[name]
        found["billable_tokens"] = sum(found[name] for name in COUNTERS)
        return found

    def first(self):
        return self._first

    def last(self):
        return self._last

    def intervals(self):
        """The bounded work intervals of this transcript, in order.

        Consecutive requests closer together than ``IDLE_GAP_SECONDS`` are one
        interval; a longer gap contributes nothing at all.
        """
        return list(self.spans)

    def active_seconds(self):
        return float(
            sum((end - start).total_seconds() for start, end in self.spans)
        )


def read(path, relative, session, kind, root=""):
    """Read one transcript into a :class:`Transcript`.

    Nothing but a timestamp and four integers crosses out of this function,
    and even those are folded away rather than kept.
    """
    item = Transcript(path, relative, session, kind, root)
    with open(path, "r", encoding="utf-8", errors="replace") as stream:
        for line in stream:
            if not line.strip():
                continue
            item.lines_seen += 1
            try:
                parsed = json.loads(line)
            except ValueError:
                item.unparsed += 1
                continue
            record = record_from(parsed)
            if record is None:
                if isinstance(parsed, dict) and isinstance(
                    parsed.get("message"), dict
                ) and isinstance(parsed["message"].get("usage"), dict):
                    item.undated += 1
                continue
            item.add(record)
            # `parsed` and `record` go out of scope here. What the transcript
            # keeps is four integers, two instants and a span list.
    return item


def classify(relative):
    """Return ``(session, kind)`` for a relative transcript path, or ``None``.

    The one owner of section 2's layout rule, asked by both of its callers:
    :func:`locate`, of a path it walked to, and ``attribution``, of a path a
    person wrote into the registry. It refuses by returning ``None`` rather
    than raising, because the two callers want different words for the same
    refusal -- one skips the file, the other names the registry line that
    cannot address a transcript.

    A session with no name is refused along with the rest: ``.jsonl`` at the
    root would otherwise be a main thread of the empty session, and a registry
    could declare it.
    """
    if not isinstance(relative, str) or not relative.endswith(".jsonl"):
        return None
    parts = relative.split("/")
    if any(part in ("", ".", "..") or "\\" in part for part in parts):
        return None
    if len(parts) == 1:
        session, kind = parts[0][: -len(".jsonl")], "main"
    elif len(parts) >= 3 and parts[1] == "subagents":
        session, kind = parts[0], "subagent"
    else:
        return None
    return (session, kind) if session else None


Located = collections.namedtuple(
    "Located", ("path", "relative", "session", "kind", "root")
)
"""Where one transcript is and what the layout says it is. Nothing read yet."""


def locate(root):
    """Find every transcript under ``root`` **without opening one**.

    The walk and section 2's layout rule, on their own. A caller that brings
    its own fold -- ``shape.py``, which counts only the requests inside a
    window -- would otherwise pay for :func:`discover`'s fold as well and throw
    it away, reading every line of every file twice. Splitting the walk out
    lets each file be read exactly once while :func:`classify` stays the one
    owner of "which file is a main thread and which is a subagent".

    Returns ``(located, skipped)`` with both ordered by relative path, so two
    runs over one directory agree. Each entry remembers the name of the root it
    came from, because a session started in a worktree lands in a different
    projects directory than one started in the main checkout.
    """
    located = []
    skipped = []
    root_name = os.path.basename(os.path.abspath(root))
    for directory, subdirectories, filenames in os.walk(root):
        subdirectories.sort()
        relative_dir = os.path.relpath(directory, root).replace(os.sep, "/")
        parts = [] if relative_dir == "." else relative_dir.split("/")
        for filename in sorted(filenames):
            if not filename.endswith(".jsonl"):
                continue
            relative = "/".join(parts + [filename])
            placed = classify(relative)
            if placed is None:
                skipped.append(relative)
                continue
            session, kind = placed
            located.append(
                Located(
                    os.path.join(directory, filename),
                    relative,
                    session,
                    kind,
                    root_name,
                )
            )
    located.sort(key=lambda item: item.relative)
    skipped.sort()
    return located, skipped


def discover(root):
    """Find every transcript under ``root`` and fold each one.

    :func:`locate` owns the walk; this adds the read, which is what a caller
    with no fold of its own wants. Returns ``(transcripts, skipped)`` ordered
    by relative path, exactly as :func:`locate` orders them.
    """
    located, skipped = locate(root)
    found = [
        read(item.path, item.relative, item.session, item.kind, item.root)
        for item in located
    ]
    return found, skipped


def merge_intervals(spans):
    """Merge overlapping or touching intervals into a disjoint, ordered set."""
    merged = []
    for start, end in sorted(spans):
        if merged and start <= merged[-1][1]:
            if end > merged[-1][1]:
                merged[-1] = (merged[-1][0], end)
            continue
        merged.append((start, end))
    return merged


def union_seconds(spans):
    """How much distinct time a set of intervals covers."""
    return float(
        sum((end - start).total_seconds() for start, end in merge_intervals(spans))
    )
