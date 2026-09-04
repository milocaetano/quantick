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
    ObjectSnapshot, PreviewFrame, Rgba8, native::native,
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
    /// A native, by its catalog id, with the input values it was restored
    /// with — empty meaning "whatever the native declares".
    ///
    /// One variant for every native there will ever be: the worker resolves
    /// the id against [`quantick_indicators::native`] and never learns which
    /// natives exist.
    Native {
        /// Stable catalog id (`native.ema`).
        id: String,
        /// Saved input values, in binding order.
        values: Vec<InputValue>,
    },
    /// A Quantick Pine script: display name + source text (the UI owns
    /// files; the worker only ever sees text).
    Script { name: String, text: String },
}

/// Saved values that were all readable, in the cell form
/// [`quantick_indicators::bind_by_position`] takes. The preset file can yield
/// a `None` — a stored cell that no longer parses, which must still hold its
/// index — while these came from a live indicator and cannot. Widening, not a
/// loss.
fn to_cells(values: &[InputValue]) -> Vec<Option<InputValue>> {
    values.iter().cloned().map(Some).collect()
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
            IndicatorSource::Native { id, values: saved } => {
                // An id this build does not ship is an error slot saying so,
                // never a substitute indicator: silently building a different
                // native than the workspace named is how a trader ends up
                // reading an EMA and believing it is something else.
                let entry = native(id)
                    .ok_or_else(|| format!("`{id}` is not a native indicator this build ships"))?;
                // The panel is generated from `InputSpec` and binding the
                // values back is generated too, via the trait, so a new
                // native's settings cannot be silently ignored for want of a
                // match arm here.
                Ok(entry.build_with(values.unwrap_or(saved)))
            }
            IndicatorSource::Script { name, text } => match quantick_pine::compile(text, name) {
                Ok(compiled) => Ok(Box::new(match values {
                    // An empty set is a slot that has never been given one:
                    // a brand-new indicator, or one whose first compile failed
                    // before any `SetInputs` reached it. Nothing to bind and
                    // nothing to report — and note this is now genuinely rare,
                    // because `SetInputs` keeps the values even when it has no
                    // instance to apply them to.
                    Some(values) if !values.is_empty() => {
                        // Cell by cell, type-checked, through the one binder
                        // both persistence paths use — an edited or upgraded
                        // script keeps every setting whose input is still
                        // there, at its type, and only the rest fall back.
                        // Binding the whole vector on an exact count match
                        // instead would take a stale value whenever a script
                        // changed an input's TYPE without changing how many it
                        // has; refusing the whole vector on a count mismatch
                        // (which this did) meant one added knob reset every
                        // other one — "I reopened the app and my settings were
                        // gone".
                        let bound = quantick_indicators::bind_by_position(
                            &compiled.inputs,
                            &to_cells(values),
                        );
                        // Two reasons to speak, and the count is only one of
                        // them: a cell can be refused at an unchanged count
                        // when an input changed type. The other is louder than
                        // it looks — when the list grew or shrank, EVERY value
                        // after the edit may now sit on a different knob, and
                        // that binds silently whenever the types happen to
                        // line up. A count change is therefore worth a line
                        // even when nothing was refused.
                        let count_changed = values.len() != compiled.inputs.len();
                        if count_changed || bound.kept < values.len() {
                            tracing::warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "INDICATOR_INPUTS_REBOUND",
                                script = %name,
                                saved = values.len(),
                                declared = compiled.inputs.len(),
                                kept = bound.kept,
                                count_changed,
                                action = "bound_by_position",
                                "saved settings were rebound to a changed input list"
                            );
                        }
                        quantick_pine::ScriptIndicator::with_inputs(
                            compiled,
                            text.clone(),
                            bound.values,
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
            IndicatorSource::Native { id, .. } => id.clone(),
            IndicatorSource::Script { name, .. } => format!("script.{name}"),
        }
    }

    /// The display title used when the source cannot load (a healthy
    /// instance's title comes from its descriptor).
    fn fallback_title(&self) -> String {
        match self {
            IndicatorSource::Native { id, values } => native(id).map_or_else(
                // Nothing shipped under this id, so the id itself is the most
                // useful thing the error row can say.
                || id.clone(),
                |entry| entry.build_with(values).descriptor().title.clone(),
            ),
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
        /// The candle paint of each committed bar (`barcolor`), or empty when
        /// this indicator paints nothing — which is every indicator that does
        /// not ask.
        bar_paint: Vec<Option<Rgba8>>,
        /// Committed rows. Carried rather than counted from `columns`: an
        /// indicator whose whole output is candle paint declares no plots, and
        /// there would be no column to count.
        rows: usize,
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
    /// One committed row (one closed bar) for one slot, with the candle paint
    /// that bar asked for (`None`: none).
    Appended {
        slot: SlotId,
        row: Vec<f64>,
        paint: Option<Rgba8>,
    },
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

#[cfg(test)]
impl IndicatorEvent {
    /// A `Rebuilt` carrying only the shape a test cares about: no paint, no
    /// bound inputs, not stale, and the row count taken from the columns.
    ///
    /// The row count is a field rather than a derivation in production
    /// precisely because a paint-only indicator has no column to count — but
    /// every test here declares plots, so deriving it is exact for them and
    /// keeps the fixtures readable.
    pub(crate) fn rebuilt(
        slot: SlotId,
        descriptor: IndicatorDescriptor,
        columns: Vec<Vec<f64>>,
    ) -> Self {
        Self::Rebuilt {
            slot,
            descriptor,
            rows: columns.first().map_or(0, Vec::len),
            columns,
            bar_paint: Vec::new(),
            inputs: Vec::new(),
            stale: None,
        }
    }

    /// An `Appended` row that asks for no candle paint.
    pub(crate) fn appended(slot: SlotId, row: Vec<f64>) -> Self {
        Self::Appended {
            slot,
            row,
            paint: None,
        }
    }
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

/// Latest-wins per slot for `SetInputs`, like the batch loop's own
/// `PartialUpdated` coalescing: a slider drag enqueues one per UI frame, and
/// each one is a full construct-anew + replay. Only the newest of the batch
/// runs — the intermediate drafts were never going to be seen anyway.
/// Skipping an earlier `SetInputs` is safe because applying one is
/// position-independent: it replays the whole retained history no matter
/// where in the batch it sits.
fn latest_set_inputs(batch: &[IndicatorCommand]) -> BTreeMap<SlotId, usize> {
    let mut latest = BTreeMap::new();
    for (index, command) in batch.iter().enumerate() {
        if let IndicatorCommand::SetInputs { slot, .. } = command {
            latest.insert(*slot, index);
        }
    }
    latest
}

/// Keep only the last `SetInputs` per slot in a drained batch.
///
/// The settings dialog previews as you drag, so a slow replay can leave a queue
/// of supersessions behind it — sixty of them for a second on a slider — and
/// every one would construct an indicator anew and replay the whole history to
/// produce a chart the next command overwrites. Dropping them is not a
/// shortcut: a value that a later command in the *same batch* replaces was
/// never on screen, so nothing observable is lost.
///
/// Each survivor keeps the position of its last occurrence, so ordering against
/// `Add` and `Remove` for the same slot is untouched — which is what stops this
/// from resurrecting inputs for a slot the same batch removed.
fn drop_superseded_inputs(batch: &mut Vec<IndicatorCommand>) {
    if batch.len() < 2 {
        return;
    }
    let mut newest: BTreeMap<SlotId, usize> = BTreeMap::new();
    for (index, command) in batch.iter().enumerate() {
        if let IndicatorCommand::SetInputs { slot, .. } = command {
            newest.insert(*slot, index);
        }
    }
    if newest.is_empty() {
        return;
    }
    let mut index = 0;
    batch.retain(|command| {
        let keep = match command {
            IndicatorCommand::SetInputs { slot, .. } => newest[slot] == index,
            _ => true,
        };
        index += 1;
        keep
    });
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

        drop_superseded_inputs(&mut batch);

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

        let last_set_inputs = latest_set_inputs(&batch);

        for (index, command) in batch.into_iter().enumerate() {
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
                            bar_paint: Vec::new(),
                            rows: 0,
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
                    if last_set_inputs.get(&slot) != Some(&index) {
                        continue;
                    }
                    if let Some(mirror) = slots.get_mut(&slot) {
                        let Some(host_id) = mirror.host_id else {
                            // No instance to rebuild: this slot's first
                            // compile failed. The values are still the
                            // trader's, though, and `Reload` builds from this
                            // mirror — so keep them. Dropping them here is
                            // what made repairing a broken script cost every
                            // setting it had: the fixed script loaded at its
                            // declared defaults, the slot's error cleared,
                            // and the next state save wrote those defaults
                            // over the tuned ones on disk.
                            mirror.values = values;
                            continue;
                        };
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
            // Left empty for the indicators that never paint, which is all of
            // them until a script calls `barcolor`: a rebuild of a 100k-bar
            // chart must not carry a column of `None`s per indicator.
            let bar_paint: Vec<Option<Rgba8>> = if plots.paints_any() {
                (0..rows).map(|row| plots.bar_paint(row)).collect()
            } else {
                Vec::new()
            };
            let _ = events.send(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor.clone(),
                columns,
                bar_paint,
                rows,
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
                let _ = events.send(IndicatorEvent::Appended {
                    slot,
                    row,
                    paint: plots.bar_paint(row_index),
                });
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

    /// A script with three inputs, the third of which is imagined to have
    /// been added after the trader saved their settings for the first two.
    fn grown_script() -> IndicatorSource {
        IndicatorSource::Script {
            name: "grown.pine".to_owned(),
            text: concat!(
                "//@version=5\n",
                "indicator(\"Grown\", overlay=true)\n",
                "len = input.int(20, \"len\")\n",
                "factor = input.float(1.5, \"factor\")\n",
                "added = input.bool(false, \"added\")\n",
                "plot(close)\n"
            )
            .to_owned(),
        }
    }

    /// Saved indicator settings are stored positionally, so a script that
    /// gained an input arrives with fewer saved values than declared inputs.
    /// Binding only on an exact count match meant the trader lost the
    /// settings for every input that WAS still there — one added knob, and a
    /// whole tuned indicator came back at its defaults.
    #[test]
    fn a_script_that_gained_an_input_keeps_the_values_saved_for_the_older_ones() {
        let saved = vec![InputValue::Int(50), InputValue::Float(2.5)];
        let built = grown_script()
            .build_with(Some(&saved))
            .expect("the script compiles");
        assert_eq!(
            built.input_values(),
            vec![
                InputValue::Int(50),
                InputValue::Float(2.5),
                InputValue::Bool(false),
            ],
            "the two saved values survive at their own indices and the input added since takes its declared default"
        );
    }

    /// The type check is what keeps the per-cell bind honest: a value that no
    /// longer matches the input at its index is not carried over, it is
    /// dropped for that input's default — and its neighbours are unaffected.
    ///
    /// It applies at every count, including a matching one: there is no
    /// longer a fast path that hands the vector over unchecked, so a script
    /// that changed an input's TYPE without changing how many it has no
    /// longer binds the stale value.
    #[test]
    fn a_saved_value_whose_input_changed_type_falls_back_alone() {
        let saved = vec![
            InputValue::Int(50),
            InputValue::Bool(true), // a float lives at this index now
        ];
        let built = grown_script()
            .build_with(Some(&saved))
            .expect("the script compiles");
        assert_eq!(
            built.input_values(),
            vec![
                InputValue::Int(50),
                InputValue::Float(1.5),
                InputValue::Bool(false),
            ],
            "the mistyped cell falls back alone; the value before it survives"
        );
    }

    /// The count-matched case, which used to skip type checks entirely: a
    /// script that swapped an input's type while keeping the same number of
    /// them bound the stale value straight through.
    #[test]
    fn a_type_change_at_an_unchanged_count_no_longer_binds_the_stale_value() {
        let saved = vec![
            InputValue::Int(50),
            InputValue::Bool(true), // the float's index
            InputValue::Bool(true),
        ];
        let built = grown_script()
            .build_with(Some(&saved))
            .expect("the script compiles");
        assert_eq!(
            built.input_values(),
            vec![
                InputValue::Int(50),
                InputValue::Float(1.5),
                InputValue::Bool(true),
            ],
            "three saved, three declared, and the mistyped one still falls back alone"
        );
    }

    /// A slider drag enqueues one `SetInputs` per UI frame; only the newest
    /// per slot may run, and other slots' requests must survive untouched.
    #[test]
    fn set_inputs_coalesces_latest_wins_per_slot() {
        let a = SlotId(1);
        let b = SlotId(2);
        let set = |slot, value| IndicatorCommand::SetInputs {
            slot,
            values: vec![quantick_indicators::InputValue::Int(value)],
        };
        let batch = vec![
            set(a, 10),
            IndicatorCommand::BarClosed(bars_and_partial(3, 2).0[0].clone()),
            set(b, 20),
            set(a, 11),
            set(a, 12),
        ];
        let latest = latest_set_inputs(&batch);
        assert_eq!(latest.get(&a), Some(&4), "slot A keeps only its newest");
        assert_eq!(latest.get(&b), Some(&2), "slot B is untouched by A's burst");
        assert_eq!(latest.len(), 2);
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
mod batch_tests {
    use super::*;
    use quantick_indicators::InputValue;

    fn set(slot: u64, value: i64) -> IndicatorCommand {
        IndicatorCommand::SetInputs {
            slot: SlotId(slot),
            values: vec![InputValue::Int(value)],
        }
    }

    fn values(command: &IndicatorCommand) -> Option<(u64, i64)> {
        match command {
            IndicatorCommand::SetInputs { slot, values } => match values.first() {
                Some(InputValue::Int(v)) => Some((slot.0, *v)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The cost control under applying edits live: a drag that outruns the
    /// replay leaves a queue of supersessions, and each one would rebuild and
    /// replay the whole history to draw a chart the next command overwrites.
    /// Only the last per slot is worth running, and only per *slot* — two
    /// indicators tuned in the same batch must both survive.
    #[test]
    fn only_the_last_input_change_per_slot_survives_a_batch() {
        let mut batch = vec![set(0, 9), set(1, 3), set(0, 20), set(0, 21)];
        drop_superseded_inputs(&mut batch);
        let kept: Vec<_> = batch.iter().filter_map(values).collect();
        assert_eq!(
            kept,
            vec![(1, 3), (0, 21)],
            "one survivor per slot, each at its last position"
        );
    }

    /// Position is preserved, so coalescing cannot reorder an input change
    /// past the Remove that came after it — which would resurrect a slot the
    /// same batch deleted.
    #[test]
    fn coalescing_never_moves_an_edit_past_a_remove() {
        let mut batch = vec![
            set(0, 9),
            set(0, 21),
            IndicatorCommand::Remove(SlotId(0)),
            set(0, 50),
        ];
        drop_superseded_inputs(&mut batch);
        assert_eq!(batch.len(), 2, "one survivor plus the remove");
        assert!(
            matches!(batch[0], IndicatorCommand::Remove(SlotId(0))),
            "the remove still comes first"
        );
        assert_eq!(values(&batch[1]), Some((0, 50)));

        // Nothing to coalesce is left exactly as it came.
        let mut untouched = vec![IndicatorCommand::Remove(SlotId(0))];
        drop_superseded_inputs(&mut untouched);
        assert_eq!(untouched.len(), 1);
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
