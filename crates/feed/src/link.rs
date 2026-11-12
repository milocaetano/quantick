//! One consumer's live link to a running feed: the channels it drains, what
//! the transport last said, and the readings taken off the tape as it arrives.
//!
//! The desktop tab holds one and replaces its channels on every attach — a
//! reconnect, a reload, a market switch, a replay opened or closed. Everything
//! here is plain state over the [`FeedHandle`] it was built from; the wall
//! clock is always handed in, so the judgements below stay deterministic.

use tokio::sync::{mpsc, watch};

use crate::config::{FeedCapabilities, ProviderKind};
use crate::stall::{self, Stall, StallInput};
use crate::{
    DepthEvent, FeedCommand, FeedConnectionState, FeedEvent, FeedGap, FeedHandle, FeedIntegrity,
    FeedLatency, FeedNotice, ReplayLink,
};

/// The feed half of a tab: its session's channels, notices and transport
/// state, and the newest live trade it has seen.
pub struct FeedLink {
    pub events: mpsc::Receiver<FeedEvent>,
    pub book_events: mpsc::Receiver<DepthEvent>,
    /// Connection trouble the feed wants the user to know about.
    pub notices: mpsc::Receiver<FeedNotice>,
    /// The newest notice, held until the feed says it is over. A feed that
    /// blocks once and then goes quiet has to keep saying so — the chart it
    /// left empty will not.
    pub notice: FeedNotice,
    /// Whether this tab, opened from a saved workspace, rolls itself off a
    /// contract [`Self::notice`] says the venue no longer lists. Kept across
    /// attaches: the roll it makes is one.
    pub auto_roll: quantick_sources::recovery::AutoRoll,
    /// Wall clock when the running feed session was attached — the floor for
    /// judging a first connection that never lands.
    pub feed_attached_ms: i64,
    /// Wall clock when [`Self::feed_connection`] last changed.
    ///
    /// The reconnect budget is measured from here rather than from the notice,
    /// because a supervisor that alternates two lines — `Lost`, then `Waiting`,
    /// then `Lost` — changes the notice every few seconds while the transport
    /// stays exactly as broken as it was. Anchored on the notice, that pair
    /// re-stamped the clock forever and the budget never ran out, which is the
    /// failure this whole module exists to end.
    pub connection_since_ms: i64,
    /// After a reconnect that kept the timeline, the market time the chart had
    /// already reached.
    ///
    /// Every print at or before it belongs to the window the new session
    /// replays and is dropped rather than counted twice; the first print past
    /// it clears the floor and decides whether a gap has to be marked. `None`
    /// on a session that started from an empty chart, which has nothing to
    /// overlap with. See [`crate::past_resume_floor`].
    pub resume_floor_ms: Option<i64>,
    /// How many feed sessions this link has taken over since it opened: one
    /// more on every attach. Read by `feed.status` so a client that lost the
    /// answer to `feed.reconnect` or `feed.reload` can see whether the tab
    /// really took a new session, which no other field says.
    feed_generation: u64,
    /// A stall forced by `QUANTICK_FEED_STALL`, for a scripted run that has to
    /// photograph the recovery controls without breaking a real feed.
    ///
    /// Overrides the judgement when set, and set only from the hook — a live
    /// session leaves it `None` and reads the real one, exactly as
    /// [`Self::forced_latency`] does.
    pub forced_stall: Option<stall::ForcedStall>,
    /// A silence asked for by `QUANTICK_FEED_GAP`, still waiting for bars to
    /// land it between. Taken once the chart has some, and cleared, so the
    /// hook marks one gap rather than one per frame.
    pub pending_demo_gap_ms: Option<i64>,
    /// Silences in this tape that no print covers, in market time, oldest
    /// first and bounded by [`crate::MAX_REMEMBERED_GAPS`].
    ///
    /// Written by reconnects that keep the timeline and source-sequence gaps.
    /// A reload clears the timeline and its diagnostics together.
    pub feed_gaps: Vec<FeedGap>,
    /// Cumulative source-message integrity; gaps alone are a bounded view.
    pub feed_integrity: FeedIntegrity,
    /// State reported by the live trade transport, independent from how often
    /// that market prints and from the last observed arrival latency.
    pub feed_connection: FeedConnectionState,
    /// What the running feed can really do, read fresh every frame. The feed
    /// narrows it once a session tells it what the symbol actually offers.
    pub feed_capabilities: watch::Receiver<FeedCapabilities>,
    /// Where this feed's delay is being spent, read fresh every frame.
    ///
    /// A reading rather than an event, so a frame that skipped three samples
    /// sees the newest one instead of a queue. `None` on a provider that
    /// cannot cut its own chain, and until the first sample arrives.
    pub feed_latency: watch::Receiver<Option<FeedLatency>>,
    /// A latency split forced by `QUANTICK_FAKE_LATENCY_SPLIT`, for a scripted
    /// run that has to photograph the readout without a slow venue.
    ///
    /// Overrides the feed's own reading when set, and set only from the hook —
    /// a live session leaves it `None` and reads the real one.
    pub forced_latency: Option<FeedLatency>,
    pub commands: mpsc::Sender<FeedCommand>,
    /// `Some` exactly while a recorded session is this link's source; it is
    /// the one flag the rest of the UI checks, so replay never grows a second
    /// copy of "which mode are we in". Per tab: a recording plays in the tab
    /// that opened it while the others keep streaming, on their own feed
    /// threads and their own clocks.
    pub replay: Option<ReplayLink>,
    pub book_capture_epoch: u64,
    pub book_channel_closed_reported: bool,
    /// Exchange-to-UI delay measured when the newest live trade arrived.
    /// Stable while the tape is quiet: market inactivity is not transport lag.
    pub latest_trade_latency_ms: Option<i64>,
    /// Timestamp of the newest live trade (epoch ms), for the tape-age
    /// readout. The latency above is an observation frozen at arrival; this
    /// is what wall clock is compared against every frame.
    pub latest_trade_ms: Option<i64>,
    pub live_trades: u64,
}

impl FeedLink {
    /// A link on the freshly spawned `feed`, attached at `now_ms`.
    #[must_use]
    pub fn new(feed: FeedHandle, now_ms: i64) -> Self {
        Self {
            events: feed.events,
            book_events: feed.book_events,
            notices: feed.notices,
            notice: FeedNotice::Clear,
            feed_attached_ms: now_ms,
            connection_since_ms: now_ms,
            resume_floor_ms: None,
            feed_generation: 0,
            forced_stall: stall::ForcedStall::from_env(),
            pending_demo_gap_ms: crate::demo_gap_ms(),
            feed_gaps: Vec::new(),
            feed_integrity: FeedIntegrity::default(),
            feed_connection: FeedConnectionState::Connecting,
            feed_capabilities: feed.capabilities,
            feed_latency: feed.latency,
            forced_latency: crate::forced_latency_split(),
            commands: feed.commands,
            replay: feed.replay,
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            latest_trade_latency_ms: None,
            latest_trade_ms: None,
            live_trades: 0,
            auto_roll: quantick_sources::recovery::AutoRoll::default(),
        }
    }

    /// Take over a freshly spawned feed at `now_ms`: channels, capabilities,
    /// commands and replay link in one move.
    ///
    /// The old feed's trouble is not the new feed's, so the notice and the
    /// transport state start clean — switching away from a blocked source must
    /// not leave its instruction on screen.
    pub fn attach(&mut self, handle: FeedHandle, now_ms: i64) {
        self.feed_generation = self.feed_generation.saturating_add(1);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        self.feed_latency = handle.latency;
        self.notice = FeedNotice::Clear;
        self.feed_attached_ms = now_ms;
        self.connection_since_ms = self.feed_attached_ms;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;
    }

    /// Forget what this link recorded about the timeline it fed — its gaps,
    /// its integrity count and its resume floor — when that timeline is
    /// thrown away by a market switch or a reload.
    pub fn forget_timeline(&mut self) {
        self.feed_gaps.clear();
        self.feed_integrity = FeedIntegrity::default();
        self.resume_floor_ms = None;
    }

    /// How many feed sessions this link has taken over since it opened.
    #[must_use]
    pub fn feed_generation(&self) -> u64 {
        self.feed_generation
    }

    /// Drain the notice channel as of `now_ms`, keeping only the newest.
    ///
    /// Level-triggered rather than queued: only the latest state matters, and
    /// a burst of bridge output must not queue up cards to show one by one.
    /// A closed channel (a feed with nothing to report) simply yields nothing.
    pub fn drain_notices_at(&mut self, now_ms: i64) {
        while let Ok(notice) = self.notices.try_recv() {
            let next = match notice {
                FeedNotice::Connected => {
                    // A contract that streams is listed: rolls wait for a hand.
                    self.auto_roll.disarm();
                    self.set_connection(FeedConnectionState::Connected, now_ms);
                    FeedNotice::Clear
                }
                FeedNotice::Reconnecting { .. } => {
                    self.set_connection(FeedConnectionState::Reconnecting, now_ms);
                    notice
                }
                FeedNotice::Working { .. } | FeedNotice::Attention { .. } => notice,
                FeedNotice::Clear => FeedNotice::Clear,
            };
            self.notice = next;
        }
    }

    /// Move the transport to `state`, stamping when it got there.
    ///
    /// Only a real transition stamps: a provider that reports the same state
    /// twice has not changed anything, and treating it as a change would reset
    /// the budget measured from it.
    pub fn set_connection(&mut self, state: FeedConnectionState, now_ms: i64) {
        if self.feed_connection != state {
            self.feed_connection = state;
            self.connection_since_ms = now_ms;
        }
    }

    /// This link's judgement about a feed that has stopped delivering, or
    /// `None` while it is merely slow.
    ///
    /// Deterministic: the caller supplies wall clock and the tape's age, as
    /// [`Self::tape_age_at`] reads it. `transport` says whether the running
    /// source has a transport to recover at all. The decision itself lives in
    /// [`crate::stall`] and touches no clock at all.
    #[must_use]
    pub fn stall_at(
        &self,
        provider: ProviderKind,
        provider_name: &str,
        transport: bool,
        tape_age_ms: Option<i64>,
        now_ms: i64,
    ) -> Option<Stall> {
        if let Some(forced) = self.forced_stall {
            return Some(forced.stall(provider, provider_name));
        }
        stall::assess(
            &StallInput {
                notice: &self.notice,
                connection_since_ms: self.connection_since_ms,
                connection: self.feed_connection,
                tape_age_ms,
                attached_ms: self.feed_attached_ms,
                provider,
                provider_name,
                replaying: self.replay.is_some() || !transport,
            },
            now_ms,
        )
    }

    /// Delay observed when the newest live trade reached the UI.
    ///
    /// `None` while a session is replaying: those prints are as old as the day
    /// they were recorded, so their original arrival latency is unavailable.
    ///
    /// This figure freezes between prints — it is an observation, not a
    /// measurement of now. [`Self::tape_age_at`] is the one that ages.
    #[must_use]
    pub fn trade_arrival_ms(&self) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        // Never the forced split's figure. This is a *measurement* — it reaches
        // the log as APP_HIGH_TRADE_LAG, the control feed scope and the health
        // view, none of which carry a marker saying a capture run invented it,
        // and inferred data that is not labelled as such is the one thing this
        // repo does not ship. The hook drives the readout through the latency
        // port instead, where every consumer already knows the figure is the
        // feed's own and not the chart's.
        self.latest_trade_latency_ms
    }

    /// How old the newest event on the tape is, as of `now_ms`.
    ///
    /// Takes the newer of the trade stream and `book_last_event_ms`, the
    /// book's newest event, so a symbol with depth but a thin tape is not
    /// called stale while its book is live. `None` while replaying, and before
    /// anything has arrived — nothing to be stale about yet.
    #[must_use]
    pub fn tape_age_at(&self, book_last_event_ms: Option<i64>, now_ms: i64) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        let newest = match (self.latest_trade_ms, book_last_event_ms) {
            (Some(trade), Some(book)) => Some(trade.max(book)),
            (trade, book) => trade.or(book),
        }?;
        Some(now_ms.saturating_sub(newest).max(0))
    }

    /// How a recorded session says its sides were decided, while one is
    /// playing: the data-honesty label the status bar shows for a replay.
    #[must_use]
    pub fn replay_side_note(&self) -> Option<String> {
        let link = self.replay.as_ref()?;
        Some(match link.session.header.side_source.as_deref() {
            Some(source) => format!("side: {source}"),
            None => "side: not recorded".to_owned(),
        })
    }
}
