//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation.

use std::collections::BTreeSet;

use schemars::JsonSchema;

use crate::analysis::DRAWINGS_SCOPE_ID;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal, snapshot};
use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control::error::ControlError;
use quantick_control::id::PermissionId;
use quantick_control::registry::IdempotencyPolicy::Optional;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence};
use quantick_control::wire::WireU64;
use quantick_control_host::authority::COCKPIT_PERMISSION_ID;
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
    /// The tab holding the drawing, as `analysis.drawings` lists it.
    pub tab_id: WireU64,
    /// The pane holding the drawing on that tab, as `analysis.drawings`
    /// lists it. A drawing id is unique only within its pane.
    pub pane_id: WireU64,
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

impl ObjectMagnetInput {
    /// What the call answers once the drawing's magnet stands as asked.
    pub fn result(&self, changed: bool) -> ObjectMagnetResult {
        ObjectMagnetResult {
            drawing_id: self.drawing_id,
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            enabled: self.enabled,
            changed,
        }
    }

    /// The refusal for an id the named pane does not hold.
    pub fn unknown_drawing(&self) -> ControlError {
        ControlError::invalid_request(format!(
            "pane {} on tab {} holds no drawing with id {}",
            self.pane_id.get(),
            self.tab_id.get(),
            self.drawing_id.get()
        ))
    }
}

/// The refusal for a tool that snaps by its own rule: the context bar shows
/// such a tool no magnet, and the call refuses what the bar would not offer,
/// with a reason a client can branch on.
pub fn no_magnet(tool_name: &str, tool_id: &str) -> ControlError {
    let mut error = ControlError::invalid_request(format!(
        "the {tool_name} snaps by its own rule and has no magnet"
    ));
    error.context.details = Some(serde_json::json!({
        "reason": "tool_snaps_by_its_own_rule",
        "tool_id": tool_id,
    }));
    error
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
        "Turns one drawing's own OHLC magnet on or off, named by the `tab_id`, `pane_id` and `drawing_id` analysis.drawings lists (a drawing id is unique only within its pane): with it on, that drawing's re-dragged or body-dragged anchors land on the open, high, low or close of the candle under the pointer even while the rail's magnet (`annotate.magnet.set`) is off. The same switch as the magnet slot on the drawing's context bar; it is saved with the workspace, and analysis.drawings reads it back as the drawing's `magnet`. Only a tool whose anchors follow the pointer has one: turning it on for a mark that snaps by its own rule, like an arrow mark or an anchored VWAP, is refused with control.invalid_request, while turning it off is accepted on any tool so a stale flag can be cleared (`changed` reports the stored flag, and analysis.drawings reads such a tool's `magnet` as false). An id no pane on that tab holds is refused too.",
        "The call names the state it wants rather than flipping it; repeating it is harmless and the result reads the drawing's magnet back.",
        EffectPersistence::Durable,
    )
}

/// The two magnet calls share the cockpit's transient descriptor and differ
/// only in what they set and whether it is saved. Neither rearranges a
/// canvas, so neither asks for the layout permission that descriptor names.
fn descriptor<Input: JsonSchema, Output: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
    stale_input_safety: &str,
    persistence: EffectPersistence,
) -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<Input, Output>(
        id,
        ANNOTATE_MODULE_ID,
        title,
        description,
        stale_input_safety,
    );
    descriptor.persistence = persistence;
    descriptor.required_permissions =
        BTreeSet::from([PermissionId::new(COCKPIT_PERMISSION_ID).expect("static permission ID")]);
    descriptor
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
        "the row under the named `tab_id` and `pane_id` whose `drawing_id` was named carries the requested boolean",
        &[EVERY_OPTIONAL_TEST],
    ),
];
