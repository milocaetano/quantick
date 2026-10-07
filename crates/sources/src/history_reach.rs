//! How far one press of *load older* reaches into the past.
//!
//! The trade half of the chart's history is paged, never spanned: every
//! transport that serves it — the MetaTrader bridge's `load_older`, Binance's
//! `aggTrades` window — takes a **count** and a cursor, because that is what a
//! venue will answer. A trader does not think in counts. They think "show me
//! back to yesterday", and one page of two thousand prints is minutes of a
//! liquid contract.
//!
//! This module is the bridge between the two: a [`HistoryReach`] the trader
//! picks, and a [`Campaign`] that spends pages until the reach is met. The
//! campaign owns no channel and no clock — it is told what the chart holds and
//! answers what to do next — so every stop condition is a unit test rather
//! than a session with a live venue.
//!
//! # Where a session's open comes from
//!
//! Not from a calendar. quantick has no venue session table and inventing one
//! would be a second source of truth about every exchange's hours, wrong the
//! first time a holiday moved. The tape already says it: a stretch with **no
//! prints at all** longer than [`SESSION_GAP_MS`] is the market having been
//! closed, and the print on its older side is that session's last. This is
//! observed rather than assumed, which is the data-honesty rule applied to
//! time — and it costs nothing on a market that never closes, where there is
//! simply no gap and the campaign ends on its span cap instead.

use quantick_engine::trade_tape::TradeSeq;

/// A stretch with no prints longer than this reads as the market having been
/// closed rather than as a quiet patch.
///
/// One hour. B3's index future — the tape this was sized against — runs
/// 09:00–18:25 with no break and reopens the next morning, so the overnight
/// stretch is around fourteen hours and the quietest in-session minute is
/// nowhere near an hour. A venue with a real lunch break longer than this
/// reads that break as a close, which costs the trader one extra press and
/// never invents data.
pub const SESSION_GAP_MS: i64 = 60 * 60 * 1_000;

/// How far past a session's last print [`HistoryReach::PreviousSession`] keeps
/// going, so the day before is on screen to compare against rather than
/// merely touched.
///
/// Three hours: enough of a session to carry its open and the range built off
/// it, short enough that one press is not a whole extra day of prints.
pub const PREVIOUS_SESSION_LEAD_MS: i64 = 3 * 60 * 60 * 1_000;

/// Pages one campaign may spend before it stops and lets the trader decide.
///
/// A bound on round trips, not a target: a campaign that meets its reach in
/// three pages spends three. Stopping here is not a failure — the next press
/// starts from where this one reached, because the anchor moves with it.
pub const MAX_CAMPAIGN_PAGES: u32 = 64;

/// Prints requested per campaign page on transports that stream large blocks.
/// Recutting beside the frame permits useful progress without hundreds of
/// full-series publications. This stays below the bridge's 200,000-print cap.
pub const CAMPAIGN_PAGE_PRINTS: usize = 100_000;

/// Fetched-print safety ceiling, separate from the bounded work per request.
/// The bridge's opening-session envelope uses the same four million prints:
/// a measured dense B3 session contains 1,525,621 prints. The former 250,000
/// synchronous-work ceiling could not reach the advertised session target.
/// Reaching this ceiling still reports an incomplete campaign, never success.
pub const MAX_CAMPAIGN_PRINTS: usize = 4_000_000;

/// Replies that may bring nothing new before a campaign gives up.
///
/// An empty page is not by itself the end of the record, and stopping on the
/// first one would break the case this feature exists for: a bridge crossing a
/// weekend searches hours and maps no trades at all, and its own walk covers
/// up to about four days per request. Three of those is a fortnight of dead
/// air — past any holiday — while a venue that answers empty because it is
/// rate-limiting or broken costs three requests instead of sixty-four. That
/// second case is the one this number is really sized against: Binance never
/// withdraws its paging capability, and a 429 answered as an empty block is
/// indistinguishable here from a market that was closed. It is also why
/// [`CampaignEnd::NothingComingBack`]'s sentence asks the trader to wait a
/// moment rather than to press again: a note that invited an immediate retry
/// would hand back, one press at a time, the burst this budget just refused.
pub const MAX_IDLE_PAGES: u32 = 3;

/// Span one campaign may cover *while the tape has shown no close at all*.
///
/// The answer for a market that never closes: crypto has no overnight gap, so
/// [`Campaign`] would otherwise page until its budgets ran out. Two days is
/// past any "previous session" a continuous market has.
///
/// Deliberately **not** applied once a close is in sight. The first session
/// after a weekend sits further behind than any fixed span — Monday's open is
/// some sixty-two hours after Friday's close plus this reach's lead — so a cap
/// that outranked the reach would stop at the gap having pulled seconds of
/// Friday, on exactly the mornings the feature is named for.
pub const MAX_CAMPAIGN_SPAN_MS: i64 = 48 * 60 * 60 * 1_000;

/// What one press of [`HistoryReach::Page`] is told when its single reply
/// brought no prints back.
///
/// Separate from [`CampaignEnd::NothingComingBack`], which is a *run* giving
/// up after several such replies in a row: one empty answer is not evidence
/// that a record is spent, so this sentence claims less than that one does.
pub const EMPTY_PAGE_NOTICE: &str = "no older trades came back from that request";

/// What [`HistoryReach::Span`] reaches back by until the trader says otherwise.
///
/// Two hours of traded time. Long enough to be worth a press on a contract
/// printing a million and a half times a day — where the old fixed page of
/// 2 000 prints is a couple of minutes — and short enough that one press is
/// not most of a session, which is what [`HistoryReach::PreviousSession`] is
/// already for. A trader who wants the day before should ask for the day
/// before by name; this is the reach for "a bit more than I have".
///
/// The ceiling is [`MAX_CAMPAIGN_SPAN_MS`]: a run cannot reach past it, so a
/// larger value would promise a reach the budgets forbid.
pub const DEFAULT_REACH_SPAN_MS: i64 = 2 * 60 * 60 * 1_000;

/// What a press is told when its request could not even be queued.
///
/// A closed command channel is a feed that has gone; a full one is a frame so
/// busy that pressing again is the honest recovery. Neither will ever be
/// answered, so neither may be left looking like a request in flight.
pub const REQUEST_REFUSED_NOTICE: &str = "could not ask for older trades just now; press again";

/// The two bounds a [`Campaign`] measures its reach against, as the trader's
/// configuration set them.
///
/// Passed in rather than read from the constants above, because both are
/// facts about a venue and not about quantick: `[history]` in the TOML owns
/// them, and the constants are only what that section defaults to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachBounds {
    /// See [`SESSION_GAP_MS`].
    pub session_gap_ms: i64,
    /// See [`PREVIOUS_SESSION_LEAD_MS`].
    pub previous_session_lead_ms: i64,
    /// See [`DEFAULT_REACH_SPAN_MS`]. Only read by [`HistoryReach::Span`].
    pub span_ms: i64,
}

impl Default for ReachBounds {
    fn default() -> Self {
        Self {
            session_gap_ms: SESSION_GAP_MS,
            previous_session_lead_ms: PREVIOUS_SESSION_LEAD_MS,
            span_ms: DEFAULT_REACH_SPAN_MS,
        }
    }
}

/// How far one press of *load older* reaches.
///
/// Deliberately two values and not a free-form duration. A trader asking for
/// history is asking for a *session*, not for "six hours" — six hours from
/// 10:00 lands mid-morning yesterday on one instrument and inside a weekend on
/// another. The reach names the thing they mean and the tape supplies the
/// arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryReach {
    /// One page of `history_step` trades — what the button has always done,
    /// and what it still does until the trader asks for more.
    #[default]
    Page,
    /// Keep paging until the tape shows the close of the session before the
    /// one the chart already reaches, then a further
    /// [`PREVIOUS_SESSION_LEAD_MS`] into it.
    PreviousSession,
    /// Keep paging until the chart reaches [`ReachBounds::span_ms`] further
    /// back in *traded* time.
    ///
    /// The duration lives beside the reach rather than inside it, the way the
    /// page size already does: the reach names what a press means and the
    /// setting says how much, so both are one value in a saved workspace and
    /// neither has to be re-encoded when the other changes.
    ///
    /// **Traded time, not clock time.** The doc on [`HistoryReach`] argues
    /// against a free-form duration, and it is right about the thing it
    /// describes: six hours counted on the clock lands mid-morning yesterday
    /// on one instrument and inside a weekend on another, which is not a
    /// reach a trader can predict. What makes this variant answerable is that
    /// dead time is *crossed rather than counted* — a stretch with no prints
    /// wider than [`ReachBounds::session_gap_ms`] adds nothing to the total,
    /// so "two more hours" means two more hours of tape wherever the run has
    /// to go to find them, and a press that lands in an overnight gap
    /// continues into the previous session instead of stopping in the void.
    Span,
}

impl HistoryReach {
    /// Every reach, in the order the menu offers them.
    pub const ALL: [Self; 3] = [Self::Page, Self::PreviousSession, Self::Span];

    /// The label on the control.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Page => "one page",
            Self::PreviousSession => "previous session",
            Self::Span => "by time",
        }
    }

    /// What one press of this reach promises, for the hover text.
    #[must_use]
    pub const fn hover(self) -> &'static str {
        match self {
            Self::Page => {
                "one request of the page size below, prepended and done — the \
                 press this button has always been"
            }
            Self::PreviousSession => {
                "keep asking until the chart reaches back past the market's \
                 last close, plus a few hours of the session before it, so \
                 yesterday is on screen to compare against"
            }
            Self::Span => {
                "keep asking until the chart holds the span below in traded \
                 time — nights and weekends are crossed, not counted, so the \
                 hours you ask for are hours the market was open"
            }
        }
    }

    /// The stable token this reach is written and read back as — settings on
    /// disk, the harness hook, the control plane. Separate from
    /// [`label`](Self::label) on purpose: the label is prose a release may
    /// reword, and a saved workspace must survive that.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::PreviousSession => "previous-session",
            Self::Span => "span",
        }
    }

    /// Whether one press of this reach is a *run* of requests rather than a
    /// single one.
    ///
    /// Asked rather than matched on, so a third reach that also pages is a
    /// variant and its arms in this file and nothing in `tab.rs`. The tab
    /// reads this in two places — starting a run, and deciding whether one
    /// still has its trader's consent — and a `== PreviousSession` in either
    /// would be the type switch that grows.
    #[must_use]
    pub const fn runs_a_campaign(self) -> bool {
        match self {
            Self::Page => false,
            Self::PreviousSession | Self::Span => true,
        }
    }

    /// Read a reach back from its token. Unknown text is no reach at all
    /// rather than a silent default: the caller decides whether to keep what
    /// it had or say the value was not understood.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reach| reach.token() == token.trim())
    }
}

/// Why a campaign stopped, in the words its log line uses.
///
/// An enum rather than a bool because the trader's next press depends on
/// which one it was: `ReachMet` means it worked, `Exhausted` means the button
/// is about to grey out, and the two budgets mean pressing again continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampaignEnd {
    /// The tape now reaches past a session close and the lead beyond it.
    ReachMet,
    /// The feed withdrew paging: the venue has nothing older.
    Exhausted,
    /// [`MAX_CAMPAIGN_PAGES`] spent. Pressing again continues from here.
    PagesSpent,
    /// [`MAX_CAMPAIGN_PRINTS`] pulled. Pressing again continues from here.
    PrintsPulled,
    /// [`MAX_IDLE_PAGES`] replies in a row brought nothing new. Either the
    /// venue has run out without saying so, or it is refusing — and neither is
    /// worth another sixty requests.
    NothingComingBack,
    /// [`MAX_CAMPAIGN_SPAN_MS`] covered without the tape ever showing a close
    /// — a market that does not shut. Pressing again continues from here.
    SpanCovered,
    /// The chart holds no trades at all, so there is nothing to page back
    /// *from*. Only reachable when a reset lands between two replies.
    NothingCharted,
}

impl CampaignEnd {
    /// The `action` field of the log line that records this ending.
    #[must_use]
    pub const fn action(self) -> &'static str {
        match self {
            Self::ReachMet => "reach_met",
            Self::Exhausted => "venue_exhausted",
            Self::PagesSpent => "page_budget_spent",
            Self::PrintsPulled => "print_budget_spent",
            Self::NothingComingBack => "nothing_coming_back",
            Self::SpanCovered => "span_cap_covered",
            Self::NothingCharted => "nothing_charted",
        }
    }

    /// Every ending, in declaration order — the list a caller resolves a
    /// name against, so an ending that exists is reachable by name and a
    /// new one is reachable the day it is added.
    pub const ALL: [Self; 7] = [
        Self::ReachMet,
        Self::Exhausted,
        Self::PagesSpent,
        Self::PrintsPulled,
        Self::NothingComingBack,
        Self::SpanCovered,
        Self::NothingCharted,
    ];

    /// Read an ending back from its [`action`](Self::action) token.
    ///
    /// Unknown text is no ending at all rather than a silent default, for the
    /// reason every other `from_*` in this crate gives: a typo in a validation
    /// script must not photograph the wrong state and call it a pass.
    #[must_use]
    pub fn from_action(action: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|end| end.action() == action.trim())
    }

    /// What the trader is told when a run ends this way, or [`None`] when the
    /// chart has already said it better.
    ///
    /// Only [`ReachMet`](Self::ReachMet) is silent: the session before this
    /// one is on screen, and a sentence announcing that would be noise a
    /// trader learns to stop reading. Every other ending means the press left
    /// the chart where it was, or stopped short of the reach it promised —
    /// and an outcome nobody can see is how this feature came to look like a
    /// facade with no tape behind it.
    ///
    /// The sentences carry the one distinction a trader acts on: whether
    /// pressing again continues from here, or whether the record is spent and
    /// another press would ask a venue for something it has already refused.
    /// Neither names a budget's size — [`MAX_CAMPAIGN_PAGES`] and its
    /// siblings are configuration-adjacent numbers, and a sentence carrying
    /// its own copy of one starts lying the day it moves.
    #[must_use]
    pub const fn notice(self) -> Option<&'static str> {
        match self {
            Self::ReachMet => None,
            Self::Exhausted => Some("no older trades: this source has given everything it has"),
            Self::NothingComingBack => Some(
                "nothing older came back — the venue has run out, or is \
                 refusing for now; give it a moment before pressing again",
            ),
            Self::NothingCharted => Some("no bars on the chart to page back from yet"),
            Self::PagesSpent | Self::PrintsPulled => {
                Some("stopped on this run's budget — press again to keep reaching back")
            }
            Self::SpanCovered => Some(
                "this market never closed over the stretch fetched — press \
                 again to keep reaching back",
            ),
        }
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

/// A run of *load older* requests that ends on a reach rather than on a count.
///
/// One outstanding request at a time — the MetaTrader protocol refuses a
/// second and every other transport is happier for it — so the campaign is a
/// state machine driven by replies, not a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Campaign {
    /// The oldest print the chart held when the trader pressed. Everything
    /// older than this arrived because of this campaign, which is what makes
    /// a second press reach the session before the first one did instead of
    /// finding its work already done.
    anchor_ms: i64,
    /// Requests sent so far, this campaign's first press included.
    pages_spent: u32,
    /// Prints the chart held when the trader pressed, so the run can bound
    /// the *work* it causes and not only the round trips it makes.
    prints_at_start: usize,
    /// Prints the chart held when the previous reply was judged, so a page
    /// that brought nothing is recognisable as one.
    prints_seen: usize,
    /// What this venue's session gap and lead are, from `[history]`.
    bounds: ReachBounds,
    /// Replies in a row that brought nothing new.
    ///
    /// Reset by any page that moves the oldest print. Counted rather than
    /// latched because a single empty page is ordinary — a bridge crossing a
    /// weekend finds no trades in hours of searching and is still advancing.
    idle_pages: u32,
    /// Which reach this run is serving, and therefore what "arrived" means.
    ///
    /// Held rather than passed to [`Campaign::advance`] because it is a
    /// property of the press, not of the page: a trader who changes the reach
    /// mid-run is calling that run off — `tab.rs` drops the campaign — rather
    /// than redirecting it at a goal it has already spent pages against.
    reach: HistoryReach,
    /// Fixed at admission from the transport's bounded request policy.
    page_size: usize,
}

impl Campaign {
    /// Start a campaign from the oldest print the chart holds, and from how
    /// many prints it holds.
    ///
    /// The first request is the trader's press, so the budget opens at one.
    #[must_use]
    pub const fn new(
        anchor_ms: i64,
        prints_at_start: usize,
        bounds: ReachBounds,
        reach: HistoryReach,
    ) -> Self {
        Self {
            anchor_ms,
            pages_spent: 1,
            prints_at_start,
            prints_seen: prints_at_start,
            idle_pages: 0,
            bounds,
            reach,
            page_size: CAMPAIGN_PAGE_PRINTS,
        }
    }

    /// Keep transport work bounded independently of the requested time reach.
    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.clamp(1, CAMPAIGN_PAGE_PRINTS);
        self
    }

    /// Clip the next request to this campaign's remaining print allowance.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.page_size.min(
            MAX_CAMPAIGN_PRINTS
                .saturating_sub(self.prints_seen.saturating_sub(self.prints_at_start)),
        )
    }

    /// The oldest print held when this campaign started.
    #[must_use]
    pub const fn anchor_ms(&self) -> i64 {
        self.anchor_ms
    }

    /// Requests this campaign has sent.
    #[must_use]
    pub const fn pages_spent(&self) -> u32 {
        self.pages_spent
    }

    /// Decide what to do now that a page has landed.
    ///
    /// `trades` is everything the chart holds, oldest first — the chart's
    /// chunked tape, or any slice, read by position; `can_page` is the
    /// feed's own answer to whether another request could be served — it goes
    /// false the moment a venue reports its record exhausted, and asking a
    /// feed that has said so would spin against a wall.
    ///
    /// Rate: **rare** — once per history reply, never per trade or per frame.
    /// [`last_close_before`] walks back from the anchor and stops at the first
    /// break, so the usual cost is the page that just arrived; only a tape
    /// with no break in it at all is scanned whole, and that is the case the
    /// span cap ends.
    pub fn advance<T: TradeSeq + ?Sized>(&mut self, trades: &T, can_page: bool) -> CampaignStep {
        if !can_page {
            return CampaignStep::Stop(CampaignEnd::Exhausted);
        }
        let Some(oldest) = trades.first().map(|trade| trade.timestamp_ms) else {
            return CampaignStep::Stop(CampaignEnd::NothingCharted);
        };
        match self.reach {
            // Not a campaign at all; `tab.rs` never builds one for it. Answered
            // rather than ignored so that a reach added later, which forgets to
            // say whether it runs, stops after one page instead of spending a
            // budget nobody asked it to.
            HistoryReach::Page => return CampaignStep::Stop(CampaignEnd::ReachMet),
            HistoryReach::Span => {
                let covered = traded_span_before(
                    trades,
                    self.anchor_ms,
                    self.bounds.session_gap_ms,
                    self.bounds.span_ms,
                );
                if covered >= self.bounds.span_ms {
                    return CampaignStep::Stop(CampaignEnd::ReachMet);
                }
                // The overall cap still applies: a tape whose dead time is
                // never crossed — a symbol that stopped printing for good — must
                // not page until its budgets run out looking for hours that do
                // not exist.
                if self.anchor_ms.saturating_sub(oldest) >= MAX_CAMPAIGN_SPAN_MS {
                    return CampaignStep::Stop(CampaignEnd::SpanCovered);
                }
            }
            HistoryReach::PreviousSession => {
                match last_close_before(trades, self.anchor_ms, self.bounds.session_gap_ms) {
                    Some(close) => {
                        if oldest <= close.saturating_sub(self.bounds.previous_session_lead_ms) {
                            return CampaignStep::Stop(CampaignEnd::ReachMet);
                        }
                        // A close is in sight and the lead is not covered yet.
                        // The span cap deliberately does not pre-empt this: see
                        // its own doc.
                    }
                    None if self.anchor_ms.saturating_sub(oldest) >= MAX_CAMPAIGN_SPAN_MS => {
                        return CampaignStep::Stop(CampaignEnd::SpanCovered);
                    }
                    None => {}
                }
            }
        }
        // Did that page bring anything? A venue with nothing left to give does
        // not always say so — only the MetaTrader bridge withdraws its paging
        // capability, while Binance's is a compile-time `true` that answers an
        // empty block to a rate-limited request exactly as it does to a market
        // that was closed. Without this the run would spend its whole page
        // budget on back-to-back requests, which is how a 429 becomes a ban.
        if trades.len() <= self.prints_seen {
            self.idle_pages = self.idle_pages.saturating_add(1);
            if self.idle_pages >= MAX_IDLE_PAGES {
                return CampaignStep::Stop(CampaignEnd::NothingComingBack);
            }
        } else {
            self.idle_pages = 0;
        }
        self.prints_seen = trades.len();
        if trades.len().saturating_sub(self.prints_at_start) >= MAX_CAMPAIGN_PRINTS {
            return CampaignStep::Stop(CampaignEnd::PrintsPulled);
        }
        if self.pages_spent >= MAX_CAMPAIGN_PAGES {
            return CampaignStep::Stop(CampaignEnd::PagesSpent);
        }
        self.pages_spent = self.pages_spent.saturating_add(1);
        CampaignStep::Ask
    }
}

/// How much **traded** time the chart holds behind `anchor_ms`, up to `want_ms`.
///
/// Walks backwards from the anchor adding the distance between adjacent prints,
/// and adds nothing for a distance wider than `session_gap_ms` — that stretch
/// is the market having been closed, so it is crossed rather than counted.
/// This is what makes [`HistoryReach::Span`] answerable: "two more hours" is
/// two more hours of tape wherever the run has to go to find them, instead of
/// two hours on a clock that may land inside a weekend.
///
/// Stops as soon as `want_ms` is reached, and returns at most that. The early
/// exit is not an optimisation detail, it is what keeps the cost of a run
/// linear in the pages it fetched rather than quadratic: without it every
/// reply would re-walk the whole tape, and a run of sixty-four pages would
/// walk it sixty-four times. The anchor's own position is found by binary
/// search, as [`last_close_before`] does, so the prints newer than the press
/// cost nothing at all.
///
/// Rate: **rare** — once per history reply.
#[must_use]
pub fn traded_span_before<T: TradeSeq + ?Sized>(
    trades: &T,
    anchor_ms: i64,
    session_gap_ms: i64,
    want_ms: i64,
) -> i64 {
    // The first print at or after the anchor; everything below it is what this
    // campaign pulled in.
    let end = trades.partition_point(|trade| trade.timestamp_ms < anchor_ms);
    let mut covered: i64 = 0;
    let mut newer = anchor_ms;
    for older in (0..end).rev().filter_map(|index| trades.get(index)) {
        let older = older.timestamp_ms;
        let step = newer.saturating_sub(older);
        if step < session_gap_ms {
            covered = covered.saturating_add(step);
            if covered >= want_ms {
                return want_ms;
            }
        }
        newer = older;
    }
    covered
}

/// The last print of the newest session that closed strictly before `anchor_ms`
/// — the older side of the newest gap wider than `session_gap_ms` among the
/// prints older than the anchor.
///
/// `None` means the tape shows no such close: either nothing older than the
/// anchor has arrived yet, or the market in question does not close.
///
/// Walked **backwards from the anchor**, so the first break found is the
/// newest one and the search ends there. The anchor's own position is found by
/// binary search — prints are ascending by stamp, which is what the chart's
/// retained stream guarantees — so a tape of a million prints costs the same
/// as one of a thousand, and the walk itself covers only what the campaign has
/// fetched *since* the break. Scanning forward from the oldest print instead
/// would grow by a page on every reply and make the run quadratic in pages:
/// the same answer, arrived at the expensive way round.
#[must_use]
pub fn last_close_before<T: TradeSeq + ?Sized>(
    trades: &T,
    anchor_ms: i64,
    session_gap_ms: i64,
) -> Option<i64> {
    // Pairs are (i, i + 1), and only the older side has to sit before the
    // anchor — the break between the previous session and the anchor's own is
    // exactly the one whose newer side is the anchor.
    let last_pair = trades.len().checked_sub(1)?;
    let before_anchor = trades
        .partition_point(|trade| trade.timestamp_ms < anchor_ms)
        .min(last_pair);
    (0..before_anchor).rev().find_map(|index| {
        let earlier = trades.get(index)?.timestamp_ms;
        let later = trades.get(index + 1)?.timestamp_ms;
        (later.saturating_sub(earlier) > session_gap_ms).then_some(earlier)
    })
}

#[cfg(test)]
#[path = "history_reach_tests.rs"]
mod tests;
