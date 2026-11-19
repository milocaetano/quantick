//! A pane's armed strategies as plain words and numbers. The control crates
//! may not depend on `quantick-strategy` (`guards/src/graph.rs`), so a read
//! of them crosses here, where the kernel types are in reach.

use quantick_engine::Side;
use quantick_strategy::anchors::AnchoredInstance;
use quantick_strategy::{ArmedState, BreakPolicy, Execution, Rearm};
use rust_decimal::Decimal;

use crate::strategies::TabStrategies;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmedReading<D> {
    pub drawing: D,
    pub preset: String,
    pub side: Side,
    pub quantity: Decimal,
    /// Take profit at `tp_mult` × the bar's projection; zero or less, none.
    pub tp_mult: Decimal,
    /// Stop at `sl_mult` × the bar's projection; zero or less, none.
    pub sl_mult: Decimal,
    /// `ignore` or `retest_limit`: what a bar cutting through the region does.
    pub on_break: &'static str,
    /// `paper` places orders; `alarm_only` only watches.
    pub execution: &'static str,
    /// `one_shot` or `auto`.
    pub rearm: &'static str,
    /// `armed`, `fired`, `retest_resting`, `in_position`, `done` or `disarmed`.
    pub state: &'static str,
    /// Why a disarmed instance stopped, in the badge's words.
    pub disarm_reason: Option<&'static str>,
    /// The whole badge as the chart prints it beside the drawing — one per
    /// drawing, so every instance riding it reads the same words.
    pub badge: String,
}

impl<D: Copy> ArmedReading<D> {
    #[must_use]
    pub fn of(instance: &AnchoredInstance<D>, badge: String) -> Self {
        let params = instance.armed.params();
        let (state, disarm_reason) = match instance.armed.state() {
            ArmedState::Armed => ("armed", None),
            ArmedState::Fired { retest: false, .. } => ("fired", None),
            ArmedState::Fired { retest: true, .. } => ("retest_resting", None),
            ArmedState::InPosition => ("in_position", None),
            ArmedState::Done => ("done", None),
            ArmedState::Disarmed { reason } => ("disarmed", Some(reason.label())),
        };
        Self {
            drawing: instance.drawing,
            preset: instance.preset.clone(),
            side: params.side,
            quantity: params.quantity,
            tp_mult: params.tp_mult,
            sl_mult: params.sl_mult,
            on_break: match params.on_break {
                BreakPolicy::Ignore => "ignore",
                BreakPolicy::RetestLimit => "retest_limit",
            },
            execution: match params.execution {
                Execution::Paper => "paper",
                Execution::AlarmOnly => "alarm_only",
            },
            rearm: match params.rearm {
                Rearm::OneShot => "one_shot",
                Rearm::Auto => "auto",
            },
            state,
            disarm_reason,
            badge,
        }
    }
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// The instances armed on pane `id`'s drawings, in arming order; none for
    /// a pane that never held one.
    #[must_use]
    pub fn readings(&self, id: u64) -> Vec<ArmedReading<D>> {
        self.anchors(id).map_or_else(Vec::new, |anchors| {
            (anchors.instances.iter())
                .map(|instance| {
                    let stack = anchors.stack(instance.drawing);
                    ArmedReading::of(
                        instance,
                        stack.map(|stack| stack.badge()).unwrap_or_default(),
                    )
                })
                .collect()
        })
    }
}

#[cfg(test)]
#[path = "strategy_reading_tests.rs"]
mod tests;
