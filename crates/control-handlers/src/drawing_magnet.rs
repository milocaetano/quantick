//! The OHLC magnet's two calls: the rail's (`annotate.magnet.set`) and one
//! drawing's own (`annotate.drawing_magnet.set`), decided here beside
//! `drawing_scope`, whose call docks with them. The window only lends the
//! rail's switch ([`RailMagnetPort`]) or the addressed pane
//! ([`AddressedPanePort::addressed_pane`]).

use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::journal::NewEvent;
pub use quantick_control_schema::drawing_magnet::*;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use serde_json::Value;

use crate::JournalAccess;
use crate::axis_pane::AddressedPanePort;
use crate::dock::ActionDock;
use crate::drawing_scope;
use crate::invalid;

/// The drawing rail's magnet switch, owned by the tool rail.
pub trait RailMagnetPort {
    /// Turn the rail magnet on or off; answers whether it moved.
    fn switch_rail_magnet(&mut self, on: bool) -> bool;
}

/// Dock the magnet's two calls and `annotate.drawing_scope.set`: the rail's
/// switch, one drawing's own, the switch its context bar flips, and where
/// that drawing shows.
///
/// # Errors
/// A capability already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: RailMagnetPort + AddressedPanePort + ?Sized,
    A: JournalAccess,
{
    registry.register(rail_descriptor(), rail)?;
    registry.register(object_descriptor(), object)?;
    registry.register(drawing_scope::descriptor(), scope)
}

/// Record a call's result under `ANNOTATE_MODULE_ID`.
fn journal<'a, A: JournalAccess>(
    access: &'a mut A,
    actor: &'a ActorContext,
) -> impl FnOnce(&'static str, Value) -> WireU64 + 'a {
    |kind, value| access.record_event(NewEvent::by(ANNOTATE_MODULE_ID, kind, actor, value))
}

fn rail<H: RailMagnetPort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_rail(
        input,
        |on| host.switch_rail_magnet(on),
        journal(access, actor),
    )
}

fn object<H: AddressedPanePort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_object(
        input,
        move |tab, pane| Ok(host.addressed_pane(tab, pane)?.marks()),
        journal(access, actor),
    )
}

fn scope<H: AddressedPanePort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    drawing_scope::call(
        input,
        move |tab, pane| {
            let pane = host.addressed_pane(tab, pane)?;
            let bars = pane.marks_bars().to_config_string();
            Ok((pane.marks(), bars))
        },
        journal(access, actor),
    )
}

/// The rail's call: `set` flips the rail magnet and answers whether it moved,
/// `journal` records the result under its event kind.
///
/// # Errors
/// Malformed input.
pub fn set_rail<J>(
    input: &Value,
    set: impl FnOnce(bool) -> bool,
    journal: impl FnOnce(&'static str, Value) -> J,
) -> Result<Value, ControlError> {
    let input: DrawingMagnetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let result = DrawingMagnetResult {
        enabled: input.enabled,
        changed: set(input.enabled),
    };
    let _ = journal(
        DRAWING_MAGNET_EVENT_KIND,
        serde_json::json!({ "drawing_magnet": result }),
    );
    serde_json::to_value(result).map_err(invalid)
}

/// One drawing's call: `pane` finds the named pane's store, `journal` records
/// the result under its event kind. The switch is one undo step.
///
/// # Errors
/// Malformed input, a pane `pane` refuses, an id the store does not hold, and
/// on for a tool that snaps by its own rule.
pub fn set_object<'a, J>(
    input: &Value,
    pane: impl FnOnce(WireU64, WireU64) -> Result<&'a mut Drawings<DrawingTool>, ControlError>,
    journal: impl FnOnce(&'static str, Value) -> J,
) -> Result<Value, ControlError> {
    let input: ObjectMagnetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let drawings = pane(input.tab_id, input.pane_id)?;
    let changed = drawings.set_own_magnet(DrawingId(input.drawing_id.get()), input.enabled);
    let result = input.result(changed.map_err(|tool| input.refusal(tool))?);
    let _ = journal(
        OBJECT_MAGNET_EVENT_KIND,
        input.event_payload(result.changed),
    );
    serde_json::to_value(result).map_err(invalid)
}

#[cfg(test)]
#[path = "drawing_magnet_tests.rs"]
mod tests;
