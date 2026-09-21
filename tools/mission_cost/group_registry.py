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

Instants are **parsed and compared as instants**, through the one parser in
``transcripts.py``. Comparing them as strings would let the spelling decide the
group: ``2026-09-20T01:43:13Z`` sorts *after* ``2026-09-20T01:43:13+00:00``
because ``Z`` sorts after ``+``, so the same moment written two ways would land
on two sides of the same pivot. A registry instant or a pivot that is not an
ISO-8601 instant is refused rather than silently misgrouped.

A mission that straddles the pivot is excluded rather than assigned, for the
same reason section 2 never divides a session: there is no honest way to say
which side of the change its cost belongs to.

This tool reads and writes registries. It never touches a transcript.
"""

import argparse
import datetime
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCHEMA = 1


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
CANONICAL = TRANSCRIPTS.load("canonical")

render = CANONICAL.render
emit = CANONICAL.emit


class GroupingError(RuntimeError):
    """An instant this tool was given is not an instant."""


def instant(value, what):
    """Parse an ISO-8601 instant, or refuse rather than compare its spelling."""
    found = TRANSCRIPTS.parse_timestamp(value)
    if found is None:
        raise GroupingError(f"{what} is not an ISO-8601 instant: {value!r}")
    return found


def load(path):
    with open(path, encoding="utf-8") as stream:
        document = json.load(stream)
    if not isinstance(document, dict) or document.get("schema") != SCHEMA:
        raise ValueError(f"{path}: not a schema {SCHEMA} mission registry")
    if not isinstance(document.get("missions"), list):
        raise ValueError(f"{path}: the registry needs a `missions` list")
    return document


def group_for(mission, pivot):
    """Which group this mission belongs to, or ``None`` for neither.

    ``pivot`` is an instant or its ISO-8601 spelling; both ends of the
    mission's window are parsed before anything is compared.
    """
    if not isinstance(pivot, datetime.datetime):
        pivot = instant(pivot, "the pivot")
    branch = mission.get("branch")
    started, ended = mission.get("started_at"), mission.get("ended_at")
    if ended is not None and instant(ended, f"{branch}: ended_at") < pivot:
        return "before"
    if started is not None and instant(started, f"{branch}: started_at") >= pivot:
        return "after"
    return None


def regroup(document, pivot):
    """A copy of the registry with every ``group`` recomputed around ``pivot``."""
    found_pivot = instant(pivot, "the pivot")
    missions = []
    for mission in document["missions"]:
        found = dict(mission)
        found["group"] = group_for(mission, found_pivot)
        missions.append(found)
    return {"schema": SCHEMA, "missions": missions}


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
        grouped = regroup(document, options.pivot)
    except (OSError, ValueError, json.JSONDecodeError, GroupingError) as problem:
        parser.error(str(problem))
    emit(render(grouped), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
