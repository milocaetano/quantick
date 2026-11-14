//! Native mutation validation and dispatch over the existing indicator owner.
use quantick_control_schema::native_indicator as wire;
use quantick_stores::indicator_state::SavedIndicator;
pub trait NativePort {
    fn saved(&self, target: &wire::Target) -> Result<Vec<(u64, SavedIndicator, bool)>, String>;
    fn settings_open(&self) -> bool;
    fn apply(
        &mut self,
        target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, String>;
}
pub fn list(host: &impl NativePort, input: wire::ListInput) -> Result<wire::ListResult, String> {
    let saved = host.saved(&input.target)?;
    wire::capture(input, saved)
}
pub fn edit(
    host: &mut impl NativePort,
    input: wire::EditInput,
) -> Result<wire::EditResult, String> {
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

/// Transport parsing and event emission stay with the headless handler.
pub fn edit_request(
    host: &mut impl NativePort,
    access: &mut impl crate::tabs::JournalAccess,
    actor: &quantick_control::wire::ActorContext,
    input: &serde_json::Value,
) -> Result<serde_json::Value, quantick_control::error::ControlError> {
    use quantick_control::error::ControlError;
    let input = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = edit(host, input).map_err(ControlError::invalid_request)?;
    access.record_event(quantick_control_host::journal::NewEvent::by(
        "indicator",
        wire::EVENT,
        actor,
        serde_json::json!({"native_indicator":result}),
    ));
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}
pub fn list_request(
    host: &impl NativePort,
    input: &serde_json::Value,
) -> Result<serde_json::Value, quantick_control::error::ControlError> {
    use quantick_control::error::ControlError;
    let input = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = list(host, input).map_err(ControlError::invalid_request)?;
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}
