#!/usr/bin/env python3
"""How big the prompt is on a session's very first request.

Why this exists, beside ``measure.py``: the mission comparison in
``docs/quality/mission-cost/method.md`` cannot place a session on a mission in
this repository, so a change to the always-loaded context — ``CLAUDE.md``, the
agent instructions, the published tool list — has no per-mission number to move.
The first request of a session is a different kind of evidence. It carries the
standing frame, it needs no attribution at all, and every session is placed by
its own first timestamp rather than by a window that might belong to somebody
else's branch.

What it is not: this is **not** the registered mission comparison. It reads
``dispersion.py``'s already-registered thresholds unchanged, and it is
explicitly post-hoc — it was written for #565's baseline after the data existed,
which ``method.md`` section 5 is deliberately protected against. Treat it as
corroboration next to a method verdict, never as one.

What it measures, exactly: ``input_tokens + cache_creation_input_tokens +
cache_read_input_tokens`` of the first request in a session's **main-thread**
transcript. That is the whole prompt of that turn however it was cached, so the
number does not move with cache state. It includes the system prompt, the tool
definitions, the always-loaded instructions and the first user message.

What moves it besides the instructions, and therefore bounds any claim:

- the first message differs from session to session;
- the connected MCP servers differ, and their tool definitions are in the frame;
- a resumed session does not open on a fresh frame, and nothing in the five
  fields this reader takes can tell a resumption from a fresh start.

Every document it writes carries an ``inputs`` block with the roots it read,
how many main-thread transcripts it found and a digest over all of them, for the
same reason ``measure.py`` does: the transcript directory is live and
append-only, so a number taken from it is only checkable against the directory
state that produced it. The digest covers every opening found, before ``--since``
drops any of them -- narrowing a reading must not narrow its provenance.

The privacy boundary is ``transcripts.py``'s: this program calls
``record_from`` and reads the five fields it returns. It opens no line any other
way.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def _bootstrap():
    """Load `transcripts.py` beside this file, once, under a stable key."""
    key = "quantick_mission_cost_transcripts"
    if key in sys.modules:
        return sys.modules[key]
    path = os.path.join(HERE, "transcripts.py")
    if not os.path.isfile(path):
        raise RuntimeError(f"cannot load transcripts.py: no file at {path}")
    spec = importlib.util.spec_from_file_location(key, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load transcripts.py beside {__file__}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module


TRANSCRIPTS = _bootstrap()
DISPERSION = TRANSCRIPTS.load("dispersion")
CANONICAL = TRANSCRIPTS.load("canonical")

VERSION = 1
METHOD = "docs/quality/mission-cost/method.md"

# Bound here, never redefined here: `canonical.py` owns section 7's byte
# contract, and `transcripts.py` owns where this host keeps its transcripts and
# the one rule for turning text into an instant. A session's own instant comes
# from `datetime.isoformat`; a `--since` or a `--pivot` comes from whoever typed
# it, and the two are compared as instants or not at all.
render = CANONICAL.render
emit = CANONICAL.emit
default_transcripts = TRANSCRIPTS.default_transcripts
instant = TRANSCRIPTS.require_instant
InstantError = TRANSCRIPTS.InstantError


def digest(rows):
    """A stable fingerprint of what this reading was taken from.

    The transcript directory is live and append-only, so a number from it is
    only checkable against the directory state that produced it -- which is why
    `measure.py` stamps every document it writes. This tool reads one line per
    main-thread transcript, so its input set is exactly those openings: every
    row found, before `--since` drops any of them.
    """
    lines = sorted(
        f"{row['root']}/{row['session']}|{row['at']}|{row['prompt_tokens']}"
        for row in rows
    )
    text = "\n".join(lines)
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def first_record(path):
    """The first line of one transcript that carries a usage block, or None."""
    with open(path, "r", encoding="utf-8", errors="replace") as stream:
        for line in stream:
            if not line.strip():
                continue
            try:
                parsed = json.loads(line)
            except ValueError:
                continue
            record = TRANSCRIPTS.record_from(parsed)
            if record is not None:
                return record
    return None


def prompt_tokens(record):
    """Everything that was in the prompt, whichever way it was cached."""
    return (
        record.input_tokens
        + record.cache_creation_input_tokens
        + record.cache_read_input_tokens
    )


def openings(roots):
    """One row per session that has a main-thread transcript, oldest first.

    Ordered by the parsed instant, not by its spelling, for the same reason
    :func:`instant` exists.
    """
    rows = []
    for root in roots:
        name = os.path.basename(os.path.abspath(root))
        for entry in sorted(os.listdir(root)):
            if not entry.endswith(".jsonl"):
                continue
            record = first_record(os.path.join(root, entry))
            if record is None:
                continue
            rows.append(
                {
                    "root": name,
                    "session": entry[: -len(".jsonl")],
                    "at": record.timestamp.isoformat(),
                    "prompt_tokens": prompt_tokens(record),
                }
            )
    rows.sort(key=lambda row: (instant(row["at"], "an opening"), row["session"]))
    return rows


def build(rows, since, pivot):
    """The report: the rows kept, their dispersion, and the pivot split.

    Every instant is parsed once, here, and only instants are compared.
    """
    dated = [(instant(row["at"], f"{row['session']}: at"), row) for row in rows]
    floor = instant(since, "--since") if since is not None else None
    split = instant(pivot, "--pivot") if pivot is not None else None
    kept = [(at, row) for at, row in dated if floor is None or at >= floor]
    document = {
        "schema": VERSION,
        "method": METHOD,
        "metric": "opening_prompt_tokens",
        "registered_comparison": False,
        "since": since,
        "pivot": pivot,
        "inputs": {
            "roots": sorted({row["root"] for row in rows}),
            "transcripts": len(rows),
            "digest": digest(rows),
        },
        "sessions": [row for _, row in kept],
        "summary": DISPERSION.summary([row["prompt_tokens"] for _, row in kept]),
    }
    if split is not None:
        document["result"] = DISPERSION.compare(
            [row["prompt_tokens"] for at, row in kept if at < split],
            [row["prompt_tokens"] for at, row in kept if at >= split],
        )
    return document


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="The prompt size of each session's first request; not a method verdict"
    )
    parser.add_argument("--repo", default=".", help="the repository to measure from")
    parser.add_argument(
        "--transcripts",
        action="append",
        default=None,
        metavar="DIR",
        help="a session transcript directory; repeatable",
    )
    parser.add_argument(
        "--since", default=None, help="drop sessions that opened before this instant"
    )
    parser.add_argument(
        "--pivot",
        default=None,
        help="split the kept sessions at this instant and grade the split",
    )
    parser.add_argument("--out", default="-", help="where to write; - is stdout")
    options = parser.parse_args(argv)
    roots = options.transcripts or [default_transcripts(options.repo)]
    for root in roots:
        if not os.path.isdir(root):
            parser.error(
                f"no transcript directory at {root}. Transcripts are local to "
                "the trader's machine and are not in the repository; see "
                "tools/mission_cost/README.md"
            )
    try:
        document = build(openings(roots), options.since, options.pivot)
    except InstantError as problem:
        parser.error(str(problem))
    emit(render(document), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
