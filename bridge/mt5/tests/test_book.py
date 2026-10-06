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
    for seconds in range(1, 36):
        clock[0] = 105.05 + seconds
        session.pump_book()
        assert clock[0] * 1000 - session.last_book_ms <= 6000, "valid DOM freshness remains below the stale deadline"
    check("unchanged valid DOM stays confirmed across the stale deadline", 8 <= session.book_sent <= 9, session.sent)
    check("refresh timestamps keep the existing source observation policy", all(msg["time_ms"] == NOW * 1000 for msg in session.sent if msg["type"] == "book"), session.sent)
    previously_sent = session.book_sent
    rows.clear()
    clock[0] = 141.0
    session.pump_book()
    rows.extend(previous_rows)
    clock[0] = 146.0
    session.pump_book()
    check("identical depth after an empty interval still recovers", session.book_sent == previously_sent + 1, session.sent)


if __name__ == "__main__":
    run_tests(globals())
