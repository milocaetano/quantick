//! The drawing rail's OHLC magnet, shared by the rail button and automation.

pub(crate) use quantick_control_schema::drawing_magnet::*;

use crate::app::ToolRailPort;
use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, ModuleId, PermissionId,
        RiskFlagId,
    },
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::ActorContext,
};

use quantick_control::annotation::ANNOTATE_MODULE_ID;
use serde_json::Value;

use super::{
    actions::{ActionRegistry, CAPABILITY_VERSION, NO_CONFIRMATION_ID, UI_BOUNDED_COST_ID},
    contract::{COCKPIT_EFFECT_ID, COCKPIT_PERMISSION_ID},
    gateway::ControlAccess,
    journal::NewEvent,
};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        CapabilityDescriptor {
            id: CapabilityId::new(DRAWING_MAGNET_CAPABILITY_ID).expect("static capability ID is valid"),
            version: CAPABILITY_VERSION,
            title: "Set the drawing magnet".to_owned(),
            description: "Turns the drawing rail's magnet on or off: with it on, anchors placed, re-dragged or carried by a body drag land on the open, high, low or close of the candle under the pointer when the pointer is on or near that candle. The same switch the rail's magnet button and its More menu entry flip; scene.read lists it as `tool_rail.toggle.magnet`.".to_owned(),
            module: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
            input_schema: generated_schema::<DrawingMagnetInput>(),
            output_schema: generated_schema::<DrawingMagnetResult>(),
            examples: Vec::new(),
            effect: EffectId::new(COCKPIT_EFFECT_ID).expect("static effect ID is valid"),
            risk_flags: BTreeSet::<RiskFlagId>::new(),
            read_only: false,
            idempotency: IdempotencyPolicy::Optional,
            revision_policy: RevisionPolicy::OptionalForAdditive,
            stale_input_safety: Some("The call names the state it wants rather than flipping it; repeating it is harmless and the result reads the magnet back.".to_owned()),
            dry_run_supported: false,
            persistence: EffectPersistence::Transient,
            reversible: true,
            destructive: false,
            risk_reducing: false,
            required_permissions: BTreeSet::from([
                PermissionId::new(COCKPIT_PERMISSION_ID).expect("static permission ID is valid"),
            ]),
            preconditions: Vec::new(),
            confirmation_class: ConfirmationClassId::new(NO_CONFIRMATION_ID).expect("static confirmation class is valid"),
            availability: Availability::available(),
            expected_cost: ExpectedCost {
                class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
                max_items: None,
                max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
            },
            pagination: None,
        },
        set,
    )
}

fn set<P: ToolRailPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DrawingMagnetInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let result = DrawingMagnetResult {
        enabled: input.enabled,
        changed: app.set_drawing_magnet(input.enabled),
    };
    let payload = serde_json::json!({ "drawing_magnet": result });
    access.append_event(NewEvent::by(
        ANNOTATE_MODULE_ID,
        DRAWING_MAGNET_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}
