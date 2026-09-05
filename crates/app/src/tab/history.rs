//! Reaching back: venue candle history and the trade-history page runs.
//!
//! Everything a tab does to put *older* data in front of what the live feed
//! has already given it. Two reaches share this file because they share a
//! seam: the venue's own candles fold in as a prefix under the bars the tape
//! built, and a page of older trades extends the tape itself. Both are
//! request/reply state machines with one request in flight, both answer the
//! same *load older* press, and both are read without any of the drawing,
//! draining or strategy code the rest of a tab carries.

use tokio::sync::mpsc;

use super::{HISTORY_NOTE_LINGER, HistoryNote, Tab};
use crate::config::{AppConfig, FeedCapabilities};
use crate::loading::LoadingTask;
use quantick_feed::FeedCommand;
use quantick_feed::history_reach::{
    self, Campaign, CampaignEnd, CampaignStep, EMPTY_PAGE_NOTICE, REQUEST_REFUSED_NOTICE,
};

/// Put an older slice of venue candles in front of the ones already held,
/// keeping the base ascending by `open_time` and free of duplicates.
///
/// The fast path is the one progressive loading actually produces: the slice
/// is strictly older than everything held, so it is spliced in front and the
/// order is already right. The merge below exists for the case the port
/// permits but no provider aims for — a window that overlaps what is held,
/// through a venue re-reporting a bucket at a boundary. There the candle
/// already on screen wins: it is the one the trader has been reading, and a
/// bar that redraws itself for no visible reason is worse than a bar fetched
/// a second apart from an identical twin.
fn merge_older_candles(base: &mut Vec<quantick_engine::Bar>, older: Vec<quantick_engine::Bar>) {
    let disjoint = match (older.last(), base.first()) {
        (Some(newest_incoming), Some(oldest_held)) => {
            newest_incoming.open_time < oldest_held.open_time
        }
        _ => true,
    };
    if disjoint {
        base.splice(0..0, older);
        return;
    }
    let mut merged: std::collections::BTreeMap<i64, quantick_engine::Bar> =
        older.into_iter().map(|bar| (bar.open_time, bar)).collect();
    for bar in base.drain(..) {
        merged.insert(bar.open_time, bar);
    }
    *base = merged.into_values().collect();
}

/// Drop venue bars that overlap the trade-derived series.
///
/// The two series meet at a seam, and the composed chart is only searchable if
/// `open_time` never decreases across it. A venue candle covering the same
/// window as the first engine bar would sit *after* it in time while sitting
/// before it in slot order, so every venue bucket from that one on is dropped:
/// what the app cut from prints is the better record of that window anyway.
///
/// With no engine bars yet the whole prefix stands — there is nothing to
/// overlap.
fn trim_to_seam(
    mut folded: Vec<quantick_engine::Bar>,
    first_engine_bar: Option<&quantick_engine::Bar>,
    partial: Option<&quantick_engine::Bar>,
    interval_ms: i64,
) -> Vec<quantick_engine::Bar> {
    let Some(seam) = seam_bucket_ms(first_engine_bar, partial, interval_ms) else {
        return folded;
    };
    folded.retain(|bar| bar.open_time < seam);
    folded
}

/// The same trim over a block the caller only has on loan.
///
/// Two functions rather than one taking a `Cow`, because they pay for
/// different things and both paths are on the diet. The owning one above trims
/// a vector `resample::fold` just built and is about to drop — a `retain` there
/// copies nothing at all. This one is handed the venue's whole base, which the
/// tab keeps, so it copies out only the bars that survive rather than cloning a
/// week of minutes in order to throw most of them away.
fn trim_borrowed_to_seam(
    base: &[quantick_engine::Bar],
    first_engine_bar: Option<&quantick_engine::Bar>,
    partial: Option<&quantick_engine::Bar>,
    interval_ms: i64,
) -> Vec<quantick_engine::Bar> {
    let Some(seam) = seam_bucket_ms(first_engine_bar, partial, interval_ms) else {
        return base.to_vec();
    };
    base.iter()
        .filter(|bar| bar.open_time < seam)
        .cloned()
        .collect()
}

/// Where the venue's candles have to stop for the pane's own bars to begin, or
/// `None` when there are no bars yet and the whole block stands.
///
/// Buckets, not stamps. A venue candle's `open_time` is its bucket start; an
/// engine bar's is its *first trade*, which sits strictly inside the bucket.
/// Comparing the two raw would keep the venue candle covering the same window
/// and put a later-closing bar in an earlier slot.
///
/// One owner for the rule, so the two trims above cannot drift apart about
/// where the seam is.
fn seam_bucket_ms(
    first_engine_bar: Option<&quantick_engine::Bar>,
    partial: Option<&quantick_engine::Bar>,
    interval_ms: i64,
) -> Option<i64> {
    let first = first_engine_bar.or(partial)?;
    Some(crate::resample::bucket_start(first.open_time, interval_ms))
}

/// Whether the chart can reach further back for venue candles, and when it
/// cannot, why — see [`Tab::older_candles`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OlderCandles {
    /// Another span can be asked for.
    Available,
    /// This feed publishes no candle history at all.
    FeedServesNone,
    /// Nothing on this chart is cut by time, so no venue candle was ever
    /// wanted — the prefix follows what a pane *shows*, not which pane it is.
    NoChartCutByTime,
    /// A request is out; the answer is what to wait for.
    Fetching,
    /// The opening span has not landed yet. There is nothing to reach back
    /// *from* until it does.
    NotArrivedYet,
    /// A reach-back came back complete with nothing older in it. That is the
    /// venue's record, or the provider's, and it is the one reason here that
    /// had to be learned by asking.
    RecordStartsHere,
}

impl OlderCandles {
    /// Whether the control is live.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// What to tell the trader hovering a control this state disabled.
    /// `None` when it is not disabled.
    #[must_use]
    pub const fn why_not(self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::FeedServesNone => Some("this feed publishes no candle history"),
            Self::NoChartCutByTime => Some(
                "no chart here is cut by time, so there are no venue candles \
                 to extend — switch on the venue lead-in to put them in front \
                 of a chart cut by trades",
            ),
            Self::Fetching => Some("a request is already out; this is what it is fetching"),
            Self::NotArrivedYet => Some("the first span has not arrived yet"),
            Self::RecordStartsHere => Some("this is as far back as the venue's record goes"),
        }
    }
}

impl Tab {
    /// Whether any pane on this tab cuts bars by a foldable time interval —
    /// the gate for venue candle history. Capability-shaped, like every other
    /// gate in the app (audit S1): the prefix belongs to what a pane *shows*
    /// (`BarSpec::Time` at a whole number of venue candles), never to which
    /// pane object it is. `bars → time` on the flow pane earns the same span
    /// the split's time pane gets.
    fn any_pane_wants_venue_history(&self) -> bool {
        std::iter::once(&self.flow_pane)
            .chain(self.time_pane())
            .any(|pane| match pane.state.spec().time_interval_ms() {
                // Cut by time: the venue's candles fold into this pane's own
                // interval, which is what the prefix has always been. An
                // interval no whole number of minutes fits into still folds to
                // nothing, so it still wants none — the lead-in does not change
                // that, because the pane would draw the answer and discard it.
                Some(interval) => crate::resample::is_foldable(interval),
                // Cut by trades: no fold exists, so candles are wanted only
                // when the trader asked for the lead-in that installs them
                // unfolded.
                None => self.venue_lead_in,
            })
    }

    /// Ask the venue for its candle history, if there is anything to ask.
    ///
    /// Gated on the capability, never on the provider: a feed that serves no
    /// candles is not asked, and neither is a recording with no context file
    /// beside it — but a recording that *has* one is, because that file is the
    /// run-up it was downloaded to carry. One request at a time, and a base
    /// already held is not re-fetched: changing a pane's interval is a
    /// different fold over the same bars.
    pub(super) fn request_ohlcv_history(&mut self, config: &AppConfig) {
        let progressive = self.progressive_history;
        // Not gated on the source. A recording answers this from the context
        // file downloaded beside it — the run-up it exists to carry — and the
        // capability is already false on one that has none, so a replay
        // without context is simply never asked. Refusing here instead meant
        // a recording opened with no context at all and only picked it up if
        // the trader happened to press *load older*.
        if !self.any_pane_wants_venue_history()
            || self.ohlcv_pending
            || self.ohlcv_base.is_some()
            || !self.capabilities(config).ohlcv_history
        {
            return;
        }
        let slice_ms = progressive.then_some(quantick_feed::OHLCV_SLICE_SPAN_MS);
        let command = FeedCommand::FetchOhlcv {
            span_ms: quantick_feed::TIME_HISTORY_SPAN_MS,
            slice_ms,
            // The opening request: back from the live edge. Reaching further
            // than one span is `request_older_ohlcv_history`'s job.
            before_ms: None,
        };
        match self.commands.try_send(command) {
            Ok(()) => {
                self.ohlcv_pending = true;
                self.ohlcv_reaching_back = None;
                self.ohlcv_older_exhausted = false;
                self.loading.begin(LoadingTask::VenueHistory);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_REQUESTED",
                    tab = self.id,
                    symbol = %self.symbol,
                    span_ms = quantick_feed::TIME_HISTORY_SPAN_MS,
                    slice_ms = slice_ms.unwrap_or(0),
                    action = if progressive { "await_slices" } else { "await_single_reply" },
                    "asked the venue for candle history"
                );
            }
            // A full channel is a busy frame, and the every-frame poll asks
            // again. A closed one is a feed thread that is gone, which is
            // worth saying out loud: nothing will answer, ever.
            Err(mpsc::error::TrySendError::Full(_)) => tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_REQUEST_BACKPRESSURE",
                tab = self.id,
                action = "retry_next_frame",
                "candle-history request not queued; channel full"
            ),
            Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_REQUEST_CHANNEL_CLOSED",
                tab = self.id,
                symbol = %self.symbol,
                action = "no_history_until_feed_restart",
                "candle-history request cannot be sent; the feed is gone"
            ),
        }
    }

    /// Watch the capability for the false→true edge and ask when it lands.
    ///
    /// Called every frame: the check is two bools and an `Option` when there
    /// is nothing to do.
    pub fn poll_ohlcv_capability(&mut self, config: &AppConfig) {
        let capabilities = self.capabilities(config);
        let capable = capabilities.ohlcv_history;
        let rising = capable && !self.ohlcv_capable;
        self.ohlcv_capable = capable;
        if capabilities.ohlcv_generation != self.ohlcv_generation {
            // The venue re-answered. A reconnect can carry a longer block than
            // the one held, or a corrected one, so what is held goes whether or
            // not it had bars in it — the guard below then lets a fresh request
            // through, and the reply reinstalls the prefix by the same path the
            // first one took.
            self.ohlcv_generation = capabilities.ohlcv_generation;
            self.ohlcv_base = None;
            // And with it, everything learned by reaching back through it. The
            // oldest bucket a request was measured against is gone, so a reply
            // still in flight must not be compared to it; and "the record
            // starts here" was a fact about a block this generation replaces.
            // `attach` clears both for the same reason on a change of market.
            self.ohlcv_reaching_back = None;
            self.ohlcv_older_exhausted = false;
            // Slices of the discarded answer may still be on their way. They
            // describe a base that no longer exists, so they are dropped
            // rather than folded onto nothing.
            self.ohlcv_stale = self.ohlcv_pending;
        }
        if rising {
            // A session that narrowed *into* serving candles may have answered
            // an earlier request with nothing; that answer described a feed
            // that did not know itself yet. Only the edge clears it — a
            // steady-state empty base is a real "the venue has none".
            if self.ohlcv_base.as_ref().is_some_and(Vec::is_empty) {
                self.ohlcv_base = None;
            }
        }
        // Unconditional: every guard inside makes this a no-op once the
        // request is out or answered, and asking here is what actually retries
        // a request the command channel refused. The feed ignores a duplicate
        // while one is in flight, and `ohlcv_pending` means we never send one.
        self.request_ohlcv_history(config);
    }

    /// Take a candle-history reply, and put it in front of the time pane.
    ///
    /// An empty reply is a complete answer — the venue has none, the provider
    /// serves none, or the fetch failed — and is recorded as such so the tab
    /// stops asking. Either way the wait ends on the *closing* slice: a
    /// provider that answered only on success would strand the spinner on the
    /// one case that most needs explaining, and one that ended it on the first
    /// of thirteen slices would claim a quarter of history had arrived when a
    /// week of it had.
    ///
    /// Progressive slices run newest-first, so each one goes in *front* of the
    /// base already held. The prefix is rebuilt after every slice, which is
    /// the whole point — the chart grows leftwards while the rest is still
    /// being fetched, and nothing the trader is looking at moves under them
    /// (`ChartPane::install_history_prefix` shifts the viewport and every
    /// bar-anchored drawing by the same amount the prefix grew).
    pub(super) fn take_ohlcv_history(
        &mut self,
        interval_ms: i64,
        bars: Vec<quantick_engine::Bar>,
        slice: quantick_feed::OhlcvSlice,
    ) {
        let last = slice.is_last();
        if self.ohlcv_stale {
            // An answer the tab already threw away (see `poll_ohlcv_capability`).
            // The closing slice is what re-opens the door to asking again.
            if last {
                self.ohlcv_stale = false;
                self.ohlcv_pending = false;
                // Whatever this answer was measured against no longer exists.
                self.ohlcv_reaching_back = None;
                self.loading.end(LoadingTask::VenueHistory);
            }
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_SLICE_DISCARDED",
                tab = self.id,
                bars = bars.len(),
                last,
                action = "await_fresh_request",
                "dropped a slice of a candle answer that was superseded"
            );
            return;
        }
        if !last {
            self.take_ohlcv_slice(interval_ms, bars);
            return;
        }
        self.ohlcv_pending = false;
        if slice == quantick_feed::OhlcvSlice::Refused {
            // Nobody looked, so nothing is known. End the wait — the spinner
            // was raised before the command left — and touch nothing else: no
            // short-answer warning about a venue that never answered, no
            // refold of the whole base over an empty vector, and above all no
            // verdict on a reach-back that was never served.
            self.ohlcv_reaching_back = None;
            self.loading.end(LoadingTask::VenueHistory);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_REFUSED",
                tab = self.id,
                symbol = %self.symbol,
                action = "await_the_running_fetch",
                "the provider was already fetching; this request was not served"
            );
            return;
        }
        let complete = matches!(slice, quantick_feed::OhlcvSlice::Last { complete } if complete);
        if !complete {
            // Known-short, not merely short: a venue that stopped answering
            // partway, or a block clipped to a cap. An instrument younger than
            // the span is a *complete* answer with fewer bars, which is why
            // this is carried rather than guessed from the count.
            //
            // Said in the log today. §11 records where it will be said on
            // screen — beside the three-way bar count — and the badge itself
            // is deferred rather than half-built.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_INCOMPLETE",
                tab = self.id,
                symbol = %self.symbol,
                interval_ms,
                bars = bars.len(),
                action = "install_what_arrived",
                "the venue's candle history stopped short of the span asked for"
            );
        }
        self.loading.end(LoadingTask::VenueHistory);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "OHLCV_RECEIVED",
            tab = self.id,
            symbol = %self.symbol,
            interval_ms,
            bars = bars.len(),
            action = if bars.is_empty() { "no_prefix" } else { "install_prefix" },
            "candle history arrived"
        );
        let usable = self.take_ohlcv_slice(interval_ms, bars);
        // A *load older* that closed: did the oldest candle actually move? A
        // reply that re-sends what is already held is not an empty reply, so
        // the bar count cannot answer this and the oldest bucket can. Latched
        // here rather than asked again, because the only way to find out was
        // to ask.
        if let Some(was_oldest) = self.ohlcv_reaching_back.take() {
            let now_oldest = self.oldest_venue_candle_ms();
            let moved = now_oldest.is_some_and(|oldest| oldest < was_oldest);
            // Only a *complete* answer teaches anything about where the record
            // starts. A run that came up short — a venue that stopped
            // answering, a socket that failed — brought nothing older for a
            // reason that has nothing to do with the venue's depth, and
            // latching on it would retire the button for the session and tell
            // the trader their history starts here. Short means try again.
            // Complete *and* usable. A run that came up short brought nothing
            // older for a reason unrelated to the venue's depth, and a reply
            // the pane had to refuse is not an answer about depth at all.
            self.ohlcv_older_exhausted = complete && usable && !moved;
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_OLDER_SETTLED",
                tab = self.id,
                symbol = %self.symbol,
                was_oldest_ms = was_oldest,
                now_oldest_ms = now_oldest.unwrap_or(0),
                complete,
                usable,
                moved,
                exhausted = self.ohlcv_older_exhausted,
                action = if self.ohlcv_older_exhausted {
                    "stop_offering_older"
                } else if moved {
                    "older_available"
                } else {
                    "no_evidence_try_again"
                },
                "a request for older candles settled"
            );
        }
    }

    /// Merge one slice into the base and rebuild the prefix from it.
    ///
    /// Reports whether the slice was *usable* — an interval this pane can fold
    /// from. A refused slice is not an answer about the venue's depth, and the
    /// exhaustion latch upstream must not read it as one.
    ///
    /// Shared by the closing reply and every slice before it: whether an
    /// answer arrived whole or in thirteen pieces changes when the wait ends,
    /// never how the candles are installed. Merging rather than replacing is
    /// what makes that true — a run's last slice is the *oldest* week of the
    /// span, and assigning it over the base would throw away the twelve that
    /// had already been drawn.
    fn take_ohlcv_slice(&mut self, interval_ms: i64, bars: Vec<quantick_engine::Bar>) -> bool {
        if interval_ms != quantick_feed::OHLCV_BASE_INTERVAL_MS && !bars.is_empty() {
            // The event tags its own interval so a consumer never has to
            // guess; a base this fold was not written for is refused rather
            // than folded wrongly.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_UNEXPECTED_BASE",
                interval_ms,
                expected_ms = quantick_feed::OHLCV_BASE_INTERVAL_MS,
                action = if self.ohlcv_base.is_some() {
                    "refuse_slice_keep_prefix"
                } else {
                    "no_prefix"
                },
                "candle history arrived at an interval the pane cannot fold from"
            );
            // Refuse the slice, never the history. Recording an empty base is
            // right for the *opening* answer — it is how "this venue serves
            // nothing this pane can fold" is remembered — but a reach-back
            // reply at a bad interval arrives on top of a week (or a quarter)
            // the trader has already paged in, and throwing that away would
            // cost them everything they waited for over one unusable slice.
            if self.ohlcv_base.is_none() {
                self.ohlcv_base = Some(Vec::new());
            }
            return false;
        }
        let base = self.ohlcv_base.get_or_insert_with(Vec::new);
        if base.is_empty() {
            *base = bars;
        } else if !bars.is_empty() {
            merge_older_candles(base, bars);
        }
        self.refold_history_prefix();
        true
    }

    /// Hand the tab one candle-history reply directly, as the feed drain does.
    ///
    /// The `QUANTICK_VENUE_HISTORY_DEMO` hook's one door in. It goes through
    /// the same function a real reply does — a hook that installed a prefix by
    /// another route would photograph a state the app cannot actually be in.
    /// The pending flags are set first because that is what a request having
    /// gone out looks like, and an unclosed run is precisely the frame the
    /// `partial` variant exists to reach.
    pub fn deliver_ohlcv_slice(
        &mut self,
        interval_ms: i64,
        bars: Vec<quantick_engine::Bar>,
        slice: quantick_feed::OhlcvSlice,
    ) {
        if !self.ohlcv_pending {
            self.ohlcv_pending = true;
            self.loading.begin(LoadingTask::VenueHistory);
        }
        self.take_ohlcv_history(interval_ms, bars, slice);
    }

    /// How many venue candles this tab holds, at the base interval. Zero on a
    /// feed that serves none, and before the first reply lands.
    #[must_use]
    pub fn venue_candles_held(&self) -> usize {
        self.ohlcv_base.as_ref().map_or(0, Vec::len)
    }

    /// The oldest venue candle held, by bucket start.
    fn oldest_venue_candle_ms(&self) -> Option<i64> {
        self.ohlcv_base.as_ref()?.first().map(|bar| bar.open_time)
    }

    /// Whether asking for older candles could get the trader anything — and
    /// when it could not, *which* of the reasons it is.
    ///
    /// A bool would be enough to grey the control out and is not enough to
    /// say why, and "why" is the whole of the disabled tooltip. Under the
    /// data-honesty rule a control that offers three possible reasons and is
    /// disabled for a fourth is telling the trader something untrue: on a tick
    /// chart the answer is not "the venue's record starts here", it is that
    /// nothing on this chart is cut by time, so no venue candle was ever
    /// wanted. One enum, so the reason shown is the reason.
    #[must_use]
    pub fn older_candles(&self, capabilities: FeedCapabilities) -> OlderCandles {
        if !capabilities.ohlcv_history {
            OlderCandles::FeedServesNone
        } else if !self.any_pane_wants_venue_history() {
            OlderCandles::NoChartCutByTime
        } else if self.ohlcv_pending {
            OlderCandles::Fetching
        } else if self.oldest_venue_candle_ms().is_none() {
            OlderCandles::NotArrivedYet
        } else if self.ohlcv_older_exhausted {
            OlderCandles::RecordStartsHere
        } else {
            OlderCandles::Available
        }
    }

    /// Put the venue's candles in front of a chart cut by trades, or take
    /// them away again.
    ///
    /// The named call behind the window's switch: it refolds on the spot, so
    /// the answer is on screen in the frame that flips it rather than at the
    /// next candle arrival. Idempotent, and cheap when nothing changes.
    pub fn set_venue_lead_in(&mut self, enabled: bool) {
        if self.venue_lead_in == enabled {
            return;
        }
        self.venue_lead_in = enabled;
        self.refold_history_prefix();
    }

    /// Shorthand for the one caller that only needs the yes/no.
    #[must_use]
    pub fn can_load_older_candles(&self, capabilities: FeedCapabilities) -> bool {
        self.older_candles(capabilities).is_available()
    }

    /// Reach one more [`quantick_feed::TIME_HISTORY_SPAN_MS`] into the past and
    /// prepend what comes back.
    ///
    /// The whole reason a chart opens on a week rather than a quarter: the
    /// deep history is available, it is simply asked for by the trader who
    /// wants it. Each call is the same request the opening one was, with its
    /// right-hand edge moved to just before the oldest candle held — so the
    /// windows never overlap and the merge on the way back in has nothing to
    /// reconcile.
    ///
    /// Reports whether a request actually went out, so a caller can tell "the
    /// venue is fetching" from "there was nothing to ask for".
    pub fn request_older_ohlcv_history(&mut self, capabilities: FeedCapabilities) -> bool {
        let oldest = self.oldest_venue_candle_ms();
        if !self.can_load_older_candles(capabilities) || oldest.is_none() {
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_OLDER_DECLINED",
                tab = self.id,
                pending = self.ohlcv_pending,
                exhausted = self.ohlcv_older_exhausted,
                held = oldest.is_some(),
                "nothing older to ask for"
            );
            return false;
        }
        // Read once above and unwrapped here: `can_load_older_candles` already
        // required it, and a second read that could disagree with the guard is
        // exactly the kind of duplicate this branch is trying not to leave.
        let oldest = oldest.expect("the guard above required a candle held");
        // One millisecond before the oldest bucket start held. The plan's
        // windows are closed at both ends, so anything else would re-fetch the
        // candle already on screen.
        let before_ms = oldest.saturating_sub(1);
        let slice_ms = self
            .progressive_history
            .then_some(quantick_feed::OHLCV_SLICE_SPAN_MS);
        let command = FeedCommand::FetchOhlcv {
            span_ms: quantick_feed::TIME_HISTORY_SPAN_MS,
            slice_ms,
            before_ms: Some(before_ms),
        };
        match self.commands.try_send(command) {
            Ok(()) => {
                self.ohlcv_pending = true;
                self.ohlcv_reaching_back = Some(oldest);
                self.loading.begin(LoadingTask::VenueHistory);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_OLDER_REQUESTED",
                    tab = self.id,
                    symbol = %self.symbol,
                    span_ms = quantick_feed::TIME_HISTORY_SPAN_MS,
                    before_ms,
                    slice_ms = slice_ms.unwrap_or(0),
                    action = "await_prepend",
                    "asked the venue for another span of older candles"
                );
                true
            }
            // Same two cases the opening request distinguishes, and for the
            // same reasons: a busy frame is worth a retry, a dead feed is
            // worth saying out loud.
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!(
                    target: "quantick::app",
                    event_code = "OHLCV_OLDER_BACKPRESSURE",
                    tab = self.id,
                    action = "retry_on_next_click",
                    "older-candle request not queued; channel full"
                );
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_OLDER_CHANNEL_CLOSED",
                    tab = self.id,
                    symbol = %self.symbol,
                    action = "no_history_until_feed_restart",
                    "older-candle request cannot be sent; the feed is gone"
                );
                false
            }
        }
    }

    /// Rebuild every pane's prefix from the base at that pane's interval.
    ///
    /// Free of the venue: a chip click lands here, not on the network. One
    /// base, one fold per time-cutting pane — the flow pane showing time
    /// bars folds exactly as the split's time pane does (audit S1).
    /// Reports whether any prefix actually changed — an installed prefix
    /// rebuilds the indicators, so the caller can skip sending a second one.
    pub fn refold_history_prefix(&mut self) -> bool {
        let Self {
            ohlcv_base,
            flow_pane,
            time_panes,
            venue_lead_in,
            ..
        } = self;
        let venue_lead_in = *venue_lead_in;
        let Some(base) = ohlcv_base.as_ref() else {
            return false;
        };
        let mut changed = false;
        for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
            // A pane not cutting by time has no interval to fold to, and a
            // sub-minute one has no whole number of venue candles in it: both
            // get no prefix, which is the honest answer rather than an
            // invented one.
            let prefix = match pane.state.spec().time_interval_ms() {
                Some(interval) => {
                    let folded = crate::resample::fold(base, interval);
                    trim_to_seam(
                        folded,
                        pane.state.bars().first(),
                        pane.state.partial(),
                        interval,
                    )
                }
                // A pane not cutting by time has no interval to fold *to*, so
                // the base goes in unfolded: the venue's own minutes, in front
                // of bars cut by trades. Real candles, counted apart from
                // built bars on the status bar — a minute never becomes a tick
                // bar, the two sit side by side and each says what it is.
                // Asked for, never assumed: without the switch the honest
                // answer is still no prefix at all.
                None if venue_lead_in => trim_borrowed_to_seam(
                    base,
                    pane.state.bars().first(),
                    pane.state.partial(),
                    quantick_feed::OHLCV_BASE_INTERVAL_MS,
                ),
                None => Vec::new(),
            };
            changed |= pane.install_history_prefix(prefix);
        }
        changed
    }

    /// Reach into the past, as far as [`Self::history_reach`] says.
    ///
    /// The named call behind the `+ older` button, the overflow entry and the
    /// `QUANTICK_LOAD_OLDER` hook — one path, so an operator without a mouse
    /// reaches exactly what a click reaches. Non-blocking: with a reach of one
    /// page this is the single request it always was, and with a longer reach
    /// it is the first of a run each reply continues
    /// ([`Self::settle_history_page`]).
    pub fn request_older_history(&mut self, config: &AppConfig) {
        if self.campaign.is_some() {
            // A run already has its one permitted request out, and the reply
            // is what sends the next. Pressing again would raise a second wait
            // on the same indicator and ask the transport for two pages it
            // will not serve at once.
            tracing::debug!(
                target: "quantick::app",
                event_code = "HISTORY_REACH_ALREADY_RUNNING",
                tab = self.id,
                action = "ignore_press",
                "a reach is already paging; this press changes nothing"
            );
            return;
        }
        // Whatever the last press had to say is spent the moment this one is
        // made: the outcome on screen must be the outcome of the press the
        // trader is waiting on, never the one before it.
        self.history_note = None;
        // Read before the request goes out: the anchor is where the chart
        // reached *before* this run, and everything older arrived because of
        // it. That is what makes a second press fetch the session before the
        // one the first press brought in, rather than finding its work done.
        let anchor_ms = self.oldest_retained_trade_ms();
        if !self.send_load_older() {
            // Nothing was even asked, so nothing will answer. Said on screen
            // rather than only in the log: to the trader this is a press that
            // did nothing, which is the whole bug.
            self.raise_history_note(REQUEST_REFUSED_NOTICE);
            return;
        }
        if self.history_reach.runs_a_campaign() {
            // A chart holding no prints has nothing to page back *from*, so
            // the single request above is the whole of this press: the next
            // one, with a tape under it, starts the run.
            let held = self.flow_pane.state.trades().len();
            // The trader's live choice outranks the config seed: the toolbar
            // and the control plane both write the window's value, and a run
            // started after that must reach what they asked for rather than
            // what the file said at startup.
            let bounds = history_reach::ReachBounds {
                span_ms: i64::from(self.history_reach_span_minutes) * 60_000,
                ..config.history.reach_bounds()
            };
            self.campaign =
                anchor_ms.map(|anchor| Campaign::new(anchor, held, bounds, self.history_reach));
        }
    }

    /// Queue one `load_older` and raise the wait its reply will resolve.
    ///
    /// Returns whether the command went out. A refusal is the end of whatever
    /// asked for it: nothing will answer, so nothing may keep waiting.
    fn send_load_older(&mut self) -> bool {
        match self.commands.try_send(FeedCommand::LoadOlder {
            count: self.history_step.max(1),
        }) {
            Ok(()) => {
                self.loading.begin(LoadingTask::History);
                tracing::info!(
                    target: "quantick::app",
                    count = self.history_step,
                    reach = self.history_reach.token(),
                    "requested older history"
                );
                true
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!(target: "quantick::app", "older-history request already pending");
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(target: "quantick::app", "feed command channel closed");
                false
            }
        }
    }

    /// Slices of the opening session still to arrive, or `None` when the chart
    /// is not being filled in behind.
    ///
    /// Read by the control plane so an operator without a mouse can tell a
    /// chart that is still arriving from one that has everything it is going
    /// to get — the same question the trader answers by watching the bars
    /// grow leftward.
    #[must_use]
    pub const fn opening_slices_remaining(&self) -> Option<u64> {
        self.opening_slices_remaining
    }

    /// The oldest print this tab still holds.
    ///
    /// Read off the flow pane: every pane is fed the same tape and cuts it its
    /// own way, and the flow pane is the one that always exists.
    fn oldest_retained_trade_ms(&self) -> Option<i64> {
        self.flow_pane
            .state
            .trades()
            .first()
            .map(|trade| trade.timestamp_ms)
    }

    /// Decide what a page that just landed means for the reach that asked —
    /// and leave the trader something to read when it means nothing arrived.
    ///
    /// Rate: **rare** — once per history reply. The scan inside
    /// [`Campaign::advance`] stops at the anchor, so its cost is the page that
    /// arrived rather than the whole retained tape.
    pub(super) fn settle_history_page(&mut self, page_len: usize) {
        let Some(mut campaign) = self.campaign.take() else {
            if page_len == 0 {
                self.raise_history_note(self.empty_page_verdict());
            }
            return;
        };
        // Putting the reach back to one page is how a run is called off. It is
        // the only stop a trader has — pressing again mid-run is refused, so
        // the button cannot be a cancel without a double-click becoming one —
        // and it is where they would look, because it is the control that
        // started this.
        if !self.history_reach.runs_a_campaign() {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_REACH_SETTLED",
                tab = self.id,
                symbol = %self.symbol,
                pages = campaign.pages_spent(),
                anchor_ms = campaign.anchor_ms(),
                reached_ms = self.oldest_retained_trade_ms().unwrap_or(0),
                action = "reach_withdrawn",
                "the reach was put back to one page; the run stops here"
            );
            return;
        }
        // The feed's own answer, not the configured one: `history_paging` goes
        // false the moment a venue reports its record exhausted, and asking
        // again after that spins a run against a wall.
        let can_page = self.feed_capabilities.borrow().history_paging;
        match campaign.advance(self.flow_pane.state.trades(), can_page) {
            CampaignStep::Ask => {
                if self.send_load_older() {
                    self.campaign = Some(campaign);
                } else {
                    // Nothing will answer, so the run is over. Said out loud
                    // rather than retried in silence: a closed channel is a
                    // feed that is gone, and a full one is a frame so busy
                    // that pressing again is the honest recovery.
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "HISTORY_REACH_STALLED",
                        tab = self.id,
                        symbol = %self.symbol,
                        pages = campaign.pages_spent(),
                        action = "stop_and_wait_for_another_press",
                        "a load-older reach could not queue its next page"
                    );
                    self.raise_history_note(REQUEST_REFUSED_NOTICE);
                }
            }
            CampaignStep::Stop(end) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_REACH_SETTLED",
                    tab = self.id,
                    symbol = %self.symbol,
                    pages = campaign.pages_spent(),
                    anchor_ms = campaign.anchor_ms(),
                    reached_ms = self.oldest_retained_trade_ms().unwrap_or(0),
                    action = end.action(),
                    "a load-older reach finished"
                );
                // The same verdict the log just took, in the trader's words.
                // `ReachMet` has none: yesterday is on the chart, and the
                // chart says it better than a sentence about the chart.
                if let Some(notice) = end.notice() {
                    self.raise_history_note(notice);
                }
            }
        }
    }

    /// Why one page came back empty, in the words the run would have used.
    ///
    /// No campaign was behind this reply — the one-page reach, or a longer one
    /// that had no tape to page back from — so nothing else is going to judge
    /// it. It reads the same two facts [`Campaign::advance`] reads first, in
    /// the same order, so the two paths cannot end up telling the trader
    /// different stories about one empty block:
    ///
    /// - the feed having withdrawn paging is [`CampaignEnd::Exhausted`], not a
    ///   request that happened to come back empty. Saying otherwise would
    ///   leave a note about this request beside a button that has just greyed
    ///   itself out, and the trader reading two accounts of one press.
    /// - a chart with nothing on it is [`CampaignEnd::NothingCharted`]. The
    ///   venue is not at fault for a press that had no anchor to reach back
    ///   from, and blaming it there is the data-honesty rule broken in the
    ///   trader's own words.
    ///
    /// Only what is left over is this reply's own: one empty answer, which is
    /// not evidence that a record is spent.
    fn empty_page_verdict(&self) -> &'static str {
        if !self.feed_capabilities.borrow().history_paging {
            return CampaignEnd::Exhausted.notice().unwrap_or(EMPTY_PAGE_NOTICE);
        }
        if self.flow_pane.state.trades().is_empty() {
            return CampaignEnd::NothingCharted
                .notice()
                .unwrap_or(EMPTY_PAGE_NOTICE);
        }
        EMPTY_PAGE_NOTICE
    }

    /// Drop the run this tab was making and the verdict it produced.
    ///
    /// One call, because the two always travel together and always mean the
    /// same thing: the run and everything it had to say belong to a tape this
    /// tab no longer shows. Split across the three reset paths they were two
    /// fields a fourth path could half-remember, and the half it forgot would
    /// hang a sentence about the previous symbol's press over the new one's
    /// chart.
    pub(super) fn abandon_history_run(&mut self) {
        self.campaign = None;
        self.history_note = None;
    }

    /// Put one sentence about the last press where the trader is looking.
    ///
    /// `pub(crate)` for the `QUANTICK_HISTORY_NOTE` hook, which photographs
    /// this surface by raising a real ending's real sentence through this very
    /// call — the state is otherwise reachable only by pressing the button
    /// against a venue that happens to be refusing.
    pub(crate) fn raise_history_note(&mut self, text: &'static str) {
        self.history_note = Some(HistoryNote {
            text,
            raised_at: std::time::Instant::now(),
        });
    }

    /// What the last *load older* press had to say, while it is still on
    /// screen. `None` is the ordinary state: no press yet, a press that landed
    /// what it promised, or one whose remark has had its time.
    #[must_use]
    pub fn history_note(&self) -> Option<&'static str> {
        self.history_note.map(|note| note.text)
    }

    /// Drop the note once it has had its [`HISTORY_NOTE_LINGER`].
    ///
    /// Rate: **per-frame**, and deliberately trivial — a `Copy` `Option` and
    /// one duration comparison, both skipped entirely while there is no note.
    /// Told the time rather than reading a clock, the way `replay` is, so a
    /// test can walk past the linger without sleeping through it.
    pub fn expire_history_note(&mut self, now: std::time::Instant) {
        if self.history_note.is_some_and(|note| {
            now.saturating_duration_since(note.raised_at) >= HISTORY_NOTE_LINGER
        }) {
            self.history_note = None;
        }
    }

    /// Whether a run of *load older* requests is in flight.
    ///
    /// Read by the toolbar, so the button can say what it is doing rather than
    /// look idle while pages land behind it.
    #[must_use]
    pub const fn history_reach_running(&self) -> bool {
        self.campaign.is_some()
    }
}
