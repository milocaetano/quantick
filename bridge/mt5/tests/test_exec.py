"""The execution channel: each request's shape, its reply, the idempotent
retry and the pushed state.

Runs against a fake broker terminal rather than MetaTrader: CI has none, and a
test that needed a real account to prove a request shape would be a test that
places real orders. The fake records every `order_send` it was handed, so a
test can say both what the terminal was asked and how often.

Run directly (`python bridge/mt5/tests/test_exec.py`) or through
`cargo test -p quantick-feed-mt5 --test bridge_paging`, which runs every suite
in this folder.
"""

from __future__ import annotations

import json
import socket
import sys
import time
import types
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import BRIDGE_DIR, check, run_tests  # noqa: E402  (after the path insert)

sys.path.insert(0, str(BRIDGE_DIR))

import quantick_bridge_exec as exe  # noqa: E402

SYMBOL = "WINZ26"
MAGIC = 7_710_001


def ns(**fields):
    return types.SimpleNamespace(**fields)


class FakeBroker:
    """The subset of the MetaTrader5 package the execution channel touches.

    Constants are the package's documented values, so a request shape asserted
    here is the shape the real terminal would receive.
    """

    TRADE_ACTION_DEAL = 1
    TRADE_ACTION_PENDING = 5
    TRADE_ACTION_SLTP = 6
    TRADE_ACTION_MODIFY = 7
    TRADE_ACTION_REMOVE = 8
    ORDER_TYPE_BUY = 0
    ORDER_TYPE_SELL = 1
    ORDER_TYPE_BUY_LIMIT = 2
    ORDER_TYPE_SELL_LIMIT = 3
    ORDER_TYPE_BUY_STOP = 4
    ORDER_TYPE_SELL_STOP = 5
    ORDER_FILLING_RETURN = 2
    ORDER_TIME_DAY = 1
    POSITION_TYPE_BUY = 0

    def __init__(self) -> None:
        self.sent: list[dict] = []
        self.result = ns(retcode=10009, comment="Request executed", order=501, deal=901, volume=1.0, price=125000.0)
        self.positions: list = []
        self.orders: list = []
        self.deals: list = []
        self.history_orders: dict[int, object] = {}
        self.tick = ns(bid=124995.0, ask=125000.0)
        self.account = ns(
            login=1234567,
            server="Broker-DEMO",
            company="Broker Ltd",
            trade_mode=0,
            margin_mode=1,
            balance=100000.0,
            currency="BRL",
        )
        self.selected: list[str] = []
        self.info = ns(digits=0)
        self.deal_ranges: list[tuple[int, int]] = []
        # Off by default: most suites stamp deals at times no real window
        # would hold. On, a read returns only what its range covers.
        self.honour_ranges = False

    def order_send(self, request):
        self.sent.append(dict(request))
        return self.result

    def last_error(self):
        return (-1, "Terminal: generic fail")

    def symbol_info_tick(self, _symbol):
        return self.tick

    def symbol_info(self, _symbol):
        return self.info

    def symbol_select(self, symbol, _enable):
        self.selected.append(symbol)
        return True

    def account_info(self):
        return self.account

    def positions_get(self, symbol=None, ticket=None):
        return [p for p in self.positions if (ticket is None or p.ticket == ticket) and (symbol is None or p.symbol == symbol)]

    def orders_get(self, symbol=None, ticket=None):
        return [o for o in self.orders if (ticket is None or o.ticket == ticket) and (symbol is None or o.symbol == symbol)]

    def history_deals_get(self, date_from, date_to):
        self.deal_ranges.append((int(date_from), int(date_to)))
        if not self.honour_ranges:
            return list(self.deals)
        return [d for d in self.deals if int(date_from) <= int(d.time_msc) // 1000 <= int(date_to)]

    def history_orders_get(self, ticket=None):
        found = self.history_orders.get(ticket)
        return [found] if found is not None else []


def session(broker: FakeBroker, cache: exe.ReplyCache | None = None) -> exe.ExecSession:
    """A session with no socket: what it sends is collected in `sent`."""
    s = exe.ExecSession(None, broker, cache or exe.ReplyCache(), "instance-a", SYMBOL)
    s.lines = []
    s.send = s.lines.append
    return s


def submit(kind: str, side: str = "buy", **extra) -> dict:
    request = {
        "type": "submit",
        "id": f"t-{kind}-{side}",
        "symbol": SYMBOL,
        "side": side,
        "kind": kind,
        "volume": "2",
        "deviation": 10,
        "magic": MAGIC,
        "comment": "q:t-1",
    }
    request.update(extra)
    return request


def position(ticket=77, side=0, volume=2.0, magic=MAGIC, sl=0.0, tp=0.0):
    return ns(ticket=ticket, symbol=SYMBOL, type=side, volume=volume, price_open=125000.0, sl=sl, tp=tp, magic=magic, time_msc=1_000)


def test_a_market_submit_is_a_deal_at_the_touch_with_the_configured_magic():
    broker = FakeBroker()
    reply = session(broker).answer(submit("market", sl="124800", tp="125300"))
    sent = broker.sent[0]
    check("one order_send", len(broker.sent) == 1, broker.sent)
    check("a deal", sent["action"] == FakeBroker.TRADE_ACTION_DEAL, sent)
    check("a buy at the ask", sent["type"] == 0 and sent["price"] == 125000.0, sent)
    check("filling return", sent["type_filling"] == FakeBroker.ORDER_FILLING_RETURN, sent)
    check("magic and deviation from the request", sent["magic"] == MAGIC and sent["deviation"] == 10, sent)
    check("native stop and target", sent["sl"] == 124800.0 and sent["tp"] == 125300.0, sent)
    check("volume as a number", sent["volume"] == 2.0, sent)
    check("the reply carries the id", reply["id"] == "t-market-buy", reply)
    check("the retcode and comment verbatim", (reply["retcode"], reply["comment"]) == (10009, "Request executed"), reply)
    check("the tickets", (reply["order"], reply["deal"]) == (501, 901), reply)
    check("a first answer is not from memory", reply["cached"] is False, reply)


def test_a_market_sell_takes_the_bid_and_no_stop_is_zero():
    broker = FakeBroker()
    session(broker).answer(submit("market", side="sell"))
    sent = broker.sent[0]
    check("a sell at the bid", sent["type"] == 1 and sent["price"] == 124995.0, sent)
    check("no stop and no target are 0.0", sent["sl"] == 0.0 and sent["tp"] == 0.0, sent)


def test_limit_and_stop_submits_are_pending_day_orders_of_the_right_type():
    expected = {
        ("limit", "buy"): 2,
        ("limit", "sell"): 3,
        ("stop", "buy"): 4,
        ("stop", "sell"): 5,
    }
    for (kind, side), order_type in expected.items():
        broker = FakeBroker()
        broker.result = ns(retcode=10008, comment="Request placed", order=601, deal=0, volume=2.0, price=0.0)
        reply = session(broker).answer(submit(kind, side=side, price="124900"))
        sent = broker.sent[0]
        check(f"{side} {kind} is pending", sent["action"] == FakeBroker.TRADE_ACTION_PENDING, sent)
        check(f"{side} {kind} type", sent["type"] == order_type, sent)
        check(f"{side} {kind} rests at its price", sent["price"] == 124900.0, sent)
        check(f"{side} {kind} is a day order", sent["type_time"] == FakeBroker.ORDER_TIME_DAY, sent)
        check(f"{side} {kind} reply is placed", (reply["retcode"], reply["order"]) == (10008, 601), reply)


def test_cancel_modify_and_sltp_shapes():
    broker = FakeBroker()
    broker.orders = [ns(ticket=601, symbol=SYMBOL, type=2, type_time=0, time_expiration=0)]
    s = session(broker)
    s.answer({"type": "cancel", "id": "c-1", "order": 601})
    s.answer({"type": "modify", "id": "m-1", "order": 601, "price": "124850", "sl": "124700", "tp": None})
    s.answer({"type": "sltp", "id": "s-1", "symbol": SYMBOL, "position": 77, "sl": "124800", "tp": "125400", "magic": MAGIC})
    cancel, modify, sltp = broker.sent
    check("cancel is a remove of the ticket", cancel == {"action": 8, "order": 601}, cancel)
    check("modify is a modify", modify["action"] == 7 and modify["order"] == 601, modify)
    check("modify carries price and both protections", (modify["price"], modify["sl"], modify["tp"]) == (124850.0, 124700.0, 0.0), modify)
    check("modify keeps the order's own time in force", modify["type_time"] == 0, modify)
    check("sltp names the position", (sltp["action"], sltp["position"], sltp["symbol"]) == (6, 77, SYMBOL), sltp)
    check("sltp carries both prices", (sltp["sl"], sltp["tp"]) == (124800.0, 125400.0), sltp)


def test_a_partial_close_is_an_opposite_deal_on_the_position_and_never_more_than_it_holds():
    broker = FakeBroker()
    broker.positions = [position(side=0, volume=3.0)]
    s = session(broker)
    s.answer({"type": "close", "id": "x-1", "position": 77, "volume": "1", "deviation": 10, "magic": MAGIC})
    s.answer({"type": "close", "id": "x-2", "position": 77, "volume": "9", "deviation": 10, "magic": MAGIC})
    partial, clamped = broker.sent
    check("closing a long sells at the bid", (partial["type"], partial["price"]) == (1, 124995.0), partial)
    check("names the position it closes", partial["position"] == 77 and partial["action"] == 1, partial)
    check("the partial volume", partial["volume"] == 1.0, partial)
    check("a close never asks for more than is open", clamped["volume"] == 3.0, clamped)


def test_a_close_for_a_position_that_is_gone_is_refused_and_never_sent():
    broker = FakeBroker()
    reply = session(broker).answer({"type": "close", "id": "x-3", "position": 77, "volume": "1", "deviation": 10, "magic": MAGIC})
    check("nothing reached the terminal", broker.sent == [], broker.sent)
    check("the reply says why, with no retcode", reply["retcode"] is None and "not open" in reply["error"], reply)


def test_a_refusal_comes_back_verbatim():
    broker = FakeBroker()
    broker.result = ns(retcode=10016, comment="Invalid stops", order=0, deal=0, volume=0.0, price=0.0)
    reply = session(broker).answer(submit("market", sl="125100"))
    check("the retcode", reply["retcode"] == 10016, reply)
    check("the broker's own words", reply["comment"] == "Invalid stops", reply)


def test_order_send_returning_nothing_reports_last_error():
    broker = FakeBroker()
    broker.result = None
    reply = session(broker).answer(submit("market"))
    check("no retcode", reply["retcode"] is None, reply)
    check("the terminal's own error", "Terminal: generic fail" in reply["error"], reply)


def test_a_repeated_request_id_is_answered_from_memory_and_never_resent():
    broker = FakeBroker()
    cache = exe.ReplyCache()
    first = session(broker, cache).answer(submit("market"))
    # A new connection — the link dropped before quantick read the reply —
    # shares the process's memory, as `main` arranges.
    again = session(broker, cache).answer(submit("market"))
    check("order_send ran once", len(broker.sent) == 1, broker.sent)
    check("the same answer", {k: again[k] for k in ("id", "retcode", "order", "deal")} == {k: first[k] for k in ("id", "retcode", "order", "deal")}, again)
    check("marked as remembered", again["cached"] is True, again)


def test_a_replay_only_request_the_bridge_never_answered_is_not_executed():
    broker = FakeBroker()
    cache = exe.ReplyCache()
    replay = dict(submit("market"), replay_only=True)
    reply = session(broker, cache).answer(replay)
    check("the terminal was not asked", broker.sent == [], broker.sent)
    check("the answer says unknown", reply["unknown"] is True and reply["retcode"] is None, reply)
    check("with the reason", "not executed" in reply["error"], reply)
    # The same id, sent again as a normal request, stays unexecuted.
    again = session(broker, cache).answer(submit("market"))
    check("still never executed", broker.sent == [] and again["unknown"] is True, again)


def test_a_replay_only_request_gets_the_remembered_answer():
    broker = FakeBroker()
    cache = exe.ReplyCache()
    first = session(broker, cache).answer(submit("market"))
    again = session(broker, cache).answer(dict(submit("market"), replay_only=True))
    check("order_send ran once", len(broker.sent) == 1, broker.sent)
    check("the remembered answer", (again["retcode"], again["order"]) == (first["retcode"], first["order"]), again)
    check("not unknown", "unknown" not in again, again)


def test_a_refusal_is_remembered_too():
    broker = FakeBroker()
    broker.result = ns(retcode=10019, comment="No money", order=0, deal=0, volume=0.0, price=0.0)
    s = session(broker)
    s.answer(submit("market"))
    broker.result = ns(retcode=10009, comment="Request executed", order=9, deal=9, volume=2.0, price=1.0)
    again = s.answer(submit("market"))
    check("not retried behind the trader's back", len(broker.sent) == 1, broker.sent)
    check("the refusal again", again["comment"] == "No money", again)


def test_the_reply_memory_is_bounded():
    cache = exe.ReplyCache(capacity=2)
    for n in range(3):
        cache.put(f"r-{n}", {"n": n})
    check("the oldest is forgotten", cache.get("r-0") is None and cache.get("r-2") == {"n": 2}, list(cache.replies))


def test_malformed_requests_are_answered_without_reaching_the_terminal():
    broker = FakeBroker()
    s = session(broker)
    cases = [
        submit("market", volume="0"),
        submit("iceberg"),
        submit("limit"),
        {"type": "cancel", "id": "c-x", "order": "601"},
        {"type": "shout", "id": "z-1"},
    ]
    for request in cases:
        reply = s.answer(request)
        check(f"refused: {request.get('type')} {request.get('kind', '')}", reply["retcode"] is None and reply["error"], reply)
    no_id = s.answer({"type": "submit"})
    check("no id, no answer to cache", no_id["id"] is None, no_id)
    check("the terminal saw none of them", broker.sent == [], broker.sent)


def test_state_carries_the_account_and_what_rests_on_the_watched_symbol():
    broker = FakeBroker()
    broker.positions = [position(ticket=77, sl=124800.0), position(ticket=78, magic=0)]
    broker.orders = [
        ns(ticket=601, symbol=SYMBOL, type=3, volume_current=1.0, price_open=125300.0, sl=0.0, tp=0.0, magic=MAGIC, time_setup_msc=2_000, type_time=1),
        ns(ticket=602, symbol="WDOZ26", type=2, volume_current=1.0, price_open=5000.0, sl=0.0, tp=0.0, magic=MAGIC, time_setup_msc=2_000, type_time=1),
    ]
    state = session(broker).build_state()
    account = state["account"]
    check("login, server and company", (account["login"], account["server"], account["company"]) == (1234567, "Broker-DEMO", "Broker Ltd"), account)
    check("demo or real, read from the terminal", account["trade_mode"] == "demo", account)
    check("the margin mode", account["margin_mode"] == "exchange", account)
    check("balance and currency", (account["balance"], account["currency"]) == ("100000.00", "BRL"), account)
    check("both positions, magics included", [(p["ticket"], p["magic"]) for p in state["positions"]] == [(77, MAGIC), (78, 0)], state["positions"])
    check("a stop as a price, no target as null", (state["positions"][0]["sl"], state["positions"][0]["tp"]) == ("124800", None), state["positions"][0])
    check("only the watched symbol's orders", [o["ticket"] for o in state["orders"]] == [601], state["orders"])
    check("an order's side and kind", (state["orders"][0]["side"], state["orders"][0]["kind"]) == ("sell", "limit"), state["orders"][0])


def test_hedging_and_real_accounts_are_reported_as_they_are():
    broker = FakeBroker()
    broker.account.trade_mode = 2
    broker.account.margin_mode = 2
    account = session(broker).build_state()["account"]
    check("real", account["trade_mode"] == "real", account)
    check("hedging is reported, not refused, here", account["margin_mode"] == "hedging", account)


def test_deals_are_pushed_once_per_connection():
    broker = FakeBroker()
    broker.deals = [
        ns(ticket=901, order=501, position_id=77, symbol=SYMBOL, type=0, entry=0, reason=3, volume=2.0, price=125000.0, magic=MAGIC, time_msc=5_000),
        ns(ticket=902, order=700, position_id=99, symbol="WDOZ26", type=0, entry=0, reason=0, volume=1.0, price=5000.0, magic=0, time_msc=5_001),
        ns(ticket=903, order=0, position_id=0, symbol=SYMBOL, type=2, entry=0, reason=0, volume=0.0, price=0.0, magic=0, time_msc=5_002),
    ]
    s = session(broker)
    first = s.build_state()["deals"]
    check("the symbol's trade deal, nothing else", [d["ticket"] for d in first] == [901], first)
    check("entry, reason and magic", (first[0]["entry"], first[0]["reason"], first[0]["magic"]) == ("in", "expert", MAGIC), first[0])
    check("not pushed twice", s.build_state()["deals"] == [], "repeat")
    check("a new connection starts over", [d["ticket"] for d in session(broker).build_state()["deals"]] == [901], "reconnect")


def test_a_vanished_order_is_reported_with_how_it_ended():
    broker = FakeBroker()
    broker.orders = [ns(ticket=601, symbol=SYMBOL, type=2, volume_current=1.0, price_open=124900.0, sl=0.0, tp=0.0, magic=MAGIC, time_setup_msc=1, type_time=1)]
    s = session(broker)
    s.build_state()
    broker.orders = []
    waiting = s.build_state()["closed_orders"]
    check("no history yet, no verdict", waiting == [], waiting)
    broker.history_orders[601] = ns(state=2)
    ended = s.build_state()["closed_orders"]
    check("cancelled, once history has it", ended == [{"ticket": 601, "state": "canceled"}], ended)
    check("reported once", s.build_state()["closed_orders"] == [], "repeat")


def test_an_order_dropped_between_two_pushes_is_resolved_through_history():
    # A market order the exchange accepted and then rejected before any
    # state listed it as working: only its ticket, from the reply, names it.
    broker = FakeBroker()
    broker.result = ns(retcode=10008, comment="Request placed", order=501, deal=0, volume=1.0, price=0.0)
    s = session(broker)
    s.answer(submit("market"))
    check("no history yet, no verdict", s.build_state()["closed_orders"] == [], "waiting")
    broker.history_orders[501] = ns(state=5)
    ended = s.build_state()["closed_orders"]
    check("rejected, once history has it", ended == [{"ticket": 501, "state": "rejected"}], ended)
    check("reported once", s.build_state()["closed_orders"] == [], "repeat")


def test_a_sent_order_that_rests_is_not_resolved_while_it_works():
    broker = FakeBroker()
    broker.result = ns(retcode=10008, comment="Request placed", order=601, deal=0, volume=1.0, price=0.0)
    s = session(broker)
    s.answer(submit("limit", price="124900"))
    broker.orders = [ns(ticket=601, symbol=SYMBOL, type=2, volume_current=1.0, price_open=124900.0, sl=0.0, tp=0.0, magic=MAGIC, time_setup_msc=1, type_time=1)]
    for _ in range(exe.MAX_UNRESOLVED_PASSES + 5):
        closed = s.build_state()["closed_orders"]
        check("a working order is never declared ended", closed == [], closed)
        if closed:
            break


def test_deal_history_is_read_from_the_last_seen_deal_onward():
    broker = FakeBroker()
    dealt_s = int(time.time()) - 3_600
    broker.deals = [
        ns(ticket=901, order=501, position_id=77, symbol=SYMBOL, type=0, entry=0, reason=3, volume=1.0, price=125000.0, magic=MAGIC, time_msc=dealt_s * 1000),
    ]
    s = session(broker)
    s.build_state()
    s.build_state()
    first, second = broker.deal_ranges
    check("the first read covers the whole window", first[0] < dealt_s - exe.DEAL_OVERLAP_S, broker.deal_ranges)
    check(
        "later reads start just before the last deal seen",
        second[0] == dealt_s - exe.DEAL_OVERLAP_S,
        broker.deal_ranges,
    )


def test_a_request_is_refused_when_the_terminal_has_no_symbol_information():
    broker = FakeBroker()
    broker.info = None
    reply = session(broker).answer(submit("market"))
    check("nothing reached the terminal", broker.sent == [], broker.sent)
    check("refused with the reason", reply["retcode"] is None and "symbol information" in reply["error"], reply)


def test_watch_switches_the_symbol_and_selects_it():
    broker = FakeBroker()
    s = session(broker)
    reply = s.answer({"type": "watch", "id": "w-1", "symbol": "WDOZ26"})
    check("watching the new symbol", s.symbol == "WDOZ26" and reply["watching"] == "WDOZ26", reply)
    check("selected in Market Watch", broker.selected == ["WDOZ26"], broker.selected)
    check("no order was sent", broker.sent == [], broker.sent)


def test_a_socket_round_trip_answers_and_pushes_state():
    broker = FakeBroker()
    ours, theirs = socket.socketpair()
    try:
        s = exe.ExecSession(ours, broker, exe.ReplyCache(), "instance-z", SYMBOL)
        s.hello()
        theirs.sendall((json.dumps(submit("market")) + "\n").encode("utf-8"))
        s.pump()
        theirs.settimeout(2)
        received = b""
        while received.count(b"\n") < 3:
            received += theirs.recv(65536)
        lines = [json.loads(line) for line in received.decode("utf-8").splitlines()]
        check("hello first, with the instance", lines[0]["type"] == "exec_hello" and lines[0]["instance"] == "instance-z", lines[0])
        check("the schema version", lines[0]["schema"] == exe.EXEC_SCHEMA_VERSION, lines[0])
        check("then the reply", lines[1]["type"] == "reply" and lines[1]["retcode"] == 10009, lines[1])
        check("then the state", lines[2]["type"] == "state" and lines[2]["symbol"] == SYMBOL, lines[2])
    finally:
        ours.close()
        theirs.close()


def now_ms() -> int:
    return int(time.time() * 1000)


def test_a_request_reached_at_or_past_its_expiry_is_refused_and_never_executed():
    broker = FakeBroker()
    broker.positions = [position(volume=2.0)]
    s = session(broker)
    past = now_ms() - 1
    late = s.answer(submit("market", expires_ms=past))
    check("nothing reached the terminal", broker.sent == [], broker.sent)
    check("refused as expired, with no retcode", late["retcode"] is None and "expired" in str(late.get("error")), late)
    again = s.answer(submit("market", expires_ms=past))
    check("remembered, so it can never run later", broker.sent == [] and again["cached"] is True, again)
    close = s.answer({"type": "close", "id": "x-late", "position": 77, "volume": "1", "deviation": 10, "magic": MAGIC, "expires_ms": past})
    check("an expired close is refused too", broker.sent == [] and "expired" in str(close.get("error")), close)
    fresh = s.answer(dict(submit("market"), id="t-fresh", expires_ms=now_ms() + 60_000))
    check("one reached in time executes", len(broker.sent) == 1 and fresh["retcode"] == 10009, fresh)
    watch = s.answer({"type": "watch", "id": "w-late", "symbol": "WDOZ26", "expires_ms": past})
    check("a watch trades nothing and never expires", watch.get("watching") == "WDOZ26", watch)


def test_exits_reach_the_terminal_without_symbol_information():
    broker = FakeBroker()
    broker.info = None
    broker.positions = [position(volume=2.0)]
    broker.orders = [ns(ticket=601, symbol=SYMBOL, type=2, type_time=0, time_expiration=0)]
    s = session(broker)
    cancel = s.answer({"type": "cancel", "id": "c-9", "order": 601})
    close = s.answer({"type": "close", "id": "x-9", "position": 77, "volume": "1", "deviation": 10, "magic": MAGIC})
    sltp = s.answer({"type": "sltp", "id": "s-9", "symbol": SYMBOL, "position": 77, "sl": "124800", "tp": None, "magic": MAGIC})
    check("cancel, close and sltp reached the terminal", [r["action"] for r in broker.sent] == [8, 1, 6], broker.sent)
    check("each answered by the terminal", [r["retcode"] for r in (cancel, close, sltp)] == [10009] * 3, (cancel, close, sltp))
    modify = s.answer({"type": "modify", "id": "m-9", "order": 601, "price": "124850", "sl": None, "tp": None})
    check("a modify, which carries a price, is still refused", len(broker.sent) == 3 and "symbol information" in str(modify.get("error")), modify)


def test_stops_targets_and_prices_are_snapped_to_the_symbol_tick():
    broker = FakeBroker()
    broker.info = ns(digits=0, trade_tick_size=5.0)
    broker.orders = [ns(ticket=601, symbol=SYMBOL, type=2, type_time=0, time_expiration=0)]
    s = session(broker)
    s.answer({"type": "sltp", "id": "s-snap", "symbol": SYMBOL, "position": 77, "sl": "172140.82", "tp": "172207.6", "magic": MAGIC})
    sltp = broker.sent[-1]
    check("a dragged stop lands on the nearest tick", sltp["sl"] == 172140.0, sltp)
    check("a dragged target lands on the nearest tick", sltp["tp"] == 172210.0, sltp)
    s.answer(submit("limit", price="124902.4", sl="124801", tp="125298"))
    limit = broker.sent[-1]
    check("a limit price lands on the grid", limit["price"] == 124900.0, limit)
    check("its protections too", (limit["sl"], limit["tp"]) == (124800.0, 125300.0), limit)
    s.answer({"type": "modify", "id": "m-snap", "order": 601, "price": "124853", "sl": None, "tp": None})
    modify = broker.sent[-1]
    check("a modified price lands on the grid and no stop stays 0.0", (modify["price"], modify["sl"], modify["tp"]) == (124855.0, 0.0, 0.0), modify)


def test_a_protection_snaps_to_its_own_side_and_never_to_none():
    broker = FakeBroker()
    broker.info = ns(digits=0, trade_tick_size=5.0)
    broker.positions = [ns(ticket=77, symbol=SYMBOL, type=0), ns(ticket=78, symbol=SYMBOL, type=1)]
    s = session(broker)
    s.answer({"type": "sltp", "id": "s-long", "symbol": SYMBOL, "position": 77, "sl": "172143", "tp": "172207", "magic": MAGIC})
    long = broker.sent[-1]
    check("a long's stop goes down to its tick, its target up", (long["sl"], long["tp"]) == (172140.0, 172210.0), long)
    s.answer({"type": "sltp", "id": "s-short", "symbol": SYMBOL, "position": 78, "sl": "172143", "tp": "172207", "magic": MAGIC})
    short = broker.sent[-1]
    check("a short's stop goes up to its tick, its target down", (short["sl"], short["tp"]) == (172145.0, 172205.0), short)
    s.answer({"type": "sltp", "id": "s-tiny", "symbol": SYMBOL, "position": 78, "sl": "7", "tp": "2", "magic": MAGIC})
    tiny = broker.sent[-1]
    check("a protection under half a tick never becomes 0.0", (tiny["sl"], tiny["tp"]) == (10.0, 5.0), tiny)
    s.answer({"type": "sltp", "id": "s-grid", "symbol": SYMBOL, "position": 77, "sl": "172140", "tp": "172210", "magic": MAGIC})
    grid = broker.sent[-1]
    check("a level already on the grid stays put", (grid["sl"], grid["tp"]) == (172140.0, 172210.0), grid)


def test_prices_are_untouched_when_the_tick_size_is_unknown():
    broker = FakeBroker()
    broker.info = ns(digits=0)
    s = session(broker)
    s.answer({"type": "sltp", "id": "s-raw", "symbol": SYMBOL, "position": 77, "sl": "172140.82", "tp": "172207.6", "magic": MAGIC})
    sltp = broker.sent[-1]
    check("no tick size: the prices go as asked", (sltp["sl"], sltp["tp"]) == (172140.82, 172207.6), sltp)
    broker.info = None
    s.answer({"type": "sltp", "id": "s-none", "symbol": SYMBOL, "position": 77, "sl": "172140.82", "tp": None, "magic": MAGIC})
    sltp = broker.sent[-1]
    check("no symbol information: the stop still goes, as asked", (sltp["sl"], sltp["tp"]) == (172140.82, 0.0), sltp)


def test_an_order_history_still_calls_placed_is_followed_until_it_ends():
    broker = FakeBroker()
    broker.orders = [ns(ticket=601, symbol=SYMBOL, type=2, volume_current=1.0, price_open=124900.0, sl=0.0, tp=0.0, magic=MAGIC, time_setup_msc=1, type_time=1)]
    s = session(broker)
    s.build_state()
    broker.orders = []
    for state in (0, 1):
        broker.history_orders[601] = ns(state=state)
        pending = s.build_state()["closed_orders"]
        check(f"history state {state} is no ending", pending == [], pending)
    broker.history_orders[601] = ns(state=2)
    ended = s.build_state()["closed_orders"]
    check("its ending is reported once history has one", ended == [{"ticket": 601, "state": "canceled"}], ended)
    check("and only once", s.build_state()["closed_orders"] == [], "repeat")


def test_a_deal_recorded_late_with_an_older_time_is_sent_exactly_once():
    broker = FakeBroker()
    broker.honour_ranges = True
    now_s = int(time.time())

    def deal(ticket, symbol, at_s):
        return ns(ticket=ticket, order=900, position_id=77, symbol=symbol, type=1, entry=1, reason=3, volume=1.0, price=125100.0, magic=MAGIC, time_msc=at_s * 1000)

    broker.deals = [deal(901, SYMBOL, now_s - 3_600), deal(902, "WDOZ26", now_s - 60)]
    s = session(broker)
    tickets = lambda: [d["ticket"] for d in s.build_state()["deals"]]  # noqa: E731
    check("the watched symbol's deal", tickets() == [901], broker.deal_ranges)
    # Another symbol's newer deal never moves the watched symbol's window.
    broker.deals.append(deal(903, SYMBOL, now_s - 1_800))
    check("a late deal after the last one seen arrives", tickets() == [903], broker.deal_ranges)
    # Written so late it is older than the last deal seen: the next full
    # sweep of the window finds it.
    broker.deals.append(deal(904, SYMBOL, now_s - 7_200))
    s.deals_swept_at -= exe.DEAL_SWEEP_S
    check("the sweep sends it", tickets() == [904], broker.deal_ranges)
    s.deals_swept_at -= exe.DEAL_SWEEP_S
    check("and never twice", tickets() == [], broker.deal_ranges)


if __name__ == "__main__":
    sys.exit(run_tests(dict(globals())))
