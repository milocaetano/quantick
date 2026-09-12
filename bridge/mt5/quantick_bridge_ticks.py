"""The live tape: what the instrument prints, and the book beside it.

Everything that runs once per pass while the market is open — which tick flags
this venue needs, the tick pump, the Depth of Market poll and the heartbeat
that proves a quiet tape is still alive.
"""

from __future__ import annotations

import json
import time

from quantick_bridge_core import (
    MAX_PUMP_ROUNDS,
    MAX_TICKS_PER_REQUEST,
    TAPE_PROBE_DAYS,
    TICKS_PER_PUMP_ROUND,
    _volume_text,
    log,
    mt5,
)


class TicksMixin:
    """The live half of a session: ticks, the book, and the heartbeat.

    Mixed into `Session`, which owns everything read here. State:
    `args`, `symbol`, `tape`, `seq`, `cursor_msc`, `sent_at_cursor`,
    `ticks_sent`, `offset_s`, `last_heartbeat`, `pump_round_limits`,
    `book_subscribed`, `book_sent`, `book_seq`, `book_skipped`,
    `last_book_body`, `last_book_ms`. Behaviour from siblings: `send`,
    `flush`, `price`, `server_now_ms` (`TransportMixin`).
    """

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
        # A heartbeat that waits in a buffer is not a heartbeat. Its whole job
        # is to prove the bridge is alive during work that does not return to
        # the loop for seconds at a time -- `walk_back` calls this from inside
        # its own search -- and the loop's flush is exactly the thing that is
        # not running then.
        self.flush()
        if self.book_subscribed:
            log(
                "BRIDGE_BOOK_STATS",
                symbol=self.symbol,
                images_sent=self.book_sent,
                images_skipped=self.book_skipped,
            )
