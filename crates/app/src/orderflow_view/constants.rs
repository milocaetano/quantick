//! The order-flow view's tunables: the FLOW worker's queue and idle wait,
//! and the Bubbles tab's control domains and choices. One file, so retuning
//! touches one place.

use std::time::Duration;

/// Capacity of the FLOW worker's bounded input queue, in chunks; one wake drains at most this many.
pub(super) const QUEUE_CHUNKS: usize = 2;
/// Flush a stalled partial without treating normal inter-frame packet gaps as idle.
pub(super) const PARTIAL_IDLE_WAIT: Duration = Duration::from_millis(250);

pub(super) use quantick_orderflow::config::controls::*;
