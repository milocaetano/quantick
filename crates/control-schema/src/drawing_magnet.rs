//! The drawing rail's OHLC magnet, shared by the rail button and automation.

use schemars::JsonSchema;

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::registry::IdempotencyPolicy::Optional;
use serde::{Deserialize, Serialize};

pub const DRAWING_MAGNET_CAPABILITY_ID: &str = "annotate.magnet.set";

pub const DRAWING_MAGNET_EVENT_KIND: &str = "annotate.magnet.changed";

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

/// How a client reconciles an interrupted call that sets the magnet.
pub const READBACKS: &[Readback] = &[journal(
    DRAWING_MAGNET_CAPABILITY_ID,
    Optional,
    DRAWING_MAGNET_EVENT_KIND,
    "payload.drawing_magnet.enabled",
    "an event after the pre-call cursor carries the requested boolean",
    &[EVERY_OPTIONAL_TEST],
)];
