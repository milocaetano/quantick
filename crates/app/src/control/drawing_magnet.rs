//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation: the
//! hand's two paths onto the calls `quantick_control_handlers::drawing_magnet`
//! docks.

pub(crate) use quantick_control_schema::drawing_magnet::*;

use crate::app::{GatewayPort, TabsPort, ToolRailPort};

use crate::drawings::DrawingId;

use quantick_control::wire::WireU64;

use quantick_control::annotation::ANNOTATE_MODULE_ID;

use super::journal::NewEvent;

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
    let payload = input.event_payload(true);
    let event = NewEvent::by(
        ANNOTATE_MODULE_ID,
        OBJECT_MAGNET_EVENT_KIND,
        &actor,
        payload,
    );
    access.append_event(event);
}
