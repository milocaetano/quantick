//! A pane's armed strategies as a reader sees them: plain words and numbers,
//! no kernel type, so a control call can list them without reaching into the
//! strategy crate. The words are the kernel's own badge vocabulary.

use quantick_engine::Side;
use quantick_strategy::anchors::{AnchoredInstance, badge_text};
use quantick_strategy::{ArmedState, Execution, Rearm};
use rust_decimal::Decimal;

use crate::strategies::TabStrategies;

/// One armed instance, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmedReading<D> {
    /// The drawing the instance rides.
    pub drawing: D,
    /// The preset name the badge shows.
    pub preset: String,
    pub side: Side,
    pub quantity: Decimal,
    /// `paper` places orders; `alarm_only` only watches.
    pub execution: &'static str,
    /// `one_shot` or `auto`.
    pub rearm: &'static str,
    /// `armed`, `fired`, `retest_resting`, `in_position`, `done` or `disarmed`.
    pub state: &'static str,
    /// Why a disarmed instance stopped, in the badge's words.
    pub disarm_reason: Option<&'static str>,
    /// The whole badge as the chart prints it.
    pub badge: String,
}

impl<D: Copy> ArmedReading<D> {
    /// `instance`, read.
    #[must_use]
    pub fn of(instance: &AnchoredInstance<D>) -> Self {
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
            badge: badge_text(instance),
        }
    }
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// The instances armed on pane `id`'s drawings, in arming order; none for
    /// a pane that never held one.
    #[must_use]
    pub fn readings(&self, id: u64) -> Vec<ArmedReading<D>> {
        self.anchors(id).map_or_else(Vec::new, |anchors| {
            anchors.instances.iter().map(ArmedReading::of).collect()
        })
    }
}

#[cfg(test)]
#[path = "strategy_reading_tests.rs"]
mod tests;
