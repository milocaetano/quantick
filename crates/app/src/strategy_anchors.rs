//! Armed strategy instances anchored to drawings — the app's half of the
//! `quantick-strategy` kernel.
//!
//! The kernel owns the judgement (trigger, region test, state machine);
//! this module owns the *attachment*: which drawing carries which armed
//! instance, how simulator events fan out to them, and when an instance
//! dies with its drawing. One instance per drawing — arming a drawing that
//! already carries one replaces it, so a rectangle never hides a stack of
//! bots behind one badge.

use quantick_strategy::{ArmedState, ArmedStrategy, DisarmReason};

use crate::drawings::DrawingId;

/// One armed strategy riding one drawing.
pub struct AnchoredInstance {
    pub drawing: DrawingId,
    /// The preset name the badge and tooltip show ("BF compra 1x1").
    pub preset: String,
    pub armed: ArmedStrategy,
}

/// The pane's armed instances. A plain `Vec`: evaluation order is creation
/// order, stable and deterministic.
#[derive(Default)]
pub struct StrategyAnchors {
    pub instances: Vec<AnchoredInstance>,
}

impl StrategyAnchors {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// The instance riding `drawing`, if any.
    #[must_use]
    pub fn for_drawing(&self, drawing: DrawingId) -> Option<&AnchoredInstance> {
        self.instances
            .iter()
            .find(|instance| instance.drawing == drawing)
    }

    #[must_use]
    pub fn for_drawing_mut(&mut self, drawing: DrawingId) -> Option<&mut AnchoredInstance> {
        self.instances
            .iter_mut()
            .find(|instance| instance.drawing == drawing)
    }

    /// Attach `instance`, replacing any instance already on its drawing.
    pub fn arm(&mut self, instance: AnchoredInstance) {
        self.instances
            .retain(|existing| existing.drawing != instance.drawing);
        self.instances.push(instance);
    }

    /// Remove the instance riding `drawing`, whatever its state. The
    /// caller deleted the drawing or dismissed the strategy: a live
    /// operation's position and bracket stay in the simulator, now the
    /// human's to manage — the honest reading of "I deleted the bot".
    pub fn remove_for_drawing(&mut self, drawing: DrawingId) {
        self.instances
            .retain(|instance| instance.drawing != drawing);
    }

    /// Disarm every instance with one shared reason — the timeline reset /
    /// spec change / market switch sweeps.
    pub fn disarm_all(&mut self, reason: DisarmReason) {
        for instance in &mut self.instances {
            instance.armed.disarm(reason);
        }
    }

    /// Drop instances whose drawing no longer exists (deleted from any
    /// surface). Run once per evaluation sweep, never per frame.
    pub fn drop_orphans(&mut self, exists: impl Fn(DrawingId) -> bool) {
        self.instances.retain(|instance| exists(instance.drawing));
    }

    /// How many instances are actively watching (armed or mid-operation) —
    /// what decides whether the paper host buffers print events.
    #[must_use]
    pub fn watching(&self) -> usize {
        self.instances
            .iter()
            .filter(|instance| {
                matches!(
                    instance.armed.state(),
                    ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition
                )
            })
            .count()
    }
}

/// Badge text for one instance state — the on-chart label next to the
/// drawing. Colors are the paint site's business (`theme`); words are
/// decided here so every surface says the same thing.
#[must_use]
pub fn badge_text(instance: &AnchoredInstance) -> String {
    match instance.armed.state() {
        ArmedState::Armed => format!("⚡ {}", instance.preset),
        ArmedState::Fired { .. } => format!("⚡ {} · fired", instance.preset),
        ArmedState::InPosition => format!("⚡ {} · in position", instance.preset),
        ArmedState::Done => format!("⚡ {} · done", instance.preset),
        ArmedState::Disarmed { reason } => {
            format!("⚡ {} · {}", instance.preset, reason.label())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use quantick_strategy::{ForceParams, ForceTrigger, Rearm, StrategyParams};
    use rust_decimal::Decimal;

    fn instance(drawing: DrawingId) -> AnchoredInstance {
        AnchoredInstance {
            drawing,
            preset: "test".to_owned(),
            armed: ArmedStrategy::new(
                StrategyParams {
                    side: Side::Buy,
                    quantity: Decimal::ONE,
                    tp_mult: Decimal::ONE,
                    sl_mult: Decimal::ONE,
                    rearm: Rearm::OneShot,
                },
                Box::new(ForceTrigger::new(ForceParams::default_band())),
            ),
        }
    }

    #[test]
    fn one_instance_per_drawing_and_orphans_die_with_their_drawing() {
        let mut anchors = StrategyAnchors::default();
        anchors.arm(instance(DrawingId(1)));
        anchors.arm(instance(DrawingId(2)));
        anchors.arm(instance(DrawingId(1)));
        assert_eq!(
            anchors.instances.len(),
            2,
            "re-arming replaced, not stacked"
        );

        anchors.drop_orphans(|id| id == DrawingId(2));
        assert_eq!(anchors.instances.len(), 1);
        assert_eq!(anchors.instances[0].drawing, DrawingId(2));

        anchors.remove_for_drawing(DrawingId(2));
        assert!(anchors.is_empty());
    }

    #[test]
    fn disarm_all_names_the_reason_and_watching_counts_live_states() {
        let mut anchors = StrategyAnchors::default();
        anchors.arm(instance(DrawingId(1)));
        anchors.arm(instance(DrawingId(2)));
        assert_eq!(anchors.watching(), 2);

        anchors.disarm_all(DisarmReason::TimelineReset);
        assert_eq!(anchors.watching(), 0);
        for i in &anchors.instances {
            assert_eq!(
                i.armed.state(),
                &ArmedState::Disarmed {
                    reason: DisarmReason::TimelineReset
                }
            );
        }
    }
}
