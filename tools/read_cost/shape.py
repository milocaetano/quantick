#!/usr/bin/env python3
"""What a measurement says about the files it counted.

``measure.py`` answers with a list of files, each carrying the roles it played
in the change: ``touched`` for a file the diff wrote, ``referenced`` for one
it reads without writing. Every consumer of that answer — the ledger row, the
pull-request comment, the ``pr-gate`` advisory — asks the same two questions
of it, so the questions live here rather than inside whichever consumer
happened to need them first.

The distinction that matters is *referenced but not touched*: those are the
files a reader has to open and the change never edits, which is the only part
of the number detaching something would lower.
"""

# How many referenced files a reader is shown. Enough to act on, few enough
# to read: the tail of a long list is noise, and the number a branch is over
# by is almost always in the first few rows.
TOP_REFERENCED = 5


def touched_files(report):
    """The production sources the diff wrote."""
    return [entry for entry in report["files"] if "touched" in entry["roles"]]


def referenced_files(report):
    """The files pulled in by reference alone, largest first.

    A file that is both touched and referenced is excluded: it is already in
    the diff, so detaching the reference saves the reader nothing.
    """
    rows = [
        entry
        for entry in report["files"]
        if "referenced" in entry["roles"] and "touched" not in entry["roles"]
    ]
    return sorted(rows, key=lambda entry: (-entry["lines"], entry["path"]))


def top_referenced(report, limit=TOP_REFERENCED):
    return referenced_files(report)[:limit]
