//! Save drawing uses the same addressed owner as the chart's native checkbox.
use crate::wire::{
    chart::MODULE_ID,
    drawing_autosave::{self as wire, Input, Output},
};
use crate::{JournalAccess, chart_settings::ChartStylePort, dock::ActionDock, invalid};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use serde_json::{Value, json};

/// The addressed pane is valid, but its scope or persistence is not ready.
pub fn unavailable(reason: String) -> ControlError {
    let code = quantick_control::id::ErrorCode::new(
        quantick_control::error::codes::CAPABILITY_UNAVAILABLE,
    )
    .expect("static error code");
    let mut error = ControlError::new(code, reason, true);
    error.context.details = Some(json!({ "reason": "drawing_autosave_not_written" }));
    error
}

pub fn register<D: ActionDock<H, A>, H: ChartStylePort + ?Sized, A: JournalAccess>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(wire::descriptor(), set)
}

fn set<H: ChartStylePort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: Input = serde_json::from_value(input.clone()).map_err(invalid)?;
    let (on, changed) = host.drawing_autosave(input.tab_id.get(), input.pane_id.get(), input.on)?;
    let result = Output {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        on,
        changed,
    };
    if changed {
        access.record_event(NewEvent::by(
            MODULE_ID,
            wire::EVENT_KIND,
            actor,
            json!({ "drawing_autosave": result }),
        ));
    }
    serde_json::to_value(result).map_err(invalid)
}

#[cfg(test)]
#[path = "drawing_autosave_tests.rs"]
mod tests;
