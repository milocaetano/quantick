"""The venue's deal counter rides on every live tick, and on nothing else.

MetaTrader folds several exchange deals into one tick and keeps no count per
tick; the session's running total (`SYMBOL_SESSION_DEALS`) is the only deal
count it has. The bridge reads it once per pump round and stamps it on the
ticks that round fetched, so quantick can cut bars every N deals where
ProfitChart's Trades chart cuts them. These tests pin the three things the
feed relies on: the stamp is the terminal's reading, history carries none,
and a venue without a counter sends no field at all.

Run directly (`python bridge/mt5/tests/test_deals.py`) or through
`cargo test -p quantick-feed-mt5 --test bridge_paging`.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import (  # noqa: E402  (deliberately after the path insert)
    FakeTerminal,
    check,
    load_bridge,
    run_tests,
    session_at,
    tick_at,
)

NOW = 1_784_824_260


def live_ticks(session) -> list[dict]:
    return [msg for msg in session.sent if msg["type"] == "tick"]


def test_live_ticks_carry_the_terminals_session_deal_count():
    term = FakeTerminal(0, NOW)
    term.ticks = [tick_at((NOW + i) * 1000) for i in range(3)]
    term.session_deals = 2_301_455
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW)
    session.deal_counter = True
    session.pump_ticks()

    ticks = live_ticks(session)
    check("the round forwarded every tick", len(ticks) == 3, len(ticks))
    check(
        "each one is stamped with the counter the terminal reported",
        [t.get("deals") for t in ticks] == [2_301_455] * 3,
        [t.get("deals") for t in ticks],
    )

    term.ticks.append(tick_at((NOW + 10) * 1000))
    term.session_deals = 2_301_462
    session.pump_ticks()
    newest = live_ticks(session)[-1]
    check(
        "the next round reads the counter again",
        newest.get("deals") == 2_301_462,
        newest.get("deals"),
    )


def test_history_ticks_carry_no_count():
    """The terminal has no count for a tick it stored; stamping one would
    attach today's reading to yesterday's print."""
    term = FakeTerminal(0, NOW)
    term.ticks = [tick_at((NOW - 3600 + i) * 1000) for i in range(60)]
    term.session_deals = 4_000_000
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW, backfill_minutes=720)
    session.deal_counter = True
    session.backfill()

    stamped = [t for t in live_ticks(session) if "deals" in t]
    check("no history tick is stamped", not stamped, len(stamped))


def test_a_venue_without_a_counter_sends_no_field():
    term = FakeTerminal(0, NOW)
    term.ticks = [tick_at((NOW + i) * 1000) for i in range(2)]
    term.session_deals = None
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW)
    session.deal_counter = True
    session.pump_ticks()

    stamped = [t for t in live_ticks(session) if "deals" in t]
    check("a terminal answering no counter stamps nothing", not stamped, len(stamped))

    session.deal_counter = False
    term.session_deals = 10
    term.ticks.append(tick_at((NOW + 5) * 1000))
    session.pump_ticks()
    stamped = [t for t in live_ticks(session) if "deals" in t]
    check("a session that declared no counter stamps nothing", not stamped, len(stamped))


def test_the_hello_declares_the_counter_on_a_trades_tape_only():
    bridge = load_bridge(FakeTerminal(0, NOW))
    check(
        "a trades tape with a counter declares it",
        bridge.declares_deal_counter("trades", 12) is True,
    )
    check(
        "a trades tape whose terminal reports no counter does not",
        bridge.declares_deal_counter("trades", None) is False,
    )
    check(
        "a quoted instrument never does, whatever the terminal says",
        bridge.declares_deal_counter("quotes", 12) is False,
    )


if __name__ == "__main__":
    sys.exit(run_tests(globals()))
