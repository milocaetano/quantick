//! The Pine scripts a tab arms on its panes — ringing, or placing paper
//! orders — in plain words and numbers.
//!
//! The kernel types live in `quantick-strategy`, including its `script`
//! module; this module owns the instances a tab arms, so a call
//! arms, lists and disarms through here, as [`crate::strategy_reading`] reads
//! the drawn instances. Each instance rides
//! one pane — the bars it judges are that pane's closes — and is evaluated
//! with the pane's other strategies ([`TabStrategies::evaluate`]).

use quantick_engine::{Bar, Side};
use quantick_sim::Command;
use quantick_strategy::runner::StrategyPort;
pub use quantick_strategy::script::ScriptPlacement;
use quantick_strategy::script::{Script, ScriptInstance, ScriptState, WARMUP_BARS};
pub use quantick_strategy::{OrderQuantity, SignalAction, SignalOrder};
use quantick_trading::VenueEvent;
use rust_decimal::Decimal;

use crate::strategies::TabStrategies;

/// One arm: which script, read by the caller from its library, on which
/// pane, hunting which side, where, doing what when it fires.
#[derive(Debug, Clone, Copy)]
pub struct ScriptArm<'a, D> {
    pub pane_id: u64,
    /// The script's file name in the library.
    pub script: &'a str,
    pub source: &'a str,
    pub side: Side,
    pub place: ScriptPlacement<D>,
    pub action: SignalAction,
}

/// One armed script as a caller reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptReading<D> {
    /// Stable for the tab's life: what a disarm names.
    pub id: u64,
    pub pane_id: u64,
    pub script: String,
    pub side: Side,
    pub place: ScriptPlacement<D>,
    pub action: SignalAction,
    /// Why it stopped — `line N: <CODE> <message>` for a script that failed,
    /// else the disarm reason; `None` while it watches.
    pub disarm_reason: Option<String>,
    /// The kernel's badge line: what the last bar did.
    pub status: String,
}

impl<D: Copy> ScriptReading<D> {
    fn of(pane_id: u64, instance: &ScriptInstance<D>) -> Self {
        Self {
            id: instance.id,
            pane_id,
            script: instance.script.clone(),
            side: instance.side(),
            place: instance.placement,
            action: instance.action(),
            disarm_reason: match instance.state() {
                ScriptState::Watching => None,
                ScriptState::Disarmed { reason } => Some(reason),
            },
            status: instance.armed().status_line(),
        }
    }
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// Compile `request.source` and arm it on `request.pane_id` to do
    /// `request.action`,
    /// warmed on `warmup(depth)` — closed bars with their footprint POC,
    /// oldest first. A script that fails while warming is armed already
    /// disarmed, and its reading says why.
    ///
    /// # Errors
    ///
    /// The sentence the caller reads: an order with no size or a negative
    /// distance, a script that does not compile, an alert title that names
    /// no side, or none naming `request.side`.
    pub fn arm_script(
        &mut self,
        request: ScriptArm<'_, D>,
        warmup: impl FnOnce(usize) -> Vec<(Bar, Option<Decimal>)>,
    ) -> Result<ScriptReading<D>, String> {
        let name = request.script;
        action_refusal(request.action)
            .map_err(|refusal| format!("`{name}` cannot be armed: {refusal}"))?;
        let script = Script::load(name, request.source)
            .map_err(|refusal| format!("`{name}` cannot be armed: {refusal}"))?;
        if !script.sides().iter().any(|(_, side)| *side == request.side) {
            let titles: Vec<_> = (script.sides().iter())
                .map(|(title, _)| format!("`{title}`"))
                .collect();
            return Err(format!(
                "`{name}` cannot be armed to {}: none of its alert titles ({}) names that side",
                side_word(request.side),
                titles.join(", ")
            ));
        }
        let warm = warmup(WARMUP_BARS);
        let instance = ScriptInstance::arm(
            self.next_script_id,
            &script,
            request.side,
            request.place,
            &warm,
            request.action,
        );
        Ok(self.publish_script(request.pane_id, instance, script))
    }

    /// Number `instance`, put it on chart `pane_id` and record its compiled
    /// `script` in the desk, so every published script can be read, duplicated
    /// and saved there.
    pub(crate) fn publish_script(
        &mut self,
        pane_id: u64,
        mut instance: ScriptInstance<D>,
        script: Script,
    ) -> ScriptReading<D> {
        instance.id = self.next_script_id;
        self.next_script_id += 1;
        let reading = ScriptReading::of(pane_id, &instance);
        self.pane_mut(pane_id).scripts.push(instance);
        self.desk.record_script(reading.id, script);
        reading
    }

    /// Disarm and remove script instance `id`, answering it as it stood once
    /// disarmed; `None` when no pane holds it. A pending entry's cancel is
    /// queued with the pane's cleanup; an open position stays the trader's.
    pub fn disarm_script(&mut self, id: u64) -> Option<ScriptReading<D>> {
        let removed = self.panes_mut().find_map(|(pane_id, pane)| {
            let at = pane.scripts.iter().position(|script| script.id == id)?;
            let instance = pane.remove_script_at(at);
            Some(ScriptReading::of(pane_id, &instance))
        });
        self.desk.forget_script(id);
        removed
    }

    /// Every armed script, panes in id order, each pane's in arming order.
    #[must_use]
    pub fn script_readings(&self) -> Vec<ScriptReading<D>> {
        self.panes()
            .flat_map(|(pane_id, pane)| {
                (pane.scripts.iter()).map(move |script| ScriptReading::of(pane_id, script))
            })
            .collect()
    }
}

/// Why `action` cannot be armed: an order of no size, or a stop or gain
/// below zero, which would read as a leg and place none.
pub(crate) fn action_refusal(action: SignalAction) -> Result<(), &'static str> {
    let SignalAction::Order(order) = action else {
        return Ok(());
    };
    if let OrderQuantity::Fixed(quantity) = order.quantity
        && quantity <= Decimal::ZERO
    {
        return Err("an order's fixed quantity must be above zero");
    }
    if order.stop < Decimal::ZERO || order.gain < Decimal::ZERO {
        return Err("stop and gain are distances from the entry: zero for no leg, never below");
    }
    Ok(())
}

/// The account a replayed copy of a script judges against: always flat, and
/// it takes nothing. The copies arm [`SignalAction::Alarm`], so nothing is
/// ever offered.
pub(crate) struct NoAccount;

impl StrategyPort for NoAccount {
    fn apply(&mut self, _command: Command) -> Vec<VenueEvent> {
        Vec::new()
    }

    fn is_flat(&self) -> bool {
        true
    }
}

fn side_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}
