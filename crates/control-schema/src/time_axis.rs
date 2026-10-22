//! A pane's time axis back where it opened: the strip double-click, by name;
//! or the candles back to live at the overview spacing: the open-canvas
//! double-click, by name.

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
        "Returns the candles' bar spacing to the one the pane opened on, or the native tape's window to the asset's own setting — what a double-click on that time strip does; that never moves the right edge or the other axis. With `to: live_overview` on the candles it returns them to the live edge at the overview spacing instead — what a double-click on open canvas does.",
        "Stable tab and pane IDs select one axis; a reset is idempotent, and the result reports the spacing, the live follow and the tape window after it.",
    )
}

pub const READBACKS: &[Readback] = &[snapshot(
    TIME_AXIS_RESET_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    SCOPE_ID,
    "panes[].viewport",
    "The target pane reports its opening pixels_per_bar, or its tape window back on the asset's setting; after the live overview, follows_live and the overview pixels_per_bar.",
    &[
        EVERY_OPTIONAL_TEST,
        "the_time_axis_reset_call_matches_the_strip_double_click_and_reads_it_back",
        "the_live_overview_call_matches_the_open_canvas_double_click",
    ],
)];
