//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation.

use schemars::JsonSchema;

use crate::analysis::DRAWINGS_SCOPE_ID;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal, snapshot};
use quantick_control::registry::IdempotencyPolicy::Optional;
use quantick_control::wire::WireU64;
use serde::{Deserialize, Serialize};

pub const DRAWING_MAGNET_CAPABILITY_ID: &str = "annotate.magnet.set";

pub const DRAWING_MAGNET_EVENT_KIND: &str = "annotate.magnet.changed";

pub const OBJECT_MAGNET_CAPABILITY_ID: &str = "annotate.drawing_magnet.set";

pub const OBJECT_MAGNET_EVENT_KIND: &str = "annotate.drawing_magnet.changed";

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingMagnetInput {
    /// `true` snaps drawing anchors to the open / high / low / close of the
    /// candle under the pointer; `false` leaves them where the pointer is.
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DrawingMagnetResult {
    /// The magnet as it stands after the call.
    pub enabled: bool,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

/// One drawing's own magnet, which snaps that object's drags whether or not
/// the rail's magnet is on.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectMagnetInput {
    /// The drawing, by the `drawing_id` `analysis.drawings` lists.
    pub drawing_id: WireU64,
    /// `true` snaps this drawing's dragged anchors to the open / high / low /
    /// close of the candle under the pointer; `false` leaves it to the rail.
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObjectMagnetResult {
    pub drawing_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The drawing's own magnet as it stands after the call.
    pub enabled: bool,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

/// How a client reconciles an interrupted call that sets a magnet.
pub const READBACKS: &[Readback] = &[
    journal(
        DRAWING_MAGNET_CAPABILITY_ID,
        Optional,
        DRAWING_MAGNET_EVENT_KIND,
        "payload.drawing_magnet.enabled",
        "an event after the pre-call cursor carries the requested boolean",
        &[EVERY_OPTIONAL_TEST],
    ),
    snapshot(
        OBJECT_MAGNET_CAPABILITY_ID,
        Optional,
        DRAWINGS_SCOPE_ID,
        "tabs[].panes[].drawings[].magnet",
        "the row whose `drawing_id` was named carries the requested boolean",
        &[EVERY_OPTIONAL_TEST],
    ),
];
