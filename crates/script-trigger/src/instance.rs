//! One script armed alarm-only on one placement.

use std::cell::RefCell;
use std::rc::Rc;

use quantick_engine::{Bar, Side};
use quantick_strategy::runner::placement_or_hold;
use quantick_strategy::sound::Cue;
use quantick_strategy::{
    AlarmParams, ArmedState, ArmedStrategy, BreakPolicy, DisarmReason, Execution, Rearm,
    SignalAlarm, StrategyParams,
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

/// One script armed [`Execution::AlarmOnly`] on one placement: the kernel's
/// state machine judging the script's signals, and the alarm that rings on
/// what it judged.
pub struct ScriptInstance<D> {
    /// Stable for the session: what a caller lists and disarms it by.
    pub id: u64,
    /// The library file the script was loaded from.
    pub script: String,
    pub placement: ScriptPlacement<D>,
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
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

impl<D: Copy> ScriptInstance<D> {
    /// Arm `script` hunting `side` on `placement`, its run warmed on
    /// `warmup` — closed bars with their footprint POC, oldest first, that
    /// ring nothing. A script that fails while warming is armed already
    /// disarmed, with the reason.
    #[must_use]
    pub fn arm(
        id: u64,
        script: &Script,
        side: Side,
        placement: ScriptPlacement<D>,
        warmup: &[(Bar, Option<Decimal>)],
    ) -> Self {
        let (trigger, run) = ScriptTrigger::new(script, side);
        let params = StrategyParams {
            side,
            quantity: Decimal::ONE,
            tp_mult: Decimal::ZERO,
            sl_mult: Decimal::ZERO,
            rearm: Rearm::Auto,
            on_break: BreakPolicy::Ignore,
            execution: Execution::AlarmOnly,
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

    /// Stop listening for `reason`. Alarm-only, so nothing is owed.
    pub fn disarm(&mut self, reason: DisarmReason) {
        let owed = self.armed.disarm(reason);
        debug_assert!(owed.is_empty(), "an alarm-only instance places nothing");
        self.alarm.reset();
    }

    /// Stop listening for a reason outside the kernel's; the first one stands.
    pub fn halt(&mut self, reason: &'static str) {
        self.halted.get_or_insert(reason);
        self.alarm.reset();
    }

    /// Judge the bar that just closed, whose footprint POC was `poc`, against
    /// `resolved` (the placement for this bar, `None` when it could not be
    /// read: the gates shut and the script still runs). Answers the sound
    /// to play when the alarm rang.
    pub fn on_closed_bar(
        &mut self,
        bar: &Bar,
        poc: Option<Decimal>,
        resolved: Option<&(ResolvedPlacement, bool)>,
        now_ms: u64,
    ) -> Option<Cue> {
        if !self.watching() {
            return None;
        }
        self.run.borrow_mut().poc = poc;
        let (placement, active) = placement_or_hold(resolved);
        // Alarm-only never reads the account: a flat one is the neutral answer.
        let owed = self.armed.on_closed_bar(bar, placement, active, true);
        debug_assert!(owed.is_empty(), "an alarm-only instance places nothing");
        if self.run.borrow().fault.is_some() {
            self.alarm.reset();
            return None;
        }
        let qualifies = self.armed.last_close_opportunity().is_some();
        let event = self.alarm.on_closed(qualifies, now_ms)?;
        event.sounds().then_some(self.cue)
    }
}
