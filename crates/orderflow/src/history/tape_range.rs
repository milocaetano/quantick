//! Bounded reads of the retained tape for a window that is not the live one.

use super::{Aggression, LiquidityHistory};

impl LiquidityHistory {
    /// Retained prints a cut at `from_ms` must look at, up to the first one
    /// delivered after the tape had reached `reach_ms`; `None` walks to the
    /// newest delivery, as [`Self::aggressions_since`] does.
    ///
    /// Both ends bisect the running newest timestamp, so a cut an hour back
    /// costs its own stretch, never the hour after it. A print delivered once
    /// the tape was already past `reach_ms` is left out, which is why a past
    /// window reaches one window beyond its end: a print later than that was
    /// never on the live tape either.
    pub fn aggressions_between(
        &self,
        from_ms: i64,
        reach_ms: Option<i64>,
    ) -> impl Iterator<Item = &Aggression> {
        let start = self
            .aggression_max_ms
            .partition_point(|&newest| newest < from_ms);
        let end = reach_ms.map_or(self.aggressions.len(), |reach| {
            self.aggression_max_ms
                .partition_point(|&newest| newest < reach)
        });
        self.aggressions.range(start..end.max(start))
    }

    /// The first instant the retained tape is complete from: where recording
    /// began, or just after the newest print retention, a cap or a grid reset
    /// evicted. `None` before any stream reached this history.
    #[must_use]
    pub fn tape_retained_from_ms(&self) -> Option<i64> {
        let after_eviction = self
            .evicted_through_ms
            .map(|horizon| horizon.saturating_add(1));
        match (self.first_stream_ms, after_eviction) {
            (Some(recorded), after) => Some(after.map_or(recorded, |after| recorded.max(after))),
            (None, after) => after,
        }
    }
}
