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
import json
import os

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


def _counter(usage, name):
    value = usage.get(name)
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


class Transcript:
    """One transcript file, reduced to records and the counts around them."""

    def __init__(self, path, relative, session, kind, root=""):
        self.path = path
        self.relative = relative
        self.root = root
        self.session = session
        self.kind = kind
        self.records = []
        self.lines_seen = 0
        self.unparsed = 0
        self.undated = 0

    def totals(self):
        found = dict(EMPTY_TOTALS)
        found["requests"] = len(self.records)
        for record in self.records:
            for name in COUNTERS:
                found[name] += getattr(record, name)
        found["billable_tokens"] = sum(found[name] for name in COUNTERS)
        return found

    def first(self):
        return self.records[0].timestamp if self.records else None

    def last(self):
        return self.records[-1].timestamp if self.records else None

    def intervals(self):
        """The bounded work intervals of this transcript, in order.

        Consecutive requests closer together than ``IDLE_GAP_SECONDS`` are one
        interval; a longer gap contributes nothing at all.
        """
        spans = []
        for earlier, later in zip(self.records, self.records[1:]):
            gap = (later.timestamp - earlier.timestamp).total_seconds()
            if 0 <= gap <= IDLE_GAP_SECONDS:
                spans.append((earlier.timestamp, later.timestamp))
        return spans

    def active_seconds(self):
        return float(
            sum((end - start).total_seconds() for start, end in self.intervals())
        )

    def span_seconds(self):
        if len(self.records) < 2:
            return 0.0
        return (self.last() - self.first()).total_seconds()


def read(path, relative, session, kind, root=""):
    """Read one transcript into a :class:`Transcript`.

    Nothing but a timestamp and four integers crosses out of this function.
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
            if not isinstance(parsed, dict):
                continue
            message = parsed.get("message")
            usage = message.get("usage") if isinstance(message, dict) else None
            if not isinstance(usage, dict):
                continue
            stamp = parse_timestamp(parsed.get("timestamp"))
            if stamp is None:
                item.undated += 1
                continue
            item.records.append(
                Record(stamp, *(_counter(usage, name) for name in COUNTERS))
            )
            # `parsed`, `message` and `usage` go out of scope here; only the
            # five-field record survives the loop.
    item.records.sort(key=lambda record: record.timestamp)
    return item


def _classify(parts, name):
    """Return ``(session, kind)`` for a transcript path, or ``None``."""
    if not parts:
        return name[: -len(".jsonl")], "main"
    if len(parts) >= 2 and parts[1] == "subagents":
        return parts[0], "subagent"
    return None


def discover(root):
    """Find every transcript under ``root``.

    Returns ``(transcripts, skipped)`` with both ordered by relative path, so
    two runs over one directory agree. Each transcript remembers the name of
    the root it came from, because a session started in a worktree lands in a
    different projects directory than one started in the main checkout.
    """
    found = []
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
            placed = _classify(parts, filename)
            if placed is None:
                skipped.append(relative)
                continue
            session, kind = placed
            found.append(
                read(
                    os.path.join(directory, filename),
                    relative,
                    session,
                    kind,
                    root_name,
                )
            )
    found.sort(key=lambda item: item.relative)
    skipped.sort()
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
