//! Tunable cadences, windows and spans of the crate-root modules: the engine,
//! the bar timeline, the tape view and the recorded tape history. One place
//! for later config wiring; the projection keeps its own in
//! `projection/constants.rs`.

use std::time::Duration;

// Engine (`engine.rs`, `engine_pending.rs`).

/// Minimum interval between dirty rebuilds of the finished half of the chart.
///
/// History or bar-boundary changes mark the projection dirty. This cadence
/// coalesces a burst of updates, while a clean projection remains cached
/// indefinitely. The live half ignores this interval entirely — see
/// [`crate::engine::BookEngine::project_at`].
pub const PROJECTION_INTERVAL: Duration = Duration::from_millis(220);

/// Maximum raw book levels per side copied into a published
/// [`crate::engine::BookLadder`]. Bounds the per-batch copy in
/// [`crate::engine::BookEngine::published`] and the memory the UI clones per
/// frame; deeper books stay fully captured in history, they are just not
/// republished level-by-level.
pub const LADDER_LEVELS_PER_SIDE: usize = 128;

/// The typical bar duration, in exchange milliseconds, a pending tape sizes
/// its lane window from when the request carries none.
pub const PENDING_LANE_REFERENCE_MS: i64 = 15_000;

// Native tape (retention metadata and display projection).

/// Fixed execution window, in milliseconds, before visual aggregation.
pub(crate) const NATIVE_TAPE_WINDOW_MS: i64 = 100;

// Recorded tape history (`history/openings.rs`).

/// Seven days of supported history plus the current date. WIN trades during
/// the UTC date of its B3 daytime session; this is not an exchange auction flag.
pub(crate) const RECORDED_DATES: i64 = 8;

// Tape view (`tape_view.rs`).

/// How far the window may reach before the first retained print, as a share
/// of itself, when the tape is panned to the start of its history. The
/// boundary then sits inside the tape, labelled, instead of scrolling out of
/// view — an empty stretch the trader can see is history the chart never had.
pub const RETAINED_EDGE_SHARE: f64 = 0.5;

// Bar timeline (`timeline.rs`).

/// How many recent closed bars decide how much market time the lane shows.
///
/// A single bar is too fragile a reference: one session break, one burst bar,
/// and the lane would be calibrated to a duration that never repeats. A median
/// over a short window follows the instrument's rhythm without letting one
/// outlier set it.
pub(crate) const RESERVE_SAMPLE_BARS: usize = 8;

/// Market time the lane shows before any bar has closed, in exchange
/// milliseconds. Only the very first bars of a fresh series see it.
pub(crate) const DEFAULT_RESERVE_MS: i64 = 1_000;

/// Shortest window the lane will show, in exchange milliseconds.
///
/// Bar durations are wildly uneven on an activity-sampled series: a burst
/// closes a tick bar in well under a second while a quiet stretch takes a
/// minute, so the median alone can collapse to almost nothing right after a
/// busy patch and leave the tape empty. Below a few seconds there is no tape
/// left to read, whatever the bars did.
pub(crate) const MIN_LANE_SPAN_MS: i64 = 4_000;

/// Number of discrete magnitude bands the heatmap collapses intensity into.
/// Fewer bands read as flatter walls; more bands recover gradient but let the
/// book's per-update jitter fragment a band. Eight keeps walls crisp while
/// still separating quiet / medium / heavy liquidity.
pub(crate) const HEAT_LEVELS: f32 = 8.0;
