//! One tab's session core: every decision a tab makes about reaching back,
//! behind one command bus.
//!
//! A tab puts *older* data in front of what the live feed has given it in two
//! ways that share a seam: the venue's own candles fold in as a prefix under
//! the bars the tape built, and a run of trade-history pages extends the tape
//! itself. Both are request/reply state machines with one request in flight,
//! and both answer the same *load older* press — from the toolbar, from the
//! History menu, from `feed.history.load` on the control plane, from a
//! scripted hook.
//!
//! [`TabSession`] holds that state and makes those decisions, and nothing
//! else does. The window around it is a shell: it hands the session one
//! [`Msg`] — an [`Act`] plus the [`TabView`] facts it is judged against — and
//! carries out the [`Effect`]s that come back: a request for the feed, a
//! prefix for a pane, a wait for the loading overlay. Every entry point is the
//! same [`TabSession::dispatch`], so a click and a control call cannot drift
//! into two accounts of the same press.
//!
//! Headless like everything below the window: no UI, no thread, no channel,
//! no network. The session is told the time on every dispatch and never reads
//! a clock, and it never names the feed host — a request leaves as an
//! [`Effect::Feed`] the shell sends, and its delivery comes back as
//! [`Act::FeedAck`].

mod paging;
mod venue;
mod view;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use quantick_civil::TzOffset;
use quantick_engine::{Bar, Trade};
use quantick_sources::candle_base::CandleAsk;
pub use quantick_sources::candles::OlderCandles;
use quantick_sources::config::FeedCapabilities;
use quantick_sources::history_reach::{HistoryReach, ReachOutcome};
use quantick_sources::history_run::{Cancelled, Press, RunStatus};
use quantick_sources::ohlcv::OhlcvSlice;

pub use paging::REBUILD_FAILED_NOTE;
pub use view::{PaneView, TabView};

use paging::TradePaging;
use venue::VenueCandles;

/// How long the outcome of a *load older* press stays on screen.
///
/// Long enough to read one short line without hunting for it, short enough
/// that it is gone before the trader's next decision. It leaves on its own
/// because it is a remark and not a fault: nothing here needs acknowledging,
/// and a card waiting to be dismissed over a live chart interrupts more than
/// the silence it replaced.
pub const HISTORY_NOTE_LINGER: Duration = Duration::from_secs(6);

/// How many [`SessionEvent`]s the session keeps for a reader that has not
/// drained them. The oldest go first: the trail is a recent record, never a
/// growing one.
pub const MAX_RETAINED_EVENTS: usize = 256;

/// The window's history choices every tab mirrors on each frame's drain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryPolicy {
    /// Whether the next candle request asks for slices.
    pub progressive: bool,
    /// Whether a pane not cut by time carries the venue's minutes.
    pub venue_lead_in: bool,
    /// What the History button needs this frame.
    pub frame: HistoryFrame,
}

/// What a tab's history button needs from the window each frame. A tab
/// counts as on screen until a frame says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryFrame {
    /// What the main click loads on a tab that never pressed.
    pub default_reach: HistoryReach,
    /// The clock face the note is written in.
    pub tz: TzOffset,
    /// Whether the tab is on screen; a hidden tab's run stops paging.
    pub visible: bool,
}

impl Default for HistoryFrame {
    fn default() -> Self {
        Self {
            default_reach: HistoryReach::default(),
            tz: TzOffset::default(),
            visible: true,
        }
    }
}

/// One command to the session, and the facts it is judged against.
///
/// The facts travel with the act rather than living in the session because
/// the session does not own them: the panes, the tape and the feed's
/// capabilities belong to the window, and a copy kept here would be a second
/// opinion about them that could go stale between two frames.
pub struct Msg<'a> {
    /// What is asked.
    pub act: Act<'a>,
    /// The tab as it stands when it is asked.
    pub tab: &'a TabView<'a>,
}

/// What a tab can be asked to do about reaching back.
#[derive(Debug)]
pub enum Act<'a> {
    /// Load back to a target: the toolbar's press, a History menu action and
    /// `feed.history.load` alike. A failed rebuild is retried first, keeping
    /// every page it held; the press itself follows as [`Effect::Resume`].
    PressHistory(HistoryReach),
    /// The main click: load back to this tab's last target, or the window's
    /// default until it has pressed.
    PressMainHistory,
    /// Stop the run and keep what arrived, or drop a press still waiting.
    CancelHistory,
    /// A page of older trades landed: judge it before any rebuild, so the
    /// next request goes out while the chart is still being rebuilt.
    HistoryPage(&'a [Trade]),
    /// A pane's rebuild failed: end the run where it stood and say so.
    RebuildFailed,
    /// The tape the run was paging is gone: drop the run and its verdict.
    AbandonRun,
    /// Put one sentence about the last press where the trader is looking.
    RaiseNote(String),
    /// Drop the note once it has had its [`HISTORY_NOTE_LINGER`].
    ExpireNote,
    /// The window's switches and this frame's History button facts; a press
    /// queued behind the opening fill begins, and a paused run resumes.
    MirrorPolicy(HistoryPolicy),
    /// Once a frame: follow the panes to the candle base they want, watch the
    /// capability edge, and ask the venue when there is anything to ask.
    PollCandles {
        /// The tab, for the log.
        tab_id: u64,
    },
    /// One reply to a candle request.
    CandleReply {
        /// The tab, for the log.
        tab_id: u64,
        /// The interval the candles were served at.
        interval_ms: i64,
        /// The candles, ascending.
        bars: Vec<Bar>,
        /// Where this reply sits in its run.
        slice: OhlcvSlice,
    },
    /// A candle reply handed in directly, as a scripted hook does: the wait a
    /// request raises is raised first when none is out.
    DeliverCandles {
        /// The tab, for the log.
        tab_id: u64,
        /// The interval the candles were served at.
        interval_ms: i64,
        /// The candles, ascending.
        bars: Vec<Bar>,
        /// Where this reply sits in its run.
        slice: OhlcvSlice,
    },
    /// Reach one more span of venue candles into the past.
    LoadOlderCandles {
        /// The tab, for the log.
        tab_id: u64,
    },
    /// Rebuild every pane's prefix from the candles held.
    Refold,
    /// Put the venue's minutes in front of a chart cut by trades, or not.
    SetVenueLeadIn(bool),
    /// A context pane was just built: its copy of the tape counts against a
    /// running campaign, and it wants a prefix of its own.
    PaneAdded {
        /// The tab, for the log.
        tab_id: u64,
    },
    /// A fresh feed session was attached; `keep_timeline` when it serves the
    /// same market, so what the old session built is kept.
    Attach {
        /// Whether the candles and what reaching back learned are kept.
        keep_timeline: bool,
    },
    /// What became of a request the session handed out.
    FeedAck {
        /// The request, as the session phrased it.
        request: FeedRequest,
        /// Whether the feed took it.
        delivery: Delivery,
    },
    /// The second half of an act that needed the shell to run effects first.
    Resume(Resume),
}

/// An act's second half, dispatched back by the shell once the effects ahead
/// of it have run, so it is judged against the tab they left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// The press, after every failed rebuild was retried.
    Press(HistoryReach),
}

/// What the shell does for the session. Each is carried out in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send this request to the feed, and answer with [`Act::FeedAck`].
    Feed(FeedRequest),
    /// Put `bars` in front of pane `pane`'s own bars (flow first, then the
    /// context stack), with the venue's seam `lead` merged into its first
    /// engine bar and whether that leaves the seam bucket fully accounted for.
    InstallPrefix {
        /// The pane, in the tab's address order.
        pane: usize,
        /// The prefix; empty takes the prefix away.
        bars: Vec<Bar>,
        /// The venue's record of the seam bucket before the first trade.
        lead: Option<Bar>,
        /// Whether `lead` covers the whole seam bucket.
        covers_seam: bool,
        /// What the seam outcome was, for the shell's log beside the pane.
        seam: Option<SeamTrace>,
    },
    /// Hold every pane's rebuild while a run pages, or release them.
    HoldPages(bool),
    /// Retry every pane's failed rebuild, keeping the pages it held.
    RetryRebuilds,
    /// Raise, end or clear one of the waits the loading overlay shows.
    Wait(Wait, WaitChange),
    /// Dispatch [`Act::Resume`] with this once the effects before it ran.
    Resume(Resume),
}

/// A request for the feed, phrased by the session and sent by the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedRequest {
    /// Venue candles: `FeedCommand::FetchOhlcv` with this ask.
    Candles {
        /// The tab that asked, for the log.
        tab_id: u64,
        /// What is asked of the venue.
        ask: CandleAsk,
        /// Why, which decides what its delivery changes.
        purpose: CandlePurpose,
    },
    /// One page of older trades: `FeedCommand::LoadOlder`.
    OlderTrades {
        /// How many prints.
        count: usize,
    },
}

/// Why a candle request is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandlePurpose {
    /// The opening request, from the live edge.
    Opening,
    /// The minutes a daily base's seam leads lack.
    SeamMinutes,
    /// *Load older*: one more span behind the oldest candle held.
    Older {
        /// The oldest candle held when it was asked.
        oldest_ms: i64,
    },
}

/// What the feed's command channel did with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Queued.
    Sent,
    /// The channel was full: a busy frame, worth asking again.
    Busy,
    /// The channel is closed: the feed is gone and nothing will answer.
    Gone,
}

/// The waits the session raises on the loading overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// A venue candle request is out.
    VenueCandles,
    /// A page of older trades is out.
    OlderTrades,
}

/// What happens to a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitChange {
    /// One more request is out.
    Begin,
    /// One request was answered.
    End,
    /// Every outstanding one belonged to a channel that is gone.
    Clear,
}

/// The seam outcome behind one prefix install, for the line the shell logs
/// when the pane's seam bar changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeamTrace {
    /// The pane's first print, by its bar's open.
    pub first_print_ms: i64,
    /// The pane's interval.
    pub interval_ms: i64,
    /// The seam outcome's stable word.
    pub token: &'static str,
}

/// Why an act changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A run is already paging; nothing new starts.
    AlreadyRunning,
    /// Nothing older to ask the venue for, and which reason it is.
    OlderCandles(OlderCandles),
}

/// What the session decided, for a reader of its trail.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// A press was judged.
    Pressed {
        /// The target pressed.
        reach: HistoryReach,
        /// What the press did.
        press: Press,
    },
    /// A cancel was judged.
    Cancelled(Cancelled),
    /// A run finished.
    RunSettled(ReachOutcome),
    /// A request reached the feed.
    Asked(FeedRequest),
    /// A request did not.
    Undelivered {
        /// The request.
        request: FeedRequest,
        /// Why not.
        delivery: Delivery,
    },
    /// The panes now fold from a different candle base.
    BaseChanged {
        /// The base before.
        was_ms: i64,
        /// The base wanted now.
        now_ms: i64,
    },
    /// A candle reply was taken, or dropped as superseded.
    CandleReply {
        /// The interval served.
        interval_ms: i64,
        /// How many candles it carried.
        bars: usize,
        /// Where it sat in its run.
        slice: OhlcvSlice,
        /// Whether it belonged to an answer already thrown away.
        discarded: bool,
    },
    /// A *load older candles* settled.
    OlderCandlesSettled {
        /// Whether the oldest candle moved.
        moved: bool,
        /// Whether the venue's record starts here.
        exhausted: bool,
    },
    /// A note went up.
    NoteRaised(String),
    /// The note came down.
    NoteCleared,
}

/// Everything the session holds that a reader may compare, in one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// Where the history run stands.
    pub run: RunStatus,
    /// What the main click would load.
    pub main_reach: HistoryReach,
    /// What the last press did.
    pub last_press: Option<Press>,
    /// What the last cancel stopped.
    pub last_cancel: Option<Cancelled>,
    /// The note on screen.
    pub note: Option<String>,
    /// Venue candles held, at the base interval.
    pub candles_held: usize,
    /// Whether a candle request is out.
    pub candles_pending: bool,
    /// Whether the venue's record starts at the oldest candle held.
    pub older_exhausted: bool,
    /// Whether a pane cut by trades carries the venue's minutes.
    pub venue_lead_in: bool,
    /// Whether candle requests ask for slices.
    pub progressive: bool,
}

/// One tab's reaching back: venue candles and trade-history paging.
#[derive(Debug)]
pub struct TabSession {
    venue: VenueCandles,
    paging: TradePaging,
    events: VecDeque<SessionEvent>,
}

impl Default for TabSession {
    fn default() -> Self {
        Self::new()
    }
}

/// One dispatch's working set: the facts it is judged against, the time it
/// is told, and what it hands back.
struct Step<'a> {
    tab: &'a TabView<'a>,
    now: Instant,
    effects: Vec<Effect>,
    events: Vec<SessionEvent>,
}

impl Step<'_> {
    fn feed(&mut self, request: FeedRequest) {
        self.effects.push(Effect::Feed(request));
    }

    fn wait(&mut self, wait: Wait, change: WaitChange) {
        self.effects.push(Effect::Wait(wait, change));
    }

    fn hold(&mut self, held: bool) {
        self.effects.push(Effect::HoldPages(held));
    }

    fn clear_prefixes(&mut self) {
        for pane in 0..self.tab.panes.len() {
            self.effects.push(Effect::InstallPrefix {
                pane,
                bars: Vec::new(),
                lead: None,
                covers_seam: false,
                seam: None,
            });
        }
    }

    fn event(&mut self, event: SessionEvent) {
        self.events.push(event);
    }

    fn delivered(&mut self, request: FeedRequest, delivery: Delivery) {
        self.event(match delivery {
            Delivery::Sent => SessionEvent::Asked(request),
            Delivery::Busy | Delivery::Gone => SessionEvent::Undelivered { request, delivery },
        });
    }
}

impl TabSession {
    /// A session with nothing fetched, nothing pressed, and progressive candle
    /// history on.
    #[must_use]
    pub fn new() -> Self {
        Self {
            venue: VenueCandles::default(),
            paging: TradePaging::default(),
            events: VecDeque::new(),
        }
    }

    /// The one entry point: judge `msg` at `now` and say what the shell must
    /// do. A refusal changes nothing the shell has to carry out.
    pub fn dispatch(&mut self, msg: Msg<'_>, now: Instant) -> Result<Vec<Effect>, Refusal> {
        let Msg { act, tab } = msg;
        let mut step = Step {
            tab,
            now,
            effects: Vec::new(),
            events: Vec::new(),
        };
        let answer = self.apply(act, &mut step);
        for event in step.events {
            if self.events.len() == MAX_RETAINED_EVENTS {
                self.events.pop_front();
            }
            self.events.push_back(event);
        }
        answer.map(|()| step.effects)
    }

    fn apply(&mut self, act: Act<'_>, step: &mut Step<'_>) -> Result<(), Refusal> {
        match act {
            Act::PressHistory(reach) => self.paging.prepare_press(step, reach),
            Act::PressMainHistory => {
                let reach = self.paging.main_reach();
                self.paging.prepare_press(step, reach);
            }
            Act::Resume(Resume::Press(reach)) => return self.paging.press(step, reach),
            Act::CancelHistory => self.paging.cancel(step),
            Act::HistoryPage(page) => self.paging.judge_page(step, page),
            Act::RebuildFailed => self.paging.rebuild_failed(step),
            Act::AbandonRun => self.paging.abandon(step),
            Act::RaiseNote(text) => self.paging.raise_note(step, text),
            Act::ExpireNote => self.paging.expire_note(step),
            Act::MirrorPolicy(policy) => {
                // The switches live on the window; the reach does not: a run
                // and the last target pressed belong to this tab.
                self.venue.progressive = policy.progressive;
                self.paging.frame = policy.frame;
                self.venue.set_lead_in(step, policy.venue_lead_in);
                self.paging.poll(step);
            }
            Act::PollCandles { tab_id } => self.venue.poll(step, tab_id),
            Act::CandleReply {
                tab_id,
                interval_ms,
                bars,
                slice,
            } => self
                .venue
                .take_reply(step, tab_id, interval_ms, bars, slice),
            Act::DeliverCandles {
                tab_id,
                interval_ms,
                bars,
                slice,
            } => self.venue.deliver(step, tab_id, interval_ms, bars, slice),
            Act::LoadOlderCandles { tab_id } => return self.venue.request_older(step, tab_id),
            Act::Refold => self.venue.refold(step),
            Act::SetVenueLeadIn(enabled) => self.venue.set_lead_in(step, enabled),
            Act::PaneAdded { tab_id } => {
                self.paging.count_copies(step.tab.panes.len());
                self.venue.pane_added(step, tab_id);
            }
            Act::Attach { keep_timeline } => {
                self.venue.attach(step, keep_timeline);
                // The run belonged to the old session's tape; its reply is on
                // a channel about to be dropped.
                self.paging.abandon(step);
                step.wait(Wait::VenueCandles, WaitChange::Clear);
            }
            Act::FeedAck { request, delivery } => {
                step.delivered(request, delivery);
                match request {
                    FeedRequest::Candles {
                        tab_id,
                        ask,
                        purpose,
                    } => self.venue.acknowledge(step, tab_id, ask, purpose, delivery),
                    FeedRequest::OlderTrades { .. } => self.paging.acknowledge(step, delivery),
                }
            }
        }
        Ok(())
    }

    /// Everything a reader may compare, in one value.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            run: self.paging.status(),
            main_reach: self.paging.main_reach(),
            last_press: self.paging.last_press,
            last_cancel: self.paging.last_cancel,
            note: self.paging.note().map(str::to_owned),
            candles_held: self.venue.candles_held(),
            candles_pending: self.venue.pending,
            older_exhausted: self.venue.older_exhausted,
            venue_lead_in: self.venue.lead_in,
            progressive: self.venue.progressive,
        }
    }

    /// Take the trail of decisions since the last drain, oldest first.
    pub fn drain_events(&mut self) -> Vec<SessionEvent> {
        self.events.drain(..).collect()
    }

    /// Where this tab's run stands.
    #[must_use]
    pub fn history_status(&self) -> RunStatus {
        self.paging.status()
    }

    /// What the main click would load.
    #[must_use]
    pub fn main_history_reach(&self) -> HistoryReach {
        self.paging.main_reach()
    }

    /// What the last press had to say, while it is still on screen.
    #[must_use]
    pub fn history_note(&self) -> Option<&str> {
        self.paging.note()
    }

    /// Whether a run is paging. Read by the toolbar and the control plane.
    #[must_use]
    pub const fn history_reach_running(&self) -> bool {
        self.paging.holds_pages()
    }

    /// Whether a page request is out, perhaps from a run since cancelled.
    #[must_use]
    pub const fn awaiting_reply(&self) -> bool {
        self.paging.awaiting_reply()
    }

    /// This frame's History button facts, as last mirrored.
    #[must_use]
    pub const fn history_frame(&self) -> HistoryFrame {
        self.paging.frame
    }

    /// The pane copies the running campaign's memory ceiling counts.
    #[must_use]
    pub fn history_tape_copies(&self) -> Option<usize> {
        self.paging.tape_copies()
    }

    /// How many venue candles this tab holds, at the base interval. Zero on a
    /// feed that serves none, and before the first reply lands.
    #[must_use]
    pub fn venue_candles_held(&self) -> usize {
        self.venue.candles_held()
    }

    /// Whether any venue candle base is held, even an empty answer.
    #[must_use]
    pub const fn holds_venue_candles(&self) -> bool {
        self.venue.holds_base()
    }

    /// Whether the venue's record starts inside the `interval_ms` bucket its
    /// oldest candle folds into: that bar holds only part of its bucket.
    #[must_use]
    pub fn venue_record_starts_inside(&self, interval_ms: i64) -> bool {
        self.venue.record_starts_inside(interval_ms)
    }

    /// Whether asking for older candles could get the trader anything — and
    /// when it could not, *which* of the reasons it is.
    #[must_use]
    pub fn older_candles(
        &self,
        capabilities: FeedCapabilities,
        panes: &[PaneView<'_>],
    ) -> OlderCandles {
        self.venue.older_candles(capabilities, panes)
    }

    /// Whether the next candle request asks for slices.
    #[must_use]
    pub const fn progressive_history(&self) -> bool {
        self.venue.progressive
    }

    /// Whether a pane not cut by time carries the venue's minutes.
    #[must_use]
    pub const fn venue_lead_in(&self) -> bool {
        self.venue.lead_in
    }

    /// Forget which candle generation was acted on, so the next poll treats
    /// the feed's as new — what a reconnect storing a fresh block does. For
    /// a test that has to stage one.
    pub fn forget_candle_generations(&mut self) {
        self.venue.forget_generations();
    }
}

#[cfg(test)]
mod session_tests;
