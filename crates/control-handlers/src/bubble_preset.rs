//! Named whole looks over the pane owner's narrow port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_preset::{ApplyInput, ApplyResult, ListInput, ListResult};
use serde_json::Value;

pub trait BubblePresetPort {
    fn apply_bubble_preset(&mut self, input: &ApplyInput) -> Result<ApplyResult, ControlError>;
    fn list_bubble_presets(&mut self, input: &ListInput) -> Result<ListResult, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: BubblePresetPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(
        quantick_control_schema::bubble_preset::apply_descriptor(),
        apply,
    )?;
    registry.register(
        quantick_control_schema::bubble_preset::list_descriptor(),
        list,
    )
}

fn apply<H: BubblePresetPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.apply_bubble_preset(&input)?).map_err(invalid)
}

fn list<H: BubblePresetPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.list_bubble_presets(&input)?).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// The existing chart-only selector retains its independent contract.
pub trait TickBubblePort {
    fn set_tick_bubbles(
        &mut self,
        input: &quantick_control_schema::tick_bubbles::TickBubblesInput,
    ) -> Result<quantick_control_schema::tick_bubbles::TickBubblesResult, ControlError>;
}
pub fn register_tick<D: ActionDock<H, A>, H: TickBubblePort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(quantick_control_schema::tick_bubbles::descriptor(), tick)
}
fn tick<H: TickBubblePort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.set_tick_bubbles(&input)?).map_err(invalid)
}
