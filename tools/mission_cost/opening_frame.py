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

The privacy boundary is ``transcripts.py``'s: this program calls
``record_from`` and reads the five fields it returns. It opens no line any other
way.
"""

import argparse
import importlib.util
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def _bootstrap():
    """Load `transcripts.py` beside this file, once, under a stable key."""
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
DISPERSION = TRANSCRIPTS.load("dispersion")

VERSION = 1
METHOD = "docs/quality/mission-cost/method.md"


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
    """One row per session that has a main-thread transcript, oldest first."""
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
    rows.sort(key=lambda row: (row["at"], row["session"]))
    return rows


def default_transcripts(repo):
    """Where this host keeps the transcripts of sessions rooted at ``repo``."""
    slug = re.sub(r"[^A-Za-z0-9]", "-", os.path.abspath(repo))
    return os.path.join(os.path.expanduser("~"), ".claude", "projects", slug)


def build(rows, since, pivot):
    """The report: the rows kept, their dispersion, and the pivot split."""
    kept = [row for row in rows if since is None or row["at"] >= since]
    document = {
        "schema": VERSION,
        "method": METHOD,
        "metric": "opening_prompt_tokens",
        "registered_comparison": False,
        "since": since,
        "pivot": pivot,
        "sessions": kept,
        "summary": DISPERSION.summary([row["prompt_tokens"] for row in kept]),
    }
    if pivot is not None:
        document["result"] = DISPERSION.compare(
            [row["prompt_tokens"] for row in kept if row["at"] < pivot],
            [row["prompt_tokens"] for row in kept if row["at"] >= pivot],
        )
    return document


def render(document):
    """Canonical JSON: sorted keys, ASCII, one trailing newline."""
    return json.dumps(document, sort_keys=True, indent=2, ensure_ascii=True) + "\n"


def emit(text, where):
    """Write LF-terminated bytes, so standard output and ``--out`` agree."""
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
    emit(render(build(openings(roots), options.since, options.pivot)), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
