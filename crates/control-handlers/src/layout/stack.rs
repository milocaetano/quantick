//! Wire-to-canvas adapter for the addressed vertical splitter operation.

use quantick_control::{error::ControlError, wire::ActorContext, wire::WireU64};
use quantick_control_host::wire::canonical_f32;
use quantick_control_schema::layout::{ResizePairInput, ResizePairResult};
use quantick_control_schema::workspace::SPLIT_FRACTION_DECIMAL_PLACES;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde_json::Value;

use super::{LayoutHost, parse, tab_at, tab_index};

pub(super) fn resize<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizePairInput = parse(input)?;
    let fraction: Decimal = input
        .fraction
        .as_str()
        .parse()
        .map_err(|error| ControlError::invalid_request(format!("fraction: {error}")))?;
    if fraction.scale() > SPLIT_FRACTION_DECIMAL_PLACES
        || fraction < Decimal::ZERO
        || fraction > Decimal::ONE
    {
        return Err(ControlError::invalid_request(
            "fraction must be between 0 and 1 with at most six decimal places",
        ));
    }
    let fraction = fraction
        .to_f32()
        .ok_or_else(|| ControlError::invalid_request("fraction is out of range"))?;
    let index = tab_index(app, input.target)?;
    let tab_id = app.tab_id_at(index);
    let pair = (input.upper_pane_id.get(), input.lower_pane_id.get());
    let (fraction, changed) = tab_at(app, index)?
        .resize_context_pair(tab_id, pair, fraction, actor.actor_kind)
        .ok_or_else(|| {
            ControlError::invalid_request(
                "the current context stack must be visible and drawn before resizing",
            )
        })?
        .map_err(ControlError::invalid_request)?;
    let result = ResizePairResult {
        tab_id: WireU64::new(tab_id),
        upper_pane_id: input.upper_pane_id,
        lower_pane_id: input.lower_pane_id,
        fraction: canonical_f32(fraction, SPLIT_FRACTION_DECIMAL_PLACES)
            .expect("a clamped canvas fraction is a finite canonical decimal"),
        changed,
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}
