use crate::{
    IndicatorCommand, IndicatorSession, LaneDiagnostics, LaneSample, SessionEffects, SlotId,
};
use quantick_engine::Bar;
use std::collections::BTreeMap;
/// Survivors retain original envelope indexes, including intervening markers.
#[derive(Default)]
struct InputSelection {
    latest: BTreeMap<SlotId, usize>,
    superseded: usize,
}
impl InputSelection {
    fn new<'c>(commands: impl Iterator<Item = (usize, &'c IndicatorCommand)>) -> Self {
        let mut selection = Self::default();
        for (index, command) in commands {
            if let IndicatorCommand::SetInputs { slot, .. } = command {
                selection.superseded +=
                    usize::from(selection.latest.insert(*slot, index).is_some());
            }
        }
        selection
    }
    fn keeps(&self, index: usize, command: &IndicatorCommand) -> bool {
        match command {
            IndicatorCommand::SetInputs { slot, .. } => self.latest.get(slot) == Some(&index),
            _ => true,
        }
    }
}
/// Batch-local reduction; the caller consumes its original envelope in order.
pub struct Applying<'s> {
    session: &'s mut IndicatorSession,
    selection: InputSelection,
    partial: Option<Option<Bar>>,
    lane_request: Option<usize>,
    rebuilt: bool,
}
/// Publication after final partial and ladder evaluation.
pub struct Publishing<'s> {
    session: &'s mut IndicatorSession,
    rebuilt: bool,
    lane: BTreeMap<SlotId, Vec<LaneSample>>,
}
impl IndicatorSession {
    /// Scan borrowed commands without retaining references or cloning payloads.
    /// Supply exactly the domain commands that will be consumed by `apply`,
    /// each at its unique original envelope index. Keep intervening transport
    /// markers in that envelope; they do not split input supersession groups.
    pub fn begin_batch<'s, 'c>(
        &'s mut self,
        commands: impl Iterator<Item = (usize, &'c IndicatorCommand)>,
    ) -> Applying<'s> {
        Applying {
            session: self,
            selection: InputSelection::new(commands),
            partial: None,
            lane_request: None,
            rebuilt: false,
        }
    }
}
impl<'s> Applying<'s> {
    pub fn inputs_superseded(&self) -> usize {
        self.selection.superseded
    }
    pub fn lane_diagnostics(&self) -> LaneDiagnostics {
        self.session.lane_diagnostics()
    }
    /// Update the caller's earned scalar before a later effect may unwind.
    /// Consume the prepared domain commands once, in original envelope order,
    /// using the same indexes passed to `begin_batch`. An unprepared late input
    /// edit is not a new batch and has no selected survivor position.
    pub fn apply(
        &mut self,
        index: usize,
        command: IndicatorCommand,
        partials_superseded: &mut usize,
        effects: &mut impl SessionEffects,
    ) {
        if !self.selection.keeps(index, &command) {
            return;
        }
        match command {
            IndicatorCommand::Backfilled(bars) => {
                self.session.host.rebuild(&bars, None);
                self.rebuilt = true;
                self.cut();
            }
            IndicatorCommand::BarClosed(bar) => {
                self.session.host.push_closed_bar(&bar);
                self.cut();
            }
            IndicatorCommand::PartialUpdated {
                partial,
                run,
                rungs,
            } => {
                if partial.is_none() || rungs == 0 {
                    self.session.cut_lane();
                } else {
                    self.session.lane_run.extend(run);
                }
                *partials_superseded += usize::from(self.partial.is_some());
                self.partial = Some(partial);
                self.lane_request = Some(rungs);
            }
            IndicatorCommand::Rebuild(bars, partial) => {
                self.session.host.rebuild(&bars, partial.as_ref());
                self.rebuilt = true;
                self.cut();
            }
            IndicatorCommand::Add { slot, source } => self.session.add(slot, source, effects),
            IndicatorCommand::SetInputs { slot, values } => {
                self.session.set_inputs(slot, values, effects)
            }
            IndicatorCommand::Reload { slot, source } => self.session.reload(slot, source, effects),
            IndicatorCommand::Remove(slot) => self.session.remove(slot),
        }
    }
    fn cut(&mut self) {
        self.session.cut_lane();
        self.partial = None;
        self.lane_request = None;
    }
    /// Complete evaluation before the caller announces its Publishing phase.
    pub fn finish_applying(self) -> Publishing<'s> {
        if let Some(partial) = self.partial {
            self.session.host.set_partial(partial.as_ref());
        }
        let lane = self
            .lane_request
            .map(|rungs| {
                crate::lane::walk_lane(
                    &mut self.session.host,
                    &self.session.slots,
                    &self.session.lane_run,
                    rungs,
                )
            })
            .unwrap_or_default();
        Publishing {
            session: self.session,
            rebuilt: self.rebuilt,
            lane,
        }
    }
}
impl Publishing<'_> {
    /// Emit one delta at a time; no output collection or cancellation on failure.
    pub fn publish(mut self, effects: &mut impl SessionEffects) {
        crate::publication::publish_deltas(
            &self.session.host,
            &mut self.session.slots,
            effects,
            self.rebuilt,
            &mut self.lane,
        );
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    fn drop_superseded_inputs(batch: &mut Vec<IndicatorCommand>) {
        let selection = InputSelection::new(batch.iter().enumerate());
        let mut index = 0;
        batch.retain(|command| {
            let keep = selection.keeps(index, command);
            index += 1;
            keep
        });
    }
    use quantick_engine::{BarBuilder as _, Side, TickBarBuilder, Trade, golden as engine_golden};
    use quantick_indicators::InputValue;
    use rust_decimal::Decimal;
    fn trade(i: u64) -> Trade {
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
        let batch = [
            set(a, 10),
            IndicatorCommand::BarClosed(bars_and_partial(3, 2).0[0].clone()),
            set(b, 20),
            set(a, 11),
            set(a, 12),
        ];
        let latest = InputSelection::new(batch.iter().enumerate()).latest;
        assert_eq!(latest.get(&a), Some(&4), "slot A keeps only its newest");
        assert_eq!(latest.get(&b), Some(&2), "slot B is untouched by A's burst");
        assert_eq!(latest.len(), 2);
    }

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
