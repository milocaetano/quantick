//! An indicator written in conversation: compile Quantick Pine, read the
//! diagnostics as data, attach what compiled, detach it again.
//!
//! `quantick_pine::compile` returns `Vec<PineError>` with a stable code, a
//! byte span, a message and notes, and the indicator host is headless. This
//! family refuses to render them into a string first: an agent that has to
//! parse "line 4, column 12: …" back out of prose cannot fix its own script
//! reliably.
use crate::annotate::AnnotateAccess;
use crate::indicator_host::IndicatorControlHost;
use crate::{dock::ActionDock, tabs::JournalAccess};
use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, ActorKind, WireU64},
};
use quantick_control_host::{authority::SCRIPT_MODULE_ID, journal::NewEvent};
use quantick_control_schema::script::{
    AttachInput, AttachResult, DetachInput, DetachResult, SCRIPT_ATTACHED_EVENT_KIND,
    SCRIPT_DETACHED_EVENT_KIND, attach_descriptor, declared_inputs, detach_descriptor,
};
use serde::Serialize;
use serde_json::{Value, json};

/// Dock the script attach and detach.
pub fn register<H, A>(registry: &mut impl ActionDock<H, A>) -> Result<(), RegistryError>
where
    H: IndicatorControlHost + ?Sized,
    A: JournalAccess + AnnotateAccess,
{
    registry.register(attach_descriptor(), attach)?;
    registry.register(detach_descriptor(), detach)
}

fn attach<H, A>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError>
where
    H: IndicatorControlHost + ?Sized,
    A: JournalAccess + AnnotateAccess,
{
    let input: AttachInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    // Compile before touching the chart: the diagnostics are the point of
    // this capability, and a script that cannot run never becomes a slot the
    // trader has to clean up.
    let declared_inputs = declared_inputs(&input.source, &input.name)?;
    // Attached *by an operator* when it was not the trader's own hand, which
    // is what the detach then checks before it removes anything. A rerun asks
    // the recorded run whose hand it was, exactly as an annotation does:
    // replaying a script the trader attached by hand as automation's would
    // hand this tier a detach on the trader's own indicator.
    let attached_by = access
        .recorded_actor()
        .map_or(actor.actor_kind, |(kind, _)| kind);
    let by_operator = attached_by != ActorKind::HumanUi;
    let attached = host
        .indicators()
        .attach_script(input.name.clone(), input.source, by_operator);
    let result = AttachResult {
        slot_id: WireU64::new(attached.slot),
        tab_id: WireU64::new(attached.tab_id),
        pane_side: attached.pane_side,
        name: input.name,
        declared_inputs,
    };
    journal(access, actor, SCRIPT_ATTACHED_EVENT_KIND, &result)?;
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("attach result: {error}")))
}

fn detach<H, A>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError>
where
    H: IndicatorControlHost + ?Sized,
    A: JournalAccess + AnnotateAccess,
{
    let input: DetachInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let detached = host
        .indicators()
        .detach_operator_script(input.slot_id.get())?;
    let result = DetachResult {
        slot_id: input.slot_id,
        detached,
    };
    if detached {
        journal(access, actor, SCRIPT_DETACHED_EVENT_KIND, &result)?;
    }
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("detach result: {error}")))
}

fn journal<T: Serialize>(
    access: &mut impl JournalAccess,
    actor: &ActorContext,
    kind: &str,
    payload: &T,
) -> Result<(), ControlError> {
    let payload = serde_json::to_value(payload)
        .map_err(|error| ControlError::invalid_request(format!("script event: {error}")))?;
    let payload = json!({ "script": payload });
    JournalAccess::record_event(access, NewEvent::by(SCRIPT_MODULE_ID, kind, actor, payload));
    Ok(())
}

#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
