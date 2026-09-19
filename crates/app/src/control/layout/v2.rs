//! Version 2 of the eight layout calls that answer with the arrangement: the
//! same acts, with the context column's share carried as an exact decimal.
//!
//! Version 1 declares `fraction` as a JSON number, and the control wire
//! refuses floating-point JSON in both directions (`quantick_control::codec`).
//! So a v1 `layout.pane.resize` can only be sent the integers 0 and 1, and
//! every v1 call that answers a `LayoutResult` — `layout.focus.set`,
//! `layout.pane.collapse`, `layout.pane.expand`, `layout.pane.move`,
//! `layout.preset.apply`, `layout.pane.set_interval`,
//! `layout.pane.set_bar_spec` and a successful `layout.pane.resize` — acts, and then the gateway cannot encode the answer
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

use crate::app::{LayoutPort, TabsMutPort, TabsPort};
pub(crate) use quantick_control_schema::layout_v2::*;

use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};

use rust_decimal::{Decimal, prelude::ToPrimitive};

use serde_json::Value;

use super::super::{actions::ActionRegistry, gateway::ControlAccess};

// Decimal places `fraction` is written and accepted with: `workspace.summary`'s
// own, so the answer and the readback are one number written one way.
use super::super::workspace::SPLIT_FRACTION_DECIMAL_PLACES as FRACTION_DECIMAL_PLACES;

use super::{
    APPLY_PRESET_CAPABILITY_ID, BAR_SPEC_CAPABILITY_ID, COLLAPSE_CAPABILITY_ID,
    EXPAND_CAPABILITY_ID, FOCUS_CAPABILITY_ID, INTERVAL_CAPABILITY_ID, MOVE_PANE_CAPABILITY_ID,
    RESIZE_CAPABILITY_ID,
};

/// The eight calls, each with the v2 handler that answers for it.
const CALLS: [(&str, super::super::actions::ActionHandler); 8] = [
    (APPLY_PRESET_CAPABILITY_ID, apply_preset),
    (MOVE_PANE_CAPABILITY_ID, move_pane),
    (RESIZE_CAPABILITY_ID, resize),
    (COLLAPSE_CAPABILITY_ID, collapse),
    (EXPAND_CAPABILITY_ID, expand),
    (FOCUS_CAPABILITY_ID, focus),
    (INTERVAL_CAPABILITY_ID, set_interval),
    (BAR_SPEC_CAPABILITY_ID, set_bar_spec),
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

fn apply_preset<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::apply_preset(app, access, actor, input)?)
}

fn move_pane<P: TabsPort + LayoutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::move_pane(app, access, actor, input)?)
}

fn collapse<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::collapse(app, access, actor, input)?)
}

fn expand<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::expand(app, access, actor, input)?)
}

fn focus<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::focus(app, access, actor, input)?)
}

fn set_interval<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::set_interval(app, access, actor, input)?)
}

fn set_bar_spec<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    exact(super::set_bar_spec(app, access, actor, input)?)
}

/// Resize, with the share read as an exact decimal and handed to the v1 body
/// as the number it has always taken — in process, where no wire is crossed.
fn resize<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
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
    // The v1 body clamps to 0..1 and only its canvas-width floor would refuse
    // an out-of-range share — a floor it skips on a tab not drawn yet. v1
    // could never carry anything but 0 or 1; v2 can, so it refuses here what
    // clamping would otherwise quietly rewrite into a different share.
    if fraction < Decimal::ZERO || fraction > Decimal::ONE {
        return Err(ControlError::invalid_request(format!(
            "a share is between 0 and 1; {fraction} is not"
        )));
    }
    // The width floor a drag is held to is checked against the canvas the tab
    // last drew, and a tab not drawn yet — a background tab after a restore —
    // has none. v1 could only send 0 or 1 there; v2 can send any share, and
    // one under the floor would be stored as a width no drag can reach, which
    // the trader's next nudge then collapses. So v2 refuses until the tab has
    // been shown.
    let index = super::tab_index(app, input.target)?;
    let drawn = app
        .tab_reads()
        .tab_at(index)
        .is_some_and(|tab| tab.last_canvas_width() > 0.0);
    if !drawn {
        return Err(ControlError::invalid_request(
            "this tab has not been drawn yet, so no share can be held to the floor a drag is held to; resize it once the tab is shown",
        ));
    }
    let share = fraction
        .to_f64()
        .ok_or_else(|| ControlError::invalid_request("fraction is out of range"))?;
    let mut v1_input = serde_json::to_value(input.target)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    v1_input["fraction"] = Value::from(share);
    exact(super::resize(app, access, actor, &v1_input)?)
}
