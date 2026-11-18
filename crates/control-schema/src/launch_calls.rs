//! A scripted launch's values read as the named calls they stand for: each
//! returns the registered capability and its input, or nothing for a value it
//! cannot read, so a typo in a validation script changes nothing.

use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_layers::ChartLayer;
use serde_json::{Value, json};

use crate::{layers, layout, price_axis};

/// A registered capability and the input a launch runs it with.
pub type LaunchCall = (&'static str, Value);

/// A reader for a value that sets one pane.
pub type PaneCall = fn(&str, u64, u64) -> Option<LaunchCall>;

/// `low:high`: hold that price range on the pane.
#[must_use]
pub fn price_range(value: &str, tab_id: u64, pane_id: u64) -> Option<LaunchCall> {
    let (low, high) = value.split_once(':')?;
    let mode = price_axis::PriceAxisMode::Manual {
        low: CanonicalDecimal::new(low.trim()).ok()?,
        high: CanonicalDecimal::new(high.trim()).ok()?,
    };
    mode.range().ok()??;
    let (tab_id, pane_id) = (WireU64::new(tab_id), WireU64::new(pane_id));
    let input = price_axis::PriceAxisInput {
        tab_id,
        pane_id,
        mode,
    };
    Some((
        price_axis::PRICE_AXIS_CAPABILITY_ID,
        serde_json::to_value(input).ok()?,
    ))
}

/// `on` or `off`: the Tape switch on the pane's header.
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

/// A layout by name: switch to it, or create it when the book has none.
#[must_use]
pub fn layout_tab(name: &str, known: bool) -> LaunchCall {
    let id = if known {
        layout::TAB_SWITCH_CAPABILITY_ID
    } else {
        layout::TAB_CREATE_CAPABILITY_ID
    };
    (id, json!({ "name": name }))
}

/// A `1` switch for a call that takes no input of its own; anything else is
/// no call.
#[must_use]
pub fn when_one(value: Option<&str>, capability: &'static str) -> Option<LaunchCall> {
    crate::launch_values::one(value).then(|| (capability, json!({})))
}

#[cfg(test)]
#[path = "launch_calls_tests.rs"]
mod tests;
