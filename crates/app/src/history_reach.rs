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
/// A bound, not a target: a campaign that meets its reach in three pages
/// spends three. This exists so a venue answering empty forever — a bridge
/// walking a delisted symbol, a cursor that stops advancing — ends in a
/// bounded number of round trips instead of asking until the app is closed.
pub const MAX_CAMPAIGN_PAGES: u32 = 64;

/// Span one campaign may cover, whatever the tape says about sessions.
///
/// The answer for a market that never closes: crypto has no overnight gap, so
/// [`Campaign`] would otherwise page until its page budget ran out. Two days
/// is past any "previous session" a continuous market has.
pub const MAX_CAMPAIGN_SPAN_MS: i64 = 48 * 60 * 60 * 1_000;

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
            Self::SpanCovered => "span_cap_covered",
            Self::NothingCharted => "nothing_charted",
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
}

impl Campaign {
    /// Start a campaign from the oldest print the chart holds.
    ///
    /// The first request is the trader's press, so the budget opens at one.
    #[must_use]
    pub const fn new(anchor_ms: i64) -> Self {
        Self {
            anchor_ms,
            pages_spent: 1,
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
    /// The scan below stops at the anchor, so its cost is the page that just
    /// arrived rather than the whole retained tape.
    pub fn advance(&mut self, trades: &[Trade], can_page: bool) -> CampaignStep {
        if !can_page {
            return CampaignStep::Stop(CampaignEnd::Exhausted);
        }
        let Some(oldest) = trades.first().map(|trade| trade.timestamp_ms) else {
            return CampaignStep::Stop(CampaignEnd::NothingCharted);
        };
        if let Some(close) = last_close_before(trades, self.anchor_ms)
            && oldest <= close.saturating_sub(PREVIOUS_SESSION_LEAD_MS)
        {
            return CampaignStep::Stop(CampaignEnd::ReachMet);
        }
        if self.anchor_ms.saturating_sub(oldest) >= MAX_CAMPAIGN_SPAN_MS {
            return CampaignStep::Stop(CampaignEnd::SpanCovered);
        }
        if self.pages_spent >= MAX_CAMPAIGN_PAGES {
            return CampaignStep::Stop(CampaignEnd::PagesSpent);
        }
        self.pages_spent = self.pages_spent.saturating_add(1);
        CampaignStep::Ask
    }
}

/// The last print of the newest session that closed strictly before `anchor_ms`
/// — the older side of the newest gap wider than [`SESSION_GAP_MS`] among the
/// prints older than the anchor.
///
/// `None` means the tape shows no such close: either nothing older than the
/// anchor has arrived yet, or the market in question does not close.
///
/// Scanned forward from the oldest print and stopped at the anchor rather than
/// walked back from the newest, because everything older than the anchor is
/// exactly what this campaign fetched — a retained tape of a million prints
/// costs the same here as one of a thousand.
#[must_use]
pub fn last_close_before(trades: &[Trade], anchor_ms: i64) -> Option<i64> {
    let mut close = None;
    for pair in trades.windows(2) {
        let (earlier, later) = (pair[0].timestamp_ms, pair[1].timestamp_ms);
        if earlier >= anchor_ms {
            break;
        }
        if later.saturating_sub(earlier) > SESSION_GAP_MS {
            close = Some(earlier);
        }
    }
    close
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
            last_close_before(&dense, i64::MAX),
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
            last_close_before(&tape, i64::MAX),
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
            last_close_before(&tape, i64::MAX),
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
            last_close_before(&tape, i64::MAX),
            Some(second_close),
            "with no anchor in the way, the newest close wins"
        );
        assert_eq!(
            last_close_before(&tape, second_close),
            Some(first_close),
            "an anchor at the newest close pushes the answer to the one before it"
        );
    }

    #[test]
    fn a_campaign_keeps_asking_while_the_tape_is_still_inside_one_session() {
        let tape = run(0, MINUTE, 120);
        let anchor = tape.first().unwrap().timestamp_ms;
        let mut campaign = Campaign::new(anchor);
        assert_eq!(
            campaign.advance(&tape, true),
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
        let mut campaign = Campaign::new(anchor);

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

    #[test]
    fn a_feed_that_has_run_out_stops_the_campaign_rather_than_being_asked_again() {
        let tape = run(0, MINUTE, 10);
        let mut campaign = Campaign::new(tape.first().unwrap().timestamp_ms);
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
        let mut campaign = Campaign::new(anchor);
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::SpanCovered),
            "crypto has no overnight break; the cap is what ends the run"
        );
    }

    #[test]
    fn the_page_budget_ends_a_campaign_a_silent_venue_would_never_end() {
        // A venue answering empty forever: the tape never grows, so neither
        // the reach nor the span cap can ever be met.
        let tape = run(0, MINUTE, 10);
        let mut campaign = Campaign::new(tape.first().unwrap().timestamp_ms);
        for page in 1..MAX_CAMPAIGN_PAGES {
            assert_eq!(
                campaign.advance(&tape, true),
                CampaignStep::Ask,
                "page {page} is inside the budget"
            );
        }
        assert_eq!(
            campaign.advance(&tape, true),
            CampaignStep::Stop(CampaignEnd::PagesSpent),
            "the budget is what stops a venue that answers nothing forever"
        );
        assert_eq!(campaign.pages_spent(), MAX_CAMPAIGN_PAGES);
    }

    #[test]
    fn a_chart_emptied_under_a_running_campaign_stops_it() {
        let mut campaign = Campaign::new(0);
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
}
