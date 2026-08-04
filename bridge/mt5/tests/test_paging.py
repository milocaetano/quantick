"""Tests for the bridge's candle paging, without a terminal.

The bridge imports `MetaTrader5`, which exists only on Windows next to an
installed terminal — so CI can never import the real thing. This stubs the
module in `sys.modules` before importing the bridge, with the documented
`copy_rates_from` semantics *including* the refusal that caused the bug these
tests exist for: the terminal validates a request's bar count against its
"Max bars in chart" setting and returns `(-2, 'Terminal: Invalid params')`
rather than truncating.

Run directly (`python bridge/mt5/tests/test_paging.py`) or through
`cargo test -p quantick-feed-mt5 --test bridge_paging`, which shells out to
exactly this file so the four checks cover it.
"""

from __future__ import annotations

import sys
import types
from pathlib import Path

BRIDGE_DIR = Path(__file__).resolve().parent.parent

# Bars the fake terminal will serve, newest last.
M1 = 60


class FakeTerminal:
    """The subset of the MetaTrader5 API the candle path touches."""

    TIMEFRAME_M1 = 1

    def __init__(self, available: int, newest_s: int, maxbars: int = 100_000) -> None:
        self.times = [newest_s - i * M1 for i in range(available)][::-1]
        self.maxbars = maxbars
        self.calls: list[tuple[int, int]] = []
        self.error = (0, "ok")
        # Pages that should fail outright, by 1-based call index.
        self.fail_on: set[int] = set()

    def copy_rates_from(self, _symbol, _timeframe, anchor, count):
        self.calls.append((int(anchor), int(count)))
        if len(self.calls) in self.fail_on:
            self.error = (-1, "Terminal: some other failure")
            return None
        if count > self.maxbars:
            # The bug: refused on the size of the request, not on the data.
            self.error = (-2, "Terminal: Invalid params")
            return None
        self.error = (0, "ok")
        upto = [t for t in self.times if t <= anchor]
        return [
            {
                "time": t,
                "open": 100.0,
                "high": 101.0,
                "low": 99.0,
                "close": 100.5,
                "tick_volume": 7,
                "real_volume": 3,
            }
            for t in upto[-count:]
        ]

    def last_error(self):
        return self.error


def load_bridge(terminal: FakeTerminal):
    """Import the bridge against `terminal`, fresh each time."""
    module = types.ModuleType("MetaTrader5")
    module.TIMEFRAME_M1 = FakeTerminal.TIMEFRAME_M1
    module.copy_rates_from = terminal.copy_rates_from
    module.last_error = terminal.last_error
    sys.modules["MetaTrader5"] = module
    sys.path.insert(0, str(BRIDGE_DIR))
    sys.modules.pop("quantick_bridge", None)
    import quantick_bridge  # noqa: PLC0415  (deliberately late, after the stub)

    return quantick_bridge


class FakeArgs:
    def __init__(self, rates_max_bars=200_000, rates_months=3):
        self.rates_max_bars = rates_max_bars
        self.rates_months = rates_months


def session_for(bridge, terminal, **args):
    """A Session wired to `terminal`, bypassing __init__'s socket."""
    session = object.__new__(bridge.Session)
    session.symbol = "WINQ26"
    session.args = FakeArgs(**args)
    session.offset_s = 0
    session.digits = 0
    session.tape = "trades"
    session.sent: list[dict] = []
    session.send = session.sent.append
    return session


NOW = 1_784_824_260
SPAN = 93 * 86_400
FROM = NOW - SPAN

FAILURES: list[str] = []


def check(name, condition, detail=""):
    if condition:
        print(f"  ok   {name}")
    else:
        FAILURES.append(f"{name}: {detail}")
        print(f"  FAIL {name}: {detail}")


def test_young_contract_stops_on_the_short_page():
    """The probed WINQ26 case: 38 723 bars exist, 93 days were asked for."""
    term = FakeTerminal(38_723, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    rates, pages, partial = session.fetch_rates(FROM, NOW)
    check("young contract returns everything it has", len(rates) == 38_723, len(rates))
    check("young contract stops on the short page", pages == 2, pages)
    check("young contract is not partial", partial is False, partial)
    check(
        "no page exceeds the terminal cap",
        all(count <= term.maxbars for _, count in term.calls),
        term.calls,
    )


def test_full_history_covers_the_span():
    term = FakeTerminal(93 * 1440 + 5_000, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    rates, pages, partial = session.fetch_rates(FROM, NOW)
    covered = (rates[-1]["time"] - rates[0]["time"]) / 86_400
    check("full history covers the span", abs(covered - 93) < 0.01, covered)
    check("full history pages a handful of times", pages <= 10, pages)
    check("full history is not partial", partial is False, partial)
    check(
        "nothing older than the request survives",
        all(r["time"] >= FROM for r in rates),
        "clipped",
    )


def test_a_symbol_with_no_history():
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    rates, pages, partial = session.fetch_rates(FROM, NOW)
    check("empty symbol returns nothing", rates == [], rates)
    check("empty symbol asks once", pages == 1, pages)
    check("empty symbol is not partial", partial is False, partial)


def test_a_narrow_terminal_is_retried_at_half_the_page():
    """The setting one click away from reproducing the original bug."""
    term = FakeTerminal(50_000, NOW, maxbars=15_000)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    rates, _pages, _partial = session.fetch_rates(FROM, NOW)
    first, second = term.calls[0], term.calls[1]
    check("the first page is refused at the full width", first[1] == 20_000, first)
    check("the retry halves it", second[1] == 10_000, second)
    check(
        "every later page keeps the narrowed width",
        all(count == 10_000 for _, count in term.calls[1:]),
        term.calls,
    )
    check("and the walk still collects the history", len(rates) == 50_000, len(rates))


def test_a_page_failing_partway_reports_partial():
    term = FakeTerminal(100_000, NOW)
    term.fail_on = {2}
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    rates, _pages, partial = session.fetch_rates(FROM, NOW)
    check("a mid-walk failure keeps what was collected", len(rates) == 20_000, len(rates))
    check("a mid-walk failure is reported partial", partial is True, partial)


def test_the_budget_bounds_a_terminal_that_never_advances():
    """A terminal that answers in full but never goes further back.

    Note what this is *not* testing: a terminal returning one bar for a 20 000
    request is a short page, which the walk correctly reads as exhausted
    history and stops on. The budget guards the other shape — full pages that
    never reach further back, so no other stopping rule ever fires.
    """
    term = FakeTerminal(200_000, NOW, maxbars=100_000)
    bridge = load_bridge(term)
    session = session_for(bridge, term, rates_max_bars=10**9)
    calls: list[int] = []

    def stuck(_symbol, _timeframe, anchor, count):
        calls.append(int(anchor))
        # A full page, always the same one, whatever is asked for.
        return [{"time": NOW - i * M1} for i in range(count)][::-1]

    sys.modules["MetaTrader5"].copy_rates_from = stuck
    _rates, pages, partial = session.fetch_rates(FROM, NOW)
    check("the page budget binds", pages == bridge.RATES_MAX_PAGES, pages)
    check("and says the answer is short", partial is True, partial)
    check("it really did keep asking", len(calls) == bridge.RATES_MAX_PAGES, len(calls))


def test_the_partial_flag_reaches_the_wire():
    """rates_end carries `partial` only when something really is missing."""
    term = FakeTerminal(38_723, NOW)
    bridge = load_bridge(term)

    whole = session_for(bridge, term)
    whole.price = lambda v: f"{v:.2f}"
    whole.send_rates()
    ends = [m for m in whole.sent if m["type"] == "rates_end"]
    check("a whole block sends one rates_end", len(ends) == 1, ends)
    check("a whole block omits the flag", "partial" not in ends[0], ends[0])

    clipped = session_for(bridge, term, rates_max_bars=100)
    clipped.price = lambda v: f"{v:.2f}"
    clipped.send_rates()
    ends = [m for m in clipped.sent if m["type"] == "rates_end"]
    check("a clipped block flags itself partial", ends[0].get("partial") is True, ends[0])
    bars = sum(len(m["bars"]) for m in clipped.sent if m["type"] == "rate")
    check("a clipped block sends the capped count", bars == 100, bars)


def test_a_total_failure_still_delivers_the_empty_pair():
    term = FakeTerminal(1_000, NOW, maxbars=10)  # refuses even the minimum page
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    session.price = lambda v: f"{v:.2f}"
    session.send_rates()
    kinds = [m["type"] for m in session.sent]
    check("a total failure still brackets the absence", kinds == ["rates_start", "rates_end"], kinds)


def main() -> int:
    tests = [value for name, value in sorted(globals().items()) if name.startswith("test_")]
    for test in tests:
        print(f"{test.__name__}:")
        test()
    if FAILURES:
        print(f"\n{len(FAILURES)} check(s) failed:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print(f"\nall checks passed across {len(tests)} tests")
    return 0


if __name__ == "__main__":
    sys.exit(main())
