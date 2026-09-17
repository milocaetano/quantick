use crate::IndicatorSource;
use quantick_engine::{Bar, Trade};
use quantick_indicators::{
    EvalError, IndicatorDescriptor, InputValue, ObjectSnapshot, PreviewFrame, Rgba8,
};

/// One rung of the lane ladder as the UI receives it: the instant on the
/// tape, and what one slot's plots showed there.
///
/// The forming bar only — see [`IndicatorHost::walk_partial_prefixes`] for
/// why the ladder cannot reach back past its open.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneSample {
    /// Exchange timestamp of the last print in this rung's prefix.
    pub close_time: i64,
    /// One value per declared plot, in descriptor order; NaN = nothing to
    /// draw, exactly as in a committed row.
    pub values: Vec<f64>,
}

/// UI-side handle for one indicator slot. The UI allocates these (so it can
/// track an indicator it just requested without waiting for the worker); the
/// worker maps them to the host's own instance ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(pub u64);

/// Commands mirror the host's mutation surface (plan §4.1).
pub enum IndicatorCommand {
    /// Initial history landed: replay it (equivalent to a rebuild).
    Backfilled(Vec<Bar>),
    /// One live bar closed.
    BarClosed(Bar),
    /// Latest forming bar and ordered new trades; only the preview is latest-wins.
    PartialUpdated {
        partial: Option<Bar>,
        /// Unsent forming trades, in occurrence order. A reset/enable seeds
        /// the current run once; later updates append only new prints.
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
}

/// Delta events back to the UI. Bounded cost per event: only [`Rebuilt`]
/// carries bulk data, and only when history really was recomputed.
///
/// [`Rebuilt`]: IndicatorEvent::Rebuilt
pub enum IndicatorEvent {
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

/// The superseding rule for commands parked behind a full queue: `newer`
/// folds into `older`, the last parked command, only where the worker would
/// have produced the same result from the pair. Anything else keeps its
/// place (`Some(newer)`).
///
/// - Forming-bar updates carry the unsent prints of the forming run. When the
///   older one extends the run (a partial and a lane), the newer's prints are
///   appended to it and its partial and budget win — the batch loop's own
///   rule. When the newer one clears the run, it replaces the older outright.
///   An older one that clears followed by a newer one that extends cannot be
///   expressed as one command, so both stay.
/// - Input sets for the same slot: the newer replaces the older, which the
///   batch loop would have skipped during batch preparation.
/// - A replay from scratch (`Backfilled`, `Rebuild`) followed by another: the
///   newer replays everything the older would have.
pub fn fold_parked(
    older: &mut IndicatorCommand,
    newer: IndicatorCommand,
) -> Option<IndicatorCommand> {
    match (&mut *older, newer) {
        (
            IndicatorCommand::PartialUpdated {
                partial,
                run,
                rungs,
            },
            IndicatorCommand::PartialUpdated {
                partial: next_partial,
                run: next_run,
                rungs: next_rungs,
            },
        ) => {
            let newer_clears = next_partial.is_none() || next_rungs == 0;
            let older_extends = partial.is_some() && *rungs > 0;
            if newer_clears {
                *partial = next_partial;
                *run = next_run;
                *rungs = next_rungs;
                None
            } else if older_extends {
                run.extend(next_run);
                *partial = next_partial;
                *rungs = next_rungs;
                None
            } else {
                Some(IndicatorCommand::PartialUpdated {
                    partial: next_partial,
                    run: next_run,
                    rungs: next_rungs,
                })
            }
        }
        (
            IndicatorCommand::SetInputs { slot, values },
            IndicatorCommand::SetInputs {
                slot: next_slot,
                values: next_values,
            },
        ) if *slot == next_slot => {
            *values = next_values;
            None
        }
        (
            IndicatorCommand::Backfilled(_) | IndicatorCommand::Rebuild(..),
            replay @ (IndicatorCommand::Backfilled(_) | IndicatorCommand::Rebuild(..)),
        ) => {
            *older = replay;
            None
        }
        (_, newer) => Some(newer),
    }
}
