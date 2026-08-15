"""Tests for export_session.py: the side-inference policy, and the export
itself driven end to end against a fake terminal.

They prove the exporter without a logged-in MetaTrader, which is what lets CI
run them. Run either way:

    python tools/mt5/test_export_session.py
    python -m pytest tools/mt5/test_export_session.py
"""

import contextlib
import io
import json
import tempfile
from collections import namedtuple
from datetime import datetime, timezone
from pathlib import Path

import export_session
from export_session import (
    ONE_SIDED_PROOF_PRINTS,
    TICK_FLAG_BUY,
    TICK_FLAG_SELL,
    export_tape,
    fill_missing_sides,
    flags_are_one_sided,
    offset_from_any_cached_symbol,
    rederive_sides,
    spread_side,
)

STAMP = datetime(2026, 8, 13, 12, 0, 0, tzinfo=timezone.utc)


def tape(*rows):
    """(price, bid, ask, side) → the print tuple export_tape builds."""
    return [(STAMP, price, bid, ask, 1.0, side) for price, bid, ask, side in rows]


def sides(prints):
    return [side for *_, side in prints]


def test_one_sided_detection_needs_proof_and_tolerates_typos():
    # A handful of one-sided prints proves nothing.
    assert not flags_are_one_sided(10, 0)
    # A whole stamped session does — including one stamped with a few typos.
    assert flags_are_one_sided(750_000, 0)
    assert flags_are_one_sided(2_000, 3)
    # A market with both sides present keeps its flags.
    assert not flags_are_one_sided(1_500, 500)


def test_tick_rule_rederives_a_stamped_day():
    prints = tape(
        (100.0, 0.0, 0.0, "B"),  # leader: no movement yet
        (101.0, 0.0, 0.0, "B"),  # uptick → B
        (100.0, 0.0, 0.0, "B"),  # downtick → S
        (100.0, 0.0, 0.0, "B"),  # unchanged → carries S
    )
    rederived, inferred = rederive_sides(prints)
    assert sides(rederived) == ["B", "B", "S", "S"]
    assert inferred == 4


def test_leaders_inherit_the_first_decided_side():
    prints = tape(
        (100.0, 0.0, 0.0, "B"),
        (100.0, 0.0, 0.0, "B"),
        (99.0, 0.0, 0.0, "B"),  # first movement: downtick → S
    )
    rederived, _ = rederive_sides(prints)
    assert sides(rederived) == ["S", "S", "S"]


def test_an_undecidable_day_refuses_to_export():
    prints = tape((100.0, 0.0, 0.0, ""), (100.0, 0.0, 0.0, ""))
    try:
        rederive_sides(prints)
    except ValueError:
        pass
    else:
        raise AssertionError("a made-up side would chart a lie")


def test_venue_gaps_fall_back_to_spread_then_carry_forward():
    prints = tape(
        (100.0, 99.0, 100.0, "B"),
        (100.0, 99.0, 100.0, ""),  # spread: at the ask → B
        (99.0, 99.0, 100.0, ""),  # spread: at the bid → S
        (99.5, 0.0, 0.0, ""),  # inside, no quote → carries S
    )
    filled, inferred = fill_missing_sides(prints)
    assert sides(filled) == ["B", "B", "S", "S"]
    assert inferred == 3


def test_spread_side_is_evidence_or_nothing():
    assert spread_side(100.0, 99.0, 100.0) == "B"
    assert spread_side(99.0, 99.0, 100.0) == "S"
    assert spread_side(99.5, 99.0, 100.0) == ""
    assert spread_side(100.0, 0.0, 0.0) == ""


class FakeMt5:
    """The slice of the MetaTrader5 package `export_tape` actually touches.

    Small on purpose: a fake that grows past what the code under test calls
    stops being evidence and starts being a second implementation to maintain.
    """

    COPY_TICKS_TRADE = 2

    def __init__(self, ticks):
        self._ticks = ticks
        self.asked_for = None

    def copy_ticks_range(self, symbol, start, end, flags):
        self.asked_for = (symbol, start, end, flags)
        return self._ticks

    def last_error(self):
        return (0, "no error")


def tick(time_ms, price, flags, bid=0.0, ask=0.0, volume=1.0):
    """One row shaped like MT5's tick array, keyed the way export_tape reads it."""
    return {
        "time_msc": time_ms,
        "last": price,
        "bid": bid,
        "ask": ask,
        "volume": volume,
        # The terminal reports 0.0 when it has no real volume; export_tape then
        # falls back to `volume`, and both paths must stay exercised.
        "volume_real": 0.0,
        "flags": flags,
    }


#: The clock every B3 broker's server runs on, seconds from UTC.
B3_UTC_OFFSET_S = -10_800

#: Price decimals for the mini index: it trades in whole points.
WIN_PRICE_DIGITS = 0

Export = namedtuple("Export", "body written events relative")
Refusal = namedtuple("Refusal", "exit_code events files_written")


def read_events(captured):
    """The JSON diagnostics from a captured stderr, in order."""
    return [
        json.loads(line) for line in captured.getvalue().splitlines() if line.strip()
    ]


def run_export(ticks, symbol="WINV26", day="2026-08-12"):
    """Drive `export_tape` end to end against a fake terminal.

    Returns what landed on disk, how many prints it holds, the JSON
    diagnostics it emitted, and where the file was written relative to the
    output folder.
    """
    captured = io.StringIO()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        with contextlib.redirect_stderr(captured):
            path, written = export_tape(
                FakeMt5(ticks), symbol, day, B3_UTC_OFFSET_S, WIN_PRICE_DIGITS, out
            )
        body = path.read_text(encoding="utf-8")
        relative = path.relative_to(out).as_posix()
    return Export(body, written, read_events(captured), relative)


def run_refusal(ticks, symbol="WINV26", day="2026-08-12"):
    """Drive `export_tape` on a day it must refuse.

    Returns the exit code it stopped with, what it said, and every file it
    left behind — which has to be none: a refusal that writes half a tape is
    a day the session browser would offer and the trader could not use.
    """
    captured = io.StringIO()
    exit_code = None
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        with contextlib.redirect_stderr(captured):
            try:
                export_tape(
                    FakeMt5(ticks), symbol, day, B3_UTC_OFFSET_S, WIN_PRICE_DIGITS, out
                )
            except SystemExit as stop:
                exit_code = stop.code
        files_written = sorted(path.name for path in out.rglob("*") if path.is_file())
    return Refusal(exit_code, read_events(captured), files_written)


def one_event(events, code):
    """The single diagnostic with this code, or an assertion about why not."""
    matches = [event for event in events if event["event_code"] == code]
    assert len(matches) == 1, f"expected one {code}, saw {len(matches)}"
    return matches[0]


def data_rows(body):
    """The tape's prints: everything past the `#` block and the column line."""
    declared = [line for line in body.splitlines() if line and not line.startswith("#")]
    return declared[1:]


def test_a_flagged_day_reaches_disk_with_its_sides_declared():
    # The regression test for the export that died between 2026-08-13 and this
    # fix: the summary line sits between the last formatted row and the write,
    # so a name that does not exist there means no file is ever written. Every
    # test above this one covers a pure helper — none of them calls export_tape,
    # which is exactly why a NameError shipped.
    export = run_export(
        [
            tick(1_786_000_000_000, 172260.0, TICK_FLAG_BUY, 172255.0, 172260.0),
            tick(1_786_000_001_000, 172255.0, TICK_FLAG_SELL, 172255.0, 172260.0),
            # No flag, and priced inside the spread: nothing to read it off but
            # the side before it.
            tick(1_786_000_002_000, 172258.0, 0, 172255.0, 172260.0),
        ]
    )

    assert export.relative == "WINV26/2026-08-12.csv"
    assert export.written == 3
    assert len(data_rows(export.body)) == 3
    # One print was inferred, so the file must not claim the venue answered.
    assert "# side_source=venue_flags_with_bid_ask_fallback" in export.body
    summary = one_event(export.events, "EXPORT_TAPE_SIDES")
    assert summary["venue_flagged"] == 2
    assert summary["inferred"] == 1
    written = one_event(export.events, "EXPORT_TAPE_WRITTEN")
    assert written["ticks"] == 3
    assert written["bytes"] > 0


def test_a_stamped_day_is_rederived_and_credits_the_venue_with_nothing():
    # The B3 broker stamps BUY on every print. The whole day is re-derived, and
    # the summary must not credit the venue for answers that never reached the
    # file — 0, not the count it stamped.
    export = run_export(
        [
            tick(1_786_000_000_000 + index * 100, 172000.0 + index % 7, TICK_FLAG_BUY)
            for index in range(ONE_SIDED_PROOF_PRINTS + 1)
        ]
    )

    assert "# side_source=tick_rule" in export.body
    summary = one_event(export.events, "EXPORT_TAPE_SIDES")
    assert summary["venue_flagged"] == 0, "no stamped flag survived into the file"
    assert summary["inferred"] == export.written
    assert one_event(export.events, "EXPORT_TAPE_SIDES_ONE_SIDED")["flag_buys"] == (
        ONE_SIDED_PROOF_PRINTS + 1
    )
    # The tick rule found both sides in a market that only ever printed BUY.
    assert {row.split(",")[-1] for row in data_rows(export.body)} == {"B", "S"}


def test_a_day_with_no_prints_refuses_instead_of_writing_an_empty_tape():
    # A quote-only day (COPY_TICKS_TRADE lets those through) is not a session.
    refusal = run_refusal(
        [tick(1_786_000_000_000, 0.0, TICK_FLAG_BUY, 172255.0, 172260.0)]
    )

    assert refusal.exit_code == 4
    assert refusal.files_written == [], "a refused day must leave nothing behind"
    assert one_event(refusal.events, "EXPORT_TAPE_EMPTY")["fix"]


def test_a_terminal_that_serves_no_ticks_names_the_day_and_the_next_step():
    # `copy_ticks_range` answers None when the terminal holds no tick history
    # for the symbol — the likeliest remaining failure on a contract that has
    # just started trading, and the one a trader can actually act on by
    # opening its chart once.
    refusal = run_refusal(None)

    assert refusal.exit_code == 4
    assert refusal.files_written == []
    failure = one_event(refusal.events, "EXPORT_TAPE_FAILED")
    assert failure["day"] == "2026-08-12"
    assert failure["symbol"] == "WINV26"
    assert "chart" in failure["fix"], "the fix must be something to go and do"


@contextlib.contextmanager
def clock_caches(*paths):
    """Run with `paths` as the clock caches consulted, in order."""
    original = list(export_session.BRIDGE_CACHE_PATHS)
    export_session.BRIDGE_CACHE_PATHS[:] = list(paths)
    try:
        yield
    finally:
        export_session.BRIDGE_CACHE_PATHS[:] = original


def write_clock(path, symbol, offset_s):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({symbol: {"utc_offset_s": offset_s}}), "utf-8")


def test_the_durable_clock_is_read_before_the_one_in_the_checkout():
    # After the close there is no fresh tick to measure from, so which cache
    # answers decides whether a download runs at all. The durable home is the
    # one the app writes; a stale copy left in a checkout must not win.
    with tempfile.TemporaryDirectory() as tmp:
        durable = Path(tmp) / "Quantick" / "mt5-clock.json"
        legacy = Path(tmp) / "checkout" / ".quantick_bridge_cache.json"
        write_clock(durable, "WINV26", -10800)
        write_clock(legacy, "WINV26", 7200)
        with clock_caches(durable, legacy):
            assert offset_from_any_cached_symbol() == (-10800, "WINV26")


def test_a_clock_measured_before_the_move_still_answers():
    # The whole reason the legacy path is still read: a trader who measured
    # their broker's clock months ago must not be told to measure it again.
    with tempfile.TemporaryDirectory() as tmp:
        durable = Path(tmp) / "Quantick" / "mt5-clock.json"
        legacy = Path(tmp) / "checkout" / ".quantick_bridge_cache.json"
        write_clock(legacy, "WIN$N", -10800)
        with clock_caches(durable, legacy):
            assert offset_from_any_cached_symbol() == (-10800, "WIN$N")


def test_no_clock_anywhere_is_not_a_crash():
    with tempfile.TemporaryDirectory() as tmp:
        with clock_caches(Path(tmp) / "nothing.json"):
            assert offset_from_any_cached_symbol() is None


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_") and callable(test):
            test()
            print(f"ok - {name}")
