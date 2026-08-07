//! Background thread that owns the [`IndicatorHost`].
//!
//! Modelled on [`crate::orderflow_worker`]: the UI thread never touches host
//! state. Commands go down an unbounded channel; **delta events** come back —
//! the UI owns its own copy of the plot columns and applies deltas, so a full
//! column set crosses the channel only on a rebuild, and appending a live bar
//! costs O(plots) per indicator, not a clone of history.
//!
//! Coalescing rule (mirrors `BookWorker::run`): the queue is drained into a
//! batch and only the newest `PartialUpdated` of the batch is evaluated —
//! preview cost is bounded by worker cadence, not by feed rate, so a 50×
//! replay cannot melt it. The partial is applied after the batch's closes,
//! which is always correct: a preview only ever describes the newest forming
//! bar.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, channel};

use quantick_engine::{Bar, Trade};
use quantick_indicators::{
    EvalError, Indicator, IndicatorDescriptor, IndicatorHost, InputValue, InstanceId,
    ObjectSnapshot, PreviewFrame, SourceId,
    native::{Cvd, Ema},
};

/// Most rungs a lane ladder is ever walked with.
///
/// The ceiling is a cost statement, not a resolution one: a rung is a full
/// evaluation of every hosted indicator, so the count is what bounds a
/// publish. Sixty-four rungs across a lane a few hundred pixels wide is finer
/// than the band can show, and the renderer asks for fewer than this whenever
/// the lane is narrower.
pub(crate) const MAX_LANE_RUNGS: usize = 64;

/// One rung of the lane ladder as the UI receives it: the instant on the
/// tape, and what one slot's plots showed there.
///
/// The forming bar only — see [`IndicatorHost::walk_partial_prefixes`] for
/// why the ladder cannot reach back past its open.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LaneSample {
    /// Exchange timestamp of the last print in this rung's prefix.
    pub close_time: i64,
    /// One value per declared plot, in descriptor order; NaN = nothing to
    /// draw, exactly as in a committed row.
    pub values: Vec<f64>,
}

/// Fold `run` into forming-bar prefixes, at most `rungs` of them.
///
/// The trades are the forming bar's own, in occurrence order, so folding all
/// of them reproduces the bar the chart is drawing — which is why the last
/// print always gets a rung: the lane's right edge is the live edge, and
/// stopping a sample short of it would draw the tape as if the newest prints
/// had not happened.
fn lane_prefixes(run: &[Trade], rungs: usize) -> Vec<Bar> {
    if run.is_empty() || rungs == 0 {
        return Vec::new();
    }
    let step = run.len().div_ceil(rungs.min(run.len())).max(1);
    let mut prefixes = Vec::with_capacity(run.len().div_ceil(step) + 1);
    let mut forming: Option<Bar> = None;
    for (index, trade) in run.iter().enumerate() {
        match &mut forming {
            None => forming = Some(Bar::opened_by(trade)),
            Some(bar) => bar.extend(trade),
        }
        let last = index + 1 == run.len();
        if last || (index + 1) % step == 0 {
            prefixes.push(forming.clone().expect("a trade opened the bar"));
        }
    }
    prefixes
}

/// UI-side handle for one indicator slot. The UI allocates these (so it can
/// track an indicator it just requested without waiting for the worker); the
/// worker maps them to the host's own instance ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SlotId(pub u64);

/// What to instantiate behind a slot.
#[derive(Debug, Clone)]
pub(crate) enum IndicatorSource {
    /// Native EMA over a selectable source series.
    NativeEma { len: usize, source: SourceId },
    /// Native cumulative volume delta pane.
    NativeCvd,
    /// A Quantick Pine script: display name + source text (the UI owns
    /// files; the worker only ever sees text).
    Script { name: String, text: String },
}

impl IndicatorSource {
    /// Build the indicator, or explain why the script does not load. The
    /// error string is the full human rendering — every problem, each with
    /// file:line:col and its stable code.
    fn build(&self) -> Result<Box<dyn Indicator>, String> {
        self.build_with(None)
    }

    /// Build with bound input values (the settings apply path). `None` =
    /// declared defaults. Values are defensive: a missing or mistyped cell
    /// falls back to its default rather than panicking the worker.
    fn build_with(&self, values: Option<&[InputValue]>) -> Result<Box<dyn Indicator>, String> {
        match self {
            IndicatorSource::NativeEma { len, source } => {
                let base = Ema::new(*len, *source);
                // The panel is generated from `InputSpec`; binding the values
                // back is generated too, via the trait, so a future native
                // cannot forget to extend a match here and have its settings
                // silently ignored.
                Ok(match values.and_then(|values| base.rebind(values)) {
                    Some(bound) => bound,
                    None => Box::new(base),
                })
            }
            IndicatorSource::NativeCvd => Ok(Box::new(Cvd::new())),
            IndicatorSource::Script { name, text } => match quantick_pine::compile(text, name) {
                Ok(compiled) => Ok(Box::new(match values {
                    Some(values) if values.len() == compiled.inputs.len() => {
                        quantick_pine::ScriptIndicator::with_inputs(
                            compiled,
                            text.clone(),
                            values.to_vec(),
                        )
                    }
                    _ => quantick_pine::ScriptIndicator::new(compiled, text.clone()),
                })),
                Err(errors) => Err(errors
                    .iter()
                    .map(|e| e.render(name, text))
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    )),
            },
        }
    }

    /// The constructor this instance was added through, as a stable string.
    ///
    /// It is the durable half of a pane's identity: unlike the slot id (a
    /// monotonic counter, so remove + add always yields a new one) and unlike
    /// the title (which moves with the inputs), this is the same string
    /// before and after the trader takes an indicator off the chart and puts
    /// it back. Drawings anchored to a pane are keyed on it — see
    /// [`crate::drawings::PaneKey`].
    ///
    /// Deliberately excludes the input values: changing a period changes the
    /// series, not which pane the trader was annotating.
    pub(crate) fn kind_id(&self) -> String {
        match self {
            IndicatorSource::NativeEma { .. } => "native.ema".to_owned(),
            IndicatorSource::NativeCvd => "native.cvd".to_owned(),
            IndicatorSource::Script { name, .. } => format!("script.{name}"),
        }
    }

    /// The display title used when the source cannot load (a healthy
    /// instance's title comes from its descriptor).
    fn fallback_title(&self) -> String {
        match self {
            IndicatorSource::NativeEma { len, .. } => format!("EMA({len})"),
            IndicatorSource::NativeCvd => "CVD".to_owned(),
            IndicatorSource::Script { name, .. } => name.clone(),
        }
    }
}

/// Commands mirror the host's mutation surface (plan §4.1).
pub(crate) enum IndicatorCommand {
    /// Initial history landed: replay it (equivalent to a rebuild).
    Backfilled(Vec<Bar>),
    /// One live bar closed.
    BarClosed(Bar),
    /// The forming bar changed (or vanished). Latest-wins within a batch.
    PartialUpdated {
        partial: Option<Bar>,
        /// The forming bar's own trades, in occurrence order — the run the
        /// lane ladder is folded from. Empty when nothing is forming.
        run: Vec<Trade>,
        /// How many rungs the chart's live lane can show. `0` means there is
        /// no lane on screen and no ladder is walked at all: a chart without
        /// a tape must not pay for one.
        rungs: usize,
    },
    /// Spec switch / prepended history / source reset: replay from scratch.
    Rebuild(Vec<Bar>, Option<Bar>),
    /// Instantiate `source` behind `slot` and catch it up over history.
    Add {
        slot: SlotId,
        source: IndicatorSource,
    },
    /// Rebind a slot input set: construct anew, replace, replay — the
    /// running instance never observes an input changing mid-stream.
    SetInputs {
        slot: SlotId,
        values: Vec<InputValue>,
    },
    /// Replace a slot's source wholesale (hot reload). On a compile error
    /// the last good version keeps running and the UI is told the slot is
    /// stale — an edit with errors must never take a working chart away.
    Reload {
        slot: SlotId,
        source: IndicatorSource,
    },
    /// Drop a slot.
    Remove(SlotId),
    /// Test barrier: acknowledged only after every earlier command has been
    /// applied and its events sent.
    #[allow(dead_code)]
    Flush(Sender<()>),
}

/// Delta events back to the UI. Bounded cost per event: only [`Rebuilt`]
/// carries bulk data, and only when history really was recomputed.
///
/// [`Rebuilt`]: IndicatorEvent::Rebuilt
pub(crate) enum IndicatorEvent {
    /// Full state of one slot (after add / backfill / rebuild): descriptor
    /// plus every committed plot column.
    Rebuilt {
        slot: SlotId,
        descriptor: IndicatorDescriptor,
        columns: Vec<Vec<f64>>,
        /// The values currently bound to the declared inputs (defaults on
        /// first load) — what the settings dialog opens with.
        inputs: Vec<InputValue>,
        /// The failed-reload errors, when the running version is older than
        /// the file on disk. Carried on every rebuild because the worker owns
        /// this flag: the UI mirrors it rather than guessing, so an unrelated
        /// rebuild (scrolling back to prepend history, a source reset) cannot
        /// quietly clear an amber dot while the stale code is still running.
        stale: Option<String>,
    },
    /// One committed row (one closed bar) for one slot.
    Appended { slot: SlotId, row: Vec<f64> },
    /// Latest forming-bar frame for one slot (`None`: partial vanished).
    Preview {
        slot: SlotId,
        frame: Option<PreviewFrame>,
    },
    /// The forming bar sampled across the live lane's window for one slot,
    /// oldest rung first. Empty when there is no lane or nothing is forming —
    /// which is how a vanished lane clears the curve it was drawing.
    Lane {
        slot: SlotId,
        samples: Vec<LaneSample>,
    },
    /// The slot's indicator failed and is disabled until rebuilt/replaced.
    Error { slot: SlotId, error: EvalError },
    /// The full retained draw-object set of one slot (bounded by the
    /// 500-per-kind caps; published only when its revision moved).
    Objects {
        slot: SlotId,
        objects: ObjectSnapshot,
    },
    /// A hot reload failed to compile; the previous version keeps running
    /// ("stale — edit has errors").
    ReloadFailed { slot: SlotId, message: String },
}

/// Worker-side bookkeeping for one slot: the host id plus what the UI is
/// known to have, so every event is an exact delta.
struct SlotMirror {
    /// `None`: the source never loaded (script compile error) — the slot
    /// exists only as a UI entry carrying its error.
    host_id: Option<InstanceId>,
    /// What to rebuild from when inputs change or a script reloads.
    source: IndicatorSource,
    /// Values currently bound to the declared inputs.
    values: Vec<InputValue>,
    /// Committed rows the UI has (via Rebuilt/Appended events).
    known_rows: usize,
    /// Whether the UI has been sent a `Rebuilt` for this slot's current
    /// shape. `known_rows == 0` cannot stand in for it: before the first bar
    /// closes — the whole first bar on a slow tape, and again after every
    /// reset or replay seek — that would re-send a full descriptor + column
    /// clone on every drained batch.
    synced: bool,
    /// Whether the UI has been told about the current error state.
    error_reported: bool,
    /// Set when a reload failed to compile: the instance still running is
    /// older than the file on disk. Cleared by the next good load.
    stale: Option<String>,
    /// Store revision last published to the UI (0 = nothing yet).
    objects_revision: u64,
}

/// UI-side handle: send commands, drain events each frame.
pub(crate) struct IndicatorWorker {
    commands: Sender<IndicatorCommand>,
    events: Receiver<IndicatorEvent>,
    /// Forming-bar updates sent, so a test can hold the UI to one per drain.
    #[cfg(test)]
    partial_updates: std::cell::Cell<usize>,
}

impl IndicatorWorker {
    /// Spawn the indicator thread.
    #[must_use]
    pub(crate) fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = channel::<IndicatorCommand>();
        let (evt_tx, evt_rx) = channel::<IndicatorEvent>();
        std::thread::Builder::new()
            .name("quantick-indicators".to_owned())
            .spawn(move || run(&cmd_rx, &evt_tx))
            .expect("spawn indicator worker thread");
        Self {
            commands: cmd_tx,
            events: evt_rx,
            #[cfg(test)]
            partial_updates: std::cell::Cell::new(0),
        }
    }

    /// Queue one command. A send failure means the worker died — worth a log
    /// line, never a full queue (the channel is unbounded and command volume
    /// is bounded by feed cadence).
    pub(crate) fn send(&self, command: IndicatorCommand) {
        #[cfg(test)]
        if matches!(command, IndicatorCommand::PartialUpdated { .. }) {
            self.partial_updates.set(self.partial_updates.get() + 1);
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

    /// Every event the worker published since the last drain.
    pub(crate) fn drain_events(&self) -> Vec<IndicatorEvent> {
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

    /// Block until every command sent before this call has been applied and
    /// its events published. Tests use this to make the pipeline
    /// deterministic (pattern: `BookWorker::flush`).
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(IndicatorCommand::Flush(ack_tx));
        let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(10));
    }
}

fn run(rx: &Receiver<IndicatorCommand>, events: &Sender<IndicatorEvent>) {
    let mut host = IndicatorHost::new();
    // BTreeMap: deterministic iteration order for event emission.
    let mut slots: BTreeMap<SlotId, SlotMirror> = BTreeMap::new();

    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }

        let mut flushes: Vec<Sender<()>> = Vec::new();
        // Latest-wins; `Some(None)` means "partial vanished" must be applied.
        let mut partial_update: Option<Option<Bar>> = None;
        // The run and rung budget that came with the newest partial of the
        // batch. Coalesced with it rather than separately: a ladder folded
        // from one batch's trades and previewed against another's forming bar
        // would draw a curve that never happened.
        let mut lane_request: Option<(Vec<Trade>, usize)> = None;
        // After a rebuild every slot's full columns go out; appends would be
        // redundant (the snapshot is taken after the whole batch).
        let mut rebuilt = false;

        for command in batch {
            match command {
                IndicatorCommand::Backfilled(bars) => {
                    host.rebuild(&bars, None);
                    rebuilt = true;
                }
                IndicatorCommand::BarClosed(bar) => host.push_closed_bar(&bar),
                IndicatorCommand::PartialUpdated {
                    partial,
                    run,
                    rungs,
                } => {
                    partial_update = Some(partial);
                    lane_request = Some((run, rungs));
                }
                IndicatorCommand::Rebuild(bars, partial) => {
                    host.rebuild(&bars, partial.as_ref());
                    rebuilt = true;
                    // The rebuild already previewed this partial; a stale
                    // earlier PartialUpdated in the same batch must not
                    // override it backwards. Its ladder goes with it — the
                    // run it was folded from belongs to the pre-rebuild
                    // series.
                    partial_update = None;
                    lane_request = None;
                }
                IndicatorCommand::Add { slot, source } => match source.build() {
                    Ok(indicator) => {
                        // Straight from the instance, not from the declared
                        // defaults: `Ema::new(3, ..)` still *declares* 9, so
                        // a mirror seeded from the schema would report a
                        // value the indicator is not running, and Apply
                        // without touching a widget would write it back.
                        let values = indicator.input_values();
                        let host_id = host.add(indicator);
                        slots.insert(
                            slot,
                            SlotMirror {
                                host_id: Some(host_id),
                                source,
                                values,
                                known_rows: 0,
                                synced: false,
                                error_reported: false,
                                stale: None,
                                objects_revision: 0,
                            },
                        );
                    }
                    Err(message) => {
                        // The slot still exists UI-side, carrying its load
                        // error: a script that does not compile is shown,
                        // with lines and codes, never silently dropped.
                        let _ = events.send(IndicatorEvent::Rebuilt {
                            slot,
                            descriptor: IndicatorDescriptor {
                                title: source.fallback_title(),
                                short_title: None,
                                overlay: false,
                                plots: Vec::new(),
                                inputs: Vec::new(),
                                fills: Vec::new(),
                            },
                            columns: Vec::new(),
                            inputs: Vec::new(),
                            stale: None,
                        });
                        let _ = events.send(IndicatorEvent::Error {
                            slot,
                            error: EvalError {
                                bar_index: 0,
                                message,
                            },
                        });
                        slots.insert(
                            slot,
                            SlotMirror {
                                host_id: None,
                                source,
                                values: Vec::new(),
                                known_rows: 0,
                                synced: true,
                                error_reported: true,
                                stale: None,
                                objects_revision: 0,
                            },
                        );
                    }
                },
                IndicatorCommand::SetInputs { slot, values } => {
                    if let Some(mirror) = slots.get_mut(&slot)
                        && let Some(host_id) = mirror.host_id
                    {
                        match mirror.source.build_with(Some(&values)) {
                            Ok(indicator) => {
                                // Mirror what the instance bound, not what
                                // was asked for: every fallback inside the
                                // build is silent, and a discarded input
                                // recorded as applied is exactly the
                                // "inferred data, silently patched" the
                                // honesty rule forbids.
                                mirror.values = indicator.input_values();
                                host.replace(host_id, indicator);
                                // Force a full Rebuilt on publish: the new
                                // instance replayed the whole history, and
                                // its descriptor may have changed too.
                                mirror.known_rows = 0;
                                mirror.synced = false;
                                mirror.error_reported = false;
                                mirror.objects_revision = 0;
                            }
                            Err(message) => {
                                let _ = events.send(IndicatorEvent::Error {
                                    slot,
                                    error: EvalError {
                                        bar_index: 0,
                                        message,
                                    },
                                });
                            }
                        }
                    }
                }
                IndicatorCommand::Reload { slot, source } => {
                    if let Some(mirror) = slots.get_mut(&slot) {
                        // Build with the values the user set, not with the
                        // declared defaults: editing a comment in a script
                        // used to reset `len = 50` back to 20 with no
                        // message. `build_with` falls back to the defaults
                        // by itself when the input set changed.
                        match source.build_with(Some(&mirror.values)) {
                            Ok(indicator) => {
                                let values = indicator.input_values();
                                match mirror.host_id {
                                    Some(host_id) => {
                                        host.replace(host_id, indicator);
                                    }
                                    // The slot never loaded (its first
                                    // compile failed): the reload is its
                                    // first working version.
                                    None => mirror.host_id = Some(host.add(indicator)),
                                }
                                mirror.source = source;
                                mirror.values = values;
                                // A reload can change the title, the plots
                                // and the input set, so the UI needs the
                                // whole slot again, not an append.
                                mirror.known_rows = 0;
                                mirror.synced = false;
                                mirror.error_reported = false;
                                mirror.objects_revision = 0;
                                // A good load is no longer stale.
                                mirror.stale = None;
                            }
                            Err(message) => {
                                mirror.stale = Some(message.clone());
                                let _ = events.send(IndicatorEvent::ReloadFailed { slot, message });
                            }
                        }
                    }
                }
                IndicatorCommand::Remove(slot) => {
                    if let Some(mirror) = slots.remove(&slot)
                        && let Some(host_id) = mirror.host_id
                    {
                        host.remove(host_id);
                    }
                }
                IndicatorCommand::Flush(ack) => flushes.push(ack),
            }
        }

        if let Some(partial) = partial_update {
            host.set_partial(partial.as_ref());
        }

        // The ladder is walked here, on the worker's own cadence, for the same
        // reason previews are: the cost is per drained batch, never per print
        // and never per frame, so a 50x replay cannot melt it.
        let mut lane = lane_request
            .map(|(run, rungs)| walk_lane(&mut host, &slots, &run, rungs))
            .unwrap_or_default();

        publish_deltas(&host, &mut slots, events, rebuilt, &mut lane);
        for ack in flushes {
            let _ = ack.send(());
        }
    }
}

/// Sample every slot's plots across the forming bar's run, oldest rung first.
///
/// Returns an empty map when there is no lane, no run, or nothing forming —
/// the three ways a chart says "no curve on the tape", all of which must
/// clear whatever the lane was drawing rather than leave it frozen.
fn walk_lane(
    host: &mut IndicatorHost,
    slots: &BTreeMap<SlotId, SlotMirror>,
    run: &[Trade],
    rungs: usize,
) -> BTreeMap<SlotId, Vec<LaneSample>> {
    let mut lane: BTreeMap<SlotId, Vec<LaneSample>> = BTreeMap::new();
    let prefixes = lane_prefixes(run, rungs.min(MAX_LANE_RUNGS));
    if prefixes.is_empty() {
        return lane;
    }
    for (&slot, mirror) in slots {
        if mirror.host_id.is_some() {
            lane.insert(slot, Vec::with_capacity(prefixes.len()));
        }
    }
    host.walk_partial_prefixes(&prefixes, |prefix, previews| {
        for (&slot, mirror) in slots {
            let Some(host_id) = mirror.host_id else {
                continue;
            };
            // A slot in its error state previews nothing, and nothing is
            // exactly what the lane should show for it.
            let Some(frame) = previews.get(host_id) else {
                continue;
            };
            if let Some(samples) = lane.get_mut(&slot) {
                samples.push(LaneSample {
                    close_time: prefix.close_time,
                    values: frame.values.clone(),
                });
            }
        }
    });
    lane
}

/// Emit exactly the difference between what the UI has and what the host now
/// holds: full snapshots after a rebuild, appended rows otherwise, error
/// transitions once, and the current preview frame for every healthy slot.
fn publish_deltas(
    host: &IndicatorHost,
    slots: &mut BTreeMap<SlotId, SlotMirror>,
    events: &Sender<IndicatorEvent>,
    rebuilt: bool,
    lane: &mut BTreeMap<SlotId, Vec<LaneSample>>,
) {
    for (&slot, mirror) in slots.iter_mut() {
        let Some(host_id) = mirror.host_id else {
            continue;
        };
        let Some(plots) = host.plots(host_id) else {
            continue;
        };
        let descriptor = host
            .descriptor(host_id)
            .expect("instance with plots has a descriptor");
        let rows = plots.len();

        if rebuilt || !mirror.synced {
            let columns: Vec<Vec<f64>> = descriptor
                .plots
                .iter()
                .map(|spec| plots.column(spec.id).to_vec())
                .collect();
            let _ = events.send(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor.clone(),
                columns,
                stale: mirror.stale.clone(),
                inputs: mirror.values.clone(),
            });
            mirror.known_rows = rows;
            mirror.synced = true;
            mirror.error_reported = false;
        } else {
            for row_index in mirror.known_rows..rows {
                let row: Vec<f64> = descriptor
                    .plots
                    .iter()
                    .map(|spec| plots.value(spec.id, row_index))
                    .collect();
                let _ = events.send(IndicatorEvent::Appended { slot, row });
            }
            mirror.known_rows = rows;
        }

        match host.error(host_id) {
            Some(error) if !mirror.error_reported => {
                let _ = events.send(IndicatorEvent::Error {
                    slot,
                    error: error.clone(),
                });
                mirror.error_reported = true;
            }
            Some(_) => {}
            None => mirror.error_reported = false,
        }

        let _ = events.send(IndicatorEvent::Preview {
            slot,
            frame: host.preview(host_id).cloned(),
        });

        // Sent on the same cadence as the preview, empty vector included: the
        // lane's curve is as transient as the forming bar it describes, and a
        // slot that has stopped producing rungs must stop drawing them.
        // Taken, not cloned: each slot is visited once, and the rungs are
        // already a fresh allocation from this batch's walk.
        let _ = events.send(IndicatorEvent::Lane {
            slot,
            samples: lane.remove(&slot).unwrap_or_default(),
        });

        if let Some(revision) = host.objects_revision(host_id)
            && revision != mirror.objects_revision
        {
            let _ = events.send(IndicatorEvent::Objects {
                slot,
                objects: host.objects_snapshot(host_id).unwrap_or_default(),
            });
            mirror.objects_revision = revision;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::IndicatorViews;
    use quantick_engine::{BarBuilder as _, Side, TickBarBuilder, Trade, golden as engine_golden};
    use quantick_indicators::{PlotId, native::Ema};
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
            source: IndicatorSource::NativeEma {
                len: 3,
                source: SourceId::Close,
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
            source: IndicatorSource::NativeCvd,
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
        assert_eq!(view.rows(), fine.len());
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
            (doomed, IndicatorSource::NativeCvd),
            (survivor, IndicatorSource::NativeCvd),
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
        assert_eq!(views.all()[0].rows(), 3, "the survivor saw the new bar");
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
            source: IndicatorSource::NativeCvd,
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
            source: IndicatorSource::NativeCvd,
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
        assert_eq!(view.rows(), bars.len());
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
    use quantick_indicators::{IndicatorHost, PlotId, native::Ema};

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
            source: IndicatorSource::NativeEma {
                len: 3,
                source: SourceId::Close,
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
            "the source cell is bound too — applying `Close` to an instance              that already had it proved nothing"
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
        assert_eq!(view.rows(), bars.len(), "replayed over the full history");
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
            view.rows(),
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
            view.rows(),
            bars.len(),
            "and the healed instance caught up over the existing history"
        );
    }
}
