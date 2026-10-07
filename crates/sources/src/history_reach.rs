//! How far one press of *History* reaches into the past.
//!
//! The trade half of the chart's history is paged, never spanned: every
//! transport that serves it — the MetaTrader bridge's `load_older`, Binance's
//! `aggTrades` window — takes a **count** and a cursor, because that is what a
//! venue will answer. A trader does not think in counts. They think "show me
//! yesterday" or "two more hours", and one page of a hundred thousand prints
//! is an hour of a liquid contract.
//!
//! This module is the bridge between the two: a [`HistoryReach`] target the
//! trader picks, and a [`Campaign`] that spends pages until the target is met.
//! The campaign owns no channel and no clock — it is told what the chart held
//! at the press and what each raw page brought, and answers what to do next —
//! so every stop condition is a unit test rather than a session with a live
//! venue. It judges the **raw page**, never the rebuilt chart, so the next
//! request goes out while the chart is still being rebuilt behind the frame.
//!
//! # Where a session's open comes from
//!
//! Not from a calendar. quantick has no venue session table and inventing one
//! would be a second source of truth about every exchange's hours, wrong the
//! first time a holiday moved. The tape already says it: a stretch with **no
//! prints at all** longer than [`SESSION_GAP_MS`] is the market having been
//! closed, and the print on its newer side is the next session's open. A
//! market that never closes shows no such stretch, and there a day is
//! [`DAY_OF_TAPE_MS`] of tape — which the note says out loud.
//!
//! A silence the feed itself marked as lost — a reconnect, a confirmed
//! source gap — is an [`Outage`], not a close: the market traded through it
//! and nobody was listening. The tab hands its marked outages in with
//! [`TapeFacts`], and a silence they explain is crossed without counting a
//! session, and said in the note. What stays out of reach, and is pinned by a
//! test: a hole nobody marked (one older than this run of the app, or one in
//! the venue's own record) still reads as a close, and an outage that spans a
//! real close hides that close.

use quantick_engine::Trade;
use quantick_engine::trade_tape::TradeSeq;

/// A stretch with no prints longer than this reads as the market having been
/// closed rather than as a quiet patch.
///
/// One hour. B3's index future — the tape this was sized against — runs
/// 09:00–18:25 with no break and reopens the next morning, so the overnight
/// stretch is around fourteen hours and the quietest in-session minute is
/// nowhere near an hour. A venue with a real lunch break longer than this
/// reads that break as a close, which costs the trader one extra session of
/// reach and never invents data.
pub const SESSION_GAP_MS: i64 = 60 * 60 * 1_000;

/// The old *previous session* reach's lead past a close. Only the config
/// default reads it now: the session targets land on a session's open.
pub const PREVIOUS_SESSION_LEAD_MS: i64 = 3 * 60 * 60 * 1_000;

const HOUR_MS: i64 = 60 * 60 * 1_000;

/// What one "day" is on a feed whose tape never shows a close: twenty-four
/// hours of traded time, counted back from the live edge.
pub const DAY_OF_TAPE_MS: i64 = 24 * HOUR_MS;

/// The most hours one `hours:N` target may ask for: [`MAX_CAMPAIGN_SPAN_MS`].
pub const MAX_REACH_HOURS: u32 = (MAX_CAMPAIGN_SPAN_MS / HOUR_MS) as u32;

/// The most sessions one `sessions:N` target may ask for.
pub const MAX_REACH_SESSIONS: u32 = 10;

/// Prints requested per campaign page on transports that stream large blocks.
/// This stays below the MetaTrader bridge's 200,000-print cap per request.
pub const CAMPAIGN_PAGE_PRINTS: usize = 100_000;

/// The densest B3 session measured on this host: WINV26, 1,525,621 prints.
pub const MEASURED_DENSE_SESSION_PRINTS: usize = 1_525_621;

/// Prints one session of a `sessions:N` target may pull: the measured dense
/// session with about two-thirds headroom for a busier day.
pub const PRINTS_PER_SESSION_BUDGET: usize = 2_500_000;

/// Prints one traded hour of an `hours:N` target may pull: the measured dense
/// session averages 162,000 an hour, and its opening hour runs about three
/// times that.
pub const PRINTS_PER_TRADED_HOUR_BUDGET: usize = 500_000;

/// What one held print costs a chart pane, measured: 56 bytes of tape plus
/// about 90 of footprint ladders (`docs/quality/live-envelope.md`).
pub const BYTES_PER_HELD_PRINT: usize = 146;

/// The memory a tab's tapes may grow to through history runs, **every pane's
/// copy together**: each pane holds its own copy of the tape, so a split with
/// a time pane spends this twice as fast (see [`TapeFacts::copies`]).
pub const HELD_TAPE_CEILING_BYTES: usize = 4 * 1024 * 1024 * 1024;

/// [`HELD_TAPE_CEILING_BYTES`] in prints, across every copy: about
/// twenty-nine million, which holds five dense B3 sessions and today on a
/// three-pane tab (a flow pane and two context panes), the trader's WIN setup.
/// A fourth pane stops a five-day run at [`CampaignEnd::MemoryCeiling`], and
/// says so.
pub const MAX_HELD_PRINTS: usize = HELD_TAPE_CEILING_BYTES / BYTES_PER_HELD_PRINT;

// A measured dense session fits one session's budget, and five of them plus
// today fit under the ceiling on three panes: checked when the crate compiles.
const _: () = assert!(PRINTS_PER_SESSION_BUDGET >= MEASURED_DENSE_SESSION_PRINTS);
const _: () = assert!(3 * 6 * MEASURED_DENSE_SESSION_PRINTS < MAX_HELD_PRINTS);

/// Requests beyond the print budget's own, for replies that cross dead time
/// (a weekend, a holiday) and bring nothing.
pub const PAGE_SLACK: u32 = 8;

/// The most requests one run may make, whatever its page size: a venue with
/// small pages is not asked thousands of times for one press.
pub const MAX_CAMPAIGN_PAGES: u32 = 512;

/// Replies in a row that may bring nothing new before a campaign gives up.
///
/// An empty page is not by itself the end of the record — a bridge crossing a
/// weekend searches hours and maps no trades — while a venue that answers
/// empty because it is rate-limiting costs three requests instead of hundreds.
pub const MAX_IDLE_PAGES: u32 = 3;

/// The longest traded span one run may ask for, two days: the hours ceiling,
/// and the bridge's own opening walk on a market that never closes.
pub const MAX_CAMPAIGN_SPAN_MS: i64 = 48 * 60 * 60 * 1_000;

/// What a legacy `span` token reaches by until the config says otherwise.
pub const DEFAULT_REACH_SPAN_MS: i64 = 2 * HOUR_MS;

/// What a press is told when its request could not even be queued.
pub const REQUEST_REFUSED_NOTICE: &str = "could not ask for older trades just now; press again";

/// The bound a [`Campaign`] measures sessions with, as the trader's
/// configuration set it (`[history] session_gap_minutes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachBounds {
    /// See [`SESSION_GAP_MS`].
    pub session_gap_ms: i64,
}

impl Default for ReachBounds {
    fn default() -> Self {
        Self {
            session_gap_ms: SESSION_GAP_MS,
        }
    }
}

/// A stretch the feed itself marked as lost: no prints, and the market not
/// closed. Bounds as the feed stamped them, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outage {
    /// The last print or message before the silence.
    pub from_ms: i64,
    /// The first one after it.
    pub to_ms: i64,
}

/// What the tab knows about its tape beyond the prints a target is judged on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TapeFacts {
    /// Panes each holding their own copy of the tape; the memory ceiling
    /// counts every copy. Zero reads as one.
    pub copies: usize,
    /// Silences the feed marked as lost, which are crossed without counting
    /// a session close.
    pub outages: Vec<Outage>,
}

/// How far one press of *History* reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryReach {
    /// This many more hours of **traded** time before the oldest print the
    /// chart holds. Nights and weekends are crossed, never counted.
    Hours(u32),
    /// Back to the open of the Nth previous session, counted from the live
    /// edge: `Sessions(1)` is yesterday's open.
    Sessions(u32),
}

impl Default for HistoryReach {
    /// What the main click does on a tab that never pressed: yesterday.
    fn default() -> Self {
        Self::Sessions(1)
    }
}

/// Why a target's text was not understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachParseError {
    /// Nothing was given.
    Empty,
    /// The part before `:` is neither `hours` nor `sessions`.
    UnknownKind(String),
    /// The count is not a whole number.
    BadCount(String),
    /// The count is outside `1..=max`.
    OutOfRange {
        kind: &'static str,
        count: u32,
        max: u32,
    },
}

impl std::fmt::Display for ReachParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const GRAMMAR: &str = "a reach is `hours:N` or `sessions:N`";
        match self {
            Self::Empty => write!(f, "no reach given; {GRAMMAR}"),
            Self::UnknownKind(kind) => write!(f, "unknown reach `{kind}`; {GRAMMAR}"),
            Self::BadCount(count) => {
                write!(f, "`{count}` is not a whole number; {GRAMMAR}")
            }
            Self::OutOfRange { kind, count, max } => {
                write!(f, "{kind}:{count} is outside 1..={max}")
            }
        }
    }
}

impl std::error::Error for ReachParseError {}

impl HistoryReach {
    /// The menu's one-click actions, in the order it offers them.
    pub const PRESETS: [Self; 5] = [
        Self::Hours(2),
        Self::Hours(4),
        Self::Sessions(1),
        Self::Sessions(3),
        Self::Sessions(5),
    ];

    /// The words on the control.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Hours(hours) => format!("+{hours} h"),
            Self::Sessions(1) => "Yesterday".to_owned(),
            Self::Sessions(days) => format!("{days} days"),
        }
    }

    /// What one press of this target promises, for the hover text.
    #[must_use]
    pub fn hover(self) -> String {
        match self {
            Self::Hours(hours) => format!(
                "{hours} more hours of traded time before the oldest loaded print; \
                 nights and weekends are crossed, not counted"
            ),
            Self::Sessions(days) => format!(
                "back to the open of the session {days} before today's, found from the \
                 tape's overnight gaps (24 h of tape per day on a market that never closes)"
            ),
        }
    }

    /// The stable token a workspace, a hook and the control plane use.
    #[must_use]
    pub fn token(self) -> String {
        match self {
            Self::Hours(hours) => format!("hours:{hours}"),
            Self::Sessions(sessions) => format!("sessions:{sessions}"),
        }
    }

    /// The nearest token of the earlier reach menu, for the frozen v1
    /// `workspace.summary` field that documents them: `span` for an hours
    /// target (its minutes are the hours), `previous-session` for yesterday,
    /// and `None` for a target that vocabulary has no word for.
    #[must_use]
    pub const fn legacy_token(self) -> Option<&'static str> {
        match self {
            Self::Hours(_) => Some("span"),
            Self::Sessions(1) => Some("previous-session"),
            Self::Sessions(_) => None,
        }
    }

    /// Read a target back: `hours:N`, `sessions:N`, or one of the tokens the
    /// earlier reach menu saved (`page`, `previous-session`, `span`).
    pub fn parse(text: &str) -> Result<Self, ReachParseError> {
        let text = text.trim();
        match text {
            "" => return Err(ReachParseError::Empty),
            // One page was a couple of minutes of WIN: the smallest target.
            "page" => return Ok(Self::Hours(2)),
            "previous-session" => return Ok(Self::Sessions(1)),
            "span" => return Ok(Self::Hours(hours_of(DEFAULT_REACH_SPAN_MS / 60_000))),
            _ => {}
        }
        let Some((kind, count)) = text.split_once(':') else {
            return Err(ReachParseError::UnknownKind(text.to_owned()));
        };
        let (kind, count) = (kind.trim(), count.trim());
        let (name, max, build): (&'static str, u32, fn(u32) -> Self) = match kind {
            "hours" => ("hours", MAX_REACH_HOURS, Self::Hours),
            "sessions" => ("sessions", MAX_REACH_SESSIONS, Self::Sessions),
            other => return Err(ReachParseError::UnknownKind(other.to_owned())),
        };
        let count: u32 = count
            .parse()
            .map_err(|_| ReachParseError::BadCount(count.to_owned()))?;
        if !(1..=max).contains(&count) {
            return Err(ReachParseError::OutOfRange {
                kind: name,
                count,
                max,
            });
        }
        Ok(build(count))
    }

    /// [`Self::parse`], or `None` for text that is not a target.
    #[must_use]
    pub fn from_token(text: &str) -> Option<Self> {
        Self::parse(text).ok()
    }

    /// Read a saved token whose legacy `span` meant the saved minutes.
    #[must_use]
    pub fn from_legacy_span(text: &str, span_minutes: u32) -> Option<Self> {
        if text.trim() == "span" {
            return Some(Self::Hours(hours_of(i64::from(span_minutes))));
        }
        Self::from_token(text)
    }

    /// Prints this target may pull before it stops partial.
    ///
    /// A session target crosses into one more session to see the close that
    /// proves the Nth one's open, so it is budgeted one session more.
    #[must_use]
    pub fn print_budget(self) -> usize {
        match self {
            Self::Hours(hours) => hours as usize * PRINTS_PER_TRADED_HOUR_BUDGET,
            Self::Sessions(sessions) => (sessions as usize + 1) * PRINTS_PER_SESSION_BUDGET,
        }
    }

    /// Requests this target may make with pages of `page_size`.
    #[must_use]
    pub fn page_budget(self, page_size: usize) -> u32 {
        let pages = self.print_budget().div_ceil(page_size.max(1));
        u32::try_from(pages)
            .unwrap_or(u32::MAX)
            .saturating_add(PAGE_SLACK)
            .min(MAX_CAMPAIGN_PAGES)
    }
}

/// Whole hours covering `minutes`, at least one and at most the ceiling.
fn hours_of(minutes: i64) -> u32 {
    u32::try_from((minutes.max(1) + 59) / 60)
        .unwrap_or(MAX_REACH_HOURS)
        .clamp(1, MAX_REACH_HOURS)
}

/// Why a run stopped, in the words its log line uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignEnd {
    /// The target is on the chart.
    ReachMet,
    /// The target was already on the chart; nothing was asked.
    AlreadyThere,
    /// The feed withdrew paging: the venue has nothing older.
    Exhausted,
    /// The run's request budget is spent. Pressing again continues.
    PagesSpent,
    /// The run's print budget is spent. Pressing again continues.
    PrintsPulled,
    /// The tab's tapes, every pane's copy together, reached
    /// [`MAX_HELD_PRINTS`].
    MemoryCeiling,
    /// [`MAX_IDLE_PAGES`] replies in a row brought nothing new.
    NothingComingBack,
    /// The chart holds no trades, so there is nothing to page back from.
    NothingCharted,
    /// The trader (or a control call) stopped it.
    Cancelled,
    /// A request could not be queued.
    RequestRefused,
}

impl CampaignEnd {
    /// Every ending, in declaration order.
    pub const ALL: [Self; 10] = [
        Self::ReachMet,
        Self::AlreadyThere,
        Self::Exhausted,
        Self::PagesSpent,
        Self::PrintsPulled,
        Self::MemoryCeiling,
        Self::NothingComingBack,
        Self::NothingCharted,
        Self::Cancelled,
        Self::RequestRefused,
    ];

    /// The `action` field of the log line that records this ending.
    #[must_use]
    pub const fn action(self) -> &'static str {
        match self {
            Self::ReachMet => "reach_met",
            Self::AlreadyThere => "already_loaded",
            Self::Exhausted => "venue_exhausted",
            Self::PagesSpent => "page_budget_spent",
            Self::PrintsPulled => "print_budget_spent",
            Self::MemoryCeiling => "memory_ceiling",
            Self::NothingComingBack => "nothing_coming_back",
            Self::NothingCharted => "nothing_charted",
            Self::Cancelled => "cancelled",
            Self::RequestRefused => "request_refused",
        }
    }

    /// Read an ending back from its [`action`](Self::action) token.
    #[must_use]
    pub fn from_action(action: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|end| end.action() == action.trim())
    }

    /// The reason in the trader's words, as the note's middle clause.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ReachMet => "target reached",
            Self::AlreadyThere => "already on the chart",
            Self::Exhausted => "the venue has nothing older",
            Self::PagesSpent => "request budget spent; press again to continue",
            Self::PrintsPulled => "print budget spent; press again to continue",
            Self::MemoryCeiling => "memory ceiling reached",
            Self::NothingComingBack => "nothing older came back; give it a moment",
            Self::NothingCharted => "no bars to page back from yet",
            Self::Cancelled => "cancelled",
            Self::RequestRefused => REQUEST_REFUSED_NOTICE,
        }
    }

    /// Whether the target is on the chart.
    #[must_use]
    pub const fn complete(self) -> bool {
        matches!(self, Self::ReachMet | Self::AlreadyThere)
    }
}

/// What a campaign does after one page lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignStep {
    /// Ask for another page.
    Ask,
    /// Stop, for this reason.
    Stop(CampaignEnd),
}

/// Where a run stands, for the button, the note and the control plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachProgress {
    /// The target being served.
    pub reach: HistoryReach,
    /// The oldest print held or fetched so far.
    pub oldest_ms: i64,
    /// Previous sessions whose open a close behind it has proven.
    pub sessions_reached: u32,
    /// Traded time counted: from the live edge for a session target, from
    /// the oldest print at the press for an hours target.
    pub traded_ms: i64,
    /// Prints this run has pulled.
    pub prints_pulled: usize,
    /// Requests this run has made.
    pub pages: u32,
}

/// How a run ended, with everything its note says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachOutcome {
    /// The target that was served.
    pub reach: HistoryReach,
    /// Why it stopped.
    pub end: CampaignEnd,
    /// The oldest print the chart holds, or `None` on an empty chart.
    pub oldest_ms: Option<i64>,
    /// The open the target named, when a close proved it.
    pub reached_open_ms: Option<i64>,
    /// Previous sessions whose open is proven on the chart.
    pub sessions_reached: u32,
    /// Traded time counted (see [`ReachProgress::traded_ms`]).
    pub traded_ms: i64,
    /// Whether the tape showed no close, so a day meant 24 h of tape.
    pub gapless: bool,
    /// Silences longer than a close that the feed had marked as outages,
    /// crossed without counting a session.
    pub outages_crossed: u32,
}

impl ReachOutcome {
    /// An ending with nothing counted, from the chart's oldest print: what a
    /// capture hook raises to photograph an ending's sentence.
    #[must_use]
    pub const fn ended(reach: HistoryReach, end: CampaignEnd, oldest_ms: Option<i64>) -> Self {
        Self {
            reach,
            end,
            oldest_ms,
            reached_open_ms: None,
            sessions_reached: 0,
            traded_ms: 0,
            gapless: false,
            outages_crossed: 0,
        }
    }

    /// Whether the target is on the chart.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.end.complete()
    }

    /// The note, with times written by the caller's clock face.
    pub fn sentence(&self, time: impl Fn(i64) -> String) -> String {
        let mut sentence = self.verdict(time);
        if self.outages_crossed > 0 && matches!(self.reach, HistoryReach::Sessions(_)) {
            sentence.push_str("; a feed outage on the way was not counted as a close");
        }
        sentence
    }

    fn verdict(&self, time: impl Fn(i64) -> String) -> String {
        let Some(oldest) = self.oldest_ms else {
            return format!("Nothing loaded \u{2014} {}", self.end.reason());
        };
        match self.end {
            CampaignEnd::AlreadyThere => format!("Already loaded back to {}", time(oldest)),
            CampaignEnd::ReachMet => format!(
                "Loaded back to {} ({})",
                time(self.reached_open_ms.unwrap_or(oldest)),
                self.target_words()
            ),
            end => format!(
                "Stopped at {} \u{2014} {} ({})",
                time(oldest),
                end.reason(),
                self.progress_words()
            ),
        }
    }

    fn target_words(&self) -> String {
        match self.reach {
            HistoryReach::Hours(hours) => format!("+{hours} h of trading"),
            HistoryReach::Sessions(days) if self.gapless => {
                format!("{days} \u{00d7} 24 h of tape")
            }
            HistoryReach::Sessions(1) => "1 session".to_owned(),
            HistoryReach::Sessions(days) => format!("{days} sessions"),
        }
    }

    fn progress_words(&self) -> String {
        match self.reach {
            HistoryReach::Hours(hours) => {
                format!("{} of {hours} h", traded_words(self.traded_ms))
            }
            HistoryReach::Sessions(days) => {
                format!("{} of {days} sessions", self.sessions_reached)
            }
        }
    }
}

/// `1 h 30 m`, `45 m`, `2 h`.
fn traded_words(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} m"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} m"),
    }
}

/// What a press finds before any request goes out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampaignStart {
    /// The target is not on the chart: page for it.
    Run(Campaign),
    /// The target is already on the chart; ask nothing.
    AlreadyMet(ReachOutcome),
    /// The chart is empty; there is nothing to page back from.
    NothingCharted(ReachOutcome),
    /// The tab's tapes are already at [`MAX_HELD_PRINTS`]; ask nothing.
    AtCeiling(ReachOutcome),
}

/// A run of *load older* requests that ends on a target rather than a count.
///
/// One outstanding request at a time — the MetaTrader protocol refuses a
/// second — so the campaign is a state machine driven by replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campaign {
    reach: HistoryReach,
    session_gap_ms: i64,
    page_size: usize,
    /// Panes holding a copy of the tape, at least one.
    copies: usize,
    outages: Vec<Outage>,
    outages_crossed: u32,
    /// The oldest print held or fetched; the next page continues from here.
    oldest_ms: i64,
    /// Session closes crossed behind the live edge.
    closes: u32,
    /// The open the target named, once a close proved it.
    reached_open_ms: Option<i64>,
    traded_ms: i64,
    held_at_start: usize,
    pulled: usize,
    pages: u32,
    idle_pages: u32,
    met: bool,
}

impl Campaign {
    /// Judge the chart at the press: the target may already be there, the
    /// chart may be empty, or a run starts from what it holds.
    ///
    /// Rate: **rare** — once per press. A session target walks back from the
    /// live edge and stops as soon as it is met, so it reads at most the
    /// sessions it asks for.
    pub fn start<T: TradeSeq + ?Sized>(
        held: &T,
        facts: &TapeFacts,
        reach: HistoryReach,
        bounds: ReachBounds,
        page_size: usize,
    ) -> CampaignStart {
        let (Some(oldest), Some(edge)) = (held.first(), held.get(held.len().wrapping_sub(1)))
        else {
            return CampaignStart::NothingCharted(ReachOutcome::ended(
                reach,
                CampaignEnd::NothingCharted,
                None,
            ));
        };
        let mut campaign = Self {
            reach,
            session_gap_ms: bounds.session_gap_ms,
            page_size: page_size.clamp(1, CAMPAIGN_PAGE_PRINTS),
            copies: facts.copies.max(1),
            outages: facts.outages.clone(),
            outages_crossed: 0,
            oldest_ms: edge.timestamp_ms,
            closes: 0,
            reached_open_ms: None,
            traded_ms: 0,
            held_at_start: held.len(),
            pulled: 0,
            pages: 0,
            idle_pages: 0,
            met: false,
        };
        // Hours are counted behind the oldest print, so nothing held counts;
        // sessions are counted from the live edge, through what is held.
        if let HistoryReach::Sessions(_) = reach {
            for index in (0..held.len().saturating_sub(1)).rev() {
                match held.get(index) {
                    Some(trade) if !campaign.take(trade.timestamp_ms) => {}
                    _ => break,
                }
            }
        }
        campaign.oldest_ms = oldest.timestamp_ms;
        if campaign.met {
            return CampaignStart::AlreadyMet(campaign.finish(CampaignEnd::AlreadyThere));
        }
        if campaign.held_at_start >= campaign.ceiling() {
            return CampaignStart::AtCeiling(campaign.finish(CampaignEnd::MemoryCeiling));
        }
        CampaignStart::Run(campaign)
    }

    /// Prints one copy of the tape may hold: the ceiling shared by every
    /// pane's copy.
    fn ceiling(&self) -> usize {
        MAX_HELD_PRINTS / self.copies
    }

    /// How much of the silence between `older_ms` and `newer_ms` a marked
    /// outage explains. Rate: **rare** — only for a silence longer than a
    /// close, over a handful of remembered outages.
    fn explained_ms(&self, older_ms: i64, newer_ms: i64) -> i64 {
        self.outages
            .iter()
            .map(|outage| (outage.to_ms.min(newer_ms) - outage.from_ms.max(older_ms)).max(0))
            .sum()
    }

    /// Count one print no newer than everything judged so far. Reports
    /// whether the target is now met.
    fn take(&mut self, older_ms: i64) -> bool {
        let step = self.oldest_ms.saturating_sub(older_ms);
        let lost = step > self.session_gap_ms
            && step.saturating_sub(self.explained_ms(older_ms, self.oldest_ms))
                <= self.session_gap_ms;
        if lost {
            // The market traded through it; nobody was listening.
            self.outages_crossed = self.outages_crossed.saturating_add(1);
            self.traded_ms = self.traded_ms.saturating_add(step);
        } else if step > self.session_gap_ms {
            self.closes = self.closes.saturating_add(1);
            if let HistoryReach::Sessions(days) = self.reach
                && self.closes == days.saturating_add(1)
            {
                // The newer side of this gap is the Nth previous session's open.
                self.reached_open_ms = Some(self.oldest_ms);
                self.met = true;
            }
        } else {
            self.traded_ms = self.traded_ms.saturating_add(step);
        }
        self.oldest_ms = older_ms;
        self.met |= match self.reach {
            HistoryReach::Hours(hours) => self.traded_ms >= i64::from(hours) * HOUR_MS,
            HistoryReach::Sessions(days) => {
                let days = i64::from(days);
                (self.closes == 0 && self.traded_ms >= days * DAY_OF_TAPE_MS)
                    || self.traded_ms >= (days + 1) * DAY_OF_TAPE_MS
            }
        };
        self.met
    }

    /// The size of the next request, counted as sent.
    pub fn next_request(&mut self) -> usize {
        self.pages = self.pages.saturating_add(1);
        let held = self.held_at_start.saturating_add(self.pulled);
        // Never zero: a run at the ceiling stops in `advance`, and a press at
        // it never starts.
        self.page_size
            .min(self.reach.print_budget().saturating_sub(self.pulled))
            .min(self.ceiling().saturating_sub(held))
            .max(1)
    }

    /// Judge one raw page, oldest first, as it came off the feed.
    ///
    /// `can_page` is the feed's own answer to whether another request could
    /// be served. A page that meets the target counts even when the venue
    /// says it was the last one.
    ///
    /// Rate: **rare** — once per reply, linear in the page.
    pub fn advance(&mut self, page: &[Trade], can_page: bool) -> CampaignStep {
        let before = self.oldest_ms;
        self.pulled = self.pulled.saturating_add(page.len());
        for trade in page.iter().rev() {
            if trade.timestamp_ms <= self.oldest_ms && self.take(trade.timestamp_ms) {
                break;
            }
        }
        if let Some(first) = page.first() {
            self.oldest_ms = self.oldest_ms.min(first.timestamp_ms);
        }
        if self.met {
            return CampaignStep::Stop(CampaignEnd::ReachMet);
        }
        if !can_page {
            return CampaignStep::Stop(CampaignEnd::Exhausted);
        }
        if self.oldest_ms < before {
            self.idle_pages = 0;
        } else {
            self.idle_pages = self.idle_pages.saturating_add(1);
            if self.idle_pages >= MAX_IDLE_PAGES {
                return CampaignStep::Stop(CampaignEnd::NothingComingBack);
            }
        }
        if self.held_at_start.saturating_add(self.pulled) >= self.ceiling() {
            return CampaignStep::Stop(CampaignEnd::MemoryCeiling);
        }
        if self.pulled >= self.reach.print_budget() {
            return CampaignStep::Stop(CampaignEnd::PrintsPulled);
        }
        if self.pages >= self.reach.page_budget(self.page_size) {
            return CampaignStep::Stop(CampaignEnd::PagesSpent);
        }
        CampaignStep::Ask
    }

    /// Where the run stands.
    #[must_use]
    pub fn progress(&self) -> ReachProgress {
        ReachProgress {
            reach: self.reach,
            oldest_ms: self.oldest_ms,
            sessions_reached: self.sessions_reached(),
            traded_ms: self.traded_ms,
            prints_pulled: self.pulled,
            pages: self.pages,
        }
    }

    fn sessions_reached(&self) -> u32 {
        match self.reach {
            HistoryReach::Hours(_) => 0,
            HistoryReach::Sessions(days) if self.met => days,
            HistoryReach::Sessions(days) => self.closes.saturating_sub(1).min(days),
        }
    }

    /// The outcome of stopping now, for this reason.
    #[must_use]
    pub fn finish(&self, end: CampaignEnd) -> ReachOutcome {
        ReachOutcome {
            reach: self.reach,
            end,
            oldest_ms: Some(self.oldest_ms),
            reached_open_ms: self.reached_open_ms,
            sessions_reached: self.sessions_reached(),
            traded_ms: self.traded_ms,
            gapless: self.closes == 0,
            outages_crossed: self.outages_crossed,
        }
    }
}

#[cfg(test)]
#[path = "history_reach_tests.rs"]
mod tests;
