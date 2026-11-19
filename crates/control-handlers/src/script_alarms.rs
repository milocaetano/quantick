//! `alerts.script.history` and the `alerts.script.fired` journal event: the
//! alarms indicator scripts raised, read from the alert runtime that keeps
//! them and journaled as the window delivers them.
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::alerts::MODULE_ID;
use quantick_control_schema::script_alarms::{
    self as wire, FIRED_EVENT_KIND, History, ScriptAlarm,
};
use quantick_stores::script_alarms::{ScriptAlarmEntry, ScriptAlarmLog};
use serde_json::Value;

use crate::JournalAccess;
use crate::alerts::AlertDefinitionsPort;
use crate::dock::ActionDock;
use crate::workspace_book::Journal;

/// # Errors
/// The capability is already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: AlertDefinitionsPort + ?Sized,
{
    registry.register(wire::descriptor(), request)
}

fn request<H: AlertDefinitionsPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: wire::Input = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    serde_json::to_value(history(&host.alert_runtime().script_alarms, &input))
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn history(log: &ScriptAlarmLog, input: &wire::Input) -> History {
    History {
        alarms: log
            .after(input.after.map(WireU64::get))
            .map(ScriptAlarm::from)
            .collect(),
        last_sequence: WireU64::new(log.last_sequence()),
        oldest_sequence: WireU64::new(log.oldest_sequence()),
    }
}

/// Journal each delivered alarm as `alerts.script.fired` under the actor
/// `journal` names (a window with no journal journals nothing), and answer
/// one toast for them all, so a burst does not bury the chart.
pub fn journal<A: JournalAccess>(
    journal: Journal<'_, A>,
    delivered: &[ScriptAlarmEntry],
) -> Vec<String> {
    if let Some((access, actor)) = journal {
        for entry in delivered {
            let payload =
                serde_json::to_value(ScriptAlarm::from(entry)).expect("a script alarm serializes");
            access.record_event(NewEvent::by(MODULE_ID, FIRED_EVENT_KIND, actor, payload));
        }
    }
    match delivered {
        [] => Vec::new(),
        [one] => vec![one.notice()],
        [first, rest @ ..] => vec![format!("{} (+{} more)", first.notice(), rest.len())],
    }
}

#[cfg(test)]
#[path = "script_alarms_tests.rs"]
mod tests;
