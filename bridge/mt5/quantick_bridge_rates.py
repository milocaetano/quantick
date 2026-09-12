"""Historical candles: paging M1 bars out of the terminal and onto the wire.

The candle block is sent once per session, before the tape starts, so the cost
that matters here is the terminal's own paging behaviour rather than the
per-tick path.
"""

from __future__ import annotations

import time

from quantick_bridge_core import (
    DAYS_PER_MONTH,
    M1_INTERVAL_MS,
    MAX_BARS_PER_RATE_LINE,
    MT5_INVALID_PARAMS,
    RATES_MAX_PAGES,
    RATES_MIN_PAGE_BARS,
    RATES_PAGE_BARS,
    log,
    mt5,
)


class RatesMixin:
    """The session's candle block."""

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
