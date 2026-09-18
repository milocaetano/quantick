//! Indicator runtime: bounded admission, lifecycle and synchronous effects.
//! The headless session owns source binding, batch reduction and publication.
use crate::live_envelope::{INDICATOR_COMMAND_QUEUE, INDICATOR_EVENT_QUEUE};
use crate::worker_progress::{
    Coalescing, ObservedOutput, ObservedSender, ProgressSnapshot, SharedProgress, WorkerProgress,
};
use quantick_engine::Bar;
use quantick_engine::trade_tape::TradeTape;
#[cfg(test)]
use quantick_engine::{Trade, forming_run::FormingRun};
#[cfg(test)]
use quantick_indicator_session::fold_parked;
pub(crate) use quantick_indicator_session::{
    IndicatorCommand, IndicatorEvent, IndicatorSource, LaneSample, MAX_LANE_RUNGS, SlotId,
};
use quantick_indicator_session::{IndicatorSession, InputsRebound, SessionEffects};
#[cfg(test)]
use quantick_indicators::{IndicatorHost, InputValue, Rgba8};
use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc::channel;
use std::sync::mpsc::{Receiver, Sender, SyncSender, sync_channel};
#[cfg(test)]
#[path = "indicator_worker/tests/event_fixture.rs"]
pub(crate) mod event_fixture;
/// Transport markers stay outside the domain command vocabulary.
pub(crate) enum WorkerCommand {
    Domain(IndicatorCommand),
    #[allow(dead_code)]
    Flush(Sender<()>),
    #[cfg(test)]
    InspectLane(Sender<LaneProbe>),
}
impl From<IndicatorCommand> for WorkerCommand {
    fn from(command: IndicatorCommand) -> Self {
        Self::Domain(command)
    }
}
fn fold_commands(older: &mut WorkerCommand, newer: WorkerCommand) -> Option<WorkerCommand> {
    match (older, newer) {
        (WorkerCommand::Domain(older), WorkerCommand::Domain(newer)) => {
            quantick_indicator_session::fold_parked(older, newer).map(WorkerCommand::Domain)
        }
        (_, newer) => Some(newer),
    }
}
struct RuntimeEffects<'a> {
    output: ObservedOutput<'a, IndicatorEvent>,
}
impl SessionEffects for RuntimeEffects<'_> {
    fn event(&mut self, event: IndicatorEvent) {
        let _ = self.output.send(event);
    }
    fn inputs_rebound(&mut self, d: InputsRebound<'_>) {
        tracing::warn!(target: "quantick::app", schema_version = 1_u8,
            event_code = "INDICATOR_INPUTS_REBOUND", script = %d.script,
            saved = d.saved, declared = d.declared, kept = d.kept,
            count_changed = d.count_changed, action = "bound_by_position",
            "saved settings were rebound to a changed input list");
    }
}
/// Producer cursor for the worker-owned forming run. No trade storage lives here.
#[derive(Default)]
pub(crate) struct LaneTransport {
    rungs: usize,
    sent: usize,
}

impl LaneTransport {
    pub(crate) fn reset(&mut self) {
        self.sent = 0;
    }

    pub(crate) fn set_rungs(&mut self, rungs: usize) -> bool {
        let changed = self.rungs != rungs;
        if rungs == 0 {
            self.reset();
        }
        self.rungs = rungs;
        changed
    }

    /// The forming-bar update for this frame: the partial, and the forming
    /// run's prints not yet sent.
    ///
    /// Rate: **per frame**. The copy is the unsent suffix of the tape only —
    /// the prints since the previous command, or the forming bar's whole run
    /// after a reset — taken chunk slice by chunk slice, never the tape.
    pub(crate) fn command(&mut self, partial: Option<Bar>, trades: &TradeTape) -> IndicatorCommand {
        let count = partial
            .as_ref()
            .filter(|_| self.rungs > 0)
            .map_or(0, |bar| {
                usize::try_from(bar.trade_count)
                    .unwrap_or(usize::MAX)
                    .min(trades.len())
            });
        let start = trades.len() - count + self.sent.min(count);
        let mut run = Vec::with_capacity(trades.len() - start);
        for slice in trades.slices(start..) {
            run.extend_from_slice(slice);
        }
        self.sent = count;
        IndicatorCommand::PartialUpdated {
            partial,
            run,
            rungs: self.rungs,
        }
    }
}

/// What the worker thread reports about its forming run and its own work.
#[cfg(test)]
pub(crate) struct LaneProbe {
    /// Prints the forming run holds.
    pub len: usize,
    /// Prints its storage can hold without growing.
    pub capacity: usize,
    /// Prints the worker thread has folded into bars, appends and walks,
    /// over every run it has held.
    pub folds: u64,
    /// The worker thread's heap work since it started; its largest copy is
    /// the largest since the previous probe.
    pub heap: crate::work_meter::Tally,
}

/// UI-side handle: send commands, drain events each frame.
pub(crate) struct IndicatorWorker {
    commands: ObservedSender<WorkerCommand>,
    events: Receiver<IndicatorEvent>,
    /// Forming-bar updates sent, so a test can hold the UI to one per drain.
    #[cfg(test)]
    partial_updates: std::cell::Cell<usize>,
    #[cfg(test)]
    lane_traffic: std::cell::Cell<usize>,
}

impl IndicatorWorker {
    /// Spawn the indicator thread.
    #[must_use]
    pub(crate) fn spawn() -> Self {
        Self::spawn_with_progress(WorkerProgress::new())
    }

    pub(crate) fn spawn_with_progress(progress: WorkerProgress) -> Self {
        Self::spawn_bounded(progress, INDICATOR_EVENT_QUEUE)
    }

    /// Spawn with an event channel of `event_capacity`: production uses
    /// [`INDICATOR_EVENT_QUEUE`]; a test shrinks it to reach the full path.
    pub(crate) fn spawn_bounded(progress: WorkerProgress, event_capacity: usize) -> Self {
        let (cmd_tx, cmd_rx) = sync_channel::<WorkerCommand>(INDICATOR_COMMAND_QUEUE);
        let (evt_tx, evt_rx) = sync_channel::<IndicatorEvent>(event_capacity);
        let observed = progress.consumer();
        std::thread::Builder::new()
            .name("quantick-indicators".to_owned())
            .spawn(move || run_observed(&cmd_rx, &evt_tx, observed))
            .expect("spawn indicator worker thread");
        Self {
            commands: progress.bind_merging(cmd_tx, fold_commands),
            events: evt_rx,
            #[cfg(test)]
            partial_updates: std::cell::Cell::new(0),
            #[cfg(test)]
            lane_traffic: std::cell::Cell::new(0),
        }
    }

    /// Queue one command, or park it when the bounded queue is full; either
    /// way it reaches the worker and the caller never waits. A send failure
    /// means the worker died — worth a log line.
    pub(crate) fn send(&self, command: impl Into<WorkerCommand>) {
        let command = command.into();
        #[cfg(test)]
        if let WorkerCommand::Domain(IndicatorCommand::PartialUpdated { run, .. }) = &command {
            self.partial_updates.set(self.partial_updates.get() + 1);
            self.lane_traffic.set(self.lane_traffic.get() + run.len());
        }
        if self.commands.send(command).is_err() {
            tracing::error!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_WORKER_DOWN",
                action = "indicators_frozen_until_restart",
                "indicator worker thread is gone; indicator commands are being dropped"
            );
        }
    }

    pub(crate) fn progress(&self) -> ProgressSnapshot {
        self.commands.snapshot()
    }

    /// Every event the worker published since the last drain.
    ///
    /// Also the per-frame retry point for commands parked behind a full
    /// queue: one length read when nothing is parked.
    pub(crate) fn drain_events(&self) -> Vec<IndicatorEvent> {
        self.commands.pump();
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }

    /// How many forming-bar updates have been sent down this channel.
    ///
    /// The cost the UI controls: the worker coalesces them anyway, so sending
    /// one per print is work that buys nothing.
    #[cfg(test)]
    pub(crate) fn partial_updates_for_test(&self) -> usize {
        self.partial_updates.get()
    }

    #[cfg(test)]
    pub(crate) fn lane_traffic_for_test(&self) -> usize {
        self.lane_traffic.get()
    }

    #[cfg(test)]
    pub(crate) fn retained_lane_for_test(&self) -> (usize, usize) {
        let probe = self.lane_probe_for_test();
        (probe.len, probe.capacity)
    }

    /// The worker thread's own report on its forming run and heap work.
    #[cfg(test)]
    pub(crate) fn lane_probe_for_test(&self) -> LaneProbe {
        let (tx, rx) = channel();
        self.send(WorkerCommand::InspectLane(tx));
        self.await_reply(&rx)
            .expect("the worker reports retained lane storage")
    }

    /// Wait up to ten seconds for a reply to a command just sent, pumping any
    /// parked commands meanwhile — a test is its own UI frame loop, and a
    /// parked barrier would otherwise wait on a queue nobody refills.
    #[cfg(test)]
    pub(crate) fn await_reply<T>(&self, rx: &Receiver<T>) -> Option<T> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            self.commands.pump();
            if let Ok(reply) = rx.recv_timeout(std::time::Duration::from_millis(5)) {
                return Some(reply);
            }
        }
        None
    }

    /// Block until every command sent before this call has been applied and
    /// its events published. Tests use this to make the pipeline
    /// deterministic (pattern: `BookWorker::flush`).
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(WorkerCommand::Flush(ack_tx));
        let _ = self.await_reply(&ack_rx);
    }
}

fn run_observed(
    rx: &Receiver<WorkerCommand>,
    sender: &SyncSender<IndicatorEvent>,
    progress: Arc<SharedProgress>,
) {
    let _lifecycle = progress.lifecycle();
    let mut effects = RuntimeEffects {
        output: ObservedOutput {
            sender,
            progress: &progress,
        },
    };
    let mut session = IndicatorSession::new();
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }
        progress.begin(batch.len());
        let mut coalescing = Coalescing::new(&progress);
        let mut applying = session.begin_batch(batch.iter().enumerate().filter_map(
            |(index, command)| match command {
                WorkerCommand::Domain(command) => Some((index, command)),
                _ => None,
            },
        ));
        coalescing.inputs = applying.inputs_superseded();
        let mut flushes = Vec::new();
        for (index, command) in batch.into_iter().enumerate() {
            match command {
                WorkerCommand::Domain(command) => {
                    applying.apply(index, command, &mut coalescing.partials, &mut effects)
                }
                WorkerCommand::Flush(ack) => flushes.push(ack),
                #[cfg(test)]
                WorkerCommand::InspectLane(ack) => {
                    let d = applying.lane_diagnostics();
                    let _ = ack.send(LaneProbe {
                        len: d.len,
                        capacity: d.capacity,
                        folds: d.folds,
                        heap: crate::work_meter::tally(),
                    });
                    crate::work_meter::reset_largest();
                }
            }
        }
        let publishing = applying.finish_applying();
        coalescing.publishing();
        publishing.publish(&mut effects);
        progress.finish(false);
        for ack in flushes {
            let _ = ack.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::IndicatorViews;
    use quantick_engine::{BarBuilder as _, Side, TickBarBuilder, Trade, golden as engine_golden};

    /// The ladder of `run` as the worker samples it.
    fn lane_prefixes(run: &[Trade], rungs: usize) -> Vec<Bar> {
        let mut forming = FormingRun::default();
        forming.extend(run.to_vec());
        forming.prefixes(rungs)
    }
    use quantick_indicators::{PlotId, SourceId, native::Ema};
    use rust_decimal::Decimal;

    pub(super) fn trade(i: u64) -> Trade {
        Trade {
            agg_id: i,
            timestamp_ms: 1_000 + i as i64 * 100,
            // A deterministic ±2 wiggle so the EMA has something to smooth.
            price: Decimal::from(100 + (i % 5) as i64 - 2),
            quantity: Decimal::ONE,
            side: if i.is_multiple_of(2) {
                Side::Buy
            } else {
                Side::Sell
            },
        }
    }

    fn bars_and_partial(n_trades: u64, tick: u64) -> (Vec<Bar>, Option<Bar>) {
        let trades: Vec<Trade> = (1..=n_trades).map(trade).collect();
        let mut builder = TickBarBuilder::new(tick);
        let bars = engine_golden::replay(&mut builder, &trades);
        (bars, builder.partial().cloned())
    }

    /// The §6 host/worker property: applying the delta-event stream to the
    /// UI-side views reproduces exactly the columns a directly-driven host
    /// holds — backfill, live closes, preview and all.
    #[test]
    fn delta_stream_replays_to_identical_columns() {
        let (bars, partial) = bars_and_partial(11, 2);
        let split = bars.len() - 2;

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Native {
                id: "native.ema".to_owned(),
                values: vec![InputValue::Int(3), InputValue::Source(SourceId::Close)],
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars[..split].to_vec()));
        for bar in &bars[split..] {
            worker.send(IndicatorCommand::BarClosed(bar.clone()));
        }
        worker.send(IndicatorCommand::PartialUpdated {
            partial: partial.clone(),
            run: Vec::new(),
            rungs: 0,
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        let mut host = IndicatorHost::new();
        let id = host.add(Box::new(Ema::new(3, SourceId::Close)));
        for bar in &bars {
            host.push_closed_bar(bar);
        }
        host.set_partial(partial.as_ref());

        let view = &views.all()[0];
        assert_eq!(view.descriptor.title, "EMA(3)");
        assert_eq!(
            format!("{:?}", view.columns[0]),
            format!("{:?}", host.plots(id).unwrap().column(PlotId::new(0))),
            "delta replay must equal the direct host run, bit for bit"
        );
        assert!(partial.is_some(), "fixture must exercise the preview path");
        assert_eq!(
            view.preview.as_ref().map(|f| format!("{:?}", f.values)),
            host.preview(id).map(|f| format!("{:?}", f.values)),
        );
    }

    /// A Rebuild (spec switch shape) replaces the columns wholesale and the
    /// result matches a host fed the new bars from scratch.
    #[test]
    fn rebuild_resyncs_the_views() {
        let (coarse, _) = bars_and_partial(12, 3);
        let (fine, fine_partial) = bars_and_partial(12, 2);

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(coarse));
        worker.send(IndicatorCommand::Rebuild(
            fine.clone(),
            fine_partial.clone(),
        ));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        let mut host = IndicatorHost::new();
        let id = host.add(Box::new(quantick_indicators::native::Cvd::new()));
        host.rebuild(&fine, fine_partial.as_ref());

        let view = &views.all()[0];
        assert_eq!(
            format!("{:?}", view.columns[0]),
            format!("{:?}", host.plots(id).unwrap().column(PlotId::new(0))),
        );
        assert_eq!(view.rows, fine.len());
    }

    /// Removing a slot stops its events; the other slot keeps flowing.
    #[test]
    fn remove_isolates_the_survivor() {
        let (bars, _) = bars_and_partial(8, 2);
        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let doomed = views.allocate_slot("test.indicator");
        let survivor = views.allocate_slot("test.indicator");
        for (slot, source) in [
            (
                doomed,
                IndicatorSource::Native {
                    id: "native.cvd".to_owned(),
                    values: Vec::new(),
                },
            ),
            (
                survivor,
                IndicatorSource::Native {
                    id: "native.cvd".to_owned(),
                    values: Vec::new(),
                },
            ),
        ] {
            worker.send(IndicatorCommand::Add { slot, source });
        }
        worker.send(IndicatorCommand::Backfilled(bars[..2].to_vec()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        assert_eq!(views.all().len(), 2);

        views.remove(doomed);
        worker.send(IndicatorCommand::Remove(doomed));
        worker.send(IndicatorCommand::BarClosed(bars[2].clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        assert_eq!(views.all().len(), 1);
        assert_eq!(views.all()[0].slot, survivor);
        assert_eq!(views.all()[0].rows, 3, "the survivor saw the new bar");
    }

    /// The rung budget is a ceiling on cost, and the newest print is never the
    /// one dropped to meet it: the lane's right edge is the live edge.
    #[test]
    fn a_ladder_respects_its_budget_and_always_reaches_the_newest_print() {
        let run: Vec<Trade> = (1..=50).map(trade).collect();

        for rungs in [1_usize, 3, 7, 64] {
            let prefixes = lane_prefixes(&run, rungs);
            assert!(
                prefixes.len() <= rungs.max(1) + 1,
                "{rungs} rungs asked for, {} produced",
                prefixes.len()
            );
            let last = prefixes.last().expect("a non-empty run has rungs");
            assert_eq!(
                last.trade_count,
                run.len() as u64,
                "the last rung is the whole run"
            );
            assert_eq!(last.close_time, run[run.len() - 1].timestamp_ms);
        }
    }

    /// A rung is a prefix of the run, folded exactly as a builder folds. The
    /// last one must therefore *be* the forming bar the chart is drawing —
    /// otherwise the lane's right edge and the candle beside it disagree.
    #[test]
    fn the_last_rung_is_the_forming_bar_itself() {
        let (_, partial) = bars_and_partial(11, 4);
        let partial = partial.expect("the fixture leaves a bar forming");
        let run: Vec<Trade> = (1..=11)
            .map(trade)
            .filter(|t| t.timestamp_ms >= partial.open_time)
            .collect();

        let prefixes = lane_prefixes(&run, 8);
        assert_eq!(prefixes.last(), Some(&partial));
    }

    /// No lane (`rungs == 0`) and no run are both "walk nothing": a chart
    /// without a tape must not pay for a ladder it cannot draw.
    #[test]
    fn no_lane_and_no_run_both_produce_no_rungs() {
        let run: Vec<Trade> = (1..=5).map(trade).collect();
        assert!(lane_prefixes(&run, 0).is_empty());
        assert!(lane_prefixes(&[], 16).is_empty());
    }

    /// End to end through the worker: a partial published with its run comes
    /// back as lane samples whose last rung equals the slot's own preview.
    /// The curve's live end and the pane's headline number are the same fact,
    /// and this is what keeps them from drifting apart.
    #[test]
    fn the_lane_samples_end_where_the_preview_does() {
        let (bars, partial) = bars_and_partial(11, 4);
        let partial = partial.expect("the fixture leaves a bar forming");
        let run: Vec<Trade> = (1..=11)
            .map(trade)
            .filter(|t| t.timestamp_ms >= partial.open_time)
            .collect();

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.send(IndicatorCommand::PartialUpdated {
            partial: Some(partial),
            run: run.clone(),
            rungs: 8,
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        let view = &views.all()[0];
        assert!(
            !view.lane.is_empty(),
            "a run with a lane budget produces rungs"
        );
        assert_eq!(
            view.lane.last().map(|sample| sample.close_time),
            run.last().map(|t| t.timestamp_ms),
            "the newest rung sits at the newest print"
        );
        assert_eq!(
            view.lane
                .last()
                .map(|sample| format!("{:?}", sample.values)),
            view.preview
                .as_ref()
                .map(|frame| format!("{:?}", frame.values)),
            "the lane's live end is the preview"
        );
        assert!(
            view.lane
                .windows(2)
                .all(|pair| pair[0].close_time <= pair[1].close_time),
            "rungs are in occurrence order"
        );
    }

    /// A publish with no lane budget clears whatever the lane was drawing.
    /// Toggling the tape off must not leave a frozen curve behind.
    #[test]
    fn dropping_the_lane_clears_the_samples_already_published() {
        let (bars, partial) = bars_and_partial(11, 4);
        let partial = partial.expect("the fixture leaves a bar forming");
        let run: Vec<Trade> = (1..=11)
            .map(trade)
            .filter(|t| t.timestamp_ms >= partial.open_time)
            .collect();

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.send(IndicatorCommand::PartialUpdated {
            partial: Some(partial.clone()),
            run,
            rungs: 8,
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        assert!(!views.all()[0].lane.is_empty(), "the fixture drew a lane");

        worker.send(IndicatorCommand::PartialUpdated {
            partial: Some(partial),
            run: Vec::new(),
            rungs: 0,
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        let view = &views.all()[0];
        assert!(view.lane.is_empty(), "no lane, no curve");
        assert!(
            view.preview.is_some(),
            "and the forming bar still previews — the walk restored it"
        );
    }
}

#[cfg(test)]
mod script_load_tests {
    use super::*;
    use crate::indicators::IndicatorViews;

    /// The M2 acceptance: a script using request.security loads into an
    /// error slot whose message carries the line number and the stable code.
    #[test]
    fn a_rejected_script_surfaces_its_error_with_line_and_code() {
        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "sec.pine".to_owned(),
                text: "//@version=5\nindicator(\"t\")\ns = request.security(close)\nplot(s)\n"
                    .to_owned(),
            },
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        let error = view.error.as_ref().expect("the slot carries the error");
        assert!(error.message.contains("sec.pine:3:"), "{}", error.message);
        assert!(
            error.message.contains("PINE_NO_SECURITY"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("activity-sampled"),
            "{}",
            error.message
        );
    }

    /// An embedded starter script runs end to end through the worker: bars
    /// in, plot columns out — the whole scripted pipe.
    #[test]
    fn an_embedded_script_plots_through_the_worker() {
        let (name, text) = crate::indicators::library::EMBEDDED_SCRIPTS[0];
        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: name.to_owned(),
                text: text.to_owned(),
            },
        });
        let trades: Vec<quantick_engine::Trade> = (1..=24).map(tests::trade).collect();
        let mut builder = quantick_engine::TickBarBuilder::new(2);
        let bars = quantick_engine::golden::replay(&mut builder, &trades);
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        assert!(view.error.is_none(), "{:?}", view.error);
        assert_eq!(view.descriptor.title, "EMA");
        assert!(view.descriptor.overlay);
        assert_eq!(view.rows, bars.len());
        // EMA(9) over 12 bars: warmup NaN then values.
        let column = &view.columns[0];
        assert!(column[0].is_nan());
        assert!(column.last().is_some_and(|v| !v.is_nan()));
    }
}

#[cfg(test)]
mod object_event_tests {
    use super::*;
    use crate::indicators::IndicatorViews;

    /// The M3 acceptance through the worker: the embedded zigzag script
    /// produces committed lines and HH/LH/HL/LL labels, published on the
    /// Objects event and surviving a rebuild.
    #[test]
    fn zigzag_objects_flow_to_the_views_and_survive_rebuild() {
        let (_, text) = crate::indicators::library::EMBEDDED_SCRIPTS
            .iter()
            .find(|(name, _)| *name == "zigzag.pine")
            .expect("zigzag is embedded");
        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "zigzag.pine".to_owned(),
                text: (*text).to_owned(),
            },
        });
        // A wiggling tape (period-5 price cycle) makes pivots inevitable.
        let trades: Vec<quantick_engine::Trade> = (1..=60).map(tests::trade).collect();
        let mut builder = quantick_engine::TickBarBuilder::new(1);
        let bars = quantick_engine::golden::replay(&mut builder, &trades);
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        assert!(view.error.is_none(), "{:?}", view.error);
        let objects = view.render_objects();
        assert!(!objects.lines.is_empty(), "swings draw segments");
        assert!(!objects.labels.is_empty(), "pivots draw labels");
        let texts: Vec<&str> = objects.labels.iter().map(|l| l.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| *t == "HH" || *t == "LH"),
            "highs are classified: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| *t == "HL" || *t == "LL"),
            "lows are classified: {texts:?}"
        );

        // A rebuild over the same bars reproduces the identical object set —
        // determinism through the seek/spec-switch path.
        let before = objects.clone();
        worker.send(IndicatorCommand::Rebuild(bars, None));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        assert_eq!(
            *views.all()[0].render_objects(),
            before,
            "same bars in, same objects out"
        );
    }
}

#[cfg(test)]
mod set_inputs_tests {
    use super::*;
    use crate::indicators::IndicatorViews;
    use quantick_indicators::{IndicatorHost, PlotId, SourceId, native::Ema};

    /// Applying settings = construct anew + replace + replay: the columns
    /// The second implementer of the input port: a script's bound values must
    /// move its output, which nothing in the workspace exercised.
    #[test]
    fn set_inputs_binds_a_scripts_declared_input() {
        let trades: Vec<quantick_engine::Trade> = (1..=8).map(tests::trade).collect();
        let mut builder = quantick_engine::TickBarBuilder::new(2);
        let bars = quantick_engine::golden::replay(&mut builder, &trades);

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "scaled.pine".to_owned(),
                text: "//@version=5
indicator(\"scaled\")
k = input.int(1, \"k\")
plot(close * k)
"
                .to_owned(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.send(IndicatorCommand::SetInputs {
            slot,
            values: vec![InputValue::Int(3)],
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        let view = &views.all()[0];
        assert_eq!(view.input_values, vec![InputValue::Int(3)], "bound");
        let plotted = &view.columns[0];
        let expected: Vec<f64> = bars
            .iter()
            .map(|b| b.close.to_string().parse::<f64>().unwrap_or(f64::NAN) * 3.0)
            .collect();
        assert_eq!(
            format!("{plotted:?}"),
            format!("{expected:?}"),
            "the script's output moved with its input"
        );
    }

    /// after SetInputs must equal a host that ran the new inputs from bar
    /// zero, and the UI must receive a full Rebuilt carrying the new values.
    #[test]
    fn set_inputs_recomputes_like_a_fresh_instance() {
        let trades: Vec<quantick_engine::Trade> = (1..=20).map(tests::trade).collect();
        let mut builder = quantick_engine::TickBarBuilder::new(2);
        let bars = quantick_engine::golden::replay(&mut builder, &trades);

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Native {
                id: "native.ema".to_owned(),
                values: vec![InputValue::Int(3), InputValue::Source(SourceId::Close)],
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.send(IndicatorCommand::SetInputs {
            slot,
            values: vec![InputValue::Int(2), InputValue::Source(SourceId::Hl2)],
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        let mut reference = IndicatorHost::new();
        let id = reference.add(Box::new(Ema::new(2, SourceId::Hl2)));
        for bar in &bars {
            reference.push_closed_bar(bar);
        }

        let view = &views.all()[0];
        assert_eq!(view.descriptor.title, "EMA(2, hl2)", "descriptor followed");
        assert_eq!(view.input_values[0], InputValue::Int(2), "values followed");
        assert_eq!(
            view.input_values[1],
            InputValue::Source(SourceId::Hl2),
            "the source cell is bound too — applying `Close` to an instance that already had it proved nothing"
        );
        assert_eq!(
            format!("{:?}", view.columns[0]),
            format!("{:?}", reference.plots(id).unwrap().column(PlotId::new(0))),
            "SetInputs must replay as if the new inputs had always been set"
        );
    }
}

#[cfg(test)]
mod reload_tests {
    use super::*;
    use crate::indicators::IndicatorViews;

    const GOOD_V1: &str = "//@version=5\nindicator(\"r\")\nplot(close)\n";
    const GOOD_V2: &str = "//@version=5\nindicator(\"r2\")\nplot(close * 2)\n";
    const BROKEN: &str = "//@version=5\nindicator(\"r\")\nplot(request.security(close))\n";

    fn drive() -> (
        IndicatorWorker,
        IndicatorViews,
        SlotId,
        Vec<quantick_engine::Bar>,
    ) {
        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "r.pine".to_owned(),
                text: GOOD_V1.to_owned(),
            },
        });
        let trades: Vec<quantick_engine::Trade> = (1..=8).map(tests::trade).collect();
        let mut builder = quantick_engine::TickBarBuilder::new(2);
        let bars = quantick_engine::golden::replay(&mut builder, &trades);
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        // Drain the initial publication (a frame passes before any reload
        // in reality; batching them would let the initial Rebuilt clear the
        // stale flag the reload sets).
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        (worker, views, slot, bars)
    }

    #[test]
    fn a_good_reload_recompiles_and_replays() {
        let (worker, mut views, slot, bars) = drive();
        worker.send(IndicatorCommand::Reload {
            slot,
            source: IndicatorSource::Script {
                name: "r.pine".to_owned(),
                text: GOOD_V2.to_owned(),
            },
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        assert_eq!(view.descriptor.title, "r2", "the new version runs");
        assert!(view.stale.is_none());
        assert_eq!(view.rows, bars.len(), "replayed over the full history");
    }

    #[test]
    fn a_broken_reload_keeps_the_last_good_version_and_flags_stale() {
        let (worker, mut views, slot, bars) = drive();
        worker.send(IndicatorCommand::Reload {
            slot,
            source: IndicatorSource::Script {
                name: "r.pine".to_owned(),
                text: BROKEN.to_owned(),
            },
        });
        worker.send(IndicatorCommand::BarClosed(bars[0].clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        assert_eq!(view.descriptor.title, "r", "the old version keeps running");
        assert!(view.error.is_none(), "stale is not a runtime error");
        let stale = view.stale.as_ref().expect("flagged stale");
        assert!(stale.contains("PINE_NO_SECURITY"), "{stale}");
        assert_eq!(
            view.rows,
            bars.len() + 1,
            "the running version even saw the new bar"
        );
    }

    /// The flag belongs to the worker, so an unrelated rebuild cannot clear
    /// it. Before this, scrolling back to prepend history dropped the amber
    /// dot while the pre-edit script was still the one running — and it never
    /// came back, because the poll had already advanced its mtime.
    #[test]
    fn a_rebuild_does_not_clear_the_stale_flag() {
        let (worker, mut views, slot, bars) = drive();
        worker.send(IndicatorCommand::Reload {
            slot,
            source: IndicatorSource::Script {
                name: "r.pine".to_owned(),
                text: BROKEN.to_owned(),
            },
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        assert!(views.all()[0].stale.is_some(), "flagged after a bad edit");

        // Exactly what scrolling left does.
        worker.send(IndicatorCommand::Rebuild(bars.clone(), None));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        assert!(
            view.stale.is_some(),
            "the file on disk still has errors; the dot must stay"
        );
        assert_eq!(view.descriptor.title, "r", "and the old version still runs");
    }

    /// The branch the commit message advertises — "a slot whose first load
    /// failed is healed by its first good reload" — reachable whenever a
    /// script reads but does not compile.
    #[test]
    fn a_slot_that_never_loaded_is_healed_by_a_good_reload() {
        let trades: Vec<quantick_engine::Trade> = (1..=6).map(tests::trade).collect();
        let mut builder = quantick_engine::TickBarBuilder::new(2);
        let bars = quantick_engine::golden::replay(&mut builder, &trades);

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "r.pine".to_owned(),
                text: BROKEN.to_owned(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        assert!(views.all()[0].error.is_some(), "the first load failed");

        worker.send(IndicatorCommand::Reload {
            slot,
            source: IndicatorSource::Script {
                name: "r.pine".to_owned(),
                text: GOOD_V1.to_owned(),
            },
        });
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }
        let view = &views.all()[0];
        assert!(view.error.is_none(), "the good reload healed the slot");
        assert_eq!(
            view.rows,
            bars.len(),
            "and the healed instance caught up over the existing history"
        );
    }
}

/// The bar paint channel end to end: the embedded script, the real worker,
/// the real delta events, the views the renderer reads.
///
/// Every other test in this change proves one link — the buffer, the
/// interpreter, the script's rules, the resolution across views. This is the
/// chain: a trader loading `force_bar.pine` from the menu gets colours on
/// candles, with the script's own defaults, and nothing here stubs a step.
#[cfg(test)]
mod paint_tests {
    use super::*;
    use crate::indicators::IndicatorViews;
    use crate::indicators::library::EMBEDDED_SCRIPTS;
    use quantick_engine::{Side, TickBarBuilder, Trade, golden as engine_golden};
    use rust_decimal::Decimal;

    /// `color.silver` in the dialect's palette — what a bullish biggest bar
    /// wears with the script's declared defaults (force holds the loud
    /// white/yellow pair; the range extreme is context and whispers).
    const SILVER: Rgba8 = Rgba8::new(0xB2, 0xB5, 0xBE, 0xFF);

    fn print(id: u64, price: i64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: 1_000 + id as i64 * 100,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    /// 22 tick(2) bars: twenty quiet ones (body and range 1), then one ten
    /// times as wide, then quiet again.
    ///
    /// The script's default windows are 20 bars, so bar 20 is the first that
    /// can be judged at all — and it is the widest of its window by a factor
    /// of ten, which is the least ambiguous thing a tape can say.
    fn tape() -> Vec<Bar> {
        let mut trades = Vec::new();
        for bar in 0..20u64 {
            trades.push(print(bar * 2 + 1, 100));
            trades.push(print(bar * 2 + 2, 101));
        }
        trades.push(print(41, 100));
        trades.push(print(42, 110));
        trades.push(print(43, 100));
        trades.push(print(44, 101));
        engine_golden::replay(&mut TickBarBuilder::new(2), &trades)
    }

    #[test]
    fn the_embedded_force_bar_paints_candles_through_the_whole_chain() {
        let bars = tape();
        assert_eq!(bars.len(), 22, "fixture shape");

        let source = EMBEDDED_SCRIPTS
            .iter()
            .find(|(name, _)| *name == "force_bar.pine")
            .expect("force_bar.pine is embedded")
            .1;

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("script.force_bar");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "force_bar.pine".to_owned(),
                text: source.to_owned(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        assert!(views.all()[0].error.is_none(), "the script loaded");
        assert_eq!(
            views.all()[0].rows,
            bars.len(),
            "a script with no plots still commits a row per bar — the row \
             count cannot come from a column that does not exist"
        );
        assert!(views.paints_any(), "the chart now has paint to look up");
        assert_eq!(
            views.bar_paint(20),
            Some(SILVER),
            "the widest bar of its window, bullish, with the declared defaults"
        );
        assert_eq!(views.bar_paint(0), None, "warm-up paints nothing");
        assert_eq!(
            views.bar_paint(21),
            None,
            "and the quiet bar after the big one is ordinary again"
        );
    }
}

/// The marker channel end to end, for `exhaustion_reversal.pine`.
///
/// `exhaustion_reversal_semantics.rs` proves the rules against the corpus
/// copy with shrunken windows. This proves the other half: that the script
/// the menu actually offers, run with **its own declared defaults** through
/// the real worker and the real delta events, puts a triangle in the column
/// the renderer reads. A script can be semantically perfect and still reach
/// the chart with nothing on it.
#[cfg(test)]
mod exhaustion_reversal_chain_tests {
    use super::*;
    use crate::indicators::IndicatorViews;
    use crate::indicators::library::EMBEDDED_SCRIPTS;
    use quantick_engine::{Side, TickBarBuilder, Trade, golden as engine_golden};
    use rust_decimal::Decimal;

    fn print(id: u64, price: i64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: 1_000 + id as i64 * 100,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    /// Quiet bars before the force bar. The 20-bar body average this script
    /// defaults to first exists on bar 20 (`body[1]` is `na` on bar 0 and
    /// that NaN sits in the window until it slides out), so a fixture with
    /// exactly 20 would put the force bar on the very first bar that can be
    /// judged at all — and any edit that shifts warm-up by one would fail
    /// this test with an empty marker column, which reads as a broken
    /// channel rather than as a fixture one bar short.
    const WARMUP_BARS: usize = 25;

    /// 29 tick(2) bars sized for the script's *defaults*: the quiet bullish
    /// run above (body 1) to warm a 20-bar body average and a 10-bar extreme,
    /// then a body-10 bar taking out the high, then three bearish bars
    /// handing 80% of it back.
    fn tape() -> Vec<Bar> {
        // Two prints per bar, so each pair below *is* one bar's open and
        // close (and, with no third print, its low and high).
        let mut prices: Vec<i64> = Vec::new();
        for _ in 0..WARMUP_BARS {
            prices.extend([100, 101]);
        }
        // 20: the force bar — body 10 against an average of 1, high 111
        // against a 10-bar extreme of 101.
        prices.extend([101, 111]);
        // 21..23: three bearish bars, the last closing at 103 — 80% of the
        // force bar's range given back, on the third bar of the run.
        prices.extend([111, 109]);
        prices.extend([109, 107]);
        prices.extend([107, 103]);

        let trades: Vec<Trade> = prices
            .iter()
            .enumerate()
            .map(|(index, price)| print(index as u64 + 1, *price))
            .collect();
        engine_golden::replay(&mut TickBarBuilder::new(2), &trades)
    }

    /// Rows of `title` carrying a mark, read the way the renderer reads them.
    fn marks(views: &IndicatorViews, title: &str) -> Vec<usize> {
        let view = &views.all()[0];
        let index = view
            .descriptor
            .plots
            .iter()
            .position(|plot| plot.title == title)
            .unwrap_or_else(|| panic!("plot {title:?} is declared"));
        view.columns[index]
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.is_nan())
            .map(|(row, _)| row)
            .collect()
    }

    #[test]
    fn the_embedded_exhaustion_reversal_marks_through_the_whole_chain() {
        let bars = tape();
        assert_eq!(bars.len(), WARMUP_BARS + 4, "fixture shape");

        let source = EMBEDDED_SCRIPTS
            .iter()
            .find(|(name, _)| *name == "exhaustion_reversal.pine")
            .expect("exhaustion_reversal.pine is embedded")
            .1;

        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("script.exhaustion_reversal");
        worker.send(IndicatorCommand::Add {
            slot,
            source: IndicatorSource::Script {
                name: "exhaustion_reversal.pine".to_owned(),
                text: source.to_owned(),
            },
        });
        worker.send(IndicatorCommand::Backfilled(bars.clone()));
        worker.flush();
        for event in worker.drain_events() {
            views.apply(event);
        }

        assert!(views.all()[0].error.is_none(), "the script loaded");
        assert_eq!(views.all()[0].rows, bars.len());
        assert_eq!(
            marks(&views, "Exhaustion reversal: sell"),
            vec![WARMUP_BARS + 3],
            "the triangle lands on the bar closing the give-back, with the \
             defaults a trader gets from the menu — no test-only inputs"
        );
        assert_eq!(
            marks(&views, "Exhaustion reversal: buy"),
            Vec::<usize>::new(),
            "and the other side stays empty on a tape that only fades a rally"
        );
    }
}
#[cfg(test)]
mod incremental_lane_tests {
    use super::*;
    use crate::indicators::IndicatorViews;
    use quantick_engine::Side;
    use rust_decimal::Decimal;

    fn print(id: u64, quantity: i64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: id as i64,
            price: Decimal::from(100),
            quantity: Decimal::from(quantity.abs()),
            side: if quantity > 0 { Side::Buy } else { Side::Sell },
        }
    }

    fn bar(run: &[Trade]) -> Bar {
        let mut bar = Bar::opened_by(&run[0]);
        for trade in &run[1..] {
            bar.extend(trade);
        }
        bar
    }

    fn update(run: &[Trade], new: &[Trade], rungs: usize) -> IndicatorCommand {
        IndicatorCommand::PartialUpdated {
            partial: (!run.is_empty()).then(|| bar(run)),
            run: new.to_vec(),
            rungs,
        }
    }

    fn add() -> IndicatorCommand {
        IndicatorCommand::Add {
            slot: SlotId(0),
            source: IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        }
    }

    /// Queue everything before running: this exercises one actual worker batch.
    fn one_batch(commands: Vec<IndicatorCommand>) -> (IndicatorViews, Vec<LaneSample>) {
        let (tx, rx) = sync_channel(INDICATOR_COMMAND_QUEUE);
        let (events, output) = sync_channel(INDICATOR_EVENT_QUEUE);
        let progress = WorkerProgress::new();
        let observed = progress.consumer();
        let tx = progress.bind_merging(tx, fold_commands);
        for command in commands {
            tx.send(command.into()).unwrap();
        }
        drop(tx);
        run_observed(&rx, &events, observed);
        drop(events);
        collect(output.try_iter().collect())
    }

    /// Flush-only batches may emit an empty Lane in the existing event protocol.
    /// Keep the last nonempty publication to inspect the evaluated partial itself.
    fn collect(events: Vec<IndicatorEvent>) -> (IndicatorViews, Vec<LaneSample>) {
        let mut views = IndicatorViews::new();
        views.allocate_slot("native.cvd");
        let mut last_lane = Vec::new();
        for event in events {
            if let IndicatorEvent::Lane { samples, .. } = &event
                && !samples.is_empty()
            {
                last_lane = samples.clone();
            }
            views.apply(event);
        }
        (views, last_lane)
    }

    fn assert_final(views: &IndicatorViews, lane: &[LaneSample]) {
        // History is +10. The signed live prints are +2,-1,+4,-2,+3,-1,+5.
        // Seven prints sampled with three rungs end at prints 3,6,7.
        assert_eq!(views.all()[0].columns, vec![vec![10.0]]);
        assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![20.0]);
        assert_eq!(
            lane,
            &[
                LaneSample {
                    close_time: 3,
                    values: vec![15.0]
                },
                LaneSample {
                    close_time: 6,
                    values: vec![15.0]
                },
                LaneSample {
                    close_time: 7,
                    values: vec![20.0]
                },
            ]
        );
    }

    #[test]
    fn irregular_drains_and_batched_commands_match_independent_cvd_rungs() {
        let prints: Vec<_> = [2, -1, 4, -2, 3, -1, 5]
            .into_iter()
            .enumerate()
            .map(|(index, quantity)| print(index as u64 + 1, quantity))
            .collect();
        let history = bar(&[print(0, 10)]);
        let (whole, whole_lane) = one_batch(vec![
            add(),
            IndicatorCommand::Backfilled(vec![history.clone()]),
            update(&prints, &prints, 3),
        ]);
        assert_final(&whole, &whole_lane);
        let (batched, batched_lane) = one_batch(vec![
            add(),
            IndicatorCommand::Backfilled(vec![history.clone()]),
            update(&prints[..1], &prints[..1], 3),
            update(&prints[..4], &prints[1..4], 3),
            update(&prints[..4], &[], 3),
            update(&prints[..6], &prints[4..6], 3),
            update(&prints, &prints[6..], 3),
        ]);
        assert_final(&batched, &batched_lane);
        let worker = IndicatorWorker::spawn();
        worker.send(add());
        worker.send(IndicatorCommand::Backfilled(vec![history]));
        let mut before = 0;
        let mut events = Vec::new();
        for after in [1, 4, 4, 6, 7] {
            worker.send(update(&prints[..after], &prints[before..after], 3));
            worker.flush();
            events.extend(worker.drain_events());
            assert_eq!(worker.retained_lane_for_test().0, after);
            before = after;
        }
        let (separate, separate_lane) = collect(events);
        assert_final(&separate, &separate_lane);
    }

    #[test]
    fn close_cuts_an_earlier_partial_before_the_next_run_in_the_same_batch() {
        let old = [print(1, 2), print(2, -1)];
        let next = [print(3, 7), print(4, -2)];
        let (views, lane) = one_batch(vec![
            add(),
            IndicatorCommand::Backfilled(vec![bar(&[print(0, 10)])]),
            update(&old, &old, 8),
            IndicatorCommand::BarClosed(bar(&old)),
            update(&next, &next[..1], 8),
            update(&next, &next[1..], 8),
        ]);
        assert_eq!(views.all()[0].columns, vec![vec![10.0, 11.0]]);
        assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![16.0]);
        assert_eq!(
            lane,
            vec![
                LaneSample {
                    close_time: 3,
                    values: vec![18.0]
                },
                LaneSample {
                    close_time: 4,
                    values: vec![16.0]
                },
            ]
        );
        let (closed, lane) = one_batch(vec![
            add(),
            update(&old, &old, 8),
            IndicatorCommand::BarClosed(bar(&old)),
        ]);
        assert_eq!(closed.all()[0].columns, vec![vec![1.0]]);
        assert!(closed.all()[0].preview.is_none());
        assert!(lane.is_empty());
    }

    #[test]
    fn rebuild_backfill_vanished_partial_and_disabled_lane_cut_stale_runs() {
        let stale = [print(1, 90)];
        let current = [print(2, 3), print(3, -1)];
        for reset in [
            IndicatorCommand::Rebuild(vec![bar(&[print(0, 20)])], None),
            IndicatorCommand::Backfilled(vec![bar(&[print(0, 20)])]),
        ] {
            let (views, lane) = one_batch(vec![
                add(),
                update(&stale, &stale, 8),
                reset,
                update(&current, &current, 8),
            ]);
            assert_eq!(views.all()[0].columns, vec![vec![20.0]]);
            assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![22.0]);
            assert_eq!(
                lane,
                vec![
                    LaneSample {
                        close_time: 2,
                        values: vec![23.0]
                    },
                    LaneSample {
                        close_time: 3,
                        values: vec![22.0]
                    },
                ]
            );
        }
        for reset in [update(&[], &[], 8), update(&stale, &[], 0)] {
            let (views, lane) = one_batch(vec![
                add(),
                update(&stale, &stale, 8),
                reset,
                update(&current, &current, 8),
                update(&current, &[], 8),
            ]);
            assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![2.0]);
            assert_eq!(
                lane,
                vec![
                    LaneSample {
                        close_time: 2,
                        values: vec![3.0]
                    },
                    LaneSample {
                        close_time: 3,
                        values: vec![2.0]
                    },
                ]
            );
        }
    }

    #[test]
    fn retired_epoch_releases_peak_capacity_and_preserves_small_run_outputs() {
        fn publish(worker: &IndicatorWorker, views: &mut IndicatorViews) -> Vec<LaneSample> {
            worker.flush();
            let mut lane = Vec::new();
            for event in worker.drain_events() {
                // Flush/probe-only batches may publish an empty lane afterwards.
                if let IndicatorEvent::Lane { samples, .. } = &event
                    && !samples.is_empty()
                {
                    lane = samples.clone();
                }
                views.apply(event);
            }
            lane
        }

        let large: Vec<_> = (1..=4096)
            .map(|id| print(id, if id % 2 == 1 { 1 } else { -1 }))
            .collect();
        let history = bar(&[print(0, 10)]);
        for (reset, mut columns) in [
            (IndicatorCommand::BarClosed(bar(&large)), vec![10.0, 10.0]),
            (
                IndicatorCommand::Backfilled(vec![history.clone()]),
                vec![10.0],
            ),
            (
                IndicatorCommand::Rebuild(vec![history.clone()], None),
                vec![10.0],
            ),
        ] {
            let worker = IndicatorWorker::spawn();
            let mut views = IndicatorViews::new();
            views.allocate_slot("native.cvd");
            worker.send(add());
            worker.send(IndicatorCommand::Backfilled(vec![history.clone()]));
            worker.send(update(&large[..2048], &large[..2048], 2));
            publish(&worker, &mut views);
            assert_eq!(worker.retained_lane_for_test().0, 2048);
            worker.send(update(&large, &large[2048..], 2));
            let lane = publish(&worker, &mut views);
            let (length, peak) = worker.retained_lane_for_test();
            assert_eq!(length, 4096);
            assert!(peak >= 4096);
            assert_eq!(views.all()[0].columns, vec![vec![10.0]]);
            assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![10.0]);
            assert_eq!(
                lane,
                vec![
                    LaneSample {
                        close_time: 2048,
                        values: vec![10.0]
                    },
                    LaneSample {
                        close_time: 4096,
                        values: vec![10.0]
                    },
                ]
            );

            worker.send(reset);
            assert!(publish(&worker, &mut views).is_empty());
            assert_eq!(worker.retained_lane_for_test(), (0, 0));
            assert_eq!(views.all()[0].columns, vec![columns.clone()]);
            assert!(views.all()[0].preview.is_none());

            // Two successive small epochs must not inherit the retired peak.
            for (run, expected, preview) in [
                ([print(4097, 7), print(4098, -2)], [17.0, 15.0], 15.0),
                ([print(4099, 3), print(4100, -1)], [18.0, 17.0], 17.0),
            ] {
                worker.send(update(&run[..1], &run[..1], 2));
                publish(&worker, &mut views);
                assert_eq!(worker.retained_lane_for_test().0, 1);
                worker.send(update(&run, &run[1..], 2));
                let lane = publish(&worker, &mut views);
                let (length, capacity) = worker.retained_lane_for_test();
                assert_eq!(length, 2);
                assert!(capacity < peak);
                assert_eq!(views.all()[0].columns, vec![columns.clone()]);
                assert_eq!(
                    views.all()[0].preview.as_ref().unwrap().values,
                    vec![preview]
                );
                assert_eq!(
                    lane,
                    vec![
                        LaneSample {
                            close_time: run[0].timestamp_ms,
                            values: vec![expected[0]]
                        },
                        LaneSample {
                            close_time: run[1].timestamp_ms,
                            values: vec![expected[1]]
                        },
                    ]
                );
                worker.send(update(&run, &[], 2));
                assert_eq!(publish(&worker, &mut views), lane);
                assert_eq!(worker.retained_lane_for_test(), (length, capacity));
                worker.send(IndicatorCommand::BarClosed(bar(&run)));
                assert!(publish(&worker, &mut views).is_empty());
                assert_eq!(worker.retained_lane_for_test(), (0, 0));
                columns.push(preview);
                assert_eq!(views.all()[0].columns, vec![columns.clone()]);
                assert!(views.all()[0].preview.is_none());
            }
        }
    }
}

#[cfg(test)]
mod event_backpressure_tests;

#[cfg(test)]
mod fold_tests;

#[cfg(test)]
mod progress_tests;

#[cfg(test)]
mod session_boundary_tests;
