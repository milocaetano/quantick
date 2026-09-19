//! Durable per-indicator price-hover guide, shared by UI and automation.

use crate::app::LayoutPort;
use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, EventKind, ModuleId,
        PermissionId, RiskFlagId,
    },
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::indicator_worker::SlotId;

use super::{
    actions::{ActionRegistry, CAPABILITY_VERSION, NO_CONFIRMATION_ID, UI_BOUNDED_COST_ID},
    contract::{COCKPIT_EFFECT_ID, COCKPIT_LAYOUT_PERMISSION_ID, COCKPIT_PERMISSION_ID},
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    script::SCRIPT_MODULE_ID,
};

pub(crate) const INDICATOR_GUIDE_CAPABILITY_ID: &str = "indicator.mouse_vertical_line.set";
pub(crate) const INDICATOR_GUIDE_EVENT_KIND: &str = "indicator.mouse_vertical_line.changed";

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndicatorGuideInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub slot_id: WireU64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct IndicatorGuideResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub slot_id: WireU64,
    pub enabled: bool,
}

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        CapabilityDescriptor {
            id: CapabilityId::new(INDICATOR_GUIDE_CAPABILITY_ID).expect("static capability ID is valid"),
            version: CAPABILITY_VERSION,
            title: "Set an indicator's mouse vertical line".to_owned(),
            description: "Shows or hides a subtle dashed vertical line in one non-price indicator pane at the x coordinate hovered in its price chart. The choice is stored in the pane's workspace layout.".to_owned(),
            module: ModuleId::new(SCRIPT_MODULE_ID).expect("static module ID is valid"),
            input_schema: generated_schema::<IndicatorGuideInput>(),
            output_schema: generated_schema::<IndicatorGuideResult>(),
            examples: Vec::new(),
            effect: EffectId::new(COCKPIT_EFFECT_ID).expect("static effect ID is valid"),
            risk_flags: BTreeSet::<RiskFlagId>::new(),
            read_only: false,
            idempotency: IdempotencyPolicy::Optional,
            revision_policy: RevisionPolicy::OptionalForAdditive,
            stale_input_safety: Some("The call names the exact tab, pane and live indicator slot; repeating the same state is harmless and the result reads it back.".to_owned()),
            dry_run_supported: false,
            persistence: EffectPersistence::Durable,
            reversible: true,
            destructive: false,
            risk_reducing: false,
            required_permissions: [COCKPIT_PERMISSION_ID, COCKPIT_LAYOUT_PERMISSION_ID]
                .into_iter()
                .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
                .collect(),
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

fn set<P: LayoutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: IndicatorGuideInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let changed = crate::app::set_indicator_mouse_vertical_line(
        &mut app.layout_adapter(),
        input.tab_id.get(),
        input.pane_id.get(),
        SlotId(input.slot_id.get()),
        input.enabled,
    );
    if !changed {
        return Err(ControlError::invalid_request(
            "the target must name a live non-price indicator reported by analysis.indicators",
        ));
    }
    let result = IndicatorGuideResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        slot_id: input.slot_id,
        enabled: input.enabled,
    };
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(SCRIPT_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(INDICATOR_GUIDE_EVENT_KIND).expect("static event kind is valid"),
            actor: Some(EventActor {
                kind: actor.actor_kind,
                client_name: actor.client_name.clone(),
            }),
            payload: serde_json::json!({ "indicator_guide": result }),
        },
        crate::metrics::wall_clock_ms(),
    );
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}
