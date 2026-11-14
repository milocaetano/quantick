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
pub(crate) use quantick_indicator_session::LaneSample;
#[cfg(test)]
use quantick_indicator_session::fold_parked;
pub(crate) use quantick_indicator_session::{
    IndicatorCommand, IndicatorEvent, IndicatorSource, MAX_LANE_RUNGS, SlotId,
};
use quantick_indicator_session::{IndicatorSession, InputsRebound, SessionEffects};
#[cfg(test)]
use quantick_indicators::{IndicatorHost, InputValue, Rgba8};
use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc::channel;
use std::sync::mpsc::{Receiver, Sender, SyncSender, sync_channel};
#[cfg(test)]
#[path = "indicator_worker/tests/barrier.rs"]
pub(crate) mod barrier;
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
    fn event(&mut self, mut event: IndicatorEvent) {
        if let IndicatorEvent::Rebuilt { descriptor, .. } = &mut event {
            descriptor.title = quantick_stores::script_library::script_title(&descriptor.title);
            if let Some(title) = &mut descriptor.short_title {
                *title = quantick_stores::script_library::script_title(title);
            }
        }
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
        Self::spawn_with_progress(crate::worker_progress::monotonic())
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
}

#[cfg(test)]
impl IndicatorWorker {
    /// How many forming-bar updates have been sent down this channel.
    ///
    /// The cost the UI controls: the worker coalesces them anyway, so sending
    /// one per print is work that buys nothing.
    pub(crate) fn partial_updates_for_test(&self) -> usize {
        self.partial_updates.get()
    }

    pub(crate) fn lane_traffic_for_test(&self) -> usize {
        self.lane_traffic.get()
    }

    pub(crate) fn retained_lane_for_test(&self) -> (usize, usize) {
        let probe = self.lane_probe_for_test();
        (probe.len, probe.capacity)
    }

    /// The worker thread's own report on its forming run and heap work.
    pub(crate) fn lane_probe_for_test(&self) -> LaneProbe {
        let (tx, rx) = channel();
        self.send(WorkerCommand::InspectLane(tx));
        self.await_reply(&rx)
            .expect("the worker reports retained lane storage")
    }
}

fn run_observed(
    rx: &Receiver<WorkerCommand>,
    sender: &SyncSender<IndicatorEvent>,
    progress: Arc<SharedProgress>,
) {
    let _lifecycle = progress.lifecycle(std::thread::panicking);
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
mod tests;

#[cfg(test)]
mod script_load_tests;

#[cfg(test)]
mod object_event_tests;

#[cfg(test)]
mod set_inputs_tests;

#[cfg(test)]
mod reload_tests;

/// The bar paint channel end to end: the embedded script, the real worker,
/// the real delta events, the views the renderer reads.
///
/// Every other test in this change proves one link — the buffer, the
/// interpreter, the script's rules, the resolution across views. This is the
/// chain: a trader loading `force_bar.pine` from the menu gets colours on
/// candles, with the script's own defaults, and nothing here stubs a step.
#[cfg(test)]
mod paint_tests;

/// The marker channel end to end, for `exhaustion_reversal.pine`.
///
/// `exhaustion_reversal_semantics.rs` proves the rules against the corpus
/// copy with shrunken windows. This proves the other half: that the script
/// the menu actually offers, run with **its own declared defaults** through
/// the real worker and the real delta events, puts a triangle in the column
/// the renderer reads. A script can be semantically perfect and still reach
/// the chart with nothing on it.
#[cfg(test)]
mod exhaustion_reversal_chain_tests;
#[cfg(test)]
mod incremental_lane_tests;

#[cfg(test)]
mod event_backpressure_tests;

#[cfg(test)]
mod fold_tests;

#[cfg(test)]
mod progress_tests;

#[cfg(test)]
mod session_boundary_tests;
