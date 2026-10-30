//! Refilling the hole a reconnect left, as pure state.
//!
//! A bridge that comes back after the terminal went away opens with the
//! newest slice of the session it is in, and everything between that slice
//! and the last print the chart holds is missing. The terminal still has it:
//! the opening slices that follow the block reach back to the session's first
//! print, and a *load older* reaches past that. So the hole is asked for, not
//! guessed at, and the chart is handed exactly what one continuous load of the
//! same tape would have charted.
//!
//! The prints arrive newest-first — the block, then each opening slice, then
//! each page — while the live tape keeps running. Charting the live prints as
//! they come would put them in front of history still on its way, so they are
//! held, in order, behind the refill and released after it. That keeps the
//! tape in time order, which is the whole determinism contract: the same
//! prints in the same order build the same bars.
//!
//! Both seams are exact. The new end is the bridge's own: its block, slices
//! and pages never overlap one another, and its live tape starts after the
//! block. The old end is where the chart stopped, and MT5 prints share
//! milliseconds with no stable id, so [`ResumeSeam`] carries how many prints
//! at the last millisecond the chart holds; exactly that many are skipped
//! there and the rest kept.
//!
//! A refill that cannot reach the old end says so: the part it did not cover
//! is handed back as a [`FeedGap`], which the chart marks, and the reason is
//! a [`BackfillStop`]. Nothing here owns a channel, a clock or a log line —
//! the driver in `refill.rs` does the sending.

use quantick_engine::Trade;
use quantick_sources::history_reach::{
    CAMPAIGN_PAGE_PRINTS, MAX_IDLE_PAGES, PAGE_SLACK, PRINTS_PER_SESSION_BUDGET, SESSION_GAP_MS,
};

use crate::config::{BackfillState, BackfillStop, TapeBackfill};
use crate::{FeedEvent, FeedGap, ResumeSeam};

/// How many session closes a refill may cross: the deepest reach the
/// History button offers (`sessions:5`). A hole older than that is a chart
/// left open for a week, and the part beyond it stays a marked gap.
pub(super) const REFILL_REACH_SESSIONS: u32 = 5;

/// Prints asked for per page: the History run's own page size, which stays
/// under the Python bridge's 200,000-print cap per request.
pub(super) const REFILL_PAGE_PRINTS: u64 = CAMPAIGN_PAGE_PRINTS as u64;

/// The most prints one refill may collect: the History run's per-session
/// budget, for every session the reach allows.
pub(super) const REFILL_MAX_PRINTS: usize =
    REFILL_REACH_SESSIONS as usize * PRINTS_PER_SESSION_BUDGET;

/// The most pages one refill may ask for: enough to spend the print budget,
/// plus the History run's slack for pages that cross dead time.
pub(super) const REFILL_MAX_PAGES: u32 =
    REFILL_REACH_SESSIONS * (PRINTS_PER_SESSION_BUDGET / CAMPAIGN_PAGE_PRINTS) as u32 + PAGE_SLACK;

/// The most live events held behind a refill. A busy B3 tape prints about
/// 160,000 times an hour, so this is several hours of tape: reaching it means
/// the source stopped answering, not that the hole is large.
pub(super) const MAX_HELD_EVENTS: usize = 1_000_000;

/// What the driver does next.
pub(super) enum FillStep {
    /// Something is on its way; keep holding.
    Wait,
    /// Ask the bridge for up to `count` prints older than `before_utc_ms`.
    Ask { count: u64, before_utc_ms: i64 },
    /// The refill is over: chart this.
    Release(Release),
}

/// What a finished refill hands the chart, in the order it goes out.
pub(super) struct Release {
    /// The part of the hole nothing covered, marked before the prints.
    pub(super) gap: Option<FeedGap>,
    /// The refilled prints, ascending, both seams deduplicated.
    pub(super) trades: Vec<Trade>,
    /// The final report.
    pub(super) report: TapeBackfill,
    /// The live events held behind the refill, in arrival order.
    pub(super) held: Vec<FeedEvent>,
    /// A page the refill asked for is still on its way. Nobody wants it
    /// now; the driver drops it when it lands.
    pub(super) page_owed: bool,
}

/// One refill in progress.
pub(super) struct GapFill {
    seam: ResumeSeam,
    /// The oldest print of the block the new session opened with.
    to_ms: i64,
    /// What arrived, newest block first; each strictly older than the last.
    chunks: Vec<Vec<Trade>>,
    /// The oldest print in hand.
    oldest_ms: i64,
    /// Where the next page is asked from: the oldest print, or how far the
    /// bridge said its search reached when that is further.
    cursor_ms: i64,
    received: usize,
    /// Session closes crossed between the prints in hand.
    closes: u32,
    held: Vec<FeedEvent>,
    can_page: bool,
    page_owed: bool,
    pages: u32,
    /// Pages in a row that brought nothing.
    idle: u32,
    exhausted: bool,
    /// The session answered a page without searching: it is ending.
    unanswered: bool,
}

impl GapFill {
    /// Open a refill from the chart's `seam` to the reconnect block, which
    /// is non-empty and ascending.
    pub(super) fn begin(seam: ResumeSeam, block: Vec<Trade>, can_page: bool) -> Self {
        let to_ms = block.first().map_or(i64::MAX, |t| t.timestamp_ms);
        let mut fill = Self {
            seam,
            to_ms,
            chunks: Vec::new(),
            oldest_ms: i64::MAX,
            cursor_ms: i64::MAX,
            received: 0,
            closes: 0,
            held: Vec::new(),
            can_page,
            page_owed: false,
            pages: 0,
            idle: 0,
            exhausted: false,
            unanswered: false,
        };
        fill.absorb(block);
        fill
    }

    /// The report while the refill runs.
    pub(super) fn report(&self) -> TapeBackfill {
        self.report_as(BackfillState::Filling)
    }

    /// Whether the refill has a page on the wire.
    pub(super) fn page_owed(&self) -> bool {
        self.page_owed
    }

    /// The driver put the last [`FillStep::Ask`] on the wire.
    pub(super) fn asked(&mut self) {
        self.page_owed = true;
        self.pages += 1;
    }

    /// A slice of the session the new block belongs to, older than it.
    pub(super) fn opening(&mut self, trades: Vec<Trade>) {
        self.absorb(trades);
    }

    /// The answer to the refill's own page.
    pub(super) fn page(&mut self, trades: Vec<Trade>, exhausted: bool, scanned_to: Option<i64>) {
        self.page_owed = false;
        // A reply with no search behind it is the session answering what it
        // will never fetch — it is ending, or it cannot page after all.
        self.unanswered |= trades.is_empty() && scanned_to.is_none() && !exhausted;
        self.exhausted |= exhausted;
        if self.absorb(trades) == 0 {
            self.idle += 1;
        } else {
            self.idle = 0;
        }
        if let Some(reached) = scanned_to {
            self.cursor_ms = self.cursor_ms.min(reached);
        }
    }

    /// A live event arrived while the refill runs: hold it. Returns whether
    /// the hold reached its bound.
    pub(super) fn hold(&mut self, event: FeedEvent) -> bool {
        self.held.push(event);
        self.held.len() >= MAX_HELD_EVENTS
    }

    /// Decide what happens next.
    pub(super) fn step(&mut self) -> FillStep {
        if self.oldest_ms <= self.seam.last_ms {
            return FillStep::Release(self.finish(None));
        }
        if let Some(stop) = self.stop() {
            return FillStep::Release(self.finish(Some(stop)));
        }
        if self.page_owed {
            return FillStep::Wait;
        }
        FillStep::Ask {
            count: REFILL_PAGE_PRINTS,
            before_utc_ms: self.cursor_ms,
        }
    }

    /// End the refill now with what is in hand.
    pub(super) fn stop_with(&mut self, stop: BackfillStop) -> Release {
        self.finish(Some(stop))
    }

    fn stop(&self) -> Option<BackfillStop> {
        if self.unanswered {
            Some(BackfillStop::SessionLost)
        } else if !self.can_page {
            Some(BackfillStop::NoPaging)
        } else if self.exhausted {
            Some(BackfillStop::SourceExhausted)
        } else if self.idle >= MAX_IDLE_PAGES {
            Some(BackfillStop::NothingFound)
        } else if self.closes > REFILL_REACH_SESSIONS
            || self.received >= REFILL_MAX_PRINTS
            || self.pages >= REFILL_MAX_PAGES
        {
            Some(BackfillStop::Reach)
        } else if self.held.len() >= MAX_HELD_EVENTS {
            Some(BackfillStop::HeldTooLong)
        } else {
            None
        }
    }

    /// Take the prints strictly older than those in hand. Returns how many.
    fn absorb(&mut self, mut trades: Vec<Trade>) -> usize {
        trades.retain(|t| t.timestamp_ms < self.oldest_ms);
        let (Some(first), Some(last)) = (trades.first(), trades.last()) else {
            return 0;
        };
        let (first_ms, last_ms) = (first.timestamp_ms, last.timestamp_ms);
        let silences = trades
            .windows(2)
            .filter(|pair| pair[1].timestamp_ms - pair[0].timestamp_ms > SESSION_GAP_MS)
            .count();
        let seam =
            usize::from(self.oldest_ms != i64::MAX && self.oldest_ms - last_ms > SESSION_GAP_MS);
        self.closes = self
            .closes
            .saturating_add(u32::try_from(silences + seam).unwrap_or(u32::MAX));
        self.oldest_ms = first_ms;
        self.cursor_ms = self.cursor_ms.min(first_ms);
        self.received += trades.len();
        let count = trades.len();
        self.chunks.push(trades);
        count
    }

    fn report_as(&self, state: BackfillState) -> TapeBackfill {
        TapeBackfill {
            from_ms: self.seam.last_ms,
            to_ms: self.to_ms,
            state,
            received: self.received as u64,
        }
    }

    fn finish(&mut self, stop: Option<BackfillStop>) -> Release {
        let covered = self.oldest_ms <= self.seam.last_ms;
        let mut trades = Vec::with_capacity(self.received);
        for chunk in self.chunks.drain(..).rev() {
            trades.extend(chunk);
        }
        // The old seam: everything before the chart's last millisecond is
        // held already, and so are the first `held_at_last` prints at it.
        let last_ms = self.seam.last_ms;
        let mut skip = self.seam.held_at_last;
        trades.retain(|t| match t.timestamp_ms.cmp(&last_ms) {
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal if skip > 0 => {
                skip -= 1;
                false
            }
            _ => true,
        });
        let (state, gap) = match stop {
            Some(stop) if !covered => {
                // Nothing in hand ends the hole at the new block.
                let unfilled_to_ms = self.oldest_ms.min(self.to_ms);
                (
                    BackfillState::Partial {
                        unfilled_to_ms,
                        stop,
                    },
                    Some(FeedGap {
                        from_ms: last_ms,
                        to_ms: unfilled_to_ms,
                    }),
                )
            }
            _ => (BackfillState::Filled, None),
        };
        Release {
            gap,
            trades,
            report: self.report_as(state),
            held: std::mem::take(&mut self.held),
            page_owed: self.page_owed,
        }
    }
}

#[cfg(test)]
mod gap_fill_tests;
