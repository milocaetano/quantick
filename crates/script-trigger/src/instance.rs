//! One script armed on one placement: alarm-only, or placing paper orders
//! through the kernel's armed path.

use std::cell::RefCell;
use std::rc::Rc;

use quantick_engine::{Bar, Side};
use quantick_sim::Command;
use quantick_strategy::runner::{self, RunnerInstance, StrategyPort};
use quantick_strategy::sound::Cue;
use quantick_strategy::{
    AlarmParams, ArmedState, ArmedStrategy, BreakPolicy, DisarmReason, Execution, OrderQuantity,
    PriceBracket, Rearm, SignalAction, SignalAlarm, StrategyParams,
};
use rust_decimal::Decimal;

use crate::placement::{ResolvedPlacement, ScriptPlacement};
use crate::script::{Script, ScriptFault};
use crate::trigger::{ScriptRun, ScriptTrigger};

/// Whether an instance is still listening, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptState {
    Watching,
    /// Stopped for good: `line N: <CODE> <message>` when its script failed,
    /// the reason it was halted, or the kernel's disarm label.
    Disarmed {
        reason: String,
    },
}

/// One script armed on one placement: the kernel's state machine judging the
/// script's signals, the alarm that rings on what it judged, and — for a
/// [`SignalAction::Order`] — the paper order the kernel places.
pub struct ScriptInstance<D> {
    /// Stable for the session: what a caller lists and disarms it by.
    pub id: u64,
    /// The library file the script was loaded from.
    pub script: String,
    pub placement: ScriptPlacement<D>,
    action: SignalAction,
    armed: ArmedStrategy,
    run: Rc<RefCell<ScriptRun>>,
    alarm: SignalAlarm,
    cue: Cue,
    /// Why the instance was stopped from outside the kernel — its region's
    /// drawing removed — which no [`DisarmReason`] names.
    halted: Option<&'static str>,
}

impl<D> std::fmt::Debug for ScriptInstance<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptInstance")
            .field("id", &self.id)
            .field("script", &self.script)
            .field("action", &self.action)
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

impl<D: Copy> ScriptInstance<D> {
    /// Arm `script` hunting `side` on `placement` to do `action`, its run
    /// warmed on `warmup` — closed bars with their footprint POC, oldest
    /// first, that ring and place nothing. A script that fails while warming
    /// is armed already disarmed, with the reason.
    ///
    /// An order sized by the ticket places nothing until
    /// [`Self::refresh_ticket`] tells it the ticket's quantity.
    #[must_use]
    pub fn arm(
        id: u64,
        script: &Script,
        side: Side,
        placement: ScriptPlacement<D>,
        warmup: &[(Bar, Option<Decimal>)],
        action: SignalAction,
    ) -> Self {
        let (trigger, run) = ScriptTrigger::new(script, side);
        let (execution, quantity, bracket) = kernel_terms(action, None);
        let params = StrategyParams {
            side,
            quantity,
            tp_mult: Decimal::ZERO,
            sl_mult: Decimal::ZERO,
            bracket,
            // A script rides no drawing to be re-armed from: every
            // qualifying close on a flat account may fire.
            rearm: Rearm::Auto,
            on_break: BreakPolicy::Ignore,
            execution,
        };
        let mut armed = ArmedStrategy::new(params, Box::new(trigger));
        for (bar, poc) in warmup {
            run.borrow_mut().poc = *poc;
            armed.warm(std::slice::from_ref(bar));
        }
        // A flip among the warmup bars rang nothing; the badge must not
        // narrate it as the last bar's.
        run.borrow_mut().forget_last();
        Self {
            id,
            script: script.name().to_owned(),
            placement,
            action,
            armed,
            run,
            alarm: SignalAlarm::new(AlarmParams::default()),
            cue: Cue::default(),
            halted: None,
        }
    }

    #[must_use]
    pub fn side(&self) -> Side {
        self.armed.params().side
    }

    /// What a fired signal does.
    #[must_use]
    pub fn action(&self) -> SignalAction {
        self.action
    }

    /// Whether a fired signal places a paper order.
    #[must_use]
    pub fn places_orders(&self) -> bool {
        matches!(self.action, SignalAction::Order(_))
    }

    /// Do `action` from the next closed bar on. A live order or position
    /// keeps its course; an order sized by the ticket waits for
    /// [`Self::refresh_ticket`].
    pub fn set_action(&mut self, action: SignalAction) {
        self.action = action;
        let (execution, quantity, bracket) = kernel_terms(action, None);
        self.armed.set_execution(execution, quantity, bracket);
    }

    /// Size the next order by the ticket's `quantity` — `None` when the
    /// ticket cannot say, which holds the entry. Only an order sized by the
    /// ticket listens; the consumer calls it before each closed bar.
    pub fn refresh_ticket(&mut self, quantity: Option<Decimal>) {
        if let SignalAction::Order(order) = self.action
            && order.quantity == OrderQuantity::Ticket
        {
            let (execution, quantity, bracket) = kernel_terms(self.action, quantity);
            self.armed.set_execution(execution, quantity, bracket);
        }
    }

    /// The kernel instance, for its badge line and refusals.
    #[must_use]
    pub fn armed(&self) -> &ArmedStrategy {
        &self.armed
    }

    #[must_use]
    pub fn fault(&self) -> Option<ScriptFault> {
        self.run.borrow().fault.clone()
    }

    #[must_use]
    pub fn state(&self) -> ScriptState {
        if let Some(fault) = self.fault() {
            return ScriptState::Disarmed {
                reason: fault.to_string(),
            };
        }
        if let Some(reason) = self.halted {
            return ScriptState::Disarmed {
                reason: reason.to_owned(),
            };
        }
        match self.armed.state() {
            ArmedState::Disarmed { reason } => ScriptState::Disarmed {
                reason: reason.label().to_owned(),
            },
            _ => ScriptState::Watching,
        }
    }

    /// Whether the instance still judges bars. Asked per print, so it
    /// formats nothing.
    #[must_use]
    pub fn watching(&self) -> bool {
        self.halted.is_none()
            && self.run.borrow().fault.is_none()
            && !matches!(self.armed.state(), ArmedState::Disarmed { .. })
    }

    /// Feed a closed bar nobody heard live — a refilled hole — to the script
    /// only: it rings nothing and judges no gate, so the script's history
    /// stays the one the chart's copy of it computed.
    pub fn warm(&mut self, bar: &Bar, poc: Option<Decimal>) {
        if !self.watching() {
            return;
        }
        self.run.borrow_mut().poc = poc;
        self.armed.warm(std::slice::from_ref(bar));
        self.run.borrow_mut().forget_last();
    }

    /// Stop listening for `reason`, answering what the account is owed: the
    /// cancel of an entry still pending. A live position stays the trader's.
    #[must_use = "apply the returned cleanup commands to the account"]
    pub fn disarm(&mut self, reason: DisarmReason) -> Vec<Command> {
        self.alarm.reset();
        self.armed.disarm(reason)
    }

    /// Stop listening for a reason outside the kernel's; the first one
    /// stands. Answers what the account is owed, as [`Self::disarm`] does.
    #[must_use = "apply the returned cleanup commands to the account"]
    pub fn halt(&mut self, reason: &'static str) -> Vec<Command> {
        if self.halted.is_some() {
            return Vec::new();
        }
        self.halted = Some(reason);
        self.alarm.reset();
        self.armed.disarm(DisarmReason::User)
    }

    /// Judge the bar that just closed, whose footprint POC was `poc`, against
    /// `resolved` (the placement for this bar, `None` when it could not be
    /// read: the gates shut and the script still runs), through the runner
    /// every armed instance steps by: an order goes out through `port`, and
    /// its events come back to this instance. Answers the sound to play when
    /// the alarm rang.
    pub fn on_closed_bar<P: StrategyPort + ?Sized>(
        &mut self,
        bar: &Bar,
        poc: Option<Decimal>,
        resolved: Option<&(ResolvedPlacement, bool)>,
        port: &mut P,
        now_ms: u64,
    ) -> Option<Cue> {
        if !self.watching() {
            return None;
        }
        self.run.borrow_mut().poc = poc;
        let cues = runner::step_closed_bar(
            std::slice::from_mut(self),
            bar,
            0,
            |(), _| resolved.copied(),
            port,
            now_ms,
        );
        cues.into_iter().next()
    }
}

/// The kernel's terms for `action`; an order sized by the ticket takes
/// `ticket`, and zero — which holds — when that is unknown.
fn kernel_terms(
    action: SignalAction,
    ticket: Option<Decimal>,
) -> (Execution, Decimal, Option<PriceBracket>) {
    match action {
        SignalAction::Alarm => (Execution::AlarmOnly, Decimal::ONE, None),
        SignalAction::Order(order) => {
            let quantity = match order.quantity {
                OrderQuantity::Fixed(quantity) => quantity,
                OrderQuantity::Ticket => ticket.unwrap_or(Decimal::ZERO),
            };
            (Execution::Paper, quantity, Some(order.bracket()))
        }
    }
}

/// A script instance as the runner steps it. Its placement is resolved by
/// the caller for each bar, so it rides no anchor of its own.
impl<D> RunnerInstance for ScriptInstance<D> {
    type Anchor = ();

    fn anchor(&self) {}

    fn armed_mut(&mut self) -> &mut ArmedStrategy {
        &mut self.armed
    }

    /// The alarm judges the close the kernel just judged; a script that
    /// failed on it rings nothing.
    fn alarm_on_closed_bar(&mut self, now_ms: u64) -> Option<Cue> {
        if self.run.borrow().fault.is_some() {
            self.alarm.reset();
            return None;
        }
        let qualifies = self.armed.last_close_opportunity().is_some();
        let event = self.alarm.on_closed(qualifies, now_ms)?;
        event.sounds().then_some(self.cue)
    }
}
