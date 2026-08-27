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

use crate::region::{BodyCut, Region};
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

/// What the instance does when the trigger bar cuts *through* the region —
/// same side, region active, and a body ([`Region::body_cut`]) that opened
/// on the region's side of the edge the trade leaves by and closed beyond
/// it. Closing inside still fires at market; closing beyond the *opposite*
/// edge, or beyond an edge the body never crossed, never fires at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BreakPolicy {
    /// Hold fire, exactly as before this option existed. The default.
    #[default]
    Ignore,
    /// Rest a limit at the cut edge — the price the tape must revisit to
    /// retest the region — bracketed off the trigger bar like the market
    /// entry would have been, and cancelled if the tape reaches the bar's
    /// projected take profit before returning. With no take-profit leg
    /// there is no target to reach, so the order rests until it fills or
    /// the instance disarms.
    RetestLimit,
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
    /// What a region-cutting trigger bar does. See [`BreakPolicy`].
    pub on_break: BreakPolicy,
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
    /// The retest limit was cancelled by its own cancel-at level: the tape
    /// reached the trigger bar's projected target before returning to the
    /// region edge. The move completed without the retest — no trade
    /// happened, and a one-shot instance stops and says so rather than
    /// pretending an operation ran.
    TargetBeforeRetest,
    /// The retest limit stood down at its own fill moment because a
    /// position was open — a human traded while the order rested, and a
    /// bot never trades against an open manual position. No fill, no
    /// trade; the badge says why the opportunity was declined.
    AccountOccupied,
}

impl DisarmReason {
    /// Whether the disarm named a *rebuilt series* — a timeline reset, a
    /// bar-spec change, a market switch. Re-arming after one resets the
    /// trigger's window (an average blending two tapes judges with a ruler
    /// no chart shows), and the consumer re-warms it from its own series.
    /// One list, shared by the kernel's [`ArmedStrategy::rearm`] and the
    /// chart's re-warm, so a new series-changing reason cannot reset the
    /// ruler while silently skipping the re-warm.
    #[must_use]
    pub fn resets_series(&self) -> bool {
        matches!(
            self,
            Self::TimelineReset | Self::BarSpecChanged | Self::MarketChanged
        )
    }

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
            Self::TargetBeforeRetest => "target hit before retest",
            Self::AccountOccupied => "stood down — account busy",
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
        /// `true` when the entry is a resting retest limit at the region
        /// edge (which may wait a long time and cancel itself at the
        /// target) rather than a market order meeting the next print.
        /// Badges narrate the difference; the machine walks the same path.
        retest: bool,
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
        // Every gate that holds a seen trigger names itself in the note, so
        // the badge never shows a bare "armed" over a force bar that did
        // nothing — the mystery that reads as a broken bot. A bar with no
        // signal clears the note and lets the trigger's own status narrate.
        let Some(signal) = signal else {
            self.note = None;
            return Vec::new();
        };
        if signal.side != self.params.side {
            self.note = Some("trigger held: opposite side");
            return Vec::new();
        }
        if !region_active {
            self.note = Some("trigger held: region not active on this bar");
            return Vec::new();
        }
        if !account_flat {
            self.note = Some("trigger held: account not flat");
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
        // The geometry is the bar's own body — open to close, wicks
        // ignored — because that is what "the force bar cut the region"
        // means to the trader reading the chart. `signal.reference` keeps
        // its separate job: the price the bracket projects from.
        let edge = match region.body_cut(self.params.side, bar.open, bar.close) {
            BodyCut::ClosedInside => {
                self.note = None;
                self.state = ArmedState::Fired {
                    order_id: None,
                    retest: false,
                };
                return vec![Command::PlaceMarket {
                    side: self.params.side,
                    quantity: self.params.quantity,
                    bracket,
                }];
            }
            // The body crossed the edge and finished beyond it. Under the
            // retest policy that edge is where the limit rests.
            BodyCut::CutThrough { edge } => edge,
            BodyCut::NoCut => {
                // The bar closed past the edge but opened past it too: it
                // travelled beyond a region it never crossed into. Resting
                // a limit on that edge would put an order in a band this
                // bar has no claim on.
                self.note = Some("trigger held: the body never cut the region");
                return Vec::new();
            }
            BodyCut::ClosedAway => {
                self.note = Some("trigger held: closed outside region");
                return Vec::new();
            }
        };
        let BreakPolicy::RetestLimit = self.params.on_break else {
            self.note = Some("trigger held: closed outside region");
            return Vec::new();
        };
        // The projected legs anchor on the trigger bar, but the entry now
        // prices at the edge: a leg that does not clear the edge would be
        // dropped (or rejected) by the simulator, and "never fire
        // unprotected" makes that a reason to hold, not to fire bare.
        let legs_clear_edge = match self.params.side {
            Side::Sell => {
                bracket.stop_loss.is_none_or(|level| level > edge)
                    && bracket.take_profit.is_none_or(|level| level < edge)
            }
            Side::Buy => {
                bracket.stop_loss.is_none_or(|level| level < edge)
                    && bracket.take_profit.is_none_or(|level| level > edge)
            }
        };
        if !legs_clear_edge {
            self.note = Some("retest bracket does not clear the edge — held fire");
            return Vec::new();
        }
        self.note = None;
        self.state = ArmedState::Fired {
            order_id: None,
            retest: true,
        };
        vec![Command::PlaceLimit {
            side: self.params.side,
            quantity: self.params.quantity,
            price: edge,
            bracket,
            // The projected target doubles as the order's expiry: the move
            // completing without the retest removes the reason to enter.
            cancel_at: bracket.take_profit,
            // The flat gate held at fire time, but this order can rest for
            // hours: if a human opens a position while it waits, filling
            // would trade against that position — so the simulator stands
            // the order down at its own fill moment instead. "A bot never
            // trades against an open manual position" has to survive the
            // resting window, not just the trigger bar.
            flat_only: true,
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
                (
                    ArmedState::Fired {
                        order_id: None,
                        retest,
                    },
                    SimEvent::Placed(order),
                ) => {
                    self.state = ArmedState::Fired {
                        order_id: Some(order.id),
                        retest: *retest,
                    };
                }
                (ArmedState::Fired { order_id: None, .. }, SimEvent::Rejected(reason)) => {
                    self.state = ArmedState::Disarmed {
                        reason: DisarmReason::EntryRejected(*reason),
                    };
                }
                (
                    ArmedState::Fired {
                        order_id: Some(id), ..
                    },
                    SimEvent::Filled(fill),
                ) if fill.role == quantick_sim::FillRole::Entry(*id) => {
                    self.state = ArmedState::InPosition;
                    my_fill_in_batch = true;
                }
                (
                    ArmedState::Fired {
                        order_id: Some(id), ..
                    },
                    SimEvent::Cancelled { order, reason },
                ) if order.id == *id => {
                    self.state = match reason {
                        // The order's own cancel-at level: the tape reached
                        // the projected target before the retest. No trade
                        // happened — an auto instance goes straight back to
                        // hunting, a one-shot stops with the reason named.
                        quantick_sim::CancelReason::PriceTouched => match self.params.rearm {
                            Rearm::Auto => ArmedState::Armed,
                            Rearm::OneShot => ArmedState::Disarmed {
                                reason: DisarmReason::TargetBeforeRetest,
                            },
                        },
                        // The order stood down because a human's position
                        // occupied the account at its fill moment. An auto
                        // instance returns to hunting (its fire gate will
                        // hold until the account is clean again); one shot
                        // stops with the reason on the badge.
                        quantick_sim::CancelReason::AccountOccupied => match self.params.rearm {
                            Rearm::Auto => ArmedState::Armed,
                            Rearm::OneShot => ArmedState::Disarmed {
                                reason: DisarmReason::AccountOccupied,
                            },
                        },
                        // A human hand (or a reset) swept the entry away;
                        // the bot does not insist.
                        _ => ArmedState::Disarmed {
                            reason: DisarmReason::EntryCancelled,
                        },
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
    ///
    /// A *pending* entry is different — the bot asked for it and no human
    /// ever saw it fill, so the instance sweeps it on the way out: the
    /// returned commands cancel it, and the caller applies them to the
    /// simulator. (Under [`DisarmReason::TimelineReset`] the simulator's
    /// own reset is already cancelling everything with the honest reason,
    /// so nothing is returned — a second cancel would only address an id
    /// that no longer exists.) Before retest limits a pending entry lived
    /// for exactly one print; now it can rest for hours, which is what
    /// makes the sweep worth commanding.
    /// The cancel for this instance's still-pending entry, if one is
    /// resting or queued. One definition of "what the instance owes the
    /// simulator on the way out", shared by [`Self::disarm`] and every
    /// removal path in the consumer — two copies of this match would drift
    /// over the money path.
    #[must_use]
    pub fn pending_entry_cancel(&self) -> Option<Command> {
        match &self.state {
            ArmedState::Fired {
                order_id: Some(id), ..
            } => Some(Command::CancelOrder { id: *id }),
            _ => None,
        }
    }

    #[must_use = "apply the returned cleanup commands to the simulator"]
    pub fn disarm(&mut self, reason: DisarmReason) -> Vec<Command> {
        let cleanup = if matches!(reason, DisarmReason::TimelineReset) {
            Vec::new()
        } else {
            self.pending_entry_cancel().into_iter().collect()
        };
        self.state = ArmedState::Disarmed { reason };
        cleanup
    }

    /// Re-arm from `Done` or `Disarmed`. A live operation (`Fired`,
    /// `InPosition`) cannot be re-armed over; `Armed` is already armed.
    ///
    /// When the disarm reason says the *series itself* changed under the
    /// ruler ([`DisarmReason::resets_series`]) the trigger's running window
    /// is reset too: an average blending bodies from two different tapes
    /// would judge with a ruler no chart shows. The honest cost is a fresh
    /// warmup — which the consumer can pay down with [`Self::warm`] — and
    /// the badge narrates whatever remains.
    pub fn rearm(&mut self) {
        match &self.state {
            ArmedState::Disarmed { reason } if reason.resets_series() => {
                self.trigger.reset();
                self.note = None;
                self.state = ArmedState::Armed;
            }
            ArmedState::Done | ArmedState::Disarmed { .. } => {
                self.note = None;
                self.state = ArmedState::Armed;
            }
            ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition => {}
        }
    }

    /// Feed already-closed bars to the trigger only — the arm-time (and
    /// post-reset re-arm) warmup. The state machine and its gates never
    /// see these bars: they closed before the instance existed, so a force
    /// bar among them must warm the ruler without stamping a "trigger
    /// held" note about a judgement that never happened.
    pub fn warm(&mut self, bars: &[Bar]) {
        for bar in bars {
            let _ = self.trigger.on_closed_bar(bar);
        }
        self.note = None;
    }

    /// One line for the on-chart badge.
    #[must_use]
    pub fn status_line(&self) -> String {
        match &self.state {
            ArmedState::Armed => match self.note {
                Some(note) => format!("armed · {note}"),
                None => format!("armed · {}", self.trigger.status()),
            },
            ArmedState::Fired { retest: false, .. } => "fired · waiting for fill".to_owned(),
            // Only promise the self-cancel when the order actually carries
            // one: without a take-profit leg there is no target level.
            ArmedState::Fired { retest: true, .. } => {
                if self.params.tp_mult > Decimal::ZERO {
                    "retest limit resting at the edge · cancels at target".to_owned()
                } else {
                    "retest limit resting at the edge · until filled or disarmed".to_owned()
                }
            }
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
            on_break: BreakPolicy::Ignore,
        }
    }

    fn force_instance(side: Side) -> ArmedStrategy {
        ArmedStrategy::new(
            params(side),
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_body: Decimal::ZERO,
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
        assert_eq!(
            instance.state(),
            &ArmedState::Fired {
                order_id: None,
                retest: false
            }
        );
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
        let _ = instance.disarm(DisarmReason::User);
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
            cancel_at: None,
            flat_only: false,
            placed_ms: 0,
        };
        let _ = instance.on_sim_events(&[SimEvent::Placed(order.clone())]);
        assert_eq!(
            instance.state(),
            &ArmedState::Fired {
                order_id: Some(OrderId(7)),
                retest: false
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
                min_body: Decimal::ZERO,
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
            cancel_at: None,
            flat_only: false,
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
            cancel_at: None,
            flat_only: false,
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
                min_body: Decimal::ZERO,
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
            cancel_at: None,
            flat_only: false,
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
            cancel_at: None,
            flat_only: false,
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
        let _ = instance.disarm(DisarmReason::BarSpecChanged);
        instance.rearm();
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(instance.trigger().status(), "waiting for bars 0/3");

        // Same series, one-shot done: the window survives and the very
        // next force bar fires without a fresh warmup.
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        let _ = instance.disarm(DisarmReason::User);
        instance.rearm();
        let commands = instance.on_closed_bar(&bar("102", "107.6"), &region, true, true);
        assert_eq!(
            commands.len(),
            1,
            "a user disarm keeps the warm window: {commands:?}"
        );
    }

    // ---- the break/retest policy ----

    fn retest_instance(side: Side) -> ArmedStrategy {
        ArmedStrategy::new(
            StrategyParams {
                on_break: BreakPolicy::RetestLimit,
                ..params(side)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_body: Decimal::ZERO,
            })),
        )
    }

    /// Warm the window, then close a body-4 sell force bar at 104, cutting
    /// below the 105–115 region. Range 6 (high 109, low 103): the bracket
    /// anchored on the bar is SL 110 / TP 98.
    fn warm_then_sell_cut(instance: &mut ArmedStrategy, region: &Region) -> Vec<Command> {
        assert!(
            instance
                .on_closed_bar(&bar("110", "109"), region, true, true)
                .is_empty()
        );
        assert!(
            instance
                .on_closed_bar(&bar("109", "108"), region, true, true)
                .is_empty()
        );
        instance.on_closed_bar(&bar("108", "104"), region, true, true)
    }

    #[test]
    fn a_sell_cut_below_the_region_rests_a_retest_limit_at_the_cut_edge() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = retest_instance(Side::Sell);
        let commands = warm_then_sell_cut(&mut instance, &region);
        assert_eq!(
            commands,
            vec![Command::PlaceLimit {
                side: Side::Sell,
                quantity: Decimal::ONE,
                price: dec("105"),
                bracket: Bracket {
                    stop_loss: Some(dec("110")),
                    take_profit: Some(dec("98")),
                },
                cancel_at: Some(dec("98")),
                flat_only: true,
            }],
            "the limit rests at the cut edge, bracketed off the trigger bar, \
             expiring at the bar's own target"
        );
        assert_eq!(
            instance.state(),
            &ArmedState::Fired {
                order_id: None,
                retest: true
            }
        );
        assert!(instance.status_line().starts_with("retest limit resting"));
    }

    #[test]
    fn a_buy_cut_above_the_region_mirrors_the_retest() {
        let region = Region::new(dec("90"), dec("100"));
        let mut instance = retest_instance(Side::Buy);
        instance.on_closed_bar(&bar("95", "96"), &region, true, true);
        instance.on_closed_bar(&bar("96", "97"), &region, true, true);
        let commands = instance.on_closed_bar(&bar("97", "101"), &region, true, true);
        // Close 101, range 6 (high 102, low 96): TP 107, SL 95, edge 100.
        assert_eq!(
            commands,
            vec![Command::PlaceLimit {
                side: Side::Buy,
                quantity: Decimal::ONE,
                price: dec("100"),
                bracket: Bracket {
                    stop_loss: Some(dec("95")),
                    take_profit: Some(dec("107")),
                },
                cancel_at: Some(dec("107")),
                flat_only: true,
            }]
        );
    }

    /// Default off: the cut holds fire exactly as before the option
    /// existed, and the badge names the gate instead of staying mute.
    #[test]
    fn the_default_policy_ignores_the_cut_and_names_the_gate() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = ArmedStrategy::new(
            params(Side::Sell),
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_body: Decimal::ZERO,
            })),
        );
        assert!(warm_then_sell_cut(&mut instance, &region).is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: closed outside region"
        );
    }

    /// A close beyond the *opposite* edge is a miss, never a retest: a sell
    /// bar closing above the region has not cut anything downward.
    #[test]
    fn a_cut_beyond_the_opposite_edge_never_arms_a_retest() {
        let region = Region::new(dec("90"), dec("100"));
        let mut instance = retest_instance(Side::Sell);
        instance.on_closed_bar(&bar("110", "109"), &region, true, true);
        instance.on_closed_bar(&bar("109", "108"), &region, true, true);
        // A body-4 sell force bar closing at 104 — above the whole region.
        let commands = instance.on_closed_bar(&bar("108", "104"), &region, true, true);
        assert!(commands.is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);
    }

    /// The reported bug. A sell force bar whose whole body sits below the
    /// region — it opened under the low and closed lower still — never cut
    /// anything: the region is above it, untouched. Resting a limit on the
    /// low there puts an order in a band the bar never crossed, which is
    /// exactly what the trader saw on the chart.
    #[test]
    fn a_body_wholly_below_the_region_is_no_cut_and_rests_nothing() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = retest_instance(Side::Sell);
        instance.on_closed_bar(&bar("102", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "100"), &region, true, true);
        // Body 4, ratio 2 — a genuine force bar, opening at 100 (below the
        // 105 edge) and closing at 96. It cut nothing.
        let commands = instance.on_closed_bar(&bar("100", "96"), &region, true, true);
        assert!(
            commands.is_empty(),
            "a body that never crossed the edge rests no limit: {commands:?}"
        );
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: the body never cut the region"
        );
    }

    /// The buy mirror: a body wholly above a buy region cut nothing either.
    #[test]
    fn a_body_wholly_above_the_region_is_no_cut_and_rests_nothing() {
        let region = Region::new(dec("90"), dec("100"));
        let mut instance = retest_instance(Side::Buy);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        instance.on_closed_bar(&bar("102", "103"), &region, true, true);
        let commands = instance.on_closed_bar(&bar("103", "107"), &region, true, true);
        assert!(
            commands.is_empty(),
            "a body that never crossed the edge rests no limit: {commands:?}"
        );
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: the body never cut the region"
        );
    }

    /// The open is read for the *cut*, never for the market entry: a bar
    /// that opened clear above a sell region and closed inside it sells at
    /// market, because where it opened does not matter once it finished in
    /// the band.
    #[test]
    fn an_open_outside_the_region_never_blocks_a_close_inside_it() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = retest_instance(Side::Sell);
        instance.on_closed_bar(&bar("120", "119"), &region, true, true);
        instance.on_closed_bar(&bar("119", "118"), &region, true, true);
        // Body 4, ratio 2: opens at 112 above the 110 edge, closes at 108
        // inside the band.
        let commands = instance.on_closed_bar(&bar("112", "108"), &region, true, true);
        assert!(
            matches!(commands.as_slice(), [Command::PlaceMarket { .. }]),
            "a close inside fires at market whatever the open did: {commands:?}"
        );
    }

    /// The option only changes what a *cut* does: a close inside the region
    /// still fires the market entry.
    #[test]
    fn inside_the_region_still_fires_at_market_with_retest_on() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = retest_instance(Side::Buy);
        let commands = warm_then_force(&mut instance, &region);
        assert!(
            matches!(commands.as_slice(), [Command::PlaceMarket { .. }]),
            "a close inside the region is the market path: {commands:?}"
        );
    }

    fn retest_order(id: u64) -> quantick_sim::Order {
        quantick_sim::Order {
            id: OrderId(id),
            side: Side::Sell,
            kind: quantick_sim::EntryKind::Limit,
            price: Some(dec("105")),
            quantity: Decimal::ONE,
            bracket: Bracket {
                stop_loss: Some(dec("110")),
                take_profit: Some(dec("98")),
            },
            cancel_at: Some(dec("98")),
            flat_only: true,
            placed_ms: 0,
        }
    }

    /// The tape reaching the target first cancels the order by price: no
    /// trade happened, so one-shot stops with the reason named and auto
    /// goes straight back to hunting.
    #[test]
    fn the_target_cancel_walks_by_rearm_policy() {
        let region = Region::new(dec("105"), dec("115"));

        let mut one_shot = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut one_shot, &region);
        let order = retest_order(4);
        let _ = one_shot.on_sim_events(&[SimEvent::Placed(order.clone())]);
        let _ = one_shot.on_sim_events(&[SimEvent::Cancelled {
            order,
            reason: quantick_sim::CancelReason::PriceTouched,
        }]);
        assert_eq!(
            one_shot.state(),
            &ArmedState::Disarmed {
                reason: DisarmReason::TargetBeforeRetest
            }
        );
        assert_eq!(one_shot.status_line(), "target hit before retest");

        let mut auto = ArmedStrategy::new(
            StrategyParams {
                rearm: Rearm::Auto,
                on_break: BreakPolicy::RetestLimit,
                ..params(Side::Sell)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_body: Decimal::ZERO,
            })),
        );
        warm_then_sell_cut(&mut auto, &region);
        let order = retest_order(5);
        let _ = auto.on_sim_events(&[SimEvent::Placed(order.clone())]);
        let _ = auto.on_sim_events(&[SimEvent::Cancelled {
            order,
            reason: quantick_sim::CancelReason::PriceTouched,
        }]);
        assert_eq!(
            auto.state(),
            &ArmedState::Armed,
            "auto re-arms and keeps hunting after the target kills the order"
        );
    }

    /// The stand-down (the simulator declined the fill because a human's
    /// position occupied the account) walks like the target cancel: named
    /// stop for one-shot, straight back to hunting for auto.
    #[test]
    fn the_stand_down_walks_by_rearm_policy() {
        let region = Region::new(dec("105"), dec("115"));

        let mut one_shot = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut one_shot, &region);
        let order = retest_order(14);
        let _ = one_shot.on_sim_events(&[SimEvent::Placed(order.clone())]);
        let _ = one_shot.on_sim_events(&[SimEvent::Cancelled {
            order,
            reason: quantick_sim::CancelReason::AccountOccupied,
        }]);
        assert_eq!(
            one_shot.state(),
            &ArmedState::Disarmed {
                reason: DisarmReason::AccountOccupied
            }
        );
        assert_eq!(one_shot.status_line(), "stood down — account busy");

        let mut auto = ArmedStrategy::new(
            StrategyParams {
                rearm: Rearm::Auto,
                on_break: BreakPolicy::RetestLimit,
                ..params(Side::Sell)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_body: Decimal::ZERO,
            })),
        );
        warm_then_sell_cut(&mut auto, &region);
        let order = retest_order(15);
        let _ = auto.on_sim_events(&[SimEvent::Placed(order.clone())]);
        let _ = auto.on_sim_events(&[SimEvent::Cancelled {
            order,
            reason: quantick_sim::CancelReason::AccountOccupied,
        }]);
        assert_eq!(auto.state(), &ArmedState::Armed);
    }

    /// A human sweep (flatten / cancel-all) of the resting retest limit is
    /// still the old story: the bot does not insist.
    #[test]
    fn a_swept_retest_limit_disarms_as_entry_cancelled() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut instance, &region);
        let order = retest_order(6);
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
    fn the_retest_fill_walks_to_in_position() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut instance, &region);
        let _ = instance.on_sim_events(&[SimEvent::Placed(retest_order(7))]);
        let fill = quantick_sim::Fill {
            timestamp_ms: 9,
            agg_id: 30,
            side: Side::Sell,
            price: dec("105"),
            quantity: Decimal::ONE,
            role: quantick_sim::FillRole::Entry(OrderId(7)),
        };
        let _ = instance.on_sim_events(&[SimEvent::Filled(fill)]);
        assert_eq!(instance.state(), &ArmedState::InPosition);
    }

    /// Disarming over a resting entry sweeps it: the returned commands
    /// cancel the order the bot placed — except under a timeline reset,
    /// where the simulator's own reset already cancels everything with the
    /// honest reason.
    #[test]
    fn disarm_sweeps_the_pending_retest_limit() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut instance, &region);
        let _ = instance.on_sim_events(&[SimEvent::Placed(retest_order(8))]);
        let commands = instance.disarm(DisarmReason::User);
        assert_eq!(commands, vec![Command::CancelOrder { id: OrderId(8) }]);

        let mut instance = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut instance, &region);
        let _ = instance.on_sim_events(&[SimEvent::Placed(retest_order(9))]);
        let commands = instance.disarm(DisarmReason::TimelineReset);
        assert!(
            commands.is_empty(),
            "the simulator's reset owns that cancellation"
        );
    }

    /// A projected leg that does not clear the entry edge would be dropped
    /// at fill time — so it holds fire instead, and says why.
    #[test]
    fn a_bracket_leg_that_does_not_clear_the_edge_holds_fire() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = ArmedStrategy::new(
            StrategyParams {
                sl_mult: dec("0.1"),
                on_break: BreakPolicy::RetestLimit,
                ..params(Side::Sell)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_body: Decimal::ZERO,
            })),
        );
        // SL = 104 + 0.1 × 6 = 104.6, below the 105 edge a short entered
        // there needs it above.
        assert!(warm_then_sell_cut(&mut instance, &region).is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(
            instance.status_line(),
            "armed · retest bracket does not clear the edge — held fire"
        );
    }

    /// Every gate that holds a seen trigger names itself, and the note
    /// clears on the next signal-less bar so the ruler narrates again.
    #[test]
    fn held_triggers_name_their_gate_on_the_badge() {
        let region = Region::new(dec("100"), dec("110"));

        // Opposite side: a buy force bar for a sell instance.
        let mut instance = force_instance(Side::Sell);
        warm_then_force(&mut instance, &region);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: opposite side"
        );

        // Region not active on this bar.
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        instance.on_closed_bar(&bar("102", "106"), &region, false, true);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: region not active on this bar"
        );

        // Account not flat.
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        instance.on_closed_bar(&bar("102", "106"), &region, true, false);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: account not flat"
        );

        // The next quiet bar clears the note: the ruler speaks again.
        instance.on_closed_bar(&bar("106", "107"), &region, true, true);
        assert!(
            instance.status_line().starts_with("armed · quiet"),
            "a signal-less bar hands the badge back to the ruler: {}",
            instance.status_line()
        );
    }

    /// The force ruler declares its own warmup depth, so a consumer can
    /// re-warm it from the series after a reset instead of hardcoding 20.
    #[test]
    fn the_force_trigger_declares_its_warmup_depth() {
        let instance = force_instance(Side::Buy);
        assert_eq!(instance.trigger().warmup_bars(), 3);
    }
}
