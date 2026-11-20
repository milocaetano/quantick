//! The Pine scripts a tab arms alarm-only on its panes, in plain words and
//! numbers.
//!
//! The kernel types live in `quantick-script-trigger` and
//! `quantick-strategy`, which the control crates may not reach
//! (`guards/src/graph.rs`); a call arms, lists and disarms through here, as
//! [`crate::strategy_reading`] reads the drawn instances. Each instance rides
//! one pane — the bars it judges are that pane's closes — and is evaluated
//! with the pane's other strategies ([`TabStrategies::evaluate`]).

use quantick_engine::{Bar, Side};
pub use quantick_script_trigger::ScriptPlacement;
use quantick_script_trigger::{Script, ScriptInstance, ScriptState, WARMUP_BARS};
use quantick_strategy::DisarmReason;
use rust_decimal::Decimal;

use crate::strategies::TabStrategies;

/// One arm: which script, read by the caller from its library, on which
/// pane, hunting which side, where.
#[derive(Debug, Clone, Copy)]
pub struct ScriptArm<'a, D> {
    pub pane_id: u64,
    /// The script's file name in the library.
    pub script: &'a str,
    pub source: &'a str,
    pub side: Side,
    pub place: ScriptPlacement<D>,
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
            disarm_reason: match instance.state() {
                ScriptState::Watching => None,
                ScriptState::Disarmed { reason } => Some(reason),
            },
            status: instance.armed().status_line(),
        }
    }
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// Compile `request.source` and arm it alarm-only on `request.pane_id`,
    /// warmed on `warmup(depth)` — closed bars with their footprint POC,
    /// oldest first. A script that fails while warming is armed already
    /// disarmed, and its reading says why.
    ///
    /// # Errors
    ///
    /// The sentence the caller reads: a script that does not compile, an
    /// alert title that names no side, or none naming `request.side`.
    pub fn arm_script(
        &mut self,
        request: ScriptArm<'_, D>,
        warmup: impl FnOnce(usize) -> Vec<(Bar, Option<Decimal>)>,
    ) -> Result<ScriptReading<D>, String> {
        let name = request.script;
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
        let id = self.next_script_id;
        self.next_script_id += 1;
        let warm = warmup(WARMUP_BARS);
        let instance = ScriptInstance::arm(id, &script, request.side, request.place, &warm);
        let reading = ScriptReading::of(request.pane_id, &instance);
        self.pane_mut(request.pane_id).scripts.push(instance);
        Ok(reading)
    }

    /// Disarm and remove script instance `id`, answering it as it stood once
    /// disarmed; `None` when no pane holds it. Alarm-only, so nothing is
    /// owed on the way out.
    pub fn disarm_script(&mut self, id: u64) -> Option<ScriptReading<D>> {
        self.panes_mut().find_map(|(pane_id, pane)| {
            let at = pane.scripts.iter().position(|script| script.id == id)?;
            let mut instance = pane.scripts.remove(at);
            instance.disarm(DisarmReason::User);
            Some(ScriptReading::of(pane_id, &instance))
        })
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

fn side_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}
