//! Armed strategy instances anchored to drawings — the app's half of the
//! `quantick-strategy` kernel.
//!
//! The kernel owns the judgement (trigger, region test, state machine);
//! this module owns the *attachment*: which drawing carries which armed
//! instance, how simulator events fan out to them, and when an instance
//! dies with its drawing. One instance per drawing — arming a drawing that
//! already carries one replaces it, so a rectangle never hides a stack of
//! bots behind one badge.

use quantick_sim::Command;
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
    /// human's to manage — the honest reading of "I deleted the bot". A
    /// *pending* entry is different — nothing filled, nobody owns it — so
    /// the returned commands sweep it; apply them to the simulator.
    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn remove_for_drawing(&mut self, drawing: DrawingId) -> Vec<Command> {
        let mut cleanup = Vec::new();
        self.instances.retain(|instance| {
            if instance.drawing != drawing {
                return true;
            }
            cleanup.extend(pending_entry_cleanup(&instance.armed));
            false
        });
        cleanup
    }

    /// Disarm every instance with one shared reason — the timeline reset /
    /// spec change / market switch sweeps. The returned commands sweep any
    /// pending entries (the kernel returns none under a timeline reset,
    /// where the simulator's own reset already cancels them by name).
    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn disarm_all(&mut self, reason: DisarmReason) -> Vec<Command> {
        let mut cleanup = Vec::new();
        for instance in &mut self.instances {
            cleanup.extend(instance.armed.disarm(reason));
        }
        cleanup
    }

    /// Drop instances whose drawing no longer exists (deleted from any
    /// surface). Run once per evaluation sweep, never per frame. Sweeps
    /// pending entries like [`Self::remove_for_drawing`].
    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn drop_orphans(&mut self, exists: impl Fn(DrawingId) -> bool) -> Vec<Command> {
        let mut cleanup = Vec::new();
        self.instances.retain(|instance| {
            if exists(instance.drawing) {
                return true;
            }
            cleanup.extend(pending_entry_cleanup(&instance.armed));
            false
        });
        cleanup
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

/// The cleanup an instance leaving the anchors owes the simulator: its
/// pending entry, if one is still resting or queued. Removal is not a
/// [`DisarmReason`], so the kernel's disarm sweep cannot cover this path.
fn pending_entry_cleanup(armed: &ArmedStrategy) -> Option<Command> {
    match armed.state() {
        ArmedState::Fired {
            order_id: Some(id), ..
        } => Some(Command::CancelOrder { id: *id }),
        _ => None,
    }
}

/// Badge text for one instance state — the on-chart label next to the
/// drawing. Colors are the paint site's business (`theme`); words are
/// decided here so every surface says the same thing.
#[must_use]
pub fn badge_text(instance: &AnchoredInstance) -> String {
    match instance.armed.state() {
        ArmedState::Armed => format!("⚡ {}", instance.preset),
        ArmedState::Fired { retest: false, .. } => format!("⚡ {} · fired", instance.preset),
        ArmedState::Fired { retest: true, .. } => {
            format!("⚡ {} · retest resting", instance.preset)
        }
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
                    on_break: quantick_strategy::BreakPolicy::Ignore,
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

        let cleanup = anchors.drop_orphans(|id| id == DrawingId(2));
        assert_eq!(anchors.instances.len(), 1);
        assert_eq!(anchors.instances[0].drawing, DrawingId(2));
        assert!(cleanup.is_empty(), "armed instances have nothing pending");

        let cleanup = anchors.remove_for_drawing(DrawingId(2));
        assert!(anchors.is_empty());
        assert!(cleanup.is_empty());
    }

    /// An instance whose entry is still pending leaves a cancel behind when
    /// its drawing (and with it the badge) goes away — a resting bot order
    /// with no bot is the one thing removal must not orphan.
    #[test]
    fn removal_sweeps_the_pending_entry() {
        use quantick_sim::{Bracket, Command, EntryKind, Order, OrderId, SimEvent};

        fn bar(open: i64, close: i64) -> quantick_engine::Bar {
            quantick_engine::Bar {
                open_time: 0,
                close_time: 0,
                open: Decimal::from(open),
                high: Decimal::from(open.max(close)) + Decimal::ONE,
                low: Decimal::from(open.min(close)) - Decimal::ONE,
                close: Decimal::from(close),
                buy_volume: Decimal::ONE,
                sell_volume: Decimal::ONE,
                trade_count: 2,
            }
        }

        // A 3-bar ruler that fires on the third bar (body 4 over average 2).
        let mut riding = AnchoredInstance {
            drawing: DrawingId(3),
            preset: "test".to_owned(),
            armed: ArmedStrategy::new(
                StrategyParams {
                    side: Side::Buy,
                    quantity: Decimal::ONE,
                    tp_mult: Decimal::ONE,
                    sl_mult: Decimal::ONE,
                    rearm: Rearm::OneShot,
                    on_break: quantick_strategy::BreakPolicy::Ignore,
                },
                Box::new(ForceTrigger::new(ForceParams {
                    window: 3,
                    min_factor: "1.5".parse().expect("fixture"),
                    max_factor: "2.5".parse().expect("fixture"),
                    min_body: Decimal::ZERO,
                })),
            ),
        };
        let region = quantick_strategy::Region::new(Decimal::from(100), Decimal::from(110));
        let _ = riding
            .armed
            .on_closed_bar(&bar(100, 101), &region, true, true);
        let _ = riding
            .armed
            .on_closed_bar(&bar(101, 102), &region, true, true);
        let commands = riding
            .armed
            .on_closed_bar(&bar(102, 106), &region, true, true);
        assert_eq!(commands.len(), 1, "the fixture fires: {commands:?}");
        let _ = riding.armed.on_sim_events(&[SimEvent::Placed(Order {
            id: OrderId(11),
            side: Side::Buy,
            kind: EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            cancel_at: None,
            placed_ms: 0,
        })]);

        let mut anchors = StrategyAnchors::default();
        anchors.arm(riding);
        let cleanup = anchors.remove_for_drawing(DrawingId(3));
        assert_eq!(
            cleanup,
            vec![Command::CancelOrder { id: OrderId(11) }],
            "removing the bot sweeps its pending entry"
        );
        assert!(anchors.is_empty());
    }

    #[test]
    fn disarm_all_names_the_reason_and_watching_counts_live_states() {
        let mut anchors = StrategyAnchors::default();
        anchors.arm(instance(DrawingId(1)));
        anchors.arm(instance(DrawingId(2)));
        assert_eq!(anchors.watching(), 2);

        let cleanup = anchors.disarm_all(DisarmReason::TimelineReset);
        assert!(
            cleanup.is_empty(),
            "the simulator's reset owns those cancellations"
        );
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
