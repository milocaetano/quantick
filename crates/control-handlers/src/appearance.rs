//! Appearance decisions and errors over a headless store port.
use crate::dock::{ActionDock, ProjectionDock};
use quantick_control::{
    error::ControlError,
    id::{ModuleId, SnapshotScopeId},
    registry::{ModuleDescriptor, RegistryError},
    wire::{ActorContext, WireU64},
};
use quantick_control_host::projection::{CaptureContext, ProjectionRegistryError};
use quantick_control_schema::appearance::{
    AppearanceSnapshot, ApplyInput, ApplyResult, MODULE_ID, SCOPE_ID, descriptor,
};
use quantick_stores::appearance::{AppearanceError, AppearanceStore};
use serde_json::{Value, json};

pub trait AppearancePort {
    fn appearance_store(&self) -> &AppearanceStore;
    fn appearance_store_mut(&mut self) -> &mut AppearanceStore;
    /// Synchronize all rendering/default owners before another command can observe them.
    fn publish_appearance(&mut self);
}

pub fn register_actions<D: ActionDock<H, A>, H: AppearancePort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), apply)
}
pub fn register<D: ProjectionDock<H>, H: AppearancePort + ?Sized + 'static>(
    registry: &mut D,
) -> Result<(), ProjectionRegistryError> {
    let module = ModuleId::new(MODULE_ID).expect("static module");
    registry.register_module(
        ModuleDescriptor {
            id: module.clone(),
            title: "Appearance".into(),
            description: "Active configuration and persistence status.".into(),
        },
        revision::<H>,
    )?;
    registry.register_scope(SnapshotScopeId::new(SCOPE_ID).expect("static scope"), module, 1,
        "Appearance state", "Bounded active TOML, paint revision, source and last-known persistence status; no filesystem paths.",
        &["observe", "observe.workspace"], project::<H>)
}
fn revision<H: AppearancePort + ?Sized>(host: &H) -> (u64, bool) {
    let store = host.appearance_store();
    (store.inspection_revision(), store.writes_enabled())
}
fn project<H: AppearancePort + ?Sized>(host: &H, _: CaptureContext) -> AppearanceSnapshot {
    snapshot(host)
}
pub fn snapshot<H: AppearancePort + ?Sized>(host: &H) -> AppearanceSnapshot {
    let store = host.appearance_store();
    AppearanceSnapshot {
        revision: WireU64::new(store.revision()),
        origin: store.origin().into(),
        source: store
            .source()
            .unwrap_or(quantick_stores::appearance::DEFAULT_SOURCE)
            .into(),
        durable: store.durable(),
        writes_enabled: store.writes_enabled(),
        load_failed: store.load_failed(),
    }
}
fn apply<H: AppearancePort + ?Sized, A>(
    host: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: ApplyInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let before = host.appearance_store().revision();
    host.appearance_store_mut()
        .apply_candidate(&input.source, input.persist)
        .map_err(refusal)?;
    host.publish_appearance();
    serde_json::to_value(ApplyResult {
        changed: host.appearance_store().revision() != before,
        state: snapshot(host),
    })
    .map_err(|error| ControlError::invalid_request(error.to_string()))
}
fn refusal(error: AppearanceError) -> ControlError {
    let (code, field, message) = match error {
        AppearanceError::Invalid(error) => ("invalid_theme", error.field, error.message),
        AppearanceError::Io(error) => (
            "persistence_failed",
            "persist".into(),
            format!(
                "selected appearance store write failed ({:?})",
                error.kind()
            ),
        ),
    };
    let mut error = ControlError::invalid_request(message);
    error.context.details = Some(json!({"code": code, "field": field}));
    error
}
