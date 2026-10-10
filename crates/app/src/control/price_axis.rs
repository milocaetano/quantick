//! Named price and tape framing over the same pane state as their gestures.
use super::actions::ActionRegistry;
use super::{gateway::ControlAccess, layout};
use crate::app::{TabsMutPort, TabsPort};
use crate::pane::ChartPane;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control::{error::ControlError, registry::RegistryError};
use quantick_control_schema::price_axis::{PriceAxisInput, PriceAxisResult, descriptor};
use quantick_control_schema::tape_view::{TapeViewInput, TapeViewResult};
use serde_json::Value;

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)?;
    registry.register(quantick_control_schema::tape_view::descriptor(), set_tape)
}

pub(super) fn invalid(error: impl ToString) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// The pane a call names, on the tab it names.
pub(super) fn pane<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    tab_id: WireU64,
    pane_id: WireU64,
) -> Result<&mut ChartPane, ControlError> {
    let tab_id = Some(tab_id);
    let index = layout::tab_index(app, layout::TabTarget { tab_id })?;
    let tab = app.tabs_mut().tab_at_mut(index);
    tab.ok_or_else(|| invalid("the tab closed while the call ran"))?
        .panes_mut()
        .find(|pane| pane.id == pane_id.get())
        .ok_or_else(|| invalid("the call names an unknown pane on the requested tab"))
}

fn set<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: PriceAxisInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let range = input.mode.range()?;
    let pane = pane(app, input.tab_id, input.pane_id)?;
    pane.sync_price_axis_mode();
    match range {
        Some((low, high)) => {
            pane.price_view.set_manual_range(low, high);
        }
        None => pane.price_view.reset(),
    }
    let viewport = super::chart::viewport_snapshot(pane);
    let (tab_id, pane_id) = (input.tab_id, input.pane_id);
    serde_json::to_value(PriceAxisResult {
        tab_id,
        pane_id,
        viewport,
    })
    .map_err(invalid)
}

/// The window first, so a past end is clamped against the window it shows.
fn set_tape<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: TapeViewInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let (end, window) = input.request()?;
    let tape = pane(app, input.tab_id, input.pane_id)?.orderflow.as_mut();
    let tape = tape
        .filter(|tape| tape.cached_config().native_tape())
        .ok_or_else(|| invalid("the pane draws no native tape"))?;
    // The wheel's path: the view moves, the asset's settings do not.
    if let Some(window) = window {
        tape.navigate_live_lane_window(window);
    }
    if let Some(end) = end {
        tape.set_tape_end(end);
    }
    let tape = tape
        .tape_view_snapshot()
        .ok_or_else(|| invalid("no native tape"))?;
    let (tab_id, pane_id) = (input.tab_id, input.pane_id);
    serde_json::to_value(TapeViewResult {
        tab_id,
        pane_id,
        tape,
    })
    .map_err(invalid)
}
