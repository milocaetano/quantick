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
use quantick_sim::{Bracket, Command, OrderId, RejectReason, VenueEvent};
use rust_decimal::Decimal;

use crate::region::{BodyCut, Region};
use crate::trigger::{Signal, Trigger};

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
    /// Whether this instance trades or only watches. See [`Execution`].
    pub execution: Execution,
}

/// What an instance does when its setup happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Execution {
    /// Place the order, exactly as before this option existed. The default.
    #[default]
    Paper,
    /// Watch and judge, and place nothing — ever. The instance still runs
    /// its ruler, still tests the region, and still reports the
    /// opportunity, so a [signal alarm](crate::SignalAlarm) riding it
    /// speaks on every setup; it simply never emits a command.
    ///
    /// This is for the trader whose hands are on another platform. The
    /// simulated position such a trader does not intend to take is not
    /// harmless bookkeeping: it occupies the account, and the flat gate
    /// would then hold the *next* opportunity — silencing exactly what
    /// they armed the instance to be told about.
    AlarmOnly,
}

/// Where a qualifying bar puts the entry — the shape of an opportunity,
/// with no account and no order in it yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opportunity {
    /// The body closed inside the region: a market entry.
    Market,
    /// The body cut through the region and finished beyond `edge`, which
    /// the retest limit rests on.
    Retest { edge: Decimal },
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

/// Does this signal belong to this instance at all — right side, live
/// region? The first half of the opportunity test.
///
/// Split from [`entry_geometry`] rather than fused with it because the
/// order path has to consult the account *between* the two halves: a busy
/// account holds an order that the geometry would otherwise have placed.
/// The alarm runs the two halves back to back, and both surfaces read the
/// same two functions — there is no second copy of either to drift.
fn signal_is_ours(
    params: &StrategyParams,
    region_active: bool,
    signal: &Signal,
) -> Result<(), &'static str> {
    if signal.side != params.side {
        return Err("opposite side");
    }
    if !region_active {
        return Err("region not active on this bar");
    }
    Ok(())
}

/// Where this bar's body puts the entry, or the named reason there is no
/// entry to put. The second half of the opportunity test.
///
/// The geometry is the bar's own body — open to close, wicks ignored —
/// because that is what "the force bar cut the region" means to the trader
/// reading the chart. A signal's `reference` keeps its separate job: the
/// price the bracket projects from.
///
/// Each arm is complete on its own, policy included, so that a fifth
/// [`BodyCut`] or a third [`BreakPolicy`] cannot fall through into a
/// neighbour's outcome and rest a real order by accident. Every refusal
/// names the gate that actually decided it — the geometry when the bar
/// could never have traded, the policy when the option was simply off.
fn entry_geometry(
    params: &StrategyParams,
    region: &Region,
    bar: &Bar,
) -> Result<Opportunity, &'static str> {
    match region.body_cut(params.side, bar) {
        BodyCut::ClosedInside => Ok(Opportunity::Market),
        // The body crossed the edge and finished beyond it — the one
        // geometry the retest option exists for. With the option off, the
        // policy is what held fire, and the badge says so rather than
        // blaming a bar that did its part.
        BodyCut::CutThrough { edge } => match params.on_break {
            BreakPolicy::RetestLimit => Ok(Opportunity::Retest { edge }),
            BreakPolicy::Ignore => Err("cut the region, retest option off"),
        },
        // The bar closed past the edge but opened past it too: it travelled
        // beyond a region it never crossed into. Resting a limit on that
        // edge would put an order in a band this bar has no claim on. True
        // whatever the policy says, so the geometry is the honest reason.
        BodyCut::NoCut => Err("the body never cut the region"),
        BodyCut::ClosedAway => Err("closed outside region"),
    }
}

/// What the last judged bar left standing on an instance.
///
/// Two shapes, because they are two different sentences. A [`Note::Held`] is
/// a gate refusing a signal — the badge frames it as "trigger held" and, once
/// the bar has passed, keeps it as *the last thing that was refused*. A
/// [`Note::Aside`] is a remark about the instance itself, true for as long as
/// it is armed and not worth remembering across bars.
///
/// The distinction earns its keep because a single quiet bar used to erase
/// the reason the last setup was declined, and the trader reads the badge
/// *after* the move, never during it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Note {
    Held(&'static str),
    Aside(&'static str),
}

impl Note {
    /// The line as the badge shows it on the bar it happened.
    fn line(self) -> String {
        match self {
            Self::Held(reason) => format!("trigger held: {reason}"),
            Self::Aside(remark) => remark.to_owned(),
        }
    }
}

/// A gate's refusal, and whether it is about the bar that just closed.
///
/// `fresh` is the difference between "this is why nothing happened just now"
/// and "this is the last thing that was refused". Both are worth showing;
/// only one of them is a statement about the present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldReason {
    pub reason: &'static str,
    pub fresh: bool,
}

/// One strategy armed on one region.
pub struct ArmedStrategy {
    params: StrategyParams,
    trigger: Box<dyn Trigger>,
    state: ArmedState,
    /// What the bar just judged left standing, cleared by the next bar the
    /// trigger had nothing to say about.
    note: Option<Note>,
    /// The most recent gate that refused a signal, kept **across** the quiet
    /// bars that follow it.
    ///
    /// `note` alone was the whole story, and it was erased by the first bar
    /// carrying no signal — so the badge forgot why the setup the trader was
    /// watching had been declined, usually within seconds of them looking
    /// away. A refusal is cleared by something happening, not by nothing
    /// happening: an order going out, a re-arm, a re-warm.
    last_hold: Option<&'static str>,
    /// Whether the bar most recently fed to [`ArmedStrategy::on_closed_bar`]
    /// presented this instance's setup — judged with the account and the
    /// state machine deliberately left out. The alarm's closed-bar reading;
    /// the order path never consults it.
    last_close_opportunity: Option<Opportunity>,
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
            last_hold: None,
            last_close_opportunity: None,
        }
    }

    #[must_use]
    pub fn params(&self) -> &StrategyParams {
        &self.params
    }

    /// Whether the last closed bar presented this instance's setup.
    ///
    /// This is the *opportunity*, not the trade: the account, the state
    /// machine and [`Execution::AlarmOnly`] have no say in it. A trader
    /// reading it is asking "did my setup just happen?", which stays true
    /// whether or not this instance was in a position to act — and that is
    /// precisely the question a [signal alarm](crate::SignalAlarm) sounds
    /// for someone who will act somewhere else.
    #[must_use]
    pub fn last_close_opportunity(&self) -> Option<Opportunity> {
        self.last_close_opportunity
    }

    /// The same question, asked of the bar still **forming**.
    ///
    /// Read-only by design, all the way down: the ruler is weighed rather
    /// than advanced ([`Trigger::preview`]), no state is touched, and no
    /// command can result. The answer is provisional — the bar keeps
    /// moving, and it may stop qualifying before it closes — so a consumer
    /// showing it owes the trader the word "preview".
    ///
    /// It reads the same two gate functions the order path does, so the
    /// alarm cannot come to answer this question differently from the
    /// strategy that will trade it.
    #[must_use]
    pub fn preview_opportunity(
        &self,
        bar: &Bar,
        region: &Region,
        region_active: bool,
    ) -> Option<Opportunity> {
        let signal = self.trigger.preview(bar)?;
        signal_is_ours(&self.params, region_active, &signal).ok()?;
        entry_geometry(&self.params, region, bar).ok()
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
    /// the drawing still cover this bar in time?".
    ///
    /// `account_flat` must reflect the whole account, not just this
    /// instance's operation — a bot never trades against an open manual
    /// position, and it decides two things: whether a signal becomes an
    /// order, and whether a live operation has finished.
    ///
    /// It reads as a cross-region coupling, and one region's parked order
    /// muting another region's setup is a real complaint. It cannot be
    /// lifted here: `quantick-sim` models **one** netted position carrying
    /// **one** exit ladder, and a bracketed second entry on the same side
    /// replaces that ladder wholesale (the old legs are cancelled as
    /// `BracketReplaced`) — the first instance keeps a badge reading "in
    /// position" over a position whose protection it no longer owns. Giving each region its own account is
    /// a simulator change, and until it lands this flag is what keeps a
    /// stop attached to the operation that projected it.
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
        // The *opportunity* is judged for every closed bar, whatever the
        // state machine is doing and whoever holds the account: it is a
        // statement about the setup, not about this instance's ability to
        // act on it. The alarm reads it from here, which is how a trader
        // executing on another platform still hears a signal their spent
        // one-shot instance would never trade.
        self.last_close_opportunity = signal.as_ref().and_then(|signal| {
            signal_is_ours(&self.params, region_active, signal).ok()?;
            entry_geometry(&self.params, region, bar).ok()
        });

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
        // signal clears the note and lets the trigger's own status narrate;
        // `last_hold` is what carries the refusal past it.
        let Some(signal) = signal else {
            self.note = None;
            return Vec::new();
        };
        if let Err(reason) = signal_is_ours(&self.params, region_active, &signal) {
            self.hold(reason);
            return Vec::new();
        }
        // The geometry runs before the account and before the projection: a
        // bar whose body never came near the region could not have traded
        // under any multiplier or any account state, and blaming an
        // unpriceable leg — or a busy account — there sends the trader
        // tuning something that was never the reason. The badge names the
        // gate that actually decided.
        let opportunity = match entry_geometry(&self.params, region, bar) {
            Ok(opportunity) => opportunity,
            Err(reason) => {
                self.hold(reason);
                return Vec::new();
            }
        };
        // An instance that places no orders stops here, having judged the
        // bar exactly as a trading one would — geometry included, so its
        // badge still names what held a silent alarm. It is not disarmed
        // and not spent: it keeps watching, which is the whole reason a
        // trader arms one.
        if self.params.execution == Execution::AlarmOnly {
            // Something *was* judged and it qualified, so the standing
            // refusal is spent: a badge reading "last held: opposite side"
            // over a bar that just rang the alarm is the misinformation
            // `last_hold` exists to end.
            self.last_hold = None;
            self.note = Some(Note::Aside("alarm only — no order placed"));
            return Vec::new();
        }
        if !account_flat {
            self.hold("account not flat");
            return Vec::new();
        }
        let edge = match opportunity {
            Opportunity::Market => {
                let Some(bracket) = self.project(&signal) else {
                    return Vec::new();
                };
                self.clear_notes();
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
            Opportunity::Retest { edge } => edge,
        };
        let Some(bracket) = self.project(&signal) else {
            return Vec::new();
        };
        // The projected legs anchor on the trigger bar, but the entry now
        // prices at the edge: a leg that does not clear the edge would be
        // dropped (or rejected) by the simulator, and "never fire
        // unprotected" makes that a reason to hold, not to fire bare.
        let legs_clear_edge = match self.params.side {
            Side::Sell => {
                bracket.stop_loss().is_none_or(|level| level > edge)
                    && bracket.take_profit().is_none_or(|level| level < edge)
            }
            Side::Buy => {
                bracket.stop_loss().is_none_or(|level| level < edge)
                    && bracket.take_profit().is_none_or(|level| level > edge)
            }
        };
        if !legs_clear_edge {
            self.hold("the retest bracket does not clear the edge");
            return Vec::new();
        }
        self.clear_notes();
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
            cancel_at: bracket.take_profit(),
            // The flat gate held at fire time, but this order can rest for
            // hours: if a human opens a position while it waits, filling
            // would trade against that position — so the simulator stands
            // the order down at its own fill moment instead. "A bot never
            // trades against an open manual position" has to survive the
            // resting window, not just the trigger bar.
            flat_only: true,
        }]
    }

    /// The protective bracket for this signal, or `None` with the reason
    /// already on the badge. A promised leg that cannot be priced is a
    /// reason not to fire, never a reason to fire unprotected — and one
    /// definition of that refusal serves both the market entry and the
    /// retest limit.
    fn project(&mut self, signal: &Signal) -> Option<Bracket> {
        let bracket = project_bracket(
            self.params.side,
            signal.reference,
            signal.projection,
            self.params.tp_mult,
            self.params.sl_mult,
        );
        if bracket.is_none() {
            self.hold("the projection is not priceable");
        }
        bracket
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
    pub fn on_sim_events(&mut self, events: &[VenueEvent]) -> Vec<Command> {
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
                    VenueEvent::Placed(order),
                ) => {
                    self.state = ArmedState::Fired {
                        order_id: Some(order.id),
                        retest: *retest,
                    };
                }
                (ArmedState::Fired { order_id: None, .. }, VenueEvent::Rejected(reason)) => {
                    self.state = ArmedState::Disarmed {
                        reason: DisarmReason::EntryRejected(*reason),
                    };
                }
                (
                    ArmedState::Fired {
                        order_id: Some(id), ..
                    },
                    VenueEvent::Filled(fill),
                ) if fill.role == quantick_sim::FillRole::Entry(*id) => {
                    self.state = ArmedState::InPosition;
                    my_fill_in_batch = true;
                }
                (
                    ArmedState::Fired {
                        order_id: Some(id), ..
                    },
                    VenueEvent::Cancelled { order, reason },
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
                (ArmedState::InPosition, VenueEvent::BracketDropped { .. }) if my_fill_in_batch => {
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
                self.clear_notes();
                self.state = ArmedState::Armed;
            }
            ArmedState::Done | ArmedState::Disarmed { .. } => {
                self.clear_notes();
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
        self.clear_notes();
    }

    /// Forget both the bar's note and the standing refusal — used where
    /// something actually happened: an order went out, or the instance was
    /// re-warmed onto a series it has not judged yet.
    fn clear_notes(&mut self) {
        self.note = None;
        self.last_hold = None;
    }

    /// Record a gate's refusal: the badge shows it on this bar, and keeps
    /// it as the last refusal after the bar has passed. One door, so a new
    /// gate cannot be added that names itself on the badge today and is
    /// forgotten by the next quiet bar.
    fn hold(&mut self, reason: &'static str) {
        self.note = Some(Note::Held(reason));
        self.last_hold = Some(reason);
    }

    /// The refusal standing over this instance: the reason, and whether it
    /// is about the bar that just closed.
    ///
    /// Both halves, because the reason alone is a claim about *now* and a
    /// standing refusal is not one. "account not flat" printed flat over an
    /// account the trader has since closed out of is the badge saying the
    /// bot is blocked when it is not — the same misinformation in the
    /// opposite direction from the silence this exists to end. A surface
    /// that shows a stale reason owes it a word (`last held`), and one with
    /// no room for that word shows only the fresh one.
    ///
    /// Handed out unframed so every surface phrases it for itself, and so
    /// an operator that is not looking at any of them can compare a value
    /// instead of parsing English out of `status_line`.
    #[must_use]
    pub fn hold_reason(&self) -> Option<HoldReason> {
        match self.note {
            Some(Note::Held(reason)) => Some(HoldReason {
                reason,
                fresh: true,
            }),
            Some(Note::Aside(_)) => None,
            None => self.last_hold.map(|reason| HoldReason {
                reason,
                fresh: false,
            }),
        }
    }

    /// One line for the on-chart badge.
    #[must_use]
    pub fn status_line(&self) -> String {
        match &self.state {
            // Fresh first: the bar just judged is the most useful sentence
            // there is. Once it has passed, the ruler's live reading leads
            // and the last refusal rides behind it — the trader reads this
            // badge after the move, not during it.
            ArmedState::Armed => match (self.note, self.last_hold) {
                (Some(note), _) => format!("armed · {}", note.line()),
                (None, Some(held)) => {
                    format!("armed · {} · last held: {held}", self.trigger.status())
                }
                (None, None) => format!("armed · {}", self.trigger.status()),
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
    Some(Bracket::whole(stop_loss, take_profit))
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

    /// A bar whose shadows reach past its body, so a fixture can put the
    /// wick inside the region and the body outside it — the shape the whole
    /// rule turns on.
    fn wicked(open: &str, close: &str, high: &str, low: &str) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: dec(open),
            high: dec(high),
            low: dec(low),
            close: dec(close),
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
            execution: Execution::Paper,
        }
    }

    fn force_instance(side: Side) -> ArmedStrategy {
        ArmedStrategy::new(
            params(side),
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_size: Decimal::ZERO,
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
                bracket: Bracket::whole(Some(dec("100")), Some(dec("112"))),
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
                bracket: Bracket::whole(Some(dec("100")), Some(dec("88"))),
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

        // Closed away from the region: the buy force bar closes at 106,
        // below a band sitting at 110–120, so it never reached it.
        let above_region = Region::new(dec("110"), dec("120"));
        let mut instance = force_instance(Side::Buy);
        assert!(warm_then_force(&mut instance, &above_region).is_empty());
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: closed outside region"
        );

        // Past the edge without crossing it: the same bar opens at 102,
        // already above a band sitting at 90–100. Two different holds, two
        // different sentences — asserted here so neither can drift into
        // the other's wording unnoticed.
        let low_region = Region::new(dec("90"), dec("100"));
        let mut instance = force_instance(Side::Buy);
        assert!(warm_then_force(&mut instance, &low_region).is_empty());
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: the body never cut the region"
        );

        // Region no longer active in time.
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        assert!(
            instance
                .on_closed_bar(&bar("102", "106"), &region, false, true)
                .is_empty()
        );

        // Account not flat — the gate the simulator's single netted
        // position still requires.
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
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        };
        let _ = instance.on_sim_events(&[VenueEvent::Placed(order.clone())]);
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
        let _ = instance.on_sim_events(&[VenueEvent::Filled(foreign_fill)]);
        assert!(matches!(instance.state(), ArmedState::Fired { .. }));

        let fill = quantick_sim::Fill {
            role: quantick_sim::FillRole::Entry(OrderId(7)),
            ..foreign_fill
        };
        let _ = instance.on_sim_events(&[VenueEvent::Filled(fill)]);
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
                min_size: Decimal::ZERO,
            })),
        );
        warm_then_force(&mut instance, &region);
        let _ = instance.on_sim_events(&[VenueEvent::Placed(quantick_sim::Order {
            id: OrderId(1),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
            placed_ms: 0,
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        })]);
        let _ = instance.on_sim_events(&[VenueEvent::Filled(quantick_sim::Fill {
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
        let _ = instance.on_sim_events(&[VenueEvent::Rejected(RejectReason::NoMarketPrice)]);
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
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        };
        let _ = instance.on_sim_events(&[VenueEvent::Placed(order.clone())]);
        let _ = instance.on_sim_events(&[VenueEvent::Cancelled {
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
                min_size: Decimal::ZERO,
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
                bracket: Bracket::whole(Some(dec("104")), Some(dec("106"))),
            }]
        );
        assert!(instance.status_line().starts_with("fired"));
    }

    /// The other half of the port's new method: a ruler with no honest
    /// provisional reading keeps the default and is simply never previewed.
    ///
    /// The fake below fires on *every* closed bar and declines to preview,
    /// which is exactly the pair that would expose a consumer guessing. An
    /// alarm that fell back to the closed-bar path for a trigger saying "I
    /// cannot judge a bar that has not finished" would announce a signal
    /// that ruler never made.
    #[test]
    fn a_trigger_that_declines_to_preview_is_never_previewed() {
        struct EveryClosedBar;
        impl Trigger for EveryClosedBar {
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
        let mut instance = ArmedStrategy::new(params(Side::Buy), Box::new(EveryClosedBar));
        let forming = bar("100", "105");
        assert_eq!(
            instance.preview_opportunity(&forming, &region, true),
            None,
            "the default preview says nothing, and nothing is what the caller hears"
        );
        // The same bar, closed, is a signal — so the silence above is the
        // port's default speaking, not the fixture failing to qualify.
        assert!(
            !instance
                .on_closed_bar(&forming, &region, true, true)
                .is_empty()
        );
        assert_eq!(instance.last_close_opportunity(), Some(Opportunity::Market));
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
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        };
        let _ = instance.on_sim_events(&[VenueEvent::Placed(order)]);

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
            VenueEvent::Filled(fill),
            VenueEvent::BracketDropped {
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
        let _ = bystander.on_sim_events(&[VenueEvent::Placed(quantick_sim::Order {
            id: OrderId(9),
            side: Side::Buy,
            kind: quantick_sim::EntryKind::Market,
            price: None,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
            placed_ms: 0,
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        })]);
        let commands = bystander.on_sim_events(&[VenueEvent::BracketDropped {
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
                min_size: Decimal::ZERO,
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
                bracket: Bracket::whole(Some(dec("110")), Some(dec("98"))),
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
                bracket: Bracket::whole(Some(dec("95")), Some(dec("107"))),
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
                min_size: Decimal::ZERO,
            })),
        );
        assert!(warm_then_sell_cut(&mut instance, &region).is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: cut the region, retest option off",
            "the bar did its part; the option is what held fire, and the \
             badge blames the option rather than the bar"
        );
    }

    /// A close beyond the *opposite* edge is a miss, never a retest: a sell
    /// bar closing above the region has not cut anything downward. Its note
    /// is pinned here so it cannot silently become the same sentence a real
    /// cut produces — the two mean opposite things to a trader.
    ///
    /// Like the market-path test above, this holds on both sides of the
    /// fix: it guards against over-correction and against the note drifting,
    /// not against the reported bug.
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
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: closed outside region"
        );
    }

    /// The geometry outranks the projection. A crash bar whose body never
    /// came near the region also prices an impossible take profit; before
    /// the gates were reordered the badge blamed the projection, sending
    /// the trader to tune multipliers that were never the reason this bar
    /// did not trade.
    #[test]
    fn a_bar_that_cut_nothing_blames_the_geometry_not_the_projection() {
        // Region far above the tape: nothing here could ever have traded.
        let region = Region::new(dec("300"), dec("320"));
        let mut instance = retest_instance(Side::Sell);
        instance.on_closed_bar(&bar("102", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "100"), &region, true, true);
        // Close 90 with range 110 prices the take profit at -20: invalid.
        let commands =
            instance.on_closed_bar(&wicked("100", "96", "200", "90"), &region, true, true);
        assert!(commands.is_empty());
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: the body never cut the region",
            "the bar's geometry is the honest reason, not a leg it would never have used"
        );
    }

    /// The reported bug, in the shape that proves it. A sell region sits at
    /// 105–115 and the force bar's *shadow* reaches 106, right into the
    /// band — but its body opened at 100 and closed at 96, entirely below
    /// the region, so it cut nothing. The old close-only rule rested a
    /// limit at 105: an order inside a band the bar's body never entered,
    /// which is what the trader saw on the chart.
    ///
    /// The numbers are chosen so the pre-fix code really did place that
    /// order — range 10 puts SL at 106 and TP at 86, both clear of the 105
    /// edge — otherwise this test would pass against the bug it names.
    #[test]
    fn a_body_below_the_region_rests_nothing_however_far_its_wick_reaches() {
        let region = Region::new(dec("105"), dec("115"));
        let mut instance = retest_instance(Side::Sell);
        instance.on_closed_bar(&bar("102", "101"), &region, true, true);
        instance.on_closed_bar(&bar("101", "100"), &region, true, true);
        // Body 4 over average (1+1+4)/3 = 2: a genuine force bar.
        let commands =
            instance.on_closed_bar(&wicked("100", "96", "106", "96"), &region, true, true);
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

    /// The buy mirror, built the same way: the shadow dips to 99 inside the
    /// 90–100 band while the body runs 101 → 105 above it. Range 6 puts SL
    /// at 99 and TP at 111 around the 100 edge, so the pre-fix code rested
    /// a buy limit at 100 here.
    #[test]
    fn a_body_above_the_region_rests_nothing_however_far_its_wick_reaches() {
        let region = Region::new(dec("90"), dec("100"));
        let mut instance = retest_instance(Side::Buy);
        instance.on_closed_bar(&bar("101", "102"), &region, true, true);
        instance.on_closed_bar(&bar("102", "103"), &region, true, true);
        let commands =
            instance.on_closed_bar(&wicked("101", "105", "105", "99"), &region, true, true);
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
    ///
    /// This one passes against the old rule too, deliberately — it does not
    /// guard the reported bug, it guards the *over-correction*: a fix that
    /// started demanding something of the open on the market path would
    /// turn this green test red, which is the point of keeping it.
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
            bracket: Bracket::whole(Some(dec("110")), Some(dec("98"))),
            cancel_at: Some(dec("98")),
            flat_only: true,
            placed_ms: 0,
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
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
        let _ = one_shot.on_sim_events(&[VenueEvent::Placed(order.clone())]);
        let _ = one_shot.on_sim_events(&[VenueEvent::Cancelled {
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
                min_size: Decimal::ZERO,
            })),
        );
        warm_then_sell_cut(&mut auto, &region);
        let order = retest_order(5);
        let _ = auto.on_sim_events(&[VenueEvent::Placed(order.clone())]);
        let _ = auto.on_sim_events(&[VenueEvent::Cancelled {
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
        let _ = one_shot.on_sim_events(&[VenueEvent::Placed(order.clone())]);
        let _ = one_shot.on_sim_events(&[VenueEvent::Cancelled {
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
                min_size: Decimal::ZERO,
            })),
        );
        warm_then_sell_cut(&mut auto, &region);
        let order = retest_order(15);
        let _ = auto.on_sim_events(&[VenueEvent::Placed(order.clone())]);
        let _ = auto.on_sim_events(&[VenueEvent::Cancelled {
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
        let _ = instance.on_sim_events(&[VenueEvent::Placed(order.clone())]);
        let _ = instance.on_sim_events(&[VenueEvent::Cancelled {
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
        let _ = instance.on_sim_events(&[VenueEvent::Placed(retest_order(7))]);
        let fill = quantick_sim::Fill {
            timestamp_ms: 9,
            agg_id: 30,
            side: Side::Sell,
            price: dec("105"),
            quantity: Decimal::ONE,
            role: quantick_sim::FillRole::Entry(OrderId(7)),
        };
        let _ = instance.on_sim_events(&[VenueEvent::Filled(fill)]);
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
        let _ = instance.on_sim_events(&[VenueEvent::Placed(retest_order(8))]);
        let commands = instance.disarm(DisarmReason::User);
        assert_eq!(commands, vec![Command::CancelOrder { id: OrderId(8) }]);

        let mut instance = retest_instance(Side::Sell);
        warm_then_sell_cut(&mut instance, &region);
        let _ = instance.on_sim_events(&[VenueEvent::Placed(retest_order(9))]);
        let commands = instance.disarm(DisarmReason::TimelineReset);
        assert!(
            commands.is_empty(),
            "the simulator's reset owns that cancellation"
        );
    }

    /// The account gate, restored after a branch that removed it: an
    /// instance holds while the account carries a position, and the badge
    /// names that gate like any other.
    ///
    /// It reads as a cross-region coupling and the trader asked for it
    /// gone. It cannot go while `quantick-sim` models **one** netted
    /// position with **one** exit ladder: a bracketed second entry on the
    /// same side replaces that ladder wholesale (`simulator.rs`), so the
    /// first instance keeps a badge reading "in position" over a position
    /// whose stop it no longer owns. Removing
    /// this gate is a simulator change — per-region accounts — not a
    /// kernel one.
    #[test]
    fn a_busy_account_holds_the_order_and_still_reports_the_opportunity() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        instance.on_closed_bar(&bar("100", "101"), &region, true, false);
        instance.on_closed_bar(&bar("101", "102"), &region, true, false);
        let commands = instance.on_closed_bar(&bar("102", "106"), &region, true, false);

        assert!(commands.is_empty(), "a busy account places nothing");
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: account not flat"
        );
        assert_eq!(
            instance.hold_reason(),
            Some(HoldReason {
                reason: "account not flat",
                fresh: true,
            })
        );
        assert_eq!(
            instance.last_close_opportunity(),
            Some(Opportunity::Market),
            "the setup happened, whoever was holding the account"
        );
    }

    /// A refusal is a statement about a bar, and it stops being one when
    /// the next bar closes. A surface that prints it flat says "the bot is
    /// blocked" about an account that may have gone flat twenty bars ago —
    /// the same misinformation as the silence this exists to end, pointing
    /// the other way. So the reason travels with its tense.
    #[test]
    fn a_refusal_says_whether_it_is_about_the_bar_that_just_closed() {
        // Wide enough that the bar which finally fires still closes inside
        // it: the point here is the tense of the reason, not the geometry.
        let region = Region::new(dec("100"), dec("115"));
        let mut instance = force_instance(Side::Buy);
        assert_eq!(instance.hold_reason(), None, "nothing refused yet");

        // A busy account holds this bar's force bar.
        instance.on_closed_bar(&bar("100", "101"), &region, true, false);
        instance.on_closed_bar(&bar("101", "102"), &region, true, false);
        instance.on_closed_bar(&bar("102", "106"), &region, true, false);
        assert_eq!(
            instance.hold_reason(),
            Some(HoldReason {
                reason: "account not flat",
                fresh: true,
            }),
            "about the bar that just closed"
        );

        // The next bar carries no signal: the refusal stands, and says so.
        instance.on_closed_bar(&bar("106", "107"), &region, true, true);
        assert_eq!(
            instance.hold_reason(),
            Some(HoldReason {
                reason: "account not flat",
                fresh: false,
            }),
            "still readable, no longer a claim about now"
        );
        assert!(
            instance
                .status_line()
                .ends_with("· last held: account not flat"),
            "and the sentence carries the tense too: {}",
            instance.status_line()
        );

        // Something happening clears it: the order goes out.
        let commands = instance.on_closed_bar(&bar("107", "113"), &region, true, true);
        assert!(
            matches!(commands.as_slice(), [Command::PlaceMarket { .. }]),
            "the setup fired: {commands:?}"
        );
        assert_eq!(
            instance.hold_reason(),
            None,
            "a refusal is cleared by something happening, never by nothing"
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
                min_size: Decimal::ZERO,
            })),
        );
        // SL = 104 + 0.1 × 6 = 104.6, below the 105 edge a short entered
        // there needs it above.
        assert!(warm_then_sell_cut(&mut instance, &region).is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: the retest bracket does not clear the edge"
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

        // The next quiet bar hands the badge back to the ruler — and keeps
        // the refusal behind it. A trader reads this badge after the move,
        // and a single quiet bar used to erase the only sentence explaining
        // why the setup they watched go by was declined.
        instance.on_closed_bar(&bar("106", "107"), &region, true, true);
        let line = instance.status_line();
        assert!(
            line.starts_with("armed · quiet"),
            "the ruler speaks again: {line}"
        );
        assert!(
            line.ends_with("· last held: region not active on this bar"),
            "and the refusal is still readable: {line}"
        );

        // Something *happening* is what clears it: an order going out.
        let mut instance = force_instance(Side::Buy);
        warm_then_force(&mut instance, &region);
        assert!(
            !instance.status_line().contains("last held"),
            "a fired instance carries no standing refusal: {}",
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

    fn alarm_only_instance(side: Side) -> ArmedStrategy {
        ArmedStrategy::new(
            StrategyParams {
                execution: Execution::AlarmOnly,
                ..params(side)
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: dec("1.5"),
                max_factor: dec("2.5"),
                min_size: Decimal::ZERO,
            })),
        )
    }

    /// A spent one-shot instance has stopped trading, not stopped watching.
    /// Its ruler keeps running (the port's contract) and so does the
    /// opportunity it reports — which is what keeps the alarm speaking
    /// after the single operation the trader allowed it has closed.
    #[test]
    fn a_finished_one_shot_still_reports_the_opportunities_it_will_not_take() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = force_instance(Side::Buy);
        assert!(!warm_then_force(&mut instance, &region).is_empty());
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
            role: quantick_sim::OrderRole::Entry,
            oco: None,
            reduce_only: false,
        };
        let _ = instance.on_sim_events(&[VenueEvent::Placed(order)]);
        let _ = instance.on_sim_events(&[VenueEvent::Filled(quantick_sim::Fill {
            timestamp_ms: 1,
            agg_id: 10,
            side: Side::Buy,
            price: dec("106"),
            quantity: Decimal::ONE,
            role: quantick_sim::FillRole::Entry(OrderId(7)),
        })]);
        // Flat again on the next bar: the one shot is spent.
        instance.on_closed_bar(&bar("106", "107"), &region, true, true);
        assert_eq!(instance.state(), &ArmedState::Done);

        // Another force bar closing inside the region, with nothing left to
        // trade it: bodies 1, 1, then 4 — ratio 2, the same force.
        instance.on_closed_bar(&bar("107", "108"), &region, true, true);
        let commands = instance.on_closed_bar(&bar("104", "108"), &region, true, true);
        assert!(commands.is_empty(), "a finished one shot places nothing");
        assert_eq!(
            instance.last_close_opportunity(),
            Some(Opportunity::Market),
            "a spent instance still recognises the setup it will not trade"
        );
    }

    /// [`Execution::AlarmOnly`]: the instance judges the bar exactly as a
    /// trading one does, reports the opportunity, emits nothing, and — the
    /// part that matters — stays armed. An instance that spent itself on an
    /// order it never placed would go quiet on the very next setup.
    #[test]
    fn an_alarm_only_instance_places_nothing_and_keeps_watching() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = alarm_only_instance(Side::Buy);
        let commands = warm_then_force(&mut instance, &region);
        assert!(commands.is_empty(), "alarm only places nothing");
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(
            instance.status_line(),
            "armed · alarm only — no order placed"
        );
        assert_eq!(instance.last_close_opportunity(), Some(Opportunity::Market));

        // And again on the next setup: nothing was spent. Two body-1 bars
        // walk the window back to [1, 1] before the body-4 force bar.
        instance.on_closed_bar(&bar("106", "107"), &region, true, true);
        instance.on_closed_bar(&bar("107", "108"), &region, true, true);
        let commands = instance.on_closed_bar(&bar("104", "108"), &region, true, true);
        assert!(commands.is_empty());
        assert_eq!(instance.state(), &ArmedState::Armed);
        assert_eq!(instance.last_close_opportunity(), Some(Opportunity::Market));
    }

    /// An alarm-only instance is not a blind one: a bar that never reached
    /// the region is refused by the geometry and named by it, so a trader
    /// tuning a silent alarm is told what actually held it.
    #[test]
    fn an_alarm_only_instance_still_names_the_geometry_that_held_it() {
        let region = Region::new(dec("100"), dec("110"));
        let mut instance = alarm_only_instance(Side::Buy);
        instance.on_closed_bar(&bar("120", "121"), &region, true, true);
        instance.on_closed_bar(&bar("121", "122"), &region, true, true);
        instance.on_closed_bar(&bar("122", "126"), &region, true, true);
        assert_eq!(
            instance.status_line(),
            "armed · trigger held: the body never cut the region"
        );
        assert_eq!(instance.last_close_opportunity(), None);
    }

    /// The line the mid-bar alarm must never cross: reading the forming bar
    /// is `&self`, so it cannot place an order, cannot move the state
    /// machine, and cannot advance the ruler. The proof is that the *same*
    /// bar, previewed any number of times and then closed, walks exactly
    /// the path it would have walked with no preview at all.
    #[test]
    fn previewing_the_forming_bar_emits_no_command_and_moves_nothing() {
        let region = Region::new(dec("100"), dec("110"));
        let mut previewed = force_instance(Side::Buy);
        let mut untouched = force_instance(Side::Buy);
        for warm in [bar("100", "101"), bar("101", "102")] {
            previewed.on_closed_bar(&warm, &region, true, true);
            untouched.on_closed_bar(&warm, &region, true, true);
        }

        // The bar forming toward the force close, judged over and over.
        let forming = bar("102", "106");
        for _ in 0..50 {
            assert_eq!(
                previewed.preview_opportunity(&forming, &region, true),
                Some(Opportunity::Market),
                "the forming bar already qualifies"
            );
        }
        assert_eq!(previewed.state(), &ArmedState::Armed);
        assert_eq!(
            previewed.status_line(),
            untouched.status_line(),
            "a preview leaves the ruler's own narration alone"
        );

        // Now close it. Both instances must answer identically: fifty
        // previews changed nothing at all.
        let previewed_commands = previewed.on_closed_bar(&forming, &region, true, true);
        let untouched_commands = untouched.on_closed_bar(&forming, &region, true, true);
        assert_eq!(previewed_commands, untouched_commands);
        assert_eq!(
            previewed_commands.len(),
            1,
            "the closed bar is the one that fires"
        );
        assert_eq!(previewed.state(), untouched.state());
        assert_eq!(previewed.status_line(), untouched.status_line());
    }

    /// The preview reads the same gates the order does, so it holds on the
    /// same bars: the wrong side, a dead region, a body that never reached
    /// the band. A preview that were more permissive would alarm the trader
    /// into a trade this strategy would not take.
    #[test]
    fn the_preview_holds_on_exactly_the_bars_the_order_holds_on() {
        let region = Region::new(dec("100"), dec("110"));
        let mut sell = force_instance(Side::Sell);
        let mut buy = force_instance(Side::Buy);
        for warm in [bar("100", "101"), bar("101", "102")] {
            sell.on_closed_bar(&warm, &region, true, true);
            buy.on_closed_bar(&warm, &region, true, true);
        }
        let force = bar("102", "106");

        // A buy force bar for a sell instance: the wrong side.
        assert_eq!(sell.preview_opportunity(&force, &region, true), None);
        // The region is not active on this bar.
        assert_eq!(buy.preview_opportunity(&force, &region, false), None);
        // A quiet bar is not a signal, however active the region is.
        assert_eq!(
            buy.preview_opportunity(&bar("102", "103"), &region, true),
            None
        );
        // The qualifying case, so the test cannot pass by always saying no.
        assert_eq!(
            buy.preview_opportunity(&force, &region, true),
            Some(Opportunity::Market)
        );
    }

    /// A cut is an opportunity only when the policy would rest on it. The
    /// preview follows [`BreakPolicy`] rather than inventing a second
    /// answer, so the alarm never announces a setup the strategy ignores.
    #[test]
    fn the_preview_follows_the_break_policy_on_a_cutting_bar() {
        let region = Region::new(dec("105"), dec("115"));
        // Opens at 110, inside the band, and finishes at 102, below its
        // low: the body crossed the edge a sell leaves by. Body 8 against
        // two body-2 neighbours is ratio 2 — force.
        let cutting = bar("110", "102");
        let ruler = ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_size: Decimal::ZERO,
        };
        let mut ignoring = ArmedStrategy::new(
            params(Side::Sell),
            Box::new(ForceTrigger::new(ruler.clone())),
        );
        let mut resting = ArmedStrategy::new(
            StrategyParams {
                on_break: BreakPolicy::RetestLimit,
                ..params(Side::Sell)
            },
            Box::new(ForceTrigger::new(ruler)),
        );
        for warm in [bar("112", "110"), bar("110", "112")] {
            ignoring.on_closed_bar(&warm, &region, true, true);
            resting.on_closed_bar(&warm, &region, true, true);
        }

        assert_eq!(
            ignoring.preview_opportunity(&cutting, &region, true),
            None,
            "with the retest option off, a cut is not an opportunity"
        );
        assert_eq!(
            resting.preview_opportunity(&cutting, &region, true),
            Some(Opportunity::Retest { edge: dec("105") })
        );
    }
}
