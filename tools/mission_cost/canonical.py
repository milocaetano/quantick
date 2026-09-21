#!/usr/bin/env python3
"""The one canonical-JSON writer for everything in this package.

Section 7 of ``docs/quality/mission-cost/method.md`` is a contract about bytes:
sorted keys, ASCII, one trailing newline, and the same bytes whether the report
went to standard output or to a file. A contract kept in three places is three
places for it to drift, and a reading whose bytes differ by its output channel
cannot be compared with one taken yesterday.

Nothing here reads a transcript, a registry or the network.
"""

import json
import sys


def render(document):
    """Canonical JSON: sorted keys, ASCII, one trailing newline."""
    return json.dumps(document, sort_keys=True, indent=2, ensure_ascii=True) + "\n"


def emit(text, where):
    """Write the text with LF endings, whichever way it leaves.

    Through the text layer, Windows turns every newline into CRLF on the way to
    standard output while a file write keeps LF, so the same report would hash
    two ways depending on how it was captured. The bytes go out as bytes.
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
