//! Tunable gesture thresholds of the interaction owners. One place for later
//! config wiring.

// Tape drag (`tape_drag.rs`).

/// Travel, in screen pixels, before the hand's initial direction can be
/// distinguished from a click.
pub(crate) const DRAG_JUDGED_AFTER_PX: f32 = 6.0;
