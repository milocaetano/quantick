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

use quantick_engine::Bar;
use quantick_indicators::{
    EvalError, Indicator, IndicatorDescriptor, IndicatorHost, InstanceId, PreviewFrame, SourceId,
    native::{Cvd, Ema},
};

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
        match self {
            IndicatorSource::NativeEma { len, source } => Ok(Box::new(Ema::new(*len, *source))),
            IndicatorSource::NativeCvd => Ok(Box::new(Cvd::new())),
            IndicatorSource::Script { name, text } => match quantick_pine::compile(text, name) {
                Ok(compiled) => Ok(Box::new(quantick_pine::ScriptIndicator::new(
                    compiled,
                    text.clone(),
                ))),
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
    PartialUpdated(Option<Bar>),
    /// Spec switch / prepended history / source reset: replay from scratch.
    Rebuild(Vec<Bar>, Option<Bar>),
    /// Instantiate `source` behind `slot` and catch it up over history.
    Add {
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
    },
    /// One committed row (one closed bar) for one slot.
    Appended { slot: SlotId, row: Vec<f64> },
    /// Latest forming-bar frame for one slot (`None`: partial vanished).
    Preview {
        slot: SlotId,
        frame: Option<PreviewFrame>,
    },
    /// The slot's indicator failed and is disabled until rebuilt/replaced.
    Error { slot: SlotId, error: EvalError },
}

/// Worker-side bookkeeping for one slot: the host id plus what the UI is
/// known to have, so every event is an exact delta.
struct SlotMirror {
    /// `None`: the source never loaded (script compile error) — the slot
    /// exists only as a UI entry carrying its error.
    host_id: Option<InstanceId>,
    /// Committed rows the UI has (via Rebuilt/Appended events).
    known_rows: usize,
    /// Whether the UI has been told about the current error state.
    error_reported: bool,
}

/// UI-side handle: send commands, drain events each frame.
pub(crate) struct IndicatorWorker {
    commands: Sender<IndicatorCommand>,
    events: Receiver<IndicatorEvent>,
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
        }
    }

    /// Queue one command. A send failure means the worker died — worth a log
    /// line, never a full queue (the channel is unbounded and command volume
    /// is bounded by feed cadence).
    pub(crate) fn send(&self, command: IndicatorCommand) {
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
                IndicatorCommand::PartialUpdated(partial) => partial_update = Some(partial),
                IndicatorCommand::Rebuild(bars, partial) => {
                    host.rebuild(&bars, partial.as_ref());
                    rebuilt = true;
                    // The rebuild already previewed this partial; a stale
                    // earlier PartialUpdated in the same batch must not
                    // override it backwards.
                    partial_update = None;
                }
                IndicatorCommand::Add { slot, source } => match source.build() {
                    Ok(indicator) => {
                        let host_id = host.add(indicator);
                        slots.insert(
                            slot,
                            SlotMirror {
                                host_id: Some(host_id),
                                known_rows: 0,
                                error_reported: false,
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
                            },
                            columns: Vec::new(),
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
                                known_rows: 0,
                                error_reported: true,
                            },
                        );
                    }
                },
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

        publish_deltas(&host, &mut slots, events, rebuilt);
        for ack in flushes {
            let _ = ack.send(());
        }
    }
}

/// Emit exactly the difference between what the UI has and what the host now
/// holds: full snapshots after a rebuild, appended rows otherwise, error
/// transitions once, and the current preview frame for every healthy slot.
fn publish_deltas(
    host: &IndicatorHost,
    slots: &mut BTreeMap<SlotId, SlotMirror>,
    events: &Sender<IndicatorEvent>,
    rebuilt: bool,
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

        if rebuilt || mirror.known_rows == 0 {
            let columns: Vec<Vec<f64>> = descriptor
                .plots
                .iter()
                .map(|spec| plots.column(spec.id).to_vec())
                .collect();
            let _ = events.send(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor.clone(),
                columns,
            });
            mirror.known_rows = rows;
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
        let slot = views.allocate_slot();
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
        worker.send(IndicatorCommand::PartialUpdated(partial.clone()));
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
        let slot = views.allocate_slot();
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
        let doomed = views.allocate_slot();
        let survivor = views.allocate_slot();
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
        let slot = views.allocate_slot();
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
        let slot = views.allocate_slot();
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
