//! One answer per symbol to "is this chart being fed, and what is it waiting
//! on?", read by every surface that says so: the header's chip and slot, the
//! overlay over an empty pane and the status word. The clock is told, never
//! read, so every threshold below is a table in the tests.
//!
//! The rule the trader set: messages about the connection look like a platform
//! bug when they flicker. So a short blip shows nothing, and offline **latches**:
//! once the stall has fired the state holds until data arrives or a hand presses
//! Reconnect, whatever the feed's automatic retries do in between.

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
    /// A print arrived since the last step.
    pub data_arrived: bool,
    /// A press's minimum attempt is still running.
    pub press_holding: bool,
}

/// The next link, from the last one and this frame's facts.
///
/// Only two things leave [`Link::Offline`] for an attempt: a press (a respawned
/// session, which holds) and data. An automatic retry changes `transport` or
/// the notice behind `failed` on every tick, and neither moves it, which is
/// what stops the words bouncing while the feed's own loop runs. A transport
/// that has landed clears the latch: "no connection" would then be false.
#[must_use]
pub fn next_link(previous: Link, facts: LinkFacts) -> Link {
    if !facts.has_transport {
        return Link::NoTransport;
    }
    if facts.press_holding {
        return Link::Attempting;
    }
    if facts.failed {
        return Link::Offline;
    }
    if facts.quiet {
        return Link::Quiet;
    }
    if facts.data_arrived || facts.transport == Transport::Connected {
        return Link::Live;
    }
    if previous == Link::Offline {
        Link::Offline
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
    pub has_transport: bool,
    pub transport: Transport,
    pub verdict: Verdict<'a>,
    /// Live prints seen on this tab, a count that only rises.
    pub live_trades: u64,
    /// Some pane is on screen with no bars.
    pub empty_pane: bool,
    /// The chart holds anything at all.
    pub any_bars: bool,
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
    state: ConnectionState,
    attached_ms: Option<i64>,
    /// When the current attach started, against which startup settles.
    settle_from_ms: i64,
    /// Set by a respawn on the same symbol: an attempt a hand asked for.
    pressed: bool,
    press_hold_until: i64,
    attempting_since: i64,
    live_trades: u64,
    book_shown: Book,
    book_raw: Book,
    book_raw_since: i64,
    provider_says: Option<String>,
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
            live_trades: 0,
            book_shown: Book::Off,
            book_raw: Book::Off,
            book_raw_since: 0,
            provider_says: None,
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

    /// Take this frame's facts. Allocates only when the symbol or the
    /// provider's own words changed.
    pub fn observe(&mut self, facts: &ConnectionFacts<'_>, jobs: &LoadingTracker) {
        let now = facts.now_ms;
        self.observe_attach(facts);
        let previous = self.state.link;
        let data_arrived = facts.live_trades > self.live_trades;
        self.live_trades = facts.live_trades;
        if data_arrived {
            self.state.last_data_at = Some(now);
        }
        let link = next_link(
            previous,
            LinkFacts {
                has_transport: facts.has_transport,
                transport: facts.transport,
                failed: matches!(facts.verdict, Verdict::Failed { .. }),
                quiet: matches!(facts.verdict, Verdict::Quiet),
                data_arrived,
                press_holding: now < self.press_hold_until,
            },
        );
        if link == Link::Attempting && previous != Link::Attempting {
            self.attempting_since = now;
        }
        self.retrying = facts.has_transport && facts.transport != Transport::Connected;
        self.settling = !self.pressed && now.saturating_sub(self.settle_from_ms) < BLIP_MS;
        if facts.transport == Transport::Connected {
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
        self.empty_since = facts
            .empty_pane
            .then(|| self.empty_since.unwrap_or(now));
        self.observe_jobs(jobs, facts.fraction, now);
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
        if new_symbol {
            facts.symbol.clone_into(&mut self.symbol);
            self.state.last_connected_at = None;
            self.state.last_data_at = None;
            self.state.feed = Stream::Idle;
            self.state.trades = Stream::Idle;
            self.provider_says = None;
            self.empty_since = None;
            self.live_trades = facts.live_trades;
        }
        if self.attached_ms != Some(facts.attached_ms) {
            let respawn = self.attached_ms.is_some() && !new_symbol;
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

    fn observe_jobs(&mut self, jobs: &LoadingTracker, fraction: Option<f32>, now: i64) {
        self.any_job = jobs.any_active();
        let mut newest: Option<(LoadingTask, i64)> = None;
        for task in LoadingTask::ALL {
            let started = &mut self.job_started[task.index()];
            if !jobs.is_active(task) {
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

    /// Whether the overlay stands over the empty panes this frame.
    #[must_use]
    pub fn overlay_up(&self, now_ms: i64) -> bool {
        let stood = self
            .empty_since
            .is_some_and(|since| now_ms.saturating_sub(since) >= OVERLAY_DELAY_MS);
        // "No data" about a recording still being read would be false.
        stood && !(self.state.link == Link::NoTransport && self.any_job)
    }

    /// Whether what the transport is doing may be spoken yet.
    fn attempt_shown(&self, now_ms: i64) -> bool {
        self.pressed || now_ms.saturating_sub(self.attempting_since) >= BLIP_MS
    }

    /// The overlay's words, or `None` when it is not up. Formats only then.
    #[must_use]
    pub fn overlay(&self, feed: &str, now_ms: i64) -> Option<OverlayWords> {
        if !self.overlay_up(now_ms) {
            return None;
        }
        let state = self.state;
        Some(match state.link {
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
                let meta = if self.retrying {
                    format!("{lead} · retrying automatically")
                } else {
                    lead
                };
                OverlayWords {
                    verb: "No connection to",
                    meta,
                    reconnect: true,
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
        match state.link {
            Link::Live if self.settling && state.last_connected_at.is_none() => None,
            Link::Live => Some(still("live")),
            Link::Quiet => Some(still("quiet")),
            Link::NoTransport if self.any_bars => Some(still("replay")),
            Link::NoTransport => Some(still("no data")),
            Link::Attempting if self.attempt_shown(now_ms) => Some(StatusWord {
                pulses: true,
                ..still("reconnecting")
            }),
            // A blip keeps the word it interrupted; a session opening has none.
            Link::Attempting => (state.feed == Stream::Ok).then(|| still("live")),
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
                    reconnect: !overlay,
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

    /// Whether anything on screen is moving and so owes a repaint.
    #[must_use]
    pub fn breathing(&self, now_ms: i64) -> bool {
        self.state.link == Link::Attempting && self.attempt_shown(now_ms)
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
