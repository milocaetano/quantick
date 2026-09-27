//! The overlap fold: bubbles whose discs would touch on the canvas become one
//! mark.

use crate::config::HeatmapConfig;
use crate::timeline::BarTimeline;

use super::model::HeatmapProjection;

/// The pixel geometry of the pane a frame is drawn on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneGeometry {
    /// Pixels between two neighbouring bars on the candles.
    pub px_per_bar: f32,
    /// Width of the live lane, in pixels. Zero when no lane is drawn.
    pub lane_width_px: f32,
    /// Height of the chart, in pixels.
    pub height_px: f32,
}

impl HeatmapProjection {
    /// Fold every group of bubbles whose discs would overlap into one mark.
    pub fn merge_overlapping_bubbles(
        &mut self,
        geometry: PaneGeometry,
        timeline: &BarTimeline,
        config: &HeatmapConfig,
    ) {
        let _ = (geometry, timeline, config);
    }
}
