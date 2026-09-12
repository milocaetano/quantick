//! Version 2 of the seven layout calls that answer with the arrangement: the
//! same acts, with the context column's share carried as an exact decimal.
//!
//! Version 1 declares `fraction` as a JSON number, and the control wire
//! refuses floating-point JSON in both directions (`quantick_control::codec`).
//! So a v1 `layout.pane.resize` can only be sent the integers 0 and 1, and
//! every v1 call that answers a `LayoutResult` — `layout.focus.set`,
//! `layout.pane.collapse`, `layout.pane.expand`, `layout.pane.move`,
//! `layout.preset.apply`, `layout.pane.set_interval` and a successful
//! `layout.pane.resize` — acts, and then the gateway cannot encode the answer
//! and tells the client `control.capability_unavailable`. The retry matrix's
//! transport tests found it: the call applied, and the client was told it
//! had not.
//!
//! Changing v1's field is a breaking change to a published capability, which
//! the contract answers by bumping the capability's version and keeping the
//! previous one (`docs/control-plane/control-contract.md` §4). So v1 stays
//! registered and unchanged, and this module docks v2 beside it: one more
//! descriptor per call, derived from the v1 descriptor rather than restated,
//! and one handler per call that runs the v1 handler and re-encodes its
//! answer.
//!
//! # Why a decimal string and not integer millionths
//!
//! `CanonicalDecimal` is how this contract already writes an exact decimal:
//! prices, and `workspace.summary`'s own `split_fraction` — the very value
//! this field reports. Integer millionths would be a second encoding of one
//! quantity with a unit nothing else uses, and the answer and its readback
//! could then disagree in format. Both sides here round the stored `f32` to
//! [`FRACTION_DECIMAL_PLACES`] exactly as `workspace.summary` does, and an
//! input is refused past that many places, so any fraction a client sends is
//! the fraction it reads back: an `f32` in 0..1 is accurate to far better
//! than half a unit in the sixth place.

use quantick_control::{
    error::ControlError,
    registry::{CapabilityDescriptor, RegistryError},
    schema::generated_schema,
    wire::{ActorContext, CanonicalDecimal, WireU64},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::QuantickApp;

use super::super::{actions::ActionRegistry, gateway::ControlAccess, types::canonical_f64};
// Decimal places `fraction` is written and accepted with: `workspace.summary`'s
// own, so the answer and the readback are one number written one way.
use super::super::workspace::SPLIT_FRACTION_DECIMAL_PLACES as FRACTION_DECIMAL_PLACES;
use super::{
    APPLY_PRESET_CAPABILITY_ID, COLLAPSE_CAPABILITY_ID, EXPAND_CAPABILITY_ID, FOCUS_CAPABILITY_ID,
    INTERVAL_CAPABILITY_ID, MOVE_PANE_CAPABILITY_ID, RESIZE_CAPABILITY_ID, TabTarget,
};

/// The version this module registers.
pub(crate) const VERSION: u32 = 2;

/// What a v2 layout call answers with: v1's `LayoutResult`, with `fraction`
/// exact.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct LayoutResultV2 {
    /// The tab that changed.
    pub tab_id: WireU64,
    /// The preset the canvas now matches.
    pub preset_id: String,
    /// How many panes it draws.
    pub pane_count: WireU64,
    /// The focused pane's address.
    pub focused_pane: WireU64,
    /// The context column's share of the canvas, 0..1, to six places — the
    /// same number `workspace.summary` reports as `split_fraction`.
    pub fraction: CanonicalDecimal,
    /// Whether the context column is collapsed to its rail.
    pub collapsed: bool,
    /// Whether the call changed anything. `false` is a real answer: applying
    /// the layout that is already showing is a no-op, not a failure.
    pub changed: bool,
}

/// `layout.pane.resize` v2's input: v1's, with `fraction` exact.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub(crate) struct ResizeInputV2 {
    #[serde(flatten)]
    pub target: TabTarget,
    /// The share, 0..1, as an exact decimal with at most six places. Held
    /// inside the same floor a drag is held to.
    pub fraction: CanonicalDecimal,
}

/// The seven calls, each with the v2 handler that answers for it.
const CALLS: [(&str, super::super::actions::ActionHandler); 7] = [
    (APPLY_PRESET_CAPABILITY_ID, apply_preset),
    (MOVE_PANE_CAPABILITY_ID, move_pane),
    (RESIZE_CAPABILITY_ID, resize),
    (COLLAPSE_CAPABILITY_ID, collapse),
    (EXPAND_CAPABILITY_ID, expand),
    (FOCUS_CAPABILITY_ID, focus),
    (INTERVAL_CAPABILITY_ID, set_interval),
];

/// Dock v2 of every call in [`CALLS`]. Runs after v1 is registered: each v2
/// descriptor is the v1 descriptor with the version, the result schema and
/// — for resize — the input schema replaced, so the two cannot drift apart in
/// anything but what v2 changes.
pub(super) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    for (id, handler) in CALLS {
        let v1 = registry
            .lookup(id, super::CAPABILITY_VERSION)
            .map(|action| action.descriptor.clone())
            .ok_or_else(|| RegistryError::Unknown {
                kind: "capability",
                id: id.to_owned(),
            })?;
        registry.register(descriptor(v1), handler)?;
    }
    Ok(())
}

fn descriptor(v1: CapabilityDescriptor) -> CapabilityDescriptor {
    let input_schema = if v1.id.as_str() == RESIZE_CAPABILITY_ID {
        generated_schema::<ResizeInputV2>()
    } else {
        v1.input_schema.clone()
    };
    CapabilityDescriptor {
        version: VERSION,
        description: format!(
            "{} Version 2 carries the context column's share as an exact decimal; version 1's number cannot cross the wire.",
            v1.description
        ),
        input_schema,
        output_schema: generated_schema::<LayoutResultV2>(),
        ..v1
    }
}

/// v1's answer, re-encoded: `fraction` from the number v1 computed to the
/// exact decimal v2 publishes.
fn exact(answer: Value) -> Result<Value, ControlError> {
    let mut answer = answer;
    let fraction = answer
        .get("fraction")
        .and_then(Value::as_f64)
        .and_then(|fraction| canonical_f64(fraction, FRACTION_DECIMAL_PLACES))
        .ok_or_else(|| ControlError::invalid_request("the layout result carries no fraction"))?;
    answer["fraction"] = Value::String(fraction.as_str().to_owned());
    Ok(answer)
}

fn apply_preset(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::apply_preset(app, access, actor, input)?)
}

fn move_pane(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::move_pane(app, access, actor, input)?)
}

fn collapse(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::collapse(app, access, actor, input)?)
}

fn expand(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::expand(app, access, actor, input)?)
}

fn focus(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::focus(app, access, actor, input)?)
}

fn set_interval(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::set_interval(app, access, actor, input)?)
}

/// Resize, with the share read as an exact decimal and handed to the v1 body
/// as the number it has always taken — in process, where no wire is crossed.
fn resize(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizeInputV2 = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let fraction: Decimal = input
        .fraction
        .as_str()
        .parse()
        .map_err(|error| ControlError::invalid_request(format!("fraction: {error}")))?;
    if fraction.scale() > FRACTION_DECIMAL_PLACES {
        return Err(ControlError::invalid_request(format!(
            "a share is written with at most {FRACTION_DECIMAL_PLACES} decimal places, so the \
             one read back is the one sent"
        )));
    }
    let share = fraction
        .to_f64()
        .ok_or_else(|| ControlError::invalid_request("fraction is out of range"))?;
    let mut v1_input = serde_json::to_value(input.target)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    v1_input["fraction"] = Value::from(share);
    exact(super::resize(app, access, actor, &v1_input)?)
}
