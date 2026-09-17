//! Borrowed temporal projection of closed bars and their optional forming bar.
use crate::Bar;

#[derive(Debug, Clone, Copy)]
pub struct BarTimeline<'a> {
    closed: &'a [Bar],
    partial: Option<&'a Bar>,
}

impl<'a> BarTimeline<'a> {
    /// Bars must be in non-decreasing open-time order, as builders emit them.
    pub fn new(closed: &'a [Bar], partial: Option<&'a Bar>) -> Self {
        Self { closed, partial }
    }

    /// The forming bar occupies exactly the slot after the closed series.
    pub fn slot_open_time(self, index: usize) -> Option<i64> {
        match self.closed.get(index) {
            Some(bar) => Some(bar.open_time),
            None if index == self.closed.len() => self.partial.map(|bar| bar.open_time),
            None => None,
        }
    }

    /// Last slot opened at/before the instant, clamped to the first slot.
    /// Empty series have no slot; equal open times select the latest slot.
    pub fn slot_at_time(self, timestamp_ms: i64) -> Option<usize> {
        if self.closed.is_empty() && self.partial.is_none() {
            return None;
        }
        if self
            .partial
            .is_some_and(|bar| bar.open_time <= timestamp_ms)
        {
            return Some(self.closed.len());
        }
        let after = self
            .closed
            .partition_point(|bar| bar.open_time <= timestamp_ms);
        Some(after.saturating_sub(1))
    }
}
