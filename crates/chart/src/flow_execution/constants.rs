//! Tunable sizes, offsets and search bounds of regional FLOW: the projection
//! request, the earned circles and peak placement. One place for later config
//! wiring; values are logical pixels unless a name says otherwise.

// Regional projection request (`flow_execution.rs`).

/// Ordinary reference radius, in logical pixels. Compact marks and a separate
/// spatial support preserve the surrounding candle path.
pub(crate) const FLOW_RADIUS_LIMIT_PX: f32 = 12.0;

/// Spatial support radius, in logical pixels. A small geometric support pools
/// unresolved neighbours without letting the largest volume in another region
/// determine which executions belong together.
pub(crate) const FLOW_MERGE_SUPPORT_RADIUS_PX: f32 = 6.0;

// Earned circles (`disc.rs`).

/// Uniform `[x, y]` translation of every FLOW circle, in logical pixels. One
/// translation preserves the execution path and every inter-region distance.
pub const FLOW_EXECUTION_OFFSET: [f32; 2] = [-18.0, -18.0];

// Peak placement (`presentation.rs`, `presentation/placement_index.rs`).

/// Most circles in one leaf of the placement index. Small leaves bound exact
/// circle checks after spatial bounds reject a branch.
pub(crate) const CIRCLE_INDEX_LEAF_CAPACITY: usize = 8;

/// Farthest a peak circle may move from its source, in logical pixels.
/// Movement stays near the factual source, preserving local reading.
pub(crate) const MAX_PEAK_DISPLACEMENT_PX: usize = 24;

/// Distance between successive placement rings, in logical pixels.
pub(crate) const PEAK_DISPLACEMENT_STEP_PX: usize = 2;

/// Gap a relocated peak keeps from placed circles, in logical pixels. Only
/// relocated circles request breathing room; separate source circles stay put.
pub(crate) const RELOCATED_PEAK_GAP_PX: f32 = 1.0;

/// Placement directions, in screen-space degrees: above-left first, then
/// nearby upward alternatives.
pub(crate) const PEAK_DISPLACEMENT_ANGLES_DEGREES: [f32; 9] = [
    -135.0, -90.0, -45.0, -180.0, 0.0, -157.5, -112.5, -67.5, -22.5,
];
