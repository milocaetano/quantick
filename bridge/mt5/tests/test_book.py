"""Missing DOM polls back off while ticks continue and depth recovers."""

import sys
import types
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import (  # noqa: E402
    FakeTerminal,
    check,
    load_bridge,
    patch_bridge,
    run_tests,
    session_at,
    tick_at,
)

NOW = 1_784_824_260


def test_missing_book_retries_quietly_and_recovers_without_resubscribing():
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    clock = [100.0]
    patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0]))
    session = session_at(bridge, term, NOW)
    patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0]))
    session.book_subscribed = True
    session.last_book_body = None
    session.last_book_ms = 0.0
    session.book_seq = session.book_sent = session.book_skipped = 0
    session.args.book_min_interval_ms = 20
    module = sys.modules["MetaTrader5"]
    module.BOOK_TYPE_BUY = 2
    module.BOOK_TYPE_SELL = 1
    calls = []
    rows = []
    module.market_book_get = lambda symbol: (calls.append(symbol), rows)[1]
    module.symbol_info_tick = lambda _symbol: None

    session.pump_book()
    for milliseconds in range(1, 5000, 50):
        clock[0] = 100.0 + milliseconds / 1000
        session.pump_book()
    check("missing depth uses one quiet probe per five seconds", len(calls) == 1, calls)
    check("no empty image claims synchronized depth", session.book_sent == 0, session.sent)

    term.ticks = [tick_at(NOW * 1000), tick_at(NOW * 1000 + 1)]
    session.pump_ticks()
    check("live ticks still run during book backoff", session.ticks_sent == 2, session.ticks_sent)

    clock[0] = 105.0
    rows.extend([
        types.SimpleNamespace(type=2, price=100, volume_dbl=3, volume=3),
        types.SimpleNamespace(type=1, price=105, volume_dbl=4, volume=4),
    ])
    session.pump_book()
    check("next quiet probe recovers depth", session.book_sent == 1, session.sent)
    clock[0] = 105.05
    rows[0].volume_dbl = 9
    session.pump_book()
    check("available depth returns to the normal fast cadence", session.book_sent == 2, session.sent)
    check("the existing subscription survives backoff", session.book_subscribed, session.book_subscribed)
    previous_rows = list(rows)
    read_step_ms = 50
    sent_at = [105_050]
    for milliseconds in range(105_050 + read_step_ms, 140_050 + 1, read_step_ms):
        clock[0] = milliseconds / 1000
        before = session.book_sent
        session.pump_book()
        if session.book_sent > before:
            sent_at.append(milliseconds)
        assert clock[0] * 1000 - session.last_book_ms <= 6000, "valid DOM freshness remains below the stale deadline"
    gaps = [later - earlier for earlier, later in zip(sent_at, sent_at[1:])]
    check("unchanged valid DOM is confirmed throughout", len(gaps) > 0, sent_at)
    check("confirmations are at least a confirm interval apart", all(gap >= 50 for gap in gaps), gaps)
    check("no confirmation is later than one read past the interval", all(gap <= 50 + read_step_ms for gap in gaps), gaps)
    check("refresh timestamps keep the existing source observation policy", all(msg["time_ms"] == NOW * 1000 for msg in session.sent if msg["type"] == "book"), session.sent)
    previously_sent = session.book_sent
    rows.clear()
    clock[0] = 141.0
    session.pump_book()
    rows.extend(previous_rows)
    clock[0] = 146.0
    session.pump_book()
    check("identical depth after an empty interval still recovers", session.book_sent == previously_sent + 1, session.sent)


def test_zero_only_depth_notifies_then_probes_quietly_and_recovers():
    for zero_volume in (0, 0.001):
        # A subprecision quantity also serializes as zero: the receiver cannot
        # use liquidity that the wire image does not actually carry.
        term = FakeTerminal(0, NOW)
        bridge = load_bridge(term)
        clock = [100.0]
        session = session_at(bridge, term, NOW)
        patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0]))
        session.book_subscribed = True
        session.last_book_body = None
        session.last_book_ms = 0.0
        session.book_seq = session.book_sent = session.book_skipped = 0
        session.args.book_min_interval_ms = 20
        module = sys.modules["MetaTrader5"]
        module.BOOK_TYPE_BUY = 2
        module.BOOK_TYPE_SELL = 1
        rows = [
            types.SimpleNamespace(type=2, price=100, volume_dbl=3, volume=3),
            types.SimpleNamespace(type=1, price=105, volume_dbl=4, volume=4),
        ]
        calls = []
        module.market_book_get = lambda symbol: (calls.append(symbol), rows)[1]
        module.symbol_info_tick = lambda _symbol: None
        session.pump_book()
        assert session.book_sent == 1

        clock[0] = 100.05
        for row in rows:
            row.volume_dbl = zero_volume
            row.volume = 0
        session.pump_book()
        assert session.book_sent == 2, "the zero-depth transition is still sent"
        image = session.sent[-1]
        assert image["bids"][0][0] == "100" and float(image["bids"][0][1]) == 0
        assert image["asks"][0][0] == "105" and float(image["asks"][0][1]) == 0
        for milliseconds in range(50, 5000, 50):
            clock[0] = 100.05 + milliseconds / 1000
            session.pump_book()
        assert len(calls) == 2, "zero-only DOM is probed once, then quiet for five seconds"
        assert session.book_sent == 2

        clock[0] = 105.05
        session.pump_book()
        assert len(calls) == 3, "still unavailable DOM gets one five-second probe"
        assert session.book_sent == 3, "the probe retains explicit zero-depth semantics"
        rows[0].volume_dbl = 3
        clock[0] = 110.05
        session.pump_book()
        assert session.book_sent == 4, "positive depth recovers at the next quiet probe"
        assert float(session.sent[-1]["asks"][0][1]) == 0, "mixed images retain zero rows"
        rows[0].volume_dbl = 7
        clock[0] = 110.1
        session.pump_book()
        assert session.book_sent == 5, "mixed usable depth resumes the normal fast cadence"
        for seconds in range(1, 36):
            clock[0] = 110.1 + seconds
            session.pump_book()
            assert clock[0] * 1000 - session.last_book_ms <= 6000
        assert session.book_sent >= 11, "unchanged valid depth stays confirmed beyond thirty seconds"
        assert session.book_subscribed, "no resubscription is needed for recovery"



def _confirming_session(wall_s):
    """A subscribed session over a standing two-row DOM, its clocks in `wall_s`.

    `wall_s[0]` drives both `time.time` and `time.monotonic`, so the local
    server clock and the confirm cadence advance together, as they do live.
    """
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW)
    patch_bridge("time", types.SimpleNamespace(time=lambda: wall_s[0], monotonic=lambda: wall_s[0]))
    session.book_subscribed = True
    session.last_book_body = None
    session.last_book_ms = 0.0
    session.book_seq = session.book_sent = session.book_skipped = 0
    session.args.book_min_interval_ms = 20
    module = sys.modules["MetaTrader5"]
    module.BOOK_TYPE_BUY = 2
    module.BOOK_TYPE_SELL = 1
    rows = [
        types.SimpleNamespace(type=2, price=100, volume_dbl=3, volume=3),
        types.SimpleNamespace(type=1, price=105, volume_dbl=4, volume=4),
    ]
    module.market_book_get = lambda _symbol: rows
    return session, module


def test_confirmations_advance_with_the_clock_when_the_terminal_leads_it():
    """A book stamp moves at every confirmation, even between prints.

    Measured on WINV26: the terminal's tick times ran ~0.8 s ahead of this
    host's clock plus the snapped UTC offset. Stamping `max(last tick, local
    now)` then froze every confirmation at the newest print's time until the
    next print, so the mapper saw equal stamps, dropped them, and the book
    clock stood still between prints. The lead the ticks reveal is learned
    and carried forward instead.
    """
    wall_s = [float(NOW)]
    session, module = _confirming_session(wall_s)
    lead_ms = 800
    module.symbol_info_tick = lambda _symbol: types.SimpleNamespace(time_msc=NOW * 1000 + lead_ms)

    for milliseconds in range(0, 1000, 5):
        wall_s[0] = NOW + milliseconds / 1000
        session.pump_book()
    stamps = [msg["time_ms"] for msg in session.sent if msg["type"] == "book"]
    check("every confirmation has a newer stamp", all(b > a for a, b in zip(stamps, stamps[1:])), stamps)
    check(
        "the stamp is the local clock carried by the lead the ticks revealed",
        stamps[-1] == NOW * 1000 + lead_ms + 950,
        stamps,
    )
    check("no stamp is older than the newest tick", all(stamp >= NOW * 1000 + lead_ms for stamp in stamps), stamps)


def test_unchanged_depth_is_confirmed_twenty_times_a_second():
    """A short tape needs the book clock to move while the DOM stands still.

    Measured on WINV26: the DOM changes about three times a second, so a
    bridge that resends an unchanged image only every five seconds leaves the
    book clock up to a second behind the prints, and one every 100 ms still
    left the heat a median ~90 ms short of a 200 ms tape. Each confirmation
    is a fresh read of the terminal, so it is an observation, not a guess.
    """
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    clock = [100.0]
    session = session_at(bridge, term, NOW)
    patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0]))
    session.book_subscribed = True
    session.last_book_body = None
    session.last_book_ms = 0.0
    session.book_seq = session.book_sent = session.book_skipped = 0
    session.args.book_min_interval_ms = 20
    module = sys.modules["MetaTrader5"]
    module.BOOK_TYPE_BUY = 2
    module.BOOK_TYPE_SELL = 1
    rows = [
        types.SimpleNamespace(type=2, price=100, volume_dbl=3, volume=3),
        types.SimpleNamespace(type=1, price=105, volume_dbl=4, volume=4),
    ]
    module.market_book_get = lambda _symbol: rows
    module.symbol_info_tick = lambda _symbol: None

    sent_at = []
    for milliseconds in range(0, 1000, 5):
        clock[0] = 100.0 + milliseconds / 1000
        before = session.book_sent
        session.pump_book()
        if session.book_sent > before:
            sent_at.append(milliseconds)
    check("the first image and nineteen confirmations in one second", len(sent_at) == 20, sent_at)
    gaps = [later - earlier for earlier, later in zip(sent_at, sent_at[1:])]
    check("never more than one confirmation per 50 ms", all(gap >= 50 for gap in gaps), gaps)


if __name__ == "__main__":
    raise SystemExit(run_tests(globals()))
