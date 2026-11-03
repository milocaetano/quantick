//! Durable per-indicator price-hover guide, shared by UI and automation.

use std::collections::BTreeSet;

use quantick_control::wire::WireU64;
use quantick_control::{
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, ModuleId, PermissionId,
        RiskFlagId,
    },
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
};
use quantick_control_host::authority::{
    CAPABILITY_VERSION, COCKPIT_EFFECT_ID, COCKPIT_LAYOUT_PERMISSION_ID, COCKPIT_PERMISSION_ID,
    NO_CONFIRMATION_ID, SCRIPT_MODULE_ID, UI_BOUNDED_COST_ID,
};

use schemars::JsonSchema;

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::registry::IdempotencyPolicy::Optional;
use serde::{Deserialize, Serialize};

pub const INDICATOR_GUIDE_CAPABILITY_ID: &str = "indicator.mouse_vertical_line.set";

pub const INDICATOR_GUIDE_EVENT_KIND: &str = "indicator.mouse_vertical_line.changed";

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndicatorGuideInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub slot_id: WireU64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct IndicatorGuideResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub slot_id: WireU64,
    pub enabled: bool,
}

/// How a client reconciles an interrupted call that moves the guide line.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[journal(
    "indicator.mouse_vertical_line.set",
    Optional,
    INDICATOR_GUIDE_EVENT_KIND,
    "payload.indicator_guide.enabled",
    "an event after the pre-call cursor carries the requested boolean and its payload names the exact tab, pane and slot",
    &[EVERY_OPTIONAL_TEST],
)];

/// The capability as the registry lists it.
#[must_use]
pub fn descriptor() -> CapabilityDescriptor {
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
        }
}
