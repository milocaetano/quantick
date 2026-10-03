//! The native tape's own time frame: where its right edge is — live, or a
//! past instant — and how much market time it shows. The same state the
//! tape's wheel, drag and double click change.

use quantick_control::{
    error::ControlError,
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chart::{MODULE_ID, SCOPE_ID};
use crate::orderflow::{LaneWindowSnapshot, lane_window};
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TAPE_VIEW_CAPABILITY_ID: &str = "chart.tape_view.set";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TapeViewInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Where the right edge goes; absent keeps it.
    #[serde(default)]
    pub end: Option<TapeEndInput>,
    /// How much market time the tape shows; absent keeps it.
    #[serde(default)]
    pub window: Option<TapeWindowInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TapeEndInput {
    /// Pin the right edge to now again.
    Live,
    /// Hold the right edge at this instant. Clamped to the retained tape and
    /// to now; the result reports where it landed.
    Past {
        #[schemars(extend("x-unit" = "unix_milliseconds"))]
        end_unix_ms: i64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TapeWindowInput {
    /// Follow the recent bars' typical duration again.
    Auto,
    /// Show this many exchange milliseconds.
    Fixed {
        #[schemars(extend("x-unit" = "milliseconds"))]
        ms: i64,
    },
}

impl TapeViewInput {
    /// The requested end and window. At least one is required, and a fixed
    /// window is a positive duration.
    pub fn request(&self) -> Result<(Option<TapeEnd>, Option<LaneWindow>), ControlError> {
        if self.end.is_none() && self.window.is_none() {
            return Err(ControlError::invalid_request(
                "a tape view call names an end, a window or both",
            ));
        }
        let window = match self.window {
            None => None,
            Some(TapeWindowInput::Auto) => Some(LaneWindow::default()),
            Some(TapeWindowInput::Fixed { ms }) if ms > 0 => Some(LaneWindow::Fixed { ms }),
            Some(TapeWindowInput::Fixed { .. }) => {
                return Err(ControlError::invalid_request(
                    "a fixed tape window is a positive number of milliseconds",
                ));
            }
        };
        let end = self.end.as_ref().map(|end| match end {
            TapeEndInput::Live => TapeEnd::Live,
            TapeEndInput::Past { end_unix_ms } => TapeEnd::Past {
                end_ms: *end_unix_ms,
            },
        });
        Ok((end, window))
    }
}

/// The tape's time frame as a pane reports it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeViewSnapshot {
    /// The right edge is now. False while the tape is held in the past.
    pub follows_live: bool,
    /// The past instant the right edge is held at; absent while it follows
    /// live, where the edge is the moving tape clock.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub end_unix_ms: Option<i64>,
    pub window: LaneWindowSnapshot,
    /// The market time the tape shows now, resolved.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub window_ms: i64,
    /// First instant the retained tape is complete from: a past end cannot
    /// hold the window further back than half of it before this.
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub retained_from_unix_ms: Option<i64>,
}

impl TapeViewSnapshot {
    #[must_use]
    pub fn new(
        end: TapeEnd,
        window: LaneWindow,
        window_ms: i64,
        retained_from_ms: Option<i64>,
    ) -> Self {
        Self {
            follows_live: end.is_live(),
            end_unix_ms: end.past_ms(),
            window: lane_window(window),
            window_ms,
            retained_from_unix_ms: retained_from_ms,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TapeViewResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub tape: TapeViewSnapshot,
}

pub fn descriptor() -> CapabilityDescriptor {
    crate::layout::transient_descriptor::<TapeViewInput, TapeViewResult>(
        TAPE_VIEW_CAPABILITY_ID,
        MODULE_ID,
        "Move the native tape through time",
        "Holds the native tape's right edge at a past instant or pins it to live, and sets how much market time it shows. Never moves the candles beside it.",
        "Stable tab and pane IDs select one tape; the same end or window again is harmless, and the result reports the end after clamping.",
    )
}

pub const READBACKS: &[Readback] = &[snapshot(
    TAPE_VIEW_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    SCOPE_ID,
    "panes[].viewport.tape",
    "The target pane's tape reports the requested end (or live) and window.",
    &[
        EVERY_OPTIONAL_TEST,
        "the_tape_view_call_moves_the_tape_end_and_window_and_reads_them_back",
    ],
)];
