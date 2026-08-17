//! The armed instance: one strategy attached to one region, at most one
//! live operation at a time.
//!
//! The state machine is the safety story. An instance fires only when its
//! trigger, its region, its side and a flat account all agree on the same
//! closed bar; after firing it follows its one operation through fill and
//! closure before it may fire again — and by default ([`Rearm::OneShot`])
//! it does not fire again at all until a human re-arms it. Every way an
//! instance stops watching the market is a named [`DisarmReason`] shown on
//! the chart, never a silent halt.

use quantick_engine::{Bar, Side};
use quantick_sim::{Bracket, Command, OrderId, RejectReason, SimEvent};
use rust_decimal::Decimal;

use crate::region::Region;
use crate::trigger::Trigger;

/// What the instance does after its operation closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rearm {
    /// One shot per arming: the operation closes and the instance is done
    /// until a human re-arms it. The default, and the over-fire guard.
    OneShot,
    /// Re-arm automatically: while the drawing lives and the region is
    /// active, every qualifying bar on a flat account may fire.
    Auto,
}

/// The preset half of an instance: everything a strategy bank row stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyParams {
    /// The side this instance hunts. A sell instance ignores buy-side force
    /// bars entirely — the trigger is side-agnostic, the *instance* is not.
    pub side: Side,
    pub quantity: Decimal,
    /// Take profit = reference + `tp_mult` × projection, in the trade's
    /// favour. Zero or negative means "no take profit leg".
    pub tp_mult: Decimal,
    /// Stop loss = reference − `sl_mult` × projection, against the trade.
    /// Zero or negative means "no stop leg".
    pub sl_mult: Decimal,
    pub rearm: Rearm,
}

/// Why an instance stopped watching the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisarmReason {
    /// A human disarmed it from the drawing's menu.
    User,
    /// A replay seek or session reset rebuilt the past; judgements made on
    /// the old series do not carry.
    TimelineReset,
    /// The bar spec changed, and with it what the body average measures.
    BarSpecChanged,
    /// The tab moved to another market; the region belongs to the old one.
    MarketChanged,
    /// The simulator refused the entry; the reason is the curriculum.
    EntryRejected(RejectReason),
    /// The pending entry was swept away (a manual flatten or cancel-all)
    /// before it filled. A human overruled the bot; the bot does not insist.
    EntryCancelled,
    /// A promised protective leg was dropped at fill time (the market
    /// outran the level between the trigger close and the fill). The
    /// instance closed the position at the next print — the exit the leg
    /// would have taken, executed late — and stopped, because an operation
    /// that lost its protection mid-flight is not the operation that was
    /// armed.
    ProtectionDropped,
}

impl DisarmReason {
    /// Short badge label; the full story goes in tooltips.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::User => "disarmed",
            Self::TimelineReset => "timeline reset",
            Self::BarSpecChanged => "bar spec changed",
            Self::MarketChanged => "market changed",
            Self::EntryRejected(_) => "entry rejected",
            Self::EntryCancelled => "entry cancelled",
            Self::ProtectionDropped => "protection dropped — closed",
        }
    }
}

/// Where an instance stands. See the crate docs for the full walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmedState {
    /// Watching every closed bar for the trigger + region + flat gate.
    Armed,
    /// Entry command emitted; waiting for the tape to answer. `order_id`
    /// is filled in when the simulator acknowledges placement.
    Fired {
        order_id: Option<OrderId>,
    },
    /// The entry filled; the operation is live until the account is flat
    /// again (its bracket, a manual close — the instance does not care
    /// which hand closes it).
    InPosition,
    /// A one-shot instance completed its operation.
    Done,
    Disarmed {
        reason: DisarmReason,
    },
}

/// One strategy armed on one region.
pub struct ArmedStrategy {
    params: StrategyParams,
    trigger: Box<dyn Trigger>,
    state: ArmedState,
    /// One-line note about the last non-fire (an invalid projection), for
    /// status honesty.
    note: Option<&'static str>,
}

impl std::fmt::Debug for ArmedStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArmedStrategy")
            .field("params", &self.params)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl ArmedStrategy {
    /// A freshly armed instance.
    #[must_use]
    pub fn new(params: StrategyParams, trigger: Box<dyn Trigger>) -> Self {
        Self {
            params,
            trigger,
            state: ArmedState::Armed,
            note: None,
        }
    }

    #[must_use]
    pub fn params(&self) -> &StrategyParams {
        &self.params
    }

    #[must_use]
    pub fn state(&self) -> &ArmedState {
        &self.state
    }

    #[must_use]
    pub fn trigger(&self) -> &dyn Trigger {
        self.trigger.as_ref()
    }

    /// Feed one closed bar. `region_active` is the caller's answer to "does
    /// the drawing still cover this bar in time?"; `account_flat` must
    /// reflect the whole account, not just this instance's operation — a
    /// bot never trades against an open manual position.
    ///
    /// The trigger is fed **unconditionally** so its running averages stay
    /// warm across disarmed stretches; the gates only decide whether a
    /// signal becomes a command.
    pub fn on_closed_bar(
        &mut self,
        bar: &Bar,
        region: &Region,
        region_active: bool,
        account_flat: bool,
    ) -> Vec<Command> {
        let signal = self.trigger.on_closed_bar(bar);

        // A live operation ends the moment the account is flat again: its
        // bracket fired, or a human hand closed it — the instance does not
        // care which. Completing before judging the signal lets an Auto
        // instance treat this very bar as a fresh opportunity.
        if self.state == ArmedState::InPosition && account_flat {
            self.state = match self.params.rearm {
                Rearm::OneShot => ArmedState::Done,
                Rearm::Auto => ArmedState::Armed,
            };
        }

        if self.state != ArmedState::Armed {
            return Vec::new();
        }
        let Some(signal) = signal else {
            return Vec::new();
        };
        if signal.side != self.params.side
            || !region_active
            || !region.contains(signal.reference)
            || !account_flat
        {
            return Vec::new();
        }
        let Some(bracket) = project_bracket(
            self.params.side,
            signal.reference,
            signal.projection,
            self.params.tp_mult,
            self.params.sl_mult,
        ) else {
            // A promised protective leg that cannot be priced is a reason
            // not to fire, never a reason to fire unprotected.
            self.note = Some("projection invalid — held fire");
            return Vec::new();
        };
        self.note = None;
        self.state = ArmedState::Fired { order_id: None };
        vec![Command::PlaceMarket {
            side: self.params.side,
            quantity: self.params.quantity,
            bracket,
        }]
    }

    /// Feed simulator events back in, and apply whatever the instance
    /// answers with. Call with the batch returned by applying this
    /// instance's own commands (placement acknowledgements and rejections
    /// are attributed by arrival there), and with every batch the simulator
    /// emits — prints *and* manual commands: a flatten from the human's
    /// hand sweeps the bot's pending entry through the same stream.
    ///
    /// The returned commands are the instance protecting itself: today only
    /// `ClosePosition` when the simulator dropped a promised protective leg
    /// at fill time (`BracketDropped` in the same batch as this instance's
    /// entry fill — netting and the flat gate make that attribution exact).
    /// The market outran the level between trigger close and fill, so the
    /// exit the leg would have taken is executed at the next print, late but
    /// honest, and the instance disarms as [`DisarmReason::ProtectionDropped`].
    /// Applying a returned `ClosePosition` emits no events of its own, so
    /// feeding its (empty) apply-result back is a no-op by construction.
    #[must_use]
    pub fn on_sim_events(&mut self, events: &[SimEvent]) -> Vec<Command> {
        // Whether this batch contains this instance's own entry fill: the
        // window in which a BracketDropped can only be about our bracket.
        let mut my_fill_in_batch = false;
        let mut commands = Vec::new();
        for event in events {
            match (&self.state, event) {
                (ArmedState::Fired { order_id: None }, SimEvent::Placed(order)) => {
                    self.state = ArmedState::Fired {
                        order_id: Some(order.id),
                    };
                }
                (ArmedState::Fired { order_id: None }, SimEvent::Rejected(reason)) => {
                    self.state = ArmedState::Disarmed {
                        reason: DisarmReason::EntryRejected(*reason),
                    };
                }
                (ArmedState::Fired { order_id: Some(id) }, SimEvent::Filled(fill))
                    if fill.role == quantick_sim::FillRole::Entry(*id) =>
                {
                    self.state = ArmedState::InPosition;
                    my_fill_in_batch = true;
                }
                (ArmedState::Fired { order_id: Some(id) }, SimEvent::Cancelled { order, .. })
                    if order.id == *id =>
                {
                    self.state = ArmedState::Disarmed {
                        reason: DisarmReason::EntryCancelled,
                    };
                }
                (ArmedState::InPosition, SimEvent::BracketDropped { .. }) if my_fill_in_batch => {
                    // "Never fire unprotected" extends past the fill: an
                    // operation whose promised leg could not be priced is
                    // closed, not ridden bare.
                    self.state = ArmedState::Disarmed {
                        reason: DisarmReason::ProtectionDropped,
                    };
                    commands.push(Command::ClosePosition);
                }
                _ => {}
            }
        }
        commands
    }

    /// Stop watching, with the reason the badge will show. An in-position
    /// instance hands its operation to the human: the position and its
    /// bracket live on in the simulator untouched.
    pub fn disarm(&mut self, reason: DisarmReason) {
        self.state = ArmedState::Disarmed { reason };
    }

    /// Re-arm from `Done` or `Disarmed`. A live operation (`Fired`,
    /// `InPosition`) cannot be re-armed over; `Armed` is already armed.
    ///
    /// When the disarm reason says the *series itself* changed under the
    /// ruler — a rebuilt timeline, another bar spec, another market — the
    /// trigger's running window is reset too: an average blending bodies
    /// from two different tapes would judge with a ruler no chart shows.
    /// The honest cost is a fresh warmup, and the badge narrates it.
    pub fn rearm(&mut self) {
        match &self.state {
            ArmedState::Disarmed {
                reason:
                    DisarmReason::TimelineReset
                    | DisarmReason::BarSpecChanged
                    | DisarmReason::MarketChanged,
            } => {
                self.trigger.reset();
                self.state = ArmedState::Armed;
            }
            ArmedState::Done | ArmedState::Disarmed { .. } => {
                self.state = ArmedState::Armed;
            }
            ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition => {}
        }
    }

    /// One line for the on-chart badge.
    #[must_use]
    pub fn status_line(&self) -> String {
        match &self.state {
            ArmedState::Armed => match self.note {
                Some(note) => format!("armed · {note}"),
                None => format!("armed · {}", self.trigger.status()),
            },
            ArmedState::Fired { .. } => "fired · waiting for fill".to_owned(),
            ArmedState::InPosition => "in position".to_owned(),
            ArmedState::Done => "done · one shot".to_owned(),
            ArmedState::Disarmed { reason } => reason.label().to_owned(),
        }
    }
}

/// Project the protective bracket off the trigger bar's measurement. A
/// non-positive multiplier means "no leg on that side"; a promised leg that
/// prices at or below zero (or a zero projection) is a refusal, not a
/// silent drop.
fn project_bracket(
    side: Side,
    reference: Decimal,
    projection: Decimal,
    tp_mult: Decimal,
    sl_mult: Decimal,
) -> Option<Bracket> {
    let wants_tp = tp_mult > Decimal::ZERO;
    let wants_sl = sl_mult > Decimal::ZERO;
    if (wants_tp || wants_sl) && projection <= Decimal::ZERO {
        return None;
    }
    let towards = |mult: Decimal| match side {
        Side::Buy => reference + mult * projection,
        Side::Sell => reference - mult * projection,
    };
    let against = |mult: Decimal| match side {
        Side::Buy => reference - mult * projection,
        Side::Sell => reference + mult * projection,
    };
    let take_profit = wants_tp.then(|| towards(tp_mult));
    let stop_loss = wants_sl.then(|| against(sl_mult));
    if let Some(price) = take_profit
        && price <= Decimal::ZERO
    {
        return None;
    }
    if let Some(price) = stop_loss
        && price <= Decimal::ZERO
    {
        return None;
    }
    Some(Bracket {
        stop_loss,
        take_profit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::force::ForceParams;
    use crate::trigger::{ForceTrigger, Signal};
    use core::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn bar(open: &str, close: &str) -> Bar {
        let open = dec(open);
        let close = dec(close);
        Bar {
            open_time: 0,
            close_time: 0,
            open,
            high: open.max(close) + Decimal::ONE,
            low: open.min(close) - Decimal::ONE,
            close,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    fn params(side: Side) -> StrategyParams {
        StrategyParams {
            side,
            quantity: Decimal::ONE,
            tp_mult: Decimal::ONE,
            sl_mult: Decimal::ONE,
            rearm: Rearm::OneShot,
        }
    }

    fn force_instance(side: Side) -> ArmedStrategy {
        ArmedStrategy::new(
            params(side),
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
            })),
        )
    }

    /// Warm the 3-bar window with two body-1 bars, then close a body-4 buy
    /// force bar (ratio 2) at `close`.
    fn warm_then_force(instance: &mut ArmedStrategy, region: &Region) -> Vec<Command> {
        assert!(
            instance
                .on_closed_bar(&bar("100", "101"), region, true, true)
                .is_empty()
        );
        assert!(
            instance
                .on_closed_bar(&bar("101", "102"), region, true, true)
                .is_empty()
        );
        instance.on_closed_bar(&bar("102", "106"), region, true, true)
    }

    #[test]
    fn a_force_bar_inside_the_region_fires_a_bracketed_market_order() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        let commands = warm_then_force(&mut instance, &region);
        // Close 106, range 6 (high 107, low 101): TP 112, SL 100.
        assert_eq!(
            commands,
            vec![Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: Bracket {
                    stop_loss: Some(dec("100")),
                    take_profit: Some(dec("112")),
                },
            }]
        );
        assert_eq!(instance.state(), &ArmedState::Fired { order_id: None });
    }

    #[test]
    fn a_sell_instance_projects_the_mirror_bracket() {
        let region = Region::new(dec("90"), dec("110"));
        let mut instance = force_instance(Side::Sell);
        instance.on_closed_bar(&bar("100", "99"), &region, true, true);
        instance.on_closed_bar(&bar("99", "98"), &region, true, true);
        let commands = instance.on_closed_bar(&bar("98", "94"), &region, true, true);
        // Close 94, range 6 (high 99, low 93): TP 88, SL 100.
        assert_eq!(
            commands,
            vec![Command::PlaceMarket {
                side: Side::Sell,
                quantity: Decimal::ONE,
                bracket: Bracket {
                    stop_loss: Some(dec("100")),
                    take_profit: Some(dec("88")),
                },
            }]
        );
    }

    #[test]
    fn every_gate_holds_fire_on_its_own() {
        // Wrong side: a buy force bar does nothing for a sell instance.
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Sell);
        assert!(warm_then_force(&mut instance, &region).is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);

        // Close outside the region.
        let low_region = Region::new(dec("90"), dec("100"));
        let mut instance = force_instance(Side::Buy);
        assert!(warm_then_force(&mut instance, &low_region).is_empty());

        // Region no longer active in time.
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        assert!(
            instance
                .on_closed_bar(&bar("102", "106"), &region, false, true)
                .is_empty()
        );

        // Account not flat.
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        assert!(
            instance
                .on_closed_bar(&bar("102", "106"), &region, true, false)
                .is_empty()
        );

        // Disarmed instances keep their window warm but never fire.
        let mut instance = force_instance(Side::Buy);
        instance.disarm(DisarmReason::User);
        assert!(warm_then_force(&mut instance, &region).is_empty());
        instance.rearm();
        // The window is already warm: the next force bar fires immediately.
        let commands = instance.on_closed_bar(&bar("102", "107.5"), &region, true, true);
        assert_eq!(
            commands.len(),
            1,
            "re-armed instance fires on a warm window"
        );
    }

    #[test]
    fn the_operation_walks_fired_filled_flat_and_one_shot_ends_done() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);

        let order = quantick_sim::Order {
            id: OrderId(7),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            placed_ms: 0,
        };
        let _ = instance.on_sim_events(&[SimEvent::Placed(order.clone())]);
        assert_eq!(
            instance.state(),
            &ArmedState::Fired {
                order_id: Some(OrderId(7))
            }
        );

        // A fill for some *other* order must not advance the machine.
        let foreign_fill = quantick_sim::Fill {
            timestamp_ms: 1,
            agg_id: 10,
            side: Side::Buy,
            price: dec("106"),
            quantity: Decimal::ONE,
            role: quantick_sim::FillRole::Entry(OrderId(99)),
        };
        let _ = instance.on_sim_events(&[SimEvent::Filled(foreign_fill)]);
        assert!(matches!(instance.state(), ArmedState::Fired { .. }));

        let fill = quantick_sim::Fill {
            role: quantick_sim::FillRole::Entry(OrderId(7)),
            ..foreign_fill
        };
        let _ = instance.on_sim_events(&[SimEvent::Filled(fill)]);
        assert_eq!(instance.state(), &ArmedState::InPosition);

        // Account flat again on the next closed bar: one shot → done, and
        // even a fresh force bar in region does not fire.
        let commands = instance.on_closed_bar(&bar("106", "111"), &region, true, true);
        assert!(commands.is_empty());
        assert_eq!(instance.state(), &ArmedState::Done);
    }

    #[test]
    fn auto_rearm_goes_straight_back_to_armed_and_may_refire() {
        let region = Region::new(dec("100"), dec("120"));
        let mut instance = ArmedStrategy::new(
            StrategyParams {
                rearm: Rearm::Auto,
                ..params(Side::Buy)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
            })),
        );
        warm_then_force(&mut instance, &region);
        let _ = instance.on_sim_events(&[SimEvent::Placed(quantick_sim::Order {
            id: OrderId(1),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            placed_ms: 0,
        })]);
        let _ = instance.on_sim_events(&[SimEvent::Filled(quantick_sim::Fill {
            timestamp_ms: 1,
            agg_id: 10,
            side: Side::Buy,
            price: dec("106"),
            quantity: Decimal::ONE,
            role: quantick_sim::FillRole::Entry(OrderId(1)),
        })]);
        assert_eq!(instance.state(), &ArmedState::InPosition);

        // Flat again, and the very completion bar is itself a force bar in
        // region: bodies 1, 4 then 8 over window 3 → avg 13/3, ratio 24/13
        // ≈ 1.85 → force. The instance re-arms and fires on the same bar.
        let commands = instance.on_closed_bar(&bar("106", "114"), &region, true, true);
        assert_eq!(commands.len(), 1, "auto re-arm fires on the completion bar");
    }

    #[test]
    fn a_rejected_entry_disarms_with_the_simulators_reason() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        let _ = instance.on_sim_events(&[SimEvent::Rejected(RejectReason::NoMarketPrice)]);
        assert_eq!(
            instance.state(),
            &ArmedState::Disarmed {
                reason: DisarmReason::EntryRejected(RejectReason::NoMarketPrice)
            }
        );
    }

    #[test]
    fn a_swept_pending_entry_disarms_instead_of_waiting_forever() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        let order = quantick_sim::Order {
            id: OrderId(3),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            placed_ms: 0,
        };
        let _ = instance.on_sim_events(&[SimEvent::Placed(order.clone())]);
        let _ = instance.on_sim_events(&[SimEvent::Cancelled {
            order,
            reason: quantick_sim::CancelReason::Flatten,
        }]);
        assert_eq!(
            instance.state(),
            &ArmedState::Disarmed {
                reason: DisarmReason::EntryCancelled
            }
        );
    }

    #[test]
    fn rearm_never_interrupts_a_live_operation() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        instance.rearm();
        assert!(
            matches!(instance.state(), ArmedState::Fired { .. }),
            "rearm over a fired instance must be a no-op"
        );
    }

    #[test]
    fn zero_multipliers_mean_no_leg_and_never_block_the_fire() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = ArmedStrategy::new(
            StrategyParams {
                tp_mult: Decimal::ZERO,
                sl_mult: Decimal::ZERO,
                ..params(Side::Buy)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
            })),
        );
        let commands = warm_then_force(&mut instance, &region);
        assert_eq!(
            commands,
            vec![Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: Bracket::none(),
            }]
        );
    }

    /// The trigger port accepts a second implementation — the docking test.
    #[test]
    fn a_fake_trigger_docks_and_drives_the_same_machine() {
        struct EveryBar;
        impl Trigger for EveryBar {
            fn on_closed_bar(&mut self, bar: &Bar) -> Option<Signal> {
                Some(Signal {
                    side: Side::Buy,
                    reference: bar.close,
                    projection: Decimal::ONE,
                })
            }
            fn status(&self) -> String {
                "always".to_owned()
            }
        }
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = ArmedStrategy::new(params(Side::Buy), Box::new(EveryBar));
        let commands = instance.on_closed_bar(&bar("100", "105"), &region, true, true);
        assert_eq!(
            commands,
            vec![Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: Bracket {
                    stop_loss: Some(dec("104")),
                    take_profit: Some(dec("106")),
                },
            }]
        );
        assert!(instance.status_line().starts_with("fired"));
    }

    /// "Never fire unprotected" extends past the fill: a bracket the
    /// simulator dropped at fill time closes the operation at the next
    /// print and disarms by name.
    #[test]
    fn a_dropped_bracket_closes_the_position_and_disarms_by_name() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        let order = quantick_sim::Order {
            id: OrderId(7),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            placed_ms: 0,
        };
        let _ = instance.on_sim_events(&[SimEvent::Placed(order)]);

        // The fill and the drop arrive in one batch, exactly as the
        // simulator reports them when the market outran the level.
        let fill = quantick_sim::Fill {
            timestamp_ms: 1,
            agg_id: 10,
            side: Side::Buy,
            price: dec("102.9"),
            quantity: Decimal::ONE,
            role: quantick_sim::FillRole::Entry(OrderId(7)),
        };
        let commands = instance.on_sim_events(&[
            SimEvent::Filled(fill),
            SimEvent::BracketDropped {
                reason: RejectReason::StopLossOnWrongSide(Side::Buy),
            },
        ]);
        assert_eq!(commands, vec![Command::ClosePosition]);
        assert_eq!(
            instance.state(),
            &ArmedState::Disarmed {
                reason: DisarmReason::ProtectionDropped
            }
        );

        // A drop in some *later* batch (a manual SetBracket the human
        // fumbled) is not this instance's to act on: no fill of ours
        // arrived with it.
        let mut bystander = force_instance(Side::Buy);
        warm_then_force(&mut bystander, &region);
        let _ = bystander.on_sim_events(&[SimEvent::Placed(quantick_sim::Order {
            id: OrderId(9),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            placed_ms: 0,
        })]);
        let commands = bystander.on_sim_events(&[SimEvent::BracketDropped {
            reason: RejectReason::TakeProfitOnWrongSide(Side::Buy),
        }]);
        assert!(commands.is_empty());
        assert!(matches!(bystander.state(), ArmedState::Fired { .. }));
    }

    /// Re-arming after the *series itself* changed resets the ruler — an
    /// average blending two tapes judges with a ruler no chart shows — but
    /// re-arming after a completed shot keeps the warm window.
    #[test]
    fn rearm_resets_the_ruler_only_when_the_series_changed_under_it() {
        let region = Region::new(dec("100"), dec("110"));

        // Series changed: the window restarts and the badge says warmup.
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        instance.disarm(DisarmReason::BarSpecChanged);
        instance.rearm();
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(instance.trigger().status(), "waiting for bars 0/3");

        // Same series, one-shot done: the window survives and the very
        // next force bar fires without a fresh warmup.
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        instance.disarm(DisarmReason::User);
        instance.rearm();
        let commands = instance.on_closed_bar(&bar("102", "107.6"), &region, true, true);
        assert_eq!(
            commands.len(),
            1,
            "a user disarm keeps the warm window: {commands:?}"
        );
    }
}
