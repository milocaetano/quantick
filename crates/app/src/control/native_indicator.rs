//! Dock the native handler against the window's existing layout port.
use super::{actions::ActionRegistry, gateway::ControlAccess};
use crate::app::LayoutPort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::native_indicator as wire;
use serde_json::Value;
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(wire::edit_descriptor(), edit)?;
    registry.register(wire::list_descriptor(), list)
}
fn edit<P: LayoutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    quantick_control_handlers::native_indicator::edit_request(
        &mut app.layout_adapter(),
        access,
        actor,
        input,
    )
}
fn list<P: LayoutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    quantick_control_handlers::native_indicator::list_request(&app.layout_adapter(), input)
}
