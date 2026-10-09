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
    patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0], perf_counter=lambda: clock[0]))
    session = session_at(bridge, term, NOW)
    patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0], perf_counter=lambda: clock[0]))
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
        patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0], perf_counter=lambda: clock[0]))
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

    `wall_s[0]` drives `time.time`, `time.monotonic` and `time.perf_counter`,
    so the local server clock and the confirm cadence advance together, as
    they do live.
    """
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW)
    patch_bridge(
        "time",
        types.SimpleNamespace(time=lambda: wall_s[0], monotonic=lambda: wall_s[0], perf_counter=lambda: wall_s[0]),
    )
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


def test_the_confirm_cadence_does_not_inherit_a_coarse_clock():
    """Windows' `time.monotonic` is GetTickCount64, which moves in 15.625 ms steps.

    Measured live: a 50 ms cadence read off it confirmed every 62.5 ms, and
    the 100 ms one every 109 ms, because the first step at or past the
    interval is four (seven) ticks away. The cadence needs a clock finer than
    the interval it keeps.
    """
    wall_s = [float(NOW)]
    session, module = _confirming_session(wall_s)
    tick_s = 0.015625
    patch_bridge(
        "time",
        types.SimpleNamespace(
            time=lambda: wall_s[0],
            monotonic=lambda: (wall_s[0] // tick_s) * tick_s,
            perf_counter=lambda: wall_s[0],
        ),
    )
    module.symbol_info_tick = lambda _symbol: None

    sent_at = []
    for milliseconds in range(0, 1000, 5):
        wall_s[0] = NOW + milliseconds / 1000
        before = session.book_sent
        session.pump_book()
        if session.book_sent > before:
            sent_at.append(milliseconds)
    gaps = [later - earlier for earlier, later in zip(sent_at, sent_at[1:])]
    check("a confirmation is due one read after the interval, not one coarse tick", all(gap <= 55 for gap in gaps), gaps)


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
    patch_bridge("time", types.SimpleNamespace(time=lambda: float(NOW), monotonic=lambda: clock[0], perf_counter=lambda: clock[0]))
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


def _stamp_run(host_s, print_at, duration_ms):
    """Pump a standing DOM every 5 ms for `duration_ms` and read each stamp.

    `host_s(t)` is this host's wall clock at `t` ms of real time; the confirm
    cadence runs on real time, so a step in the wall clock does not move it.
    `print_at(t)` is the `time_msc` of the terminal's newest print at `t`, or
    `None` before the first. Returns `(t, stamp, bound)` per image, where
    `bound` is the furthest any print received so far plus the real time
    since it arrived could reach: the latest moment a print has vouched for.
    """
    real_s = [0.0]
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session = session_at(bridge, term, NOW)
    patch_bridge(
        "time",
        types.SimpleNamespace(
            time=lambda: host_s(real_s[0] * 1000),
            monotonic=lambda: 500.0 + real_s[0],
            perf_counter=lambda: 500.0 + real_s[0],
        ),
    )
    session.offset_s = 0
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
    shown = [None]
    module.symbol_info_tick = lambda _symbol: (
        None if shown[0] is None else types.SimpleNamespace(time_msc=shown[0])
    )
    arrived = []  # (time_msc, real ms it first showed)
    out = []
    for t in range(0, duration_ms, 5):
        real_s[0] = t / 1000
        newest = print_at(t)
        if newest is not None and newest != shown[0]:
            arrived.append((newest, t))
        shown[0] = newest
        before = session.book_sent
        session.pump_book()
        if session.book_sent > before:
            bound = max((msc + t - at for msc, at in arrived), default=None)
            out.append((t, session.sent[-1]["time_ms"], bound))
    return out


def _ahead(run):
    return [(t, stamp, bound) for t, stamp, bound in run if bound is not None and stamp > bound]


def test_a_print_from_the_future_does_not_leave_a_lasting_lead():
    """One print with a bad `time_msc` must not move every later stamp ahead.

    The learned lead never fell: a print 5 s in the future kept every later
    book image 5 s ahead of the tape, claiming depth at moments no print had
    reached. Its stamps may hold at that print's time, never above it plus the
    real time since, and once real prints pass it they lead again.
    """
    base = NOW * 1000

    def print_at(t):
        if t < 1000:
            return base + 5000  # one bad print, five seconds early
        return base + (t // 100) * 100  # then the honest tape, every 100 ms

    run = _stamp_run(lambda t: NOW + t / 1000, print_at, 10_000)
    check("no stamp is ahead of a print plus the real time since it", not _ahead(run), _ahead(run)[:5])
    late = [(t, stamp) for t, stamp, _ in run if t >= 8000]
    check(
        "after the real tape passes the bad print, stamps follow the real tape",
        all(stamp <= base + t for t, stamp in late) and all(stamp >= base + t - 100 for t, stamp in late),
        late[:5],
    )


def test_a_host_clock_that_catches_up_leaves_no_stamp_ahead_of_the_prints():
    """An NTP step forward on this host must not push book stamps past the tape.

    The host starts 800 ms behind the terminal, then steps to the terminal's
    time at 2 s. A lead learned before the step, carried after it, would put
    every image 800 ms ahead of the newest print.
    """
    base = NOW * 1000

    def host_s(t):
        return NOW + (t - 800) / 1000 if t < 2000 else NOW + t / 1000

    run = _stamp_run(host_s, lambda t: base + (t // 300) * 300, 5000)
    check("no stamp is ahead of a print plus the real time since it", not _ahead(run), _ahead(run)[:5])


def test_stamps_advance_between_prints_at_the_confirm_cadence():
    """Confirmations between two prints each carry a newer stamp, 50 ms on."""
    base = NOW * 1000
    run = _stamp_run(lambda t: NOW + t / 1000, lambda t: base if t < 1000 else base + 1000, 2000)
    stamps = [stamp for _, stamp, _ in run]
    gaps = [b - a for a, b in zip(stamps, stamps[1:])]
    check("twenty images a second", len(run) == 40, len(run))
    check("each confirmation stamp is at least 50 ms newer", all(gap >= 50 for gap in gaps), gaps)
    check("no stamp is ahead of a print plus the real time since it", not _ahead(run), _ahead(run)[:5])


def test_stamps_never_go_backwards():
    """A print corrected backwards, or a host clock stepped back, never rewinds the book.

    Before the first print the stamp is the host clock, as it always was.
    """
    base = NOW * 1000

    def print_at(t):
        if t < 300:
            return None
        if t < 1000:
            return base + 600
        return base + 200  # the terminal corrects its newest print backwards

    run = _stamp_run(lambda t: NOW + t / 1000 if t < 1500 else NOW + (t - 1000) / 1000, print_at, 3000)
    stamps = [stamp for _, stamp, _ in run]
    check("before any print the stamp is the host clock", run[0][1] == base, run[:2])
    check("stamps never decrease", all(b >= a for a, b in zip(stamps, stamps[1:])), stamps)


if __name__ == "__main__":
    raise SystemExit(run_tests(globals()))
