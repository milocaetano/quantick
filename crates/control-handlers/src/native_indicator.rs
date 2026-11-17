//! Native mutation validation and dispatch over the existing indicator owner.
use crate::indicator_host::{IndicatorHost, IndicatorRefusal, NativePort};
use crate::{dock::ActionDock, tabs::JournalAccess};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::native_indicator as wire;
use serde_json::Value;

/// Dock the native indicator edit and list.
pub fn register<H: IndicatorHost + ?Sized, A: JournalAccess>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register(wire::edit_descriptor(), edit_request)?;
    registry.register(wire::list_descriptor(), list_request)
}

/// The page of `input.target`'s native indicators.
///
/// # Errors
///
/// The target or the page is refused.
pub fn list(
    host: &mut (impl NativePort + ?Sized),
    input: wire::ListInput,
) -> Result<wire::ListResult, IndicatorRefusal> {
    let saved = host.saved(&input.target)?;
    Ok(wire::capture(input, saved)?)
}

/// Validate and apply one native edit.
///
/// # Errors
///
/// The edit is refused, by the request schema or the owner.
pub fn edit(
    host: &mut (impl NativePort + ?Sized),
    input: wire::EditInput,
) -> Result<wire::EditResult, IndicatorRefusal> {
    let slot = input.operation.slot();
    let saved = host.saved(&input.target)?;
    let selected = saved
        .into_iter()
        .filter(|(id, _, _)| Some(*id) == slot)
        .collect();
    let state = wire::capture(
        wire::ListInput {
            target: input.target.clone(),
            offset: 0,
            limit: 1,
        },
        selected,
    )?;
    let prepared = wire::prepare(input.operation, state.entries.first(), host.settings_open())?;
    let removed = matches!(prepared, wire::Prepared::Remove);
    let slot = host.apply(&input.target, slot, prepared)?;
    Ok(wire::EditResult {
        target: input.target,
        slot_id: quantick_control::wire::WireU64::new(slot),
        removed,
    })
}

fn edit_request<H: IndicatorHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = edit(&mut *host.indicators(), input)?;
    access.record_event(NewEvent::by(
        "indicator",
        wire::EVENT,
        actor,
        serde_json::json!({"native_indicator":result}),
    ));
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}

fn list_request<H: IndicatorHost + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = list(&mut *host.indicators(), input)?;
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}

#[cfg(test)]
#[path = "native_indicator_tests.rs"]
mod tests;
