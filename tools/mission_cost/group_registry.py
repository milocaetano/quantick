#!/usr/bin/env python3
"""Split a mission registry into `before` and `after` around one instant.

Section 5 of ``docs/quality/mission-cost/method.md`` grades two groups of
missions, and section 6 gives each mission one ``group`` field. Grading seven
merged optimizations therefore needs seven groupings of the same population,
and hand-editing eighty-seven records seven times is neither reproducible nor
checkable.

The rule, applied mechanically and stated here so a reader never has to infer
it from the output:

- **before** — the mission's window closed strictly before the pivot, so every
  hour of its work ran under the old loop;
- **after** — the mission's window opened at or after the pivot, so every hour
  of its work ran under the new one;
- **neither** — the mission straddles the pivot, or has no window. It keeps
  ``group: null`` and takes no part in the comparison. The mission that
  delivered the change lands here by construction: its window ends at the
  pivot, not before it.

A mission that straddles the pivot is excluded rather than assigned, for the
same reason section 2 never divides a session: there is no honest way to say
which side of the change its cost belongs to.

This tool reads and writes registries. It never touches a transcript.
"""

import argparse
import json
import sys

SCHEMA = 1


def load(path):
    with open(path, encoding="utf-8") as stream:
        document = json.load(stream)
    if not isinstance(document, dict) or document.get("schema") != SCHEMA:
        raise ValueError(f"{path}: not a schema {SCHEMA} mission registry")
    if not isinstance(document.get("missions"), list):
        raise ValueError(f"{path}: the registry needs a `missions` list")
    return document


def group_for(mission, pivot):
    """Which group this mission belongs to, or ``None`` for neither."""
    started, ended = mission.get("started_at"), mission.get("ended_at")
    if ended is not None and ended < pivot:
        return "before"
    if started is not None and started >= pivot:
        return "after"
    return None


def regroup(document, pivot):
    """A copy of the registry with every ``group`` recomputed around ``pivot``."""
    missions = []
    for mission in document["missions"]:
        found = dict(mission)
        found["group"] = group_for(mission, pivot)
        missions.append(found)
    return {"schema": SCHEMA, "missions": missions}


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
        description="Group a mission registry around one instant, per method section 5"
    )
    parser.add_argument(
        "--registry",
        default="docs/quality/mission-cost/missions.json",
        help="the registry to group (default: the campaign registry)",
    )
    parser.add_argument(
        "--pivot",
        required=True,
        help="the instant the change landed, as it appears in the registry windows",
    )
    parser.add_argument("--out", default="-", help="where to write; - is stdout")
    options = parser.parse_args(argv)
    try:
        document = load(options.registry)
    except (OSError, ValueError, json.JSONDecodeError) as problem:
        parser.error(str(problem))
    emit(render(regroup(document, options.pivot)), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
