//! Persisted reference for chart-relative note sizes.
use serde::{Deserialize, Serialize};

const MIN_RENDERED_TEXT_PX: f32 = 0.5;
const MAX_RENDERED_TEXT_PX: f32 = 512.0;

/// The chart's two scales where a note is shown: pixels per bar slot (time
/// axis) and pixels per unit of its band's value axis. Zero on either axis
/// means the caller could not say, and that axis leaves the size alone.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextSpacing {
    pub px_per_bar: f32,
    pub px_per_value: f64,
}

impl TextSpacing {
    #[must_use]
    pub fn new(px_per_bar: f32, px_per_value: f64) -> Self {
        Self {
            px_per_bar,
            px_per_value,
        }
    }

    /// A pane's time-axis zoom and, when the band has one, its value scale.
    #[must_use]
    pub fn at(px_per_bar: f32, scale: Option<&crate::chart::PriceScale>) -> Self {
        Self::new(
            px_per_bar,
            scale.map_or(0.0, crate::chart::PriceScale::px_per_price),
        )
    }

    /// Only the time axis is known: an operation that binds a size outside a
    /// paint, before the frame's value axis is projected.
    #[must_use]
    pub fn horizontal(px_per_bar: f32) -> Self {
        Self::new(px_per_bar, 0.0)
    }

    fn bar(self) -> Option<f32> {
        (self.px_per_bar.is_finite() && self.px_per_bar > 0.0).then_some(self.px_per_bar)
    }

    fn value(self) -> Option<f64> {
        (self.px_per_value.is_finite() && self.px_per_value > 0.0).then_some(self.px_per_value)
    }
}

/// The scales a note's chosen size belongs to. Rendering follows whichever
/// axis shrank the most since, so the note keeps to the chart area it was
/// placed over: a squeezed value axis shrinks it as a zoomed-out time axis
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TextZoom {
    px_per_bar: f32,
    size_px: f32,
    /// Absent in notes saved before the value axis took part; bound on the
    /// next display, so their current appearance is kept.
    #[serde(default)]
    px_per_value: f64,
}

impl TextZoom {
    pub(crate) fn valid(self) -> bool {
        self.px_per_bar.is_finite()
            && self.px_per_bar > 0.0
            && self.size_px.is_finite()
            && self.size_px > 0.0
    }

    fn value_bound(self) -> bool {
        self.px_per_value.is_finite() && self.px_per_value > 0.0
    }

    /// Bind `size_px` to `spacing`. A size already bound keeps its time-axis
    /// reference and only takes a missing value-axis one.
    pub(crate) fn bind(&mut self, size_px: f32, spacing: TextSpacing) -> bool {
        if self.bound_to(size_px) {
            if self.value_bound() {
                return false;
            }
            let Some(value) = spacing.value() else {
                return false;
            };
            self.px_per_value = value;
            return true;
        }
        let Some(bar) = spacing.bar() else {
            return false;
        };
        *self = Self {
            px_per_bar: bar,
            size_px,
            px_per_value: spacing.value().unwrap_or(0.0),
        };
        true
    }

    pub(crate) fn bound_to(self, size_px: f32) -> bool {
        self.valid() && self.size_px == size_px
    }

    /// Bound on both axes for `size_px`: nothing left for a display to record.
    pub(crate) fn complete(self, size_px: f32) -> bool {
        self.bound_to(size_px) && self.value_bound()
    }

    pub(crate) fn rendered_size(self, size_px: f32, spacing: TextSpacing) -> f32 {
        let ratio = if self.bound_to(size_px) {
            let bar = spacing.bar().map(|bar| bar / self.px_per_bar);
            let value = spacing
                .value()
                .filter(|_| self.value_bound())
                .map(|value| (value / self.px_per_value) as f32);
            match (bar, value) {
                (Some(bar), Some(value)) => bar.min(value),
                (Some(ratio), None) | (None, Some(ratio)) => ratio,
                (None, None) => 1.0,
            }
        } else {
            1.0
        };
        (size_px * ratio).clamp(MIN_RENDERED_TEXT_PX, MAX_RENDERED_TEXT_PX)
    }
}
