//! The candle footprint layer's own state: whether it is on, how it is
//! configured here, and the forming bar's ladder as last snapshotted.
//!
//! Five fields on [`super::ChartPane`] answered for one layer, and they are
//! read together everywhere they are read at all — the draw asks whether the
//! layer is on, which thresholds this chart uses, how coarse the zoom has made
//! it and what the live edge held at the last snapshot, in that order and in
//! one pass. The range-profile drawings ask the last two as a cache key.

/// The footprint layer as this pane has it. See the module docs.
#[derive(Default)]
pub struct PaneFootprint {
    /// Whether the candle footprint layer is on. What a fresh launch opens
    /// with is `config/chart-layers.toml`, not this initialiser — see
    /// [`crate::chart_layers`]; the ladder still follows the zoom's LOD, so it
    /// draws nothing where the candle is too narrow to read.
    pub visible: bool,
    /// This chart's own footprint setup, once it has been configured here.
    ///
    /// `None` — the default — means "follow the window's last setup", which
    /// keeps the common case (one chart, one taste) behaving like a global
    /// setting. A split layout is two readings of the same market (a 90-day
    /// context chart beside a 50-tick flow chart) and one set of thresholds
    /// cannot serve both, so the moment a chart is configured on its own it
    /// keeps its own.
    pub config: Option<crate::footprint_config::FootprintConfig>,
    /// The footprint's sticky detail level (hysteresis on zoom-out).
    pub(super) lod: crate::footprint_render::FootprintLod,
    /// The forming bar's ladder as last snapshotted for drawing, with the
    /// frame time it was taken and the slot it belongs to. Refreshed at
    /// ~10 Hz, not per print — the eye reads patterns, and a frozen layout
    /// cannot reflow under the pointer — but *immediately* when the slot
    /// changes: at a bar close the previous bar's ladder must never linger
    /// on the new bar, not even for one throttle interval.
    pub(super) live: Option<(f64, usize, quantick_engine::BarFootprint)>,
    /// Bumped whenever [`Self::live`] is re-taken or cleared — the cache key
    /// the range-profile drawings use to notice the live edge moved, so they
    /// re-fold at the snapshot cadence, never per paint.
    pub(super) live_version: u64,
}
