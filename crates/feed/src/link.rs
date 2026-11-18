//! One consumer's live link to a running feed: the channels it drains, what
//! the transport last said, and the readings taken off the tape as it arrives.
//!
//! The desktop tab holds one and replaces its channels on every attach — a
//! reconnect, a reload, a market switch, a replay opened or closed. Everything
//! here is plain state over the [`FeedHandle`] it was built from; the wall
//! clock is always handed in, so the judgements below stay deterministic.

use quantick_sources::recovery::AutoRoll;
use tokio::sync::{mpsc, watch};

use crate::config::{FeedCapabilities, ProviderKind};
use crate::notice_report::{NoticeAction, RollNote};
use crate::stall::{self, Stall, StallInput};
use crate::sync::{self as feed_sync, Desync, LinkState, SyncInput, SyncLimits, SyncWatch};
use crate::{
    DepthEvent, FeedCommand, FeedConnectionState, FeedEvent, FeedGap, FeedHandle, FeedIntegrity,
    FeedLatency, FeedNotice, ReplayLink,
};

/// Why a tab shows another contract than it was opened on: it rolled off one
/// the venue no longer lists onto the one its feed config names next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rolled {
    /// The contract the venue does not list.
    pub from: String,
    /// The contract the tab shows now.
    pub to: String,
}

impl Rolled {
    /// What the trader reads beside the status word.
    #[must_use]
    pub fn sentence(&self) -> String {
        quantick_sources::recovery::rolled_sentence(&self.from, &self.to)
    }
}

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
    /// The roll the attach that started this session explains, standing
    /// beside whatever [`Self::notice`] says. Set by the attach that follows
    /// a roll and replaced by the next attach — a switch, a reconnect, a
    /// reload — and by nothing the feed says: progress, a connect, a
    /// reconnect or an attention for the new contract describe the present
    /// and leave the roll that led there true.
    pub rolled: Option<Rolled>,
    /// Whether this tab, opened from a saved workspace, rolls itself off a
    /// contract [`Self::notice`] says the venue no longer lists, and the roll
    /// on offer. Kept across attaches: the roll it makes is one.
    pub auto_roll: AutoRoll,
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
    /// Drain-receipt-to-paper-trading delay of the newest live prints, never
    /// a recording's; reset by [`Self::attach`], so a new feed session starts
    /// its own window.
    pub intake_lag: quantick_operability::intake_lag::IntakeLag,
    /// A session of this link has connected at least once.
    connected_once: bool,
    /// When new entries paused on this link, kept across attaches.
    sync_watch: SyncWatch,
    /// Wall clock when the newest live print or book event of this session
    /// arrived here: the entry pause ages the tape from it, never from a
    /// venue stamp a clock offset would skew.
    latest_arrival_ms: Option<i64>,
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
            rolled: None,
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
            intake_lag: Default::default(),
            auto_roll: AutoRoll::default(),
            connected_once: false,
            sync_watch: SyncWatch::default(),
            latest_arrival_ms: None,
        }
    }

    /// Take over a freshly spawned feed at `now_ms`: channels, capabilities,
    /// commands and replay link in one move.
    ///
    /// The old feed's trouble is not the new feed's, so the notice and the
    /// transport state start clean — switching away from a blocked source must
    /// not leave its instruction on screen. A roll off an unlisted contract
    /// leaves its explanation in [`Self::rolled`], and any other attach
    /// takes it away.
    pub fn attach(&mut self, handle: FeedHandle, now_ms: i64) {
        self.feed_generation = self.feed_generation.saturating_add(1);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        self.feed_latency = handle.latency;
        self.notice = FeedNotice::Clear;
        self.rolled = self
            .auto_roll
            .take_rolled()
            .map(|(from, to)| Rolled { from, to });
        self.feed_attached_ms = now_ms;
        self.connection_since_ms = self.feed_attached_ms;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;
        self.intake_lag.clear();
        self.latest_arrival_ms = None;
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
    ///
    /// A contract the venue does not list stays named through the retries
    /// until a session connects or another attention replaces it: an
    /// attention names its own contract or none, so the roll off it never
    /// outlives the report it answers. A roll's explanation is not a notice:
    /// it stands in [`Self::rolled`], beside every notice this drains.
    pub fn drain_notices_at(&mut self, now_ms: i64) {
        while let Ok(notice) = self.notices.try_recv() {
            let held = matches!(
                self.notice,
                FeedNotice::Attention {
                    unlisted: Some(_),
                    ..
                }
            );
            let next = match notice {
                FeedNotice::Connected => {
                    // A contract that streams is listed: rolls wait for a hand.
                    self.auto_roll.disarm();
                    self.set_connection(FeedConnectionState::Connected, now_ms);
                    FeedNotice::Clear
                }
                FeedNotice::Reconnecting { .. } => {
                    self.set_connection(FeedConnectionState::Reconnecting, now_ms);
                    if held {
                        continue;
                    }
                    notice
                }
                FeedNotice::Working { .. } | FeedNotice::Clear if held => continue,
                FeedNotice::Clear | FeedNotice::Working { .. } | FeedNotice::Attention { .. } => {
                    notice
                }
            };
            self.notice = next;
        }
    }

    /// A saved workspace opened this tab on `symbol`: it may roll itself off
    /// it — unless its feed has already connected or streamed here, which
    /// proves the contract listed, as when an import adopts a live tab.
    pub fn arm_auto_roll(&mut self, symbol: &str) {
        let streamed = self.connected_once
            || self.feed_connection == FeedConnectionState::Connected
            || self.live_trades > 0;
        if !streamed {
            self.auto_roll.arm(symbol);
        }
    }

    /// The roll off the unlisted contract the notice names, as the last settle
    /// found it: the configured next contract, or why it cannot roll now.
    /// Every surface that offers the roll, and the control plane, read this.
    #[must_use]
    pub fn roll_offer(&self) -> Option<Result<&str, &'static str>> {
        let FeedNotice::Attention {
            unlisted: Some(from),
            ..
        } = &self.notice
        else {
            return None;
        };
        let offer = self.auto_roll.offer().filter(|offer| &offer.from == from)?;
        Some(offer.to.as_deref().map_err(|reason| *reason))
    }

    /// [`Self::roll_offer`] as a report carries it.
    #[must_use]
    pub fn roll_note(&self) -> Option<RollNote<'_>> {
        self.roll_offer().map(RollNote::from)
    }

    /// The one-click control beside the word and on the empty chart: the roll
    /// on offer, else Reconnect — what the popup offers first.
    #[must_use]
    pub fn quick_action(&self) -> NoticeAction {
        match self.roll_offer() {
            Some(Ok(to)) => NoticeAction::Roll(to.to_owned()),
            _ => NoticeAction::Reconnect,
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
        self.connected_once |= state == FeedConnectionState::Connected;
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

    /// Whether new entries pause on this link as of `now_ms`: the judgement
    /// is [`crate::sync`]'s; this gathers the link's own readings and logs
    /// each edge of the pause. `symbol` names the tab in the log; `offline`
    /// is the chart's latched stall. The transport is attempting while it
    /// reconnects, or connects with nothing yet arrived this session.
    pub fn sync_at(
        &mut self,
        symbol: &str,
        offline: bool,
        limits: SyncLimits,
        now_ms: i64,
    ) -> Option<Desync> {
        let link = match self.feed_connection {
            _ if offline => LinkState::Offline,
            FeedConnectionState::Reconnecting => LinkState::Attempting,
            FeedConnectionState::Connecting if self.latest_arrival_ms.is_none() => {
                LinkState::Attempting
            }
            _ => LinkState::Up,
        };
        let transport = self.feed_capabilities.borrow().transport;
        let desync = feed_sync::assess(&SyncInput {
            replaying: self.replay.is_some() || !transport,
            link,
            link_for_ms: now_ms.saturating_sub(self.connection_since_ms),
            latency: self.latency_split(),
            // Before the session's first arrival, silence counts from when
            // the transport last changed state: a feed that connects and
            // never streams pauses, as one that stops streaming does.
            tape_age_ms: Some(
                now_ms
                    .saturating_sub(self.latest_arrival_ms.unwrap_or(self.connection_since_ms))
                    .max(0),
            ),
            notice: &self.notice,
            limits,
        });
        if let Some(change) = self.sync_watch.step(desync.is_some(), now_ms) {
            let intake = self.intake_lag.summary();
            feed_sync::log_change(change, desync.as_ref(), symbol, intake);
        }
        desync
    }

    /// A book event drained at `received_at_ms`: the tape is still arriving.
    pub fn observe_book_arrival(&mut self, received_at_ms: i64) {
        self.note_arrival(received_at_ms);
    }

    fn note_arrival(&mut self, received_at_ms: i64) {
        if self.replay.is_none() {
            let newest = self
                .latest_arrival_ms
                .map_or(received_at_ms, |at| at.max(received_at_ms));
            self.latest_arrival_ms = Some(newest);
        }
    }

    /// Book one live print received at `received_at_ms`, as the window hands
    /// it to its owners: the arrival delay, the tape-age stamp, the count and,
    /// off a recording, one intake-lag sample read now — the moment paper
    /// trading, the first live-trade stage, runs.
    pub fn observe_live_trade(&mut self, received_at_ms: i64, trade_ms: i64) {
        self.observe_live_trade_at(received_at_ms, trade_ms, crate::clock::wall_clock_ms());
    }

    fn observe_live_trade_at(&mut self, received_at_ms: i64, trade_ms: i64, now_ms: i64) {
        self.latest_trade_latency_ms = quantick_engine::feed_lag_ms(received_at_ms, Some(trade_ms));
        self.latest_trade_ms = Some(trade_ms);
        self.live_trades += 1;
        self.note_arrival(received_at_ms);
        // A recording's drain is not live intake; its sibling figures say so too.
        if self.replay.is_none() {
            self.intake_lag.record(received_at_ms, now_ms);
        }
    }

    /// Where this feed's delay is being spent: the hook's forced split, else
    /// the provider's own. `None` while replaying — a recording's prints are
    /// as old as the day they were captured — and on a provider that cannot
    /// cut its chain.
    #[must_use]
    pub fn latency_split(&self) -> Option<FeedLatency> {
        if self.replay.is_some() {
            return None;
        }
        self.forced_latency.or(*self.feed_latency.borrow())
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

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_sources::recovery::RollOffer;

    /// A link over channels the test speaks on.
    fn link() -> (FeedLink, mpsc::Sender<FeedNotice>) {
        let (handle, notices) = handle();
        (FeedLink::new(handle, 0), notices)
    }

    #[test]
    fn a_live_print_books_its_delay_age_count_and_intake_sample() {
        let (mut link, _notices) = link();
        link.observe_live_trade_at(1_000, 600, 1_003);
        link.observe_live_trade_at(1_000, 1_200, 1_009);
        assert_eq!(
            link.latest_trade_latency_ms,
            Some(-200),
            "signed, as measured"
        );
        assert_eq!((link.latest_trade_ms, link.live_trades), (Some(1_200), 2));
        let lag = link.intake_lag.summary().expect("two samples");
        assert_eq!((lag.samples, lag.p50_ms, lag.max_ms), (2, 3, 9));
        link.attach(handle().0, 2_000);
        assert_eq!(
            link.intake_lag.summary(),
            None,
            "a new session, a new window"
        );
    }

    /// The pause ages the tape from when prints and book events arrived
    /// here: a venue clock hours off moves nothing, a silent wire does.
    #[test]
    fn the_pause_ages_the_tape_by_local_arrival_not_the_venue_stamp() {
        use crate::sync::{SyncLimits, SyncReading};
        let (mut link, _notices) = link();
        streaming(&mut link);
        link.set_connection(FeedConnectionState::Connected, 999_000);
        let limits = SyncLimits::default();
        let at = |link: &mut FeedLink, now| link.sync_at("WINZ26", false, limits, now);
        assert_eq!(at(&mut link, 1_000_000), None, "connected a second ago");
        // A venue stamp three hours behind the local clock.
        link.observe_live_trade_at(1_000_000, 1_000_000 - 10_800_000, 1_000_000);
        assert_eq!(at(&mut link, 1_004_999), None, "an offset clock is not age");
        link.observe_book_arrival(1_004_000);
        assert_eq!(at(&mut link, 1_008_999), None, "the book still arrives");
        let paused = at(&mut link, 1_009_000).expect("five seconds of silence");
        assert_eq!(paused.reading, SyncReading::TapeAge);
        assert_eq!(paused.tape_age_ms, Some(5_000));
    }

    fn streaming(link: &mut FeedLink) {
        link.feed_capabilities = crate::fixed_capabilities(FeedCapabilities {
            transport: true,
            ..FeedCapabilities::none()
        });
    }

    /// A new session starts its own silence: the old session's arrivals do
    /// not keep it fresh, and a connected feed that never streams pauses once
    /// the tape-age limit has run from when it connected.
    #[test]
    fn a_connected_feed_that_never_streams_pauses_after_the_tape_age_limit() {
        use crate::sync::{SyncLimits, SyncReading};
        let limits = SyncLimits::default();
        let (mut link, _notices) = link();
        streaming(&mut link);
        link.observe_book_arrival(1_000);
        link.attach(handle().0, 2_000);
        streaming(&mut link);
        link.set_connection(FeedConnectionState::Connected, 3_000);
        let at = |link: &mut FeedLink, now| link.sync_at("WINZ26", false, limits, now);
        assert_eq!(
            at(&mut link, 7_999),
            None,
            "the limit runs from the connect"
        );
        let paused = at(&mut link, 8_000).expect("connected and silent");
        assert_eq!(
            (paused.reading, paused.tape_age_ms),
            (SyncReading::TapeAge, Some(5_000))
        );
        link.observe_book_arrival(8_500);
        assert_eq!(at(&mut link, 9_000), None, "the first arrival ends it");
    }

    /// The link's word for the pause: the chart's offline latch, a
    /// reconnect, or a connect with nothing arrived yet; a feed that streams
    /// without saying it connected is up.
    #[test]
    fn the_pause_reads_the_transport_from_the_feed_and_the_charts_latch() {
        use crate::sync::{LinkState, SyncLimits};
        let limits = SyncLimits::default();
        let (mut link, _notices) = link();
        streaming(&mut link);
        let word = |link: &mut FeedLink, offline| {
            (link.sync_at("WINZ26", offline, limits, 10)).map(|paused| paused.link)
        };
        assert_eq!(word(&mut link, false), Some(LinkState::Attempting));
        link.observe_book_arrival(5);
        assert_eq!(word(&mut link, false), None, "it streams");
        assert_eq!(word(&mut link, true), Some(LinkState::Offline));
        link.set_connection(FeedConnectionState::Reconnecting, 8);
        assert_eq!(word(&mut link, false), Some(LinkState::Attempting));
    }

    fn handle() -> (FeedHandle, mpsc::Sender<FeedNotice>) {
        let (notices, notice_rx) = mpsc::channel(16);
        let handle = FeedHandle {
            events: mpsc::channel(1).1,
            book_events: mpsc::channel(1).1,
            notices: notice_rx,
            capabilities: crate::fixed_capabilities(FeedCapabilities::none()),
            latency: crate::unsplit_latency(),
            commands: mpsc::channel(1).0,
            replay: None,
        };
        (handle, notices)
    }

    fn unlisted(symbol: &str) -> FeedNotice {
        FeedNotice::attention(format!("MetaTrader does not list {symbol}"), "Add it.")
            .with_unlisted(Some(symbol.to_owned()))
    }

    fn say(link: &mut FeedLink, notices: &mpsc::Sender<FeedNotice>, notice: FeedNotice) {
        notices.try_send(notice).expect("room on the channel");
        link.drain_notices_at(1);
    }

    #[test]
    fn an_unlisted_contract_stays_named_through_retries_and_the_give_up() {
        let (mut link, notices) = link();
        say(&mut link, &notices, unlisted("WINV26"));
        link.auto_roll
            .set_offer(Some(offer("WINV26", Ok("WINZ26"))));
        assert_eq!(link.quick_action(), NoticeAction::Roll("WINZ26".to_owned()));

        say(
            &mut link,
            &notices,
            FeedNotice::working("starting the MetaTrader bridge"),
        );
        say(
            &mut link,
            &notices,
            FeedNotice::reconnecting("MetaTrader 5 disconnected"),
        );
        say(&mut link, &notices, FeedNotice::Clear);
        assert_eq!(link.notice, unlisted("WINV26"), "a retry says nothing new");
        assert_eq!(link.feed_connection, FeedConnectionState::Reconnecting);

        // The bridge's give-up names the contract its last attempt reported.
        say(
            &mut link,
            &notices,
            FeedNotice::attention("the MetaTrader bridge would not stay running", "Fix it.")
                .with_unlisted(Some("WINV26".to_owned())),
        );
        assert_eq!(link.notice.unlisted_symbol(), Some("WINV26"));
        assert_eq!(link.roll_offer(), Some(Ok("WINZ26")));
        assert_eq!(link.roll_note(), Some(RollNote::To("WINZ26")));

        say(&mut link, &notices, FeedNotice::Connected);
        assert_eq!(
            link.notice,
            FeedNotice::Clear,
            "a session that connects ends it"
        );
        assert_eq!(link.roll_offer(), None);
        assert_eq!(link.quick_action(), NoticeAction::Reconnect);
    }

    fn offer(from: &str, to: Result<&str, &'static str>) -> RollOffer {
        RollOffer {
            from: from.to_owned(),
            to: to.map(str::to_owned),
        }
    }

    /// SELECT_FAILED proves the contract is listed: it takes the unlisted
    /// contract, its roll and the licence's target away, whatever clears later.
    #[test]
    fn any_other_attention_drops_the_unlisted_contract_and_its_roll() {
        let (mut link, notices) = link();
        link.arm_auto_roll("WINV26");
        say(&mut link, &notices, unlisted("WINV26"));
        link.auto_roll.set_offer(Some(offer(
            "WINV26",
            Err("Finish the drawing gesture first"),
        )));
        assert_eq!(link.notice.unlisted_symbol(), Some("WINV26"));
        for other in [
            FeedNotice::attention("MetaTrader would not open WINV26", "Retry."),
            FeedNotice::attention("MetaTrader 5 is not running", "Start it."),
        ] {
            say(&mut link, &notices, unlisted("WINV26"));
            say(&mut link, &notices, other.clone());
            assert_eq!(link.notice, other);
            // The refusal clears: the next settle finds the roll again.
            link.auto_roll
                .set_offer(Some(offer("WINV26", Ok("WINZ26"))));
            assert_eq!(
                link.notice.unlisted_symbol(),
                None,
                "no contract to roll from"
            );
            assert_eq!(link.roll_offer(), None, "no roll_to");
            assert_eq!(link.quick_action(), NoticeAction::Reconnect);
            say(&mut link, &notices, FeedNotice::Clear);
            assert_eq!(link.notice, FeedNotice::Clear, "nothing holds it any more");
        }
    }

    /// An import that adopts a tab whose feed already connected or streamed
    /// does not license it to roll itself; a fresh restored tab is licensed.
    #[test]
    fn a_link_that_streamed_is_not_armed() {
        let (mut fresh, _notices) = link();
        fresh.arm_auto_roll("WINV26");
        assert!(fresh.auto_roll.armed_for("WINV26"));

        let (mut live, notices) = link();
        say(&mut live, &notices, FeedNotice::Connected);
        say(
            &mut live,
            &notices,
            FeedNotice::reconnecting("MetaTrader 5 disconnected"),
        );
        live.arm_auto_roll("WINV26");
        assert!(!live.auto_roll.is_armed(), "it connected once");

        let (mut streaming, _notices) = link();
        streaming.live_trades = 1;
        streaming.arm_auto_roll("WINV26");
        assert!(!streaming.auto_roll.is_armed(), "it streamed");
    }

    #[test]
    fn an_offer_about_another_contract_is_not_offered() {
        let (mut link, notices) = link();
        say(&mut link, &notices, unlisted("WINV26"));
        link.auto_roll
            .set_offer(Some(offer("WINQ26", Ok("WINV26"))));
        assert_eq!(link.roll_offer(), None);
        link.auto_roll.set_offer(Some(offer(
            "WINV26",
            Err("Finish the drawing gesture first"),
        )));
        assert_eq!(
            link.roll_note(),
            Some(RollNote::Refused("Finish the drawing gesture first"))
        );
        assert_eq!(link.quick_action(), NoticeAction::Reconnect);
    }

    fn rolled_note() -> Option<Rolled> {
        Some(Rolled {
            from: "WINV26".to_owned(),
            to: "WINZ26".to_owned(),
        })
    }

    /// A link that just rolled WINV26 to WINZ26, speaking for the new feed.
    fn rolled_link() -> (FeedLink, mpsc::Sender<FeedNotice>) {
        let (mut link, _old) = link();
        link.auto_roll.rolling("WINV26", "WINZ26", true);
        let (next, notices) = handle();
        link.attach(next, 5);
        assert_eq!(link.notice, FeedNotice::Clear, "the new feed starts clean");
        assert_eq!(link.rolled, rolled_note());
        (link, notices)
    }

    /// The note and the notice are two slots: every notice the new feed
    /// says reaches the notice, before and after its connect, and none of
    /// them takes the note away.
    #[test]
    fn a_roll_note_stands_beside_every_notice() {
        let (mut link, notices) = rolled_link();
        for notice in [
            FeedNotice::working("waiting for a MetaTrader bridge on 127.0.0.1:5555"),
            FeedNotice::working("waiting for the MetaTrader bridge to connect"),
            FeedNotice::Clear,
            FeedNotice::Connected,
            FeedNotice::working("loading WINZ26 history"),
            FeedNotice::reconnecting("MetaTrader 5 disconnected"),
            FeedNotice::attention("MetaTrader is not running", "Start it."),
            FeedNotice::Clear,
        ] {
            say(&mut link, &notices, notice.clone());
            let shown = match notice {
                FeedNotice::Connected => FeedNotice::Clear,
                other => other,
            };
            assert_eq!(link.notice, shown, "the notice says what the feed said");
            assert_eq!(link.rolled, rolled_note(), "the note stands");
        }
        assert_eq!(
            link.rolled.as_ref().map(Rolled::sentence).as_deref(),
            Some("WINV26 is no longer listed; rolled to WINZ26")
        );
        assert_eq!(link.notice.unlisted_symbol(), None, "the note is no report");
    }

    /// The next attach — a switch, a reconnect, a reload — takes the note
    /// away; the attach after another roll explains that one.
    #[test]
    fn the_next_attach_replaces_the_roll_note() {
        let (mut link, _notices) = rolled_link();
        let (next, _notices) = handle();
        link.attach(next, 7);
        assert_eq!(link.rolled, None, "an attach that rolled nothing is clean");

        link.auto_roll.rolling("WINZ26", "WING27", true);
        let (next, _notices) = handle();
        link.attach(next, 9);
        assert_eq!(
            link.rolled,
            Some(Rolled {
                from: "WINZ26".to_owned(),
                to: "WING27".to_owned(),
            })
        );
    }
}
