//! `chart.price_axis.set` and `chart.tape_view.set`: named price and tape
//! framing on the addressed pane ([`crate::axis_pane::AxisPane`]), over the
//! same pane state as their gestures.
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::price_axis::{PriceAxisInput, PriceAxisResult};
use quantick_control_schema::tape_view::{TapeViewInput, TapeViewResult};
use serde_json::Value;

use crate::chart::ChartPort;
use crate::dock::ActionDock;

const NO_NATIVE_TAPE: &str = "the pane draws no native tape";

/// Dock `chart.price_axis.set` and `chart.tape_view.set`.
///
/// # Errors
/// A capability already docked.
pub fn register<D: ActionDock<H, A>, H: ChartPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(quantick_control_schema::price_axis::descriptor(), set_price)?;
    registry.register(quantick_control_schema::tape_view::descriptor(), set_tape)
}

fn invalid(error: impl ToString) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

fn set_price<H: ChartPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: PriceAxisInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let range = input.mode.range()?;
    let pane = host.axis_pane(input.tab_id, input.pane_id)?;
    pane.frame_prices(range);
    serde_json::to_value(PriceAxisResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        viewport: pane.framing(),
    })
    .map_err(invalid)
}

/// The window first, so a past end is clamped against the window it shows.
fn set_tape<H: ChartPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: TapeViewInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let (end, window) = input.request()?;
    let pane = host.axis_pane(input.tab_id, input.pane_id)?;
    let tape = pane.native_tape().ok_or_else(|| invalid(NO_NATIVE_TAPE))?;
    if let Some(window) = window {
        tape.move_tape_window(window, input.file);
    }
    if let Some(end) = end {
        tape.move_tape_end(end);
    }
    let tape = tape.tape_frame().ok_or_else(|| invalid("no native tape"))?;
    serde_json::to_value(TapeViewResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        tape,
    })
    .map_err(invalid)
}

#[cfg(test)]
#[path = "price_axis_tests.rs"]
mod tests;
