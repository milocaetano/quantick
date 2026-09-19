//! Existing registered script actions: authority and shell composition.
use super::ControlWindow;
use crate::control::{ControlAccess, script::*};
use quantick_control::{
    error::{ControlError, codes},
    wire::{ActorContext, WireU64},
};
use serde_json::Value;

pub(crate) fn attach_script(
    app: &mut ControlWindow,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: AttachInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    // Compile before touching the chart: the diagnostics are the point of
    // this capability, and a script that cannot run never becomes a slot the
    // trader has to clean up.
    let compiled = quantick_pine::compile(&input.source, &input.name)
        .map_err(|errors| compile_error(&errors, &input.source))?;
    let declared_inputs = compiled
        .inputs
        .iter()
        .map(|spec| spec.name().to_owned())
        .collect::<Vec<_>>();
    // Attached *by an operator* when it was not the trader's own hand, which
    // is what the detach then checks before it removes anything. A rerun asks
    // the recorded run whose hand it was, exactly as an annotation does:
    // replaying a script the trader attached by hand as automation's would
    // hand this tier a detach on the trader's own indicator.
    let attached_by = access
        .recorded_author()
        .map_or(actor.actor_kind, |recorded| recorded.actor_kind);
    let by_operator = attached_by != quantick_control::wire::ActorKind::HumanUi;
    let (tab_id, pane_side, slot) =
        app.attach_script(input.name.clone(), input.source, by_operator);
    let pane_side = pane_side.into();
    let result = AttachResult {
        slot_id: WireU64::new(slot.0),
        tab_id: WireU64::new(tab_id),
        pane_side,
        name: input.name,
        declared_inputs,
    };
    journal_script(access, actor, SCRIPT_ATTACHED_EVENT_KIND, &result)?;
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("attach result: {error}")))
}

pub(crate) fn detach_script(
    app: &mut ControlWindow,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DetachInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let detached = app.detach_operator_script(input.slot_id.get()).map_err(|()| {
        known_error(
            codes::PERMISSION_DENIED,
            "that indicator is the trader's own; this tier detaches only what an operator attached",
            false,
        )
    })?;
    let result = DetachResult {
        slot_id: input.slot_id,
        detached,
    };
    if detached {
        journal_script(access, actor, SCRIPT_DETACHED_EVENT_KIND, &result)?;
    }
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("detach result: {error}")))
}
