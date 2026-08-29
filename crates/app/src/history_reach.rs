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

use quantick_engine::Trade;

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

/// Prints one campaign may pull before it stops and lets the trader decide.
///
/// The page budget bounds *round trips*; this bounds *work*. Every page is
/// prepended through `ChartState::prepend_history`, which re-cuts every bar
/// the chart holds — so the cost of a run is the tape it fetched, not the
/// number of requests, and a trader who raised the page size to 50 000 would
/// otherwise buy sixty-four of those rebuilds with one press. A quarter of a
/// million prints is a heavy session's worth: enough that a reach lands in one
/// press on any ordinary tape, bounded enough that the frame it costs is one
/// frame.
pub const MAX_CAMPAIGN_PRINTS: usize = 250_000;

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
}

impl Default for ReachBounds {
    fn default() -> Self {
        Self {
            session_gap_ms: SESSION_GAP_MS,
            previous_session_lead_ms: PREVIOUS_SESSION_LEAD_MS,
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
}

impl HistoryReach {
    /// Every reach, in the order the menu offers them.
    pub const ALL: [Self; 2] = [Self::Page, Self::PreviousSession];

    /// The label on the control.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Page => "one page",
            Self::PreviousSession => "previous session",
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
            Self::PreviousSession => true,
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
}

impl Campaign {
    /// Start a campaign from the oldest print the chart holds, and from how
    /// many prints it holds.
    ///
    /// The first request is the trader's press, so the budget opens at one.
    #[must_use]
    pub const fn new(anchor_ms: i64, prints_at_start: usize, bounds: ReachBounds) -> Self {
        Self {
            anchor_ms,
            pages_spent: 1,
            prints_at_start,
            prints_seen: prints_at_start,
            idle_pages: 0,
            bounds,
        }
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
    /// `trades` is everything the chart holds, oldest first; `can_page` is the
    /// feed's own answer to whether another request could be served — it goes
    /// false the moment a venue reports its record exhausted, and asking a
    /// feed that has said so would spin against a wall.
    ///
    /// Rate: **rare** — once per history reply, never per trade or per frame.
    /// [`last_close_before`] walks back from the anchor and stops at the first
    /// break, so the usual cost is the page that just arrived; only a tape
    /// with no break in it at all is scanned whole, and that is the case the
    /// span cap ends.
    pub fn advance(&mut self, trades: &[Trade], can_page: bool) -> CampaignStep {
        if !can_page {
            return CampaignStep::Stop(CampaignEnd::Exhausted);
        }
        let Some(oldest) = trades.first().map(|trade| trade.timestamp_ms) else {
            return CampaignStep::Stop(CampaignEnd::NothingCharted);
        };
        match last_close_before(trades, self.anchor_ms, self.bounds.session_gap_ms) {
            Some(close) => {
                if oldest <= close.saturating_sub(self.bounds.previous_session_lead_ms) {
                    return CampaignStep::Stop(CampaignEnd::ReachMet);
                }
                // A close is in sight and the lead is not covered yet. The span
                // cap deliberately does not pre-empt this: see its own doc.
            }
            None if self.anchor_ms.saturating_sub(oldest) >= MAX_CAMPAIGN_SPAN_MS => {
                return CampaignStep::Stop(CampaignEnd::SpanCovered);
            }
            None => {}
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
pub fn last_close_before(trades: &[Trade], anchor_ms: i64, session_gap_ms: i64) -> Option<i64> {
    // Pairs are (i, i + 1), and only the older side has to sit before the
    // anchor — the break between the previous session and the anchor's own is
    // exactly the one whose newer side is the anchor.
    let last_pair = trades.len().checked_sub(1)?;
    let before_anchor = trades
        .partition_point(|trade| trade.timestamp_ms < anchor_ms)
        .min(last_pair);
    (0..before_anchor).rev().find_map(|index| {
        let earlier = trades[index].timestamp_ms;
        let later = trades[index + 1].timestamp_ms;
        (later.saturating_sub(earlier) > session_gap_ms).then_some(earlier)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use rust_decimal::Decimal;

    /// A print at `ms`. Only the stamp matters here — the reach is arithmetic
    /// over time, and price and size never enter it.
    fn trade(ms: i64) -> Trade {
        Trade {
            agg_id: ms as u64,
            timestamp_ms: ms,
            price: Decimal::ONE,
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    /// Prints every `step_ms` from `start_ms`, `count` of them.
    fn run(start_ms: i64, step_ms: i64, count: usize) -> Vec<Trade> {
        (0..count)
            .map(|i| trade(start_ms + step_ms * i as i64))
            .collect()
    }

    const MINUTE: i64 = 60 * 1_000;
    const HOUR: i64 = 60 * MINUTE;

    #[test]
    fn a_tape_with_no_break_in_it_shows_no_close() {
        let dense = run(0, MINUTE, 600);
        assert_eq!(
            last_close_before(&dense, i64::MAX, SESSION_GAP_MS),
            None,
            "ten hours of minute prints is one session, however long it runs"
        );
    }

    #[test]
    fn the_print_on_the_older_side_of_the_break_is_the_close() {
        let mut tape = run(0, MINUTE, 60);
        let close = tape.last().expect("the run is not empty").timestamp_ms;
        tape.extend(run(close + 14 * HOUR, MINUTE, 60));
        assert_eq!(
            last_close_before(&tape, i64::MAX, SESSION_GAP_MS),
            Some(close),
            "the session ended at its last print, not at the next one's open"
        );
    }

    #[test]
    fn a_quiet_stretch_shorter_than_the_threshold_is_not_a_close() {
        let mut tape = run(0, MINUTE, 10);
        let quiet = tape.last().unwrap().timestamp_ms + SESSION_GAP_MS;
        tape.extend(run(quiet, MINUTE, 10));
        assert_eq!(
            last_close_before(&tape, i64::MAX, SESSION_GAP_MS),
            None,
            "an hour of silence is a thin market, not a closed one"
        );
    }

    #[test]
    fn the_newest_close_older_than_the_anchor_is_the_one_reported() {
        // Three sessions, so there are two closes to choose between.
        let mut tape = run(0, MINUTE, 30);
        let first_close = tape.last().unwrap().timestamp_ms;
        tape.extend(run(first_close + 14 * HOUR, MINUTE, 30));
        let second_close = tape.last().unwrap().timestamp_ms;
        tape.extend(run(second_close + 14 * HOUR, MINUTE, 30));

        assert_eq!(
            last_close_before(&tape, i64::MAX, SESSION_GAP_MS),
            Some(second_close),
            "with no anchor in the way, the newest close wins"
        );
        assert_eq!(
            last_close_before(&tape, second_close, SESSION_GAP_MS),
            Some(first_close),
            "an anchor at the newest close pushes the answer to the one before it"
        );
    }

    /// A tape long enough to page over, `count` prints of it, ending just
    /// before minute zero. The campaigns below start from its oldest print.
    fn session(count: usize) -> Vec<Trade> {
        run(-(count as i64) * MINUTE, MINUTE, count)
    }

    #[test]
    fn a_campaign_keeps_asking_while_the_tape_is_still_inside_one_session() {
        let mut tape = session(120);
        let anchor = tape.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor, tape.len(), ReachBounds::default());
        // One page arrives, still inside the same session.
        let mut older = run(anchor - 60 * MINUTE, MINUTE, 60);
        older.append(&mut tape);
        assert_eq!(
            campaign.advance(&older, true),
            CampaignStep::Ask,
            "no break has been reached, so there is more to fetch"
        );
        assert_eq!(campaign.pages_spent(), 2, "the press plus this request");
    }

    #[test]
    fn a_campaign_stops_once_the_lead_past_the_close_is_covered() {
        // Today's session, and the anchor at its first print.
        let today = run(20 * HOUR, MINUTE, 60);
        let anchor = today.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor, today.len(), ReachBounds::default());

        // Yesterday arrives, but only its last hour: short of the lead.
        let close = anchor - 14 * HOUR;
        let mut tape = run(close - HOUR, MINUTE, 61);
        tape.extend_from_slice(&today);
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Ask,
            "one hour of the previous session is not the lead asked for"
        );

        // Now enough of it.
        let mut tape = run(close - PREVIOUS_SESSION_LEAD_MS, MINUTE, 181);
        tape.extend_from_slice(&today);
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::ReachMet),
            "the previous session is on screen with its lead"
        );
    }

    /// Monday. Friday's close is some sixty-two hours behind Monday's open, so
    /// a span cap that outranked the reach would stop the run at the weekend
    /// gap having brought back minutes of Friday — on exactly the mornings
    /// this reach exists for.
    #[test]
    fn a_weekend_does_not_let_the_span_cap_pre_empt_the_reach() {
        let monday_open = 100 * 24 * HOUR;
        let today = run(monday_open, MINUTE, 30);
        let anchor = today.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor, today.len(), ReachBounds::default());

        // Friday's last print, a weekend and a half-session behind.
        let friday_close = anchor - 63 * HOUR;
        let mut tape = run(friday_close - 30 * MINUTE, MINUTE, 31);
        tape.extend_from_slice(&today);
        assert!(
            anchor - tape.first().unwrap().timestamp_ms > MAX_CAMPAIGN_SPAN_MS,
            "the fixture has to be past the cap or it proves nothing"
        );
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Ask,
            "the close is in sight, so the run keeps going for its lead"
        );

        let mut tape = run(friday_close - PREVIOUS_SESSION_LEAD_MS, MINUTE, 181);
        tape.extend_from_slice(&today);
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::ReachMet),
            "and Friday afternoon is on the chart"
        );
    }

    #[test]
    fn a_feed_that_has_run_out_stops_the_campaign_rather_than_being_asked_again() {
        let tape = session(10);
        let mut campaign = Campaign::new(
            tape.first().unwrap().timestamp_ms,
            tape.len(),
            ReachBounds::default(),
        );
        assert_eq!(
            campaign.advance(&tape, false),
            CampaignStep::Stop(CampaignEnd::Exhausted),
            "a venue that reported its record exhausted is not asked once more"
        );
    }

    #[test]
    fn a_market_that_never_closes_ends_on_the_span_cap() {
        // Continuous prints reaching further back than the cap: no gap will
        // ever appear, so the span is the only thing that can stop this.
        let anchor = MAX_CAMPAIGN_SPAN_MS + 10 * MINUTE;
        let tape = run(0, 10 * MINUTE, (anchor / (10 * MINUTE)) as usize + 1);
        let mut campaign = Campaign::new(anchor, 1, ReachBounds::default());
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::SpanCovered),
            "crypto has no overnight break; the cap is what ends the run"
        );
    }

    /// The stop that keeps one press from becoming sixty-four requests against
    /// a venue that is refusing rather than empty.
    ///
    /// Only the MetaTrader bridge ever withdraws its paging capability.
    /// Binance's is a compile-time `true` and answers a rate-limited fetch with
    /// the same empty block it answers "nothing older" with, so `can_page`
    /// cannot be what stops this — and sixty-four back-to-back REST calls is
    /// how a 429 becomes an IP ban.
    #[test]
    fn a_venue_that_keeps_answering_empty_is_not_asked_sixty_four_times() {
        let tape = session(10);
        let mut campaign = Campaign::new(
            tape.first().unwrap().timestamp_ms,
            tape.len(),
            ReachBounds::default(),
        );
        for page in 1..MAX_IDLE_PAGES {
            assert_eq!(
                campaign.advance(&tape, true),
                CampaignStep::Ask,
                "empty page {page} could still be dead time being crossed"
            );
        }
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::NothingComingBack),
            "but a run of them is a venue with nothing coming back"
        );
        assert!(
            campaign.pages_spent() < MAX_CAMPAIGN_PAGES,
            "and it cost a handful of requests, not the whole budget"
        );
    }

    /// The other half of the same rule: an empty page is *ordinary* while a
    /// bridge crosses a weekend, and stopping on the first one would break the
    /// case this reach is named for.
    #[test]
    fn a_single_empty_page_does_not_end_a_run_crossing_dead_time() {
        let today = session(60);
        let anchor = today.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor, today.len(), ReachBounds::default());
        assert_eq!(
            campaign.advance(&today, true),
            CampaignStep::Ask,
            "the search moved hours and mapped no trades; that is a weekend"
        );
        // The next page lands the previous session, and the idle count clears.
        let close = anchor - 14 * HOUR;
        let mut tape = run(close - PREVIOUS_SESSION_LEAD_MS, MINUTE, 181);
        tape.extend_from_slice(&today);
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::ReachMet)
        );
    }

    /// Every page is prepended through a full re-cut of the chart's bars, so
    /// the run bounds the prints it pulls and not only the requests it makes.
    #[test]
    fn the_print_budget_bounds_the_work_one_press_causes() {
        let today = session(10);
        let anchor = today.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor, today.len(), ReachBounds::default());
        // One enormous page, inside a session that never breaks: nothing but
        // this budget can stop it before the span cap, and the tape is far
        // newer than that.
        let mut tape = run(anchor - MAX_CAMPAIGN_PRINTS as i64, 1, MAX_CAMPAIGN_PRINTS);
        tape.extend_from_slice(&today);
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::PrintsPulled),
            "a quarter of a million prints is one press's worth of re-cutting"
        );
    }

    #[test]
    fn the_page_budget_bounds_a_run_that_keeps_making_progress() {
        let today = session(10);
        let anchor = today.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor, today.len(), ReachBounds::default());
        // Every page brings one more print and never a break, so neither the
        // idle count nor the reach can end this. The prints stay far inside
        // both the span cap and the print budget.
        let mut tape = today.clone();
        for page in 1..MAX_CAMPAIGN_PAGES {
            tape.insert(0, trade(tape[0].timestamp_ms - MINUTE));
            assert_eq!(
                campaign.advance(&tape, true),
                CampaignStep::Ask,
                "page {page} is inside the budget"
            );
        }
        tape.insert(0, trade(tape[0].timestamp_ms - MINUTE));
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::PagesSpent),
            "sixty-four round trips is enough for one press"
        );
        assert_eq!(campaign.pages_spent(), MAX_CAMPAIGN_PAGES);
    }

    #[test]
    fn a_chart_emptied_under_a_running_campaign_stops_it() {
        let mut campaign = Campaign::new(0, 1, ReachBounds::default());
        assert_eq!(
            campaign.advance(&[], true),
            CampaignStep::Stop(CampaignEnd::NothingCharted),
            "a reset between two replies leaves nothing to page back from"
        );
    }

    #[test]
    fn every_reach_round_trips_through_its_token() {
        for reach in HistoryReach::ALL {
            assert_eq!(
                HistoryReach::from_token(reach.token()),
                Some(reach),
                "{} must survive a save and a reload",
                reach.label()
            );
        }
        assert_eq!(
            HistoryReach::from_token("a reach from a later release"),
            None,
            "unknown text is no reach at all, never a silent default"
        );
    }

    #[test]
    fn the_default_reach_is_the_press_this_button_has_always_had() {
        assert_eq!(HistoryReach::default(), HistoryReach::Page);
    }

    /// The ending a trader never has to be told about is the one that worked.
    /// Every other ending left the chart where it was or stopped short of what
    /// the press promised, and a press whose outcome is invisible is exactly
    /// how this feature shipped looking like a facade.
    /// An ending that exists is reachable by its own name, both ways.
    #[test]
    fn every_ending_survives_a_round_trip_through_its_token() {
        for end in CampaignEnd::ALL {
            assert_eq!(
                CampaignEnd::from_action(end.action()),
                Some(end),
                "{} must be reachable by the name its log line uses",
                end.action()
            );
        }
        assert_eq!(
            CampaignEnd::from_action("an ending from a later release"),
            None,
            "unknown text is no ending at all, never a silent default"
        );
    }

    #[test]
    fn every_ending_but_the_one_that_worked_has_something_to_say() {
        assert_eq!(
            CampaignEnd::ReachMet.notice(),
            None,
            "the session before this one is on the chart; the chart says it better"
        );
        for end in CampaignEnd::ALL
            .into_iter()
            .filter(|end| *end != CampaignEnd::ReachMet)
        {
            let notice = end
                .notice()
                .unwrap_or_else(|| panic!("{} stops the run in silence", end.action()));
            assert!(
                !notice.is_empty(),
                "{} has an empty sentence, which is silence with extra steps",
                end.action()
            );
        }
    }

    /// Two endings mean *the record is spent* and two mean *press again*. A
    /// trader acts on that difference, so the sentence has to carry it.
    #[test]
    fn an_ending_that_continues_invites_another_press() {
        for end in [
            CampaignEnd::PagesSpent,
            CampaignEnd::PrintsPulled,
            CampaignEnd::SpanCovered,
        ] {
            assert!(
                end.notice().expect("a sentence").contains("press again"),
                "{} continues from here and must say so",
                end.action()
            );
        }
        assert!(
            !CampaignEnd::Exhausted
                .notice()
                .expect("a sentence")
                .contains("press again"),
            "a spent record must not invite a press that cannot be served"
        );
    }
}
