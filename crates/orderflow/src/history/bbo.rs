//! The top of the book over time: the best bid and best ask the history's
//! book held, one step per change.
//!
//! Read off the exact book, not the capture's buckets, so a line drawn from
//! it sits on the price a print at the touch would trade at. A step is cut
//! only when either best price or the generation moves; quantity churn at
//! the touch, and a confirmation that changes nothing, add nothing. A gap
//! records a book-less step, so a reader never connects the two sides of it.

use std::collections::VecDeque;

use quantick_orderbook::OrderBook;
use rust_decimal::Decimal;

/// The book's top from `start_ms` until the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BboStep {
    /// First timestamp this top held.
    pub start_ms: i64,
    /// Synchronized generation; `None` while there is no book (a gap).
    pub generation: Option<u64>,
    /// Best bid price, `None` when the bid side is empty.
    pub bid: Option<Decimal>,
    /// Best ask price, `None` when the ask side is empty.
    pub ask: Option<Decimal>,
}

/// Every retained [`BboStep`], oldest first, start times non-decreasing.
#[derive(Debug, Clone, Default)]
pub struct BboTrail {
    steps: VecDeque<BboStep>,
}

impl BboTrail {
    /// The retained steps, oldest first.
    pub fn steps(&self) -> impl Iterator<Item = &BboStep> {
        self.steps.iter()
    }

    /// The steps in force anywhere in `[from_ms, until_ms)`: the one holding
    /// at `from_ms` and every one starting before `until_ms`.
    pub fn steps_between(&self, from_ms: i64, until_ms: i64) -> impl Iterator<Item = &BboStep> {
        let after = self.steps.partition_point(|step| step.start_ms <= from_ms);
        self.steps
            .range(after.saturating_sub(1)..)
            .take_while(move |step| step.start_ms < until_ms)
    }

    /// The step in force at `at_ms`: the last one starting at or before it.
    #[must_use]
    pub fn step_at(&self, at_ms: i64) -> Option<&BboStep> {
        let after = self.steps.partition_point(|step| step.start_ms <= at_ms);
        after.checked_sub(1).and_then(|index| self.steps.get(index))
    }

    /// Bytes the retained steps hold, for the history's byte budget.
    #[must_use]
    pub fn approximate_bytes(&self) -> usize {
        self.steps.len() * std::mem::size_of::<BboStep>()
    }

    /// Record `book`'s top at `timestamp_ms` under `generation`, when it
    /// differs from the last step. A change at the last step's own instant
    /// replaces it, and one that restores the step before it removes it.
    pub(crate) fn observe(&mut self, timestamp_ms: i64, generation: Option<u64>, book: &OrderBook) {
        let next = BboStep {
            start_ms: timestamp_ms,
            generation,
            bid: book.best_bid().map(|level| level.price()),
            ask: book.best_ask().map(|level| level.price()),
        };
        let same_top = |step: &BboStep| {
            (step.generation, step.bid, step.ask) == (next.generation, next.bid, next.ask)
        };
        if self.steps.back().is_some_and(same_top) {
            return;
        }
        if self
            .steps
            .back()
            .is_some_and(|last| last.start_ms == timestamp_ms)
        {
            self.steps.pop_back();
            if self.steps.back().is_some_and(same_top) {
                return;
            }
        }
        self.steps.push_back(next);
    }

    /// Drop every step that ended at or before `cutoff_ms`, keeping the one
    /// still in force there.
    pub(crate) fn truncate_before(&mut self, cutoff_ms: i64) {
        while self
            .steps
            .get(1)
            .is_some_and(|next| next.start_ms <= cutoff_ms)
        {
            self.steps.pop_front();
        }
    }

    /// Forget every step: the book they describe is gone.
    pub(crate) fn clear(&mut self) {
        self.steps.clear();
    }
}

#[cfg(test)]
#[path = "bbo_tests.rs"]
pub(crate) mod tests;
