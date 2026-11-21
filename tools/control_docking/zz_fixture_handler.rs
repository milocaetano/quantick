//! A real mutating handler docked through the disposable manifest row.
use crate::{
    JournalAccess,
    dock::{ActionDock, ProjectionDock},
    drawing_magnet::RailMagnetPort,
};
use quantick_control::{
    error::ControlError,
    id::ModuleId,
    registry::{CapabilityDescriptor, ModuleDescriptor, RegistryError},
    wire::ActorContext,
};
use quantick_control_host::{
    catalogue::SnapshotScopeDescriptor, journal::NewEvent, projection::ProjectionRegistryError,
};
use quantick_control_schema::zz_fixture as wire;
use serde_json::Value;

pub fn register_actions<D: ActionDock<H, A>, H: RailMagnetPort + ?Sized, A: JournalAccess>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(wire::descriptor(), call)
}
fn call<H: RailMagnetPort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: wire::Input = serde_json::from_value(value.clone()).map_err(crate::invalid)?;
    let result = wire::Output {
        enabled: input.enabled,
        changed: host.switch_rail_magnet(input.enabled),
    };
    let result = serde_json::to_value(result).map_err(crate::invalid)?;
    access.record_event(NewEvent::by(
        "fixture",
        "fixture.changed",
        actor,
        result.clone(),
    ));
    Ok(result)
}
pub(crate) fn action_descriptors() -> Vec<CapabilityDescriptor> {
    vec![wire::descriptor()]
}
pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new("fixture").unwrap(),
        title: "Fixture".into(),
        description: "Synthetic family.".into(),
    }
}
fn scope() -> crate::dock::ScopeSpec<bool> {
    crate::dock::ScopeSpec::new(
        "fixture.available",
        "fixture",
        1,
        "Fixture",
        "Fixture availability.",
        &["observe"],
    )
}
pub(crate) fn scope_descriptors() -> Vec<SnapshotScopeDescriptor> {
    vec![scope().descriptor()]
}
pub fn register<D: ProjectionDock<H>, H: ?Sized + 'static>(
    registry: &mut D,
) -> Result<(), ProjectionRegistryError> {
    registry.register_module(module_descriptor(), |_| 0_u64)?;
    scope().register(registry, |_, _| true)
}

#[cfg(test)]
mod tests;
