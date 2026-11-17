#!/usr/bin/env python3
"""Carry quantick's orders to the logged-in MetaTrader 5 terminal.

The execution channel: a second, separate socket beside the tick feed. It
attaches to the terminal that is already running and already logged in, dials
quantick's execution listener on its own port, and from then on does two
things — answers each order request with the terminal's verbatim reply, and
pushes the account, the positions, the working orders and the deals of the
watched symbol whenever they change.

    python bridge/mt5/quantick_bridge_exec.py --port 9180

The wire contract is the "Execution channel" section of PROTOCOL.md, and
``crates/broker-mt5`` is its executable counterpart.

Three promises shape this file:

- **quantick listens and this bridge dials.** A local process that connects to
  quantick's port can at worst show it a false account; a bridge that listened
  would let any local process place orders. No credentials exist anywhere in
  this path, and nothing leaves ``127.0.0.1``.
- **A request id is answered once.** Every reply is remembered by the request's
  id, and a request whose id was already answered — quantick resending after a
  dropped link — gets the remembered reply again. It never reaches
  ``order_send`` a second time, and a resend (``replay_only``) this process
  never answered is reported ``unknown`` rather than executed late. The memory lives as long as this process; the
  ``instance`` in the hello tells quantick when a restart has emptied it.
- **Nothing runs after quantick stopped waiting.** An order request carries
  ``expires_ms``, the moment quantick could declare this bridge lost for its
  silence; one reached at or past it is refused unexecuted, so a bridge that
  hung and wakes on the same socket never fills what quantick already gave up
  on.
- **Nothing is decided here.** The terminal's return code and comment travel
  verbatim; which ones mean success, and what is the trader's and what is
  another program's, is quantick's to judge. The one refusal this file makes
  on its own is a close for a position that is not there: on a netting account
  that deal would *open* a position instead, which no caller ever means.

Its own process, not a thread of the tick bridge: a history page can keep the
feed loop busy for seconds, and an order must never wait behind one.
"""
from __future__ import annotations

import argparse
import json
import socket
import time
import uuid
from collections import OrderedDict

from quantick_bridge_core import (
    BridgeExit,
    _volume_text,
    log,
    mt5,
)
from quantick_bridge_transport import TransportMixin

EXEC_SCHEMA_VERSION = 2
EXEC_BRIDGE_NAME = "quantick-mt5-exec-py"
EXEC_BRIDGE_VERSION = "0.1.0"

#: quantick's execution listener when nothing else is configured. Mirrors the
#: default `exec_port` of `[broker.metatrader]` in `crates/app/config/feeds.toml`.
DEFAULT_EXEC_PORT = 9180

#: Answered request ids remembered for an idempotent retry. A retry follows its
#: original within one reconnect, so this only has to outlast a burst of
#: clicks; the bound is what keeps a long session's memory flat.
REPLY_CACHE_SIZE = 4096

#: Longest request id accepted. quantick's ids are a session tag and a counter.
MAX_REQUEST_ID_CHARS = 64

#: MetaTrader truncates an order comment past this many characters.
MAX_COMMENT_CHARS = 31

#: How far back, and ahead, the deal history is read. Server time is not UTC
#: and the package takes epoch seconds in server time, so the window is wide
#: on both sides rather than exact; quantick ignores deals it has already seen.
DEAL_WINDOW_S = 2 * 24 * 60 * 60

#: Between sweeps, history is read from this long before the newest deal of
#: the watched symbol already seen, not the whole window again every pass: a
#: deal the terminal writes a moment late is still inside it.
DEAL_OVERLAP_S = 5 * 60

#: Seconds between two reads of the whole window. A deal written later than
#: the overlap, stamped older than the newest one seen, is found by the next
#: sweep; deals already sent are never sent again, so it arrives once.
DEAL_SWEEP_S = 10.0

#: Passes a vanished order may wait for a history record of how it ended before
#: it is reported with state "unknown". The terminal writes history a moment
#: after it drops the order from the working list; this bounds how long that
#: may take.
MAX_UNRESOLVED_PASSES = 200

#: Seconds between two state pushes when nothing changed, so quantick can tell
#: a quiet account from a dead bridge.
STATE_HEARTBEAT_S = 2.0

#: The request types this channel answers.
REQUEST_TYPES = ("watch", "submit", "cancel", "modify", "sltp", "close")

#: The requests that place or move an order at a price of their own.
PRICED_REQUEST_TYPES = ("submit", "modify")

# Terminal enumerations, by documented value. Read from the package when it has
# them so a renumbering would follow the terminal, but never required: the
# suites run on machines with no terminal at all.
ORDER_TYPE_KINDS = {
    0: ("buy", "market"),
    1: ("sell", "market"),
    2: ("buy", "limit"),
    3: ("sell", "limit"),
    4: ("buy", "stop"),
    5: ("sell", "stop"),
    6: ("buy", "stop_limit"),
    7: ("sell", "stop_limit"),
    8: ("none", "close_by"),
}
TRADE_MODES = {0: "demo", 1: "contest", 2: "real"}
MARGIN_MODES = {0: "netting", 1: "exchange", 2: "hedging"}
DEAL_ENTRIES = {0: "in", 1: "out", 2: "inout", 3: "out_by"}
DEAL_REASONS = {
    0: "client",
    1: "mobile",
    2: "web",
    3: "expert",
    4: "sl",
    5: "tp",
    6: "so",
    7: "rollover",
    8: "vmargin",
    9: "split",
}
ORDER_STATES = {
    0: "started",
    1: "placed",
    2: "canceled",
    3: "partial",
    4: "filled",
    5: "rejected",
    6: "expired",
}
#: The history states that say how an order ended. Any other - started,
#: placed, or a request still in progress - is no verdict yet.
FINAL_ORDER_STATES = frozenset(("canceled", "partial", "filled", "rejected", "expired"))


def const(terminal, name: str, fallback: int) -> int:
    """A terminal constant, or its documented value when the package lacks it."""
    return int(getattr(terminal, name, fallback))


class ReplyCache:
    """Answered requests by id, oldest forgotten first."""

    def __init__(self, capacity: int = REPLY_CACHE_SIZE) -> None:
        self.capacity = capacity
        self.replies: OrderedDict[str, dict] = OrderedDict()

    def get(self, request_id: str) -> dict | None:
        reply = self.replies.get(request_id)
        return None if reply is None else dict(reply)

    def put(self, request_id: str, reply: dict) -> None:
        self.replies[request_id] = dict(reply)
        self.replies.move_to_end(request_id)
        while len(self.replies) > self.capacity:
            self.replies.popitem(last=False)


class RequestError(Exception):
    """A request this bridge cannot even put to the terminal."""


class ExecSession(TransportMixin):
    """One connection to quantick's execution listener."""

    def __init__(self, sock, terminal, cache: ReplyCache, instance: str, symbol: str = "") -> None:
        self.sock = sock
        self.terminal = terminal
        self.cache = cache
        self.instance = instance
        self.symbol = symbol
        self.outbox = bytearray()
        self.inbox = b""
        # Deals already pushed on this connection. A new connection starts
        # empty on purpose: quantick dedups by ticket, and a reconnect must
        # hand it every deal that happened while the link was down.
        self.sent_deals: set[int] = set()
        # Order tickets in the last pushed working list, tickets replies named
        # that no list has shown yet, and vanished ones still waiting for their
        # history record, with the passes waited.
        self.known_orders: set[int] = set()
        self.sent_orders: set[int] = set()
        self.unresolved: dict[int, int] = {}
        # Server time of the watched symbol's newest deal seen; between
        # sweeps of the whole window, history is read from just before it.
        self.newest_deal_s: int | None = None
        self.deals_swept_at: float | None = None
        self.last_state_body: str | None = None
        self.last_state_at = 0.0
        self.digits_by_symbol: dict[str, int] = {}

    # -- framing -----------------------------------------------------------

    def hello(self) -> None:
        self.send(
            {
                "type": "exec_hello",
                "schema": EXEC_SCHEMA_VERSION,
                "bridge": EXEC_BRIDGE_NAME,
                "bridge_version": EXEC_BRIDGE_VERSION,
                "instance": self.instance,
            }
        )
        self.flush()

    def close(self, reason: str) -> None:
        try:
            self.send({"type": "bye", "reason": reason})
            self.flush()
        except OSError:
            pass

    def pump(self) -> None:
        """Answer what arrived, then push the state if it moved."""
        for request in self.read_requests():
            self.send(self.answer(request))
        self.push_state()
        self.flush()

    # -- requests ----------------------------------------------------------

    def answer(self, request: dict) -> dict:
        """The reply to one request: remembered, or the terminal's own."""
        request_id = request.get("id")
        if not isinstance(request_id, str) or not request_id or len(request_id) > MAX_REQUEST_ID_CHARS:
            log("EXEC_REQUEST_WITHOUT_ID", got=json.dumps(request)[:200], action="refuse")
            return {"type": "reply", "id": None, "retcode": None, "error": "request has no usable id"}
        remembered = self.cache.get(request_id)
        if remembered is not None:
            remembered["cached"] = True
            log("EXEC_REQUEST_REPEATED", id=request_id, action="answer_from_memory")
            return remembered
        if request.get("replay_only") is True:
            # Quantick asks again about a request whose answer it lost. This
            # process never answered it, so it never executed it: executing it
            # now could fill after the trader stood down. Remembered, so the
            # id can never run later either.
            reply = {
                "type": "reply",
                "id": request_id,
                "retcode": None,
                "unknown": True,
                "error": "never received: not executed",
                "cached": False,
            }
            self.cache.put(request_id, reply)
            log("EXEC_REPLAY_UNANSWERED", id=request_id, action="report_not_executed")
            return reply
        try:
            reply = self.execute(request)
        except RequestError as error:
            reply = {"type": "reply", "id": request_id, "retcode": None, "error": str(error)}
        reply["cached"] = False
        self.cache.put(request_id, reply)
        log(
            "EXEC_REQUEST_ANSWERED",
            id=request_id,
            request=str(request.get("type")),
            retcode=reply.get("retcode"),
            comment=reply.get("comment", reply.get("error")),
        )
        return reply

    def execute(self, request: dict) -> dict:
        kind = request.get("type")
        if kind not in REQUEST_TYPES:
            raise RequestError(f"unknown request type {kind!r}")
        if kind == "watch":
            symbol = request.get("symbol")
            if not isinstance(symbol, str) or not symbol:
                raise RequestError("watch needs a symbol")
            if symbol != self.symbol:
                self.symbol = symbol
                self.sent_deals.clear()
                self.known_orders.clear()
                self.sent_orders.clear()
                self.unresolved.clear()
                self.newest_deal_s = None
                self.deals_swept_at = None
                self.last_state_body = None
                if not self.terminal.symbol_select(symbol, True):
                    log("EXEC_SYMBOL_SELECT_FAILED", symbol=symbol, mt5_error=str(self.terminal.last_error()))
            return {"type": "reply", "id": request["id"], "retcode": None, "watching": symbol}
        symbol = request.get("symbol") if isinstance(request.get("symbol"), str) else self.symbol
        if kind in PRICED_REQUEST_TYPES and self.digits(symbol) is None:
            # A price typed for the symbol is in its digits; sent without
            # knowing them, it could be one the trader never meant. A cancel,
            # a close or a stop change is always let through: an exit must
            # never wait on symbol information.
            raise RequestError(
                f"the terminal has no symbol information for {symbol or 'the watched symbol'}: "
                "refusing rather than guessing its price digits"
            )
        trade_request = getattr(self, f"build_{kind}")(request)
        self.snap_to_tick(trade_request, symbol)
        refuse_if_expired(request)
        return self.send_order(request["id"], trade_request)

    def snap_to_tick(self, trade: dict, symbol: str | None) -> None:
        """Put every price the request carries on the symbol's tick grid.

        A price dragged on the chart is wherever the pointer was (172140.82);
        B3 refuses a stop or a target off its grid as "Invalid stops" (10016),
        so the nearest tick is what the trader meant. Unknown symbol
        information leaves the prices as they are: an exit never waits on it.
        """
        name = symbol or self.symbol
        info = self.terminal.symbol_info(name) if name else None
        tick = float(getattr(info, "trade_tick_size", 0.0) or 0.0) if info is not None else 0.0
        if tick <= 0.0:
            return
        digits = int(getattr(info, "digits", 0) or 0)
        for key in ("price", "sl", "tp"):
            value = trade.get(key)
            if isinstance(value, (int, float)) and not isinstance(value, bool) and value > 0.0:
                trade[key] = round(round(float(value) / tick) * tick, digits)

    def send_order(self, request_id: str, trade_request: dict) -> dict:
        result = self.terminal.order_send(trade_request)
        if result is None:
            code, message = self.terminal.last_error()
            return {
                "type": "reply",
                "id": request_id,
                "retcode": None,
                "error": f"order_send returned nothing ({code}): {message}",
            }
        if int(result.order):
            # Followed until it works or history says how it ended: an order
            # dropped between two pushes would otherwise never be reported.
            self.sent_orders.add(int(result.order))
        return {
            "type": "reply",
            "id": request_id,
            "retcode": int(result.retcode),
            "comment": str(result.comment),
            "order": int(result.order),
            "deal": int(result.deal),
            "volume": _volume_text(float(result.volume)),
            "price": self.price(float(result.price)),
        }

    def build_submit(self, request: dict) -> dict:
        t = self.terminal
        symbol = need_text(request, "symbol")
        side = need_choice(request, "side", ("buy", "sell"))
        kind = need_choice(request, "kind", ("market", "limit", "stop"))
        trade = {
            "symbol": symbol,
            "volume": need_number(request, "volume"),
            "sl": optional_number(request, "sl"),
            "tp": optional_number(request, "tp"),
            "deviation": need_int(request, "deviation"),
            "magic": need_int(request, "magic"),
            "comment": str(request.get("comment", ""))[:MAX_COMMENT_CHARS],
            "type_filling": const(t, "ORDER_FILLING_RETURN", 2),
        }
        if kind == "market":
            tick = t.symbol_info_tick(symbol)
            if tick is None:
                raise RequestError(f"no price for {symbol}: the terminal has no tick to send a market order at")
            trade["action"] = const(t, "TRADE_ACTION_DEAL", 1)
            trade["type"] = const(t, "ORDER_TYPE_BUY", 0) if side == "buy" else const(t, "ORDER_TYPE_SELL", 1)
            trade["price"] = float(tick.ask if side == "buy" else tick.bid)
            return trade
        trade["action"] = const(t, "TRADE_ACTION_PENDING", 5)
        trade["price"] = need_number(request, "price")
        trade["type_time"] = const(t, "ORDER_TIME_DAY", 1)
        names = {
            ("buy", "limit"): ("ORDER_TYPE_BUY_LIMIT", 2),
            ("sell", "limit"): ("ORDER_TYPE_SELL_LIMIT", 3),
            ("buy", "stop"): ("ORDER_TYPE_BUY_STOP", 4),
            ("sell", "stop"): ("ORDER_TYPE_SELL_STOP", 5),
        }
        trade["type"] = const(t, *names[(side, kind)])
        return trade

    def build_cancel(self, request: dict) -> dict:
        return {
            "action": const(self.terminal, "TRADE_ACTION_REMOVE", 8),
            "order": need_int(request, "order"),
        }

    def build_modify(self, request: dict) -> dict:
        ticket = need_int(request, "order")
        trade = {
            "action": const(self.terminal, "TRADE_ACTION_MODIFY", 7),
            "order": ticket,
            "price": need_number(request, "price"),
            "sl": optional_number(request, "sl"),
            "tp": optional_number(request, "tp"),
            "type_time": const(self.terminal, "ORDER_TIME_DAY", 1),
        }
        # Keep the order's own time-in-force: a modify that silently turned a
        # good-till-cancelled order into a day order would be a change nobody
        # asked for.
        current = self.terminal.orders_get(ticket=ticket)
        if current:
            trade["type_time"] = int(current[0].type_time)
            trade["expiration"] = int(getattr(current[0], "time_expiration", 0))
        return trade

    def build_sltp(self, request: dict) -> dict:
        return {
            "action": const(self.terminal, "TRADE_ACTION_SLTP", 6),
            "symbol": need_text(request, "symbol"),
            "position": need_int(request, "position"),
            "sl": optional_number(request, "sl"),
            "tp": optional_number(request, "tp"),
            "magic": need_int(request, "magic"),
        }

    def build_close(self, request: dict) -> dict:
        t = self.terminal
        ticket = need_int(request, "position")
        found = t.positions_get(ticket=ticket)
        if not found:
            # On a netting account a "closing" deal for a position that is gone
            # opens a fresh one the other way. Refused here, never sent.
            raise RequestError(f"position {ticket} is not open: nothing to close")
        position = found[0]
        buy_type = const(t, "POSITION_TYPE_BUY", 0)
        volume = min(need_number(request, "volume"), float(position.volume))
        closing_buy = int(position.type) != buy_type
        tick = t.symbol_info_tick(position.symbol)
        if tick is None:
            raise RequestError(f"no price for {position.symbol}: the terminal has no tick to close at")
        return {
            "action": const(t, "TRADE_ACTION_DEAL", 1),
            "symbol": position.symbol,
            "volume": volume,
            "type": const(t, "ORDER_TYPE_BUY", 0) if closing_buy else const(t, "ORDER_TYPE_SELL", 1),
            "position": ticket,
            "price": float(tick.ask if closing_buy else tick.bid),
            "deviation": need_int(request, "deviation"),
            "magic": need_int(request, "magic"),
            "comment": str(request.get("comment", ""))[:MAX_COMMENT_CHARS],
            "type_filling": const(t, "ORDER_FILLING_RETURN", 2),
        }

    # -- state -------------------------------------------------------------

    def digits(self, symbol: str | None = None) -> int | None:
        """A symbol's price digits (the watched one's by default), asked of
        the terminal once; `None` while the terminal has no information."""
        symbol = self.symbol if symbol is None else symbol
        if symbol not in self.digits_by_symbol:
            info = self.terminal.symbol_info(symbol) if symbol else None
            if info is None:
                return None
            self.digits_by_symbol[symbol] = int(info.digits)
        return self.digits_by_symbol[symbol]

    def price(self, value: float) -> str:
        digits = self.digits()
        if digits is None:
            # Unknown digits: the value as the terminal holds it, never
            # rounded to a whole number.
            return f"{value:.10f}".rstrip("0").rstrip(".")
        return f"{value:.{digits}f}"

    def optional_price(self, value: float) -> str | None:
        """MetaTrader writes "no stop" as 0.0; on the wire it is `null`."""
        return None if not value else self.price(value)

    def build_state(self) -> dict:
        """Everything quantick reads about the watched symbol, in one line.

        One line rather than four so quantick applies it as one fact: deals
        before positions, never a position from one moment beside the deals of
        another.
        """
        state = {"type": "state", "symbol": self.symbol, "account": self.account()}
        if not self.symbol:
            state.update(positions=[], orders=[], deals=[], closed_orders=[])
            return state
        state["positions"] = [self.position_line(p) for p in self.terminal.positions_get(symbol=self.symbol) or ()]
        orders = list(self.terminal.orders_get(symbol=self.symbol) or ())
        state["orders"] = [self.order_line(o) for o in orders]
        state["deals"] = self.new_deals()
        state["closed_orders"] = self.closed_orders({int(o.ticket) for o in orders})
        return state

    def account(self) -> dict | None:
        info = self.terminal.account_info()
        if info is None:
            return None
        return {
            "login": int(info.login),
            "server": str(info.server),
            "company": str(info.company),
            "trade_mode": TRADE_MODES.get(int(info.trade_mode), "unknown"),
            "margin_mode": MARGIN_MODES.get(int(info.margin_mode), "unknown"),
            "balance": f"{float(info.balance):.2f}",
            "currency": str(info.currency),
        }

    def position_line(self, p) -> dict:
        return {
            "ticket": int(p.ticket),
            "side": "buy" if int(p.type) == const(self.terminal, "POSITION_TYPE_BUY", 0) else "sell",
            "volume": _volume_text(float(p.volume)),
            "price_open": self.price(float(p.price_open)),
            "sl": self.optional_price(float(p.sl)),
            "tp": self.optional_price(float(p.tp)),
            "magic": int(p.magic),
            "time_msc": int(p.time_msc),
        }

    def order_line(self, o) -> dict:
        side, kind = ORDER_TYPE_KINDS.get(int(o.type), ("none", "unknown"))
        return {
            "ticket": int(o.ticket),
            "side": side,
            "kind": kind,
            "volume": _volume_text(float(o.volume_current)),
            "price": self.price(float(o.price_open)),
            "sl": self.optional_price(float(o.sl)),
            "tp": self.optional_price(float(o.tp)),
            "magic": int(o.magic),
            "time_setup_msc": int(o.time_setup_msc),
        }

    def new_deals(self) -> list[dict]:
        now = int(time.time())
        start = now - DEAL_WINDOW_S
        sweep = self.deals_swept_at is None or time.monotonic() - self.deals_swept_at >= DEAL_SWEEP_S
        if sweep:
            self.deals_swept_at = time.monotonic()
        elif self.newest_deal_s is not None:
            start = max(start, self.newest_deal_s - DEAL_OVERLAP_S)
        deals = self.terminal.history_deals_get(start, now + DEAL_WINDOW_S) or ()
        lines = []
        for d in deals:
            if d.symbol != self.symbol:
                # Another symbol's deal says nothing about where this one's
                # history is complete.
                continue
            dealt_s = int(d.time_msc) // 1000
            if self.newest_deal_s is None or dealt_s > self.newest_deal_s:
                self.newest_deal_s = dealt_s
            ticket = int(d.ticket)
            if ticket in self.sent_deals or int(d.type) not in (0, 1):
                continue
            self.sent_deals.add(ticket)
            lines.append(
                {
                    "ticket": ticket,
                    "order": int(d.order),
                    "position_id": int(d.position_id),
                    "side": "buy" if int(d.type) == 0 else "sell",
                    "entry": DEAL_ENTRIES.get(int(d.entry), "unknown"),
                    "reason": DEAL_REASONS.get(int(d.reason), "unknown"),
                    "volume": _volume_text(float(d.volume)),
                    "price": self.price(float(d.price)),
                    "magic": int(d.magic),
                    "time_msc": int(d.time_msc),
                }
            )
        lines.sort(key=lambda line: (line["time_msc"], line["ticket"]))
        return lines

    def closed_orders(self, working: set[int]) -> list[dict]:
        """How each order that left the working list ended, once history says.

        A history record that is not yet an ending (`started`, `placed`) is
        waited on like a missing one: quantick reads only endings, so
        reporting it would stop the order being followed for good."""
        for ticket in (self.known_orders | self.sent_orders) - working:
            self.unresolved.setdefault(ticket, 0)
        self.sent_orders.clear()
        self.known_orders = working
        resolved = []
        for ticket in sorted(self.unresolved):
            if ticket in working:
                # Listed after all: the working list follows it from here.
                self.unresolved.pop(ticket)
                continue
            history = self.terminal.history_orders_get(ticket=ticket)
            state = ORDER_STATES.get(int(history[0].state), "unknown") if history else None
            if state not in FINAL_ORDER_STATES:
                if self.unresolved[ticket] < MAX_UNRESOLVED_PASSES:
                    self.unresolved[ticket] += 1
                    continue
                state = "unknown"
            resolved.append({"ticket": ticket, "state": state})
        for line in resolved:
            self.unresolved.pop(line["ticket"], None)
        return resolved

    def push_state(self) -> None:
        state = self.build_state()
        body = json.dumps(state, separators=(",", ":"), sort_keys=True)
        quiet = state.get("deals") == [] and state.get("closed_orders") == []
        due = time.monotonic() - self.last_state_at >= STATE_HEARTBEAT_S
        if quiet and body == self.last_state_body and not due:
            return
        self.last_state_body = body
        self.last_state_at = time.monotonic()
        self.send(state)


# -- request fields -----------------------------------------------------------


def now_ms() -> int:
    """Unix milliseconds: the clock quantick stamps `expires_ms` with, on the
    same machine."""
    return int(time.time() * 1000)


def refuse_if_expired(request: dict) -> None:
    """Refuse a request reached at or after its `expires_ms`: quantick may
    already have declared the link lost and reported the request's outcome
    unknown, and executing it now could fill after the trader stood down.
    Checked last, just before `order_send`. Absent, there is no deadline."""
    expires_ms = request.get("expires_ms")
    if expires_ms is None:
        return
    if isinstance(expires_ms, bool) or not isinstance(expires_ms, int):
        raise RequestError("expires_ms must be whole milliseconds")
    late_ms = now_ms() - expires_ms
    if late_ms >= 0:
        raise RequestError(f"expired {late_ms} ms ago: quantick stopped waiting for it - not executed")


def need_text(request: dict, key: str) -> str:
    value = request.get(key)
    if not isinstance(value, str) or not value:
        raise RequestError(f"{key} is missing")
    return value


def need_choice(request: dict, key: str, choices: tuple[str, ...]) -> str:
    value = request.get(key)
    if value not in choices:
        raise RequestError(f"{key} must be one of {', '.join(choices)}")
    return value


def need_number(request: dict, key: str) -> float:
    """A decimal sent as a string, as a positive float for the terminal."""
    value = request.get(key)
    try:
        number = float(value)
    except (TypeError, ValueError):
        raise RequestError(f"{key} is not a number") from None
    if not number > 0:
        raise RequestError(f"{key} must be greater than zero")
    return number


def optional_number(request: dict, key: str) -> float:
    """An absent or null stop is MetaTrader's 0.0, "none"."""
    if request.get(key) is None:
        return 0.0
    return need_number(request, key)


def need_int(request: dict, key: str) -> int:
    value = request.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise RequestError(f"{key} must be a whole number")
    return value


# -- loop ---------------------------------------------------------------------


def run_session(args: argparse.Namespace, cache: ReplyCache, instance: str) -> None:
    with socket.create_connection((args.host, args.port), timeout=10) as sock:
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        session = ExecSession(sock, mt5, cache, instance, args.symbol or "")
        session.hello()
        log("EXEC_SESSION_STARTED", host=args.host, port=args.port, instance=instance)
        interval = args.poll_ms / 1000.0
        try:
            while True:
                session.pump()
                time.sleep(interval)
        except KeyboardInterrupt:
            session.close("interrupted")
            raise
        except OSError as error:
            session.close("socket error")
            raise ConnectionError(str(error)) from error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--host", default="127.0.0.1", help="quantick execution listener host")
    parser.add_argument("--port", type=int, default=DEFAULT_EXEC_PORT, help="quantick execution listener port")
    parser.add_argument("--symbol", default=None, help="symbol to watch until quantick names one")
    parser.add_argument("--poll-ms", type=float, default=50.0, help="how often requests and state are pumped")
    parser.add_argument("--retry-seconds", type=float, default=2.0)
    args = parser.parse_args()
    if mt5 is None:
        log("BRIDGE_NO_MT5_PACKAGE", hint="pip install MetaTrader5")
        return 2
    if not mt5.initialize():
        log("BRIDGE_TERMINAL_ATTACH_FAILED", mt5_error=str(mt5.last_error()), hint="is the terminal running and logged in?")
        return 2
    # One instance per process: quantick compares it across reconnects to
    # know whether the reply memory survived.
    instance = uuid.uuid4().hex
    cache = ReplyCache()
    try:
        while True:
            try:
                run_session(args, cache, instance)
            except BridgeExit:
                return 2
            except (ConnectionError, OSError, socket.timeout) as error:
                log(
                    "EXEC_DISCONNECTED",
                    reason=str(error) or type(error).__name__,
                    retry_in_s=args.retry_seconds,
                    hint="is quantick running with [broker.metatrader] enabled?",
                )
                time.sleep(args.retry_seconds)
    except KeyboardInterrupt:
        log("BRIDGE_STOPPED", reason="interrupted")
        return 0
    finally:
        mt5.shutdown()


if __name__ == "__main__":
    raise SystemExit(main())
