//! A synthetic mutation used only in the disposable docking fixture.
pub use crate::drawing_magnet::{DrawingMagnetInput as Input, DrawingMagnetResult as Output};
use quantick_control::{
    id::{CapabilityId, ModuleId, PermissionId},
    registry::{CapabilityDescriptor, IdempotencyPolicy},
};
pub const FIXTURE_CAPABILITY_ID: &str = "fixture.magnet.set";
pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::journal(
    FIXTURE_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    "fixture.changed",
    "payload.enabled",
    "the event reports the requested magnet state",
    &["fake_mutating_family_reaches_every_manifest_consumer"],
)];
pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::drawing_magnet::rail_descriptor();
    descriptor.id = CapabilityId::new(FIXTURE_CAPABILITY_ID).unwrap();
    descriptor.module = ModuleId::new("fixture").unwrap();
    descriptor.title = "Fixture mutation".into();
    descriptor.description = "Set the fixture magnet and journal the result.".into();
    descriptor
        .required_permissions
        .insert(PermissionId::new("cockpit.fixture").unwrap());
    descriptor
}
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "fixture-input-v1.schema.json",
            quantick_control::schema::generated_schema::<Input>(),
        ),
        (
            "fixture-output-v1.schema.json",
            quantick_control::schema::generated_schema::<Output>(),
        ),
    ]
}
pub fn behaviour_is_docked() -> bool {
    quantick_operability::registry::UI_BEHAVIOURS
        .iter()
        .any(|row| row.id == "fixture.magnet")
}
