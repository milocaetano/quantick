//! The venue's own candles, folded in as a prefix under the bars the tape
//! built.
//!
//! One base per market — minutes, or days for charts cut at a day or longer —
//! fetched once and folded locally to whatever interval each pane shows. The
//! base is the *market's* history, not a pane's: changing a pane's interval
//! refolds it, a move between the minute and the daily base parks it until it
//! is wanted again, and only a change of market (or a new answer from the
//! venue) throws it away.

use quantick_engine::Bar;
use quantick_sources::candle_base::{CandleAsk, CandleBaseInterval, SeamMinutesTaken};
use quantick_sources::candles::{
    OlderCandles, fold, is_foldable, merge_older_candles, starts_inside_bucket,
    trim_borrowed_to_seam, trim_to_seam,
};
use quantick_sources::config::FeedCapabilities;
use quantick_sources::ohlcv::{OHLCV_BASE_INTERVAL_MS, OhlcvSlice};

use crate::view::PaneView;
use crate::{
    CandlePurpose, Delivery, Effect, FeedRequest, Refusal, SeamTrace, SessionEvent, Step, Wait,
    WaitChange,
};

/// What a tab knows about the venue's candles for its market.
#[derive(Debug)]
pub(crate) struct VenueCandles {
    /// `None` until a reply lands; `Some(empty)` after one that carried
    /// nothing, which is what keeps a failed or unsupported fetch from being
    /// retried every frame.
    base: Option<Vec<Bar>>,
    /// The interval the candles in `base` were served at, the one the panes
    /// want, the base parked for the other wish, and the candle generations
    /// already acted on.
    ///
    /// A pull feed leaves the generations at zero forever — it answers
    /// whenever asked, so nothing changes behind us. A push feed moves one
    /// every time a block changes the answer to that base's request, including
    /// a replacement for one already delivered, and that is the only signal
    /// saying "the answer changed, ask again". A rising *capability* edge
    /// cannot say it: the flag rises once and stays, so a block arriving after
    /// an empty answer would sit unread.
    interval: CandleBaseInterval,
    /// Whether a fetch is out. One at a time — the *closing* reply is what
    /// clears it, and every provider always sends one. A progressive fetch
    /// stays pending across all of its slices: it is one request throughout.
    pub(crate) pending: bool,
    /// Whether the slices still arriving belong to a request whose answer is
    /// no longer wanted.
    ///
    /// A push feed can store a fresh block partway through a progressive run,
    /// and that discards the base being built. The slices already in flight
    /// know nothing about it, and folding them onto an empty base would build
    /// a history missing exactly the newest part. So they are dropped, and the
    /// closing one re-opens the door for a fresh request.
    stale: bool,
    /// The oldest candle held when the in-flight request went out, for a
    /// request that was reaching *back* past it — `None` for the opening one.
    ///
    /// Kept so the closing reply can answer the only question the trader has
    /// after clicking "older": did that get me anything? A provider serving
    /// from a block it already holds honestly re-sends the same candles, so
    /// the answer is "did the oldest bar move", a fact rather than an
    /// inference.
    reaching_back: Option<i64>,
    /// Whether the last *load older* came back with nothing older than what
    /// was already held: the venue's record starts here, or the provider's
    /// reach does. Latched rather than recomputed, because the only way to
    /// know is to have asked; cleared by a change of market and by the
    /// opening request of a new one, never by time passing.
    pub(crate) older_exhausted: bool,
    /// What `ohlcv_history` said last poll, so the rising edge can be seen.
    ///
    /// MetaTrader narrows its capabilities when the bridge says hello, which
    /// happens *after* the pane may already have asked and been answered
    /// with nothing. The edge is what asks again once the answer can be real.
    capable: bool,
    /// Whether the next candle request asks for slices. Only read when a
    /// request is *sent*: flipping the switch mid-run never reshapes an
    /// answer already being fetched.
    pub(crate) progressive: bool,
    /// Whether a pane that is *not* cut by time may carry the venue's own
    /// candles in front of its bars.
    ///
    /// Off by default, so a tick chart opens with the prints this session saw
    /// and nothing invented in front of them. On, the venue's 1-minute candles
    /// are installed unfolded — real candles, counted apart from built bars —
    /// and an indicator running across the seam averages minute candles with
    /// bars cut by trades, which is the honest cost the View menu states.
    pub(crate) lead_in: bool,
}

impl Default for VenueCandles {
    fn default() -> Self {
        Self {
            base: None,
            interval: CandleBaseInterval::default(),
            pending: false,
            stale: false,
            reaching_back: None,
            older_exhausted: false,
            capable: false,
            progressive: true,
            lead_in: false,
        }
    }
}

impl VenueCandles {
    /// Whether any pane cuts bars by a foldable time interval — the gate for
    /// venue candle history. Capability-shaped: the prefix belongs to what a
    /// pane *shows*, never to which pane object it is.
    fn any_pane_wants(&self, panes: &[PaneView<'_>]) -> bool {
        panes.iter().any(|pane| match pane.interval_ms {
            // An interval no whole number of minutes fits into folds to
            // nothing, so it wants none.
            Some(interval) => is_foldable(OHLCV_BASE_INTERVAL_MS, interval),
            // Cut by trades: candles are wanted only under the lead-in.
            None => self.lead_in,
        })
    }

    /// Follow the panes to the base they want now. Reports whether the wish
    /// changed — and with it every prefix, which this already rebuilt.
    ///
    /// A settled base is parked rather than dropped, and the one parked for
    /// the new wish comes back with what *load older* learned about it, so a
    /// trip to 1d and back neither refetches the minutes nor loses the history
    /// paged into them. With nothing parked, every prefix goes and the next
    /// poll asks.
    fn follow_wanted(&mut self, step: &mut Step<'_>) -> bool {
        let wanted = CandleBaseInterval::wanted_for(
            step.tab.panes.iter().map(|pane| pane.interval_ms),
            self.lead_in,
        );
        let Some(was_ms) = self.interval.want(wanted) else {
            return false;
        };
        let pending = self.pending;
        let held = self.base.is_some() || pending;
        // A base still filling is not parked: its remaining slices are stale.
        // One whose only request out is for the seam's minutes is settled.
        let settled = if pending && !self.interval.seam_minutes_out() {
            None
        } else {
            self.base.take()
        };
        let restored = self
            .interval
            .swap_parked(was_ms, settled, self.older_exhausted);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "OHLCV_BASE_INTERVAL_CHANGED",
            symbol = %step.tab.symbol,
            was_ms,
            now_ms = wanted,
            action = match (&restored, held) {
                (Some(_), _) => "restore_parked",
                (None, true) => "park_and_fetch",
                (None, false) => "fetch",
            },
            "the panes now fold from a different candle base"
        );
        step.event(SessionEvent::BaseChanged {
            was_ms,
            now_ms: wanted,
        });
        self.base = None;
        self.reaching_back = None;
        self.older_exhausted = false;
        // Slices of the superseded answer may still be on their way.
        self.stale = pending;
        if let Some(restored) = restored {
            self.base = Some(restored.bars);
            self.older_exhausted = restored.older_exhausted;
            self.fold_into_panes(step);
        } else if held {
            step.clear_prefixes();
        }
        true
    }

    /// Ask the venue for its candle history, if there is anything to ask.
    ///
    /// Gated on the capability, never on the provider: a feed that serves no
    /// candles is not asked, and neither is a recording with no context file
    /// beside it — but a recording that *has* one is. One request at a time,
    /// and a base already held is not re-fetched: only a base never fetched
    /// is asked, and a base held asks only for the minutes its seam leads
    /// lack.
    fn request_opening(&mut self, step: &mut Step<'_>, tab_id: u64) {
        // The base is read off the panes first: a context pane just built on
        // a restored 1mo asks for days from its first request.
        self.follow_wanted(step);
        if !self.any_pane_wants(step.tab.panes) || self.pending {
            return;
        }
        let seam_minutes = self.base.is_some();
        let ask = if seam_minutes {
            self.seam_minutes_ask(step.tab.panes)
        } else {
            Some(self.interval.ask(self.progressive))
        };
        let Some(ask) = ask.filter(|_| step.tab.capabilities.ohlcv_history) else {
            return;
        };
        step.feed(FeedRequest::Candles {
            tab_id,
            ask,
            purpose: if seam_minutes {
                CandlePurpose::SeamMinutes
            } else {
                CandlePurpose::Opening
            },
        });
    }

    /// What became of a candle request: the request is only *out* once the
    /// feed took it, so everything it changes waits for this.
    pub(crate) fn acknowledge(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        ask: CandleAsk,
        purpose: CandlePurpose,
        delivery: Delivery,
    ) {
        let CandleAsk {
            interval_ms,
            span_ms,
            slice_ms,
            before_ms,
        } = ask;
        let older = matches!(purpose, CandlePurpose::Older { .. });
        match delivery {
            Delivery::Sent => {
                self.pending = true;
                match purpose {
                    CandlePurpose::SeamMinutes => self.interval.seam_minutes_sent(&ask),
                    CandlePurpose::Opening => {
                        self.reaching_back = None;
                        self.older_exhausted = false;
                    }
                    CandlePurpose::Older { oldest_ms } => self.reaching_back = Some(oldest_ms),
                }
                step.wait(Wait::VenueCandles, WaitChange::Begin);
                if older {
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "OHLCV_OLDER_REQUESTED",
                        tab = tab_id,
                        symbol = %step.tab.symbol,
                        interval_ms,
                        span_ms,
                        before_ms = before_ms.unwrap_or(0),
                        slice_ms = slice_ms.unwrap_or(0),
                        action = "await_prepend",
                        "asked the venue for another span of older candles"
                    );
                } else {
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = if purpose == CandlePurpose::SeamMinutes {
                            "OHLCV_SEAM_MINUTES_REQUESTED"
                        } else {
                            "OHLCV_REQUESTED"
                        },
                        tab = tab_id,
                        symbol = %step.tab.symbol,
                        interval_ms,
                        span_ms,
                        slice_ms = slice_ms.unwrap_or(0),
                        before_ms = before_ms.unwrap_or(0),
                        action = if slice_ms.is_some() { "await_slices" } else { "await_single_reply" },
                        "asked the venue for candle history"
                    );
                }
            }
            // A full channel is a busy frame: the every-frame poll asks again,
            // and *load older* waits for the next click.
            Delivery::Busy => tracing::debug!(
                target: "quantick::app",
                event_code = if older { "OHLCV_OLDER_BACKPRESSURE" } else { "OHLCV_REQUEST_BACKPRESSURE" },
                tab = tab_id,
                action = if older { "retry_on_next_click" } else { "retry_next_frame" },
                "candle-history request not queued; channel full"
            ),
            // A closed one is a feed thread that is gone, which is worth
            // saying out loud: nothing will answer, ever.
            Delivery::Gone => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = if older { "OHLCV_OLDER_CHANNEL_CLOSED" } else { "OHLCV_REQUEST_CHANNEL_CLOSED" },
                tab = tab_id,
                symbol = %step.tab.symbol,
                action = "no_history_until_feed_restart",
                "candle-history request cannot be sent; the feed is gone"
            ),
        }
    }

    /// Watch the capability for the false→true edge and ask when it lands.
    ///
    /// Rate: **per-frame**; two bools and an `Option` when there is nothing
    /// to do.
    pub(crate) fn poll(&mut self, step: &mut Step<'_>, tab_id: u64) {
        let capabilities = step.tab.capabilities;
        let capable = capabilities.ohlcv_history;
        let rising = capable && !self.capable;
        self.capable = capable;
        if self.interval.observe(&capabilities) {
            // The venue re-answered the base this chart wants. What is held
            // goes whether or not it had bars in it, and with it everything
            // learned by reaching back through it; slices of the discarded
            // answer still on their way describe a base that no longer exists.
            self.base = None;
            self.reaching_back = None;
            self.older_exhausted = false;
            self.stale = self.pending;
        }
        // A session that narrowed *into* serving candles may have answered an
        // earlier request with nothing; only the edge clears that answer — a
        // steady-state empty base is a real "the venue has none".
        if rising && self.base.as_ref().is_some_and(Vec::is_empty) {
            self.base = None;
        }
        // A pane that changed interval since the last frame may want the
        // other base; this catches any path that changed a spec without a
        // refold.
        self.follow_wanted(step);
        // Unconditional: every guard inside makes this a no-op once the
        // request is out or answered, and asking here is what retries a
        // request the command channel refused.
        self.request_opening(step, tab_id);
    }

    /// Take a candle-history reply, and put it in front of the panes.
    ///
    /// An empty reply is a complete answer — the venue has none, the provider
    /// serves none, or the fetch failed — and is recorded as such so the tab
    /// stops asking. Either way the wait ends on the *closing* slice.
    /// Progressive slices run newest-first, so each one goes in *front* of the
    /// base already held, and the prefix is rebuilt after every slice: the
    /// chart grows leftwards while the rest is still being fetched.
    pub(crate) fn take_reply(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        interval_ms: i64,
        bars: Vec<Bar>,
        slice: OhlcvSlice,
    ) {
        let last = slice.is_last();
        step.event(SessionEvent::CandleReply {
            interval_ms,
            bars: bars.len(),
            slice,
            discarded: self.stale,
        });
        if self.stale {
            // An answer the tab already threw away. The closing slice is what
            // re-opens the door to asking again.
            if last {
                self.stale = false;
                self.pending = false;
                // Whatever this answer was measured against no longer exists.
                self.reaching_back = None;
                self.interval.forget_seam_minutes();
                step.wait(Wait::VenueCandles, WaitChange::End);
            }
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_SLICE_DISCARDED",
                tab = tab_id,
                bars = bars.len(),
                last,
                action = "await_fresh_request",
                "dropped a slice of a candle answer that was superseded"
            );
            return;
        }
        if self.interval.seam_minutes_out() {
            // The seam's minutes: parked beside the days or joining the minute
            // base, never a verdict on *load older*.
            let received = bars.len();
            let taken = self.interval.take_seam_minutes(interval_ms, bars, slice);
            tracing::info!(
                target: "quantick::app",
                event_code = "OHLCV_SEAM_MINUTES_RECEIVED",
                tab = tab_id,
                bars = received,
                last,
                action = taken.token(),
                "minutes for the seam lead arrived"
            );
            if let (SeamMinutesTaken::JoinBase(minutes), Some(base)) = (taken, self.base.as_mut()) {
                merge_older_candles(base, minutes);
            }
            if last {
                self.pending = false;
                step.wait(Wait::VenueCandles, WaitChange::End);
            }
            self.refold(step);
            return;
        }
        if !last {
            self.take_slice(step, interval_ms, bars);
            return;
        }
        self.pending = false;
        if slice == OhlcvSlice::Refused {
            // Nobody looked, so nothing is known. End the wait and touch
            // nothing else: no short-answer warning, no refold over an empty
            // vector, and no verdict on a reach-back that was never served.
            self.reaching_back = None;
            step.wait(Wait::VenueCandles, WaitChange::End);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_REFUSED",
                tab = tab_id,
                symbol = %step.tab.symbol,
                action = "await_the_running_fetch",
                "the provider was already fetching; this request was not served"
            );
            return;
        }
        let complete = matches!(slice, OhlcvSlice::Last { complete } if complete);
        if !complete {
            // Known-short, not merely short: a venue that stopped answering
            // partway, or a block clipped to a cap. An instrument younger than
            // the span is a *complete* answer with fewer bars.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_INCOMPLETE",
                tab = tab_id,
                symbol = %step.tab.symbol,
                interval_ms,
                bars = bars.len(),
                action = "install_what_arrived",
                "the venue's candle history stopped short of the span asked for"
            );
        }
        step.wait(Wait::VenueCandles, WaitChange::End);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "OHLCV_RECEIVED",
            tab = tab_id,
            symbol = %step.tab.symbol,
            interval_ms,
            bars = bars.len(),
            action = if bars.is_empty() { "no_prefix" } else { "install_prefix" },
            "candle history arrived"
        );
        let usable = self.take_slice(step, interval_ms, bars);
        if let Some(was_oldest) = self.reaching_back.take() {
            self.settle_older(step, tab_id, was_oldest, complete, usable);
        }
    }

    /// A *load older* that closed: did the oldest candle actually move? A
    /// reply that re-sends what is already held is not an empty reply, so the
    /// bar count cannot answer this and the oldest bucket can.
    ///
    /// Only a *complete* and *usable* answer teaches anything about where the
    /// record starts. A run that came up short brought nothing older for a
    /// reason unrelated to the venue's depth, and latching on it would retire
    /// the button for the session.
    fn settle_older(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        was_oldest: i64,
        complete: bool,
        usable: bool,
    ) {
        let now_oldest = self.oldest();
        let moved = now_oldest.is_some_and(|oldest| oldest < was_oldest);
        self.older_exhausted = complete && usable && !moved;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "OHLCV_OLDER_SETTLED",
            tab = tab_id,
            symbol = %step.tab.symbol,
            was_oldest_ms = was_oldest,
            now_oldest_ms = now_oldest.unwrap_or(0),
            complete,
            usable,
            moved,
            exhausted = self.older_exhausted,
            action = if self.older_exhausted {
                "stop_offering_older"
            } else if moved {
                "older_available"
            } else {
                "no_evidence_try_again"
            },
            "a request for older candles settled"
        );
        step.event(SessionEvent::OlderCandlesSettled {
            moved,
            exhausted: self.older_exhausted,
        });
    }

    /// The minutes a daily base's seam leads lack, when one does.
    fn seam_minutes_ask(&self, panes: &[PaneView<'_>]) -> Option<CandleAsk> {
        let base = self.base.as_ref()?;
        let seams = panes
            .iter()
            .filter_map(|pane| Some((pane.first_print_open_ms?, pane.interval_ms?)));
        self.interval.seam_minutes_ask(base, seams)
    }

    /// Merge one slice into the base and rebuild the prefix from it.
    ///
    /// Reports whether the slice was *usable* — an interval the panes can
    /// fold from. Merging rather than replacing is what makes a run's last
    /// slice, the *oldest* week of the span, keep the twelve already drawn.
    fn take_slice(&mut self, step: &mut Step<'_>, interval_ms: i64, bars: Vec<Bar>) -> bool {
        let base_is_empty = self.base.as_ref().is_none_or(Vec::is_empty);
        let admitted = self
            .interval
            .admit(interval_ms, base_is_empty, !bars.is_empty());
        if let Err(expected_ms) = admitted {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_UNEXPECTED_BASE",
                interval_ms,
                expected_ms,
                action = if self.base.is_some() {
                    "refuse_slice_keep_prefix"
                } else {
                    "no_prefix"
                },
                "candle history arrived at an interval the pane cannot fold from"
            );
            // Refuse the slice, never the history: an empty base is how "this
            // venue serves nothing this pane can fold" is remembered, but a
            // reach-back at a bad interval must not throw away what was paged.
            if self.base.is_none() {
                self.base = Some(Vec::new());
            }
            return false;
        }
        let base = self.base.get_or_insert_with(Vec::new);
        if base.is_empty() {
            *base = bars;
        } else if !bars.is_empty() {
            merge_older_candles(base, bars);
        }
        self.refold(step);
        true
    }

    /// A reply handed in directly, as the feed drain does. The pending flag
    /// and the wait are raised first because that is what a request having
    /// gone out looks like.
    pub(crate) fn deliver(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
        interval_ms: i64,
        bars: Vec<Bar>,
        slice: OhlcvSlice,
    ) {
        if !self.pending {
            self.pending = true;
            step.wait(Wait::VenueCandles, WaitChange::Begin);
        }
        self.take_reply(step, tab_id, interval_ms, bars, slice);
    }

    pub(crate) fn candles_held(&self) -> usize {
        self.base.as_ref().map_or(0, Vec::len)
    }

    pub(crate) const fn holds_base(&self) -> bool {
        self.base.is_some()
    }

    pub(crate) fn record_starts_inside(&self, interval_ms: i64) -> bool {
        self.base
            .as_ref()
            .is_some_and(|base| starts_inside_bucket(base, self.interval.held_ms(), interval_ms))
    }

    /// The oldest venue candle held, by bucket start.
    fn oldest(&self) -> Option<i64> {
        self.base.as_ref()?.first().map(|bar| bar.open_time)
    }

    /// Whether asking for older candles could get the trader anything — and
    /// when it could not, which of the reasons it is. One enum, so the reason
    /// the disabled control shows is the reason.
    pub(crate) fn older_candles(
        &self,
        capabilities: FeedCapabilities,
        panes: &[PaneView<'_>],
    ) -> OlderCandles {
        if !capabilities.ohlcv_history {
            OlderCandles::FeedServesNone
        } else if !self.any_pane_wants(panes) {
            OlderCandles::NoChartCutByTime
        } else if self.pending {
            OlderCandles::Fetching
        } else if self.oldest().is_none() {
            OlderCandles::NotArrivedYet
        } else if self.older_exhausted {
            OlderCandles::RecordStartsHere
        } else {
            OlderCandles::Available
        }
    }

    /// Put the venue's candles in front of a chart cut by trades, or take
    /// them away again; refolds on the spot. Idempotent.
    pub(crate) fn set_lead_in(&mut self, step: &mut Step<'_>, enabled: bool) {
        if self.lead_in == enabled {
            return;
        }
        self.lead_in = enabled;
        self.refold(step);
    }

    /// Reach one more span into the past, its right-hand edge just before the
    /// oldest candle held, so the windows never overlap and the merge on the
    /// way back has nothing to reconcile.
    pub(crate) fn request_older(
        &mut self,
        step: &mut Step<'_>,
        tab_id: u64,
    ) -> Result<(), Refusal> {
        let reason = self.older_candles(step.tab.capabilities, step.tab.panes);
        let (OlderCandles::Available, Some(oldest)) = (reason, self.oldest()) else {
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_OLDER_DECLINED",
                tab = tab_id,
                pending = self.pending,
                exhausted = self.older_exhausted,
                held = self.oldest().is_some(),
                "nothing older to ask for"
            );
            return Err(Refusal::OlderCandles(reason));
        };
        // One millisecond before the oldest bucket start held: the plan's
        // windows are closed at both ends.
        let ask = CandleAsk {
            before_ms: Some(oldest.saturating_sub(1)),
            ..self.interval.ask(self.progressive)
        };
        step.feed(FeedRequest::Candles {
            tab_id,
            ask,
            purpose: CandlePurpose::Older { oldest_ms: oldest },
        });
        Ok(())
    }

    /// A pane was built: a base already held (a layout toggled off and on) is
    /// folded rather than re-fetched; otherwise this is the first moment
    /// asking for one means anything.
    pub(crate) fn pane_added(&mut self, step: &mut Step<'_>, tab_id: u64) {
        if self.base.is_some() {
            self.refold(step);
        } else {
            self.request_opening(step, tab_id);
        }
    }

    /// Take over a fresh feed session. `keep_timeline` keeps the candles and
    /// what reaching back learned; anything in flight goes either way, with
    /// the channel carrying it.
    pub(crate) fn attach(&mut self, step: &mut Step<'_>, keep_timeline: bool) {
        if !keep_timeline {
            // The old market's candles, and how far back it reached, describe
            // the old market.
            self.base = None;
            self.reaching_back = None;
            self.older_exhausted = false;
            self.interval.drop_parked();
            self.capable = false;
            step.clear_prefixes();
        }
        self.pending = false;
        self.stale = false;
        self.interval.channel_dropped();
    }

    pub(crate) fn forget_generations(&mut self) {
        self.interval.forget_generations();
    }

    /// Rebuild every pane's prefix from the base at that pane's interval.
    ///
    /// Free of the venue: a chip click lands here, not on the network. One
    /// base, one fold per time-cutting pane.
    pub(crate) fn refold(&mut self, step: &mut Step<'_>) {
        // A change of interval can change the base itself; following it
        // already rebuilt or cleared every prefix.
        if self.follow_wanted(step) {
            return;
        }
        self.fold_into_panes(step);
    }

    fn fold_into_panes(&self, step: &mut Step<'_>) {
        let Some(base) = self.base.as_ref() else {
            return;
        };
        let base_interval_ms = self.interval.held_ms();
        for (index, pane) in step.tab.panes.iter().enumerate() {
            let effect = match pane.interval_ms {
                Some(interval_ms) => {
                    let folded = fold(base, base_interval_ms, interval_ms);
                    let bars = trim_to_seam(folded, pane.first_bar, pane.partial, interval_ms);
                    match pane.first_print_open_ms {
                        Some(first_print_ms) => {
                            let outcome =
                                self.interval.seam_lead(base, first_print_ms, interval_ms);
                            let seam = SeamTrace {
                                first_print_ms,
                                interval_ms,
                                token: outcome.token(),
                            };
                            let covers_seam = outcome.covers_seam();
                            Effect::InstallPrefix {
                                pane: index,
                                bars,
                                lead: outcome.into_bar(),
                                covers_seam,
                                seam: Some(seam),
                            }
                        }
                        None => prefix_only(index, bars),
                    }
                }
                // A pane not cutting by time has no interval to fold *to*, so
                // the base goes in unfolded under the lead-in: the venue's own
                // minutes in front of bars cut by trades, each counted as what
                // it is. Without the switch the honest answer is no prefix.
                None if self.lead_in && base_interval_ms == OHLCV_BASE_INTERVAL_MS => prefix_only(
                    index,
                    trim_borrowed_to_seam(base, pane.first_bar, pane.partial, base_interval_ms),
                ),
                None => prefix_only(index, Vec::new()),
            };
            step.effects.push(effect);
        }
    }
}

/// A prefix with no seam lead behind it.
fn prefix_only(pane: usize, bars: Vec<Bar>) -> Effect {
    Effect::InstallPrefix {
        pane,
        bars,
        lead: None,
        covers_seam: false,
        seam: None,
    }
}
