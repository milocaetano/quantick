#!/usr/bin/env python3
"""Stream a MetaTrader 5 symbol's ticks and Depth of Market to quantick.

This is the *simple* bridge: it attaches to the already-running, already
logged-in terminal through the official ``MetaTrader5`` package and dials
quantick's local listener, speaking the same newline-delimited JSON protocol as
``QuantickBridge.mq5``. Nothing needs to be compiled, copied into the terminal,
or dragged onto a chart, and no credentials exist anywhere in this path — the
socket never leaves ``127.0.0.1``.

    python bridge/mt5/quantick_bridge.py --symbol WINQ26

The protocol contract lives in PROTOCOL.md next to this file, and
``crates/feed-mt5`` is its executable counterpart. quantick cannot tell which
bridge dialed it, which is the point: pick whichever fits the day.

Honest difference from the Expert Advisor
-----------------------------------------
The EA runs *inside* the terminal, so ``OnBookEvent`` hands it every book change
at the instant it happens. This script is an outside observer: MetaTrader
exposes no push API to external processes, so it polls. Reading the book costs
about 0.06 ms (the package is a native DLL; Python only supplies the calling
convention), so polling every few milliseconds is cheap — but a change that
appears and disappears between two polls is a change quantick never sees. That
is a real fidelity gap, it is inherent to being outside the terminal, and it is
the reason the EA still exists as the higher-fidelity option.

Server time
-----------
MT5 stamps everything in *server wall time encoded as epoch seconds*, and the
Python API exposes no equivalent of MQL5's ``TimeTradeServer()``. The offset is
therefore measured from a fresh tick and snapped to the 15-minute grid every
real timezone uses, then cached. When neither a fresh tick nor a cached value is
available (first run outside market hours), the script refuses to start rather
than guessing an offset and mislabelling every timestamp downstream.
"""
from __future__ import annotations

import argparse
import json
import select  # noqa: F401  (re-exported: the suites patch `select.select` here)
import socket
import time
from pathlib import Path

from quantick_bridge_core import (
    BRIDGE_NAME,
    BRIDGE_VERSION,
    DEFAULT_BACKFILL_MAX_TICKS,
    DEFAULT_OPENING_SLICE_TICKS,
    SCHEMA_VERSION,
    BridgeExit,
    connect_terminal,
    log,
    market_is_trading,
    mt5,
)

# Re-exported, not used here: this module is the bridge's public face, and the
# feed's agreement guard and the bridge suites read these names off it. Moving
# a constant into `quantick_bridge_core` must not move where it is read from.
from quantick_bridge_core import (  # noqa: F401
    LOAD_OLDER_MAX_WINDOW_S,
    MAX_COMMAND_LINE_BYTES,
    MAX_PUMP_ROUNDS,
    OPENING_REACH_MAX_CALLS,
    RATES_MAX_PAGES,
    SESSION_GAP_MS,
    SESSION_WALK_MAX_WINDOWS,
    TICKS_PER_PUMP_ROUND,
)
from quantick_bridge_history import HistoryMixin
from quantick_bridge_rates import RatesMixin
from quantick_bridge_ticks import TicksMixin
from quantick_bridge_transport import TransportMixin

#: Timezone offsets are whole 15-minute steps everywhere on earth, so snapping
#: to that grid removes the millisecond of tick latency from the measurement.
OFFSET_GRID_S = 15 * 60
#: Where a measured offset used to be remembered: beside this script, inside
#: whatever checkout it ran from. Still read, never written — a clock measured
#: before the cache moved is one the trader already paid for.
LEGACY_CACHE_PATH = Path(__file__).with_name(".quantick_bridge_cache.json")

#: Where a measured offset is remembered between runs.
#:
#: The broker's clock is a fact about the trader's terminal, not about a source
#: tree, so a second checkout, a git worktree or a fresh clone must not lose
#: it. Losing it is not cosmetic: outside market hours there is no fresh tick
#: to measure from, so a bridge or an export with no cached offset refuses to
#: run at all — which is exactly what "the download is broken again" looks
#: like from the outside.
#:
#: `set_clock_cache` points it at a durable home; the app passes the same one
#: it keeps the trader's other files under. Left alone, this stays where it
#: always was, so a bare `python quantick_bridge.py` behaves as before.
CACHE_PATH = LEGACY_CACHE_PATH


def set_clock_cache(path: str | None) -> None:
    """Point the clock cache at `path`, if one was stated."""
    global CACHE_PATH  # noqa: PLW0603 — one module-level setting, set once at startup
    if path:
        CACHE_PATH = Path(path)


def measure_utc_offset_s(symbol: str, override: int | None) -> int:
    """Server-time minus UTC, in seconds. See the module docstring."""
    if override is not None:
        log("BRIDGE_UTC_OFFSET", source="explicit", server_utc_offset_s=override)
        return override

    if market_is_trading(symbol):
        tick = mt5.symbol_info_tick(symbol)
        # The tick was produced moments ago, so its server timestamp minus now
        # *is* the offset, give or take the transport latency the grid removes.
        raw = tick.time_msc / 1000.0 - time.time()
        snapped = round(raw / OFFSET_GRID_S) * OFFSET_GRID_S
        log(
            "BRIDGE_UTC_OFFSET",
            source="live_tick",
            server_utc_offset_s=snapped,
            raw_s=round(raw, 3),
        )
        _cache_write(symbol, snapped)
        return snapped
    log("BRIDGE_UTC_OFFSET_MARKET_QUIET", symbol=symbol)

    cached = _cache_read(symbol)
    if cached is not None:
        log(
            "BRIDGE_UTC_OFFSET",
            source="cache",
            server_utc_offset_s=cached,
            note="measured on an earlier run; re-measured on the next fresh tick",
        )
        return cached

    log(
        "BRIDGE_UTC_OFFSET_UNKNOWN",
        symbol=symbol,
        action="refuse_to_start",
        hint=(
            "no fresh tick and nothing cached: run once during market hours, "
            "or pass --utc-offset-s (B3 brokers use -10800)"
        ),
    )
    raise BridgeExit


def _cache_read_at(path: Path, symbol: str) -> int | None:
    try:
        return int(json.loads(path.read_text("utf-8"))[symbol]["utc_offset_s"])
    except (OSError, ValueError, KeyError, TypeError):
        return None


def _cache_read(symbol: str) -> int | None:
    """The remembered offset for `symbol`, from the durable home or the old one.

    The legacy location is consulted only when the durable home has nothing to
    say, and what it holds is copied forward — never moved, never deleted — so
    a trader who measured their clock months ago never measures it again just
    because the cache found a better home.
    """
    cached = _cache_read_at(CACHE_PATH, symbol)
    if cached is not None:
        return cached
    if CACHE_PATH == LEGACY_CACHE_PATH:
        return None
    legacy = _cache_read_at(LEGACY_CACHE_PATH, symbol)
    if legacy is not None:
        _cache_write(symbol, legacy)
    return legacy


def _cache_write(symbol: str, offset_s: int) -> None:
    try:
        data = json.loads(CACHE_PATH.read_text("utf-8"))
    except (OSError, ValueError):
        data = {}
    if not isinstance(data, dict):
        data = {}
    data[symbol] = {"utc_offset_s": offset_s}
    try:
        CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
        CACHE_PATH.write_text(json.dumps(data, indent=2), "utf-8")
    except OSError:
        pass  # a read-only checkout is not worth failing the feed over

class Session(TransportMixin, TicksMixin, HistoryMixin, RatesMixin):
    """One connection to quantick: framing, cursors and message building."""

    def __init__(self, sock: socket.socket, symbol: str, args: argparse.Namespace) -> None:
        self.sock = sock
        # Slices of the opening session still to go out, newest first. Drained
        # one per loop pass by `pump_opening`.
        self.pending_opening: list = []
        # Outbound bytes waiting for `flush`. A bytearray rather than a list of
        # lines: the socket wants one buffer and this is already it.
        self.outbox = bytearray()
        self.symbol = symbol
        self.args = args
        self.seq = 0
        self.ticks_sent = 0
        self.pump_round_limits = 0
        self.book_seq = 0
        self.book_sent = 0
        self.book_skipped = 0
        self.offset_s = 0
        self.digits = 0
        self.book_subscribed = False
        # What this venue prints, decided once at hello and reused by the
        # candle block to choose an honest volume source.
        self.tape = "trades"
        # Cursor: MT5 ticks share milliseconds, so it takes both the newest
        # millisecond sent and how many ticks at that millisecond already went.
        self.cursor_msc = 0
        self.sent_at_cursor = 0
        self.last_book_body: str | None = None
        self.last_book_ms = 0.0
        self.last_heartbeat = 0.0
        # Partial line from quantick, kept across polls: the back-channel is
        # NDJSON like the outbound side, and a request can arrive split across
        # two reads however short it is.
        self.inbox = b""
        # The terminal's oldest tick, learned once and reused. It is what turns
        # "this page came back empty" into "there is nothing older", and the two
        # deserve different answers on the chart.
        self.earliest_ms: int | None = None
        self.earliest_known = False

    # -- framing ----------------------------------------------------------

    def start(self, offset_s: int) -> None:
        info = mt5.symbol_info(self.symbol)
        if info is None:
            raise BridgeExit
        self.offset_s = offset_s
        self.digits = int(info.digits)
        self.tape = self.detect_tape()

        hello = {
            "type": "hello",
            "schema": SCHEMA_VERSION,
            "bridge": BRIDGE_NAME,
            "bridge_version": BRIDGE_VERSION,
            "symbol": self.symbol,
            "broker_symbol": info.basis or self.symbol,
            "digits": self.digits,
            "server_utc_offset_s": offset_s,
            "tape": self.tape,
        }
        # Candle history is announced only when this session will really send
        # it, so a feed knows immediately whether a time pane has anything
        # coming rather than waiting on a block that never arrives.
        if self.args.rates_months > 0:
            hello["rates"] = True
        # This bridge reads its socket, so quantick may write to it. A bridge
        # that does not declare this is never written to: the request would sit
        # unread in the receive buffer and, once that filled, block the peer.
        hello["history_paging"] = True
        # Depth fields are announced only when this session can really deliver
        # depth. Omitting them is the honest "no book here" quantick relies on
        # to explain an empty heatmap instead of drawing one.
        if self.args.book:
            self.book_subscribed = bool(mt5.market_book_add(self.symbol))
            if self.book_subscribed:
                hello["book_levels"] = int(info.ticks_bookdepth)
                if info.trade_tick_size > 0:
                    hello["tick_size"] = self.price(info.trade_tick_size)
            else:
                log(
                    "BRIDGE_BOOK_SUBSCRIBE_FAILED",
                    symbol=self.symbol,
                    mt5_error=str(mt5.last_error()),
                    hint="this symbol may have no Depth of Market; ticks still stream",
                )
        self.send(hello)
        # Out now, not at the bottom of the loop. The feed treats a silent
        # socket as a bridge that failed to start, and everything after this
        # line -- the session walk, then a block of a million prints -- takes
        # long enough for that to be the wrong conclusion.
        self.flush()

        self.backfill()
        if self.args.rates_months > 0:
            self.send_rates()
        log(
            "BRIDGE_SESSION_STARTED",
            symbol=self.symbol,
            host=self.args.host,
            port=self.args.port,
            book=self.book_subscribed,
            server_utc_offset_s=offset_s,
        )

def run_session(args: argparse.Namespace, offset_s: int) -> None:
    with socket.create_connection((args.host, args.port), timeout=10) as sock:
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        session = Session(sock, args.symbol, args)
        session.start(offset_s)
        tick_interval = args.tick_poll_ms / 1000.0
        book_interval = args.book_poll_ms / 1000.0
        next_tick = next_book = time.monotonic()
        try:
            while True:
                now = time.monotonic()
                if now >= next_tick:
                    session.pump_ticks()
                    next_tick = now + tick_interval
                if now >= next_book:
                    session.pump_book()
                    next_book = now + book_interval
                # Between the pumps, not on a deadline of its own: a request
                # arrives when a trader clicks, and the answer should not wait
                # on the next poll interval to start.
                session.pump_commands()
                # After the pumps and the commands: a trader's click and a live
                # print both outrank filling in this morning's history.
                session.pump_opening()
                session.maybe_heartbeat()
                # Everything this pass produced leaves before the loop waits.
                # The buffer's size cap bounds memory; this bounds *latency*,
                # and without it a quiet tape could hold a print until enough
                # others arrived to fill a quarter of a megabyte.
                session.flush()
                # Sleep to the nearest due deadline instead of spinning: the
                # terminal reads cost microseconds, the waiting is the loop.
                idle = min(next_tick, next_book) - time.monotonic()
                # Not while the opening session is still going out: there is
                # work in hand, and sleeping on it would stretch a fill that
                # takes ten seconds into one that takes minutes.
                if idle > 0 and not session.pending_opening:
                    time.sleep(min(idle, 0.05))
        except KeyboardInterrupt:
            session.close("interrupted")
            raise
        except OSError as error:
            session.close("socket error")
            raise ConnectionError(str(error)) from error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--symbol", default="WINQ26", help="contract to stream")
    parser.add_argument("--host", default="127.0.0.1", help="quantick listener host")
    parser.add_argument("--port", type=int, default=9100, help="quantick listener port")
    parser.add_argument(
        "--backfill-minutes",
        type=int,
        default=720,
        help=(
            "width of the opening block's first ask; the block itself reaches "
            "the session's first print however long that takes"
        ),
    )
    parser.add_argument(
        "--backfill-max-ticks",
        type=int,
        default=DEFAULT_BACKFILL_MAX_TICKS,
        help=(
            "bound on how many opening ticks are held in memory at once; the "
            "newest win and the rest stays one 'load older' away"
        ),
    )
    parser.add_argument(
        "--opening-slice-ticks",
        type=int,
        default=DEFAULT_OPENING_SLICE_TICKS,
        help=(
            "ticks per slice of the opening block; the first slice is what the "
            "chart paints on and the rest follow it while the tape runs"
        ),
    )
    parser.add_argument(
        "--rates-months",
        type=int,
        default=3,
        help=(
            "months of M1 candle history to send after the tick backfill "
            "(0 disables the block, and the session declares no candles)"
        ),
    )
    parser.add_argument(
        "--rates-max-bars",
        type=int,
        default=200_000,
        help="cap on candles sent; the newest win and the log says how many were left behind",
    )
    parser.add_argument("--heartbeat-seconds", type=float, default=5.0)
    parser.add_argument("--retry-seconds", type=float, default=5.0)
    parser.add_argument(
        "--no-book",
        dest="book",
        action="store_false",
        help="stream ticks only, no Depth of Market",
    )
    parser.add_argument(
        "--book-poll-ms",
        type=float,
        default=5.0,
        help="how often the book is read (a read costs ~0.06 ms)",
    )
    parser.add_argument(
        "--book-min-interval-ms",
        type=float,
        default=20.0,
        help="floor between two published images",
    )
    parser.add_argument("--tick-poll-ms", type=float, default=20.0)
    parser.add_argument(
        "--utc-offset-s",
        type=int,
        default=None,
        help="server_time - utc, in seconds; measured from a fresh tick when omitted",
    )
    parser.add_argument(
        "--clock-cache",
        default=None,
        help="file the measured broker clock is remembered in; defaults beside this script",
    )
    args = parser.parse_args()
    set_clock_cache(args.clock_cache)

    try:
        connect_terminal(args.symbol)
    except BridgeExit:
        return 2

    log(
        "BRIDGE_STARTING",
        symbol=args.symbol,
        host=args.host,
        port=args.port,
        backfill_minutes=args.backfill_minutes,
        backfill_max_ticks=args.backfill_max_ticks,
        opening_slice_ticks=args.opening_slice_ticks,
        stream_book=args.book,
    )
    try:
        while True:
            try:
                offset_s = measure_utc_offset_s(args.symbol, args.utc_offset_s)
            except BridgeExit:
                return 2
            try:
                run_session(args, offset_s)
            except (ConnectionError, OSError, socket.timeout) as error:
                log(
                    "BRIDGE_DISCONNECTED",
                    reason=str(error) or type(error).__name__,
                    retry_in_s=args.retry_seconds,
                    hint="is quantick running and listening on this port?",
                )
                time.sleep(args.retry_seconds)
    except KeyboardInterrupt:
        log("BRIDGE_STOPPED", reason="interrupted")
        return 0
    finally:
        mt5.shutdown()


if __name__ == "__main__":
    raise SystemExit(main())
