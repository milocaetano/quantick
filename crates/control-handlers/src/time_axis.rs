//! `chart.time_axis.reset`: a time strip's, or open canvas's, double- and
//! triple-click by name, on the addressed pane
//! ([`crate::axis_pane::AxisPane`]).
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::time_axis::{
    TimeAxis, TimeAxisResetInput, TimeAxisResetResult, TimeAxisTarget,
};
use serde_json::Value;

use crate::chart::ChartPort;
use crate::dock::ActionDock;
use crate::invalid;

/// Dock `chart.time_axis.reset`.
///
/// # Errors
/// A capability already docked.
pub fn register<D: ActionDock<H, A>, H: ChartPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(quantick_control_schema::time_axis::descriptor(), reset_time)
}

/// The gestures' own paths and eligibility, so the call and the clicks
/// cannot drift.
fn reset_time<H: ChartPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: TimeAxisResetInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let pane = host.addressed_pane(input.tab_id, input.pane_id)?;
    let tape = input.axis == TimeAxis::Tape;
    let applied = match input.to {
        TimeAxisTarget::Opening => pane.reset_time_strip(tape),
        TimeAxisTarget::LiveOverview => !tape && pane.show_live_overview(),
        centre => !tape && pane.centre_live(centre == TimeAxisTarget::LiveCentreZoomIn),
    };
    input.to.outcome(applied).map_err(invalid)?;
    serde_json::to_value(TimeAxisResetResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        viewport: pane.framing(),
        price_centre_pending: input.to.centres(),
    })
    .map_err(invalid)
}

#[cfg(test)]
#[path = "time_axis_tests.rs"]
mod tests;
