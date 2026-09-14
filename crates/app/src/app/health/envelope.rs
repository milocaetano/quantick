//! The live envelope, observed: worker queues and retained tape, read at the
//! health-summary cadence and set against [`crate::live_envelope`].
//!
//! Window-wide on purpose. The rest of the summary speaks for the active
//! tab, but an overflow in a background tab's pane is an overflow all the
//! same, so every queue figure here sums, and the retained tape maxes, over
//! every pane of every tab. The one exception is the depth rate `live_rate`
//! reads, which is the active tab's — see [`rate_class`].

use std::collections::BTreeMap;

use super::worker_diagnostics::pane_workers;
use crate::live_envelope::{
    BURST_TRADES_PER_S, DEPTH_UPDATES_PER_S, REFERENCE_TICKS_PER_BAR, RETAINED_BARS_PER_PANE,
    RETAINED_TRADES_PER_PANE, SUSTAINED_TRADES_PER_S,
};
use crate::tab::Tab;
use crate::worker_progress::ProgressSnapshot;

/// What the summary line reports about the envelope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct EnvelopeReading {
    /// Commands in a worker channel not yet taken into a batch, summed.
    pub backlog: u64,
    /// Commands parked outside a full channel right now, summed.
    pub parked: u64,
    /// Commands that ever found their channel full, summed and cumulative.
    pub deferred: u64,
    /// Of those, the ones folded into a parked predecessor, cumulative.
    pub coalesced: u64,
    /// Worker output sends that waited on a full output channel because the
    /// UI was behind on its reads, summed and cumulative.
    pub output_blocked: u64,
    /// Each live worker's own cumulative `(deferred, output_blocked)`, by
    /// instance: a closed pane takes its counts out of the sums above, so
    /// only a per-worker comparison can tell a new overflow from an old one.
    pub pressure_by_worker: BTreeMap<u64, (u64, u64)>,
    /// The largest retained tape of any pane, in prints.
    pub retained_trades: usize,
    /// The bars that pane holds.
    pub retained_bars: usize,
    /// The pane that holds it, as `(tab, pane)`; `None` with no pane.
    pub retained_owner: Option<(u64, u64)>,
    /// Where the measured ingest rates sit against the envelope; see
    /// [`rate_class`].
    pub live_rate: &'static str,
}

impl EnvelopeReading {
    /// Fold one worker's snapshot in.
    fn add(&mut self, progress: &ProgressSnapshot) {
        let counts = &progress.counts;
        self.backlog = self.backlog.saturating_add(counts.queued);
        self.parked = self.parked.saturating_add(counts.parked);
        self.deferred = self.deferred.saturating_add(counts.deferred);
        self.coalesced = self.coalesced.saturating_add(counts.coalesced_parked);
        self.output_blocked = self.output_blocked.saturating_add(counts.output_blocked);
        if let Some(instance) = progress.instance {
            self.pressure_by_worker
                .insert(instance, (counts.deferred, counts.output_blocked));
        }
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

/// Where the measured ingest sits against one pane's envelope: `inside` at
/// or under the sustained trade rate and the depth rate, `burst` at or under
/// the burst trade rate, `above` past either.
///
/// Two scopes, because those are the two rates the summary has: the trade
/// rate is the window's, summed over every tab, so it can only overstate
/// what one pane takes in; the depth rate is the active tab's, the book the
/// rest of the summary line describes. A replay played fast is `above` by
/// design; the queue figures beside it say whether the workers kept up.
pub(in crate::app) fn rate_class(
    window_trades_per_s: f64,
    active_depth_per_s: f64,
) -> &'static str {
    #[allow(clippy::cast_precision_loss)]
    let (sustained, burst, depth) = (
        SUSTAINED_TRADES_PER_S as f64,
        BURST_TRADES_PER_S as f64,
        DEPTH_UPDATES_PER_S as f64,
    );
    if window_trades_per_s > burst || active_depth_per_s > depth {
        "above"
    } else if window_trades_per_s > sustained {
        "burst"
    } else {
        "inside"
    }
}

/// Read every pane's workers and tape, and classify the measured rates. Two
/// locks per worker, at summary cadence only.
pub(in crate::app) fn observe(
    tabs: &[Tab],
    window_trades_per_s: f64,
    active_depth_per_s: f64,
) -> EnvelopeReading {
    let mut reading = EnvelopeReading {
        live_rate: rate_class(window_trades_per_s, active_depth_per_s),
        ..EnvelopeReading::default()
    };
    for tab in tabs {
        for (pane, _side) in tab.panes() {
            for (_kind, progress) in pane_workers(pane) {
                reading.add(&progress);
            }
            let retained = pane.state.trades().len();
            if reading.retained_owner.is_none() || retained > reading.retained_trades {
                reading.retained_trades = retained;
                reading.retained_bars = pane.state.bars().len();
                reading.retained_owner = Some((tab.id, pane.id));
            }
        }
    }
    reading
}

/// What the previous summary already said, so each warning below speaks
/// once per event rather than once per summary.
#[derive(Debug, Default)]
pub(in crate::app) struct EnvelopeWatch {
    /// Each worker's cumulative `(deferred, output_blocked)` at the last
    /// summary.
    pressure_by_worker: BTreeMap<u64, (u64, u64)>,
    /// The pane last reported past the envelope, until the largest tape is
    /// back inside it.
    exceeded_owner: Option<(u64, u64)>,
}

impl EnvelopeWatch {
    /// Warn, at summary cadence, about the two ways a window leaves the
    /// envelope: a worker queue that filled since the last summary (a
    /// command queue that parked, or an output channel the UI was behind on),
    /// and a pane retaining more tape than the envelope states. Neither loses
    /// data (the commands were parked or waited, the prints are all still
    /// there), so each is a warning that the window runs outside what was
    /// measured, not an error. The retained-tape warning fires when a pane
    /// first crosses the envelope, or another pane becomes the one past it,
    /// and not on every summary after: nothing trims the tape, so it would
    /// repeat forever.
    pub(in crate::app) fn warn(&mut self, reading: &EnvelopeReading) {
        let (new_deferred, new_blocked) = reading.pressure_by_worker.iter().fold(
            (0_u64, 0_u64),
            |(deferred_sum, blocked_sum), (instance, (deferred, blocked))| {
                let (was_deferred, was_blocked) = self
                    .pressure_by_worker
                    .get(instance)
                    .copied()
                    .unwrap_or_default();
                (
                    deferred_sum.saturating_add(deferred.saturating_sub(was_deferred)),
                    blocked_sum.saturating_add(blocked.saturating_sub(was_blocked)),
                )
            },
        );
        self.pressure_by_worker
            .clone_from(&reading.pressure_by_worker);
        if new_deferred > 0 || new_blocked > 0 {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LIVE_QUEUE_DEFERRED",
                deferred_since_summary = new_deferred,
                output_blocked_since_summary = new_blocked,
                worker_deferred = reading.deferred,
                worker_coalesced = reading.coalesced,
                worker_parked = reading.parked,
                worker_output_blocked = reading.output_blocked,
                action = "inspect_worker_cost",
                "a worker queue filled; commands were parked or waited in order, none dropped"
            );
        }
        let Some(excess) = reading.retained_excess() else {
            self.exceeded_owner = None;
            return;
        };
        if self.exceeded_owner == reading.retained_owner {
            return;
        }
        self.exceeded_owner = reading.retained_owner;
        let (tab, pane) = reading.retained_owner.unwrap_or_default();
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LIVE_ENVELOPE_EXCEEDED",
            tab,
            pane,
            retained_trades = reading.retained_trades,
            envelope_trades = RETAINED_TRADES_PER_PANE,
            retained_bars = reading.retained_bars,
            envelope_bars = RETAINED_BARS_PER_PANE,
            envelope_bars_ticks_per_bar = REFERENCE_TICKS_PER_BAR,
            excess_trades = excess,
            action = "none_evicted_see_live_envelope_doc",
            "a pane retains more tape than the supported live envelope; nothing was evicted"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_progress::Counts;

    fn snapshot(instance: u64, counts: Counts) -> ProgressSnapshot {
        let mut progress = crate::worker_progress::WorkerProgress::new()
            .observer()
            .snapshot();
        progress.instance = Some(instance);
        progress.counts = counts;
        progress
    }

    fn deferred(instance: u64, deferred: u64) -> ProgressSnapshot {
        snapshot(
            instance,
            Counts {
                deferred,
                ..Counts::default()
            },
        )
    }

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
        reading.add(&snapshot(1, busy));
        reading.add(&snapshot(2, busy));
        assert_eq!(
            (
                reading.backlog,
                reading.parked,
                reading.deferred,
                reading.coalesced
            ),
            (6, 4, 10, 2)
        );
        assert_eq!(
            reading.pressure_by_worker,
            BTreeMap::from([(1, (5, 0)), (2, (5, 0))])
        );
        reading.add(&deferred(3, u64::MAX));
        assert_eq!(reading.deferred, u64::MAX);
    }

    /// Every event `warn` emits under a JSON subscriber, as parsed rows.
    fn warnings(watch: &mut EnvelopeWatch, reading: &EnvelopeReading) -> Vec<serde_json::Value> {
        #[derive(Clone)]
        struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Sink(std::sync::Arc::default());
        let writer = sink.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .without_time()
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || watch.warn(reading));
        let bytes = sink.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn codes(rows: &[serde_json::Value]) -> Vec<&str> {
        rows.iter()
            .map(|row| row["fields"]["event_code"].as_str().unwrap())
            .collect()
    }

    fn reading_of(workers: &[ProgressSnapshot]) -> EnvelopeReading {
        let mut reading = EnvelopeReading::default();
        for worker in workers {
            reading.add(worker);
        }
        reading
    }

    #[test]
    fn a_new_overflow_warns_once_and_an_old_one_stays_quiet() {
        let mut watch = EnvelopeWatch::default();
        assert!(warnings(&mut watch, &EnvelopeReading::default()).is_empty());
        let overflowed = reading_of(&[deferred(1, 40)]);
        let rows = warnings(&mut watch, &overflowed);
        assert_eq!(codes(&rows), ["LIVE_QUEUE_DEFERRED"]);
        assert_eq!(rows[0]["fields"]["deferred_since_summary"], 40);
        assert_eq!(rows[0]["level"], "WARN");
        // The cumulative count has not moved: nothing new to say.
        assert!(warnings(&mut watch, &overflowed).is_empty());
    }

    #[test]
    fn a_worker_waiting_on_a_ui_behind_on_its_reads_warns_too() {
        let mut watch = EnvelopeWatch::default();
        let waited = reading_of(&[snapshot(
            4,
            Counts {
                output_blocked: 12,
                ..Counts::default()
            },
        )]);
        let rows = warnings(&mut watch, &waited);
        assert_eq!(codes(&rows), ["LIVE_QUEUE_DEFERRED"]);
        assert_eq!(rows[0]["fields"]["output_blocked_since_summary"], 12);
        assert_eq!(rows[0]["fields"]["deferred_since_summary"], 0);
        assert!(warnings(&mut watch, &waited).is_empty());
    }

    #[test]
    fn an_overflow_after_a_busier_pane_closed_still_warns() {
        let mut watch = EnvelopeWatch::default();
        warnings(
            &mut watch,
            &reading_of(&[deferred(1, 5_000), deferred(2, 0)]),
        );
        // Worker 1 is gone with its 5,000; worker 2 overflowed 3,000 times.
        // The window-wide total fell, and the news must still be told.
        let rows = warnings(&mut watch, &reading_of(&[deferred(2, 3_000)]));
        assert_eq!(codes(&rows), ["LIVE_QUEUE_DEFERRED"]);
        assert_eq!(rows[0]["fields"]["deferred_since_summary"], 3_000);
    }

    #[test]
    fn a_pane_past_the_envelope_warns_once_per_crossing() {
        let mut watch = EnvelopeWatch::default();
        let edge = EnvelopeReading {
            retained_trades: RETAINED_TRADES_PER_PANE,
            retained_owner: Some((3, 9)),
            ..EnvelopeReading::default()
        };
        assert!(warnings(&mut watch, &edge).is_empty());
        let past = EnvelopeReading {
            retained_trades: RETAINED_TRADES_PER_PANE + 25,
            ..edge.clone()
        };
        let rows = warnings(&mut watch, &past);
        assert_eq!(codes(&rows), ["LIVE_ENVELOPE_EXCEEDED"]);
        let fields = &rows[0]["fields"];
        assert_eq!(
            (fields["tab"].as_u64(), fields["pane"].as_u64()),
            (Some(3), Some(9))
        );
        assert_eq!(fields["excess_trades"], 25);
        // Still past it on the next summary: already said.
        assert!(warnings(&mut watch, &past).is_empty());
        // Another pane becomes the one past it: that is news.
        let other = EnvelopeReading {
            retained_owner: Some((3, 10)),
            ..past.clone()
        };
        assert_eq!(
            codes(&warnings(&mut watch, &other)),
            ["LIVE_ENVELOPE_EXCEEDED"]
        );
        // Back inside, then past again: a second crossing.
        assert!(warnings(&mut watch, &edge).is_empty());
        assert_eq!(
            codes(&warnings(&mut watch, &past)),
            ["LIVE_ENVELOPE_EXCEEDED"]
        );
    }
}
