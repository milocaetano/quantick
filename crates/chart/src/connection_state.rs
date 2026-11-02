//! One answer per symbol to "is this chart being fed, and what is it waiting
//! on?", read by every surface that says so: the header's chip and slot, the
//! overlay over an empty pane and the status word. The clock is told, never
//! read, so every threshold below is a table in the tests.
//!
//! The rule the trader set: messages about the connection look like a platform
//! bug when they flicker. So a short blip shows nothing, and offline **latches**:
//! once the stall has fired the state holds until data arrives, a hand presses
//! Reconnect, or the transport has stayed up for [`CONNECTED_HOLD_MS`], whatever
//! the feed's automatic retries do in between. A print answers the report it
//! arrived under for good: only a new report can latch again, so a thin tape
//! cannot bounce the words through its gaps.

use quantick_orderflow::engine::CaptureStatus;

use crate::loading::{LoadingTask, LoadingTracker};

/// How long a transport may be away before any surface mentions it.
pub const BLIP_MS: i64 = 2_000;
/// How long a pane must stand with no bars before the overlay covers it.
pub const OVERLAY_DELAY_MS: i64 = 2_000;
/// How long a job runs before the slot names it.
pub const SLOT_DELAY_MS: i64 = 1_000;
/// How long the slot keeps what it named, so a word can be read at all.
pub const SLOT_HOLD_MS: i64 = 500;
/// How long the attempt a press started is shown, however fast it fails.
pub const PRESS_HOLD_MS: i64 = 1_000;
/// How long a transport must stay up, unbroken, to clear the offline latch on
/// its own: a flapping feed never gets there, a closed market after a reconnect
/// does and reaches its true word.
pub const CONNECTED_HOLD_MS: i64 = 10_000;
/// How long after a print the data still counts as flowing, whatever a report
/// that arrives meanwhile says.
pub const DATA_FLOWING_MS: i64 = 2_000;
/// How long a print keeps answering the report standing over it. Longer than
/// any thin tape's gap, short enough that a feed which died after that print
/// is called offline rather than live for hours.
pub const REPORT_ANSWER_MS: i64 = 60_000;
/// How often a tab nobody is looking at is observed. The one on screen is
/// observed every frame.
pub const BACKGROUND_OBSERVE_MS: i64 = 1_000;

/// The transport as the feed reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Connecting,
    Reconnecting,
    Connected,
}

/// What the stall judgement and the provider have concluded about the feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// Nothing beyond what the transport says.
    None,
    /// Broken: the budget ran out, or the provider named a reason (its words
    /// are carried only in that second case).
    Failed { provider_says: Option<&'a str> },
    /// A connected transport with nothing coming down it.
    Quiet,
}

/// What the chart can honestly say about its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Connected and not silent.
    Live,
    /// The transport is trying.
    Attempting,
    /// The stall fired. Latched: see [`next_link`].
    Offline,
    /// Connected, and silent long enough to say so.
    Quiet,
    /// A source with no transport to be up or down.
    NoTransport,
}

/// The facts one step of [`next_link`] decides from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkFacts {
    pub has_transport: bool,
    pub transport: Transport,
    pub failed: bool,
    pub quiet: bool,
    /// A print arrived within [`DATA_FLOWING_MS`].
    pub data_flowing: bool,
    /// A print arrived after the report now standing: that report has been
    /// answered, and the gap to the next print does not bring it back.
    pub report_superseded: bool,
    /// A press's minimum attempt is still running.
    pub press_holding: bool,
    /// The transport has been up, unbroken, for [`CONNECTED_HOLD_MS`].
    pub connected_held: bool,
}

/// The next link, from the last one and this frame's facts.
///
/// Data outranks every report: prints arriving are the answer to the question,
/// and a report they answered stays answered, whatever the transport still says
/// about it, until another one replaces it.
/// Three things leave [`Link::Offline`]: a press (a respawned session, which
/// holds), data, and a transport that has stayed up for [`CONNECTED_HOLD_MS`].
/// An automatic retry changes `transport` or the notice behind `failed` on every
/// tick, and none of that moves it, which is what stops the words bouncing while
/// the feed's own loop runs.
#[must_use]
pub fn next_link(previous: Link, facts: LinkFacts) -> Link {
    if !facts.has_transport {
        return Link::NoTransport;
    }
    if facts.press_holding {
        return Link::Attempting;
    }
    if facts.data_flowing || facts.report_superseded {
        return Link::Live;
    }
    if facts.failed || (previous == Link::Offline && !facts.connected_held) {
        return Link::Offline;
    }
    if facts.quiet {
        return Link::Quiet;
    }
    if facts.transport == Transport::Connected {
        Link::Live
    } else {
        Link::Attempting
    }
}

/// One stream's dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Nothing is arriving and nothing is trying.
    Idle,
    Connecting,
    Ok,
}

/// The book's dot, which has two more honest words than the other streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Idle,
    Connecting,
    Synced,
    /// The source has none or it is switched off. Never raises the chip.
    Off,
    /// It was there and is gone, or it errored.
    Lost,
}

impl Book {
    /// The book's dot for a capture status.
    #[must_use]
    pub fn of(status: &CaptureStatus) -> Self {
        match status {
            CaptureStatus::Connecting
            | CaptureStatus::Buffering
            | CaptureStatus::SnapshotFetching
            | CaptureStatus::Resyncing { .. } => Self::Connecting,
            CaptureStatus::Live { .. } => Self::Synced,
            CaptureStatus::Disabled => Self::Off,
            CaptureStatus::Disconnected { .. } | CaptureStatus::Error => Self::Lost,
        }
    }

    const fn is_settled_ok(self) -> bool {
        matches!(self, Self::Synced | Self::Off)
    }

    /// The wire and hover word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Synced => "synced",
            Self::Off => "off",
            Self::Lost => "lost",
        }
    }
}

impl Stream {
    /// The wire and hover word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Ok => "live",
        }
    }
}

/// The job the slot names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Job {
    pub task: LoadingTask,
    /// Measured progress in `0.0..=1.0`. `None` where the worker knows none:
    /// a percentage is never invented.
    pub fraction: Option<f32>,
}

impl Job {
    /// What the slot writes.
    #[must_use]
    pub fn text(self) -> String {
        match self.fraction {
            Some(fraction) => format!(
                "{} {:.0}%",
                self.task.label(),
                fraction.clamp(0.0, 1.0) * 100.0
            ),
            None => self.task.label().to_owned(),
        }
    }
}

/// Everything the surfaces read, and what the control plane reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConnectionState {
    pub link: Link,
    pub feed: Stream,
    pub book: Book,
    pub trades: Stream,
    /// When the transport was last seen connected, wall clock.
    pub last_connected_at: Option<i64>,
    /// When the last print arrived, wall clock.
    pub last_data_at: Option<i64>,
    pub current_job: Option<Job>,
}

/// One frame's facts for one symbol, gathered by the window.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionFacts<'a> {
    pub now_ms: i64,
    pub symbol: &'a str,
    /// When this feed session was attached: moves on a respawn, and only then.
    pub attached_ms: i64,
    /// Moves whenever the tab throws its timeline away: a reload, a feed switch,
    /// a replay opening or seeking. A respawn that keeps the timeline leaves it.
    pub timeline_generation: u64,
    pub has_transport: bool,
    pub transport: Transport,
    pub verdict: Verdict<'a>,
    /// Live prints seen on this tab, a count that only rises.
    pub live_trades: u64,
    /// Some pane is on screen with no bars.
    pub empty_pane: bool,
    /// The chart holds anything at all.
    pub any_bars: bool,
    /// The flow series holds bars: the tape has seen market data, so a pane
    /// that is empty is only waiting on its own candles.
    pub flow_bars: bool,
    pub book: Book,
    pub fraction: Option<f32>,
}

/// One chip dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dot {
    pub name: &'static str,
    pub look: DotLook,
    pub hover: &'static str,
}

/// How a dot is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotLook {
    Faint,
    Pulsing,
    Solid,
}

/// What the overlay writes: `{verb} {symbol}`, then the meta line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayWords {
    pub verb: &'static str,
    pub meta: String,
    /// The one input the overlay has.
    pub reconnect: bool,
    /// The mark breathes, which is the only motion it has.
    pub breathing: bool,
}

/// What the status bar's first cell says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusWord {
    pub word: &'static str,
    /// Only an attempt moves.
    pub pulses: bool,
    /// `m:ss` since the last print, beside `offline` with bars on the chart.
    pub age: Option<String>,
    /// The one-click Reconnect beside it: only while no overlay offers one.
    pub reconnect: bool,
}

/// The frame-to-frame memory behind [`ConnectionState`]. One per tab, which is
/// one symbol.
#[derive(Debug, Clone)]
pub struct ConnectionTracker {
    symbol: String,
    timeline_generation: u64,
    state: ConnectionState,
    attached_ms: Option<i64>,
    /// When the current attach started, against which startup settles.
    settle_from_ms: i64,
    /// Set by a respawn on the same timeline: an attempt a hand asked for.
    pressed: bool,
    press_hold_until: i64,
    attempting_since: i64,
    /// What the current attempt interrupted, which a blip goes on saying.
    before_attempt: Link,
    connected_since: Option<i64>,
    observed_ms: Option<i64>,
    /// Whether the overlay has room for its control; the status bar offers
    /// Reconnect when it has not.
    control_fits: bool,
    live_trades: u64,
    book_shown: Book,
    book_raw: Book,
    book_raw_since: i64,
    provider_says: Option<String>,
    /// The failure report seen last frame, by its words: the provider's, or
    /// `None` inside for one the stall inferred.
    standing_report: Option<Option<String>>,
    /// A print has arrived since [`Self::standing_report`] first stood.
    report_superseded: bool,
    empty_since: Option<i64>,
    any_bars: bool,
    settling: bool,
    /// The feed's own loop is still trying.
    retrying: bool,
    job_started: [Option<i64>; LoadingTask::ALL.len()],
    job_shown_since: i64,
    any_job: bool,
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionTracker {
    /// A tracker that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            timeline_generation: 0,
            state: ConnectionState {
                link: Link::Attempting,
                feed: Stream::Idle,
                book: Book::Off,
                trades: Stream::Idle,
                last_connected_at: None,
                last_data_at: None,
                current_job: None,
            },
            attached_ms: None,
            settle_from_ms: 0,
            pressed: false,
            press_hold_until: i64::MIN,
            attempting_since: 0,
            before_attempt: Link::Attempting,
            connected_since: None,
            observed_ms: None,
            control_fits: true,
            live_trades: 0,
            book_shown: Book::Off,
            book_raw: Book::Off,
            book_raw_since: 0,
            provider_says: None,
            standing_report: None,
            report_superseded: false,
            empty_since: None,
            any_bars: false,
            settling: true,
            retrying: false,
            job_started: [None; LoadingTask::ALL.len()],
            job_shown_since: 0,
            any_job: false,
        }
    }

    /// The state as of the last [`Self::observe`].
    #[must_use]
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Whether a tab nobody is looking at is due another [`Self::observe`].
    #[must_use]
    pub fn background_due(&self, now_ms: i64) -> bool {
        self.observed_ms
            .is_none_or(|at| now_ms.saturating_sub(at) >= BACKGROUND_OBSERVE_MS)
    }

    /// Say whether the overlay's control fits its rect this frame. Told before
    /// either surface draws, so exactly one of them offers Reconnect.
    pub fn set_overlay_control_fits(&mut self, fits: bool) {
        self.control_fits = fits;
    }

    /// Take this frame's facts. Allocates only when the symbol, the report or
    /// the provider's own words changed.
    pub fn observe(&mut self, facts: &ConnectionFacts<'_>, jobs: &LoadingTracker) {
        let now = facts.now_ms;
        self.observed_ms = Some(now);
        self.observe_attach(facts);
        let previous = self.state.link;
        let printed = facts.live_trades > self.live_trades;
        if printed {
            self.state.last_data_at = Some(now);
        }
        self.live_trades = facts.live_trades;
        self.observe_report(facts.verdict, printed);
        self.connected_since =
            (facts.transport == Transport::Connected).then(|| self.connected_since.unwrap_or(now));
        let answered = self.report_superseded
            && self
                .state
                .last_data_at
                .is_some_and(|at| now.saturating_sub(at) < REPORT_ANSWER_MS);
        let link = next_link(
            previous,
            LinkFacts {
                has_transport: facts.has_transport,
                transport: facts.transport,
                failed: matches!(facts.verdict, Verdict::Failed { .. }) && !answered,
                quiet: matches!(facts.verdict, Verdict::Quiet),
                data_flowing: self
                    .state
                    .last_data_at
                    .is_some_and(|at| now.saturating_sub(at) < DATA_FLOWING_MS),
                report_superseded: answered,
                press_holding: now < self.press_hold_until,
                connected_held: self
                    .connected_since
                    .is_some_and(|since| now.saturating_sub(since) >= CONNECTED_HOLD_MS),
            },
        );
        if link == Link::Attempting && previous != Link::Attempting {
            self.attempting_since = now;
            self.before_attempt = previous;
        }
        self.retrying = facts.has_transport && facts.transport != Transport::Connected;
        self.settling = !self.pressed && now.saturating_sub(self.settle_from_ms) < BLIP_MS;
        // Not while offline: a sentence reading `No connection` beside
        // `last connected 0 s ago` is the bug it appears to be.
        if facts.transport == Transport::Connected && link != Link::Offline {
            self.state.last_connected_at = Some(now);
        }
        if link != Link::Offline {
            self.provider_says = None;
        }
        if let Verdict::Failed {
            provider_says: Some(words),
        } = facts.verdict
            && link == Link::Offline
            && self.provider_says.as_deref() != Some(words)
        {
            self.provider_says = Some(words.to_owned());
        }
        self.observe_book(facts.book, link, now);
        self.any_bars = facts.any_bars;
        // Only a tape with no market data at all earns the overlay: a time pane
        // merely awaiting its candles draws nothing.
        let tape_empty = !facts.flow_bars && self.state.last_data_at.is_none();
        self.empty_since =
            (facts.empty_pane && tape_empty).then(|| self.empty_since.unwrap_or(now));
        self.observe_jobs(jobs, link, facts.fraction, now);
        let attempt_shown = self.pressed || now.saturating_sub(self.attempting_since) >= BLIP_MS;
        self.state.link = link;
        self.state.feed = match link {
            Link::Live | Link::Quiet | Link::NoTransport => Stream::Ok,
            Link::Attempting if attempt_shown => Stream::Connecting,
            // A blip: what the dots last said stands.
            Link::Attempting => self.state.feed,
            Link::Offline => Stream::Idle,
        };
        self.state.trades = match link {
            Link::Live | Link::NoTransport => {
                if facts.any_bars || self.state.last_data_at.is_some() {
                    Stream::Ok
                } else {
                    Stream::Idle
                }
            }
            Link::Attempting if attempt_shown => Stream::Connecting,
            Link::Attempting => self.state.trades,
            Link::Offline | Link::Quiet => Stream::Idle,
        };
    }

    fn observe_attach(&mut self, facts: &ConnectionFacts<'_>) {
        let now = facts.now_ms;
        let new_symbol = self.symbol != facts.symbol;
        let new_timeline = new_symbol || self.timeline_generation != facts.timeline_generation;
        if new_symbol {
            facts.symbol.clone_into(&mut self.symbol);
        }
        if new_timeline {
            self.timeline_generation = facts.timeline_generation;
            // A latch, and every date, is about the timeline it was seen on.
            self.state.link = Link::Attempting;
            self.before_attempt = Link::Attempting;
            self.attempting_since = now;
            self.settle_from_ms = now;
            self.connected_since = None;
            self.pressed = false;
            self.press_hold_until = i64::MIN;
            self.state.last_connected_at = None;
            self.state.last_data_at = None;
            self.state.feed = Stream::Idle;
            self.state.trades = Stream::Idle;
            self.provider_says = None;
            self.standing_report = None;
            self.report_superseded = false;
            self.empty_since = None;
            self.live_trades = facts.live_trades;
        }
        if self.attached_ms != Some(facts.attached_ms) {
            let respawn = self.attached_ms.is_some() && !new_timeline;
            self.attached_ms = Some(facts.attached_ms);
            self.settle_from_ms = now;
            self.attempting_since = now;
            self.pressed = respawn;
            if respawn {
                self.press_hold_until = now.saturating_add(PRESS_HOLD_MS);
            }
        } else if self.pressed
            && now >= self.press_hold_until
            && self.state.link != Link::Attempting
        {
            self.pressed = false;
        }
    }

    /// Whether a print has answered the report now standing. A report first
    /// seen on this frame is new, whatever printed beside it: a feed's last
    /// print and its dying words can share a frame.
    fn observe_report(&mut self, verdict: Verdict<'_>, printed: bool) {
        let Verdict::Failed { provider_says } = verdict else {
            self.standing_report = None;
            self.report_superseded = false;
            return;
        };
        let standing = self
            .standing_report
            .as_ref()
            .is_some_and(|words| words.as_deref() == provider_says);
        if standing {
            self.report_superseded |= printed;
        } else {
            self.standing_report = Some(provider_says.map(str::to_owned));
            self.report_superseded = false;
        }
    }

    fn observe_book(&mut self, raw: Book, link: Link, now: i64) {
        // Nothing is trying while the feed itself is offline.
        let raw = if link == Link::Offline && raw == Book::Connecting {
            Book::Idle
        } else {
            raw
        };
        if raw != self.book_raw {
            self.book_raw = raw;
            self.book_raw_since = now;
        }
        // Coming good is news at once; going away waits out a blip.
        if raw.is_settled_ok() || now.saturating_sub(self.book_raw_since) >= BLIP_MS {
            self.book_shown = raw;
        }
        self.state.book = self.book_shown;
    }

    fn observe_jobs(&mut self, jobs: &LoadingTracker, link: Link, fraction: Option<f32>, now: i64) {
        self.any_job = jobs.any_active();
        let mut newest: Option<(LoadingTask, i64)> = None;
        for task in LoadingTask::ALL {
            let started = &mut self.job_started[task.index()];
            // A book cannot be syncing over a feed that is offline: the capture's
            // own "connecting" outlives the transport it was waiting on.
            let stale = task == LoadingTask::BookSync && link == Link::Offline;
            if !jobs.is_active(task) || stale {
                *started = None;
                continue;
            }
            let since = *started.get_or_insert(now);
            if now.saturating_sub(since) >= SLOT_DELAY_MS
                && newest.is_none_or(|(_, latest)| since >= latest)
            {
                newest = Some((task, since));
            }
        }
        let shown = self.state.current_job.map(|job| job.task);
        let wanted = newest.map(|(task, _)| task);
        if wanted == shown {
            if let Some(job) = &mut self.state.current_job {
                job.fraction = fraction.filter(|_| job.task == LoadingTask::BarRebuild);
            }
            return;
        }
        // What is named stays long enough to read; what replaces it is the
        // most recent job, one at a time.
        if shown.is_some() && now.saturating_sub(self.job_shown_since) < SLOT_HOLD_MS {
            return;
        }
        self.job_shown_since = now;
        self.state.current_job = wanted.map(|task| Job {
            task,
            fraction: fraction.filter(|_| task == LoadingTask::BarRebuild),
        });
    }

    /// The job slot's words and whether a job (not a remark) wrote them. A
    /// running job outranks the remark.
    #[must_use]
    pub fn slot(&self, note: Option<&str>) -> Option<(String, bool)> {
        match self.state.current_job {
            Some(job) => Some((job.text(), true)),
            None => note.map(|note| (note.to_owned(), false)),
        }
    }

    /// Whether the overlay stands over the empty panes this frame.
    #[must_use]
    pub fn overlay_up(&self, now_ms: i64) -> bool {
        let stood = self
            .empty_since
            .is_some_and(|since| now_ms.saturating_sub(since) >= OVERLAY_DELAY_MS);
        stood
            && match self.spoken_link(now_ms) {
                None => false,
                // "No trades" about a job still reading them in would be false:
                // the slot speaks until it is done.
                Some(Link::Live | Link::Quiet | Link::NoTransport) => !self.any_job,
                Some(Link::Attempting | Link::Offline) => true,
            }
    }

    /// Whether what the transport is doing may be spoken yet.
    fn attempt_shown(&self, now_ms: i64) -> bool {
        self.pressed || now_ms.saturating_sub(self.attempting_since) >= BLIP_MS
    }

    /// The link as every surface speaks it. A blip keeps the word it
    /// interrupted; a session opening has none.
    fn spoken_link(&self, now_ms: i64) -> Option<Link> {
        match self.state.link {
            Link::Attempting if !self.attempt_shown(now_ms) => {
                matches!(self.before_attempt, Link::Live | Link::Quiet)
                    .then_some(self.before_attempt)
            }
            link => Some(link),
        }
    }

    /// The overlay's words, or `None` when it is not up. Formats only then.
    #[must_use]
    pub fn overlay(&self, feed: &str, now_ms: i64) -> Option<OverlayWords> {
        if !self.overlay_up(now_ms) {
            return None;
        }
        let state = self.state;
        Some(match self.spoken_link(now_ms)? {
            Link::Live => OverlayWords {
                verb: "No trades yet on",
                meta: format!("{feed} · connected"),
                reconnect: false,
                breathing: false,
            },
            Link::Quiet => OverlayWords {
                verb: "No trades on",
                meta: format!("{feed} connected · market may be closed"),
                reconnect: false,
                breathing: false,
            },
            Link::NoTransport => OverlayWords {
                verb: "No data for",
                meta: format!("{feed} · 0 trades"),
                reconnect: false,
                breathing: false,
            },
            Link::Attempting => OverlayWords {
                verb: if self.pressed || state.last_connected_at.is_some() {
                    "Reconnecting to"
                } else {
                    "Connecting to"
                },
                meta: format!("{feed} · attempting"),
                reconnect: false,
                breathing: true,
            },
            Link::Offline => {
                let lead = match (&self.provider_says, state.last_connected_at) {
                    (Some(words), _) => words.clone(),
                    (None, Some(at)) => format!(
                        "{feed} · last connected {} ago",
                        spoken_ago(now_ms.saturating_sub(at))
                    ),
                    (None, None) => format!("{feed} · never connected"),
                };
                // The provider's report says what the hand must do; "retrying"
                // beside it would contradict a feed that has given up.
                let meta = if self.retrying && self.provider_says.is_none() {
                    format!("{lead} · retrying automatically")
                } else {
                    lead
                };
                OverlayWords {
                    verb: "No connection to",
                    meta,
                    reconnect: self.control_fits,
                    breathing: false,
                }
            }
        })
    }

    /// The status bar's word, or `None` while a fresh session settles and there
    /// is nothing true to say yet.
    #[must_use]
    pub fn status_word(&self, now_ms: i64) -> Option<StatusWord> {
        let still = |word| StatusWord {
            word,
            pulses: false,
            age: None,
            reconnect: false,
        };
        let state = self.state;
        match self.spoken_link(now_ms)? {
            Link::Live if self.settling && state.last_connected_at.is_none() => None,
            Link::Live => Some(still("live")),
            Link::Quiet => Some(still("quiet")),
            Link::NoTransport if self.any_bars => Some(still("replay")),
            Link::NoTransport => Some(still("no data")),
            Link::Attempting => Some(StatusWord {
                pulses: true,
                ..still("reconnecting")
            }),
            Link::Offline => {
                let overlay = self.overlay_up(now_ms);
                Some(StatusWord {
                    age: (!overlay)
                        .then(|| {
                            state
                                .last_data_at
                                .map(|at| clock_age(now_ms.saturating_sub(at)))
                        })
                        .flatten(),
                    reconnect: !overlay || !self.control_fits,
                    ..still("offline")
                })
            }
        }
    }

    /// The header chip's three dots, or `None` while every stream is well:
    /// nothing is drawn then, and nothing is drawn for a source with no streams
    /// or a session still settling.
    #[must_use]
    pub fn chip(&self) -> Option<[Dot; 3]> {
        let state = self.state;
        if state.link == Link::NoTransport || self.settling {
            return None;
        }
        if state.feed == Stream::Ok && state.trades == Stream::Ok && state.book.is_settled_ok() {
            return None;
        }
        Some([
            stream_dot(
                "feed",
                state.feed,
                ["feed connecting", "feed live", "feed idle"],
            ),
            book_dot(state.book),
            stream_dot(
                "trades",
                state.trades,
                ["trades connecting", "trades live", "trades idle"],
            ),
        ])
    }

    /// Whether the status word opens the recovery popup: there is something to
    /// read and press. A healthy feed and a source with no transport have not.
    #[must_use]
    pub fn has_recovery_door(&self) -> bool {
        matches!(self.state.link, Link::Offline | Link::Quiet)
    }
}

fn stream_dot(name: &'static str, stream: Stream, hovers: [&'static str; 3]) -> Dot {
    let [connecting, live, idle] = hovers;
    let (look, hover) = match stream {
        Stream::Connecting => (DotLook::Pulsing, connecting),
        Stream::Ok => (DotLook::Solid, live),
        Stream::Idle => (DotLook::Faint, idle),
    };
    Dot { name, look, hover }
}

fn book_dot(book: Book) -> Dot {
    let (look, hover) = match book {
        Book::Connecting => (DotLook::Pulsing, "book connecting"),
        Book::Synced => (DotLook::Solid, "book synced"),
        Book::Idle => (DotLook::Faint, "book idle"),
        Book::Off => (DotLook::Faint, "book off"),
        Book::Lost => (DotLook::Faint, "book lost"),
    };
    Dot {
        name: "book",
        look,
        hover,
    }
}

/// A duration as the meta line says it: `42 s`, `3 min`, `2 h`.
#[must_use]
pub fn spoken_ago(ms: i64) -> String {
    let seconds = ms.max(0) / 1_000;
    if seconds < 90 {
        format!("{seconds} s")
    } else if seconds < 90 * 60 {
        format!("{} min", seconds / 60)
    } else {
        format!("{} h", seconds / 3_600)
    }
}

/// A duration on the status bar's clock: `0:42`, `1:02:09`.
#[must_use]
pub fn clock_age(ms: i64) -> String {
    let seconds = ms.max(0) / 1_000;
    let (hours, minutes, seconds) = (seconds / 3_600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = "Binance";

    struct Run {
        tracker: ConnectionTracker,
        jobs: LoadingTracker,
        now: i64,
        attached: i64,
        timeline: u64,
        transport: Transport,
        verdict: Verdict<'static>,
        trades: u64,
        has_transport: bool,
        empty_pane: bool,
        any_bars: bool,
        flow_bars: bool,
        symbol: &'static str,
        book: Book,
    }

    impl Run {
        fn new() -> Self {
            let mut run = Self {
                tracker: ConnectionTracker::new(),
                jobs: LoadingTracker::new(),
                now: 1_000_000,
                attached: 1_000_000,
                timeline: 0,
                transport: Transport::Connecting,
                verdict: Verdict::None,
                trades: 0,
                has_transport: true,
                empty_pane: true,
                any_bars: false,
                flow_bars: false,
                symbol: "DOGEUSDT",
                book: Book::Off,
            };
            run.step(0);
            run
        }

        fn step(&mut self, ms: i64) {
            self.now += ms;
            self.tracker.observe(
                &ConnectionFacts {
                    now_ms: self.now,
                    symbol: self.symbol,
                    attached_ms: self.attached,
                    timeline_generation: self.timeline,
                    has_transport: self.has_transport,
                    transport: self.transport,
                    verdict: self.verdict,
                    live_trades: self.trades,
                    empty_pane: self.empty_pane,
                    any_bars: self.any_bars,
                    flow_bars: self.flow_bars,
                    book: self.book,
                    fraction: None,
                },
                &self.jobs,
            );
        }

        /// A respawn: what a press on Reconnect does to the session.
        fn press(&mut self) {
            self.attached = self.now;
            self.transport = Transport::Connecting;
            self.verdict = Verdict::None;
        }

        /// A new session on an empty timeline: a reload, a same-symbol feed
        /// switch, a replay opening.
        fn rebuild(&mut self) {
            self.press();
            self.timeline += 1;
            self.empty_pane = true;
            self.any_bars = false;
            self.flow_bars = false;
        }

        fn link(&self) -> Link {
            self.tracker.state().link
        }

        fn word(&self) -> Option<&'static str> {
            self.tracker.status_word(self.now).map(|word| word.word)
        }

        fn overlay(&self) -> Option<OverlayWords> {
            self.tracker.overlay(FEED, self.now)
        }

        fn pulsing(&self) -> bool {
            self.tracker
                .status_word(self.now)
                .is_some_and(|word| word.pulses)
        }

        /// Into the latch, on a tape with nothing on it.
        fn offline(&mut self) {
            self.transport = Transport::Reconnecting;
            self.verdict = FAILED;
            self.step(OVERLAY_DELAY_MS);
            assert_eq!(self.link(), Link::Offline);
        }

        fn connected(&mut self) {
            self.transport = Transport::Connected;
            self.verdict = Verdict::None;
            self.empty_pane = false;
            self.any_bars = true;
            self.step(0);
        }
    }

    const FAILED: Verdict<'static> = Verdict::Failed {
        provider_says: None,
    };

    fn facts(transport: Transport) -> LinkFacts {
        LinkFacts {
            has_transport: true,
            transport,
            failed: false,
            quiet: false,
            data_flowing: false,
            report_superseded: false,
            press_holding: false,
            connected_held: false,
        }
    }

    /// Each transition of the pure decision, the one the words come from.
    #[test]
    fn the_link_decision_table() {
        let trying = facts(Transport::Reconnecting);
        assert_eq!(next_link(Link::Live, trying), Link::Attempting);
        assert_eq!(next_link(Link::Attempting, trying), Link::Attempting);
        assert_eq!(
            next_link(
                Link::Attempting,
                LinkFacts {
                    failed: true,
                    ..trying
                }
            ),
            Link::Offline
        );
        assert_eq!(next_link(Link::Offline, trying), Link::Offline, "the latch");
        assert_eq!(
            next_link(Link::Offline, facts(Transport::Connecting)),
            Link::Offline,
            "whichever way the retry loop reports itself"
        );
        assert_eq!(
            next_link(
                Link::Offline,
                LinkFacts {
                    data_flowing: true,
                    ..trying
                }
            ),
            Link::Live,
            "data leaves the latch"
        );
        assert_eq!(
            next_link(
                Link::Offline,
                LinkFacts {
                    data_flowing: true,
                    failed: true,
                    ..trying
                }
            ),
            Link::Live,
            "data outranks a report still standing"
        );
        assert_eq!(
            next_link(
                Link::Live,
                LinkFacts {
                    report_superseded: true,
                    ..trying
                }
            ),
            Link::Live,
            "an answered report stays answered through a gap"
        );
        assert_eq!(
            next_link(Link::Offline, facts(Transport::Connected)),
            Link::Offline,
            "landing alone does not clear the latch"
        );
        let held = LinkFacts {
            connected_held: true,
            ..facts(Transport::Connected)
        };
        assert_eq!(
            next_link(Link::Offline, held),
            Link::Live,
            "staying up does"
        );
        assert_eq!(
            next_link(
                Link::Offline,
                LinkFacts {
                    quiet: true,
                    ..held
                }
            ),
            Link::Quiet,
            "and a closed market reaches its true word"
        );
        assert_eq!(
            next_link(
                Link::Offline,
                LinkFacts {
                    quiet: true,
                    ..facts(Transport::Connected)
                }
            ),
            Link::Offline,
            "quiet does not jump the hold"
        );
        assert_eq!(
            next_link(
                Link::Offline,
                LinkFacts {
                    press_holding: true,
                    failed: true,
                    ..trying
                }
            ),
            Link::Attempting,
            "a press is shown, however fast it fails"
        );
        assert_eq!(
            next_link(
                Link::Live,
                LinkFacts {
                    quiet: true,
                    ..facts(Transport::Connected)
                }
            ),
            Link::Quiet
        );
        assert_eq!(
            next_link(
                Link::Offline,
                LinkFacts {
                    has_transport: false,
                    failed: true,
                    ..trying
                }
            ),
            Link::NoTransport,
            "a source with no transport is never offline"
        );
    }

    /// The no-bounce rule: a provider that says Attention, then Working, then
    /// Attention on every retry tick leaves one word on screen.
    #[test]
    fn automatic_retries_do_not_bounce_the_words() {
        let mut run = Run::new();
        run.transport = Transport::Reconnecting;
        run.verdict = FAILED;
        run.step(100);
        let offline_title = run.tracker.status_word(run.now).expect("a word").word;
        assert_eq!(offline_title, "offline");
        for tick in 0..50 {
            run.verdict = if tick % 2 == 0 { Verdict::None } else { FAILED };
            run.transport = match tick % 3 {
                0 => Transport::Connecting,
                1 => Transport::Reconnecting,
                _ => Transport::Connected,
            };
            run.step(1_700);
            assert_eq!(run.link(), Link::Offline, "tick {tick}");
            assert_eq!(run.word(), Some("offline"), "tick {tick}");
            assert_eq!(run.tracker.state().feed, Stream::Idle, "tick {tick}");
            assert!(!run.pulsing(), "offline does not move");
        }
    }

    /// A feed that lands and drops inside the hold never leaves the latch, and
    /// the first stretch that outlasts the hold does: to its true word.
    #[test]
    fn a_flapping_feed_stays_offline_until_the_transport_holds() {
        let mut run = Run::new();
        run.offline();
        run.verdict = Verdict::None;
        for _ in 0..5 {
            run.transport = Transport::Connected;
            run.step(0);
            run.step(CONNECTED_HOLD_MS - 1);
            assert_eq!(run.link(), Link::Offline);
            run.transport = Transport::Reconnecting;
            run.step(1);
            assert_eq!(run.link(), Link::Offline);
        }
        run.transport = Transport::Connected;
        run.verdict = Verdict::Quiet;
        run.step(0);
        run.step(CONNECTED_HOLD_MS);
        assert_eq!(run.link(), Link::Quiet);
        assert_eq!(run.word(), Some("quiet"));
    }

    /// Prints under a report that still stands: the chart is being fed, for as
    /// long as they keep coming, and the gaps between them do not bounce it.
    #[test]
    fn prints_outrank_a_standing_attention() {
        let mut run = Run::new();
        run.verdict = Verdict::Failed {
            provider_says: Some("MetaTrader 5 is closed"),
        };
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.link(), Link::Offline);
        run.trades = 1;
        run.step(10);
        assert_eq!(run.link(), Link::Live);
        for _ in 0..3 {
            run.step(DATA_FLOWING_MS - 1);
            assert_eq!(run.link(), Link::Live, "a gap is not an outage");
            run.trades += 1;
            run.step(1);
            assert_eq!(run.link(), Link::Live);
        }
    }

    fn thin_tape_stays_live(run: &mut Run) {
        for gap in [3_000, 5_000, 4_000, 3_500, 5_000] {
            for _ in 0..gap / 250 {
                run.step(250);
                assert_eq!(run.link(), Link::Live, "a {gap} ms gap");
                assert_eq!(run.word(), Some("live"), "a {gap} ms gap");
            }
            run.trades += 1;
            run.step(0);
            assert_eq!(run.link(), Link::Live);
        }
    }

    /// The gaps of a thin tape run past [`DATA_FLOWING_MS`]. The first print
    /// answers the report, and from there only a new report can speak: not the
    /// gap, on a transport reported up or still reported down.
    #[test]
    fn a_thin_tape_does_not_bounce_under_a_report_it_answered() {
        for transport in [
            Transport::Connected,
            Transport::Connecting,
            Transport::Reconnecting,
        ] {
            let mut run = Run::new();
            run.transport = transport;
            run.verdict = Verdict::Failed {
                provider_says: Some("MetaTrader 5 is closed"),
            };
            run.step(OVERLAY_DELAY_MS);
            assert_eq!(run.link(), Link::Offline);
            run.trades = 1;
            run.step(1);
            thin_tape_stays_live(&mut run);

            // A stall's inferred report is answered the same way.
            run.verdict = Verdict::None;
            run.step(1);
            run.verdict = FAILED;
            run.trades += 1;
            run.step(1);
            run.step(DATA_FLOWING_MS);
            assert_eq!(run.link(), Link::Offline, "a new report latches");
            run.trades += 1;
            run.step(1);
            thin_tape_stays_live(&mut run);
        }
    }

    /// An offline link does not refresh its own age, whatever the transport
    /// claims while the latch holds.
    #[test]
    fn offline_freezes_the_last_connected_age() {
        let mut run = Run::new();
        run.transport = Transport::Connected;
        run.verdict = FAILED;
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.link(), Link::Offline);
        assert_eq!(run.tracker.state().last_connected_at, None, "never was");

        let mut run = Run::new();
        run.transport = Transport::Connected;
        run.step(1);
        let before = run.tracker.state().last_connected_at;
        assert!(before.is_some());
        run.verdict = FAILED;
        run.step(1);
        assert_eq!(run.link(), Link::Offline);
        run.step(CONNECTED_HOLD_MS / 2);
        assert_eq!(run.tracker.state().last_connected_at, before);
    }

    /// A print answers a report for a while, not for ever: a feed that died
    /// after it must not read `live` over a frozen chart.
    #[test]
    fn a_long_silence_under_an_answered_report_is_offline_again() {
        for transport in [Transport::Connected, Transport::Reconnecting] {
            let mut run = Run::new();
            run.transport = transport;
            run.verdict = Verdict::Failed {
                provider_says: Some("MetaTrader 5 is closed"),
            };
            run.step(OVERLAY_DELAY_MS);
            run.trades = 1;
            run.step(1);
            run.step(REPORT_ANSWER_MS - 1);
            assert_eq!(run.link(), Link::Live, "still answered");
            run.step(1);
            assert_eq!(run.link(), Link::Offline, "the silence outlived the answer");
            run.trades += 1;
            run.step(1);
            assert_eq!(run.link(), Link::Live, "a print answers it again");
        }
    }

    /// Another report is news, and so is the same words again after they
    /// stopped standing. A print sharing the report's first frame answers
    /// nothing: it came before the report, as far as anyone can tell.
    #[test]
    fn a_new_notice_latches_again_after_prints_answered_the_last() {
        const CLOSED: Verdict<'static> = Verdict::Failed {
            provider_says: Some("MetaTrader 5 is closed"),
        };
        const REFUSED: Verdict<'static> = Verdict::Failed {
            provider_says: Some("The bridge refused"),
        };
        let mut run = Run::new();
        run.verdict = CLOSED;
        run.step(1);
        run.trades = 1;
        run.step(1);
        run.step(DATA_FLOWING_MS);
        assert_eq!(run.link(), Link::Live, "answered");

        run.verdict = REFUSED;
        run.step(1);
        assert_eq!(run.link(), Link::Offline, "another report");
        run.trades += 1;
        run.step(1);
        run.step(DATA_FLOWING_MS);
        assert_eq!(run.link(), Link::Live, "answered in turn");

        run.verdict = Verdict::None;
        run.step(1);
        run.verdict = REFUSED;
        run.step(1);
        assert_eq!(run.link(), Link::Offline, "the same words, said again");

        run.verdict = CLOSED;
        run.trades += 1;
        run.step(1);
        assert_eq!(run.link(), Link::Live, "the print itself still counts");
        run.step(DATA_FLOWING_MS);
        assert_eq!(run.link(), Link::Offline, "but it answered nothing");
    }

    /// The suffix is inferred from the transport, so it steps aside whenever the
    /// provider has given its own reason, and stays aside through the retry
    /// loop's notices.
    #[test]
    fn retrying_is_never_claimed_beside_a_provider_report() {
        let mut run = Run::new();
        run.transport = Transport::Reconnecting;
        run.verdict = Verdict::Failed {
            provider_says: Some("Start the bridge by hand"),
        };
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay().expect("up").meta, "Start the bridge by hand");
        run.verdict = Verdict::None;
        run.step(100);
        assert_eq!(run.overlay().expect("up").meta, "Start the bridge by hand");
    }

    /// Exactly one Reconnect on screen, wherever there is room for it.
    #[test]
    fn the_status_bar_takes_reconnect_when_the_overlay_has_no_room() {
        let mut run = Run::new();
        run.offline();
        let offers = |run: &Run| {
            (
                run.overlay().expect("up").reconnect,
                run.tracker.status_word(run.now).expect("a word").reconnect,
            )
        };
        assert_eq!(offers(&run), (true, false));
        run.tracker.set_overlay_control_fits(false);
        assert_eq!(offers(&run), (false, true));
    }

    /// A tab nobody is looking at is judged once a second, not once a frame.
    #[test]
    fn a_background_tab_is_observed_once_a_second() {
        let mut run = Run::new();
        assert!(ConnectionTracker::new().background_due(run.now));
        assert!(!run.tracker.background_due(run.now));
        assert!(
            !run.tracker
                .background_due(run.now + BACKGROUND_OBSERVE_MS - 1)
        );
        assert!(run.tracker.background_due(run.now + BACKGROUND_OBSERVE_MS));
        run.step(BACKGROUND_OBSERVE_MS);
        assert!(!run.tracker.background_due(run.now));
    }

    /// A blip shorter than the tier shows nothing anywhere: no chip, no word
    /// change, on a chart with bars.
    #[test]
    fn a_short_blip_shows_nothing() {
        let mut run = Run::new();
        run.step(BLIP_MS);
        run.connected();
        assert_eq!(run.word(), Some("live"));
        assert_eq!(run.tracker.chip(), None, "all well, nothing drawn");
        run.transport = Transport::Reconnecting;
        for _ in 0..4 {
            run.step(BLIP_MS / 4 - 1);
            assert_eq!(run.word(), Some("live"));
            assert_eq!(run.tracker.chip(), None);
            assert!(!run.pulsing());
        }
        run.transport = Transport::Connected;
        run.step(10);
        assert_eq!(run.word(), Some("live"));
        // The same drop, standing past the tier, is spoken.
        run.transport = Transport::Reconnecting;
        run.step(10);
        run.step(BLIP_MS);
        let word = run.tracker.status_word(run.now).expect("a word");
        assert_eq!(word.word, "reconnecting");
        assert!(word.pulses);
        assert_eq!(run.tracker.state().feed, Stream::Connecting);
        let dots = run.tracker.chip().expect("the chip is up");
        assert_eq!(dots[0].look, DotLook::Pulsing);
        assert_eq!(dots[0].hover, "feed connecting");
    }

    /// A session opening says nothing for its first moments, so a launch that
    /// lands in a second has never mentioned the connection at all.
    #[test]
    fn a_session_that_lands_quickly_never_mentions_the_connection() {
        let mut run = Run::new();
        assert_eq!(run.word(), None);
        assert_eq!(run.tracker.chip(), None);
        assert_eq!(run.overlay(), None);
        run.step(BLIP_MS - 1);
        assert_eq!(run.word(), None);
        assert_eq!(run.tracker.chip(), None);
        assert_eq!(run.overlay(), None);
        run.connected();
        assert_eq!(run.word(), Some("live"));
    }

    #[test]
    fn the_overlay_waits_two_seconds_and_says_what_is_true_in_each_state() {
        let mut run = Run::new();
        run.step(OVERLAY_DELAY_MS - 1);
        assert_eq!(run.overlay(), None);
        run.step(1);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "Connecting to");
        assert_eq!(words.meta, "Binance · attempting");
        assert!(words.breathing && !words.reconnect);

        run.verdict = FAILED;
        run.step(10);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "No connection to");
        assert_eq!(
            words.meta,
            "Binance · never connected · retrying automatically"
        );
        assert!(words.reconnect && !words.breathing);
        // The overlay's control is the one on offer: never both.
        let word = run.tracker.status_word(run.now).expect("a word");
        assert!(!word.reconnect && word.age.is_none());

        // Live with nothing: "connection" would be false.
        run.transport = Transport::Connected;
        run.verdict = Verdict::None;
        run.step(0);
        run.step(CONNECTED_HOLD_MS);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "No trades yet on");
        assert_eq!(words.meta, "Binance · connected");
        assert!(!words.reconnect);

        // Quiet is not offline, and offers no Reconnect.
        run.verdict = Verdict::Quiet;
        run.step(10);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "No trades on");
        assert_eq!(words.meta, "Binance connected · market may be closed");
        assert!(!words.reconnect);
        assert_eq!(run.word(), Some("quiet"));

        // Gone on the first bar.
        run.empty_pane = false;
        run.any_bars = true;
        run.step(1);
        assert_eq!(run.overlay(), None);
    }

    #[test]
    fn a_lost_connection_is_dated_and_a_provider_reason_replaces_the_meta() {
        let mut run = Run::new();
        run.transport = Transport::Connected;
        run.step(5);
        run.transport = Transport::Reconnecting;
        run.verdict = FAILED;
        run.step(180_000);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(
            run.overlay().expect("up").meta,
            "Binance · last connected 3 min ago · retrying automatically"
        );
        run.verdict = Verdict::Failed {
            provider_says: Some("MetaTrader 5 is closed"),
        };
        run.step(1);
        assert_eq!(run.overlay().expect("up").meta, "MetaTrader 5 is closed");
    }

    /// Every surface keeps what a blip interrupted: the overlay does not flash
    /// an attempt the status word and the chip are still keeping quiet about.
    #[test]
    fn a_blip_under_the_overlay_keeps_its_words() {
        let mut run = Run::new();
        run.transport = Transport::Connected;
        run.verdict = Verdict::Quiet;
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay().expect("up").verb, "No trades on");
        run.transport = Transport::Reconnecting;
        run.verdict = Verdict::None;
        run.step(0);
        run.step(BLIP_MS - 1);
        assert_eq!(run.link(), Link::Attempting);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "No trades on", "the blip says nothing new");
        assert!(!words.breathing);
        assert_eq!(run.word(), Some("quiet"));
        run.step(1);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "Reconnecting to", "standing, it is spoken");
        assert!(words.breathing);
        assert_eq!(run.word(), Some("reconnecting"));
    }

    /// The overlay is about a tape with no market data at all. A time pane that
    /// is only awaiting candles beside a working flow pane says nothing, and
    /// neither does one after the tape has seen a print.
    #[test]
    fn only_a_tape_with_no_market_data_raises_the_overlay() {
        let mut run = Run::new();
        run.flow_bars = true;
        run.step(0);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay(), None, "flow has bars");

        let mut run = Run::new();
        run.trades = 1;
        run.step(0);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay(), None, "a print arrived");

        let mut run = Run::new();
        run.step(OVERLAY_DELAY_MS);
        assert!(run.overlay().is_some(), "nothing at all has arrived");
        run.flow_bars = true;
        run.step(1);
        assert_eq!(run.overlay(), None, "gone on the flow's first bar");
    }

    /// While a job reads data in, "no trades" would be a claim nobody has
    /// checked: the slot speaks and the overlay stays down until it finishes.
    /// The connection's own words stay true, so they stay up.
    #[test]
    fn a_running_job_silences_the_overlay_claims_about_trades() {
        let mut run = Run::new();
        run.transport = Transport::Connected;
        run.jobs.set_active(LoadingTask::History, true);
        run.step(0);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.link(), Link::Live);
        assert_eq!(run.overlay(), None, "not `No trades yet`");
        assert!(run.tracker.state().current_job.is_some(), "the slot speaks");
        run.verdict = Verdict::Quiet;
        run.step(1);
        assert_eq!(run.overlay(), None, "not `No trades`");
        run.has_transport = false;
        run.step(1);
        assert_eq!(run.overlay(), None, "not `0 trades`");
        run.jobs.set_active(LoadingTask::History, false);
        run.step(1);
        assert_eq!(run.overlay().expect("up").verb, "No data for");

        let mut run = Run::new();
        run.jobs.set_active(LoadingTask::History, true);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay().expect("up").verb, "Connecting to");
    }

    /// A latch is about the symbol it fired on: the next one starts attempting,
    /// settling first.
    #[test]
    fn a_new_symbol_does_not_inherit_offline() {
        let mut run = Run::new();
        run.offline();
        assert_eq!(run.word(), Some("offline"));
        run.symbol = "ETHUSDT";
        run.attached = run.now + 1;
        run.transport = Transport::Connecting;
        run.verdict = Verdict::None;
        run.step(1);
        assert_eq!(run.link(), Link::Attempting);
        assert_eq!(run.word(), None, "not a press: it settles");
        assert_eq!(run.overlay(), None);
        run.step(BLIP_MS);
        assert_eq!(run.overlay().expect("up").verb, "Connecting to");
    }

    /// A reload on the same symbol empties the chart, so the overlay is owed,
    /// and it opens like any session: settling, then connecting, with no date
    /// carried from the timeline that was thrown away.
    #[test]
    fn a_reload_on_the_same_symbol_starts_from_nothing() {
        let mut run = Run::new();
        run.connected();
        run.trades = 5;
        run.step(10);
        run.offline();
        assert!(run.tracker.state().last_data_at.is_some());

        run.rebuild();
        run.step(1);
        let state = run.tracker.state();
        assert_eq!(state.link, Link::Attempting);
        assert_eq!(state.last_data_at, None);
        assert_eq!(state.last_connected_at, None);
        assert_eq!(run.word(), None, "not a press: it settles");
        assert_eq!(run.overlay(), None);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay().expect("up").verb, "Connecting to");
    }

    /// A same-symbol switch onto a venue that never connects says so: not
    /// `reconnecting`, and no date from the venue it left.
    #[test]
    fn a_same_symbol_feed_switch_is_not_a_press_and_keeps_no_date() {
        let mut run = Run::new();
        run.connected();
        run.trades = 5;
        run.step(720_000);

        run.rebuild();
        run.step(1);
        assert!(!run.pulsing(), "not an attempt a hand asked for");
        run.verdict = FAILED;
        run.step(OVERLAY_DELAY_MS);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "No connection to");
        assert_eq!(
            words.meta,
            "Binance · never connected · retrying automatically"
        );
    }

    /// A source rewinding resets the timeline with no new session: a replay's
    /// seek, which the tracker must see as much as a respawn.
    #[test]
    fn a_reset_without_a_respawn_clears_the_data_facts() {
        let mut run = Run::new();
        run.connected();
        run.trades = 5;
        run.step(10);
        run.timeline += 1;
        run.empty_pane = true;
        run.any_bars = false;
        run.step(1);
        assert_eq!(run.tracker.state().last_data_at, None);
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.overlay().expect("up").verb, "No trades yet on");
    }

    /// A Reconnect keeps the timeline, and so what the tracker knows about it,
    /// and is still the hand-pressed attempt shown at once.
    #[test]
    fn a_true_reconnect_stays_a_press_and_keeps_its_facts() {
        let mut run = Run::new();
        run.connected();
        run.trades = 5;
        run.step(10);
        let before = run.tracker.state();
        assert!(before.last_data_at.is_some());
        run.step(DATA_FLOWING_MS);
        run.press();
        run.step(1);
        let state = run.tracker.state();
        assert_eq!(state.last_data_at, before.last_data_at);
        assert!(state.last_connected_at.is_some());
        assert_eq!(run.word(), Some("reconnecting"));
        assert!(run.pulsing());
    }

    /// With bars on the chart: no overlay, and the word carries the age and the
    /// one-click Reconnect, with the chip up.
    #[test]
    fn offline_with_bars_goes_to_the_status_bar() {
        let mut run = Run::new();
        run.connected();
        run.trades = 5;
        run.step(10);
        run.transport = Transport::Reconnecting;
        run.verdict = FAILED;
        run.step(42_000);
        assert_eq!(run.overlay(), None);
        assert_eq!(
            run.tracker.status_word(run.now),
            Some(StatusWord {
                word: "offline",
                pulses: false,
                age: Some("0:42".to_owned()),
                reconnect: true,
            })
        );
        assert!(run.tracker.chip().is_some());
        assert!(run.tracker.has_recovery_door());
    }

    /// A press shows the attempt at once and for at least a second, however
    /// fast the new session fails, and failing returns to the latch.
    #[test]
    fn a_press_is_shown_at_once_and_holds() {
        let mut run = Run::new();
        run.verdict = FAILED;
        run.step(OVERLAY_DELAY_MS);
        assert_eq!(run.link(), Link::Offline);
        run.press();
        run.verdict = Verdict::Failed {
            provider_says: Some("the bridge refused"),
        };
        run.step(1);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "Reconnecting to", "even before any connection");
        assert!(words.breathing && !words.reconnect);
        assert_eq!(
            run.word(),
            Some("reconnecting"),
            "at once, not after a blip"
        );
        run.step(PRESS_HOLD_MS - 1);
        assert_eq!(run.link(), Link::Attempting);
        run.step(1);
        assert_eq!(run.link(), Link::Offline);
        assert!(run.overlay().expect("up").reconnect);
    }

    /// Vocabulary follows the capability: a source with no transport has no
    /// data, never a connection problem, and offers no Reconnect.
    #[test]
    fn a_source_with_no_transport_has_no_data_rather_than_no_connection() {
        let mut run = Run::new();
        run.has_transport = false;
        run.step(OVERLAY_DELAY_MS);
        let words = run.overlay().expect("up");
        assert_eq!(words.verb, "No data for");
        assert_eq!(words.meta, "Binance · 0 trades");
        assert!(!words.reconnect && !words.breathing);
        assert_eq!(run.word(), Some("no data"));
        assert_eq!(run.tracker.chip(), None);
        assert!(!run.tracker.has_recovery_door());
        // Still being read: not "no data".
        run.jobs.set_active(LoadingTask::ReplaySession, true);
        run.step(1);
        assert_eq!(run.overlay(), None);
        run.jobs.set_active(LoadingTask::ReplaySession, false);
        run.empty_pane = false;
        run.any_bars = true;
        run.step(1);
        assert_eq!(run.word(), Some("replay"));
    }

    /// The chip's mapping: book first, one row per capture status.
    #[test]
    fn the_book_maps_every_capture_status() {
        for (status, book, word) in [
            (CaptureStatus::Disabled, Book::Off, "book off"),
            (
                CaptureStatus::Connecting,
                Book::Connecting,
                "book connecting",
            ),
            (
                CaptureStatus::Buffering,
                Book::Connecting,
                "book connecting",
            ),
            (
                CaptureStatus::SnapshotFetching,
                Book::Connecting,
                "book connecting",
            ),
            (
                CaptureStatus::Resyncing { reason: "gap" },
                Book::Connecting,
                "book connecting",
            ),
            (
                CaptureStatus::Live {
                    generation: 1,
                    last_update_id: 1,
                },
                Book::Synced,
                "book synced",
            ),
            (
                CaptureStatus::Disconnected { error_class: "x" },
                Book::Lost,
                "book lost",
            ),
            (CaptureStatus::Error, Book::Lost, "book lost"),
        ] {
            assert_eq!(Book::of(&status), book);
            assert_eq!(book_dot(book).hover, word);
        }
    }

    #[test]
    fn the_chip_comes_up_for_a_stream_that_is_not_well_and_off_alone_does_not_raise_it() {
        let mut run = Run::new();
        run.step(BLIP_MS);
        run.book = Book::Off;
        run.connected();
        assert_eq!(run.tracker.chip(), None, "book off alone raises nothing");
        run.book = Book::Connecting;
        run.step(0);
        run.step(BLIP_MS - 1);
        assert_eq!(run.tracker.chip(), None, "a book blip shows nothing");
        run.step(1);
        let dots = run.tracker.chip().expect("standing sync raises it");
        assert_eq!(dots[1].look, DotLook::Pulsing);
        assert_eq!(dots[0].look, DotLook::Solid, "only the book moves");
        assert_eq!(dots[2].look, DotLook::Solid);
        run.book = Book::Synced;
        run.step(1);
        assert_eq!(run.tracker.chip(), None, "well again at once");
        run.book = Book::Lost;
        run.step(0);
        run.step(BLIP_MS);
        let dots = run.tracker.chip().expect("up");
        assert_eq!(dots[1].hover, "book lost");
        assert_eq!(dots[1].look, DotLook::Faint);
    }

    #[test]
    fn offline_is_not_syncing_the_book() {
        let mut run = Run::new();
        run.jobs.set_active(LoadingTask::BookSync, true);
        run.step(0);
        run.step(SLOT_DELAY_MS);
        assert!(run.tracker.state().current_job.is_some());
        run.verdict = FAILED;
        run.step(SLOT_HOLD_MS);
        assert_eq!(run.tracker.state().current_job, None);
    }

    /// Nothing tries while the feed is offline, so the book does not pulse.
    #[test]
    fn the_book_does_not_pulse_on_a_feed_that_is_offline() {
        let mut run = Run::new();
        run.book = Book::Connecting;
        run.verdict = FAILED;
        run.step(BLIP_MS);
        run.step(BLIP_MS);
        assert_eq!(run.tracker.state().book, Book::Idle);
        assert_eq!(run.tracker.chip().expect("up")[1].hover, "book idle");
    }

    #[test]
    fn the_slot_waits_a_second_holds_half_a_second_and_the_newest_job_wins() {
        let mut run = Run::new();
        let job = |run: &Run| run.tracker.state().current_job.map(|job| job.task);
        run.jobs.begin(LoadingTask::History);
        run.step(0);
        run.step(SLOT_DELAY_MS - 1);
        assert_eq!(job(&run), None, "under a second, nothing");
        run.step(1);
        assert_eq!(job(&run), Some(LoadingTask::History));

        // A newer job replaces it once the hold has run, never stacked.
        run.jobs.set_active(LoadingTask::BookSync, true);
        run.step(0);
        run.step(SLOT_DELAY_MS);
        assert_eq!(job(&run), Some(LoadingTask::BookSync));

        // The job ending at once still leaves the word up long enough to read.
        run.jobs.set_active(LoadingTask::BookSync, false);
        run.jobs.set_active(LoadingTask::History, false);
        run.step(SLOT_HOLD_MS - 1);
        assert_eq!(job(&run), Some(LoadingTask::BookSync));
        run.step(1);
        assert_eq!(job(&run), None);

        // A job that ends before its second never appeared at all.
        run.jobs.set_active(LoadingTask::BarRebuild, true);
        run.step(0);
        run.step(SLOT_DELAY_MS - 1);
        run.jobs.set_active(LoadingTask::BarRebuild, false);
        run.step(1);
        assert_eq!(job(&run), None);
    }

    #[test]
    fn a_percent_is_only_ever_a_measured_one() {
        let job = |fraction| Job {
            task: LoadingTask::BarRebuild,
            fraction,
        };
        assert_eq!(job(None).text(), "rebuilding bars");
        assert_eq!(job(Some(0.62)).text(), "rebuilding bars 62%");
        assert_eq!(job(Some(1.7)).text(), "rebuilding bars 100%");
    }

    #[test]
    fn another_symbol_starts_from_nothing() {
        let mut run = Run::new();
        run.transport = Transport::Connected;
        run.step(10);
        assert!(run.tracker.state().last_connected_at.is_some());
        let now = run.now + 1;
        run.tracker.observe(
            &ConnectionFacts {
                now_ms: now,
                symbol: "ETHUSDT",
                attached_ms: now,
                timeline_generation: 1,
                has_transport: true,
                transport: Transport::Connecting,
                verdict: Verdict::None,
                live_trades: 0,
                empty_pane: true,
                any_bars: false,
                flow_bars: false,
                book: Book::Off,
                fraction: None,
            },
            &run.jobs,
        );
        assert_eq!(run.tracker.state().last_connected_at, None);
        assert_eq!(
            run.tracker.status_word(now),
            None,
            "not a press: it settles"
        );
    }

    #[test]
    fn a_running_job_outranks_the_remark_in_the_slot() {
        let mut run = Run::new();
        assert_eq!(run.tracker.slot(None), None);
        let note = Some("history reached its end");
        assert_eq!(
            run.tracker.slot(note),
            Some(("history reached its end".to_owned(), false))
        );
        run.jobs.begin(LoadingTask::History);
        run.step(0);
        run.step(SLOT_DELAY_MS);
        assert_eq!(
            run.tracker.slot(note),
            Some(("loading history".to_owned(), true))
        );
    }

    #[test]
    fn durations_read_as_said() {
        assert_eq!(spoken_ago(42_000), "42 s");
        assert_eq!(spoken_ago(180_000), "3 min");
        assert_eq!(spoken_ago(3 * 3_600_000), "3 h");
        assert_eq!(clock_age(42_000), "0:42");
        assert_eq!(clock_age(3_729_000), "1:02:09");
        assert_eq!(clock_age(-5), "0:00");
    }
}
