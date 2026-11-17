//! The launch hooks a scripted run sets, read as the named calls they stand
//! for: each returns the registered capability and its input, or nothing for
//! a value it cannot read, so a typo in a validation script changes nothing.

use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_layers::ChartLayer;
use serde_json::{Value, json};

use crate::{desk, layers, layout, price_axis};

/// A registered capability and the input a launch hook runs it with.
pub type LaunchCall = (&'static str, Value);

/// The hooks that set one pane, by name: a hook this does not know is no call.
#[must_use]
pub fn on_pane(hook: &str, value: &str, tab_id: u64, pane_id: u64) -> Option<LaunchCall> {
    match hook {
        "QUANTICK_PRICE_RANGE" => price_range(value, tab_id, pane_id),
        "QUANTICK_TAPE" => tape(value, tab_id, pane_id),
        _ => None,
    }
}

/// `QUANTICK_PRICE_RANGE=low:high`: hold that range on the pane.
#[must_use]
pub fn price_range(value: &str, tab_id: u64, pane_id: u64) -> Option<LaunchCall> {
    price_axis::PriceAxisMode::parse_range(value)?;
    let (low, high) = value.split_once(':')?;
    let input = price_axis::PriceAxisInput {
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        mode: price_axis::PriceAxisMode::Manual {
            low: CanonicalDecimal::new(low.trim()).ok()?,
            high: CanonicalDecimal::new(high.trim()).ok()?,
        },
    };
    Some((
        price_axis::PRICE_AXIS_CAPABILITY_ID,
        serde_json::to_value(input).ok()?,
    ))
}

/// `QUANTICK_TAPE=on|off`: the Tape switch on the pane's header.
#[must_use]
pub fn tape(value: &str, tab_id: u64, pane_id: u64) -> Option<LaunchCall> {
    let visible = match value.trim() {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    let input = layers::SetVisibilityInput {
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        layer_id: ChartLayer::TapeChart.id().to_owned(),
        visible,
    };
    Some((
        layers::SET_VISIBILITY_CAPABILITY_ID,
        serde_json::to_value(input).ok()?,
    ))
}

/// `QUANTICK_LAYOUT_TAB=<name>`: switch to the layout by that name, or create
/// it when the book has none.
#[must_use]
pub fn layout_tab(name: &str, known: bool) -> LaunchCall {
    let id = if known {
        layout::TAB_SWITCH_CAPABILITY_ID
    } else {
        layout::TAB_CREATE_CAPABILITY_ID
    };
    (id, json!({ "name": name }))
}

/// The `=1` switches that name a call with no input of its own.
#[must_use]
pub fn switch(hook: &str, value: Option<&str>) -> Option<LaunchCall> {
    let id = match hook {
        "QUANTICK_PANE_COLLAPSED" => layout::COLLAPSE_CAPABILITY_ID,
        "QUANTICK_FLOW_PANE_COLLAPSED" => layout::FLOW_COLLAPSE_CAPABILITY_ID,
        "QUANTICK_WORKSPACE_SAVE" => desk::DESK_SAVE_CAPABILITY_ID,
        _ => return None,
    };
    (value == Some("1")).then(|| (id, json!({})))
}

#[cfg(test)]
#[path = "launch_calls_tests.rs"]
mod tests;
