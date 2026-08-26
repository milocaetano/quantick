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


#: Aggressor/kind flags, as the real module exposes them. Only the bit the
#: bridge passes through matters here.
COPY_TICKS_ALL = 0
COPY_TICKS_TRADE = 1


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
        # Ticks the terminal holds, ascending by time_msc. Empty unless a test
        # gives it some; the candle tests never touch this half.
        self.ticks: list[dict] = []
        # Every (from_s, to_s, flags) the walk asked for.
        self.tick_calls: list[tuple[int, int, int]] = []
        # Every (from_s, count, flags) the live pump asked for.
        self.from_calls: list[tuple[int, int, int]] = []
        # Ranges answered with a failure, by 1-based call index.
        self.tick_fail_on: set[int] = set()

    def copy_ticks_range(self, _symbol, from_s, to_s, flags):
        self.tick_calls.append((int(from_s), int(to_s), int(flags)))
        if len(self.tick_calls) in self.tick_fail_on:
            self.error = (-1, "Terminal: no history")
            return None
        self.error = (0, "ok")
        lo, hi = int(from_s) * 1000, int(to_s) * 1000
        return [
            tick
            for tick in self.ticks
            if lo <= tick["time_msc"] <= hi
            and (flags != COPY_TICKS_TRADE or tick.get("is_trade", True))
        ]

    def copy_ticks_from(self, _symbol, from_s, count, flags):
        """The oldest tick held, and the live pump's own request."""
        self.error = (0, "ok")
        self.from_calls.append((int(from_s), int(count), int(flags)))
        lo = int(from_s) * 1000
        return [
            tick
            for tick in self.ticks
            if tick["time_msc"] >= lo
            and (flags != COPY_TICKS_TRADE or tick.get("is_trade", True))
        ][: int(count)]

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
    module.copy_ticks_range = terminal.copy_ticks_range
    module.copy_ticks_from = terminal.copy_ticks_from
    module.COPY_TICKS_ALL = COPY_TICKS_ALL
    module.COPY_TICKS_TRADE = COPY_TICKS_TRADE
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
    # The tick half's own state, which `__init__` would have set.
    session.seq = 0
    session.ticks_sent = 0
    session.earliest_ms = None
    session.earliest_known = False
    session.inbox = b""
    session.last_heartbeat = 0.0
    session.cursor_msc = 0
    session.sent_at_cursor = 0
    session.maybe_heartbeat = lambda: None
    return session


def tick_at(time_msc: int, last: float = 100.0, is_trade: bool = True) -> dict:
    """One terminal tick. `is_trade` decides whether COPY_TICKS_TRADE sees it."""
    return {
        "time_msc": time_msc,
        "bid": 99.0,
        "ask": 101.0,
        "last": last if is_trade else 0.0,
        "volume": 1 if is_trade else 0,
        "flags": 1080 if is_trade else 6,
        "is_trade": is_trade,
    }


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


def test_the_walk_takes_the_newest_ticks_before_the_cursor():
    term = FakeTerminal(0, NOW)
    # One tick every 100 ms for ten minutes, ending at the cursor.
    cursor = NOW * 1000
    term.ticks = [tick_at(cursor - offset) for offset in range(600_000, 0, -100)]
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    ticks, exhausted, scanned_to_ms, calls = session.walk_back(2_000, cursor)
    check("the page holds what was asked for", len(ticks) == 2_000, len(ticks))
    check(
        "the page is the newest 2000, not the oldest",
        ticks[-1]["time_msc"] == cursor - 100,
        ticks[-1]["time_msc"] - cursor,
    )
    check("the page is ascending", ticks == sorted(ticks, key=lambda t: t["time_msc"]))
    check("every tick is strictly older than the cursor", ticks[-1]["time_msc"] < cursor)
    check("one window sufficed", calls == 1, calls)
    check(
        "a trimmed page reports how far it really delivered",
        scanned_to_ms == ticks[0]["time_msc"],
        scanned_to_ms,
    )
    check(
        "a trimmed page never claims the end of the tape",
        exhausted is False,
        exhausted,
    )


def test_a_dense_first_window_does_not_claim_the_end_of_the_tape():
    """The defect this check exists for.

    The terminal's whole history sits inside one 300 s window, so the walk
    crosses the floor on its first call — while the surplus it found beyond
    `wanted` is trimmed off the front and never sent. Claiming `exhausted`
    there greys out the trader's button over ticks the terminal is holding.
    """
    term = FakeTerminal(0, NOW)
    cursor = NOW * 1000
    term.ticks = [tick_at(cursor - offset) for offset in range(200_000, 0, -100)]
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    ticks, exhausted, scanned_to_ms, _ = session.walk_back(500, cursor)
    check("the page is capped at what was asked", len(ticks) == 500, len(ticks))
    check(
        "the end of the tape is not claimed while ticks were trimmed",
        exhausted is False,
        exhausted,
    )
    check(
        "the reported reach stops at the oldest tick actually sent",
        scanned_to_ms == ticks[0]["time_msc"],
        scanned_to_ms,
    )


def test_reaching_the_terminal_floor_with_nothing_trimmed_is_the_end():
    term = FakeTerminal(0, NOW)
    cursor = NOW * 1000
    term.ticks = [tick_at(cursor - offset) for offset in (5_000, 4_000, 3_000)]
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    ticks, exhausted, _, _ = session.walk_back(2_000, cursor)
    check("everything the terminal had is sent", len(ticks) == 3, len(ticks))
    check("and only then is the end claimed", exhausted is True, exhausted)


def test_an_empty_stretch_widens_the_window_and_still_reports_its_reach():
    """A weekend. Nothing to find, and the walk must still make progress."""
    term = FakeTerminal(0, NOW)
    cursor = NOW * 1000
    # One tick far below, so a floor exists and the walk keeps going.
    term.ticks = [tick_at(cursor - 40 * 86_400 * 1000)]
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    ticks, exhausted, scanned_to_ms, calls = session.walk_back(2_000, cursor)
    check("nothing was found", len(ticks) == 0, len(ticks))
    check("the end was not claimed", exhausted is False, exhausted)
    check("the walk used more than one window", calls > 1, calls)
    check(
        "and reports a reach the consumer can page from",
        scanned_to_ms < cursor,
        (scanned_to_ms, cursor),
    )
    widths = [to_s - from_s for from_s, to_s, _ in term.tick_calls]
    check("the windows widened", widths[-1] > widths[0], widths[:3])
    check(
        "up to the documented ceiling",
        max(widths) <= bridge.LOAD_OLDER_MAX_WINDOW_S + 1,
        max(widths),
    )


def test_a_trades_tape_asks_the_terminal_for_trades_only():
    """The page size is labelled 'trades per load', so quotes must not fill it."""
    term = FakeTerminal(0, NOW)
    cursor = NOW * 1000
    term.ticks = [
        tick_at(cursor - offset, is_trade=(offset % 200 == 0))
        for offset in range(100_000, 0, -100)
    ]
    bridge = load_bridge(term)

    session = session_for(bridge, term)
    trades, _, _, _ = session.walk_back(100, cursor)
    check(
        "a trades tape asks for trades",
        all(flags == COPY_TICKS_TRADE for _, _, flags in term.tick_calls),
        term.tick_calls[:2],
    )
    check("and every tick it gets is one", all(t["is_trade"] for t in trades))

    # A broker-quoted symbol prints nothing, so there the quotes are the data.
    term.tick_calls.clear()
    quoted = session_for(bridge, term)
    quoted.tape = "quotes"
    quoted.walk_back(100, cursor)
    check(
        "a quoted symbol asks for everything",
        all(flags == COPY_TICKS_ALL for _, _, flags in term.tick_calls),
        term.tick_calls[:2],
    )


def test_a_failed_window_answers_with_what_is_in_hand():
    term = FakeTerminal(0, NOW)
    cursor = NOW * 1000
    term.ticks = [tick_at(cursor - offset) for offset in range(600_000, 0, -100)]
    term.tick_fail_on = {1}
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    ticks, exhausted, _, calls = session.walk_back(2_000, cursor)
    check("a failure ends the walk", calls == 1, calls)
    check("with nothing in hand", len(ticks) == 0, len(ticks))
    check("and no claim about the tape", exhausted is False, exhausted)


def test_the_block_is_announced_before_the_walk_and_always_bracketed():
    term = FakeTerminal(0, NOW)
    cursor = NOW * 1000
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    session.price = lambda v: f"{v:.2f}"

    session.serve_load_older(2_000, cursor)
    kinds = [m["type"] for m in session.sent]
    check("an empty block is still bracketed", kinds == ["history_start", "history_end"], kinds)
    check(
        "the start is announced before anything is known",
        "count_hint" not in session.sent[0],
        session.sent[0],
    )
    check(
        "the end carries the reach",
        "scanned_to_ms" in session.sent[-1],
        session.sent[-1],
    )


def test_a_command_that_is_not_an_object_does_not_kill_the_session():
    """PROTOCOL.md: one unreadable click must not cost the tick stream."""
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    class FakeSocket:
        def __init__(self, payload: bytes) -> None:
            self.payload = payload

        def recv(self, _n: int) -> bytes:
            chunk, self.payload = self.payload, b""
            return chunk

    # Valid JSON, none of it an object; `.get` on any of them would raise past
    # every `except` in the session loop.
    session.sock = FakeSocket(b'5\n"hi"\n[1,2]\nnull\n{"nope":1}\n')
    bridge.select.select = lambda r, w, x, t: ((r if session.sock.payload else []), [], [])
    try:
        session.pump_commands()
        survived = True
    except Exception as error:  # noqa: BLE001 — the point is that nothing escapes
        survived = False
        check("pump_commands survives non-object commands", False, repr(error))
    if survived:
        check("pump_commands survives non-object commands", True)
    check("and served nothing", session.sent == [], session.sent)


def test_an_endless_line_does_not_grow_the_buffer_without_bound():
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)

    class Flood:
        def recv(self, n: int) -> bytes:
            return b"x" * n

    session.sock = Flood()
    bridge.select.select = lambda r, w, x, t: (r, [], [])
    session.pump_commands()
    check(
        "the buffer is bounded",
        len(session.inbox) <= bridge.MAX_COMMAND_LINE_BYTES,
        len(session.inbox),
    )


def test_a_burst_larger_than_one_batch_is_drained_in_one_pass():
    """The delay this whole change exists to remove, at its source.

    `copy_ticks_from` returns at most the count it was asked for, so a pass that
    filled its batch has *not* caught up — it has been truncated. Forwarding one
    batch and returning left the remainder for the next pass, and on a burst
    that is how a tape falls whole seconds behind a book that never batches at
    all. The pass now keeps asking while the answers come back full.

    The Expert Advisor's `Pump()` carries the identical loop and the identical
    two bounds, and cargo can never compile MQL5 — so this is the only place
    either implementation's drain is executed by a test. Change one, change
    both, and change the numbers here with them.
    """
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    # Two and a bit batches' worth, all newer than the cursor.
    burst = bridge.TICKS_PER_PUMP_ROUND * 2 + 17
    term.ticks = [tick_at(1_000 + i, last=100.0 + (i % 5)) for i in range(burst)]

    session.pump_ticks()

    check(
        "the whole burst is forwarded in one pass",
        len(session.sent) == burst,
        f"{len(session.sent)} of {burst}",
    )
    check(
        "which took more than one request to the terminal",
        len(term.from_calls) == 3,
        term.from_calls,
    )
    check(
        "every request asked for a full batch",
        {count for _, count, _ in term.from_calls} == {bridge.TICKS_PER_PUMP_ROUND},
        term.from_calls,
    )
    check(
        "and the ticks arrive in order, none repeated",
        [msg["time_ms"] for msg in session.sent] == [1_000 + i for i in range(burst)],
        session.sent[:3],
    )
    check(
        "one stamp for the pass, on every line",
        len({msg["sent_ms"] for msg in session.sent}) == 1,
        session.sent[0]["sent_ms"],
    )

    # A second pass has nothing left to say: the cursor carries (msc,
    # count-at-msc), so no tick is ever sent twice.
    before = len(session.sent)
    session.pump_ticks()
    check("a drained pump repeats nothing", len(session.sent) == before, len(session.sent))


def test_a_terminal_that_never_advances_cannot_own_the_pump():
    """The bound under the loop above.

    A terminal that keeps answering with ticks the cursor has already passed
    would otherwise be asked forever, and this pump shares its thread with the
    book and the heartbeat. The pass gives up, says so, and comes back next
    time — the EA logs the same `BRIDGE_PUMP_ROUND_LIMIT`.
    """
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    # Every answer is a full batch of the *same* instant, so the cursor can
    # never move past it: the (msc, count-at-msc) rule sends each exactly once
    # and then has nothing new, which is the shape that used to spin.
    term.ticks = [tick_at(1_000, last=100.0) for _ in range(bridge.TICKS_PER_PUMP_ROUND)]

    session.pump_ticks()

    check(
        "the pass ends rather than spinning",
        len(term.from_calls) <= bridge.MAX_PUMP_ROUNDS,
        len(term.from_calls),
    )
    check(
        "having forwarded that instant's ticks exactly once",
        len(session.sent) == bridge.TICKS_PER_PUMP_ROUND,
        len(session.sent),
    )


def test_the_live_pump_asks_only_for_prints():
    """The tape's own latency, decided one line earlier than anyone looks.

    quantick charts a printing venue from `last` and `volume` and throws away
    every tick without a LAST bit (crates/feed-mt5/src/map.rs). Asking the
    terminal for those quotes anyway put them on the same socket as the prints,
    ahead of the prints queued behind them — and on WIN they outnumber the
    prints several times over. The delay that builds is invisible in the book,
    which restamps itself on the way out, and fully visible in the bubbles,
    which carry the instant they traded: they drift left of the tape's edge
    until they fall off it.
    """
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_for(bridge, term)
    term.ticks = [
        tick_at(1_000, is_trade=False),
        tick_at(1_100, last=100.5),
        tick_at(1_200, is_trade=False),
        tick_at(1_300, is_trade=False),
        tick_at(1_400, last=100.5),
    ]
    session.pump_ticks()
    check(
        "a printing venue asks for prints only",
        [flags for _, _, flags in term.from_calls] == [COPY_TICKS_TRADE],
        term.from_calls,
    )
    check("only the prints are forwarded", len(session.sent) == 2, len(session.sent))
    check(
        "and they are the prints, in order",
        [msg["time_ms"] for msg in session.sent] == [1_100, 1_400],
        session.sent,
    )

    # A broker-quoted symbol prints nothing at all, so there the quotes *are*
    # the tape and every tick is still wanted.
    quoting = session_for(bridge, term)
    quoting.tape = "quotes"
    quoting.pump_ticks()
    check(
        "a quote-only venue still asks for everything",
        term.from_calls[-1][2] == COPY_TICKS_ALL,
        term.from_calls[-1],
    )
    check(
        "and gets every tick",
        len(quoting.sent) == len(term.ticks),
        len(quoting.sent),
    )


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
