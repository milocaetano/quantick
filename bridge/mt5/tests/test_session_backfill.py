"""Where the opening block starts.

The bug these exist for: the opening backfill asked the terminal for
`now - backfill_minutes`, a rolling clock window. Its docstring and the README
both called that "a whole B3 session", and it only is while the clock happens
to read between roughly 18:25 and 21:00. Measured against a live terminal on
2026-08-31 at 22:10 local, WINV26's session ran 09:03:00 to 18:31:23 and the
720-minute window returned its oldest tick at 13:10 — four hours of the day
missing, with nothing on screen to say so.

The anchor is now the *tape*: the walk steps back from the newest print in
windows and stops at the first one holding nothing, because a stretch with no
prints longer than `SESSION_GAP_MS` is the market having been closed. That is
the same rule the app already applies in `crate::history_reach`, and
`crates/app/tests/session_gap_agreement.rs` fails if the two values drift.

Run directly (`python bridge/mt5/tests/test_session_backfill.py`) or through
`cargo test -p quantick-feed-mt5 --test bridge_paging`, which discovers and
runs every suite in this folder so the four checks cover them.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import (  # noqa: E402  (deliberately after the path insert)
    FakeTerminal,
    block_ticks,
    check,
    load_bridge,
    run_tests,
    session_at,
    tick_at,
)

#: An epoch second at midnight, so the tests can talk in hours of a day.
MIDNIGHT = 1_784_764_800

HOUR = 3_600
MINUTE = 60


def at(hour: int, minute: int = 0, day: int = 0) -> int:
    """The epoch second of `hour:minute` on `day` days after MIDNIGHT."""
    return MIDNIGHT + day * 24 * HOUR + hour * HOUR + minute * MINUTE


def session_between(first_s: int, last_s: int, step_s: int = MINUTE) -> list[dict]:
    """Prints every `step_s` from `first_s` to `last_s`, ascending.

    A step well under `SESSION_GAP_MS` is the point: the walk must not read an
    ordinary quiet minute as a close. One a minute over a nine-hour session is
    570 prints, which keeps the fixtures cheap to build and read.
    """
    return [tick_at(t * 1000) for t in range(first_s, last_s + 1, step_s)]


#: B3's mini index, as the live probe found it: 09:03 to 18:31.
OPEN_H, OPEN_M = 9, 3
CLOSE_H, CLOSE_M = 18, 31


def b3_day(day: int = 0) -> list[dict]:
    return session_between(at(OPEN_H, OPEN_M, day), at(CLOSE_H, CLOSE_M, day))


def first_tick_ms(session) -> int | None:
    ticks = block_ticks(session)
    return ticks[0]["time_ms"] if ticks else None


def last_tick_ms(session) -> int | None:
    ticks = block_ticks(session)
    return ticks[-1]["time_ms"] if ticks else None


def test_the_evening_opens_on_the_whole_session():
    """The reported defect, as a test.

    22:10 on a trading day. `now - 720 min` is 10:10, which would cut the
    morning off exactly the way the trader saw it. The block must start at the
    session's first print instead.
    """
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day()
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "the block starts at the session's first print",
        first_tick_ms(session) == at(OPEN_H, OPEN_M) * 1000,
        first_tick_ms(session),
    )
    check(
        "the block ends at the session's last print",
        last_tick_ms(session) == at(CLOSE_H, CLOSE_M) * 1000,
        last_tick_ms(session),
    )
    check(
        "the whole session is on the chart",
        len(block_ticks(session)) == len(term.ticks),
        len(block_ticks(session)),
    )
    check(
        "the clock window no longer decides where the day starts",
        first_tick_ms(session) < (now_s - 720 * MINUTE) * 1000,
        first_tick_ms(session),
    )


def test_opening_mid_session_still_reaches_the_open():
    """11:00, two hours in. The rolling window would have covered it by luck;
    the anchor must be the open either way."""
    now_s = at(11, 0)
    term = FakeTerminal(0, now_s)
    term.ticks = session_between(at(OPEN_H, OPEN_M), now_s)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "a chart opened mid-session starts at the open",
        first_tick_ms(session) == at(OPEN_H, OPEN_M) * 1000,
        first_tick_ms(session),
    )


def test_before_the_open_reaches_the_previous_session_whole():
    """07:00, the market shut. The session to read is yesterday's, entire —
    not the tail of it that a fixed window happens to cover."""
    now_s = at(7, 0, day=1)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day(day=0)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "the previous session arrives from its own open",
        first_tick_ms(session) == at(OPEN_H, OPEN_M) * 1000,
        first_tick_ms(session),
    )
    check(
        "the previous session arrives whole",
        len(block_ticks(session)) == len(term.ticks),
        len(block_ticks(session)),
    )


def test_a_weekend_reaches_fridays_whole_session():
    """Sunday evening. Friday is two days back and the terminal has it."""
    friday = b3_day(day=0)
    now_s = at(20, 0, day=2)
    term = FakeTerminal(0, now_s)
    term.ticks = friday
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "Friday's session arrives from its open",
        first_tick_ms(session) == at(OPEN_H, OPEN_M) * 1000,
        first_tick_ms(session),
    )
    check(
        "Friday's session arrives whole",
        len(block_ticks(session)) == len(friday),
        len(block_ticks(session)),
    )


def test_yesterdays_session_is_not_dragged_in_with_todays():
    """Two sessions on disk, an overnight gap between them. Opening today must
    stop at today's open: the day before is what `+ older` is for."""
    now_s = at(22, 10, day=1)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day(day=0) + b3_day(day=1)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "the block stops at today's open",
        first_tick_ms(session) == at(OPEN_H, OPEN_M, day=1) * 1000,
        first_tick_ms(session),
    )
    check(
        "yesterday is left for the older button",
        len(block_ticks(session)) == len(b3_day(day=1)),
        len(block_ticks(session)),
    )


def test_a_market_that_never_closes_is_bounded_rather_than_endless():
    """A CFD quoting around the clock has no gap to stop on. The walk must
    stop on its own budget, with a full chart and a bounded number of calls."""
    now_s = at(12, 0, day=3)
    term = FakeTerminal(0, now_s)
    term.ticks = session_between(MIDNIGHT, now_s)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "a continuous tape still fills the chart",
        len(block_ticks(session)) > 0,
        len(block_ticks(session)),
    )
    check(
        "a continuous tape does not walk forever",
        len(term.tick_calls) <= bridge.SESSION_WALK_MAX_WINDOWS + 2,
        len(term.tick_calls),
    )
    check(
        "a continuous tape stops within its span budget",
        last_tick_ms(session) - first_tick_ms(session)
        <= bridge.SESSION_WALK_MAX_WINDOWS * bridge.SESSION_GAP_MS,
        (last_tick_ms(session) - first_tick_ms(session)) / 3_600_000,
    )


def test_a_terminal_holding_less_than_one_session_sends_all_of_it():
    """A contract listed this morning: twenty minutes exist and twenty minutes
    is the right answer, with no failure and no invented history."""
    now_s = at(9, 23)
    ticks = session_between(at(9, 3), now_s)
    term = FakeTerminal(0, now_s)
    term.ticks = ticks
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "a young contract sends everything it has",
        len(block_ticks(session)) == len(ticks),
        len(block_ticks(session)),
    )
    check(
        "a young contract starts at its first print",
        first_tick_ms(session) == at(9, 3) * 1000,
        first_tick_ms(session),
    )


def test_a_symbol_the_terminal_has_never_held_sends_an_empty_block():
    """Nothing to anchor on. The block is still bracketed, because
    `backfill_end` is what the feed's loader waits on."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = []
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check("an empty symbol sends no ticks", block_ticks(session) == [], "ticks sent")
    check(
        "an empty symbol is still bracketed",
        [m["type"] for m in session.sent if m["type"].startswith("backfill")]
        == ["backfill_start", "backfill_end"],
        [m["type"] for m in session.sent],
    )


def test_the_block_is_ascending_and_free_of_repeats():
    """The walk collects newest window first and the block must still leave in
    tape order, with no tick counted twice at a window's seam."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day()
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    stamps = [t["time_ms"] for t in block_ticks(session)]
    check("the block is ascending", stamps == sorted(stamps), "out of order")
    check("no tick is sent twice", len(stamps) == len(set(stamps)), len(stamps))


def test_the_cursor_lands_on_the_last_tick_sent():
    """The live pump starts from where the block ended, or the first live tick
    re-sends the session's tail."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day()
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "the cursor is the newest tick sent",
        session.cursor_msc == last_tick_ms(session),
        (session.cursor_msc, last_tick_ms(session)),
    )


def test_a_session_beyond_the_cap_keeps_the_newest_and_says_so():
    """The cap is a bound on memory, never on the span. When it does bite, the
    trader is told — a silent amputation is the defect this branch exists for,
    one layer down."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day()
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720, backfill_max_ticks=100)
    logged: list[tuple[str, dict]] = []
    bridge.log = lambda code, **fields: logged.append((code, fields))
    session.backfill()

    check(
        "the cap keeps exactly what it promised",
        len(block_ticks(session)) == 100,
        len(block_ticks(session)),
    )
    check(
        "the newest ticks are the ones kept",
        last_tick_ms(session) == at(CLOSE_H, CLOSE_M) * 1000,
        last_tick_ms(session),
    )
    truncations = [fields for code, fields in logged if code == "BRIDGE_BACKFILL_TRUNCATED"]
    check("a cut is reported", len(truncations) == 1, [c for c, _ in logged])
    # Deliberately not "how many were dropped". Once the cap stops the walk the
    # bridge has not seen the rest of the session, and the only way to count
    # what it left behind would be to fetch exactly the memory the cap exists
    # to refuse. What it can say honestly is that a cap did this, how much is
    # on the wire, and where the remainder is — so that is what it says.
    cut = truncations[0] if truncations else {}
    check("the cut names the cap as its reason", cut.get("stopped_on") == "cap", cut)
    check("the cut says how much it sent", cut.get("sending") == 100, cut)
    check("the cut keeps the newest", cut.get("action") == "keep_newest", cut)
    check(
        "the cut points at the way back to the rest",
        cut.get("recoverable") == "load_older",
        cut,
    )


def test_a_terminal_that_misreports_its_oldest_tick_is_not_believed():
    """The failure that shipped an empty chart on a 1 525 621-print day.

    `copy_ticks_from(symbol, 0, 1, COPY_TICKS_ALL)` is documented as the oldest
    tick the terminal holds. A real MT5 answered 19:30 *that evening* for
    WINV26 on 2026-08-31, while holding — and serving range queries about —
    the whole session below it. Believed, that floor stops the search one step
    in: it looked at 19:03, compared against 19:30, concluded the symbol had no
    history and sent `backfill_start`/`backfill_end` with nothing between them.

    This is the case that makes the whole branch worth having, because it fails
    the way the trader's chart failed: not with an error, but with an empty
    chart that looks exactly like a market nobody traded.
    """
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day()
    bridge = load_bridge(term)
    bogus_ms = at(19, 30) * 1000
    bridge.mt5.copy_ticks_from = lambda _symbol, _from, _count, _flags: [tick_at(bogus_ms)]
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "a floor with the session underneath it does not empty the chart",
        len(block_ticks(session)) == len(term.ticks),
        len(block_ticks(session)),
    )
    check(
        "and the block still starts at the open",
        first_tick_ms(session) == at(OPEN_H, OPEN_M) * 1000,
        first_tick_ms(session),
    )


def test_a_real_floor_is_still_honoured():
    """The check falsifies a claim; it does not throw the floor away.

    A terminal whose oldest tick really is its oldest tick must still stop the
    walk there, or every symbol with a short history spends the whole window
    budget proving the same thing.
    """
    now_s = at(11, 0)
    term = FakeTerminal(0, now_s)
    term.ticks = session_between(at(9, 3), now_s)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check(
        "the honest floor is believed",
        session.earliest_ms == at(9, 3) * 1000,
        session.earliest_ms,
    )
    check(
        "and the walk stops on it rather than on its budget",
        len(block_ticks(session)) == len(term.ticks),
        len(block_ticks(session)),
    )


def drain_opening(session) -> list[list[dict]]:
    """Every parked slice, in the order the loop would send them."""
    session.sent.clear()
    blocks: list[list[dict]] = []
    while session.pending_opening:
        session.sent.clear()
        session.pump_opening()
        blocks.append([m for m in session.sent if m["type"] == "tick"])
    return blocks


def test_a_session_larger_than_a_slice_opens_on_its_newest_part():
    """D2: the chart paints on the newest slice rather than waiting for the
    whole session, which on a real contract is eleven seconds of nothing."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = session_between(at(OPEN_H, OPEN_M), at(CLOSE_H, CLOSE_M), step_s=10)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720, opening_slice_ticks=500)
    session.backfill()

    opened = block_ticks(session)
    check("the opening block is one slice", len(opened) == 500, len(opened))
    check(
        "and it is the newest end of the session",
        opened[-1]["time_ms"] == at(CLOSE_H, CLOSE_M) * 1000,
        opened[-1]["time_ms"],
    )
    check(
        "the rest is parked, not dropped",
        sum(len(s) for s in session.pending_opening) == len(term.ticks) - 500,
        sum(len(s) for s in session.pending_opening),
    )


def test_the_parked_slices_rebuild_the_session_exactly():
    """Nothing lost, nothing doubled, nothing out of order — the whole point of
    slicing is that it is invisible in the result."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = session_between(at(OPEN_H, OPEN_M), at(CLOSE_H, CLOSE_M), step_s=10)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720, opening_slice_ticks=500)
    session.backfill()
    opened = [m["time_ms"] for m in block_ticks(session)]

    blocks = drain_opening(session)
    older = [m["time_ms"] for block in blocks for m in block]
    rebuilt = sorted(older + opened)
    expected = [t["time_msc"] for t in term.ticks]

    check("every print arrives exactly once", rebuilt == expected, len(rebuilt))
    check(
        "each slice is older than the one before it",
        all(
            max(m["time_ms"] for m in blocks[i])
            < min(m["time_ms"] for m in blocks[i - 1])
            for i in range(1, len(blocks))
            if blocks[i] and blocks[i - 1]
        ),
        "slices out of order",
    )
    check(
        "and the first parked slice is older than what opened the chart",
        max(m["time_ms"] for m in blocks[0]) < min(opened),
        (max(m["time_ms"] for m in blocks[0]), min(opened)),
    )


def test_each_slice_is_marked_and_counts_down():
    """The app must be able to tell an opening slice from the answer to a
    click — they settle different debts — and the chart wants a number to show
    rather than a spinner of unknown length."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = session_between(at(OPEN_H, OPEN_M), at(CLOSE_H, CLOSE_M), step_s=10)
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720, opening_slice_ticks=500)
    session.backfill()

    starts, ends = [], []
    while session.pending_opening:
        session.sent.clear()
        session.pump_opening()
        starts += [m for m in session.sent if m["type"] == "history_start"]
        ends += [m for m in session.sent if m["type"] == "history_end"]

    check("every slice is announced as opening", all(m["opening"] for m in starts), starts[:2])
    check("and closed as one", all(m["opening"] for m in ends), ends[:2])
    check(
        "the countdown ends at zero",
        [m["remaining"] for m in ends][-1] == 0,
        [m["remaining"] for m in ends][-3:],
    )
    check(
        "and never goes up",
        all(a > b for a, b in zip([m["remaining"] for m in ends], [m["remaining"] for m in ends][1:])),
        [m["remaining"] for m in ends][:4],
    )


def test_a_session_inside_one_slice_parks_nothing():
    """The common case on a quiet contract: one block, no slices, no change in
    behaviour at all."""
    now_s = at(22, 10)
    term = FakeTerminal(0, now_s)
    term.ticks = b3_day()
    bridge = load_bridge(term)
    session = session_at(bridge, term, now_s, backfill_minutes=720)
    session.backfill()

    check("nothing is parked", session.pending_opening == [], session.pending_opening)
    check(
        "and the whole session opened the chart",
        len(block_ticks(session)) == len(term.ticks),
        len(block_ticks(session)),
    )


def main() -> int:
    return run_tests(globals())


if __name__ == "__main__":
    sys.exit(main())
