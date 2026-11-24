"""The settlement a tab measures its day's change from, and when it is said.

quantick holds a reading for the prints from its `time_ms` to the end of that
server day (PROTOCOL.md `settlement`). So the bridge says it once the session
is up, again when the terminal's value changes, again on a new server day
even if it did not, and says "none" when it vanishes. The price travels at the
terminal's full precision, never rounded to the symbol's `digits`.

Run directly (`python bridge/mt5/tests/test_settlement.py`) or through
`cargo test -p quantick-feed-mt5 --test bridge_paging`.
"""

from __future__ import annotations

import sys
import time
import types
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import (  # noqa: E402  (deliberately after the path insert)
    FakeTerminal,
    check,
    load_bridge,
    patch_bridge,
    run_tests,
    session_at,
)

NOW = 1_784_824_260
DAY_S = 86_400


def settled_session(value: dict):
    """A session whose terminal reports `value["settlement"]`."""
    term = FakeTerminal(0, NOW)
    term.symbol_info = lambda _symbol: types.SimpleNamespace(
        session_price_settlement=value["settlement"]
    )
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW)
    session.args.heartbeat_seconds = 5.0
    return session


def settlements(session) -> list[dict]:
    return [msg for msg in session.sent if msg["type"] == "settlement"]


def test_the_settlement_is_said_once_at_the_terminals_precision():
    value = {"settlement": 5412.3725}
    session = settled_session(value)
    session.say_settlement()
    session.say_settlement()
    said = settlements(session)
    check("said once while nothing changed", len(said) == 1, said)
    check(
        "more decimals than digits=0, never rounded",
        said[0] == {"type": "settlement", "time_ms": NOW * 1000, "price": "5412.3725"},
        said[0],
    )
    value["settlement"] = 171750.0
    session.say_settlement()
    check("a whole price carries no padding", settlements(session)[-1]["price"] == "171750")


def test_a_new_server_day_or_a_vanished_value_is_said_again():
    value = {"settlement": 171750.0}
    session = settled_session(value)
    session.say_settlement()
    later = NOW + DAY_S
    patch_bridge(
        "time",
        types.SimpleNamespace(
            time=lambda: float(later), monotonic=time.monotonic, perf_counter=time.perf_counter
        ),
    )
    session.say_settlement()
    said = settlements(session)
    check("the same price again on the new day", len(said) == 2, said)
    check("stamped with the new day", said[-1]["time_ms"] == later * 1000, said[-1])
    value["settlement"] = 0.0
    session.say_settlement()
    check(
        "a vanished settlement is said as none",
        settlements(session)[-1] == {"type": "settlement", "time_ms": later * 1000},
        settlements(session)[-1],
    )


def test_the_loop_asks_the_terminal_once_a_heartbeat_interval():
    value = {"settlement": 171750.0}
    session = settled_session(value)
    session.maybe_settlement()
    value["settlement"] = 171800.0
    session.maybe_settlement()
    check("the first pass says it; the next waits", len(settlements(session)) == 1)


if __name__ == "__main__":
    sys.exit(run_tests(globals()))
