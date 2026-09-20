//! Durable per-indicator price-hover guide, shared by UI and automation.

use quantick_control::wire::WireU64;

use schemars::JsonSchema;

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::registry::IdempotencyPolicy::Optional;
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

/// How a client reconciles an interrupted call that moves the guide line.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[journal(
    "indicator.mouse_vertical_line.set",
    Optional,
    INDICATOR_GUIDE_EVENT_KIND,
    "payload.indicator_guide.enabled",
    "an event after the pre-call cursor carries the requested boolean and its payload names the exact tab, pane and slot",
    &[EVERY_OPTIONAL_TEST],
)];
