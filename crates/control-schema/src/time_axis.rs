//! A pane's time axis back where it opened: the strip double-click, by name;
//! the candles back to live at the overview spacing; or back to live with the
//! last price centred: the open-canvas double- and triple-click, by name.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chart::{MODULE_ID, SCOPE_ID, ViewportSnapshot};
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TIME_AXIS_RESET_CAPABILITY_ID: &str = "chart.time_axis.reset";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeAxisResetInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub axis: TimeAxis,
    /// Where the axis goes; the opening spacing when omitted.
    #[serde(default)]
    pub to: TimeAxisTarget,
}

/// Where a reset takes the axis.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeAxisTarget {
    /// Back to the spacing or window the axis opened on.
    #[default]
    Opening,
    /// The candles only: back to the live edge at the overview spacing,
    /// the recent move and its context across the candles' width, never a
    /// zoom-in.
    LiveOverview,
    /// The candles only: back to the live edge at the spacing they have,
    /// the last price framed at mid-height once the next frame fits the
    /// live view — what a double-click on open canvas does.
    LiveCentre,
    /// `live_centre` with the candles twice as wide, up to the widest
    /// a candle may be — what a triple-click on open canvas does. Each call
    /// zooms again: retry it under an idempotency key.
    LiveCentreZoomIn,
}

impl TimeAxisTarget {
    /// `Ok` when a reset to this target `applied`, else why it was refused.
    pub fn outcome(self, applied: bool) -> Result<(), &'static str> {
        match (applied, self) {
            (true, _) => Ok(()),
            (false, Self::Opening) => Err("the pane shows no tape time strip"),
            (false, Self::LiveOverview) => {
                Err("the live overview needs the candles' axis on a laid-out pane")
            }
            (false, Self::LiveCentre | Self::LiveCentreZoomIn) => {
                Err("the live centre moves the candles' axis only")
            }
        }
    }
}

/// Which of the pane's two time strips is reset.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeAxis {
    /// The candles' bar spacing, back to the one the pane opened on.
    Candles,
    /// The native tape's window, back to the asset's own setting.
    Tape,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TimeAxisResetResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub viewport: ViewportSnapshot,
}

pub fn descriptor() -> CapabilityDescriptor {
    crate::layout::transient_descriptor::<TimeAxisResetInput, TimeAxisResetResult>(
        TIME_AXIS_RESET_CAPABILITY_ID,
        MODULE_ID,
        "Reset a pane's time axis",
        "Returns the candles' bar spacing to the one the pane opened on, or the native tape's window to the asset's own setting — what a double-click on that time strip does; that never moves the right edge or the other axis. With `to: live_overview` on the candles it returns them to the live edge at the overview spacing instead. With `to: live_centre` it returns them to the live edge at the spacing they have and, from the next frame, holds a price range centred on the last price that keeps every visible bar — what a double-click on open canvas does; `to: live_centre_zoom_in` also doubles the spacing, up to the widest candle — what a triple-click there does.",
        "Stable tab and pane IDs select one axis; every target but `live_centre_zoom_in` is idempotent, which zooms again on each call, so retry it under an idempotency key. The result reports the spacing, the live follow and the tape window after it; the centred price range reads back from the next frame.",
    )
}

pub const READBACKS: &[Readback] = &[snapshot(
    TIME_AXIS_RESET_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    SCOPE_ID,
    "panes[].viewport",
    "The target pane reports its opening pixels_per_bar, or its tape window back on the asset's setting; after the live overview, follows_live and the overview pixels_per_bar; after the live centre, follows_live, the kept (or doubled) pixels_per_bar and, from the next frame, a manual price_range centred on the last price.",
    &[
        EVERY_OPTIONAL_TEST,
        "the_time_axis_reset_call_matches_the_strip_double_click_and_reads_it_back",
        "the_live_overview_call_returns_the_candles_to_a_live_overview",
        "the_live_centre_calls_match_the_canvas_double_and_triple_click",
    ],
)];

/// Published schemas of the time axis reset.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "chart-time-axis-reset-input-v1.schema.json",
            schema::<TimeAxisResetInput>(),
        ),
        (
            "chart-time-axis-reset-result-v1.schema.json",
            schema::<TimeAxisResetResult>(),
        ),
    ]
}
