//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation.

pub(crate) use quantick_control_schema::drawing_magnet::*;

use crate::app::{GatewayPort, TabsMutPort, TabsPort, ToolRailPort};

use crate::drawings::DrawingId;

use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};

use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control_handlers::{drawing_magnet as magnet, drawing_scope as scope};
use serde_json::Value;

use super::{actions::ActionRegistry, gateway::ControlAccess, journal::NewEvent};

/// Dock the magnet's two calls and `annotate.drawing_scope.set`, each decided
/// in `quantick_control_handlers`: the rail's switch and one drawing's own,
/// the switch its context bar flips, and where that drawing shows.
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(rail_descriptor(), set)?;
    registry.register(object_descriptor(), set_object)?;
    registry.register(scope::descriptor(), set_scope)
}

/// Record a call's result under `ANNOTATE_MODULE_ID`.
fn journal<'a>(
    access: &'a mut ControlAccess,
    actor: &'a ActorContext,
) -> impl FnOnce(&'static str, Value) -> WireU64 + 'a {
    |kind, value| access.append_event(NewEvent::by(ANNOTATE_MODULE_ID, kind, actor, value))
}

fn set<P: ToolRailPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    magnet::set_rail(
        input,
        |on| app.set_drawing_magnet(on),
        journal(access, actor),
    )
}

fn set_object<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    magnet::set_object(
        input,
        move |tab, pane| Ok(&mut super::price_axis::pane(app, tab, pane)?.drawings),
        journal(access, actor),
    )
}

fn set_scope<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    scope::call(
        input,
        move |tab, pane| {
            let pane = super::price_axis::pane(app, tab, pane)?;
            let bars = pane.layout.drawings_spec.unwrap_or(*pane.state.spec());
            Ok((&mut pane.drawings, bars.to_config_string()))
        },
        journal(access, actor),
    )
}

/// A hand's flip of the rail's magnet is `annotate.magnet.set`, so it
/// journals alike. Without control access it lands directly.
pub(crate) fn flip_rail_magnet<P: GatewayPort + ToolRailPort + ?Sized>(app: &mut P, on: bool) {
    let input = serde_json::json!({ "enabled": on });
    let human = super::ActionOrigin::Human;
    if app
        .control_action(DRAWING_MAGNET_CAPABILITY_ID, 1, human, input)
        .is_err()
    {
        app.set_drawing_magnet(on);
    }
}

/// Journal a hand's flip of the selection's own magnet as
/// `annotate.drawing_magnet.changed`, the event the call leaves, by the
/// trader's hand. The flip itself landed with the context bar's edited copy,
/// one undo step with the rest of the edit, so it never enters the control
/// trace a replay re-runs. Without control access there is no journal.
pub(crate) fn journal_own_magnet_flip<P>(app: &mut P, flip: Option<(DrawingId, bool)>)
where
    P: GatewayPort + TabsPort + ?Sized,
{
    let Some((id, enabled)) = flip else {
        return;
    };
    let reads = app.tab_reads();
    let tab_index = reads.active_tab_index();
    let tabs = reads.tabs();
    let pane = tabs[tab_index].pane(tabs[tab_index].drawing_side());
    let input = ObjectMagnetInput {
        tab_id: WireU64::new(tabs.id_at(tab_index)),
        pane_id: WireU64::new(pane.id),
        drawing_id: WireU64::new(id.0),
        enabled,
    };
    let Some(access) = app.gateway_slot().as_mut() else {
        return;
    };
    let Some(actor) = access.hand_actor() else {
        return;
    };
    journal(access, &actor)(OBJECT_MAGNET_EVENT_KIND, input.event_payload(true));
}
