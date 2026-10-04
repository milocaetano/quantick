//! The order-flow view's tunables: the FLOW worker's queue and idle wait,
//! the tape header inset and fallback lane references, and the Bubbles tab's
//! control domains and choices. One file, so retuning touches one place.

use std::ops::RangeInclusive;
use std::time::Duration;

use quantick_orderflow::MAX_BUBBLE_MIN_RADIUS;

/// Capacity of the FLOW worker's bounded input queue, in chunks; one wake drains at most this many.
pub(super) const QUEUE_CHUNKS: usize = 2;
/// Flush a stalled partial without treating normal inter-frame packet gaps as idle.
pub(super) const PARTIAL_IDLE_WAIT: Duration = Duration::from_millis(250);
/// Tape header distance from the lane divider, each side of its text, in pixels.
pub(super) const TAPE_HEADER_INSET_PX: f32 = 6.0;
/// Bar duration, in exchange ms, the tape window resolves against while no frame has a live edge.
pub(super) const TAPE_BOUNDS_REFERENCE_MS: i64 = 15_000;
/// Bar duration, in exchange ms, the immediate tape sizes its price-range window from.
pub(super) const IMMEDIATE_TAPE_REFERENCE_MS: i64 = 15_000;

// Bubbles tab controls (`settings/bubble_sections.rs`): each slider's and drag
// field's domain, each drag field's speed, and each picker's choices.

/// History cluster-window choices, in milliseconds, beside their picker labels.
pub(super) const CLUSTER_WINDOW_CHOICES_MS: [(i64, &str); 7] = [
    (0, "Raw · one bubble per print"),
    (50, "50 ms"),
    (100, "100 ms"),
    (200, "200 ms"),
    (500, "500 ms"),
    (1_000, "1 s"),
    (2_000, "2 s"),
];
/// Dust-fold window choices, in milliseconds; zero folds nothing.
pub(super) const DUST_FOLD_CHOICES_MS: [i64; 5] = [0, 500, 1_500, 3_000, 10_000];
/// Price-region height choices, in display rows; one leaves rows unfolded.
pub(super) const REGION_ROW_CHOICES: [u32; 7] = [1, 2, 3, 4, 6, 8, 12];
/// Price-region window choices, in milliseconds.
pub(super) const REGION_WINDOW_CHOICES_MS: [i64; 6] = [500, 1_000, 1_500, 2_000, 3_000, 5_000];
/// Live-lane cluster-window choices, in milliseconds; `None` follows history.
pub(super) const LANE_CLUSTER_CHOICES_MS: [Option<i64>; 6] =
    [None, Some(0), Some(50), Some(100), Some(200), Some(500)];
/// Volume-dot full-size quantity domain, in contracts.
pub(super) const DOT_FULL_QUANTITY_RANGE: RangeInclusive<f64> = 1.0..=10_000_000.0;
/// Volume-dot full-size quantity drag speed, in contracts per point dragged.
pub(super) const DOT_FULL_QUANTITY_DRAG_SPEED: f64 = 10.0;
/// Fixed full-size print quantity domain, in the symbol's units.
pub(super) const SIZE_REFERENCE_QUANTITY_RANGE: RangeInclusive<f64> = 0.000_001..=1_000_000_000.0;
/// Fixed full-size print quantity drag speed, in units per point dragged.
pub(super) const SIZE_REFERENCE_QUANTITY_DRAG_SPEED: f64 = 1.0;
/// Display-floor quantity domain, in the symbol's units; zero draws everything.
pub(super) const MIN_QUANTITY_RANGE: RangeInclusive<f64> = 0.0..=1_000_000_000.0;
/// Display-floor quantity drag speed, in units per point dragged.
pub(super) const MIN_QUANTITY_DRAG_SPEED: f64 = 0.5;
/// Sphere rim-darkening domain, from flat (0) to full.
pub(super) const SPHERE_SHADING_RANGE: RangeInclusive<f32> = 0.0..=1.0;
/// Sphere light-spot strength domain, from none (0) to full.
pub(super) const SPHERE_HIGHLIGHT_RANGE: RangeInclusive<f32> = 0.0..=1.0;
/// Smallest-print radius domain, in pixels.
pub(super) const MIN_RADIUS_RANGE: RangeInclusive<f32> = 0.5..=MAX_BUBBLE_MIN_RADIUS;
/// Buy-up / sell-down separation domain, in pixels; zero pins both to the price.
pub(super) const SIDE_OFFSET_RANGE: RangeInclusive<f32> = 0.0..=20.0;
/// Bubble fill opacity domain, as alpha.
pub(super) const OPACITY_RANGE: RangeInclusive<f32> = 0.05..=1.0;
/// Rim width domain, in pixels; zero draws no rim.
pub(super) const OUTLINE_WIDTH_RANGE: RangeInclusive<f32> = 0.0..=4.0;
/// Halo strength domain, as alpha.
pub(super) const HALO_STRENGTH_RANGE: RangeInclusive<f32> = 0.0..=0.6;
/// Radius domain below which bubbles are plain dots, in pixels.
pub(super) const DETAIL_MIN_RADIUS_RANGE: RangeInclusive<f32> = 0.0..=20.0;
/// Radius domain below which a print is unreadable alone, in pixels.
pub(super) const READABLE_MIN_RADIUS_RANGE: RangeInclusive<f32> = 0.0..=24.0;
/// Consumption-front width domain, in pixels.
pub(super) const FRONT_WIDTH_RANGE: RangeInclusive<f32> = 0.5..=10.0;
/// Consumption-front length domain, in multiples of the bubble radius.
pub(super) const FRONT_LENGTH_SCALE_RANGE: RangeInclusive<f32> = 0.5..=6.0;
/// Impact-ring width domain, in pixels.
pub(super) const IMPACT_RING_WIDTH_RANGE: RangeInclusive<f32> = 0.5..=6.0;
/// Consumption-trail length domain, in pixels; zero draws no trail.
pub(super) const TRAIL_LENGTH_RANGE: RangeInclusive<f32> = 0.0..=80.0;
/// Consumption-trail opacity domain, as alpha.
pub(super) const TRAIL_OPACITY_RANGE: RangeInclusive<f32> = 0.0..=1.0;
/// Label-from radius domain, in pixels.
pub(super) const LABEL_MIN_RADIUS_RANGE: RangeInclusive<f32> = 4.0..=48.0;
