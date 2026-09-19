//! Durable per-indicator price-hover guide, shared by UI and automation.

use quantick_control::wire::WireU64;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const INDICATOR_GUIDE_CAPABILITY_ID: &str = "indicator.mouse_vertical_line.set";

pub const INDICATOR_GUIDE_EVENT_KIND: &str = "indicator.mouse_vertical_line.changed";

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndicatorGuideInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub slot_id: WireU64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct IndicatorGuideResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub slot_id: WireU64,
    pub enabled: bool,
}
