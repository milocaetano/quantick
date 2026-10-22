//! `chart.time_axis.reset`: a time strip's, or open canvas's, double-click by name.
use super::actions::ActionRegistry;
use super::gateway::ControlAccess;
use super::price_axis::{invalid, pane};
use crate::app::{TabsMutPort, TabsPort};
use quantick_control::wire::ActorContext;
use quantick_control::{error::ControlError, registry::RegistryError};
use quantick_control_schema::time_axis::{
    TimeAxis, TimeAxisResetInput, TimeAxisResetResult, TimeAxisTarget, descriptor,
};
use serde_json::Value;

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), reset)
}

/// Through [`crate::pane::ChartPane::reset_time_axis`], the gesture's own
/// path and eligibility, so the call and the strip cannot drift.
fn reset<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: TimeAxisResetInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let pane = pane(app, input.tab_id, input.pane_id)?;
    let tape = input.axis == TimeAxis::Tape;
    let applied = match input.to {
        TimeAxisTarget::Opening => pane.reset_time_axis(tape),
        TimeAxisTarget::LiveOverview => !tape && pane.model.live_overview(),
    };
    input.to.outcome(applied).map_err(invalid)?;
    let viewport = super::chart::viewport_snapshot(pane);
    let (tab_id, pane_id) = (input.tab_id, input.pane_id);
    serde_json::to_value(TimeAxisResetResult {
        tab_id,
        pane_id,
        viewport,
    })
    .map_err(invalid)
}
