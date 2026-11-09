//! Wire-to-canvas adapter for the addressed vertical splitter operation.

use crate::app::{TabsMutPort, TabsPort};
use eframe::egui;
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::super::{types::canonical_f32, workspace::SPLIT_FRACTION_DECIMAL_PLACES};
use super::*;
use crate::tab::context_resize::ResizeContextPair;

pub(super) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(resize_pair_descriptor(), resize)
}

fn resize<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizePairInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
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
    let index = super::tab_index(app, input.target)?;
    let tab_id = app.tab_reads().tabs().id_at(index);
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let geometry = tab.canvas.context_stack_geometry(&tab.time_panes);
    let (column, _) = geometry.ok_or_else(|| {
        ControlError::invalid_request(
            "the current context stack must be visible and drawn before resizing",
        )
    })?;
    let result = tab
        .canvas
        .resize_context_pair(
            tab_id,
            ResizeContextPair {
                upper_pane_id: input.upper_pane_id.get(),
                lower_pane_id: input.lower_pane_id.get(),
                wanted_y: egui::lerp(column.y_range(), fraction),
            },
            actor.actor_kind,
            &tab.time_panes,
        )
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let result = ResizePairResult {
        tab_id: WireU64::new(tab_id),
        upper_pane_id: input.upper_pane_id,
        lower_pane_id: input.lower_pane_id,
        fraction: canonical_f32(result.fraction, SPLIT_FRACTION_DECIMAL_PLACES)
            .expect("a clamped canvas fraction is a finite canonical decimal"),
        changed: result.changed,
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}
