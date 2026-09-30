//! Shared tape radius limits and insets for dots, clock labels and price fit.

use crate::HeatmapConfig;
use crate::config::{BubbleStyle, bubble_halo_padding};

/// Decoration may consume at most this fraction of the pane at each edge.
/// The actual disc radius takes precedence when its diameter nearly fills it.
const MAX_DECORATED_INSET_SHARE: f32 = 0.45;
/// Leave room between the extreme prices even in a very short pane.
const MAX_VERTICAL_RADIUS_SHARE: f32 = 0.4;

/// The tape's padded linear time span and the uniform radius it can display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeHorizontalGeometry {
    /// Largest disc radius, capped uniformly when the pane cannot fit it.
    pub max_radius: f32,
    /// Minimum axis padding above and below the tape's factual price range.
    pub price_inset_px: f32,
    /// Distance in pixels from the pane edge to the oldest visible timestamp.
    pub inset_px: f32,
    /// Distance in pixels from the oldest visible timestamp to NOW.
    pub span_px: f32,
}

impl TapeHorizontalGeometry {
    /// Resolve a common inset. Every volume retains its area ratio when a
    /// narrow or short pane requires a smaller largest radius; no dot is individually
    /// displaced. A pane exactly one diameter wide has a zero time span.
    #[must_use]
    pub fn resolve(width_px: f32, height_px: f32, bubbles: &BubbleStyle) -> Self {
        let extent = |value: f32| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        let (width, height) = (extent(width_px), extent(height_px));
        let max_radius = bubbles
            .max_radius
            .max(0.0)
            .min(width / 2.0)
            .min(height * MAX_VERTICAL_RADIUS_SHARE);
        let decorated = max_radius + bubble_halo_padding(max_radius) + bubbles.outline_width;
        let inset_px = decorated
            .min(width * MAX_DECORATED_INSET_SHARE)
            .max(max_radius);
        Self {
            max_radius,
            price_inset_px: decorated
                .min(height * MAX_DECORATED_INSET_SHARE)
                .max(max_radius),
            inset_px,
            span_px: (width - 2.0 * inset_px).max(0.0),
        }
    }

    /// The inset the native tape pads the price axis fit with, on a chart
    /// this wide and tall; `0` when the pane does not draw the native tape.
    ///
    /// Measured on the lane setting's own band, never on a divider already
    /// laid out: before the tape has a live edge no band is on screen, and a
    /// fit padded for no band would step the moment the band appears.
    #[must_use]
    pub fn native_price_inset_px(config: &HeatmapConfig, chart_width: f32, height: f32) -> f32 {
        if !config.native_tape() {
            return 0.0;
        }
        let width = config.live_lane.resolved_width_px(chart_width);
        Self::resolve(width, height, &config.bubbles).price_inset_px
    }

    /// Pixel coordinate relative to the pane's left edge for this time
    /// fraction: zero is the oldest visible timestamp and one is NOW.
    #[must_use]
    pub fn x(self, fraction: f64) -> f32 {
        self.inset_px + fraction as f32 * self.span_px
    }

    /// [`Self::x`] of the market instant `timestamp_ms` on a tape whose
    /// window of `window_ms` ends at `now_ms`: where a print of that instant
    /// is drawn, and so where the book of that instant belongs.
    #[must_use]
    pub fn x_at_ms(self, timestamp_ms: i64, now_ms: i64, window_ms: i64) -> f32 {
        let window = window_ms.max(1);
        self.x((timestamp_ms - (now_ms - window)) as f64 / window as f64)
    }
}
