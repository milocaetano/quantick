"""Export one MetaTrader 5 session as a quantick replay recording.

Run once per download, then exit — this is deliberately *not* the streaming
bridge. A tape export moves tens of millions of ticks and hundreds of
megabytes; sharing the live bridge's socket with that would mean a download
could stall, or take down, a chart someone is trading on. A separate process
also needs no EA attached to a chart: it talks to the already-running,
already-logged-in terminal through the official MetaTrader5 package, and no
credentials are read, stored or written anywhere.

It writes the two files `quantick-replay` reads:

    <out>/<SYMBOL>/<YYYY-MM-DD>.csv           the tape, one executed trade per row
    <out>/<SYMBOL>/<YYYY-MM-DD>.context.csv   minute candles for the sessions before it

Usage:
  python tools/mt5/export_session.py --symbol WINQ26 --day 2026-08-12 \
      --context-sessions 5 --out <replay folder>

  python tools/mt5/export_session.py --probe --symbol WINQ26 --out <replay folder>

`--probe` writes no files: it reports which symbols match and which days the
broker actually holds data for, which is what lets the interface grey out a day
before the trader clicks it instead of failing afterwards.

Every diagnostic line is a single JSON object with an `event_code`, so the app
(or a human with jq) reads progress without scraping prose. Progress is
reported as it goes, so a caller can show a bar and cancel by killing this
process: nothing is left half-written, because each file is built beside its
destination and moved into place only once it is whole.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

SCHEMA_VERSION = 1

#: MQL5 tick flags — the same vocabulary the bridge and the Rust decoder use.
TICK_FLAG_BUY = 32
TICK_FLAG_SELL = 64

#: What one context candle covers. A minute, as every quantick provider
#: delivers: 2m/5m/15m are folds over these, computed in the app.
CONTEXT_INTERVAL_MS = 60_000

#: How many calendar days back a probe looks for tradable days.
PROBE_WINDOW_DAYS = 120

#: Where the live bridge remembers the clock offset it measured. Read, never
#: written, from here: the bridge owns that measurement.
BRIDGE_CACHE_PATH = (
    Path(__file__).resolve().parents[2]
    / "bridge"
    / "mt5"
    / ".quantick_bridge_cache.json"
)

#: Ticks accumulated before a progress line is emitted. Small enough that a bar
#: moves visibly on a slow export, large enough that reporting is not the cost.
PROGRESS_EVERY = 250_000


def emit(event_code: str, **fields) -> None:
    """Print one structured JSON diagnostic line to stderr."""
    print(
        json.dumps({"event_code": event_code, "schema_version": SCHEMA_VERSION, **fields}),
        file=sys.stderr,
        flush=True,
    )


def load_mt5():
    """Attach to the running terminal, or explain what to do about it."""
    try:
        import MetaTrader5 as mt5
    except ImportError:
        emit("EXPORT_PKG_MISSING", fix="pip install MetaTrader5")
        raise SystemExit(2) from None

    if not mt5.initialize():
        code, message = mt5.last_error()
        emit(
            "EXPORT_ATTACH_FAILED",
            last_error_code=code,
            last_error_msg=message,
            fix="open and log in to the MetaTrader 5 terminal, then try again",
        )
        raise SystemExit(2)
    return mt5


def offset_from_any_cached_symbol() -> tuple[int, str] | None:
    """An offset the live bridge measured, for any symbol on this terminal.

    Server time minus UTC is a property of the *broker's server*, not of one
    instrument: every symbol on a terminal is stamped by the same clock. The
    bridge caches per symbol because that is where it measures; reading any
    entry back is therefore sound, and it is what makes exporting work outside
    market hours — which is when a trader actually downloads a replay.
    """
    try:
        cache = json.loads(BRIDGE_CACHE_PATH.read_text("utf-8"))
    except Exception:  # noqa: BLE001 - no cache is a normal first run
        return None
    for cached_symbol, entry in sorted(cache.items()):
        try:
            return int(entry["utc_offset_s"]), cached_symbol
        except (KeyError, TypeError, ValueError):
            continue
    return None


def server_utc_offset_s(mt5, symbol: str, override: int | None) -> int:
    """Server time minus UTC, in seconds.

    Measured by the live bridge's own routine rather than a second way here:
    the two must agree, or a downloaded day would sit an hour away from the
    same day streamed live and the seam would be invisible.
    """
    if override is not None:
        emit("EXPORT_UTC_OFFSET", source="explicit", server_utc_offset_s=override)
        return override

    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "bridge" / "mt5"))
    try:
        from quantick_bridge import measure_utc_offset_s

        offset = measure_utc_offset_s(symbol, None)
        emit("EXPORT_UTC_OFFSET", source="bridge", server_utc_offset_s=offset)
        return offset
    except Exception:  # noqa: BLE001 - a quiet market is the common case
        pass

    # The bridge refuses without a fresh tick. Exports happen after the close,
    # so fall back to what it measured earlier on this same terminal.
    cached = offset_from_any_cached_symbol()
    if cached is not None:
        offset, measured_on = cached
        emit(
            "EXPORT_UTC_OFFSET",
            source="bridge_cache",
            server_utc_offset_s=offset,
            measured_on_symbol=measured_on,
            note="server time is one clock for every symbol on this terminal",
        )
        return offset

    emit(
        "EXPORT_UTC_OFFSET_UNMEASURED",
        fix="connect this symbol live once so the clock offset is measured, "
        "or pass --utc-offset-s (B3 brokers use -10800)",
    )
    raise SystemExit(2)


def fmt_price(value: float, digits: int) -> str:
    """Format a price at the symbol's own precision.

    Fixed decimals, not `%g`: six significant digits is plenty for an index
    future and silently lossy for a five-decimal FX pair, and a replay that
    rounds prices is a replay that fills at prices the market never showed.
    """
    return f"{value:.{digits}f}"


def fmt_volume(value: float) -> str:
    """Format a quantity without trailing zeros, and without rounding."""
    text = f"{value:.8f}".rstrip("0").rstrip(".")
    return text or "0"


def offset_label(offset_s: int) -> str:
    """`-10800` → `-03:00`, the spelling the replay format declares."""
    sign = "-" if offset_s < 0 else "+"
    total = abs(offset_s) // 60
    return f"{sign}{total // 60:02d}:{total % 60:02d}"


def server_day_bounds(day: str, offset_s: int) -> tuple[datetime, datetime]:
    """The UTC instants MT5 wants for one *server-local* calendar day.

    MetaTrader stamps ticks in server time but `copy_ticks_range` takes naive
    UTC datetimes, so a day is bracketed by shifting the wall clock rather than
    by trusting either clock alone.
    """
    start = datetime.strptime(day, "%Y-%m-%d").replace(tzinfo=timezone.utc)
    return start, start + timedelta(days=1)


def resolve_symbol(mt5, symbol: str) -> int:
    """Make sure the terminal will serve this symbol; return its price digits."""
    info = mt5.symbol_info(symbol)
    if info is None:
        emit(
            "EXPORT_SYMBOL_UNKNOWN",
            symbol=symbol,
            fix="check the spelling against the terminal's Market Watch; "
            "expired contracts are often hidden until 'Show all' is on",
        )
        raise SystemExit(3)
    if not mt5.symbol_select(symbol, True):
        code, message = mt5.last_error()
        emit(
            "EXPORT_SYMBOL_UNAVAILABLE",
            symbol=symbol,
            last_error_code=code,
            last_error_msg=message,
            fix="add the symbol to Market Watch in the terminal, then try again",
        )
        raise SystemExit(3)
    return int(info.digits)


def probe(mt5, symbol: str, offset_s: int) -> int:
    """Report the days this broker actually holds data for, and write nothing."""
    resolve_symbol(mt5, symbol)
    now = datetime.now(timezone.utc) + timedelta(seconds=offset_s)
    since = now - timedelta(days=PROBE_WINDOW_DAYS)
    rates = mt5.copy_rates_range(symbol, mt5.TIMEFRAME_D1, since, now)
    days = []
    if rates is not None:
        days = [
            datetime.fromtimestamp(int(r["time"]), timezone.utc).strftime("%Y-%m-%d")
            for r in rates
            if r["tick_volume"] > 0
        ]
    emit(
        "EXPORT_DAYS_AVAILABLE",
        symbol=symbol,
        window_days=PROBE_WINDOW_DAYS,
        days=days,
        note="days the broker holds daily bars for; a day absent here has no tape either",
    )
    return 0


def write_atomically(path: Path, body: str) -> int:
    """Write beside the destination, then move into place.

    A cancelled download must not leave a half-written CSV that the session
    browser would then list as a playable day.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    scratch = path.with_suffix(path.suffix + ".part")
    scratch.write_text(body, encoding="utf-8", newline="")
    os.replace(scratch, path)
    return path.stat().st_size


def export_tape(
    mt5, symbol: str, day: str, offset_s: int, digits: int, out: Path
) -> tuple[Path, int]:
    """Write the day's executed trades as a quantick-replay tick file."""
    start, end = server_day_bounds(day, offset_s)
    ticks = mt5.copy_ticks_range(symbol, start, end, mt5.COPY_TICKS_TRADE)
    if ticks is None:
        code, message = mt5.last_error()
        emit(
            "EXPORT_TAPE_FAILED",
            symbol=symbol,
            day=day,
            last_error_code=code,
            last_error_msg=message,
            fix="the terminal served no ticks for this day; pick another day, "
            "or open the symbol's chart once so the terminal downloads its history",
        )
        raise SystemExit(4)

    # First pass: parse, keep every print. Sides are decided *after* the whole
    # day is seen, because MT5 aggressor flags are broker-dependent: the B3
    # broker probed on 2026-07-23 stamped BUY on every tick, and a whole
    # session of flags on one side is that broker lying, not a market that
    # never sold. The live bridge defaults to the tick rule for exactly this
    # reason (see `[metatrader] side_source` in feeds.toml); an export that
    # trusted those flags would replay a one-sided tape as if it were fact.
    prints: list[tuple[datetime, float, float, float, float, str]] = []
    flag_buys = 0
    flag_sells = 0
    for tick in ticks:
        price = tick["last"]
        # A tick with no traded price is a quote update that COPY_TICKS_TRADE
        # let through; it is not a print and must not become a bar.
        if price <= 0:
            continue
        stamp = datetime.fromtimestamp(int(tick["time_msc"]) / 1000.0, timezone.utc)
        flags = int(tick["flags"])
        bid, ask = tick["bid"], tick["ask"]
        if flags & TICK_FLAG_BUY:
            side = "B"
            flag_buys += 1
        elif flags & TICK_FLAG_SELL:
            side = "S"
            flag_sells += 1
        else:
            side = ""
        volume = tick["volume_real"] or tick["volume"]
        prints.append((stamp, price, bid, ask, volume, side))
        if len(prints) % PROGRESS_EVERY == 0:
            emit("EXPORT_TAPE_PROGRESS", symbol=symbol, day=day, ticks=len(prints))

    if not prints:
        emit(
            "EXPORT_TAPE_EMPTY",
            symbol=symbol,
            day=day,
            fix="this day has no executed trades — a holiday, a weekend, or "
            "before the contract started trading. Pick another day.",
        )
        raise SystemExit(4)

    #: Below this many flagged prints a one-sided day proves nothing — a
    #: handful of trades can legitimately all be buys. A full session of them
    #: cannot.
    ONE_SIDED_PROOF_PRINTS = 1_000
    flagged = flag_buys + flag_sells
    one_sided_flags = flagged >= ONE_SIDED_PROOF_PRINTS and (
        flag_buys == 0 or flag_sells == 0
    )

    inferred_sides = 0
    if one_sided_flags:
        # The flags are untrustworthy for this whole day. Re-derive every side
        # with the tick rule — uptick bought, downtick sold, unchanged keeps
        # the last direction — the same safe default the live bridge charts
        # with, so a replayed day and a lived day read on one policy.
        side_source = "tick_rule"
        emit(
            "EXPORT_TAPE_SIDES_ONE_SIDED",
            symbol=symbol,
            day=day,
            flag_buys=flag_buys,
            flag_sells=flag_sells,
            action="infer_sides_by_tick_rule",
            note="a whole session flagged on one side is a broker stamping a "
            "constant, not a market; sides are inferred like the live bridge does",
        )
        rederived: list[tuple[datetime, float, float, float, float, str]] = []
        previous_price: float | None = None
        carried_side = ""
        for stamp, price, bid, ask, volume, _ in prints:
            if previous_price is None or price == previous_price:
                side = carried_side
            elif price > previous_price:
                side = "B"
            else:
                side = "S"
            if not side:
                # No direction yet (the day's first prints at one price): the
                # price's side of the spread is the only evidence there is.
                side = (
                    "B"
                    if (ask > 0 and price >= ask)
                    else "S"
                    if (bid > 0 and price <= bid)
                    else ""
                )
            if side:
                carried_side = side
                inferred_sides += 1
            previous_price = price
            rederived.append((stamp, price, bid, ask, volume, side))
        # The day's leading prints can precede any price movement and any
        # usable quote, and the format rightly refuses an empty side. The
        # first decided side is the only evidence about them, so it reaches
        # back — still an inference, which `side_source=tick_rule` already
        # declares for the whole file.
        first_decided = next((side for *_, side in rederived if side), "")
        if not first_decided:
            emit(
                "EXPORT_TAPE_SIDES_UNDECIDABLE",
                symbol=symbol,
                day=day,
                fix="the whole day traded at one price with no usable quotes; "
                "sides cannot be inferred, and a made-up side would chart a lie",
            )
            raise SystemExit(4)
        for index, (stamp, price, bid, ask, volume, side) in enumerate(rederived):
            if side:
                break
            inferred_sides += 1
            rederived[index] = (stamp, price, bid, ask, volume, first_decided)
        prints = rederived
    else:
        side_source = "venue_flags"
        for index, (stamp, price, bid, ask, volume, side) in enumerate(prints):
            if side:
                continue
            # The venue did not say. Falling back to the price's side of the
            # spread is an inference, counted and declared below rather than
            # passed off as the exchange's own answer.
            inferred = (
                "B"
                if (ask > 0 and price >= ask)
                else "S"
                if (bid > 0 and price <= bid)
                else ""
            )
            if inferred:
                inferred_sides += 1
                prints[index] = (stamp, price, bid, ask, volume, inferred)
        if inferred_sides:
            # Data honesty: the header must not claim the venue answered when
            # it did not, so the whole file downgrades to the weaker
            # declaration.
            side_source = "venue_flags_with_bid_ask_fallback"

    rows: list[str] = [
        "# quantick-replay 1",
        f"# symbol={symbol}",
        f"# timezone={offset_label(offset_s)}",
        f"# side_source={side_source}",
        "# source=MetaTrader 5 COPY_TICKS_TRADE, exported by quantick",
        "Date,Time,Price,Bid,Ask,Volume,Side",
    ]
    written = 0
    for stamp, price, bid, ask, volume, side in prints:
        rows.append(
            "{d},{t}.{ms:03d},{p},{b},{a},{v},{s}".format(
                d=f"{stamp:%Y-%m-%d}",
                t=f"{stamp:%H:%M:%S}",
                ms=stamp.microsecond // 1000,
                p=fmt_price(price, digits),
                # Empty, never zero: a quote the export does not have is
                # absent, and a zero would read as a price of zero.
                b=fmt_price(bid, digits) if bid > 0 else "",
                a=fmt_price(ask, digits) if ask > 0 else "",
                v=fmt_volume(volume),
                s=side,
            )
        )
        written += 1

    emit(
        "EXPORT_TAPE_SIDES",
        symbol=symbol,
        side_source=side_source,
        venue_flagged=0 if one_sided_flags else flagged,
        inferred=inferred_sides,
    )

    path = out / symbol / f"{day}.csv"
    size = write_atomically(path, "\n".join(rows) + "\n")
    emit(
        "EXPORT_TAPE_WRITTEN",
        symbol=symbol,
        day=day,
        path=str(path),
        ticks=written,
        bytes=size,
    )
    return path, written


def export_context(
    mt5, symbol: str, day: str, sessions: int, offset_s: int, digits: int, out: Path
) -> Path | None:
    """Write minute candles for the `sessions` trading days before `day`."""
    if sessions <= 0:
        return None
    start, _ = server_day_bounds(day, offset_s)
    # Ask by calendar span and count *trading* days from what comes back:
    # weekends and holidays are not the trader's arithmetic to do.
    since = start - timedelta(days=sessions * 3 + 10)
    rates = mt5.copy_rates_range(symbol, mt5.TIMEFRAME_M1, since, start)
    if rates is None or len(rates) == 0:
        code, message = mt5.last_error()
        emit(
            "EXPORT_CONTEXT_EMPTY",
            symbol=symbol,
            last_error_code=code,
            last_error_msg=message,
            note="the replay still plays; the chart opens at the first print",
        )
        return None

    wanted = sorted(
        {datetime.fromtimestamp(int(r["time"]), timezone.utc).date() for r in rates},
        reverse=True,
    )[:sessions]
    keep = set(wanted)
    # Short is a fact, not a failure: a contract younger than the span asked
    # for genuinely has fewer sessions. It is declared so the app can tell that
    # apart from a fetch that stopped early.
    complete = len(wanted) >= sessions

    rows = [
        "# quantick-context 1",
        f"# symbol={symbol}",
        f"# timezone={offset_label(offset_s)}",
        f"# interval_ms={CONTEXT_INTERVAL_MS}",
        f"# complete={str(complete).lower()}",
        "# source=MetaTrader 5 broker candles (no order flow)",
        "Date,Time,Open,High,Low,Close,Volume,Trades",
    ]
    candles = 0
    for rate in rates:
        stamp = datetime.fromtimestamp(int(rate["time"]), timezone.utc)
        if stamp.date() not in keep:
            continue
        volume = rate["real_volume"] or rate["tick_volume"]
        rows.append(
            "{d},{t}.000,{o},{h},{l},{c},{v},0".format(
                d=f"{stamp:%Y-%m-%d}",
                t=f"{stamp:%H:%M:%S}",
                o=fmt_price(rate["open"], digits),
                h=fmt_price(rate["high"], digits),
                l=fmt_price(rate["low"], digits),
                c=fmt_price(rate["close"], digits),
                v=fmt_volume(volume),
            )
        )
        candles += 1

    path = out / symbol / f"{day}.context.csv"
    size = write_atomically(path, "\n".join(rows) + "\n")
    emit(
        "EXPORT_CONTEXT_WRITTEN",
        symbol=symbol,
        path=str(path),
        candles=candles,
        sessions=len(wanted),
        sessions_requested=sessions,
        complete=complete,
        bytes=size,
    )
    return path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--symbol", required=True, help="MT5 symbol, e.g. WINQ26")
    parser.add_argument("--day", help="session day to export, YYYY-MM-DD")
    parser.add_argument(
        "--context-sessions",
        type=int,
        default=5,
        help="trading days of minute candles to fetch before --day (0 for none)",
    )
    parser.add_argument("--out", required=True, help="replay folder to write into")
    parser.add_argument(
        "--probe",
        action="store_true",
        help="report available days and write nothing",
    )
    parser.add_argument(
        "--utc-offset-s",
        type=int,
        default=None,
        help="server time minus UTC, in seconds (B3 brokers use -10800)",
    )
    args = parser.parse_args()

    if not args.probe and not args.day:
        parser.error("--day is required unless --probe is given")

    mt5 = load_mt5()
    try:
        offset_s = server_utc_offset_s(mt5, args.symbol, args.utc_offset_s)
        if args.probe:
            return probe(mt5, args.symbol, offset_s)

        out = Path(args.out)
        emit(
            "EXPORT_STARTED",
            symbol=args.symbol,
            day=args.day,
            context_sessions=args.context_sessions,
            out=str(out),
        )
        digits = resolve_symbol(mt5, args.symbol)
        tape, ticks = export_tape(mt5, args.symbol, args.day, offset_s, digits, out)
        context = export_context(
            mt5, args.symbol, args.day, args.context_sessions, offset_s, digits, out
        )
        emit(
            "EXPORT_DONE",
            symbol=args.symbol,
            day=args.day,
            tape=str(tape),
            context=str(context) if context else None,
            ticks=ticks,
        )
        return 0
    finally:
        mt5.shutdown()


if __name__ == "__main__":
    raise SystemExit(main())
