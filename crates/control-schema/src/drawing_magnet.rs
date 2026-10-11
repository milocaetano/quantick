//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation.

use std::collections::BTreeSet;

use schemars::JsonSchema;

use crate::analysis::DRAWINGS_SCOPE_ID;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal, snapshot};
use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control::id::{
    CapabilityId, ConfirmationClassId, CostClassId, EffectId, ModuleId, PermissionId, RiskFlagId,
};
use quantick_control::registry::IdempotencyPolicy::Optional;
use quantick_control::registry::{
    Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
    RevisionPolicy,
};
use quantick_control::schema::generated_schema;
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{
    CAPABILITY_VERSION, COCKPIT_EFFECT_ID, COCKPIT_PERMISSION_ID, NO_CONFIRMATION_ID,
    UI_BOUNDED_COST_ID,
};
use serde::{Deserialize, Serialize};

pub const DRAWING_MAGNET_CAPABILITY_ID: &str = "annotate.magnet.set";

pub const DRAWING_MAGNET_EVENT_KIND: &str = "annotate.magnet.changed";

pub const OBJECT_MAGNET_CAPABILITY_ID: &str = "annotate.drawing_magnet.set";

pub const OBJECT_MAGNET_EVENT_KIND: &str = "annotate.drawing_magnet.changed";

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingMagnetInput {
    /// `true` snaps drawing anchors to the open / high / low / close of the
    /// candle under the pointer; `false` leaves them where the pointer is.
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DrawingMagnetResult {
    /// The magnet as it stands after the call.
    pub enabled: bool,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

/// One drawing's own magnet, which snaps that object's drags whether or not
/// the rail's magnet is on.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectMagnetInput {
    /// The drawing, by the `drawing_id` `analysis.drawings` lists.
    pub drawing_id: WireU64,
    /// `true` snaps this drawing's dragged anchors to the open / high / low /
    /// close of the candle under the pointer; `false` leaves it to the rail.
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObjectMagnetResult {
    pub drawing_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The drawing's own magnet as it stands after the call.
    pub enabled: bool,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

/// `annotate.magnet.set`: the drawing rail's magnet.
pub fn rail_descriptor() -> CapabilityDescriptor {
    descriptor::<DrawingMagnetInput, DrawingMagnetResult>(
        DRAWING_MAGNET_CAPABILITY_ID,
        "Set the drawing magnet",
        "Turns the drawing rail's magnet on or off: with it on, anchors placed, re-dragged or carried by a body drag land on the open, high, low or close of the candle under the pointer when the pointer is on or near that candle. The same switch the rail's magnet button and its More menu entry flip; scene.read lists it as `tool_rail.toggle.magnet`.",
        "The call names the state it wants rather than flipping it; repeating it is harmless and the result reads the magnet back.",
        EffectPersistence::Transient,
    )
}

/// `annotate.drawing_magnet.set`: one drawing's own magnet, the switch its
/// context bar flips.
pub fn object_descriptor() -> CapabilityDescriptor {
    descriptor::<ObjectMagnetInput, ObjectMagnetResult>(
        OBJECT_MAGNET_CAPABILITY_ID,
        "Set one drawing's own magnet",
        "Turns one drawing's own OHLC magnet on or off, by the `drawing_id` analysis.drawings lists: with it on, that drawing's re-dragged or body-dragged anchors land on the open, high, low or close of the candle under the pointer even while the rail's magnet (`annotate.magnet.set`) is off. The same switch as the magnet slot on the drawing's context bar; it is saved with the workspace, and analysis.drawings reads it back as the drawing's `magnet`.",
        "The call names the state it wants rather than flipping it; repeating it is harmless and the result reads the drawing's magnet back.",
        EffectPersistence::Durable,
    )
}

/// The two magnet calls differ only in what they set and whether it is saved.
fn descriptor<Input: JsonSchema, Output: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
    stale_input_safety: &str,
    persistence: EffectPersistence,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(ANNOTATE_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<Input>(),
        output_schema: generated_schema::<Output>(),
        examples: Vec::new(),
        effect: EffectId::new(COCKPIT_EFFECT_ID).expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Optional,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(stale_input_safety.to_owned()),
        dry_run_supported: false,
        persistence,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: BTreeSet::from([
            PermissionId::new(COCKPIT_PERMISSION_ID).expect("static permission ID is valid")
        ]),
        preconditions: Vec::new(),
        confirmation_class: ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

/// How a client reconciles an interrupted call that sets a magnet.
pub const READBACKS: &[Readback] = &[
    journal(
        DRAWING_MAGNET_CAPABILITY_ID,
        Optional,
        DRAWING_MAGNET_EVENT_KIND,
        "payload.drawing_magnet.enabled",
        "an event after the pre-call cursor carries the requested boolean",
        &[EVERY_OPTIONAL_TEST],
    ),
    snapshot(
        OBJECT_MAGNET_CAPABILITY_ID,
        Optional,
        DRAWINGS_SCOPE_ID,
        "tabs[].panes[].drawings[].magnet",
        "the row whose `drawing_id` was named carries the requested boolean",
        &[EVERY_OPTIONAL_TEST],
    ),
];
