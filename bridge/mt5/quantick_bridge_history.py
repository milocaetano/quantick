"""Reaching backwards: the opening block, the load-older walk and the search
for where a contract's history actually begins.

The slowest and most defensive part of the bridge. The terminal answers a tick
range with whatever it happens to hold, so every walk here is bounded twice —
by a window count and by a tick count — and says in the log which bound it hit.
"""

from __future__ import annotations

import time

from quantick_bridge_core import (
    LOAD_OLDER_FIRST_WINDOW_S,
    LOAD_OLDER_MAX_PAGES,
    LOAD_OLDER_MAX_TICKS,
    LOAD_OLDER_MAX_WINDOW_S,
    LOAD_OLDER_WINDOW_GROWTH,
    MAX_SLICE_TICKS_THE_FEED_ACCEPTS,
    OPENING_REACH_MAX_CALLS,
    OPENING_REACH_WINDOW_S,
    SESSION_GAP_MS,
    SESSION_WALK_MAX_SPAN_MS,
    SESSION_WALK_MAX_WINDOWS,
    log,
    mt5,
)


class HistoryMixin:
    """Everything a session does with ticks older than the one just printed."""

    def backfill(self) -> None:
        """The session the tape is in, whole, whatever the clock reads.

        This used to be a rolling clock window — `now - backfill_minutes`, 720
        of them by default — and both its own docstring and the README called
        that "a whole B3 session". It only was while the clock happened to read
        between roughly 18:25 and 21:00. Measured against a live terminal on
        2026-08-31 at 22:10 local, WINV26 had printed 09:03:00 to 18:31:23 and
        the 720-minute window returned its oldest tick at 13:10: four hours of
        the trader's day missing, with nothing on screen to say so. Opening
        half an hour earlier cut it at 09:30, which is the report this was
        rebuilt for.

        The anchor is the tape instead. `last_print_before` finds the newest
        print the terminal holds — the same step that already handled a market
        shut for the weekend — and `session_ticks` walks back from it until the
        prints stop. What arrives is the session that print belongs to, from
        its own first trade, and it is the same block whether the chart is
        opened at 11:00, at 22:10 or on a Sunday.

        `--backfill-minutes` no longer decides where the day starts, because a
        span in hours cannot: the same six hours land mid-session on one
        instrument and inside a weekend on another. It survives as the width of
        the *first* window only, which is what makes the common case one
        terminal call rather than a walk.
        """
        now_s = int(time.time() + self.offset_s)
        newest_ms, searched = self.last_print_before(now_s)
        if newest_ms is None:
            log(
                "BRIDGE_BACKFILL_NO_HISTORY",
                symbol=self.symbol,
                searched_calls=searched,
                note="the terminal holds no ticks for this symbol; sending an empty block",
            )
            # `send_backfill` parks the cursor for an empty block; doing it
            # again here would be a second owner of the same invariant.
            self.send_backfill([])
            return

        ticks, windows, stopped_on = self.session_ticks(newest_ms)
        available = len(ticks)
        # Clamped like the slice knob beside it: `ticks[-0:]` is the whole
        # block, so an unclamped zero would send everything while telling the
        # trader through `bridge_log` that their session was cut.
        cap = max(1, self.args.backfill_max_ticks)
        if available > cap:
            # The cap is a bound on *memory*, never on the span: the walk above
            # already stopped at the session's own edge. When it does bite, the
            # newest win — that is what the trader is looking at — and it is
            # said out loud. A silent amputation is the same defect as the
            # clock window one layer down: the chart would open on a partial
            # day looking exactly like a complete one.
            ticks = ticks[-cap:]
            log(
                "BRIDGE_BACKFILL_TRUNCATED",
                symbol=self.symbol,
                found=available,
                sending=len(ticks),
                action="keep_newest",
                stopped_on=stopped_on,
                recoverable="load_older",
            )
        log(
            "BRIDGE_BACKFILL_SESSION",
            symbol=self.symbol,
            count=len(ticks),
            first_ms=int(ticks[0]["time_msc"]) if len(ticks) else None,
            last_ms=int(ticks[-1]["time_msc"]) if len(ticks) else None,
            behind_s=now_s - newest_ms // 1000,
            windows=windows,
            searched_calls=searched,
            stopped_on=stopped_on,
        )
        # The newest slice opens the chart; the rest is parked for the loop.
        # `pending_opening` holds the slices newest-first, so `pop(0)` takes the
        # next one to go out and each is older than the one before it. Not
        # `pop()`: that would take the oldest first, and every later slice
        # would then fail the consumer's "older than what the chart holds"
        # filter and be dropped as overlap.
        # Clamped, not trusted: a slice larger than the feed's own per-block
        # cap arrives trimmed, and the surplus is dropped with nothing on the
        # chart to say so.
        slice_ticks = max(1, min(self.args.opening_slice_ticks, MAX_SLICE_TICKS_THE_FEED_ACCEPTS))
        opening = ticks[-slice_ticks:] if len(ticks) else ticks
        rest = ticks[:-slice_ticks] if len(ticks) > slice_ticks else []
        self.pending_opening = [
            rest[max(0, start - slice_ticks) : start]
            for start in range(len(rest), 0, -slice_ticks)
        ]
        if self.pending_opening:
            log(
                "BRIDGE_OPENING_SLICED",
                symbol=self.symbol,
                total=len(ticks),
                opening_now=len(opening),
                slices_to_follow=len(self.pending_opening),
                slice_ticks=slice_ticks,
            )
        self.send_backfill(opening)

    def pump_opening(self) -> None:
        """Send one parked slice of the opening session, if any is left.

        One per pass, deliberately. The loop this returns to is the one pumping
        live ticks and answering clicks, and a slice costs a third of a second;
        draining the queue here would hand the trader a chart that fills in
        quickly and a tape that stopped while it did.
        """
        if not self.pending_opening:
            return
        slice_ticks = self.pending_opening.pop(0)
        remaining = len(self.pending_opening)
        self.send(
            {
                "type": "history_start",
                "count_hint": len(slice_ticks),
                "opening": True,
            }
        )
        sent_ms = self.server_now_ms()
        for tick in slice_ticks:
            self.send_tick(tick, sent_ms)
        self.send({"type": "history_end", "opening": True, "remaining": remaining})
        self.flush()
        if not remaining:
            log("BRIDGE_OPENING_COMPLETE", symbol=self.symbol)

    def send_backfill(self, ticks: list) -> None:
        """Put one block on the wire and leave the live cursor after it.

        An empty block still gets both markers: `backfill_end` is the "history
        is done" signal quantick's loader waits on, so swallowing it would be a
        spinner that never stops.
        """
        self.send({"type": "backfill_start", "count_hint": len(ticks)})
        # History is stamped like anything else: `sent_ms` says when the bridge
        # handed the line over, which for a backfilled tick is hours after the
        # print happened. The feed measures latency from live prints only, so
        # the stamp here is a true statement nobody draws a lag from.
        backfill_sent_ms = self.server_now_ms()
        for tick in ticks:
            self.send_tick(tick, backfill_sent_ms)
        self.send({"type": "backfill_end"})
        self.flush()

        if len(ticks):
            self.cursor_msc = int(ticks[-1]["time_msc"])
            self.sent_at_cursor = sum(
                1 for t in ticks if int(t["time_msc"]) == self.cursor_msc
            )
        elif not self.cursor_msc:
            # An empty block still has to park the cursor, and this is the only
            # place that can: leaving it at zero makes the live pump's first
            # `copy_ticks_from(symbol, 0, ...)` ask for the *oldest* ticks the
            # terminal holds and forward years-old prints as live trades, four
            # thousand at a time, for the life of the session.
            #
            # Reachable whenever the walk comes back empty after the search
            # found a print — a terminal that fails mid-walk
            # (`stopped_on="terminal_error"`) is exactly that, and this branch
            # added the report for it. `now` is the honest anchor: nothing was
            # sent, so the live tape starts from here.
            self.cursor_msc = int(time.time() + self.offset_s) * 1000
            self.sent_at_cursor = 0
    @staticmethod
    def older_than(found, cursor_ms: int):
        """The part of a terminal answer strictly older than `cursor_ms`.

        `copy_ticks_range` answers with a numpy structured array, where this is
        one vectorised pass rather than a million Python objects. Boolean-mask
        indexing *copies*, so the window is briefly resident twice -- which is
        why the tick cap bounds roughly two to three times its own number in
        peak memory rather than exactly it.

        The fake terminal the tests run against answers with a list, which only
        a comprehension can filter.

        The difference is not academic: measured against the live terminal over
        one WINV26 session (1 525 621 prints) the comprehension cost ~840 ms and
        the mask ~10 ms, and every millisecond of it lands on the frame where
        the trader is waiting for their chart.
        """
        if hasattr(found, "dtype"):
            return found[found["time_msc"] < cursor_ms]
        return [tick for tick in found if int(tick["time_msc"]) < cursor_ms]

    @staticmethod
    def any_older_than(found, cursor_ms: int) -> bool:
        """Whether `found` holds any tick strictly older than `cursor_ms`.

        The yes/no half of `older_than`. Kept apart because the caller that
        wants a bit should not pay for an array: on the startup path this is
        asked of a two-day window, where building the filtered copy is megabytes
        thrown away one line later.
        """
        if found is None or not len(found):
            return False
        if hasattr(found, "dtype"):
            return bool((found["time_msc"] < cursor_ms).any())
        return any(int(tick["time_msc"]) < cursor_ms for tick in found)

    @staticmethod
    def session_edge_in(found, gap_ms: int) -> int:
        """Index of the first print after the newest gap of `gap_ms` in `found`,
        or 0 when the block holds no such gap.

        The walk stops when a whole window comes back empty, and that alone is
        not enough: the *first* window is `--backfill-minutes` wide -- twelve
        hours by default -- so a close shorter than that never produces an
        empty window and the previous session's tail is swept into today's.
        An instrument with a maintenance break, or any symbol whose last
        session ended less than twelve hours ago, would open on a chart whose
        left edge the app's own `history_reach` does not recognise as a session
        edge: exactly the divergence `session_gap_agreement.rs` exists to stop.

        So each block is cut here too, and the two rules are the same rule.
        """
        if len(found) < 2:
            return 0
        if hasattr(found, "dtype"):
            import numpy  # noqa: PLC0415  (only on the terminal's own arrays)

            stamps = found["time_msc"].astype("int64")
            breaks = numpy.nonzero(numpy.diff(stamps) >= gap_ms)[0]
            return int(breaks[-1]) + 1 if len(breaks) else 0
        edge = 0
        for index in range(1, len(found)):
            if int(found[index]["time_msc"]) - int(found[index - 1]["time_msc"]) >= gap_ms:
                edge = index
        return edge

    @staticmethod
    def join_windows(windows: list):
        """One block, oldest window first, without copying when there is nothing
        to join.

        The common case by far is a single window: a market that is trading
        answers the whole session in the first ask, and the walk's second ask
        only proves where it began. Returning that array untouched is what keeps
        the opening block free of a second pass over a million and a half prints.
        """
        if not windows:
            return []
        if len(windows) == 1:
            return windows[0]
        if hasattr(windows[0], "dtype"):
            import numpy  # noqa: PLC0415  (only on the multi-window path)

            return numpy.concatenate(list(reversed(windows)))
        return [tick for window in reversed(windows) for tick in window]

    def session_ticks(self, newest_ms: int) -> tuple[list, int, str]:
        """Every print of the session `newest_ms` belongs to, ascending.

        Steps back from that print in [`SESSION_GAP_MS`]-wide windows and stops
        at the first one holding nothing. That emptiness is the whole rule: the
        walk has already fetched every print newer than the window, so a window
        of its width with nothing in it means the oldest print in hand has no
        neighbour within a gap — which is the market having been closed rather
        than a quiet patch.

        Where the session's open comes from is therefore the tape, never a
        calendar. quantick has no venue session table and inventing one here
        would be a second source of truth about every exchange's hours, wrong
        the first time a holiday moved — and wrong immediately for the CFDs
        this same bridge serves. The app already reads a session boundary this
        way in `quantick_feed::history_reach`; this is that rule applied one hop
        earlier, so the two ends of one tape cannot disagree about where a day
        began.

        Returns the ticks, how many windows it spent, and what stopped it.
        """
        flags = self.tick_flags()
        floor_ms = self.earliest_tick_ms(newest_ms)
        cap = max(1, self.args.backfill_max_ticks)
        # The first window is the trader's own `--backfill-minutes`, which on a
        # market that is trading covers the whole session in a single call. The
        # walk then confirms the edge with one more.
        width_s = max(self.args.backfill_minutes * 60, SESSION_GAP_MS // 1000)
        windows: list[list] = []
        held = 0
        # Exclusive upper bound, so the newest print itself falls inside the
        # first window rather than one millisecond above it.
        cursor_ms = newest_ms + 1
        # The walk may not reach past this however it steps. The bound is on
        # the *span* rather than on the number of windows because that is what
        # the constant means: widening the first window must not buy the walk
        # another twelve hours of a continuous tape.
        span_floor_ms = newest_ms - SESSION_WALK_MAX_SPAN_MS
        stopped_on = "budget"
        spent = 0
        for _ in range(SESSION_WALK_MAX_WINDOWS + 1):
            if cursor_ms <= 0:
                stopped_on = "epoch"
                break
            if cursor_ms <= span_floor_ms:
                stopped_on = "span"
                break
            if floor_ms is not None and cursor_ms <= floor_ms:
                stopped_on = "terminal_floor"
                break
            # Whole seconds is all `copy_ticks_range` takes, so the range is
            # rounded outward and the surplus filtered below. Rounding the
            # other way would drop every tick sharing the cursor's second.
            to_s = -(-cursor_ms // 1000)
            from_s = max(0, cursor_ms // 1000 - width_s, span_floor_ms // 1000)
            found = mt5.copy_ticks_range(self.symbol, from_s, to_s, flags)
            spent += 1
            if found is None:
                log(
                    "BRIDGE_BACKFILL_WALK_FAILED",
                    symbol=self.symbol,
                    from_s=from_s,
                    to_s=to_s,
                    mt5_error=str(mt5.last_error()),
                    action="answer_with_what_is_in_hand",
                )
                stopped_on = "terminal_error"
                break
            fresh = self.older_than(found, cursor_ms)
            if not len(fresh):
                # A whole window with no prints in it: the session started
                # after this point, and what is in hand is all of it.
                stopped_on = "session_edge"
                break
            edge = self.session_edge_in(fresh, SESSION_GAP_MS)
            if edge:
                # The session began inside this window. Keep the part above the
                # break and stop -- everything below it is the day before.
                windows.append(fresh[edge:])
                stopped_on = "session_edge"
                break
            windows.append(fresh)
            held += len(fresh)
            cursor_ms = from_s * 1000
            # Every window after the first is a gap wide. The first is only
            # wider so that a trading market answers in one call.
            width_s = SESSION_GAP_MS // 1000
            if held >= cap:
                # The memory bound, reached before the session's edge was
                # found. Stopping here is what the bound is *for*; the caller
                # trims to the cap and says what it did.
                stopped_on = "cap"
                break
        return self.join_windows(windows), spent, stopped_on

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
        # Deliberately not consulted here. This is the function a
        # misreported floor burns: believing a claimed oldest tick of
        # "19:30 today" made it give up one step in and bracket an empty
        # block on a day with 1.5 M prints in it. It has no reference print
        # to judge such a claim against -- it is looking for that print --
        # so instead of judging one it does not ask. `OPENING_REACH_MAX_CALLS`
        # is the bound that keeps a symbol with no history from searching
        # forever, and an empty range is cheap.
        floor_ms = None
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
        # Out now, not at the bottom of the loop: the docstring above promises
        # this marker precedes the walk, and the walk can take seconds. Left in
        # the buffer it would wait for a heartbeat or for the walk to finish,
        # which is the silence the promise exists to prevent.
        self.flush()
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

    def earliest_tick_ms(self, newest_ms: int | None = None) -> int | None:
        """The oldest tick the terminal holds for this symbol, or None.

        Asked once per session and cached, the failure included: a terminal
        that cannot answer will not start answering mid-session, and the walk
        asks on every request.

        **The answer is checked before it is believed.** `copy_ticks_from(sym,
        0, 1, COPY_TICKS_ALL)` is documented as the oldest tick and does not
        always return it: on 2026-08-31 it answered 19:30 *that evening* for a
        WINV26 whose history the same terminal held back to 2024-12-23 and had
        served range queries about seconds earlier. The terminal appears to
        answer from whatever it has paged in rather than from the record.

        That answer is not a small error, because the floor is what stops every
        backwards walk. Believed, it made `last_print_before` give up one step
        in — the search stepped to 19:03, compared against a floor of 19:30 and
        concluded the symbol had no history — so the opening block went out
        empty on a day with 1.5 M prints in it. An empty chart that looks like
        a quiet market is the exact class of failure this branch exists to end.

        So the claim is falsified rather than trusted: ask for the window
        immediately below it, and if anything comes back, the claim was not a
        floor. That costs one terminal call per session and is decisive in the
        case that matters — below a bogus "19:30 today" sits the whole session,
        while below a real floor there is nothing at all.
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
        claimed = int(found[0]["time_msc"])
        if newest_ms is not None and newest_ms - claimed > SESSION_WALK_MAX_SPAN_MS:
            # Two days or more below the newest print this symbol has: whatever
            # else that is, it is not the failure this check exists for, which
            # is a terminal naming a tick from *inside* recent data as its
            # oldest. Believing it costs nothing, and the alternative is a
            # two-day range fetch on the startup path -- measured at 1.1 s on
            # WINV26, against 0 ms for this comparison.
            self.earliest_ms = claimed
            log("BRIDGE_TICK_FLOOR", symbol=self.symbol, earliest_ms=claimed, checked="unnecessary")
            return claimed
        to_s = claimed // 1000
        # Wide enough to see through dead time, which a four-hour look is not:
        # a terminal that named a *session's open* as its oldest tick would be
        # believed, because the hours directly below it are the night before
        # and legitimately empty. Two days clears any overnight gap and most
        # weekends, and it is one call on a range the terminal answers from
        # disk.
        from_s = max(0, to_s - SESSION_WALK_MAX_SPAN_MS // 1000)
        below = mt5.copy_ticks_range(self.symbol, from_s, to_s, mt5.COPY_TICKS_ALL)
        # Only *whether any* exist is needed, so this asks that and nothing
        # more: `older_than` would build a filtered copy of a two-day window on
        # the startup path, and the answer is one bit.
        older = self.any_older_than(below, claimed)
        if older:
            log(
                "BRIDGE_TICK_FLOOR_IMPLAUSIBLE",
                symbol=self.symbol,
                claimed_ms=claimed,
                found_below=older,
                action="ignore_the_floor",
                note="the terminal named an oldest tick with ticks underneath it; "
                "walking back is not stopped by it",
            )
            return None
        self.earliest_ms = claimed
        log("BRIDGE_TICK_FLOOR", symbol=self.symbol, earliest_ms=self.earliest_ms)
        return self.earliest_ms
