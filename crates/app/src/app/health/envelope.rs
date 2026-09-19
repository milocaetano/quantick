//! The live envelope, observed: worker queues and retained tape, read at the
//! health-summary cadence and set against [`crate::live_envelope`].
//!
//! Window-wide on purpose. The rest of the summary speaks for the active
//! tab, but an overflow in a background tab's pane is an overflow all the
//! same, so every queue figure here sums, and the retained tape maxes, over
//! every pane of every tab. The one exception is the depth rate `live_rate`
//! reads, which is the active tab's — see [`rate_class`].

use super::worker_diagnostics::pane_workers;
pub(in crate::app) use quantick_backpressure::envelope::watch::{
    EnvelopeReading, EnvelopeWatch, rate_class,
};

/// Read every pane's workers and tape, and classify the measured rates. Two
/// locks per worker, at summary cadence only.
pub(in crate::app) fn observe(
    tabs: &crate::app::arrangement_host::ArrangementHost,
    window_trades_per_s: f64,
    active_depth_per_s: f64,
) -> EnvelopeReading {
    let mut reading = EnvelopeReading {
        live_rate: rate_class(window_trades_per_s, active_depth_per_s),
        ..EnvelopeReading::default()
    };
    for (tab_id, tab) in tabs.iter_with_ids() {
        for (pane, _side) in tab.panes() {
            for (_kind, progress) in pane_workers(pane) {
                reading.add(&progress);
            }
            let retained = pane.state.trades().len();
            if reading.retained_owner.is_none() || retained > reading.retained_trades {
                reading.retained_trades = retained;
                reading.retained_bars = pane.state.bars().len();
                reading.retained_owner = Some((tab_id, pane.id));
            }
        }
    }
    reading
}
