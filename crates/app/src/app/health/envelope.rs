//! The live envelope, observed: worker queues and retained tape, read at the
//! health-summary cadence and set against [`crate::live_envelope`].
//!
//! Window-wide on purpose. The rest of the summary speaks for the active
//! tab, but an overflow in a background tab's pane is an overflow all the
//! same, so every figure here sums (queues) or maxes (retained tape) over
//! every pane of every tab.

use crate::live_envelope::{
    BURST_TRADES_PER_S, DEPTH_UPDATES_PER_S, RETAINED_TRADES_PER_PANE, SUSTAINED_TRADES_PER_S,
};
use crate::tab::Tab;
use crate::worker_progress::Counts;

/// What the summary line reports about the envelope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct EnvelopeReading {
    /// Commands in a worker channel not yet taken into a batch, summed.
    pub backlog: u64,
    /// Commands parked outside a full channel right now, summed.
    pub parked: u64,
    /// Commands that ever found their channel full, summed and cumulative.
    pub deferred: u64,
    /// Of those, the ones folded into a parked predecessor, cumulative.
    pub coalesced: u64,
    /// The largest retained tape of any pane, in prints.
    pub retained_trades: usize,
    /// The pane that holds it, as `(tab, pane)`; `None` with no pane.
    pub retained_owner: Option<(u64, u64)>,
    /// Where the measured ingest rates sit against the envelope; see
    /// [`rate_class`].
    pub live_rate: &'static str,
}

impl EnvelopeReading {
    /// Fold one worker's counts in.
    fn add(&mut self, counts: &Counts) {
        self.backlog = self.backlog.saturating_add(counts.queued);
        self.parked = self.parked.saturating_add(counts.parked);
        self.deferred = self.deferred.saturating_add(counts.deferred);
        self.coalesced = self.coalesced.saturating_add(counts.coalesced_parked);
    }

    /// Prints a pane holds beyond the envelope's retained history, if any.
    /// Nothing evicts them; this is the figure the warning reports.
    pub(in crate::app) fn retained_excess(&self) -> Option<usize> {
        outside_envelope(self.retained_trades)
    }
}

/// How far `retained` prints sit past [`RETAINED_TRADES_PER_PANE`], or
/// `None` inside it.
pub(in crate::app) fn outside_envelope(retained: usize) -> Option<usize> {
    retained
        .checked_sub(RETAINED_TRADES_PER_PANE)
        .filter(|excess| *excess > 0)
}

/// Where a window's measured ingest sits against one pane's envelope:
/// `inside` at or under the sustained trade rate and the depth rate, `burst`
/// at or under the burst trade rate, `above` past either. The window's rate
/// sums every tab, so it can only overstate what one pane takes in. A replay
/// played fast is `above` by design; the queue figures beside it say whether
/// the workers kept up anyway.
pub(in crate::app) fn rate_class(trades_per_s: f64, depth_per_s: f64) -> &'static str {
    #[allow(clippy::cast_precision_loss)]
    let (sustained, burst, depth) = (
        SUSTAINED_TRADES_PER_S as f64,
        BURST_TRADES_PER_S as f64,
        DEPTH_UPDATES_PER_S as f64,
    );
    if trades_per_s > burst || depth_per_s > depth {
        "above"
    } else if trades_per_s > sustained {
        "burst"
    } else {
        "inside"
    }
}

/// Read every pane's workers and tape, and classify the window's measured
/// rates. Two locks per worker, at summary cadence only.
pub(in crate::app) fn observe(
    tabs: &[Tab],
    trades_per_s: f64,
    depth_per_s: f64,
) -> EnvelopeReading {
    let mut reading = EnvelopeReading {
        live_rate: rate_class(trades_per_s, depth_per_s),
        ..EnvelopeReading::default()
    };
    for tab in tabs {
        for (pane, _side) in tab.panes() {
            reading.add(&pane.indicator_worker.progress().counts);
            if let Some(view) = &pane.orderflow {
                reading.add(&view.worker_progress().counts);
            }
            let retained = pane.state.trades().len();
            if reading.retained_owner.is_none() || retained > reading.retained_trades {
                reading.retained_trades = retained;
                reading.retained_owner = Some((tab.id, pane.id));
            }
        }
    }
    reading
}

/// Warn, at summary cadence, about the two ways a window leaves the
/// envelope: a worker queue that filled since the last summary, and a pane
/// retaining more tape than the envelope states. Neither loses data — the
/// commands were parked, the prints are all still there — so each is a
/// warning that the window is running outside what was measured, not an
/// error. `deferred_at_summary` carries the count between summaries.
pub(in crate::app) fn warn(reading: &EnvelopeReading, deferred_at_summary: &mut u64) {
    let new_deferred = reading.deferred.saturating_sub(*deferred_at_summary);
    *deferred_at_summary = reading.deferred;
    if new_deferred > 0 {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LIVE_QUEUE_DEFERRED",
            deferred_since_summary = new_deferred,
            worker_deferred = reading.deferred,
            worker_coalesced = reading.coalesced,
            worker_parked = reading.parked,
            action = "inspect_worker_cost",
            "a worker queue filled; commands were parked in order, none dropped"
        );
    }
    if let Some(excess) = reading.retained_excess() {
        let (tab, pane) = reading.retained_owner.unwrap_or_default();
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LIVE_ENVELOPE_EXCEEDED",
            tab,
            pane,
            retained_trades = reading.retained_trades,
            envelope_trades = RETAINED_TRADES_PER_PANE,
            excess_trades = excess,
            action = "none_evicted_see_live_envelope_doc",
            "a pane retains more tape than the supported live envelope; nothing was evicted"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_edge_is_inside_and_one_print_past_it_is_not() {
        assert_eq!(outside_envelope(0), None);
        assert_eq!(outside_envelope(RETAINED_TRADES_PER_PANE), None);
        assert_eq!(outside_envelope(RETAINED_TRADES_PER_PANE + 1), Some(1));
    }

    #[test]
    fn measured_rates_are_classed_against_the_envelope_at_its_edges() {
        let (sustained, burst, depth) = (
            SUSTAINED_TRADES_PER_S as f64,
            BURST_TRADES_PER_S as f64,
            DEPTH_UPDATES_PER_S as f64,
        );
        assert_eq!(rate_class(0.0, 0.0), "inside");
        assert_eq!(rate_class(sustained, depth), "inside");
        assert_eq!(rate_class(sustained + 1.0, 0.0), "burst");
        assert_eq!(rate_class(burst, depth), "burst");
        assert_eq!(rate_class(burst + 1.0, 0.0), "above");
        assert_eq!(rate_class(0.0, depth + 1.0), "above");
    }

    #[test]
    fn queue_figures_sum_across_workers_and_never_wrap() {
        let mut reading = EnvelopeReading::default();
        let busy = Counts {
            queued: 3,
            parked: 2,
            deferred: 5,
            coalesced_parked: 1,
            ..Counts::default()
        };
        reading.add(&busy);
        reading.add(&busy);
        assert_eq!(
            (
                reading.backlog,
                reading.parked,
                reading.deferred,
                reading.coalesced
            ),
            (6, 4, 10, 2)
        );
        reading.add(&Counts {
            deferred: u64::MAX,
            ..Counts::default()
        });
        assert_eq!(reading.deferred, u64::MAX);
    }
}
