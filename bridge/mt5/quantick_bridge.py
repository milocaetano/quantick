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
import select
import socket
import sys
import time
from pathlib import Path

try:
    import MetaTrader5 as mt5
except ImportError:  # pragma: no cover - environment problem, not logic
    #: Missing package. Reported when the bridge is *run*, not when the module
    #: is imported: everything here that is pure — the clock cache, the wire
    #: format — is testable on a machine with no MetaTrader, and CI is exactly
    #: such a machine. Exiting at import time made the bridge's own behaviour
    #: unprovable anywhere it could be proven cheaply.
    mt5 = None

SCHEMA_VERSION = 1
BRIDGE_NAME = "quantick-mt5-bridge-py"
BRIDGE_VERSION = "0.3.0"

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

#: How far back to look for one executed trade before declaring a symbol
#: tape-less. See `BridgeSession.detect_tape` for why this errs long.
TAPE_PROBE_DAYS = 30

# Milliseconds in the M1 bucket the candle block is sent in.
M1_INTERVAL_MS = 60_000

# Days charged per "month" of requested candle history. Calendar months vary and
# nothing here needs them to be exact — this only decides how far back to ask.
DAYS_PER_MONTH = 31

# Candles per `rate` line. quantick drops a session whose line exceeds 64 KiB,
# so the whole block on one line would take the connection down with it; this
# bound is sized against that cap, and mirrors MAX_BARS_PER_RATE_LINE in
# crates/feed-mt5/src/protocol.rs. See PROTOCOL.md for the arithmetic.
MAX_BARS_PER_RATE_LINE = 300

# Candles asked for per copy_rates call.
#
# NOT sized to the span, sized under the terminal's "Max bars in chart" setting.
# MT5 validates a request's *potential* bar count against that setting and
# refuses outright rather than truncating: probed 2026-08-03 against an XP
# terminal capped at 100 000, a 90-day M1 range — 129 600 potential slots —
# returned (-2, 'Terminal: Invalid params'), while a 1-day range returned its
# 563 bars and a count of 50 000 returned everything the contract had. A count
# above the cap fails the same way — and 20 000 is not "safely small": it is one
# of the values the terminal's own Max-bars dialog offers, so a user who picks it
# reproduces the bug exactly. `fetch_rates` halves and retries on the refusal
# rather than trusting this number to be low enough.
RATES_PAGE_BARS = 20_000

# Smallest page worth retrying down to. Below this the page size is not the
# problem, and halving further only multiplies round trips.
RATES_MIN_PAGE_BARS = 1_000

# The terminal's code for a request it refuses to size. It says nothing about
# whether the data exists — only that the *request* was rejected — and telling
# those two apart is the difference between "this symbol has no history" and
# "raise Max bars in chart".
MT5_INVALID_PARAMS = -2

# Pages one candle block may request. Ninety days of M1 needs ~7 at the size
# above, so reaching this means the walk is not advancing rather than that the
# span was ambitious.
RATES_MAX_PAGES = 64

# The one message quantick sends *to* this bridge. Mirrors FeedMsg::LoadOlder in
# crates/feed-mt5/src/protocol.rs, and a Rust test asserts the two names match:
# a rename on one side would look like a bridge ignoring the trader's clicks
# rather than like a protocol break. See PROTOCOL.md.
LOAD_OLDER_TYPE = "load_older"

# First window one "load older" walks back over, in seconds. Small on purpose:
# during the session a few minutes of a liquid contract already holds thousands
# of ticks, and asking for a day to serve a page of 2 000 would make the trader
# wait on ticks that get thrown away.
LOAD_OLDER_FIRST_WINDOW_S = 300

# Widest window the walk grows to. Reached only after empty pages — an
# overnight gap, a weekend, a holiday — where the point is to cross dead time
# in a few calls rather than 5 minutes at a time.
LOAD_OLDER_MAX_WINDOW_S = 4 * 60 * 60

# Terminal calls one request may make. A page that keeps coming back empty is
# crossing dead time; this bounds how far that can go before the bridge answers
# with what it has and lets the trader click again.
LOAD_OLDER_MAX_PAGES = 24

# Hard cap on one page, whatever was asked for. quantick's own step is a
# DragValue a trader can type into, and one line per tick means an absurd number
# here is a socket write measured in gigabytes.
LOAD_OLDER_MAX_TICKS = 200_000

# How much wider each empty window gets. Crossing a weekend five minutes at a
# time would spend the page budget on dead air; four is steep enough that the
# budget below reaches back days, and shallow enough that the first productive
# window is not wildly larger than the page asked for.
LOAD_OLDER_WINDOW_GROWTH = 4

# The opening reach's own window, in seconds, and its own budget in calls.
#
# Deliberately not `load_older`'s. That walk answers a click by *collecting*
# pages, so it starts narrow to avoid fetching ticks it will throw away, and it
# stops early because the trader can always click again. This one answers a
# different question — *when did this instrument last print* — and it keeps
# nothing but a timestamp, so a wide window costs the terminal one search and
# this process nothing at all. It also has no second chance: an empty block
# here is a chart with nothing on it until the market opens.
#
# Four hours a step, forty-eight steps, is eight days of reach. B3 shuts for
# Carnival (Friday afternoon to Wednesday morning, about 86 h) and for Easter,
# and a search sized to a weekend gives up in the middle of both.
OPENING_REACH_WINDOW_S = 4 * 60 * 60
OPENING_REACH_MAX_CALLS = 48

# Bytes read from the socket per pass. The inbound direction carries one short
# JSON object per trader click, so this is never the constraint — it is sized
# like any read buffer, not like the traffic.
COMMAND_READ_BYTES = 4096

# Longest inbound line accepted before the buffer is dropped. Every command this
# protocol has is under 100 bytes; the cap exists because *something* has to
# bound a buffer fed by a socket. The feed guards the mirror-image case with
# MAX_LINE_BYTES for the same reason — without it any local process that writes
# bytes and no newline grows this until the bridge dies, taking the trader's
# tick stream with it.
MAX_COMMAND_LINE_BYTES = 64 * 1024

# Reads one pass may make before returning to the pumps. A peer streaming
# without pause would otherwise keep `select` readable forever and starve the
# tick, book and heartbeat pumps that share this loop.
MAX_COMMAND_READS = 16

# Ticks one `copy_ticks_from` may return. This bounds a single request, not a
# pump pass: the pass keeps asking while the answers come back full, so a burst
# larger than one batch is drained now instead of waiting for the next pass.
TICKS_PER_PUMP_ROUND = 4096

# Hard stop on that loop, so a tape arriving faster than one pass can forward
# it ends the pass and says so rather than owning the loop the book and the
# heartbeat share.
MAX_PUMP_ROUNDS = 16

# Ceiling on the widened request below. `copy_ticks_from` takes a floor in whole
# *seconds*, so a second holding more ticks than one batch can never be walked
# past by advancing the cursor — every round would re-fetch the same batch, find
# nothing new, and the tape would wedge inside that second forever. The answer is
# to ask deeper for that one second rather than to give up, and this bounds how
# deep: four million ticks is orders of magnitude past any real second on any
# venue, and it is a bound on memory, not a tuning knob.
MAX_TICKS_PER_REQUEST = 4_194_304


def log(event_code: str, **fields: object) -> None:
    """Emit one structured line, matching what the EA prints to Experts."""
    payload = {"event_code": event_code, **fields}
    print(json.dumps(payload, separators=(",", ":")), file=sys.stderr, flush=True)


class BridgeExit(Exception):
    """Fatal, with the reason already logged."""


# --------------------------------------------------------------------------
# Terminal
# --------------------------------------------------------------------------


def connect_terminal(symbol: str) -> None:
    if mt5 is None:
        log("BRIDGE_NO_MT5_PACKAGE", hint="pip install MetaTrader5")
        raise SystemExit(2)
    if not mt5.initialize():
        log(
            "BRIDGE_TERMINAL_ATTACH_FAILED",
            mt5_error=str(mt5.last_error()),
            hint="is the terminal running and logged in?",
        )
        raise BridgeExit
    info = mt5.symbol_info(symbol)
    if info is None:
        log(
            "BRIDGE_SYMBOL_NOT_FOUND",
            symbol=symbol,
            hint="check the exact contract name in Market Watch (e.g. WINQ26)",
        )
        raise BridgeExit
    if not info.visible and not mt5.symbol_select(symbol, True):
        log("BRIDGE_SYMBOL_SELECT_FAILED", symbol=symbol, mt5_error=str(mt5.last_error()))
        raise BridgeExit


def market_is_trading(symbol: str, observe_s: float = 2.0) -> bool:
    """Whether ticks are arriving right now.

    Deciding this from a tick's timestamp would be circular — the timestamp is
    in the very clock we are trying to measure. Watching the timestamp *move*
    is not: a tick that advances while we watch was produced while we watched.

    No terminal package at all means no ticks to watch, which is the same
    answer as a closed market: fall back to what was measured earlier.
    """
    if mt5 is None:
        return False
    first = mt5.symbol_info_tick(symbol)
    if first is None or not first.time_msc:
        return False
    deadline = time.monotonic() + observe_s
    while time.monotonic() < deadline:
        time.sleep(0.05)
        current = mt5.symbol_info_tick(symbol)
        if current is not None and current.time_msc != first.time_msc:
            return True
    return False


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


# --------------------------------------------------------------------------
# Wire
# --------------------------------------------------------------------------


class Session:
    """One connection to quantick: framing, cursors and message building."""

    def __init__(self, sock: socket.socket, symbol: str, args: argparse.Namespace) -> None:
        self.sock = sock
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

    def send(self, message: dict) -> None:
        line = json.dumps(message, separators=(",", ":")) + "\n"
        self.sock.sendall(line.encode("utf-8"))

    def price(self, value: float) -> str:
        return f"{value:.{self.digits}f}"

    def server_now_ms(self) -> int:
        return int((time.time() + self.offset_s) * 1000)

    def detect_tape(self) -> str:
        """Does this venue print trades for the symbol, or only quote it?

        Asks the terminal for *one* executed trade tick in the recent past:
        cheap, and decisive. An exchange-fed instrument has printed something;
        a broker-quoted CFD never prints at all — its ticks carry a bid and an
        ask and nothing else, so charting it as a tape leaves the chart empty.

        The window is generous on purpose. Getting this wrong in the "quotes"
        direction is the expensive mistake: a real tape would chart as one-unit
        synthetic prints with volume bars switched off, and nothing on screen
        would look broken. A month of lookback costs exactly the same single
        tick request as a day, and no exchange closes for a month.

        Known limit: a contract listed but never yet traded (a fresh expiry
        opened outside session hours) is reported as quotes until it prints and
        the bridge reconnects.
        """
        since = int(time.time() + self.offset_s) - TAPE_PROBE_DAYS * 86400
        trades = mt5.copy_ticks_from(self.symbol, since, 1, mt5.COPY_TICKS_TRADE)
        found = 0 if trades is None else len(trades)
        tape = "trades" if found else "quotes"
        log(
            "BRIDGE_TAPE_DETECTED",
            symbol=self.symbol,
            tape=tape,
            probe_days=TAPE_PROBE_DAYS,
            trade_ticks_found=found,
            note="quotes = the broker prices this symbol but prints no trades",
        )
        return tape

    # -- session ----------------------------------------------------------

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

    def backfill(self) -> None:
        """The trading day, so the chart opens on the session rather than on a
        sliver of it — and on the *last* session when today has none.

        The terminal keeps the whole session (and weeks behind it) on disk and
        returns a day in a fraction of a second, so the window is generous. The
        cap is about what happens *after*: every tick becomes a JSON line on the
        socket and a bar on the chart, and a full B3 day of the mini index is
        around a million of them — sized to hold one, because a trader opening
        at 15:00 means the whole day, not the last twenty minutes of it.

        It is still a cap, and a busy day can exceed it. When it does the newest
        ticks win, the log says how many were left behind, and what was left
        behind is reachable: `load_older` pages back into exactly that history
        (`serve_load_older`). Before the back-channel existed the truncation was
        permanent for the session, which is what made a low cap expensive.

        The window's *anchor* is the second half of the answer, and it used to
        be `now` unconditionally. That is the right anchor while a market is
        trading and the wrong one every other hour of the week: at 07:00 on a
        Monday `now − 12 h` is Sunday evening, B3 last printed on Friday, and
        the block went out empty onto a chart with nothing else to show. So an
        empty window is not the end of the question — see
        `reach_last_session`.
        """
        now_s = int(time.time() + self.offset_s)
        window_s = self.args.backfill_minutes * 60
        ticks = mt5.copy_ticks_range(
            self.symbol, now_s - window_s, now_s, self.tick_flags()
        )
        if ticks is None:
            log("BRIDGE_BACKFILL_FAILED", mt5_error=str(mt5.last_error()))
            ticks = []
        elif not len(ticks):
            # Nothing in the window the clock names, which is not the same
            # thing as nothing to send. The anchor moves to the tape.
            ticks = self.reach_last_session(now_s, window_s)
        available = len(ticks)
        if available > self.args.backfill_max_ticks:
            ticks = ticks[-self.args.backfill_max_ticks :]
            log(
                "BRIDGE_BACKFILL_TRUNCATED",
                symbol=self.symbol,
                available=available,
                sending=len(ticks),
                dropped_oldest=available - len(ticks),
                action="keep_newest",
                recoverable="load_older",
            )
        count = len(ticks)

        # An empty block still gets both markers: backfill_end is the
        # "history is done" signal quantick's loader waits on.
        self.send({"type": "backfill_start", "count_hint": count})
        # History is stamped like anything else: `sent_ms` says when the bridge
        # handed the line over, which for a backfill tick is minutes after the
        # print happened. The feed measures latency from live prints only, so
        # the stamp here is a true statement nobody draws a lag from.
        backfill_sent_ms = self.server_now_ms()
        for tick in ticks:
            self.send_tick(tick, backfill_sent_ms)
        self.send({"type": "backfill_end"})

        if count:
            self.cursor_msc = int(ticks[-1]["time_msc"])
            self.sent_at_cursor = sum(
                1 for t in ticks if int(t["time_msc"]) == self.cursor_msc
            )
        else:
            self.cursor_msc = now_s * 1000
            self.sent_at_cursor = 0

    def reach_last_session(self, now_s: int, window_s: int) -> list:
        """The newest session the terminal holds, when the clock's window is
        empty.

        Seeing no data at all is worse than not being connected: a trader
        opening the chart before the open, over a weekend or on a holiday is
        preparing, and the session they want to read is already on the
        terminal's disk. Nothing was missing but the question.

        So the anchor moves from the clock to the tape. `last_print_before`
        steps back over the dead time between now and the last session, and the
        *same* window is asked for again, ending on the print it found instead
        of ending at `now`. One extra concept, and the trader's
        `--backfill-minutes` keeps meaning what it says.

        The search starts where the clock's window ended rather than at `now`:
        that interval has just been reported empty, and re-scanning it would
        spend the reach's first steps proving the same thing twice.

        Nothing here invents data. An instrument the terminal has never held
        returns an empty block, as it did before; what changed is that a
        closed market no longer looks like one.
        """
        last_ms, calls = self.last_print_before(now_s - window_s)
        if last_ms is None:
            log(
                "BRIDGE_BACKFILL_NO_HISTORY",
                symbol=self.symbol,
                searched_calls=calls,
                note="the terminal holds no ticks for this symbol; sending an empty block",
            )
            return []
        # Rounded outward: `copy_ticks_range` takes whole seconds, and rounding
        # the other way would drop the very print the anchor was found by.
        to_s = -(-last_ms // 1000)
        from_s = max(0, to_s - window_s)
        found = mt5.copy_ticks_range(self.symbol, from_s, to_s, self.tick_flags())
        if found is None or not len(found):
            # The wide ask failed where the search succeeded, so fall back to
            # the one second the print is known to be in. A chart holding a
            # handful of prints is a poor chart and an honest one; an empty
            # block here would throw away the only thing that is known.
            narrow = mt5.copy_ticks_range(
                self.symbol, max(0, to_s - 1), to_s, self.tick_flags()
            )
            log(
                "BRIDGE_BACKFILL_REANCHOR_FAILED",
                symbol=self.symbol,
                from_s=from_s,
                to_s=to_s,
                mt5_error=str(mt5.last_error()),
                recovered=0 if narrow is None else len(narrow),
                action="send_the_second_the_print_is_in",
            )
            return [] if narrow is None else list(narrow)
        log(
            "BRIDGE_BACKFILL_REANCHORED",
            symbol=self.symbol,
            last_print_ms=last_ms,
            behind_s=now_s - to_s,
            from_s=from_s,
            to_s=to_s,
            count=len(found),
            searched_calls=calls,
        )
        return list(found)

    def last_print_before(self, before_s: int) -> tuple[int | None, int]:
        """When this instrument last printed at or before `before_s`.

        Answers a timestamp and how many terminal calls it took, or `(None, n)`
        when the search reached its budget, the terminal's own floor, or an
        error, with nothing found.

        Steps back in fixed windows rather than widening into one: the caller
        has already been told the recent window is empty, so every step but the
        last is over dead time, and a step that lands inside a session answers
        on its newest tick whatever else it returned. Nothing is collected —
        `found[-1]` is one index into what the terminal handed back, not a copy
        of it — which is what separates this from `walk_back`, whose job is to
        bring pages home and which would build half a million ticks here to
        keep one.

        Silent on the socket by construction: it runs before `backfill_start`,
        where the session shape in PROTOCOL.md has no heartbeat.
        """
        floor_ms = self.earliest_tick_ms()
        flags = self.tick_flags()
        cursor_s = before_s
        for call in range(1, OPENING_REACH_MAX_CALLS + 1):
            if cursor_s <= 0:
                return None, call - 1
            if floor_ms is not None and cursor_s * 1000 <= floor_ms:
                return None, call - 1
            from_s = max(0, cursor_s - OPENING_REACH_WINDOW_S)
            found = mt5.copy_ticks_range(self.symbol, from_s, cursor_s, flags)
            if found is None:
                log(
                    "BRIDGE_BACKFILL_REACH_FAILED",
                    symbol=self.symbol,
                    from_s=from_s,
                    to_s=cursor_s,
                    mt5_error=str(mt5.last_error()),
                    action="send_an_empty_block",
                )
                return None, call
            if len(found):
                return int(found[-1]["time_msc"]), call
            cursor_s = from_s
        return None, OPENING_REACH_MAX_CALLS

    # -- back-channel ------------------------------------------------------

    def pump_commands(self) -> None:
        """Serve whatever quantick asked for since the last poll.

        Called from the same loop that pumps ticks, so the terminal is touched
        from one thread only. The read never blocks: `select` with a zero
        timeout answers "is there anything?", and a quiet socket costs one
        syscall per pass.
        """
        for request in self.read_requests():
            if request.get("type") != LOAD_OLDER_TYPE:
                log("BRIDGE_UNKNOWN_COMMAND", got=str(request.get("type")), action="ignore")
                continue
            try:
                count = int(request["count"])
                before_ms = int(request["before_ms"])
            except (KeyError, TypeError, ValueError):
                log("BRIDGE_MALFORMED_COMMAND", got=json.dumps(request), action="ignore")
                continue
            self.serve_load_older(count, before_ms)

    def read_requests(self) -> list[dict]:
        """Every complete NDJSON command quantick has sent, decoded.

        Anything that does not survive the trip is skipped and logged rather
        than taken as a broken connection: the inbound direction carries a
        trader's clicks, and one unreadable click must not cost the session its
        tick stream. Two bounds keep that promise from being turned against the
        bridge — a line that never ends, and a peer that never stops — because
        the socket accepts whatever dialled the port.
        """
        requests: list[dict] = []
        reads = 0
        while reads < MAX_COMMAND_READS and select.select([self.sock], [], [], 0)[0]:
            reads += 1
            chunk = self.sock.recv(COMMAND_READ_BYTES)
            if not chunk:
                # quantick closed its side. The next outbound write raises and
                # the caller reconnects; there is nothing to decide here.
                break
            self.inbox += chunk
            while b"\n" in self.inbox:
                line, self.inbox = self.inbox.split(b"\n", 1)
                text = line.strip()
                if not text:
                    continue
                try:
                    decoded = json.loads(text)
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    log("BRIDGE_UNDECODABLE_COMMAND", error=str(error), action="skip")
                    continue
                # `json.loads` accepts bare numbers, strings, lists and null.
                # None of them has `.get`, and reaching for it would raise past
                # every `except` in the session loop and end the bridge.
                if not isinstance(decoded, dict):
                    log(
                        "BRIDGE_UNDECODABLE_COMMAND",
                        error=f"expected an object, got {type(decoded).__name__}",
                        action="skip",
                    )
                    continue
                requests.append(decoded)
            if len(self.inbox) > MAX_COMMAND_LINE_BYTES:
                log(
                    "BRIDGE_COMMAND_LINE_TOO_LONG",
                    bytes_held=len(self.inbox),
                    cap=MAX_COMMAND_LINE_BYTES,
                    action="drop_buffer",
                )
                self.inbox = b""
        return requests

    def serve_load_older(self, count: int, before_ms: int) -> None:
        """Send one block of ticks older than `before_ms`.

        Always sends both markers, even around an empty block: quantick shows a
        spinner from the moment it asks, and `history_end` is what stops it.

        The opening marker goes out *before* the walk, not after. The walk is
        the slow part — up to `LOAD_OLDER_MAX_PAGES` blocking terminal calls,
        and on the first click of a session a reach for the very oldest tick on
        disk — and the feed drops a session it has heard nothing from for its
        read timeout. Announcing first means the wait is spent inside a block
        the feed knows is coming, and the heartbeats below keep it that way.
        """
        wanted = max(1, min(count, LOAD_OLDER_MAX_TICKS))
        started = time.monotonic()
        # No count_hint: it is optional precisely so a bridge that has not
        # counted yet can still frame the block.
        self.send({"type": "history_start"})
        ticks, exhausted, scanned_to_ms, calls = self.walk_back(wanted, before_ms)
        page_sent_ms = self.server_now_ms()
        for tick in ticks:
            self.send_tick(tick, page_sent_ms)
        self.send(
            {
                "type": "history_end",
                "exhausted": exhausted,
                "scanned_to_ms": scanned_to_ms,
            }
        )
        log(
            "BRIDGE_LOAD_OLDER_SERVED",
            symbol=self.symbol,
            requested=count,
            sent=len(ticks),
            before_ms=before_ms,
            oldest_ms=int(ticks[0]["time_msc"]) if len(ticks) else None,
            scanned_to_ms=scanned_to_ms,
            exhausted=exhausted,
            terminal_calls=calls,
            elapsed_ms=int((time.monotonic() - started) * 1000),
        )

    def walk_back(self, wanted: int, before_ms: int) -> tuple[list, bool, int, int]:
        """Collect up to `wanted` ticks from before `before_ms`.

        Walks backwards in windows rather than asking for one wide range: the
        terminal returns a liquid contract's whole session for a day-wide
        request, and throwing away 99% of a million ticks to serve a page of
        2 000 is the slow way to answer a click. A window that comes back empty
        widens instead — an overnight gap or a weekend is dead time to cross,
        not history to search five minutes at a time.

        Returns the ticks ascending, whether the terminal has nothing older,
        how far back the search actually reached, and how many calls it took.

        The third value is the one that keeps paging moving. It is the oldest
        instant *searched*, not the oldest tick returned, and the two come apart
        constantly: a page trimmed to `wanted` drops what it found beyond it,
        and a pre-open stretch is thousands of ticks that map to no trades at
        all. A consumer paging from its oldest trade would ask for the same
        window forever; paging from this always advances.
        """
        floor_ms = self.earliest_tick_ms()
        flags = self.tick_flags()
        pages: list = []
        held = 0
        cursor_ms = before_ms
        window_s = LOAD_OLDER_FIRST_WINDOW_S
        calls = 0
        while held < wanted and calls < LOAD_OLDER_MAX_PAGES:
            if floor_ms is not None and cursor_ms <= floor_ms:
                break
            # Past the epoch there is nothing to ask for, and a terminal that
            # could not name its oldest tick would otherwise spend the whole
            # page budget asking about 1970.
            if cursor_ms <= 0:
                break
            calls += 1
            # Whole seconds is all copy_ticks_range takes, so the range is
            # rounded outward and the surplus filtered below. Rounding the other
            # way would silently drop every tick sharing the cursor's second.
            to_s = -(-cursor_ms // 1000)
            from_s = max(0, cursor_ms // 1000 - window_s)
            found = mt5.copy_ticks_range(self.symbol, from_s, to_s, flags)
            if found is None:
                log(
                    "BRIDGE_LOAD_OLDER_FAILED",
                    symbol=self.symbol,
                    from_s=from_s,
                    to_s=to_s,
                    mt5_error=str(mt5.last_error()),
                    action="answer_with_what_is_in_hand",
                )
                break
            fresh = [t for t in found if int(t["time_msc"]) < cursor_ms]
            if len(fresh):
                pages.append(fresh)
                held += len(fresh)
                # A productive window is the right size; widening from here
                # would overshoot the page and spend the terminal's time on
                # ticks about to be trimmed.
                window_s = LOAD_OLDER_FIRST_WINDOW_S
            else:
                window_s = min(
                    window_s * LOAD_OLDER_WINDOW_GROWTH, LOAD_OLDER_MAX_WINDOW_S
                )
            cursor_ms = from_s * 1000
            # The walk can take seconds and nothing else runs while it does. A
            # heartbeat here is what keeps the feed from declaring the bridge
            # silent mid-answer and dropping the session the answer belongs to.
            self.maybe_heartbeat()

        # Oldest page first, and the surplus trimmed off the *front*: the ticks
        # nearest the chart are the ones the trader is about to look at.
        ticks = [tick for page in reversed(pages) for tick in page]
        trimmed = max(0, len(ticks) - wanted)
        if trimmed:
            ticks = ticks[trimmed:]

        # "Nothing older exists" needs both halves: the walk reached the
        # terminal's own floor, *and* everything it found is in this block.
        # Without the second half a dense window that overshot `wanted` would
        # claim the end of the tape while the ticks proving otherwise were the
        # ones just trimmed — and quantick retires the button on this flag, so
        # the trader could not click their way back to them.
        exhausted = floor_ms is not None and cursor_ms <= floor_ms and trimmed == 0

        # Same reasoning for how far the search reached: after a trim the walk's
        # cursor is behind ticks that were dropped, so reporting it would let
        # the consumer skip past history it never received. What it can honestly
        # claim then is the oldest tick actually sent.
        if trimmed:
            scanned_to_ms = int(ticks[0]["time_msc"])
        else:
            scanned_to_ms = cursor_ms
        return ticks, exhausted, scanned_to_ms, calls

    def earliest_tick_ms(self) -> int | None:
        """The oldest tick the terminal holds for this symbol, or None.

        Asked once per session and cached, the failure included: a terminal that
        cannot answer will not start answering mid-session, and the walk asks on
        every request.
        """
        if self.earliest_known:
            return self.earliest_ms
        self.earliest_known = True
        found = mt5.copy_ticks_from(self.symbol, 0, 1, mt5.COPY_TICKS_ALL)
        if found is None or not len(found):
            log(
                "BRIDGE_TICK_FLOOR_UNKNOWN",
                symbol=self.symbol,
                mt5_error=str(mt5.last_error()),
                note="paging still works; it just never claims to have reached the end",
            )
            return None
        self.earliest_ms = int(found[0]["time_msc"])
        log("BRIDGE_TICK_FLOOR", symbol=self.symbol, earliest_ms=self.earliest_ms)
        return self.earliest_ms

    @staticmethod
    def rates_error_code() -> int:
        """The numeric part of the terminal's last error, or 0 if unreadable."""
        error = mt5.last_error()
        try:
            return int(error[0])
        except (TypeError, IndexError, ValueError):
            return 0

    def fetch_rates(self, from_s: int, now_s: int) -> tuple[list, int, bool]:
        """Walk M1 history backwards in bounded pages.

        `copy_rates_range` over the whole span cannot be used, and the reason is
        not obvious: the terminal validates a request's **potential** bar count
        against its "Max bars in chart" setting and returns a hard error rather
        than truncating to what it will serve. Probed 2026-08-03 against an XP
        terminal capped at 100 000 bars — a 90-day M1 range is 129 600 potential
        slots and returned `(-2, 'Terminal: Invalid params')`, while the same
        call over one day returned its 563 bars. So every session was failing on
        the request itself, never on the data.

        `copy_rates_from(symbol, timeframe, anchor, count)` returns the `count`
        bars ending at `anchor`, ascending, and takes the same validation
        against `count` — so pages are counted well under any sane cap
        (RATES_PAGE_BARS) rather than sized to the span, and walked backwards
        from now until one of four things stops them: the span is covered, the
        terminal returns a short page (its history is exhausted), the caller's
        bar cap is reached, or the page budget runs out.

        Returns the merged bars ascending by time, how many pages were
        requested, and whether a page failed after others had succeeded.
        Merging through a dict keyed by bar time settles the overlap that a
        backwards walk produces at every seam, deterministically.
        """
        by_time: dict[int, object] = {}
        anchor = now_s
        pages = 0
        partial = False
        # Narrowed in place if this terminal refuses the starting width.
        page_bars = RATES_PAGE_BARS
        while pages < RATES_MAX_PAGES:
            chunk = mt5.copy_rates_from(self.symbol, mt5.TIMEFRAME_M1, anchor, page_bars)
            pages += 1
            # A refusal about the request itself, not about the data: the page
            # is wider than this terminal's Max-bars setting allows. Halve and
            # try again, and keep the smaller size for the rest of the walk.
            while (
                chunk is None
                and self.rates_error_code() == MT5_INVALID_PARAMS
                and page_bars > RATES_MIN_PAGE_BARS
            ):
                page_bars = max(page_bars // 2, RATES_MIN_PAGE_BARS)
                log(
                    "BRIDGE_RATES_PAGE_TOO_WIDE",
                    symbol=self.symbol,
                    page=pages,
                    retry_with=page_bars,
                    action="halve_and_retry",
                    hint="the terminal's Max bars in chart is below the page size",
                )
                chunk = mt5.copy_rates_from(
                    self.symbol, mt5.TIMEFRAME_M1, anchor, page_bars
                )
            if chunk is None:
                # A page that fails after others succeeded costs the oldest end
                # of the window, not the block: what was already merged is real
                # and is worth more than the nothing that refusing would give.
                partial = bool(by_time)
                log(
                    "BRIDGE_RATES_PAGE_FAILED",
                    symbol=self.symbol,
                    page=pages,
                    anchor_ms=anchor * 1000,
                    collected=len(by_time),
                    mt5_error=str(mt5.last_error()),
                    action="send_what_was_collected" if by_time else "give_up",
                )
                break
            if len(chunk) == 0:
                break
            for rate in chunk:
                by_time[int(rate["time"])] = rate
            oldest = int(chunk[0]["time"])
            if oldest <= from_s:
                break  # the requested span is covered
            if len(chunk) < page_bars:
                break  # the terminal has no history older than this
            if len(by_time) >= self.args.rates_max_bars:
                break  # the cap decides the rest; the newest are kept below
            # One minute before the oldest bar seen, so the next page ends where
            # this one began instead of repeating it wholesale.
            anchor = oldest - 60
        else:
            log(
                "BRIDGE_RATES_PAGE_BUDGET_SPENT",
                symbol=self.symbol,
                max_pages=RATES_MAX_PAGES,
                collected=len(by_time),
                action="send_what_was_collected",
            )
            partial = bool(by_time)

        # Ascending, and clipped to what was actually asked for: the last page
        # of a backwards walk usually reaches further back than the span.
        return ([by_time[t] for t in sorted(by_time) if t >= from_s], pages, partial)

    def send_rates(self) -> None:
        """Historical M1 candles, so the time pane opens with real context.

        Ticks answer "what just happened"; this answers "what has been
        happening", and the two have very different shapes. Three months of
        ticks is tens of millions of lines and is not on offer at any price —
        three months of one-minute candles is about 130 000, which fits on the
        socket in a few hundred batched lines.

        Fetched in pages, not in one call: see `fetch_rates` for the terminal
        setting that refuses the whole-span request outright.

        Batched, and bounded: quantick drops a session whose line exceeds 64
        KiB, so a whole block on one line would take the connection down with
        it. MAX_BARS_PER_RATE_LINE is sized against that cap — see
        bridge/mt5/PROTOCOL.md for the arithmetic.

        Volume follows what the venue actually prints, matching how live ticks
        are treated for the same instrument: an exchange tape reports traded
        size, and a quote-only CFD — which prints nothing — reports its tick
        count, the same one synthetic unit per tick the live path charts.
        """
        now_s = int(time.time() + self.offset_s)
        from_s = now_s - self.args.rates_months * DAYS_PER_MONTH * 86400
        rates, pages, partial = self.fetch_rates(from_s, now_s)
        if not rates:
            log(
                "BRIDGE_RATES_FAILED",
                symbol=self.symbol,
                pages=pages,
                mt5_error=str(mt5.last_error()),
                hint=(
                    "the terminal refused the request itself; raise Max bars in "
                    "chart (Tools > Options > Charts) or lower --rates-months"
                    if self.rates_error_code() == MT5_INVALID_PARAMS
                    else "the terminal returned no M1 history for this symbol"
                ),
            )
            # The hello already promised candles, so silence here would leave the
            # feed holding nothing while advertising nothing — indistinguishable
            # from a block still on its way. An empty pair delivers the absence.
            self.send(
                {"type": "rates_start", "interval_ms": M1_INTERVAL_MS, "count_hint": 0}
            )
            self.send({"type": "rates_end"})
            return

        available = len(rates)
        if available > self.args.rates_max_bars:
            rates = rates[-self.args.rates_max_bars :]
            # Clipped is short in the same sense a failed page is short: bars
            # that exist and are not being delivered.
            partial = True
            log(
                "BRIDGE_RATES_TRUNCATED",
                symbol=self.symbol,
                available=available,
                sending=len(rates),
                dropped_oldest=available - len(rates),
                action="keep_newest",
            )

        quotes_only = self.tape == "quotes"
        # How often a tape instrument reported no real volume and the tick count
        # stood in. Counted rather than silent: a WIN$N block that fell back on
        # every bar is a different chart from one that never did, and the two
        # are indistinguishable once the bars are drawn.
        fell_back = 0
        self.send(
            {
                "type": "rates_start",
                "interval_ms": M1_INTERVAL_MS,
                "count_hint": len(rates),
            }
        )
        batch = []
        sent = 0
        for rate in rates:
            volume = int(rate["tick_volume"] if quotes_only else rate["real_volume"])
            # A terminal that reports no real volume on a tape instrument
            # leaves the bar unmeasurable; the tick count is the honest
            # fallback and matches what the live path charts for it.
            if volume <= 0:
                volume = int(rate["tick_volume"])
                if not quotes_only:
                    fell_back += 1
            batch.append(
                [
                    int(rate["time"]) * 1000,
                    self.price(rate["open"]),
                    self.price(rate["high"]),
                    self.price(rate["low"]),
                    self.price(rate["close"]),
                    str(volume),
                ]
            )
            if len(batch) >= MAX_BARS_PER_RATE_LINE:
                self.send({"type": "rate", "bars": batch})
                sent += len(batch)
                batch = []
        if batch:
            self.send({"type": "rate", "bars": batch})
            sent += len(batch)
        # The flag rather than only the log: a consumer cannot read stderr, and
        # "this is all there is" and "this is all I could get" are different
        # charts. Omitted when whole, so a feed predating the field is
        # unaffected.
        end: dict = {"type": "rates_end"}
        if partial:
            end["partial"] = True
        self.send(end)
        # Requested versus covered, so "this contract only has four weeks of
        # history" is visible rather than inferred from a short chart. A young
        # B3 contract genuinely has less than the ninety days asked for, and
        # that is not a failure — but it must not look like one either.
        oldest_s = int(rates[0]["time"])
        newest_s = int(rates[-1]["time"])
        log(
            "BRIDGE_RATES_SENT",
            symbol=self.symbol,
            interval_ms=M1_INTERVAL_MS,
            bars=sent,
            pages=pages,
            partial=partial,
            requested_span_days=self.args.rates_months * DAYS_PER_MONTH,
            covered_span_days=round((newest_s - oldest_s) / 86400.0, 1),
            oldest_ms=oldest_s * 1000,
            newest_ms=newest_s * 1000,
            months=self.args.rates_months,
            volume_source="tick_volume" if quotes_only else "real_volume",
            fell_back_to_tick_volume=fell_back,
        )

    def send_tick(self, tick, sent_ms: int) -> None:
        """Queue one tick, stamped with when this bridge handed it over.

        `sent_ms` is on the same server clock as `time_ms`, so the reader can
        split one end-to-end delay into the part spent inside the terminal
        (`sent_ms - time_ms`) and the part spent on the wire (its own arrival
        minus `sent_ms`). Those are opposite faults with opposite fixes, and a
        single arrival figure cannot tell them apart. See PROTOCOL.md.

        Stamped once per pump pass rather than once per tick: it is the instant
        the batch left, never later than the line itself.
        """
        self.seq += 1
        self.send(
            {
                "type": "tick",
                "seq": self.seq,
                "time_ms": int(tick["time_msc"]),
                "sent_ms": sent_ms,
                "bid": self.price(float(tick["bid"])),
                "ask": self.price(float(tick["ask"])),
                "last": self.price(float(tick["last"])),
                "volume": int(tick["volume"]),
                "flags": int(tick["flags"]),
            }
        )
        self.ticks_sent += 1

    # -- pumps ------------------------------------------------------------

    def tick_flags(self) -> int:
        """Which ticks this session asks the terminal for.

        quantick charts a printing venue from `last` and `volume` and drops
        every tick without a LAST bit (`crates/feed-mt5/src/map.rs`), so a
        quote-only tick is data the other end deletes on arrival.

        How much that saves depends on the broker. On the one measured here it
        saves nothing: the committed WIN$N recording (1500 live ticks, pulled
        with COPY_TICKS_ALL) is 100% flags=1080 — every tick carries LAST.
        It is still the right request, and the same reasoning the load-older
        path below already runs on: free where there is nothing to filter, and
        a bound on the wire where the broker does quote separately. A
        broker-quoted symbol prints nothing at all, so there the quotes *are*
        the data and every tick is wanted.
        """
        return mt5.COPY_TICKS_TRADE if self.tape == "trades" else mt5.COPY_TICKS_ALL

    def pump_ticks(self) -> None:
        """Forward every tick newer than the cursor, exactly once.

        One `copy_ticks_from` returns at most the count it was asked for, so a
        full answer does not mean the terminal is drained — it means the batch
        was truncated. The pass keeps asking while the answers come back full,
        because leaving the remainder for the next pass is how a burst turns
        into a tape running whole seconds behind a book that never batches at
        all.

        Two things bound it. `MAX_PUMP_ROUNDS`, so one pass cannot own the loop
        the book and the heartbeat share. And the request itself widens when a
        round comes back full having forwarded nothing new: that is the shape of
        a single second holding more ticks than one batch, and since this API's
        floor is whole seconds, advancing the cursor cannot walk past it — only
        asking deeper can. Without the widening the tape wedges inside that
        second permanently and silently, which is worse than the delay this
        method exists to remove.
        """
        # One stamp per round, not per pass: a pass that forwards sixteen
        # batches takes long enough that the terminal hands over ticks during
        # it, and those would carry a `time_ms` newer than a stamp taken before
        # the pass began — a negative delay inside the terminal, which the
        # reader would charge to the wire.
        rounds = 0
        count = TICKS_PER_PUMP_ROUND
        while rounds < MAX_PUMP_ROUNDS:
            rounds += 1
            sent_ms = self.server_now_ms()
            # The request floor is whole seconds; the (msc, count-at-msc)
            # cursor below is what actually guarantees no tick is sent twice.
            ticks = mt5.copy_ticks_from(
                self.symbol, self.cursor_msc // 1000, count, self.tick_flags()
            )
            if ticks is None or not len(ticks):
                return
            at_cursor_seen = 0
            forwarded = 0
            for tick in ticks:
                msc = int(tick["time_msc"])
                if msc < self.cursor_msc:
                    continue
                if msc == self.cursor_msc:
                    at_cursor_seen += 1
                    if at_cursor_seen <= self.sent_at_cursor:
                        continue
                    self.send_tick(tick, sent_ms)
                    self.sent_at_cursor += 1
                    forwarded += 1
                else:
                    self.send_tick(tick, sent_ms)
                    self.cursor_msc = msc
                    self.sent_at_cursor = 1
                    at_cursor_seen = 0
                    forwarded += 1
            if len(ticks) < count:
                return  # the terminal said it has nothing more
            if forwarded == 0:
                # A full batch with nothing new in it: every tick in it is
                # already sent and they all share this second, so the same
                # request will keep returning the same ticks. Ask deeper.
                if count >= MAX_TICKS_PER_REQUEST:
                    log(
                        "BRIDGE_PUMP_SECOND_TOO_DENSE",
                        symbol=self.symbol,
                        second=self.cursor_msc // 1000,
                        asked=count,
                        hint="one second holds more ticks than this bridge can request",
                    )
                    return
                count = min(count * 2, MAX_TICKS_PER_REQUEST)
        # Hitting the round bound is the tape arriving faster than one pass can
        # forward it — the reading behind a late chart, so it is said aloud
        # rather than silently retried on the next pass.
        self.pump_round_limits += 1
        log(
            "BRIDGE_PUMP_LIMIT",
            symbol=self.symbol,
            rounds=MAX_PUMP_ROUNDS,
            ticks_per_round=count,
            passes=self.pump_round_limits,
        )

    def pump_book(self) -> None:
        """Send one complete DOM image, when it differs from the last one."""
        if not self.book_subscribed:
            return
        now = time.monotonic() * 1000.0
        if now - self.last_book_ms < self.args.book_min_interval_ms:
            return
        book = mt5.market_book_get(self.symbol)
        if not book:
            return

        bids, asks = [], []
        for item in book:
            # BOOK_TYPE_*_MARKET rows are orders waiting to cross, not resting
            # liquidity at a price: they carry no level to draw.
            if item.type == mt5.BOOK_TYPE_BUY:
                side = bids
            elif item.type == mt5.BOOK_TYPE_SELL:
                side = asks
            else:
                continue
            if item.price <= 0:
                continue
            volume = item.volume_dbl if item.volume_dbl > 0 else float(item.volume)
            side.append([self.price(item.price), _volume_text(volume)])

        body = json.dumps({"bids": bids, "asks": asks}, separators=(",", ":"))
        if body == self.last_book_body:
            self.book_skipped += 1
            return
        self.last_book_body = body
        self.last_book_ms = now

        self.book_seq += 1
        tick = mt5.symbol_info_tick(self.symbol)
        stamp = max(
            int(tick.time_msc) if tick is not None else 0,
            self.server_now_ms(),
        )
        self.send(
            {
                "type": "book",
                "seq": self.book_seq,
                "time_ms": stamp,
                "bids": bids,
                "asks": asks,
            }
        )
        self.book_sent += 1

    def maybe_heartbeat(self) -> None:
        now = time.monotonic()
        if now - self.last_heartbeat < self.args.heartbeat_seconds:
            return
        self.last_heartbeat = now
        self.send(
            {
                "type": "heartbeat",
                "seq_last": self.seq,
                "time_ms": self.server_now_ms(),
                "ticks_sent": self.ticks_sent,
                "server_utc_offset_s": self.offset_s,
            }
        )
        if self.book_subscribed:
            log(
                "BRIDGE_BOOK_STATS",
                symbol=self.symbol,
                images_sent=self.book_sent,
                images_skipped=self.book_skipped,
            )

    def close(self, reason: str) -> None:
        if self.book_subscribed:
            mt5.market_book_release(self.symbol)
            self.book_subscribed = False
        try:
            self.send({"type": "bye", "reason": reason})
        except OSError:
            pass


def _volume_text(volume: float) -> str:
    """Whole contracts print as integers; fractional lots keep two decimals."""
    if abs(volume - round(volume)) < 1e-9:
        return str(int(round(volume)))
    return f"{volume:.2f}"


# --------------------------------------------------------------------------
# Loop
# --------------------------------------------------------------------------


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
                session.maybe_heartbeat()
                # Sleep to the nearest due deadline instead of spinning: the
                # terminal reads cost microseconds, the waiting is the loop.
                idle = min(next_tick, next_book) - time.monotonic()
                if idle > 0:
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
        help="history window to send on connect (default covers a whole B3 session)",
    )
    parser.add_argument(
        "--backfill-max-ticks",
        type=int,
        default=1_000_000,
        help=(
            "hard cap on backfilled ticks; the newest ones win and the rest "
            "stays one 'load older' away"
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
