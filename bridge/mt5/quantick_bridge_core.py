"""Everything the bridge's session parts share: its dials, its log line and
its dealings with the terminal itself.

Split out of ``quantick_bridge.py`` so no one file owns the whole bridge. What
lives here is what more than one part needs — the tuning constants, the one
log function, the failure type, the clock-offset cache and the calls that
attach to a running MetaTrader terminal. The session's own behaviour lives in
the four mixins beside this file; ``quantick_bridge.py`` assembles them and
stays the only entry point.
"""

from __future__ import annotations

import json
import sys
import time

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

# A stretch with no prints longer than this reads as the market having been
# closed rather than as a quiet patch, and the print on its newer side is a
# session's first.
#
# One hour, and deliberately the *same* hour as the app's
# `history_reach::SESSION_GAP_MS`. Both ends of one tape decide where a day
# began, and two constants that merely happen to agree today would drift the
# first time either was tuned; `crates/guards/tests/session_gap_agreement.rs`
# fails when they do. A venue with a real lunch break longer than this reads
# that break as a close, which costs the trader one extra press and never
# invents data.
SESSION_GAP_MS = 60 * 60 * 1000

# What `--backfill-max-ticks` defaults to, and why it is not a span.
#
# The opening block is a *session* now, so the number of prints in it is the
# market's business rather than quantick's. This bounds the memory that costs,
# nothing else: it exists so a pathological day cannot take the terminal, the
# bridge and the chart down together, not to decide how much of the day the
# trader is allowed to see. Sized against the real thing — WINV26 printed
# 1 525 621 trades on 2026-08-31, and B3's mini index is the densest tape this
# bridge serves — which leaves this a little over two and a half times the
# busiest session measured. When it does bite, the newest prints win and the
# bridge says so; it never trims in silence.
DEFAULT_BACKFILL_MAX_TICKS = 4_000_000

# How far the opening walk may reach when the tape shows no close at all.
#
# The answer for a market that never closes: a 24/5 CFD has no overnight gap,
# so the walk would otherwise step back until its call budget ran out. Two days
# is past any "previous session" a continuous market has — the same span, and
# for the same reason, as the app's `history_reach::MAX_CAMPAIGN_SPAN_MS`.
SESSION_WALK_MAX_SPAN_MS = 48 * 60 * 60 * 1000

# Ticks in one slice of the opening block.
#
# The session is fetched in one go -- the terminal returns a whole day in about
# 350 ms -- but it cannot be *sent* in one go. Serialising and writing a real
# WINV26 session takes about eleven seconds, and sent as a single block that is
# eleven seconds with nothing on the chart, which a trader cannot tell apart
# from a bridge that failed to start. So the newest slice goes out as the
# opening block and the rest follows it, newest-first, between passes of the
# loop that is also pumping live ticks.
#
# Two hundred thousand prints, and the number is about the *consumer's* cost
# rather than the bridge's. Every slice the chart accepts is prepended through
# `ChartState::prepend_history`, which re-cuts every bar the chart already
# holds -- so the work of a fill is the slice count times a growing tape, and
# halving the slices nearly halves it. Measured against the live terminal on a
# 1 525 621-print session: at 50 000 (thirty-one slices) the chart fell to 43
# fps with a 142 ms worst frame and raised three `APP_SLOW_FRAMES`; the first
# slice still paints in under a second either way, because that is the opening
# block and not a slice at all.
#
# Bounded above by what the consumer will accept in one block --
# `MAX_TRADES_PER_PAGE` in `crates/feed-mt5/src/stream.rs` is 250 000, and a
# slice past it would be silently trimmed on arrival, which is exactly the
# quiet cut the opening block was rebuilt to abolish. `--opening-slice-ticks`
# is clamped to this for the same reason.
DEFAULT_OPENING_SLICE_TICKS = 200_000

# The largest block the feed will take whole, from
# `crates/feed-mt5/src/stream.rs`'s `MAX_TRADES_PER_PAGE`. Duplicated across a
# language boundary the repository cannot type-check, so it is pinned by
# `crates/guards/tests/session_gap_agreement.rs`'s
# `the_slice_cap_matches_what_the_feed_will_accept` rather than trusted: a
# slice past the feed's cap is trimmed on arrival and the surplus dropped with
# only a warn line, which is the quiet cut this block was rebuilt to abolish.
MAX_SLICE_TICKS_THE_FEED_ACCEPTS = 250_000

# Windows one opening walk may spend: the span above, in gap-wide steps.
# Derived rather than chosen, so raising the span cannot leave a budget behind
# that silently stops the walk before it.
SESSION_WALK_MAX_WINDOWS = SESSION_WALK_MAX_SPAN_MS // SESSION_GAP_MS

# Bytes queued before the outbound buffer is handed to the socket without
# waiting for the loop's own flush.
#
# The buffer exists because one `sendall` per tick costs more than everything
# else the opening block does put together (47 s of 62 s over a real WINV26
# session). This bounds how much of it is held: a quarter of a megabyte is
# about 2 500 ticks, small enough that a live print is never sitting in memory
# for a noticeable time and large enough that the syscall count drops by three
# orders of magnitude.
SEND_BUFFER_BYTES = 256 * 1024

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


# --------------------------------------------------------------------------
# Wire
# --------------------------------------------------------------------------

def _volume_text(volume: float) -> str:
    """Whole contracts print as integers; fractional lots keep two decimals."""
    if abs(volume - round(volume)) < 1e-9:
        return str(int(round(volume)))
    return f"{volume:.2f}"


# --------------------------------------------------------------------------
# Loop
# --------------------------------------------------------------------------
